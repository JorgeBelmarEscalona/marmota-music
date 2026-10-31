//! "Iniciar sesión con Google": opens a Chromium-based browser on a profile
//! of its own and waits for the user to sign in and close it. Google refuses
//! sign-ins in a browser with remote debugging on, so that window is a plain
//! one; afterwards the same profile is reopened headless with DevTools to
//! read the session, which comes back decrypted (Chrome's on-disk cookie
//! encryption keeps yt-dlp out on Windows), and saved as a cookies.txt.

use std::io::Write;
use std::net::TcpStream;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::mpsc::Sender;
use std::thread;
use std::time::{Duration, Instant};

use serde_json::{json, Value};
use tungstenite::stream::MaybeTlsStream;
use tungstenite::{Message, WebSocket};

use crate::model::Event;
use crate::paths;

const LOGIN_URL: &str =
    "https://accounts.google.com/ServiceLogin?service=youtube&continue=https%3A%2F%2Fmusic.youtube.com%2F";
/// Time the user has to sign in and close the window.
const TIMEOUT: Duration = Duration::from_secs(20 * 60);

type Socket = WebSocket<MaybeTlsStream<TcpStream>>;

/// Profile the sign-in window uses (kept so the session can be renewed).
pub fn profile_dir() -> PathBuf {
    paths::data_dir().join("google-login")
}

/// A Chromium-based browser that speaks the DevTools protocol: (name, exe).
pub fn browser() -> Option<(&'static str, PathBuf)> {
    #[cfg(windows)]
    {
        let env = |k: &str| std::env::var_os(k).map(PathBuf::from).unwrap_or_default();
        let (pf, pf86, local) = (env("ProgramFiles"), env("ProgramFiles(x86)"), env("LOCALAPPDATA"));
        let chrome = r"Google\Chrome\Application\chrome.exe";
        let edge = r"Microsoft\Edge\Application\msedge.exe";
        let brave = r"BraveSoftware\Brave-Browser\Application\brave.exe";
        let candidates = [
            ("Google Chrome", pf.join(chrome)),
            ("Google Chrome", pf86.join(chrome)),
            ("Google Chrome", local.join(chrome)),
            ("Microsoft Edge", pf86.join(edge)),
            ("Microsoft Edge", pf.join(edge)),
            ("Brave", pf.join(brave)),
            ("Brave", local.join(brave)),
        ];
        candidates.into_iter().find(|(_, p)| p.is_file())
    }
    #[cfg(unix)]
    {
        let names = [
            ("Google Chrome", "google-chrome-stable"),
            ("Google Chrome", "google-chrome"),
            ("Chromium", "chromium"),
            ("Chromium", "chromium-browser"),
            ("Brave", "brave"),
            ("Brave", "brave-browser"),
            ("Microsoft Edge", "microsoft-edge-stable"),
            ("Microsoft Edge", "microsoft-edge"),
            ("Vivaldi", "vivaldi-stable"),
        ];
        let path = std::env::var_os("PATH")?;
        names.into_iter().find_map(|(name, exe)| {
            std::env::split_paths(&path).map(|d| d.join(exe)).find(|p| p.is_file()).map(|p| (name, p))
        })
    }
}

pub fn start(tx: Sender<Event>) {
    thread::Builder::new()
        .name("login".into())
        .spawn(move || {
            let _ = tx.send(Event::LoginDone(run()));
        })
        .ok();
}

fn launch(exe: &Path, profile: &Path, args: &[&str]) -> std::io::Result<std::process::Child> {
    let mut cmd = Command::new(exe);
    cmd.arg(format!("--user-data-dir={}", profile.display()))
        .args(["--no-first-run", "--no-default-browser-check"]);
    // Both launches must agree on how cookies are encrypted; the keyring a
    // desktop session picks may not be reachable headless.
    #[cfg(unix)]
    cmd.arg("--password-store=basic");
    cmd.args(args).stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null()).spawn()
}

fn run() -> Result<PathBuf, String> {
    let (name, exe) = browser().ok_or("No encontré Chrome, Edge, Chromium ni Brave")?;
    let profile = profile_dir();
    std::fs::create_dir_all(&profile).map_err(|e| e.to_string())?;

    // 1. A plain window: the user signs in and closes it.
    let mut child = launch(&exe, &profile, &["--new-window", LOGIN_URL]).map_err(|e| format!("No pude abrir {name}: {e}"))?;
    let started = Instant::now();
    loop {
        thread::sleep(Duration::from_secs(1));
        if child.try_wait().ok().flatten().is_some() {
            break;
        }
        if started.elapsed() > TIMEOUT {
            let _ = child.kill();
            return Err("Se acabó el tiempo para iniciar sesión".into());
        }
    }

    // 2. The same profile headless, only to read what it stored. Chrome's
    // helper processes let go of the profile a moment after the window.
    let mut cookies = None;
    for _ in 0..4 {
        thread::sleep(Duration::from_secs(2));
        cookies = read_cookies(&exe, &profile);
        if cookies.is_some() {
            break;
        }
    }
    let cookies = cookies.ok_or(format!("No pude leer la sesión guardada en {name}"))?;
    if !signed_in(&cookies) {
        return Err("No se completó el inicio de sesión: entra con tu cuenta y cierra la ventana cuando veas YouTube Music".into());
    }
    let file = paths::data_dir().join("youtube-cookies.txt");
    write_cookies(&file, &cookies).map_err(|e| format!("No pude guardar la sesión: {e}"))?;
    Ok(file)
}

fn read_cookies(exe: &Path, profile: &Path) -> Option<Vec<Value>> {
    // The browser writes its debugging port here once it is up.
    let port_file = profile.join("DevToolsActivePort");
    let _ = std::fs::remove_file(&port_file);
    // Chrome may relaunch itself and let this process exit, so the port
    // file, not the process, says when it is up.
    let mut child = launch(exe, profile, &["--headless=new", "--remote-debugging-port=0", "about:blank"]).ok()?;
    let mut socket = None;
    for _ in 0..60 {
        thread::sleep(Duration::from_millis(250));
        socket = connect(&port_file);
        if socket.is_some() {
            break;
        }
    }
    let Some(mut ws) = socket else {
        let _ = child.kill();
        return None;
    };
    let mut id = 0;
    let cookies = call(&mut ws, &mut id, "Storage.getCookies")
        .ok()
        .and_then(|v| v["cookies"].as_array().cloned());
    // Closes whichever process ended up being the browser.
    let _ = call(&mut ws, &mut id, "Browser.close");
    let _ = child.wait();
    cookies
}

fn connect(port_file: &Path) -> Option<Socket> {
    let port: u16 = std::fs::read_to_string(port_file).ok()?.lines().next()?.trim().parse().ok()?;
    let info: Value = reqwest::blocking::Client::new()
        .get(format!("http://127.0.0.1:{port}/json/version"))
        .timeout(Duration::from_secs(3))
        .send()
        .ok()?
        .json()
        .ok()?;
    let url = info["webSocketDebuggerUrl"].as_str()?;
    let (ws, _) = tungstenite::connect(url).ok()?;
    Some(ws)
}

/// Send one DevTools command and wait for its reply.
fn call(ws: &mut Socket, id: &mut u64, method: &str) -> Result<Value, String> {
    *id += 1;
    let body = json!({ "id": *id, "method": method }).to_string();
    ws.send(Message::Text(body.into())).map_err(|e| e.to_string())?;
    loop {
        match ws.read().map_err(|e| e.to_string())? {
            Message::Text(text) => {
                let v: Value = serde_json::from_str(text.as_ref()).map_err(|e| e.to_string())?;
                if v["id"].as_u64() == Some(*id) {
                    return Ok(v["result"].clone());
                }
            }
            Message::Close(_) => return Err("cerrado".into()),
            _ => {}
        }
    }
}

/// Google has handed YouTube its session cookies.
fn signed_in(cookies: &[Value]) -> bool {
    cookies.iter().any(|c| {
        c["domain"].as_str().is_some_and(|d| d.ends_with("youtube.com"))
            && matches!(c["name"].as_str(), Some("SAPISID" | "__Secure-3PAPISID"))
    })
}

/// Netscape cookies.txt with the YouTube and Google cookies.
fn write_cookies(file: &Path, cookies: &[Value]) -> std::io::Result<()> {
    // Session cookies get a year so yt-dlp keeps them.
    let later = (std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap_or_default()
        + Duration::from_secs(365 * 24 * 3600))
    .as_secs();
    let mut out = String::from("# Netscape HTTP Cookie File\n");
    for c in cookies {
        let (Some(domain), Some(name), Some(value)) = (c["domain"].as_str(), c["name"].as_str(), c["value"].as_str())
        else {
            continue;
        };
        if !(domain.ends_with("youtube.com") || domain.ends_with("google.com")) {
            continue;
        }
        let expires = c["expires"].as_f64().filter(|e| *e > 0.0).map(|e| e as u64).unwrap_or(later);
        let flag = |b: bool| if b { "TRUE" } else { "FALSE" };
        out.push_str(&format!(
            "{domain}\t{}\t{}\t{}\t{expires}\t{name}\t{value}\n",
            flag(domain.starts_with('.')),
            c["path"].as_str().unwrap_or("/"),
            flag(c["secure"].as_bool().unwrap_or(false)),
        ));
    }
    let mut f = std::fs::File::create(file)?;
    // It holds a live Google session: readable by this user only.
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = f.set_permissions(std::fs::Permissions::from_mode(0o600));
    }
    f.write_all(out.as_bytes())
}
