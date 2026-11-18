//! Apple Music playback through the MusicKit helper (macOS 14+ only).
//!
//! `Marmota Apple Music.app` lives in `Contents/Helpers/` of the notch's
//! bundle. It plays catalog songs with the user's own subscription
//! (ApplicationMusicPlayer) and talks JSON lines over stdin/stdout; this
//! module keeps a snapshot of what it reports and forwards commands, like
//! `spotify.rs` does for MPRIS. Without the helper (other systems, older
//! macOS, not installed or not authorized) Apple Music songs keep playing
//! from their YouTube Music twin.

use std::sync::{Arc, Mutex};
use std::time::Instant;

#[derive(Clone)]
pub struct State {
    /// The helper is running.
    pub available: bool,
    /// MusicKit authorization: "authorized", "denied", "notDetermined"…
    pub auth: String,
    /// The account can play the catalog (an active subscription), when known.
    pub can_play: Option<bool>,
    /// Catalog id of the song loaded in the helper.
    pub id: String,
    pub status: String,
    pub pos: f64,
    pub dur: f64,
    /// When `pos` was reported, to move it forward between reports.
    pub at: Instant,
    /// Bumped each time a song ends by itself (`ended_id` says which).
    pub ended: u64,
    pub ended_id: String,
    /// Last failure: (song id, message).
    pub error: Option<(String, String)>,
}

impl Default for State {
    fn default() -> Self {
        Self {
            available: false,
            auth: String::new(),
            can_play: None,
            id: String::new(),
            status: String::new(),
            pos: 0.0,
            dur: 0.0,
            at: Instant::now(),
            ended: 0,
            ended_id: String::new(),
            error: None,
        }
    }
}

impl State {
    pub fn playing(&self) -> bool {
        self.status == "playing"
    }

    /// Worth trying a song with: running, not refused, subscription not ruled out.
    pub fn usable(&self) -> bool {
        self.available
            && !matches!(self.auth.as_str(), "denied" | "restricted")
            && self.can_play != Some(false)
    }

    /// Current position, moved forward since the last report while playing.
    pub fn position(&self) -> f64 {
        let p = if self.playing() { self.pos + self.at.elapsed().as_secs_f64() } else { self.pos };
        if self.dur > 0.0 {
            p.min(self.dur)
        } else {
            p
        }
    }
}

#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
pub enum Cmd {
    /// Catalog song id (without the "am_" prefix).
    Play(String),
    Pause,
    Resume,
    Stop,
    Seek(f64),
    /// Seconds between position reports while playing.
    Interval(f64),
}

pub struct AppleMusic {
    #[cfg(target_os = "macos")]
    tx: Option<std::sync::mpsc::Sender<Cmd>>,
    state: Arc<Mutex<State>>,
}

impl AppleMusic {
    pub fn start() -> Self {
        let state = Arc::new(Mutex::new(State::default()));
        #[cfg(target_os = "macos")]
        {
            let tx = imp::spawn(state.clone());
            Self { tx, state }
        }
        #[cfg(not(target_os = "macos"))]
        Self { state }
    }

    pub fn snapshot(&self) -> State {
        self.state.lock().map(|s| s.clone()).unwrap_or_default()
    }

    #[cfg_attr(not(target_os = "macos"), allow(unused_variables))]
    pub fn send(&self, cmd: Cmd) {
        #[cfg(target_os = "macos")]
        if let Some(tx) = &self.tx {
            let _ = tx.send(cmd);
        }
    }

    /// Forget a reported failure once it has been handled.
    pub fn clear_error(&self) {
        if let Ok(mut s) = self.state.lock() {
            s.error = None;
        }
    }
}

/// The helper's executable inside the installed bundle (or
/// `MARMOTA_APPLE_HELPER`, for development).
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
pub fn helper_path() -> Option<std::path::PathBuf> {
    if let Some(p) = std::env::var_os("MARMOTA_APPLE_HELPER") {
        return Some(p.into());
    }
    let exe = std::env::current_exe().ok()?.canonicalize().ok()?;
    // …/Marmota Music.app/Contents/MacOS/marmota-music
    let contents = exe.parent()?.parent()?;
    let p = contents.join("Helpers/Marmota Apple Music.app/Contents/MacOS/marmota-apple-music");
    p.is_file().then_some(p)
}

/// Ask for MusicKit access in the foreground (`setup apple`). Returns the
/// helper's one-line answer.
#[cfg(target_os = "macos")]
pub fn authorize() -> Option<String> {
    let out = std::process::Command::new(helper_path()?).arg("--authorize").output().ok()?;
    Some(String::from_utf8_lossy(&out.stdout).trim().to_string())
}

/// Diagnostics: what Apple answers for a song's lyrics through MusicKit.
#[cfg(target_os = "macos")]
pub fn probe_lyrics(id: &str) -> Option<String> {
    use std::io::{BufRead, BufReader, Write};
    use std::process::{Command, Stdio};
    let mut child = Command::new(helper_path()?)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .ok()?;
    let mut stdin = child.stdin.take()?;
    let cmd = serde_json::json!({ "cmd": "lyrics", "id": id });
    writeln!(stdin, "{cmd}").ok()?;
    let mut answer = None;
    for line in BufReader::new(child.stdout.take()?).lines().map_while(Result::ok) {
        if line.contains("\"lyrics\"") {
            answer = Some(line);
            break;
        }
    }
    drop(stdin);
    let _ = child.wait();
    answer
}

#[cfg(target_os = "macos")]
mod imp {
    use super::{helper_path, Cmd, State};
    use serde_json::{json, Value};
    use std::io::{BufRead, BufReader, Write};
    use std::process::{Command, Stdio};
    use std::sync::mpsc::{channel, Sender};
    use std::sync::{Arc, Mutex};
    use std::thread;
    use std::time::Instant;

    /// ApplicationMusicPlayer needs macOS 14.
    fn supported() -> bool {
        let out = Command::new("sw_vers").arg("-productVersion").output();
        let v = out.map(|o| String::from_utf8_lossy(&o.stdout).to_string()).unwrap_or_default();
        v.trim().split('.').next().and_then(|m| m.parse::<u32>().ok()).is_some_and(|m| m >= 14)
    }

    pub fn spawn(state: Arc<Mutex<State>>) -> Option<Sender<Cmd>> {
        let path = helper_path()?;
        if !supported() {
            return None;
        }
        let mut child = Command::new(&path)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .ok()?;
        let mut stdin = child.stdin.take()?;
        let stdout = child.stdout.take()?;
        if let Ok(mut s) = state.lock() {
            s.available = true;
        }

        let shared = state.clone();
        thread::Builder::new()
            .name("apple-music-read".into())
            .spawn(move || {
                for line in BufReader::new(stdout).lines().map_while(Result::ok) {
                    if let Ok(v) = serde_json::from_str::<Value>(&line) {
                        if let Ok(mut s) = shared.lock() {
                            apply(&mut s, &v);
                        }
                    }
                }
                // The helper quit or crashed: songs go back to YouTube Music.
                if let Ok(mut s) = shared.lock() {
                    s.available = false;
                    s.status.clear();
                }
                let _ = child.wait();
            })
            .ok()?;

        let (tx, rx) = channel::<Cmd>();
        thread::Builder::new()
            .name("apple-music".into())
            .spawn(move || {
                // Ask once without any prompt; the prompt comes with the first song.
                let _ = writeln!(stdin, "{}", json!({ "cmd": "status" }));
                while let Ok(cmd) = rx.recv() {
                    let msg = match cmd {
                        Cmd::Play(id) => json!({ "cmd": "play", "id": id }),
                        Cmd::Pause => json!({ "cmd": "pause" }),
                        Cmd::Resume => json!({ "cmd": "resume" }),
                        Cmd::Stop => json!({ "cmd": "stop" }),
                        Cmd::Seek(pos) => json!({ "cmd": "seek", "pos": pos }),
                        Cmd::Interval(sec) => json!({ "cmd": "interval", "sec": sec }),
                    };
                    if writeln!(stdin, "{msg}").and_then(|_| stdin.flush()).is_err() {
                        break;
                    }
                }
            })
            .ok()?;
        Some(tx)
    }

    fn apply(s: &mut State, v: &Value) {
        let text = |k: &str| v.get(k).and_then(|x| x.as_str()).unwrap_or("").to_string();
        let num = |k: &str| v.get(k).and_then(|x| x.as_f64()).unwrap_or(0.0);
        match v.get("ev").and_then(|e| e.as_str()) {
            Some("status") => {
                s.auth = text("auth");
                s.can_play = v.get("canPlay").and_then(|b| b.as_bool());
            }
            Some("state") => {
                let id = text("id");
                if id != s.id {
                    s.dur = 0.0;
                    s.id = id;
                }
                s.status = text("status");
                s.pos = num("pos");
                let dur = num("dur");
                if dur > 0.0 {
                    s.dur = dur;
                }
                s.at = Instant::now();
                // Playing at all means it is authorized and subscribed.
                if s.status == "playing" {
                    s.auth = "authorized".into();
                    s.can_play = Some(true);
                }
            }
            Some("ended") => {
                s.ended += 1;
                s.ended_id = text("id");
                s.status = "stopped".into();
            }
            Some("error") => {
                let msg = text("msg");
                if msg.contains("denied") || msg.contains("restricted") {
                    s.auth = "denied".into();
                }
                if msg.contains("suscripción") {
                    s.can_play = Some(false);
                }
                s.error = Some((text("id"), msg));
            }
            _ => {}
        }
    }
}
