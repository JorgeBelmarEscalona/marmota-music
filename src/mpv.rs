use std::ffi::{c_char, c_int, c_void, CString};
use std::io::{Read, Write};
use std::os::unix::net::UnixStream;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::{Receiver, Sender};
use std::sync::{Arc, Condvar, Mutex};
use std::thread;
use std::time::Duration;

use serde_json::{json, Value};

use crate::model::{Event, Playback};

/// Size of the software-rendered video frame. The notch paints it scaled.
pub const VIDEO_W: i32 = 768;
pub const VIDEO_H: i32 = 432;

mod ffi {
    use super::*;

    #[repr(C)]
    pub struct RenderParam {
        pub kind: c_int,
        pub data: *mut c_void,
    }

    pub const PARAM_INVALID: c_int = 0;
    pub const PARAM_API_TYPE: c_int = 1;
    pub const PARAM_SW_SIZE: c_int = 17;
    pub const PARAM_SW_FORMAT: c_int = 18;
    pub const PARAM_SW_STRIDE: c_int = 19;
    pub const PARAM_SW_POINTER: c_int = 20;
    pub const UPDATE_FRAME: u64 = 1;

    #[link(name = "mpv")]
    extern "C" {
        pub fn mpv_create() -> *mut c_void;
        pub fn mpv_set_option_string(h: *mut c_void, name: *const c_char, data: *const c_char) -> c_int;
        pub fn mpv_initialize(h: *mut c_void) -> c_int;
        pub fn mpv_render_context_create(
            res: *mut *mut c_void,
            h: *mut c_void,
            params: *mut RenderParam,
        ) -> c_int;
        pub fn mpv_render_context_set_update_callback(
            ctx: *mut c_void,
            cb: Option<extern "C" fn(*mut c_void)>,
            data: *mut c_void,
        );
        pub fn mpv_render_context_update(ctx: *mut c_void) -> u64;
        pub fn mpv_render_context_render(ctx: *mut c_void, params: *mut RenderParam) -> c_int;
    }
}

/// Latest decoded video frame, `bgr0` == Cairo RGB24 on little endian.
pub struct Frame {
    pub data: Vec<u8>,
    pub serial: AtomicU64,
}

pub type SharedFrame = Arc<Mutex<Frame>>;

struct Wake {
    pending: Mutex<bool>,
    cv: Condvar,
}

extern "C" fn on_update(data: *mut c_void) {
    // Called from mpv's threads; only signal, never render here.
    let wake = unsafe { &*(data as *const Wake) };
    if let Ok(mut p) = wake.pending.lock() {
        *p = true;
        wake.cv.notify_one();
    }
}

struct RenderCtx(*mut c_void);
unsafe impl Send for RenderCtx {}

fn render_loop(ctx: RenderCtx, wake: Arc<Wake>, frame: SharedFrame, counter: Arc<AtomicU64>) {
    let stride = (VIDEO_W * 4) as usize;
    let mut buf = vec![0u8; stride * VIDEO_H as usize];
    let mut size = [VIDEO_W as c_int, VIDEO_H as c_int];
    let mut stride_val: usize = stride;
    let format = CString::new("bgr0").unwrap();
    loop {
        {
            let mut p = wake.pending.lock().unwrap();
            while !*p {
                p = wake.cv.wait(p).unwrap();
            }
            *p = false;
        }
        let flags = unsafe { ffi::mpv_render_context_update(ctx.0) };
        if flags & ffi::UPDATE_FRAME == 0 {
            continue;
        }
        let mut params = [
            ffi::RenderParam { kind: ffi::PARAM_SW_SIZE, data: size.as_mut_ptr() as *mut c_void },
            ffi::RenderParam { kind: ffi::PARAM_SW_FORMAT, data: format.as_ptr() as *mut c_void },
            ffi::RenderParam { kind: ffi::PARAM_SW_STRIDE, data: &mut stride_val as *mut usize as *mut c_void },
            ffi::RenderParam { kind: ffi::PARAM_SW_POINTER, data: buf.as_mut_ptr() as *mut c_void },
            ffi::RenderParam { kind: ffi::PARAM_INVALID, data: std::ptr::null_mut() },
        ];
        let rc = unsafe { ffi::mpv_render_context_render(ctx.0, params.as_mut_ptr()) };
        if rc < 0 {
            continue;
        }
        if let Ok(mut f) = frame.lock() {
            std::mem::swap(&mut f.data, &mut buf);
            let n = counter.fetch_add(1, Ordering::Relaxed) + 1;
            f.serial.store(n, Ordering::Relaxed);
        }
    }
}

pub enum MpvCmd {
    Load(String),
    Pause(bool),
    Seek(f64),
    Volume(f64),
    /// Turn video on/off; with a URL, switch to that stream (the music video
    /// of the current song) keeping the position.
    Video(bool, Option<String>),
    #[allow(dead_code)]
    Stop,
}

pub struct Mpv {
    tx: Sender<MpvCmd>,
    pub frame: SharedFrame,
}

impl Mpv {
    pub fn new(shared: Arc<Mutex<Playback>>, events: Sender<Event>) -> Self {
        let sock = crate::paths::mpv_socket();
        // An older daemon (or the old external mpv) may still own the socket.
        if let Ok(mut stale) = UnixStream::connect(&sock) {
            let _ = stale.write_all(b"{\"command\":[\"quit\"]}\n");
            thread::sleep(Duration::from_millis(200));
        }
        let _ = std::fs::remove_file(&sock);

        let frame: SharedFrame = Arc::new(Mutex::new(Frame {
            data: vec![0u8; (VIDEO_W * 4 * VIDEO_H) as usize],
            serial: AtomicU64::new(0),
        }));

        unsafe {
            // libmpv refuses to start unless numbers are formatted the C way,
            // and GTK has just switched to the user's locale (es_CL: "1,5").
            libc::setlocale(libc::LC_NUMERIC, c"C".as_ptr());
            let h = ffi::mpv_create();
            if h.is_null() {
                panic!("no se pudo iniciar libmpv");
            }
            let sock_s = sock.display().to_string();
            for (k, v) in [
                ("idle", "yes"),
                ("vid", "no"),
                ("vo", "libmpv"),
                ("terminal", "no"),
                ("input-terminal", "no"),
                ("cache", "yes"),
                ("cache-pause", "no"),
                ("cache-pause-initial", "no"),
                ("demuxer-max-bytes", "67108864"),
                ("ytdl", "yes"),
                ("ytdl-format", "bestaudio/best"),
                ("audio-client-name", "Marmota Music"),
                ("title", "Marmota Music"),
                ("gapless-audio", "yes"),
                ("hwdec", "auto-copy"),
                ("keep-open", "no"),
                ("input-ipc-server", sock_s.as_str()),
            ] {
                let (k, v) = (CString::new(k).unwrap(), CString::new(v).unwrap());
                ffi::mpv_set_option_string(h, k.as_ptr(), v.as_ptr());
            }
            if ffi::mpv_initialize(h) < 0 {
                panic!("libmpv no pudo inicializarse");
            }

            let api = CString::new("sw").unwrap();
            let mut params = [
                ffi::RenderParam { kind: ffi::PARAM_API_TYPE, data: api.as_ptr() as *mut c_void },
                ffi::RenderParam { kind: ffi::PARAM_INVALID, data: std::ptr::null_mut() },
            ];
            let mut ctx: *mut c_void = std::ptr::null_mut();
            if ffi::mpv_render_context_create(&mut ctx, h, params.as_mut_ptr()) >= 0 && !ctx.is_null() {
                let wake = Arc::new(Wake { pending: Mutex::new(false), cv: Condvar::new() });
                // The callback holds a raw pointer for the life of the process.
                let raw = Arc::into_raw(wake.clone()) as *mut c_void;
                ffi::mpv_render_context_set_update_callback(ctx, Some(on_update), raw);
                let f2 = frame.clone();
                let counter = Arc::new(AtomicU64::new(0));
                let rc = RenderCtx(ctx);
                thread::Builder::new()
                    .name("mpv-render".into())
                    .spawn(move || render_loop(rc, wake, f2, counter))
                    .ok();
            }
        }

        let (tx, rx) = std::sync::mpsc::channel();
        let sock2 = sock.clone();
        thread::Builder::new()
            .name("mpv".into())
            .spawn(move || run(sock2, rx, shared, events))
            .ok();

        Self { tx, frame }
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
    pub fn video(&self, on: bool, url: Option<String>) {
        let _ = self.tx.send(MpvCmd::Video(on, url));
    }
    #[allow(dead_code)]
    pub fn stop(&self) {
        let _ = self.tx.send(MpvCmd::Stop);
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
                MpvCmd::Video(on, url) => {
                    if let Some(u) = url {
                        current_url = u;
                    }
                    if on && !current_url.is_empty() {
                        // mpv resolved the track as audio-only; reload it with a
                        // video-capable format and pick up where we left off.
                        let pos = shared.lock().map(|s| s.pos).unwrap_or(0.0);
                        send(&mut stream, &mut rid, json!(["set_property", "ytdl-format", "bestvideo[height<=720][vcodec!*=av01]+bestaudio/best[height<=720]/best"]));
                        send(&mut stream, &mut rid, json!(["set_property", "vid", "auto"]));
                        resume_on_load = Some(pos);
                        ended_sent = false;
                        send(&mut stream, &mut rid, json!(["loadfile", current_url, "replace"]));
                    } else if !on {
                        // Keep the current stream playing; just drop the picture.
                        send(&mut stream, &mut rid, json!(["set_property", "vid", "no"]));
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
            {}
            Err(_) => break,
        }
    }
}
