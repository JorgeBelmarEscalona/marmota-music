//! Minimal MPRIS client for Spotify.
//!
//! The actual playback happens in an external librespot-based service
//! (`spotifyd`, or the official client). This module polls its MPRIS
//! interface, keeps a snapshot of what is playing and forwards transport
//! commands, so the notch can drive Spotify exactly like it drives mpv.

use std::collections::HashMap;
use std::sync::mpsc::{channel, Receiver, RecvTimeoutError, Sender};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

use zbus::blocking::{Connection, Proxy};
use zbus::zvariant::{ObjectPath, OwnedValue};

#[derive(Clone, Default)]
pub struct State {
    pub available: bool,
    pub playing: bool,
    pub title: String,
    pub artist: String,
    pub album: String,
    pub art: String,
    pub track_id: String,
    pub pos: f64,
    pub dur: f64,
    pub volume: f64,
}

pub enum Cmd {
    Play,
    Pause,
    PlayPause,
    Next,
    Prev,
    Seek(f64),
    Volume(f64),
}

pub struct Spotify {
    tx: Sender<Cmd>,
    state: Arc<Mutex<State>>,
}

impl Spotify {
    pub fn start() -> Self {
        let (tx, rx) = channel();
        let state = Arc::new(Mutex::new(State::default()));
        let shared = state.clone();
        thread::spawn(move || run(rx, shared));
        Self { tx, state }
    }

    pub fn snapshot(&self) -> State {
        self.state.lock().map(|s| s.clone()).unwrap_or_default()
    }

    pub fn send(&self, cmd: Cmd) {
        let _ = self.tx.send(cmd);
    }
}

type Client = (Connection, Proxy<'static>, Proxy<'static>);

fn connect() -> Option<Client> {
    let conn = Connection::session().ok()?;
    let dbus = Proxy::new(
        &conn,
        "org.freedesktop.DBus",
        "/org/freedesktop/DBus",
        "org.freedesktop.DBus",
    )
    .ok()?;
    let names: Vec<String> = dbus.call("ListNames", &()).ok()?;
    let owner = names.into_iter().find(|n| {
        n.starts_with("org.mpris.MediaPlayer2.spotify")
    })?;
    let proxy = Proxy::new(
        &conn,
        owner.clone(),
        "/org/mpris/MediaPlayer2",
        "org.mpris.MediaPlayer2.Player",
    )
    .ok()?;
    // Read properties through org.freedesktop.DBus.Properties directly: zbus
    // caches property reads on the player proxy until a PropertiesChanged
    // signal arrives, and connectivity daemons don't signal Position.
    let props = Proxy::new(
        &conn,
        owner,
        "/org/mpris/MediaPlayer2",
        "org.freedesktop.DBus.Properties",
    )
    .ok()?;
    Some((conn, proxy, props))
}

fn text(v: Option<&OwnedValue>) -> String {
    match v {
        Some(v) => String::try_from(v.clone()).unwrap_or_default(),
        None => String::new(),
    }
}

fn trackid(v: Option<&OwnedValue>) -> String {
    let Some(v) = v else { return String::new() };
    if let Ok(s) = String::try_from(v.clone()) {
        return s;
    }
    if let Ok(p) = ObjectPath::try_from(v.clone()) {
        return p.to_string();
    }
    String::new()
}

fn number(v: Option<&OwnedValue>) -> f64 {
    let Some(v) = v else { return 0.0 };
    if let Ok(n) = i64::try_from(v.clone()) {
        return n as f64;
    }
    if let Ok(n) = u64::try_from(v.clone()) {
        return n as f64;
    }
    if let Ok(n) = f64::try_from(v.clone()) {
        return n;
    }
    0.0
}

fn prop<T>(props: &Proxy<'static>, name: &str) -> Option<T>
where
    T: TryFrom<OwnedValue>,
{
    let v: OwnedValue = props
        .call("Get", &("org.mpris.MediaPlayer2.Player", name))
        .ok()?;
    T::try_from(v).ok()
}

fn poll(props: &Proxy<'static>, prev: &State) -> Option<State> {
    let status: String = prop(props, "PlaybackStatus")?;
    let meta: HashMap<String, OwnedValue> = prop(props, "Metadata")?;
    let volume: f64 = prop(props, "Volume").unwrap_or(prev.volume);
    let pos: f64 = prop::<i64>(props, "Position")
        .map(|v| v as f64 / 1_000_000.0)
        .unwrap_or(prev.pos);
    let dur = number(meta.get("mpris:length")) / 1_000_000.0;
    Some(State {
        available: true,
        playing: status == "Playing",
        title: text(meta.get("xesam:title")),
        artist: meta
            .get("xesam:artist")
            .and_then(|v| Vec::<String>::try_from(v.clone()).ok())
            .map(|a| a.join(", "))
            .unwrap_or_default(),
        album: text(meta.get("xesam:album")),
        art: text(meta.get("mpris:artUrl")),
        track_id: trackid(meta.get("mpris:trackid")),
        pos,
        dur: if dur > 1.0 { dur } else { prev.dur },
        volume,
    })
}

fn apply(proxy: &Proxy<'static>, cmd: Cmd, state: &State) -> bool {
    match cmd {
        Cmd::Play => proxy.call_method("Play", &()).is_ok(),
        Cmd::Pause => proxy.call_method("Pause", &()).is_ok(),
        Cmd::PlayPause => proxy.call_method("PlayPause", &()).is_ok(),
        Cmd::Next => proxy.call_method("Next", &()).is_ok(),
        Cmd::Prev => proxy.call_method("Previous", &()).is_ok(),
        Cmd::Seek(pos) => {
            if state.track_id.is_empty() {
                return true;
            }
            let Ok(path) = ObjectPath::try_from(state.track_id.as_str()) else {
                return true;
            };
            proxy
                .call_method("SetPosition", &(path, (pos * 1_000_000.0) as i64))
                .is_ok()
        }
        Cmd::Volume(v) => proxy.set_property("Volume", v.clamp(0.0, 1.0)).is_ok(),
    }
}

fn run(rx: Receiver<Cmd>, st: Arc<Mutex<State>>) {
    let mut client: Option<Client> = None;
    loop {
        if client.is_none() {
            client = connect();
            let available = client.is_some();
            if let Ok(mut s) = st.lock() {
                s.available = available;
                if !available {
                    let keep = s.volume;
                    *s = State::default();
                    s.volume = keep;
                }
            }
            if !available {
                // spotifyd may start later; retry quietly.
                thread::sleep(Duration::from_secs(3));
                continue;
            }
        }

        let Some((_conn, proxy, props)) = client.as_ref() else {
            continue;
        };
        let prev = st.lock().map(|s| s.clone()).unwrap_or_default();
        match poll(props, &prev) {
            Some(next) => {
                eprintln!("[{:?}] spotify poll: {} {} pos={:.1}", std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_millis()).unwrap_or(0) % 100000, next.playing, next.title, next.pos);
                if let Ok(mut s) = st.lock() {
                    *s = next;
                }
            }
            None => {
                eprintln!("spotify poll failed");
                if let Ok(mut s) = st.lock() {
                    s.available = false;
                }
                client = None;
                continue;
            }
        }

        let state_now = st.lock().map(|s| s.clone()).unwrap_or_default();
        match rx.recv_timeout(Duration::from_millis(300)) {
            Ok(cmd) => {
                if !apply(proxy, cmd, &state_now) {
                    client = None;
                }
            }
            Err(RecvTimeoutError::Timeout) => {}
            Err(RecvTimeoutError::Disconnected) => break,
        }
    }
}
