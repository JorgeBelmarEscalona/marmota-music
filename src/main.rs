mod db;
mod draw;
mod login;
mod lyrics;
mod model;
mod mpv;
mod paths;
mod rank;
mod setup;
mod spotify;
mod spotify_api;
mod theme;
mod ui;
#[cfg(windows)]
mod win;
mod yt;
mod ytm;

use std::io::{BufRead, BufReader, Write};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use clap::{Parser, Subcommand};
use model::{Cmd, Snapshot};

#[derive(Parser)]
#[command(name = "marmota-music", version, about = "Liquid-glass music notch")]
struct Cli {
    #[command(subcommand)]
    cmd: Option<Action>,
}

#[derive(Subcommand)]
enum Action {
    /// Run the notch daemon (hidden until toggled).
    Daemon,
    /// Show the notch, expanded, on the current monitor.
    Toggle,
    Show,
    /// Collapse to the mini pill (lyrics + transport).
    Collapse,
    Hide,
    PlayPause,
    Next,
    Prev,
    /// Print the player state as JSON.
    Status,
    Quit,
    /// Toggle the mpv video window.
    Video,
    /// Inject a pointer press at a window-local coordinate (testing/automation).
    Click { x: f64, y: f64 },
    /// Jump to a track id (YouTube video id) and play it.
    Play {
        id: String,
        #[arg(long)]
        title: Option<String>,
        #[arg(long)]
        artist: Option<String>,
    },
    /// Connect YouTube Music and Spotify step by step (`setup spotify` or
    /// `setup youtube` for just one).
    Setup { only: Option<String> },
    /// Reload the library (after importing playlists from outside).
    #[command(hide = true)]
    Refresh,
    /// Look up lyrics for a song and report what was found (diagnostics).
    #[command(hide = true)]
    Lyrics { title: String, artist: String, #[arg(default_value_t = 0.0)] duration: f64 },
    /// Print ranked search results (diagnostics).
    #[command(hide = true)]
    SearchTest { query: String },
    /// Show the YouTube account the saved session belongs to (diagnostics).
    #[command(hide = true)]
    AccountTest,
    /// Open the search tab, optionally running a query.
    Search { query: Option<String> },
}

fn request(cmd: &Cmd) -> Option<String> {
    let mut stream = local::connect().ok()?;
    stream.set_read_timeout(Some(Duration::from_millis(600))).ok()?;
    let body = serde_json::to_string(cmd).ok()?;
    stream.write_all(body.as_bytes()).ok()?;
    stream.write_all(b"\n").ok()?;
    let mut reader = BufReader::new(stream);
    let mut line = String::new();
    reader.read_line(&mut line).ok()?;
    let line = line.trim().to_string();
    if line.is_empty() {
        None
    } else {
        Some(line)
    }
}

fn spawn_daemon() {
    if let Ok(exe) = std::env::current_exe() {
        let mut cmd = std::process::Command::new(exe);
        #[cfg(windows)]
        {
            win::no_console(&mut cmd);
            win::keep_stdio_private();
        }
        let _ = cmd
            .arg("daemon")
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn();
    }
}

fn send(cmd: Cmd) {
    if let Some(reply) = request(&cmd) {
        println!("{reply}");
        return;
    }
    // Nothing listening: start the daemon and retry once it is up.
    spawn_daemon();
    for _ in 0..40 {
        std::thread::sleep(Duration::from_millis(100));
        if let Some(reply) = request(&cmd) {
            println!("{reply}");
            return;
        }
    }
    eprintln!("marmota-music: no se pudo iniciar el daemon");
    std::process::exit(1);
}

fn main() {
    // Everything is painted with Cairo on the CPU, so GTK's Cairo renderer
    // skips the GPU upload: fewer dropped frames while animating and ~75 MB
    // less memory than the Vulkan default. MM_RENDERER overrides it.
    if std::env::var_os("GSK_RENDERER").is_none() {
        let r = std::env::var("MM_RENDERER").unwrap_or_else(|_| "cairo".into());
        std::env::set_var("GSK_RENDERER", r);
    }
    paths::ensure_dirs();
    let cli = Cli::parse();
    let action = cli.cmd.unwrap_or(Action::Daemon);

    match action {
        Action::Daemon => {
            if request(&Cmd::Status).is_some() {
                eprintln!("marmota-music: ya hay una instancia en ejecución");
                return;
            }
            // Started from a shortcut or a terminal: let go of the console
            // so no black window stays open next to the notch.
            #[cfg(windows)]
            win::detach_console();
            let shared = Arc::new(Mutex::new(Snapshot::default()));
            let (tx, rx) = std::sync::mpsc::channel();
            ipc::start(tx.clone(), shared.clone());
            #[cfg(windows)]
            win::hotkey(tx.clone());
            if let Err(e) = ui::run(rx, tx, shared) {
                eprintln!("marmota-music: {e}");
                std::process::exit(1);
            }
        }
        Action::Status => match request(&Cmd::Status) {
            Some(reply) => println!("{reply}"),
            None => {
                println!("{{\"playing\":false}}");
            }
        },
        Action::Toggle => send(Cmd::Toggle),
        Action::Show => send(Cmd::Show),
        Action::Collapse => send(Cmd::Collapse),
        Action::Hide => send(Cmd::Hide),
        Action::PlayPause => send(Cmd::PlayPause),
        Action::Next => send(Cmd::Next),
        Action::Prev => send(Cmd::Prev),
        Action::Quit => send(Cmd::Quit),
        Action::Video => send(Cmd::ToggleVideo),
        Action::Click { x, y } => send(Cmd::Click { x, y }),
        Action::Play { id, title, artist } => send(Cmd::Play { id, title, artist }),
        Action::Search { query } => send(Cmd::Search { query }),
        Action::Setup { only } => setup::run(only.as_deref()),
        // Only nudge a running notch; never start one just for this.
        Action::Refresh => {
            let _ = request(&Cmd::Refresh);
        }
        Action::AccountTest => match paths::cookies_file() {
            Some(file) => match ytm::account(&file) {
                Ok((name, email, photo)) => println!("{name} · {email} · foto: {}", !photo.is_empty()),
                Err(e) => println!("error: {e}"),
            },
            None => println!("sin sesión guardada"),
        },
        Action::SearchTest { query } => {
            let yt = ytm::search(&query).map(|r| r.0).unwrap_or_default();
            let sp = spotify_api::search(&query).unwrap_or_default();
            for t in rank::merge(&query, yt, sp).iter().take(12) {
                println!("{:?} {:8} vid={} q={} | {} — {}", rank::debug_key(&query, t), t.source, t.video, t.quality, t.title, t.artist);
            }
        }
        Action::Lyrics { title, artist, duration } => {
            let t = model::Track { title, artist, duration, ..Default::default() };
            match lyrics::fetch(&t) {
                Ok(l) => println!("{} · {} líneas", if l.synced { "SINCRONIZADA" } else { "solo texto" }, l.lines.len()),
                Err(e) => println!("sin letra: {e}"),
            }
        }
    }
}

/// Transport of the control socket: a Unix socket, or on Windows a loopback
/// TCP port published in a file (std has no Unix sockets there).
mod local {
    use std::io;

    #[cfg(windows)]
    pub use std::net::{TcpListener as Listener, TcpStream as Stream};
    #[cfg(unix)]
    pub use std::os::unix::net::{UnixListener as Listener, UnixStream as Stream};

    #[cfg(unix)]
    pub fn connect() -> io::Result<Stream> {
        Stream::connect(crate::paths::socket())
    }

    #[cfg(windows)]
    pub fn connect() -> io::Result<Stream> {
        let port: u16 = std::fs::read_to_string(crate::paths::socket())?
            .trim()
            .parse()
            .map_err(|_| io::Error::other("puerto inválido"))?;
        let addr = std::net::SocketAddr::from(([127, 0, 0, 1], port));
        Stream::connect_timeout(&addr, std::time::Duration::from_millis(300))
    }

    pub fn bind() -> io::Result<Listener> {
        let path = crate::paths::socket();
        let _ = std::fs::remove_file(&path);
        #[cfg(unix)]
        return Listener::bind(&path);
        #[cfg(windows)]
        {
            let listener = Listener::bind(("127.0.0.1", 0))?;
            std::fs::write(&path, listener.local_addr()?.port().to_string())?;
            Ok(listener)
        }
    }
}

mod ipc {
    use std::io::{BufRead, BufReader, Write};
    use std::sync::mpsc::Sender;
    use std::sync::{Arc, Mutex};
    use std::thread;
    use std::time::Duration;

    use crate::local::{self, Stream};
    use crate::model::{Cmd, Event, Snapshot};

    pub fn start(tx: Sender<Event>, shared: Arc<Mutex<Snapshot>>) {
        let listener = match local::bind() {
            Ok(l) => l,
            Err(e) => {
                eprintln!("marmota-music: socket: {e}");
                return;
            }
        };
        thread::Builder::new()
            .name("ipc".into())
            .spawn(move || {
                for stream in listener.incoming() {
                    let Ok(stream) = stream else { continue };
                    let tx = tx.clone();
                    let shared = shared.clone();
                    thread::spawn(move || handle(stream, tx, shared));
                }
            })
            .ok();
    }

    fn handle(mut stream: Stream, tx: Sender<Event>, shared: Arc<Mutex<Snapshot>>) {
        let _ = stream.set_read_timeout(Some(Duration::from_millis(400)));
        let mut line = String::new();
        let mut reader = match stream.try_clone() {
            Ok(s) => BufReader::new(s),
            Err(_) => return,
        };
        if reader.read_line(&mut line).is_err() {
            return;
        }
        let Ok(cmd) = serde_json::from_str::<Cmd>(line.trim()) else {
            let _ = stream.write_all(b"{\"error\":\"bad command\"}\n");
            return;
        };
        let status = matches!(cmd, Cmd::Status);
        let quitting = matches!(cmd, Cmd::Quit);
        if tx.send(Event::Cmd(cmd)).is_err() {
            return;
        }
        if quitting {
            let _ = stream.write_all(b"{\"ok\":true}\n");
            return;
        }
        if status {
            // Give the UI one frame to fold the command in, then report.
            thread::sleep(Duration::from_millis(30));
            let snap = shared.lock().map(|s| s.clone()).unwrap_or_default();
            let body = serde_json::to_string(&snap).unwrap_or_else(|_| "{}".into());
            let _ = stream.write_all(format!("{body}\n").as_bytes());
        } else {
            let _ = stream.write_all(b"{\"ok\":true}\n");
        }
    }
}
