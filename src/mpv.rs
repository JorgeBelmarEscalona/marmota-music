use std::io::{Read, Write};
use std::os::unix::net::UnixStream;
use std::process::{Child, Command, Stdio};
use std::sync::mpsc::{Receiver, Sender};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use serde_json::{json, Value};

use crate::model::{Event, Playback};

pub enum MpvCmd {
    Load(String),
    Pause(bool),
    Seek(f64),
    Volume(f64),
    Video(bool),
    #[allow(dead_code)]
    Stop,
}

pub struct Mpv {
    tx: Sender<MpvCmd>,
    child: Arc<Mutex<Option<Child>>>,
}

impl Mpv {
    pub fn new(shared: Arc<Mutex<Playback>>, events: Sender<Event>) -> Self {
        let sock = crate::paths::mpv_socket();
        // A previous daemon may have died without reaping its mpv.
        if let Ok(mut stale) = UnixStream::connect(&sock) {
            let _ = stale.write_all(b"{\"command\":[\"quit\"]}\n");
            thread::sleep(Duration::from_millis(200));
        }
        let _ = std::fs::remove_file(&sock);

        let mut cmd = Command::new("mpv");
        // Make mpv die with us, even when the daemon is killed with SIGTERM.
        unsafe {
            use std::os::unix::process::CommandExt;
            cmd.pre_exec(|| {
                libc::prctl(libc::PR_SET_PDEATHSIG, libc::SIGTERM as libc::c_ulong, 0, 0, 0);
                Ok(())
            });
        }
        let child = cmd
            .args([
                "--idle=yes",
                "--vid=no",
                "--force-window=no",
                "--no-terminal",
                "--no-input-terminal",
                "--cache=yes",
                "--cache-pause=no",
                "--cache-pause-initial=no",
                "--demuxer-max-bytes=67108864",
                "--ytdl-format=bestaudio/best",
                "--audio-client-name=Marmota Music",
                "--wayland-app-id=marmota-video",
                "--title=Marmota Music",
                "--gapless-audio=yes",
                &format!("--input-ipc-server={}", sock.display()),
            ])
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .expect("no se pudo lanzar mpv");

        let child = Arc::new(Mutex::new(Some(child)));
        let (tx, rx) = std::sync::mpsc::channel();

        let sock2 = sock.clone();
        thread::Builder::new()
            .name("mpv".into())
            .spawn(move || run(sock2, rx, shared, events))
            .ok();

        Self { tx, child }
    }

    pub fn load(&self, url: &str) {
        let _ = self.tx.send(MpvCmd::Load(url.to_string()));
    }
    pub fn pause(&self, paused: bool) {
        let _ = self.tx.send(MpvCmd::Pause(paused));
    }
    pub fn seek(&self, pos: f64) {
        let _ = self.tx.send(MpvCmd::Seek(pos));
    }
    pub fn volume(&self, value: f64) {
        let _ = self.tx.send(MpvCmd::Volume(value));
    }
    pub fn video(&self, on: bool) {
        let _ = self.tx.send(MpvCmd::Video(on));
    }
    #[allow(dead_code)]
    pub fn stop(&self) {
        let _ = self.tx.send(MpvCmd::Stop);
    }
}

impl Drop for Mpv {
    fn drop(&mut self) {
        if let Ok(mut guard) = self.child.lock() {
            if let Some(child) = guard.as_mut() {
                let _ = child.kill();
            }
        }
    }
}

fn send(stream: &mut UnixStream, id: &mut u64, cmd: Value) {
    *id += 1;
    let body = json!({ "command": cmd, "request_id": *id });
    let _ = stream.write_all(body.to_string().as_bytes());
    let _ = stream.write_all(b"\n");
}

fn run(sock: std::path::PathBuf, rx: Receiver<MpvCmd>, shared: Arc<Mutex<Playback>>, events: Sender<Event>) {
    // mpv needs a moment to create its IPC socket.
    let mut stream = None;
    for _ in 0..80 {
        if let Ok(s) = UnixStream::connect(&sock) {
            stream = Some(s);
            break;
        }
        thread::sleep(Duration::from_millis(100));
    }
    let Some(mut stream) = stream else { return };
    let _ = stream.set_read_timeout(Some(Duration::from_millis(40)));

    let mut rid = 0u64;
    for (i, prop) in [
        "time-pos",
        "duration",
        "pause",
        "volume",
        "idle-active",
        "eof-reached",
        "video-format",
    ]
    .iter()
    .enumerate()
    {
        send(&mut stream, &mut rid, json!(["observe_property", i + 1, prop]));
    }

    let mut buf: Vec<u8> = Vec::new();
    let mut tmp = [0u8; 8192];
    let mut ended_sent = false;
    let started = Instant::now();
    let mut current_url = String::new();
    let mut resume_on_load: Option<f64> = None;

    loop {
        while let Ok(cmd) = rx.try_recv() {
            match cmd {
                MpvCmd::Load(url) => {
                    ended_sent = false;
                    current_url = url.clone();
                    resume_on_load = None;
                    if let Ok(mut s) = shared.lock() {
                        s.pos = 0.0;
                        s.idle = false;
                        s.playing = true;
                    }
                    send(&mut stream, &mut rid, json!(["loadfile", url, "replace"]));
                }
                MpvCmd::Pause(paused) => {
                    send(&mut stream, &mut rid, json!(["set_property", "pause", paused]));
                }
                MpvCmd::Seek(pos) => {
                    send(&mut stream, &mut rid, json!(["seek", pos, "absolute"]));
                }
                MpvCmd::Volume(value) => {
                    send(&mut stream, &mut rid, json!(["set_property", "volume", value]));
                    if let Ok(mut s) = shared.lock() {
                        s.volume = value;
                    }
                }
                MpvCmd::Video(on) => {
                    if on && !current_url.is_empty() {
                        // mpv resolved the track as audio-only; reload it with a
                        // video-capable format and pick up where we left off.
                        let pos = shared.lock().map(|s| s.pos).unwrap_or(0.0);
                        send(&mut stream, &mut rid, json!(["set_property", "ytdl-format", "bestvideo[height<=1080]+bestaudio/best"]));
                        send(&mut stream, &mut rid, json!(["set_property", "vid", "auto"]));
                        send(&mut stream, &mut rid, json!(["set_property", "force-window", "yes"]));
                        resume_on_load = Some(pos);
                        ended_sent = false;
                        send(&mut stream, &mut rid, json!(["loadfile", current_url, "replace"]));
                    } else if !on {
                        // Keep the current stream playing; just drop the picture.
                        send(&mut stream, &mut rid, json!(["set_property", "vid", "no"]));
                        send(&mut stream, &mut rid, json!(["set_property", "force-window", "no"]));
                        send(&mut stream, &mut rid, json!(["set_property", "ytdl-format", "bestaudio/best"]));
                        if let Ok(mut s) = shared.lock() {
                            s.has_video = false;
                        }
                    }
                }
                MpvCmd::Stop => {
                    send(&mut stream, &mut rid, json!(["stop"]));
                    if let Ok(mut s) = shared.lock() {
                        s.playing = false;
                        s.idle = true;
                        s.pos = 0.0;
                    }
                }
            }
        }

        match stream.read(&mut tmp) {
            Ok(0) => break,
            Ok(n) => {
                buf.extend_from_slice(&tmp[..n]);
                while let Some(pos) = buf.iter().position(|b| *b == b'\n') {
                    let line = String::from_utf8_lossy(&buf[..pos]).to_string();
                    buf.drain(..=pos);
                    if let Ok(v) = serde_json::from_str::<Value>(line.trim()) {
                        if v.get("event").and_then(|e| e.as_str()) == Some("property-change") {
                            let name = v.get("name").and_then(|n| n.as_str()).unwrap_or("");
                            let data = &v["data"];
                            if let Ok(mut s) = shared.lock() {
                                match name {
                                    "time-pos" => {
                                        if let Some(t) = data.as_f64() {
                                            s.pos = t;
                                        }
                                    }
                                    "duration" => {
                                        if let Some(d) = data.as_f64() {
                                            s.dur = d;
                                        }
                                    }
                                    "pause" => {
                                        let paused = data.as_bool().unwrap_or(false);
                                        s.playing = !paused && !s.idle;
                                    }
                                    "volume" => {
                                        if let Some(vol) = data.as_f64() {
                                            s.volume = vol;
                                        }
                                    }
                                    "idle-active" => {
                                        s.idle = data.as_bool().unwrap_or(false);
                                        s.playing = false;
                                    }
                                    "video-format" => {
                                        s.has_video = !data.is_null();
                                    }
                                    "eof-reached" => {
                                        if data.as_bool() == Some(true) && !ended_sent {
                                            ended_sent = true;
                                            let _ = events.send(Event::Ended);
                                        }
                                    }
                                    _ => {}
                                }
                            }
                        }
                        let event = v.get("event").and_then(|e| e.as_str());
                        if event == Some("end-file") {
                            let reason = v.get("reason").and_then(|r| r.as_str()).unwrap_or("");
                            if reason == "eof" && !ended_sent {
                                ended_sent = true;
                                let _ = events.send(Event::Ended);
                            }
                        }
                        if event == Some("file-loaded") {
                            ended_sent = false;
                            if let Ok(mut s) = shared.lock() {
                                s.idle = false;
                                s.playing = true;
                            }
                            if let Some(pos) = resume_on_load.take() {
                                if pos > 1.0 {
                                    send(&mut stream, &mut rid, json!(["seek", pos, "absolute"]));
                                }
                            }
                        }
                    }
                }
            }
            Err(e)
                if e.kind() == std::io::ErrorKind::WouldBlock
                    || e.kind() == std::io::ErrorKind::TimedOut =>
            {
                if started.elapsed() > Duration::from_secs(1) {
                    // nothing to do; keep the loop polite
                }
            }
            Err(_) => break,
        }
    }
}
