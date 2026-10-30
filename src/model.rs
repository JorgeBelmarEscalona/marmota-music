use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum Source {
    #[default]
    YouTube,
    Spotify,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Track {
    pub id: String,
    pub title: String,
    pub artist: String,
    pub album: String,
    pub duration: f64,
    pub art: String,
    pub source: String,
}

impl Track {
    pub fn url(&self) -> String {
        match self.source.as_str() {
            "spotify" => format!("https://open.spotify.com/track/{}", self.id),
            "local" => self.id.clone(),
            _ => format!("https://music.youtube.com/watch?v={}", self.id),
        }
    }

    pub fn subtitle(&self) -> String {
        if self.artist.is_empty() {
            "Desconocido".into()
        } else {
            self.artist.clone()
        }
    }
}

#[derive(Clone, Debug, Default)]
pub struct LyricLine {
    pub t: f64,
    pub text: String,
}

#[derive(Clone, Debug, Default)]
pub struct Playlist {
    pub id: i64,
    pub name: String,
    pub count: i64,
    /// YouTube Music browse id for imported / followed playlists.
    pub remote: String,
    pub art: String,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum CollKind {
    Album,
    #[default]
    Playlist,
    Mix,
}

/// An album, playlist or mix that can be opened to list its songs.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Collection {
    pub id: String,
    pub title: String,
    pub subtitle: String,
    pub art: String,
    pub kind: CollKind,
}

/// Live playback values published by the mpv thread.
#[derive(Clone, Debug)]
pub struct Playback {
    pub pos: f64,
    pub dur: f64,
    pub playing: bool,
    pub volume: f64,
    pub idle: bool,
    pub has_video: bool,
}

impl Default for Playback {
    fn default() -> Self {
        Self {
            pos: 0.0,
            dur: 0.0,
            playing: false,
            volume: 70.0,
            idle: true,
            has_video: false,
        }
    }
}

/// Small, cheap view of the player that other processes (the bar, the CLI)
/// can read over the IPC socket.
#[derive(Clone, Debug, Default, Serialize)]
pub struct Snapshot {
    pub playing: bool,
    pub pos: f64,
    pub duration: f64,
    pub volume: f64,
    pub title: String,
    pub artist: String,
    pub album: String,
    pub art: String,
    pub source: String,
    pub expanded: bool,
    pub queue_len: usize,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(tag = "cmd", rename_all = "kebab-case")]
pub enum Cmd {
    Toggle,
    Show,
    Collapse,
    Hide,
    PlayPause,
    Next,
    Prev,
    Seek { pos: f64 },
    Volume { value: f64 },
    Play { id: String, title: Option<String>, artist: Option<String> },
    Search { query: Option<String> },
    Quit,
    ToggleVideo,
    Click { x: f64, y: f64 },
    Status,
}

#[derive(Debug)]
pub enum Event {
    SearchDone(String, Vec<Track>, Vec<Collection>),
    HomeDone(Vec<Collection>, Vec<Track>),
    /// (collection id, title, tracks)
    CollectionDone(String, String, Vec<Track>),
    /// (seed video id, tracks)
    RadioDone(String, Vec<Track>),
    ThumbDone(String, std::path::PathBuf),
    ImportDone(Result<usize, String>),
    /// (track id, music video id if one was found)
    VideoFound(String, Option<String>),
    LyricsDone(String, Vec<LyricLine>, bool),
    CoverDone(String, std::path::PathBuf),
    Ended,
    Cmd(Cmd),
    Error(String),
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Rect {
    pub x: f64,
    pub y: f64,
    pub w: f64,
    pub h: f64,
}

impl Rect {
    pub fn new(x: f64, y: f64, w: f64, h: f64) -> Self {
        Self { x, y, w, h }
    }
    pub fn contains(&self, px: f64, py: f64) -> bool {
        px >= self.x && px <= self.x + self.w && py >= self.y && py <= self.y + self.h
    }
    pub fn cx(&self) -> f64 {
        self.x + self.w / 2.0
    }
    pub fn cy(&self) -> f64 {
        self.y + self.h / 2.0
    }
}
