use gtk4 as gtk;
use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;
use std::sync::mpsc::{Receiver, Sender};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use gtk::prelude::*;
use gtk4_layer_shell::{Edge, KeyboardMode, Layer, LayerShell};

use crate::db::Db;
use crate::lyrics;
use crate::model::*;
use crate::mpv::{self, Mpv};
use crate::paths;
use crate::spotify::{self, Cmd as SpotifyCmd, Spotify};
use crate::theme::{self, Rgb};
use crate::{rank, spotify_api, yt, ytm};

pub const COLLAPSED_W: f64 = 664.0;
pub const COLLAPSED_H: f64 = 74.0;
pub const EXPANDED_W: f64 = 1040.0;
pub const EXPANDED_H: f64 = 612.0;
const ANIM_MS: f64 = 320.0;

pub const TRACK_ROW: f64 = 50.0;
pub const COLL_ROW: f64 = 56.0;
pub const HEADER_ROW: f64 = 30.0;
pub const DETAIL_HEAD: f64 = 112.0;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Tab {
    Lyrics,
    Library,
    Search,
    Queue,
}

/// Which list a track row points into.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum List {
    Results,
    Saved,
    History,
    Detail,
    Home,
    Queue,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum RowItem {
    Header(&'static str),
    Message(&'static str),
    Track(List, usize),
    /// "Play next" button on a track row.
    Enqueue(List, usize),
    Unsave(usize),
    Dequeue(usize),
    RemoveFromPlaylist(usize),
    SearchColl(usize),
    HomeColl(usize),
    Mix(usize),
    Playlist(usize),
    AddTo(i64),
    DeletePlaylist(i64),
    NewPlaylist,
    Import,
    /// Open the account assistant.
    Connect,
    Back,
    DetailPlay,
    DetailShuffle,
    DetailSave,
    Lyric(usize),
}

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Hit {
    Prev,
    Play,
    Next,
    Shuffle,
    Repeat,
    Chevron,
    Seek,
    Volume,
    Video,
    Spotify,
    Tab(Tab),
    SaveCurrent,
    Close,
    Row(RowItem),
}

#[derive(Clone, Copy, Debug, Default)]
pub struct Layout {
    pub collapsed: bool,
    pub art: Rect,
    pub title: Rect,
    pub artist: Rect,
    pub lyric: Rect,
    pub chevron: Rect,
    pub play: Rect,
    pub prev: Rect,
    pub next: Rect,
    pub shuffle: Rect,
    pub repeat: Rect,
    pub seek: Rect,
    pub volume: Rect,
    pub video: Rect,
    pub spotify: Rect,
    pub cover: Rect,
    pub tabs: [Rect; 4],
    pub content: Rect,
    pub entry: Rect,
    pub save: Rect,
    pub close: Rect,
    /// Where the embedded video is painted (empty when not showing video).
    pub screen: Rect,
    /// Lyrics area: the whole pane, or what is left under the video.
    pub lyrics: Rect,
}

impl Layout {
    pub fn hit(&self, x: f64, y: f64) -> Option<Hit> {
        let fixed = [
            (self.chevron, Hit::Chevron),
            (self.play, Hit::Play),
            (self.prev, Hit::Prev),
            (self.next, Hit::Next),
            (self.shuffle, Hit::Shuffle),
            (self.repeat, Hit::Repeat),
            (self.seek, Hit::Seek),
            (self.volume, Hit::Volume),
            (self.video, Hit::Video),
            (self.spotify, Hit::Spotify),
            (self.close, Hit::Close),
            (self.save, Hit::SaveCurrent),
        ];
        for (r, h) in fixed {
            if r.contains(x, y) {
                return Some(h);
            }
        }
        if !self.collapsed {
            for (i, r) in self.tabs.iter().enumerate() {
                if r.contains(x, y) {
                    return Some(Hit::Tab(match i {
                        0 => Tab::Lyrics,
                        1 => Tab::Library,
                        2 => Tab::Search,
                        _ => Tab::Queue,
                    }));
                }
            }
        }
        None
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Repeat {
    Off,
    All,
    One,
}

impl RowItem {
    fn hit(self) -> Option<Hit> {
        match self {
            RowItem::Header(_) | RowItem::Message(_) => None,
            other => Some(Hit::Row(other)),
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum EntryMode {
    Hidden,
    Search,
    NewPlaylist,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Drag {
    Seek,
    Volume,
}

/// An opened album / playlist / mix.
#[derive(Clone, Debug)]
pub struct Detail {
    pub key: String,
    pub title: String,
    pub subtitle: String,
    pub art: String,
    pub kind: CollKind,
    pub tracks: Vec<Track>,
    pub loading: bool,
    /// Local playlist id when it lives in the library.
    pub local: Option<i64>,
    /// YouTube Music browse id, if any.
    pub remote: String,
    pub tab: Tab,
}

pub struct App {
    pub app: gtk::Application,
    pub window: gtk::Window,
    pub area: gtk::DrawingArea,
    pub entry: gtk::Entry,

    pub playback: Arc<Mutex<Playback>>,
    pub shared: Arc<Mutex<Snapshot>>,
    pub mpv: Mpv,
    pub tx: Sender<Event>,
    pub rx: Receiver<Event>,
    pub db: Db,

    pub track: Option<Track>,
    pub queue: Vec<Track>,
    pub index: usize,
    pub shuffle: bool,
    pub repeat: Repeat,
    pub volume: f64,
    pub current_saved: bool,

    pub video: bool,
    pub video_at: Instant,
    pub has_video: bool,
    pub video_serial: u64,
    pub video_surface: Option<cairo::ImageSurface>,

    pub radio_seed: String,
    pub radio_loading: bool,
    pub radio_waiting: bool,

    /// The current Spotify track was started from our queue, so we decide
    /// what plays next (instead of Spotify's own autoplay).
    pub spotify_queue: bool,
    pub sp_started: Instant,
    pub sp_was_playing: bool,
    pub spotify_search: bool,
    pub accounts_ok: bool,

    pub playing: bool,
    pub pos: f64,
    pub dur: f64,
    /// Last position that was actually painted, so the progress bar only
    /// triggers a repaint when it moves by about a pixel.
    pub drawn_pos: f64,
    pub frame: u64,

    pub visible: bool,
    pub expanded: bool,
    pub hiding: bool,
    pub anim: f64,
    pub anim_from: f64,
    pub anim_to: f64,
    pub anim_t0: Instant,
    pub animating: bool,

    pub hover: Option<Hit>,
    pub cursor: Option<Hit>,
    pub dragging: Option<Drag>,
    pub mouse: (f64, f64),
    pub entry_shown: bool,
    pub lyric_manual: Instant,

    pub tab: Tab,
    pub lyrics: Vec<LyricLine>,
    pub lyrics_synced: bool,
    pub lyrics_state: u8, // 0 idle, 1 loading, 2 ready, 3 none
    pub lyric_scroll: f64,
    pub lyric_from: f64,
    pub lyric_to: f64,
    pub lyric_t0: Instant,
    pub list_scroll: f64,
    pub lyric_idx: usize,

    pub library: Vec<Track>,
    pub history: Vec<Track>,
    pub playlists: Vec<Playlist>,
    pub mixes: Vec<Collection>,
    pub detail: Option<Detail>,
    pub importing: bool,
    pub confirm_delete: Option<(i64, Instant)>,

    pub search_query: String,
    pub searched: String,
    pub search_edit: Option<Instant>,
    pub results: Vec<Track>,
    pub colls: Vec<Collection>,
    pub searching: bool,
    pub home_colls: Vec<Collection>,
    pub home_tracks: Vec<Track>,
    pub home_state: u8, // 0 idle, 1 loading, 2 ready, 3 failed

    pub thumbs: RefCell<HashMap<String, Option<cairo::ImageSurface>>>,
    pub thumb_tx: Sender<String>,

    pub accent: Rgb,
    pub cover: Option<cairo::ImageSurface>,
    pub cover_blur: Option<cairo::ImageSurface>,
    pub cover_key: String,
    pub source: Source,
    pub spotify: Spotify,
    pub spotify_seen: String,
    pub yt_track: Option<Track>,
    pub yt_pos: f64,
    pub yt_playing: bool,
    pub status: String,
    pub status_at: Instant,

    pub entry_mode: EntryMode,
    pub width: f64,
    pub height: f64,
    pub ui_scale: f64,
    pub input_rect: (i32, i32, i32, i32),
    pub dirty: bool,
    pub last_pos: f64,
}

pub fn run(
    rx: Receiver<Event>,
    tx: Sender<Event>,
    shared: Arc<Mutex<Snapshot>>,
) -> anyhow::Result<()> {
    let app = gtk::Application::builder()
        .application_id("dev.marmota.music")
        .build();

    let cell: Rc<RefCell<Option<(Receiver<Event>, Sender<Event>, Arc<Mutex<Snapshot>>)>>> =
        Rc::new(RefCell::new(Some((rx, tx, shared))));

    app.connect_activate(move |app| {
        let Some((rx, tx, shared)) = cell.borrow_mut().take() else {
            return;
        };
        if let Err(e) = build(app, rx, tx, shared) {
            eprintln!("marmota-music: {e:?}");
            app.quit();
        }
    });

    app.run_with_args::<&str>(&[]);
    Ok(())
}

/// A few threads that fetch list thumbnails into the cache.
fn thumb_workers(events: Sender<Event>) -> Sender<String> {
    let (tx, rx) = std::sync::mpsc::channel::<String>();
    let rx = Arc::new(Mutex::new(rx));
    for _ in 0..4 {
        let rx = rx.clone();
        let events = events.clone();
        std::thread::spawn(move || {
            let client = reqwest::blocking::Client::builder()
                .timeout(Duration::from_secs(10))
                .build()
                .ok();
            loop {
                let url = {
                    let Ok(guard) = rx.lock() else { return };
                    match guard.recv() {
                        Ok(u) => u,
                        Err(_) => return,
                    }
                };
                let path = paths::thumb_path(&url);
                if !path.exists() {
                    let Some(c) = &client else { continue };
                    let Ok(resp) = c.get(&url).send() else { continue };
                    if !resp.status().is_success() {
                        continue;
                    }
                    let Ok(bytes) = resp.bytes() else { continue };
                    if std::fs::write(&path, &bytes).is_err() {
                        continue;
                    }
                }
                let px = decode_pixels(&path, 112);
                if px.is_none() {
                    let _ = std::fs::remove_file(&path);
                }
                let _ = events.send(Event::ThumbDone(url, px));
            }
        });
    }
    tx
}

fn build(
    app: &gtk::Application,
    rx: Receiver<Event>,
    tx: Sender<Event>,
    shared: Arc<Mutex<Snapshot>>,
) -> anyhow::Result<()> {
    paths::ensure_dirs();

    let css = gtk::CssProvider::new();
    css.load_from_data(
        r#"
        window.mm-window { background-color: rgba(0,0,0,0); background-image: none; }
        entry.mm-entry {
            background-color: rgba(255,255,255,0.055);
            border: 1px solid rgba(255,255,255,0.10);
            border-radius: 12px;
            padding: 5px 12px;
            color: #eef0f5;
            caret-color: #e0784f;
            font-size: 13px;
            box-shadow: none;
            min-height: 26px;
        }
        entry.mm-entry:focus,
        entry.mm-entry:focus-within {
            border-color: rgba(224,120,79,0.65);
            background-color: rgba(255,255,255,0.075);
            outline: none;
            box-shadow: none;
        }
        entry.mm-entry > text { background: transparent; box-shadow: none; }
        entry.mm-entry text { background: transparent; }
        entry.mm-entry placeholder { color: rgba(255,255,255,0.32); }
        "#,
    );
    if let Some(display) = gtk::gdk::Display::default() {
        gtk::style_context_add_provider_for_display(
            &display,
            &css,
            gtk::STYLE_PROVIDER_PRIORITY_APPLICATION,
        );
    }

    let window = gtk::Window::new();
    window.set_application(Some(app));
    window.set_decorated(false);
    window.set_resizable(false);
    window.set_title(Some("Marmota Music"));
    window.add_css_class("mm-window");
    window.set_default_size(COLLAPSED_W as i32, COLLAPSED_H as i32);

    window.init_layer_shell();
    window.set_namespace(Some("marmota-music"));
    window.set_layer(Layer::Top);
    window.set_anchor(Edge::Bottom, true);
    window.set_margin(Edge::Bottom, 14);
    window.set_exclusive_zone(0);
    window.set_keyboard_mode(KeyboardMode::OnDemand);
    window.set_visible(false);

    let area = gtk::DrawingArea::new();
    area.set_hexpand(true);
    area.set_vexpand(true);

    // The entry is a direct overlay child (not inside a full-size container)
    // so it receives its own clicks while the drawing area keeps the rest.
    let entry = gtk::Entry::new();
    entry.add_css_class("mm-entry");
    entry.set_visible(false);
    entry.set_halign(gtk::Align::Start);
    entry.set_valign(gtk::Align::Start);
    entry.set_size_request(430, 38);
    entry.set_primary_icon_name(Some("system-search-symbolic"));

    let overlay = gtk::Overlay::new();
    overlay.set_child(Some(&area));
    overlay.add_overlay(&entry);
    window.set_child(Some(&overlay));

    let playback = Arc::new(Mutex::new(Playback::default()));
    let mpv = Mpv::new(playback.clone(), tx.clone());
    let db = Db::open()?;
    let thumb_tx = thumb_workers(tx.clone());

    let mut state = App {
        app: app.clone(),
        window: window.clone(),
        area: area.clone(),
        entry: entry.clone(),
        playback,
        shared,
        mpv,
        tx: tx.clone(),
        rx,
        db,
        track: None,
        queue: Vec::new(),
        index: 0,
        shuffle: false,
        repeat: Repeat::Off,
        volume: 70.0,
        current_saved: false,
        drawn_pos: -1.0,
        frame: 0,
        video: false,
        video_at: Instant::now(),
        has_video: false,
        video_serial: 0,
        video_surface: None,
        radio_seed: String::new(),
        radio_loading: false,
        radio_waiting: false,
        spotify_queue: false,
        sp_started: Instant::now(),
        sp_was_playing: false,
        spotify_search: spotify_api::configured(),
        accounts_ok: crate::setup::accounts_configured(),
        playing: false,
        pos: 0.0,
        dur: 0.0,
        visible: false,
        expanded: false,
        hiding: false,
        anim: 0.0,
        anim_from: 0.0,
        anim_to: 0.0,
        anim_t0: Instant::now(),
        animating: false,
        hover: None,
        cursor: None,
        dragging: None,
        mouse: (0.0, 0.0),
        entry_shown: false,
        lyric_manual: Instant::now() - Duration::from_secs(30),
        tab: Tab::Lyrics,
        lyrics: Vec::new(),
        lyrics_synced: false,
        lyrics_state: 0,
        lyric_scroll: 0.0,
        lyric_from: 0.0,
        lyric_to: 0.0,
        lyric_t0: Instant::now(),
        list_scroll: 0.0,
        lyric_idx: 0,
        library: Vec::new(),
        history: Vec::new(),
        playlists: Vec::new(),
        mixes: Vec::new(),
        detail: None,
        importing: false,
        confirm_delete: None,
        search_query: String::new(),
        searched: String::new(),
        search_edit: None,
        results: Vec::new(),
        colls: Vec::new(),
        searching: false,
        home_colls: Vec::new(),
        home_tracks: Vec::new(),
        home_state: 0,
        thumbs: RefCell::new(HashMap::new()),
        thumb_tx,
        accent: theme::ACCENT_FALLBACK,
        cover: None,
        cover_blur: None,
        cover_key: String::new(),
        source: Source::YouTube,
        spotify: Spotify::start(),
        spotify_seen: String::new(),
        yt_track: None,
        yt_pos: 0.0,
        yt_playing: false,
        status: String::new(),
        status_at: Instant::now(),
        entry_mode: EntryMode::Hidden,
        width: COLLAPSED_W,
        height: COLLAPSED_H,
        ui_scale: 1.0,
        input_rect: (0, 0, 0, 0),
        dirty: true,
        last_pos: -1.0,
    };
    state.refresh_library();

    let app_cell = Rc::new(RefCell::new(state));

    // --- draw -------------------------------------------------------------
    {
        let a = app_cell.clone();
        area.set_draw_func(move |_area, cr, sw, sh| {
            let s = a.borrow();
            let t0 = Instant::now();
            // The window keeps the expanded size; the pill is painted inside
            // it (bottom-centred) at its animated size. Resizing the surface
            // every frame lagged a frame behind, jittered and reallocated
            // buffers on each step.
            let _ = (sw, sh);
            s.draw(cr, s.width, s.height);
            if std::env::var_os("MM_PROFILE").is_some() {
                eprintln!("draw {:.1}ms anim={:.2} tab={:?}", t0.elapsed().as_secs_f64() * 1000.0, s.anim, s.tab);
            }
        });
    }

    // --- pointer ----------------------------------------------------------
    {
        let a = app_cell.clone();
        let motion = gtk::EventControllerMotion::new();
        motion.connect_motion(move |_, x, y| {
            let mut s = a.borrow_mut();
            s.on_motion(x, y);
        });
        area.add_controller(motion);
    }
    {
        let a = app_cell.clone();
        let a2 = app_cell.clone();
        let click = gtk::GestureClick::new();
        click.connect_pressed(move |_, _n, x, y| {
            let mut s = a.borrow_mut();
            s.on_press(x, y);
        });
        click.connect_released(move |_, _n, x, y| {
            let mut s = a2.borrow_mut();
            s.dragging = None;
            s.on_motion(x, y);
            s.dirty = true;
        });
        area.add_controller(click);
    }
    {
        let a = app_cell.clone();
        let scroll = gtk::EventControllerScroll::new(gtk::EventControllerScrollFlags::VERTICAL);
        scroll.connect_scroll(move |_, _dx, dy| {
            let mut s = a.borrow_mut();
            let l = s.layout();
            if l.volume.contains(s.mouse.0, s.mouse.1) {
                let target = s.volume - dy * 4.0;
                s.set_volume(target);
            } else if l.content.contains(s.mouse.0, s.mouse.1) {
                if s.tab == Tab::Lyrics {
                    s.lyric_scroll += dy * 40.0;
                    s.lyric_manual = Instant::now();
                    // Resume auto-follow with a fresh tween afterwards.
                    s.lyric_to = f64::MIN;
                    s.dirty = true;
                } else {
                    s.scroll_list(dy);
                }
            }
            glib::Propagation::Stop
        });
        area.add_controller(scroll);
    }

    // --- keyboard ---------------------------------------------------------
    {
        let a = app_cell.clone();
        let keys = gtk::EventControllerKey::new();
        keys.set_propagation_phase(gtk::PropagationPhase::Capture);
        keys.connect_key_pressed(move |_, key, _code, _mods| {
            let Ok(mut s) = a.try_borrow_mut() else {
                return glib::Propagation::Proceed;
            };
            if s.entry.has_focus() {
                if key == gtk::gdk::Key::Escape {
                    if s.entry_mode == EntryMode::Search && !s.search_query.is_empty() {
                        s.search_query.clear();
                        s.entry.set_text("");
                        s.results.clear();
                        s.colls.clear();
                        s.searched.clear();
                        s.dirty = true;
                    } else {
                        s.close_entry();
                        s.area.grab_focus();
                    }
                    return glib::Propagation::Stop;
                }
                return glib::Propagation::Proceed;
            }
            match key {
                gtk::gdk::Key::Escape => {
                    if s.detail.as_ref().is_some_and(|d| d.tab == s.tab) {
                        s.close_detail();
                    } else if s.expanded {
                        s.collapse();
                    } else {
                        s.hide();
                    }
                    glib::Propagation::Stop
                }
                gtk::gdk::Key::space => {
                    s.toggle_play();
                    glib::Propagation::Stop
                }
                gtk::gdk::Key::Left => {
                    s.seek_relative(-5.0);
                    glib::Propagation::Stop
                }
                gtk::gdk::Key::Right => {
                    s.seek_relative(5.0);
                    glib::Propagation::Stop
                }
                gtk::gdk::Key::Up => {
                    s.adjust_volume(4.0);
                    glib::Propagation::Stop
                }
                gtk::gdk::Key::Down => {
                    s.adjust_volume(-4.0);
                    glib::Propagation::Stop
                }
                gtk::gdk::Key::slash | gtk::gdk::Key::f => {
                    s.open_search();
                    glib::Propagation::Stop
                }
                gtk::gdk::Key::v => {
                    let on = !s.video;
                    s.set_video(on);
                    glib::Propagation::Stop
                }
                _ => glib::Propagation::Proceed,
            }
        });
        window.add_controller(keys);
    }

    // --- entry ------------------------------------------------------------
    {
        let weak = Rc::downgrade(&app_cell);
        entry.connect_changed(move |e| {
            // set_text() emits this signal synchronously while App is borrowed,
            // so never hard-borrow here.
            if let Some(a) = weak.upgrade() {
                if let Ok(mut s) = a.try_borrow_mut() {
                    if s.entry_mode == EntryMode::Search {
                        s.search_query = e.text().to_string();
                        s.search_edit = Some(Instant::now());
                    }
                }
            }
        });
    }
    {
        let weak = Rc::downgrade(&app_cell);
        entry.connect_activate(move |_| {
            if let Some(a) = weak.upgrade() {
                let Ok(mut s) = a.try_borrow_mut() else { return };
                match s.entry_mode {
                    EntryMode::Search => {
                        s.search_edit = None;
                        s.searched.clear();
                        s.run_search();
                    }
                    EntryMode::NewPlaylist => s.create_playlist_from_entry(),
                    EntryMode::Hidden => {}
                }
            }
        });
    }

    // --- ticker -----------------------------------------------------------
    {
        let a = app_cell.clone();
        // While something animates, tick from GTK's frame clock so every
        // displayed frame gets exactly one update (vsync-aligned).
        let fa = app_cell.clone();
        area.add_tick_callback(move |_, _| {
            if let Ok(mut s) = fa.try_borrow_mut() {
                if s.animating {
                    s.tick();
                }
            }
            glib::ControlFlow::Continue
        });
        glib::timeout_add_local(Duration::from_millis(16), move || {
            if let Ok(mut s) = a.try_borrow_mut() {
                if s.animating && s.area.is_mapped() {
                    return glib::ControlFlow::Continue;
                }
                let t0 = Instant::now();
                s.tick();
                let ms = t0.elapsed().as_secs_f64() * 1000.0;
                if s.animating && std::env::var_os("MM_PROFILE").is_some() {
                    eprintln!(
                        "anim {:.2} want {:.0}x{:.0} window {}x{} area {}x{} entry={}",
                        s.anim, s.width * s.ui_scale, s.height * s.ui_scale,
                        s.window.width(), s.window.height(), s.area.width(), s.area.height(),
                        WidgetExt::is_visible(&s.entry)
                    );
                }
                if ms > 4.0 && std::env::var_os("MM_PROFILE").is_some() {
                    eprintln!("tick {ms:.1}ms");
                }
            }
            glib::ControlFlow::Continue
        });
    }

    let _ = tx;
    let _ = window;
    Ok(())
}

impl App {
    // ------------------------------------------------------------- geometry
    pub fn layout(&self) -> Layout {
        if self.anim < 0.55 {
            self.collapsed_layout()
        } else {
            self.expanded_layout()
        }
    }

    pub(crate) fn collapsed_layout(&self) -> Layout {
        let (w, _) = self.surface_size();
        let mut l = Layout::default();
        l.collapsed = true;
        l.art = Rect::new(14.0, 15.0, 46.0, 46.0);
        l.title = Rect::new(72.0, 15.0, 200.0, 24.0);
        l.artist = Rect::new(72.0, 38.0, 200.0, 20.0);
        l.lyric = Rect::new(282.0, 8.0, 152.0, 58.0);
        l.prev = Rect::new(444.0, 21.0, 34.0, 34.0);
        l.play = Rect::new(484.0, 16.0, 42.0, 42.0);
        l.next = Rect::new(532.0, 21.0, 34.0, 34.0);
        l.chevron = Rect::new(w - 54.0, 19.0, 42.0, 38.0);
        l
    }

    /// Size of the surface as last painted (falls back to the requested
    /// size before the first frame).
    pub fn surface_size(&self) -> (f64, f64) {
        (self.width, self.height)
    }

    /// Top-left of the pill inside the fixed-size window (logical px).
    pub fn origin(&self) -> (f64, f64) {
        ((EXPANDED_W - self.width) / 2.0, EXPANDED_H - self.height)
    }

    /// Only the visible pill takes clicks; the transparent rest of the
    /// window lets them through to whatever is behind.
    fn sync_input_region(&mut self) {
        let k = self.ui_scale;
        let (ox, oy) = self.origin();
        let rect = (
            (ox * k).floor() as i32,
            (oy * k).floor() as i32,
            (self.width * k).ceil() as i32,
            (self.height * k).ceil() as i32,
        );
        if rect == self.input_rect {
            return;
        }
        if let Some(surface) = self.window.surface() {
            let region = cairo::Region::create_rectangle(&cairo::RectangleInt::new(
                rect.0, rect.1, rect.2, rect.3,
            ));
            surface.set_input_region(Some(&region));
            self.input_rect = rect;
        }
    }

    pub fn showing_video(&self) -> bool {
        self.video && self.has_video && self.video_surface.is_some()
    }

    pub(crate) fn expanded_layout(&self) -> Layout {
        let (w, h) = self.surface_size();
        let mut l = Layout::default();
        l.cover = Rect::new(30.0, 30.0, 218.0, 218.0);
        l.title = Rect::new(30.0, 266.0, 340.0, 38.0);
        l.artist = Rect::new(30.0, 304.0, 340.0, 22.0);
        l.seek = Rect::new(30.0, 374.0, 340.0, 18.0);
        l.shuffle = Rect::new(64.0, 420.0, 34.0, 32.0);
        l.prev = Rect::new(112.0, 414.0, 40.0, 40.0);
        l.play = Rect::new(156.0, 402.0, 64.0, 64.0);
        l.next = Rect::new(228.0, 414.0, 40.0, 40.0);
        l.repeat = Rect::new(282.0, 420.0, 34.0, 32.0);
        l.volume = Rect::new(30.0, 502.0, 340.0, 28.0);
        l.save = Rect::new(30.0, 550.0, 136.0, 34.0);
        l.video = Rect::new(180.0, 550.0, 34.0, 34.0);
        l.spotify = Rect::new(222.0, 550.0, 34.0, 34.0);
        l.chevron = Rect::new(w - 66.0, 26.0, 42.0, 38.0);
        l.tabs = [
            Rect::new(430.0, 28.0, 92.0, 36.0),
            Rect::new(534.0, 28.0, 118.0, 36.0),
            Rect::new(664.0, 28.0, 100.0, 36.0),
            Rect::new(776.0, 28.0, 84.0, 36.0),
        ];
        // The entry lines up with the content pane's edges and sits clear of it.
        l.entry = Rect::new(420.0, 80.0, w - 438.0, 40.0);
        l.content = Rect::new(430.0, 84.0, w - 458.0, h - 112.0);
        let entry_visible = self.entry_mode != EntryMode::Hidden
            && !(self.tab == Tab::Search && self.detail_here());
        if entry_visible {
            l.content = Rect::new(430.0, 150.0, w - 458.0, h - 178.0);
        }
        l.lyrics = l.content;
        if self.tab == Tab::Lyrics && self.showing_video() {
            let vh = (l.content.w * 9.0 / 16.0).round();
            l.screen = Rect::new(l.content.x, l.content.y, l.content.w, vh);
            l.lyrics = Rect::new(
                l.content.x,
                l.content.y + vh + 8.0,
                l.content.w,
                (l.content.h - vh - 8.0).max(0.0),
            );
        }
        l
    }

    /// Is the opened album/playlist showing in the current tab?
    pub fn detail_here(&self) -> bool {
        self.detail.as_ref().is_some_and(|d| d.tab == self.tab)
    }

    // ---------------------------------------------------------------- rows
    /// Every row of the current tab, positioned with the scroll offset
    /// applied. Action buttons come before the row they sit on, so hit
    /// testing finds them first.
    pub fn rows(&self, content: Rect) -> Vec<(Rect, RowItem)> {
        let mut out: Vec<(Rect, RowItem)> = Vec::new();
        if self.tab == Tab::Lyrics {
            let lr = self.layout().lyrics;
            let rowh = 46.0;
            for i in 0..self.lyrics.len() {
                let y = lr.y + 12.0 + i as f64 * rowh - self.lyric_scroll;
                if y + rowh < lr.y || y > lr.y + lr.h {
                    continue;
                }
                out.push((Rect::new(lr.x, y, lr.w, rowh), RowItem::Lyric(i)));
            }
            return out;
        }

        let x = content.x;
        let w = content.w;
        let mut y = content.y + 4.0 - self.list_scroll;

        let header = |out: &mut Vec<(Rect, RowItem)>, y: &mut f64, text: &'static str| {
            out.push((Rect::new(x, *y, w, HEADER_ROW), RowItem::Header(text)));
            *y += HEADER_ROW;
        };
        let message = |out: &mut Vec<(Rect, RowItem)>, y: &mut f64, text: &'static str| {
            out.push((Rect::new(x, *y, w, 44.0), RowItem::Message(text)));
            *y += 48.0;
        };
        // A track row with up to two small action buttons on the right.
        let track = |out: &mut Vec<(Rect, RowItem)>, y: &mut f64, item: RowItem, actions: &[RowItem]| {
            let r = Rect::new(x, *y, w, TRACK_ROW - 4.0);
            let mut bx = r.x + r.w - 72.0;
            for a in actions {
                out.push((Rect::new(bx, r.y + 7.0, 32.0, 32.0), *a));
                bx -= 36.0;
            }
            out.push((r, item));
            *y += TRACK_ROW;
        };
        let coll = |out: &mut Vec<(Rect, RowItem)>, y: &mut f64, item: RowItem, actions: &[RowItem]| {
            let r = Rect::new(x, *y, w, COLL_ROW - 4.0);
            let mut bx = r.x + r.w - 38.0;
            for a in actions {
                out.push((Rect::new(bx, r.y + 10.0, 32.0, 32.0), *a));
                bx -= 36.0;
            }
            out.push((r, item));
            *y += COLL_ROW;
        };

        if let Some(d) = self.detail.as_ref().filter(|d| d.tab == self.tab) {
            out.push((Rect::new(x, y, 92.0, 28.0), RowItem::Back));
            let by = y + 72.0;
            out.push((Rect::new(x + 108.0, by, 118.0, 32.0), RowItem::DetailPlay));
            out.push((Rect::new(x + 234.0, by, 112.0, 32.0), RowItem::DetailShuffle));
            if d.local.is_none() && !d.tracks.is_empty() {
                out.push((Rect::new(x + 354.0, by, 104.0, 32.0), RowItem::DetailSave));
            }
            y += DETAIL_HEAD;
            if d.loading && d.tracks.is_empty() {
                message(&mut out, &mut y, "Cargando canciones…");
            } else if d.tracks.is_empty() {
                message(&mut out, &mut y, "Esta lista está vacía");
            }
            let editable = d.local.is_some() && d.remote.is_empty();
            for i in 0..d.tracks.len() {
                let mut actions = vec![RowItem::Enqueue(List::Detail, i)];
                if editable {
                    actions.push(RowItem::RemoveFromPlaylist(i));
                }
                track(&mut out, &mut y, RowItem::Track(List::Detail, i), &actions);
            }
            return out;
        }

        match self.tab {
            Tab::Lyrics => {}
            Tab::Library => {
                let half = (w - 8.0) / 2.0;
                out.push((Rect::new(x, y, half, 36.0), RowItem::Import));
                out.push((Rect::new(x + half + 8.0, y, half, 36.0), RowItem::NewPlaylist));
                y += 46.0;
                header(&mut out, &mut y, "TUS LISTAS");
                if self.playlists.is_empty() {
                    message(&mut out, &mut y, "Importa tus listas de YouTube Music o crea una");
                }
                for (i, p) in self.playlists.iter().enumerate() {
                    coll(&mut out, &mut y, RowItem::Playlist(i), &[RowItem::DeletePlaylist(p.id), RowItem::AddTo(p.id)]);
                }
                y += 6.0;
                header(&mut out, &mut y, "CANCIONES GUARDADAS");
                if self.library.is_empty() {
                    message(&mut out, &mut y, "Usa «Guardar» para tener aquí tus canciones");
                }
                for i in 0..self.library.len() {
                    track(&mut out, &mut y, RowItem::Track(List::Saved, i), &[RowItem::Unsave(i), RowItem::Enqueue(List::Saved, i)]);
                }
                if !self.history.is_empty() {
                    y += 6.0;
                    header(&mut out, &mut y, "ESCUCHADAS HACE POCO");
                    for i in 0..self.history.len() {
                        track(&mut out, &mut y, RowItem::Track(List::History, i), &[RowItem::Enqueue(List::History, i)]);
                    }
                }
            }
            Tab::Search => {
                let has_results = !self.results.is_empty() || !self.colls.is_empty();
                if self.searching && !has_results {
                    message(&mut out, &mut y, "Buscando…");
                } else if has_results && !self.search_query.trim().is_empty() {
                    let top = self.results.len().min(6);
                    if top > 0 {
                        header(&mut out, &mut y, "CANCIONES");
                        for i in 0..top {
                            track(&mut out, &mut y, RowItem::Track(List::Results, i), &[RowItem::Enqueue(List::Results, i)]);
                        }
                    }
                    if !self.colls.is_empty() {
                        y += 6.0;
                        header(&mut out, &mut y, "ÁLBUMES Y LISTAS");
                        for i in 0..self.colls.len() {
                            coll(&mut out, &mut y, RowItem::SearchColl(i), &[]);
                        }
                    }
                    if !self.spotify_search {
                        y += 4.0;
                        out.push((Rect::new(x, y, w, 40.0), RowItem::Connect));
                        y += 48.0;
                    }
                    if self.results.len() > top {
                        y += 6.0;
                        header(&mut out, &mut y, "MÁS CANCIONES");
                        for i in top..self.results.len() {
                            track(&mut out, &mut y, RowItem::Track(List::Results, i), &[RowItem::Enqueue(List::Results, i)]);
                        }
                    }
                } else if !self.searched.is_empty() && !self.searching && self.search_query.trim() == self.searched {
                    message(&mut out, &mut y, "Sin resultados");
                } else {
                    if !self.mixes.is_empty() {
                        header(&mut out, &mut y, "MIX PARA TI");
                        for i in 0..self.mixes.len() {
                            coll(&mut out, &mut y, RowItem::Mix(i), &[]);
                        }
                        y += 6.0;
                    }
                    match self.home_state {
                        1 | 0 => message(&mut out, &mut y, "Cargando recomendaciones…"),
                        3 => message(&mut out, &mut y, "No se pudieron cargar las recomendaciones"),
                        _ => {
                            if !self.home_colls.is_empty() {
                                header(&mut out, &mut y, "LISTAS RECOMENDADAS");
                                for i in 0..self.home_colls.len() {
                                    coll(&mut out, &mut y, RowItem::HomeColl(i), &[]);
                                }
                                y += 6.0;
                            }
                            if !self.home_tracks.is_empty() {
                                header(&mut out, &mut y, "SELECCIÓN RÁPIDA");
                                for i in 0..self.home_tracks.len() {
                                    track(&mut out, &mut y, RowItem::Track(List::Home, i), &[RowItem::Enqueue(List::Home, i)]);
                                }
                            }
                        }
                    }
                }
            }
            Tab::Queue => {
                if self.queue.is_empty() {
                    message(&mut out, &mut y, "La cola está vacía");
                    return out;
                }
                header(&mut out, &mut y, "SONANDO AHORA");
                track(&mut out, &mut y, RowItem::Track(List::Queue, self.index), &[]);
                if self.index + 1 < self.queue.len() {
                    y += 6.0;
                    header(&mut out, &mut y, "A CONTINUACIÓN");
                    for i in self.index + 1..self.queue.len() {
                        track(&mut out, &mut y, RowItem::Track(List::Queue, i), &[RowItem::Dequeue(i)]);
                    }
                }
                if self.radio_loading {
                    message(&mut out, &mut y, "Buscando canciones parecidas…");
                }
                if self.index > 0 {
                    y += 6.0;
                    header(&mut out, &mut y, "YA SONARON");
                    for i in 0..self.index {
                        track(&mut out, &mut y, RowItem::Track(List::Queue, i), &[RowItem::Dequeue(i)]);
                    }
                }
            }
        }
        out
    }

    pub fn list_track(&self, list: List, i: usize) -> Option<&Track> {
        match list {
            List::Results => self.results.get(i),
            List::Saved => self.library.get(i),
            List::History => self.history.get(i),
            List::Detail => self.detail.as_ref().and_then(|d| d.tracks.get(i)),
            List::Home => self.home_tracks.get(i),
            List::Queue => self.queue.get(i),
        }
    }

    // ------------------------------------------------------------- input
    fn on_motion(&mut self, x: f64, y: f64) {
        let k = self.ui_scale.max(0.1);
        let (ox, oy) = self.origin();
        let (x, y) = (x / k - ox, y / k - oy);
        self.mouse = (x, y);
        let l = self.layout();
        let mut hit = l.hit(x, y);
        if hit.is_none() && !l.collapsed && l.content.contains(x, y) {
            for (r, item) in self.rows(l.content) {
                if r.contains(x, y) {
                    hit = item.hit();
                    break;
                }
            }
        }
        if hit != self.hover {
            self.hover = hit;
            self.dirty = true;
        }
        let pointer = hit.is_some();
        if pointer != self.cursor.is_some() {
            self.cursor = hit;
            self.area.set_cursor_from_name(if pointer { Some("pointer") } else { None });
        }
        if let Some(drag) = self.dragging {
            match drag {
                Drag::Seek => {
                    if l.seek.w > 0.0 {
                        let f = ((x - l.seek.x) / l.seek.w).clamp(0.0, 1.0);
                        self.seek_to(f * self.dur);
                    }
                }
                Drag::Volume => {
                    if l.volume.w > 0.0 {
                        let f = ((x - l.volume.x - 30.0) / (l.volume.w - 76.0)).clamp(0.0, 1.0);
                        self.set_volume(f * 100.0);
                    }
                }
            }
        }
    }

    fn on_press(&mut self, x: f64, y: f64) {
        let k = self.ui_scale.max(0.1);
        let (ox, oy) = self.origin();
        let (x, y) = (x / k - ox, y / k - oy);
        if x < 0.0 || y < 0.0 || x > self.width || y > self.height {
            return;
        }
        self.mouse = (x, y);
        let l = self.layout();
        if l.collapsed {
            if let Some(h) = l.hit(x, y) {
                self.act(h, x, y);
            } else {
                self.expand();
            }
            return;
        }
        if let Some(h) = l.hit(x, y) {
            self.act(h, x, y);
            return;
        }
        if l.content.contains(x, y) {
            for (r, item) in self.rows(l.content) {
                if r.contains(x, y) && item.hit().is_some() {
                    self.act_row(item);
                    return;
                }
            }
        }
        if self.entry_mode == EntryMode::NewPlaylist {
            self.close_entry();
        }
    }

    fn act(&mut self, hit: Hit, x: f64, _y: f64) {
        match hit {
            Hit::Prev => self.prev(),
            Hit::Play => self.toggle_play(),
            Hit::Next => self.advance(false),
            Hit::Shuffle => {
                self.shuffle = !self.shuffle;
                self.set_status(if self.shuffle { "Aleatorio activado" } else { "Aleatorio desactivado" });
            }
            Hit::Repeat => {
                self.repeat = match self.repeat {
                    Repeat::Off => Repeat::All,
                    Repeat::All => Repeat::One,
                    Repeat::One => Repeat::Off,
                };
                self.set_status(match self.repeat {
                    Repeat::Off => "Repetir desactivado",
                    Repeat::All => "Repetir la cola",
                    Repeat::One => "Repetir esta canción",
                });
            }
            Hit::Chevron => {
                if self.expanded {
                    self.collapse();
                } else {
                    self.expand();
                }
            }
            Hit::Close => self.hide(),
            Hit::Seek => {
                let l = self.layout();
                self.dragging = Some(Drag::Seek);
                if l.seek.w > 0.0 {
                    let f = ((x - l.seek.x) / l.seek.w).clamp(0.0, 1.0);
                    self.seek_to(f * self.dur);
                }
            }
            Hit::Video => {
                self.set_video(!self.video);
            }
            Hit::Spotify => {
                let to = if self.source == Source::Spotify {
                    Source::YouTube
                } else {
                    Source::Spotify
                };
                self.switch_source(to);
            }
            Hit::Volume => {
                let l = self.layout();
                self.dragging = Some(Drag::Volume);
                if l.volume.w > 0.0 {
                    let f = ((x - l.volume.x - 30.0) / (l.volume.w - 76.0)).clamp(0.0, 1.0);
                    self.set_volume(f * 100.0);
                }
            }
            Hit::Tab(t) => {
                if self.tab == t && self.detail_here() {
                    // Clicking the active tab again goes back to its root.
                    self.close_detail();
                    return;
                }
                self.tab = t;
                self.list_scroll = 0.0;
                if t == Tab::Library {
                    self.refresh_library();
                }
                if t == Tab::Search {
                    self.open_search();
                } else {
                    self.close_entry();
                }
                self.dirty = true;
            }
            Hit::SaveCurrent => {
                if let Some(t) = self.track.clone() {
                    let saved = !self.current_saved;
                    if self.db.set_saved(&t, saved).is_ok() {
                        self.current_saved = saved;
                        self.refresh_library();
                        self.set_status(if saved {
                            "Guardada en tu biblioteca"
                        } else {
                            "Quitada de tu biblioteca"
                        });
                    }
                }
            }
            Hit::Row(item) => self.act_row(item),
        }
    }

    fn act_row(&mut self, item: RowItem) {
        match item {
            RowItem::Track(list, i) => self.play_from(list, i),
            RowItem::Enqueue(list, i) => {
                if let Some(t) = self.list_track(list, i).cloned() {
                    self.enqueue_next(t);
                }
            }
            RowItem::Dequeue(i) => {
                if i < self.queue.len() && i != self.index {
                    self.queue.remove(i);
                    if i < self.index {
                        self.index -= 1;
                    }
                    self.clamp_scroll();
                }
            }
            RowItem::Unsave(i) => {
                if let Some(t) = self.library.get(i).cloned() {
                    let _ = self.db.set_saved(&t, false);
                    if self.track.as_ref().is_some_and(|c| c.id == t.id) {
                        self.current_saved = false;
                    }
                    self.refresh_library();
                    self.set_status("Quitada de tu biblioteca");
                }
            }
            RowItem::RemoveFromPlaylist(i) => {
                let target = self
                    .detail
                    .as_ref()
                    .and_then(|d| Some((d.local?, d.tracks.get(i)?.id.clone())));
                if let Some((pid, id)) = target {
                    let _ = self.db.remove_from_playlist(pid, &id);
                    if let Some(d) = self.detail.as_mut() {
                        d.tracks.remove(i);
                    }
                    self.refresh_library();
                    self.set_status("Quitada de la lista");
                }
            }
            RowItem::SearchColl(i) => {
                if let Some(c) = self.colls.get(i).cloned() {
                    self.open_collection(c);
                }
            }
            RowItem::HomeColl(i) => {
                if let Some(c) = self.home_colls.get(i).cloned() {
                    self.open_collection(c);
                }
            }
            RowItem::Mix(i) => {
                if let Some(c) = self.mixes.get(i).cloned() {
                    self.open_collection(c);
                }
            }
            RowItem::Playlist(i) => {
                if let Some(p) = self.playlists.get(i).cloned() {
                    self.open_playlist(p);
                }
            }
            RowItem::AddTo(pid) => match self.track.clone() {
                Some(t) => {
                    let _ = self.db.add_to_playlist(pid, &t);
                    self.refresh_library();
                    let name = self
                        .playlists
                        .iter()
                        .find(|p| p.id == pid)
                        .map(|p| p.name.clone())
                        .unwrap_or_default();
                    self.set_status(&format!("Añadida a «{name}»"));
                }
                None => self.set_status("Pon una canción para añadirla a la lista"),
            },
            RowItem::DeletePlaylist(pid) => {
                let confirmed = self
                    .confirm_delete
                    .is_some_and(|(id, at)| id == pid && at.elapsed().as_secs() < 4);
                if confirmed {
                    let _ = self.db.delete_playlist(pid);
                    self.confirm_delete = None;
                    self.refresh_library();
                    self.clamp_scroll();
                    self.set_status("Lista eliminada");
                } else {
                    self.confirm_delete = Some((pid, Instant::now()));
                    self.set_status("Pulsa otra vez la ✕ para eliminar la lista");
                }
            }
            RowItem::NewPlaylist => self.new_playlist(),
            RowItem::Import => {
                if self.accounts_ok {
                    self.import_account();
                } else {
                    self.open_setup();
                }
            }
            RowItem::Connect => self.open_setup(),
            RowItem::Back => self.close_detail(),
            RowItem::DetailPlay | RowItem::DetailShuffle => {
                if let Some(d) = self.detail.clone() {
                    if d.tracks.is_empty() {
                        return;
                    }
                    let shuffle = item == RowItem::DetailShuffle;
                    let mut list = d.tracks.clone();
                    if shuffle {
                        shuffle_vec(&mut list);
                    }
                    self.shuffle = false;
                    self.play_tracks(list, 0);
                    self.set_status(&format!("Reproduciendo «{}»", d.title));
                }
            }
            RowItem::DetailSave => self.save_detail(),
            RowItem::Lyric(i) => {
                if self.lyrics_synced {
                    if let Some(line) = self.lyrics.get(i) {
                        let t = line.t;
                        self.seek_to(t);
                        self.lyric_manual = Instant::now() - Duration::from_secs(30);
                    }
                }
            }
            RowItem::Header(_) | RowItem::Message(_) => {}
        }
        self.dirty = true;
    }

    // ----------------------------------------------------------- playback
    fn toggle_play(&mut self) {
        if self.track.is_none() {
            self.open_search();
            return;
        }
        if self.source == Source::Spotify {
            self.spotify.send(SpotifyCmd::PlayPause);
            self.playing = !self.playing;
            self.dirty = true;
            return;
        }
        if self.playing {
            self.mpv.pause(true);
            self.playing = false;
        } else {
            self.mpv.pause(false);
            self.playing = true;
        }
        self.dirty = true;
    }

    fn seek_to(&mut self, pos: f64) {
        if self.dur <= 0.0 {
            return;
        }
        let pos = pos.clamp(0.0, self.dur);
        self.pos = pos;
        if self.source == Source::Spotify {
            self.spotify.send(SpotifyCmd::Seek(pos));
        } else {
            self.mpv.seek(pos);
        }
        self.dirty = true;
    }

    fn seek_relative(&mut self, delta: f64) {
        self.seek_to(self.pos + delta);
    }

    fn set_video(&mut self, on: bool) {
        if on && self.track.is_none() {
            self.set_status("Pon una canción primero");
            return;
        }
        if on && self.source == Source::Spotify {
            self.set_status("El vídeo solo está disponible con YouTube");
            return;
        }
        self.video = on;
        self.video_at = Instant::now();
        if on {
            self.find_video();
            self.set_status("Buscando el videoclip…");
            self.tab = Tab::Lyrics;
            self.close_entry();
            if !self.expanded {
                self.expand();
            }
        } else {
            self.mpv.video(false, None);
            self.video_surface = None;
            self.set_status("Modo audio");
        }
        self.dirty = true;
    }

    /// Look up the official video of the current song and switch to it.
    fn find_video(&mut self) {
        let Some(t) = self.track.clone() else { return };
        let tx = self.tx.clone();
        std::thread::spawn(move || {
            let vid = if t.source == "youtube" {
                ytm::music_video(&t.title, &t.artist).filter(|v| *v != t.id)
            } else {
                None
            };
            let _ = tx.send(Event::VideoFound(t.id, vid));
        });
    }

    /// Switch the active audio source. The notch always reflects (and
    /// controls) exactly one engine at a time.
    fn switch_source(&mut self, src: Source) {
        if src == self.source {
            return;
        }
        match src {
            Source::Spotify => {
                let sp = self.spotify.snapshot();
                if !sp.available {
                    self.set_status("Spotify no está disponible");
                    return;
                }
                if self.video {
                    self.set_video(false);
                }
                self.yt_track = self.track.clone();
                self.yt_pos = self.pos;
                self.yt_playing = self.playing;
                self.mpv.pause(true);
                self.source = Source::Spotify;
                self.spotify_seen.clear();
                if sp.title.is_empty() {
                    self.set_status("Spotify: nada sonando");
                } else {
                    self.set_status("Fuente: Spotify");
                }
            }
            Source::YouTube => {
                self.spotify.send(SpotifyCmd::Pause);
                self.source = Source::YouTube;
                if let Some(t) = self.yt_track.clone() {
                    self.track = Some(t.clone());
                    self.pos = self.yt_pos;
                    self.dur = t.duration;
                    self.playing = self.yt_playing;
                    self.mpv.pause(!self.yt_playing);
                    self.spotify_seen.clear();
                    self.start_lyrics(t.clone());
                    self.start_cover(&t);
                } else {
                    self.track = None;
                    self.playing = false;
                    self.pos = 0.0;
                    self.dur = 0.0;
                }
                self.set_status("Fuente: YouTube Music");
            }
        }
        self.dirty = true;
    }

    /// Mirror the MPRIS snapshot into the UI, reloading lyrics and cover when
    /// Spotify moves to another track.
    fn sync_spotify(&mut self, sp: &spotify::State) {
        if sp.title.is_empty() {
            return;
        }
        let key = spotify_key(&sp.track_id);
        if self.spotify_queue && key != self.spotify_seen && self.sp_started.elapsed() < Duration::from_secs(5) {
            // Still showing the previous song while ours loads.
            return;
        }
        if self.spotify_queue && key != self.spotify_seen {
            // Spotify moved on by itself (or from the phone): follow it.
            self.spotify_queue = false;
        }
        if key == self.spotify_seen {
            if self.dur <= 0.0 && sp.dur > 0.0 {
                self.dur = sp.dur;
            }
            return;
        }
        self.spotify_seen = key.clone();
        let t = Track {
            id: key,
            title: sp.title.clone(),
            artist: sp.artist.clone(),
            album: sp.album.clone(),
            duration: sp.dur,
            art: sp.art.clone(),
            source: "spotify".into(),
            quality: 3,
            ..Default::default()
        };
        self.track = Some(t.clone());
        self.pos = sp.pos;
        self.dur = sp.dur;
        self.drawn_pos = -1.0;
        self.start_lyrics(t.clone());
        self.start_cover(&t);
        self.dirty = true;
    }

    fn set_volume(&mut self, value: f64) {
        self.volume = value.clamp(0.0, 100.0);
        if self.source == Source::Spotify {
            self.spotify.send(SpotifyCmd::Volume(self.volume / 100.0));
        } else {
            self.mpv.volume(self.volume);
        }
        self.dirty = true;
    }

    fn adjust_volume(&mut self, delta: f64) {
        self.set_volume(self.volume + delta);
    }

    fn advance(&mut self, auto: bool) {
        if self.source == Source::Spotify && !self.spotify_queue {
            self.spotify.send(SpotifyCmd::Next);
            return;
        }
        let n = self.queue.len();
        if n == 0 {
            return;
        }
        let next = if self.shuffle && n > 1 {
            let mut i = random_index(n);
            if i == self.index {
                i = (i + 1) % n;
            }
            i
        } else {
            self.index + 1
        };
        if next >= n {
            if self.repeat == Repeat::All {
                self.play_index(0);
                return;
            }
            // End of the queue: keep going with YouTube Music's radio.
            if let Some(t) = self.track.clone() {
                self.radio_waiting = true;
                self.request_radio(&t.id);
                self.set_status("Buscando canciones parecidas…");
            }
            if auto {
                self.playing = false;
                self.dirty = true;
            }
            return;
        }
        self.play_index(next);
    }

    fn prev(&mut self) {
        if self.pos > 3.0 {
            self.seek_to(0.0);
            return;
        }
        if self.source == Source::Spotify && !self.spotify_queue {
            self.spotify.send(SpotifyCmd::Prev);
            return;
        }
        let n = self.queue.len();
        if n == 0 {
            return;
        }
        let prev = if self.index == 0 { n - 1 } else { self.index - 1 };
        self.play_index(prev);
    }

    fn play_index(&mut self, i: usize) {
        if let Some(t) = self.queue.get(i).cloned() {
            self.index = i;
            self.begin_track(t);
        }
    }

    /// Play a whole list (album, playlist, saved songs) starting at `i`.
    fn play_tracks(&mut self, list: Vec<Track>, i: usize) {
        if list.is_empty() {
            return;
        }
        self.queue = list;
        self.index = i.min(self.queue.len() - 1);
        self.radio_seed.clear();
        self.radio_waiting = false;
        if let Some(t) = self.queue.get(self.index).cloned() {
            self.begin_track(t);
        }
    }

    /// Play one song and let the radio fill the queue after it, like
    /// YouTube Music does when you pick a song from search.
    fn play_single(&mut self, t: Track) {
        self.queue = vec![t.clone()];
        self.index = 0;
        self.radio_seed.clear();
        self.radio_waiting = false;
        self.begin_track(t.clone());
        self.request_radio(&t.id);
    }

    fn play_from(&mut self, list: List, i: usize) {
        let Some(t) = self.list_track(list, i).cloned() else { return };
        match list {
            List::Queue => self.play_index(i),
            List::Results | List::Home | List::History => self.play_single(t),
            List::Saved => {
                let all = self.library.clone();
                self.play_tracks(all, i);
            }
            List::Detail => {
                if let Some(d) = self.detail.clone() {
                    self.play_tracks(d.tracks, i);
                }
            }
        }
    }

    fn enqueue_next(&mut self, t: Track) {
        if self.queue.is_empty() || self.track.is_none() {
            self.play_single(t);
            return;
        }
        if let Some(pos) = self.queue.iter().position(|x| x.id == t.id) {
            if pos == self.index {
                return;
            }
            self.queue.remove(pos);
            if pos < self.index {
                self.index -= 1;
            }
        }
        let at = (self.index + 1).min(self.queue.len());
        self.queue.insert(at, t.clone());
        self.set_status(&format!("«{}» sonará a continuación", t.title));
    }

    fn request_radio(&mut self, seed: &str) {
        if seed.is_empty() {
            return;
        }
        if self.radio_loading && self.radio_seed == seed {
            return;
        }
        self.radio_seed = seed.to_string();
        self.radio_loading = true;
        let tx = self.tx.clone();
        let seed = seed.to_string();
        let spotify_track = self.queue.iter().find(|t| t.id == seed && t.is_spotify()).cloned();
        std::thread::spawn(move || {
            // The radio comes from YouTube Music; a Spotify song needs its
            // YouTube twin as the seed.
            let yt_seed = match &spotify_track {
                Some(t) => youtube_twin(t).map(|x| x.id).unwrap_or_default(),
                None => seed.clone(),
            };
            let tracks = if yt_seed.is_empty() { Vec::new() } else { ytm::radio(&yt_seed).unwrap_or_default() };
            let _ = tx.send(Event::RadioDone(seed, tracks));
        });
        self.dirty = true;
    }

    fn begin_track(&mut self, t: Track) {
        if t.is_spotify() {
            if self.spotify.snapshot().available {
                self.start_spotify(t);
                return;
            }
            // No Spotify device running: play the same song from YouTube Music.
            self.set_status("Spotify no está conectado: busco la canción en YouTube Music…");
            let tx = self.tx.clone();
            std::thread::spawn(move || {
                let twin = youtube_twin(&t);
                let _ = tx.send(Event::Substitute(t.id.clone(), twin));
            });
            return;
        }
        self.spotify_queue = false;
        if self.source == Source::Spotify {
            // Local tracks and YouTube results always play through mpv.
            self.spotify.send(SpotifyCmd::Pause);
            self.source = Source::YouTube;
            self.mpv.pause(false);
            self.yt_track = None;
        }
        self.track = Some(t.clone());
        self.pos = 0.0;
        self.dur = t.duration;
        self.playing = true;
        self.last_pos = -1.0;
        self.has_video = false;
        self.video_surface = None;
        self.mpv.load(&t.url());
        if self.video {
            self.video_at = Instant::now();
            self.find_video();
        }
        self.mpv.pause(false);
        self.db.touch_played(&t);
        self.current_saved = self.db.is_saved(&t.id);
        self.start_lyrics(t.clone());
        self.start_cover(&t);
        self.dirty = true;
        self.hiding = false;
    }

    /// Play a Spotify track on the Connect device (spotifyd) through MPRIS.
    fn start_spotify(&mut self, t: Track) {
        if self.video {
            self.set_video(false);
        }
        self.mpv.pause(true);
        if self.source != Source::Spotify {
            self.yt_track = None;
        }
        self.source = Source::Spotify;
        self.spotify.send(SpotifyCmd::OpenUri(t.url()));
        self.spotify_queue = true;
        self.sp_started = Instant::now();
        self.sp_was_playing = false;
        self.spotify_seen = t.id.clone();
        self.track = Some(t.clone());
        self.pos = 0.0;
        self.dur = t.duration;
        self.playing = true;
        self.drawn_pos = -1.0;
        self.db.touch_played(&t);
        self.current_saved = self.db.is_saved(&t.id);
        self.start_lyrics(t.clone());
        self.start_cover(&t);
        self.dirty = true;
        self.hiding = false;
    }

    fn start_lyrics(&mut self, t: Track) {
        self.lyrics.clear();
        self.lyrics_synced = false;
        self.lyrics_state = 1;
        self.lyric_idx = 0;
        self.lyric_scroll = 0.0;
        if let Some((synced, plain)) = self.db.lyrics(&t.id) {
            if !synced.is_empty() {
                self.lyrics = lyrics::parse_lrc(&synced);
                self.lyrics_synced = true;
                self.lyrics_state = 2;
                return;
            }
            if !plain.is_empty() {
                self.lyrics = lyrics::parse_plain(&plain);
                self.lyrics_state = 2;
                return;
            }
        }
        let tx = self.tx.clone();
        std::thread::spawn(move || {
            let id = t.id.clone();
            match lyrics::fetch(&t) {
                Ok(l) => {
                    // Cache only timed lyrics; plain text is retried next
                    // time in case a synced version shows up (or LRCLIB was down).
                    if l.synced {
                        if let Ok(db) = Db::open() {
                            let _ = db.save_lyrics(&id, l.synced_text.as_deref(), &l.plain_text);
                        }
                    }
                    let _ = tx.send(Event::LyricsDone(id, l.lines, l.synced));
                }
                Err(_) => {
                    let _ = tx.send(Event::LyricsDone(id, Vec::new(), false));
                }
            }
        });
    }

    fn start_cover(&mut self, t: &Track) {
        self.cover_key = t.id.clone();
        self.cover = None;
        self.cover_blur = None;
        let tx = self.tx.clone();
        let id = t.id.clone();
        let art = t.art.clone();
        // Download and decode off the UI thread: a 1280 px JPEG takes long
        // enough to drop frames.
        std::thread::spawn(move || {
            let path = paths::art_path(&id);
            let path = if path.exists() { Ok(path) } else { yt::download_cover(&id, &art) };
            if let Ok(path) = path {
                let _ = tx.send(Event::CoverDone(id, decode_pixels(&path, 640)));
            }
        });
    }

    fn apply_cover(&mut self, id: &str, px: Option<Pixels>) {
        if self.track.as_ref().map(|t| t.id.as_str()) != Some(id) {
            return;
        }
        if let Some(mut surface) = px.as_ref().and_then(to_surface) {
            self.accent = theme::accent_from_surface(&mut surface);
            // The glass backdrop only needs a 12 px version; make it once
            // instead of shrinking the full cover every frame.
            self.cover_blur = cairo::ImageSurface::create(cairo::Format::ARgb32, 12, 12)
                .ok()
                .and_then(|small| {
                    let cr = cairo::Context::new(&small).ok()?;
                    cr.scale(12.0 / surface.width() as f64, 12.0 / surface.height() as f64);
                    cr.set_source_surface(&surface, 0.0, 0.0).ok()?;
                    cr.paint().ok()?;
                    drop(cr);
                    Some(small)
                });
            self.cover = Some(surface);
            self.dirty = true;
        }
    }

    /// Thumbnail for a list row; queues a download the first time.
    pub fn thumb(&self, url: &str) -> Option<cairo::ImageSurface> {
        if url.is_empty() {
            return None;
        }
        let small = ytm::small_art(url);
        let mut map = self.thumbs.borrow_mut();
        match map.get(&small) {
            Some(s) => s.clone(),
            None => {
                map.insert(small.clone(), None);
                let _ = self.thumb_tx.send(small);
                None
            }
        }
    }

    // -------------------------------------------------------------- search
    fn open_search(&mut self) {
        if !self.expanded {
            self.expand();
        }
        self.tab = Tab::Search;
        if self.detail_here() {
            return;
        }
        self.entry_mode = EntryMode::Search;
        self.entry.set_placeholder_text(Some("Canciones, artistas, álbumes o un enlace de lista…"));
        if self.entry.text().as_str() != self.search_query {
            self.entry.set_text(&self.search_query);
        }
        self.entry_shown = false; // let sync_entry show and focus it
        self.load_home();
        self.dirty = true;
    }

    fn load_home(&mut self) {
        if self.home_state == 1 || self.home_state == 2 {
            return;
        }
        self.home_state = 1;
        let tx = self.tx.clone();
        std::thread::spawn(move || match ytm::home() {
            Ok((colls, tracks)) => {
                let _ = tx.send(Event::HomeDone(colls, tracks));
            }
            Err(_) => {
                let _ = tx.send(Event::HomeDone(Vec::new(), Vec::new()));
            }
        });
    }

    fn new_playlist(&mut self) {
        self.tab = Tab::Library;
        self.detail = None;
        self.entry_mode = EntryMode::NewPlaylist;
        self.entry.set_placeholder_text(Some("Nombre de la nueva lista y Enter…"));
        self.entry.set_text("");
        self.entry_shown = false;
        self.dirty = true;
    }

    fn close_entry(&mut self) {
        if self.entry_mode == EntryMode::NewPlaylist {
            self.entry.set_text("");
        }
        self.entry_mode = EntryMode::Hidden;
        self.entry.set_visible(false);
        self.entry_shown = false;
        self.dirty = true;
    }

    fn sync_entry(&mut self) {
        let want = self.visible
            && self.expanded
            && self.anim > 0.95
            && self.entry_mode != EntryMode::Hidden
            && !(self.tab == Tab::Search && self.detail_here())
            && (self.tab == Tab::Search || self.entry_mode == EntryMode::NewPlaylist);
        if want {
            let l = self.layout();
            let k = self.ui_scale;
            let (ox, oy) = self.origin();
            self.entry.set_margin_start(((l.entry.x + ox) * k).round() as i32);
            self.entry.set_margin_top(((l.entry.y + oy) * k).round() as i32);
            self.entry
                .set_size_request((l.entry.w * k).round() as i32, (l.entry.h * k).round() as i32);
        }
        if want != self.entry_shown {
            self.entry_shown = want;
            self.entry.set_visible(want);
            if want {
                self.entry.grab_focus();
                self.entry.set_position(-1);
            }
            self.dirty = true;
        }
    }

    fn run_search(&mut self) {
        let q = self.search_query.trim().to_string();
        if q.is_empty() || q == self.searched {
            return;
        }
        if let Some(list) = ytm::playlist_from_url(&q) {
            self.searched = q;
            self.open_collection(Collection {
                id: format!("VL{list}"),
                title: "Lista de YouTube".into(),
                subtitle: String::new(),
                art: String::new(),
                kind: CollKind::Playlist,
            });
            return;
        }
        self.searched = q.clone();
        self.searching = true;
        self.dirty = true;
        self.spotify_search = spotify_api::configured();
        let tx = self.tx.clone();
        std::thread::spawn(move || {
            let sq = q.clone();
            let spotify = std::thread::spawn(move || {
                if spotify_api::configured() {
                    spotify_api::search(&sq)
                } else {
                    Ok(Vec::new())
                }
            });
            let (yt_tracks, colls) = match ytm::search(&q) {
                Ok(r) if !r.0.is_empty() || !r.1.is_empty() => r,
                // Fall back to plain YouTube search if YouTube Music fails.
                _ => (yt::search(&q, 20).unwrap_or_default(), Vec::new()),
            };
            let sp_tracks = match spotify.join() {
                Ok(Ok(t)) => t,
                Ok(Err(e)) => {
                    let _ = tx.send(Event::Error(format!("Spotify: {e}")));
                    Vec::new()
                }
                Err(_) => Vec::new(),
            };
            let tracks = rank::merge(&q, yt_tracks, sp_tracks);
            let _ = tx.send(Event::SearchDone(q, tracks, colls));
        });
    }

    fn create_playlist_from_entry(&mut self) {
        let name = self.entry.text().trim().to_string();
        if name.is_empty() {
            return;
        }
        match self.db.create_playlist(&name) {
            Ok(pid) => {
                if let Some(t) = self.track.clone() {
                    let _ = self.db.add_to_playlist(pid, &t);
                    self.set_status(&format!("Lista «{name}» creada con la canción actual"));
                } else {
                    self.set_status(&format!("Lista «{name}» creada"));
                }
                self.refresh_library();
            }
            Err(e) => self.set_status(&format!("No se pudo crear: {e}")),
        }
        self.close_entry();
    }

    // --------------------------------------------------------- collections
    fn open_collection(&mut self, c: Collection) {
        // Already in the library? Open the local copy instead.
        if let Some(p) = self.playlists.iter().find(|p| !p.remote.is_empty() && p.remote == c.id).cloned() {
            self.open_playlist(p);
            return;
        }
        let tab = if self.tab == Tab::Library { Tab::Library } else { Tab::Search };
        self.detail = Some(Detail {
            key: c.id.clone(),
            title: c.title.clone(),
            subtitle: c.subtitle.clone(),
            art: c.art.clone(),
            kind: c.kind,
            tracks: Vec::new(),
            loading: true,
            local: None,
            remote: if c.kind == CollKind::Mix { String::new() } else { c.id.clone() },
            tab,
        });
        self.tab = tab;
        self.list_scroll = 0.0;
        let tx = self.tx.clone();
        let id = c.id.clone();
        std::thread::spawn(move || {
            let (title, tracks) = ytm::collection(&id).unwrap_or_default();
            let _ = tx.send(Event::CollectionDone(id, title, tracks));
        });
        self.dirty = true;
    }

    fn open_playlist(&mut self, p: Playlist) {
        let tracks = self.db.playlist_tracks(p.id);
        let key = format!("local:{}", p.id);
        let remote = p.remote.clone();
        let refresh = !remote.is_empty();
        self.detail = Some(Detail {
            key: key.clone(),
            title: p.name.clone(),
            subtitle: if remote.is_empty() {
                "Tu lista".into()
            } else if remote.starts_with("MPRE") {
                "Álbum guardado".into()
            } else {
                "Lista de YouTube Music".into()
            },
            art: if p.art.is_empty() {
                tracks.first().map(|t| t.art.clone()).unwrap_or_default()
            } else {
                p.art.clone()
            },
            kind: CollKind::Playlist,
            loading: refresh,
            tracks,
            local: Some(p.id),
            remote: remote.clone(),
            tab: Tab::Library,
        });
        self.tab = Tab::Library;
        self.close_entry();
        self.list_scroll = 0.0;
        if refresh {
            let tx = self.tx.clone();
            std::thread::spawn(move || {
                let list_id = remote.strip_prefix("VL").unwrap_or(&remote).to_string();
                // Public lists come straight from YouTube Music; private ones
                // (and "Liked music") need the browser session via yt-dlp.
                let mut tracks = Vec::new();
                if list_id != "LM" {
                    tracks = ytm::collection(&remote).map(|r| r.1).unwrap_or_default();
                }
                if tracks.is_empty() && remote.starts_with("VL") {
                    let url = format!("https://music.youtube.com/playlist?list={list_id}");
                    match yt::playlist(&url) {
                        Ok(t) => tracks = t,
                        Err(e) => {
                            let _ = tx.send(Event::Error(format!("No se pudo leer la lista: {e}")));
                        }
                    }
                }
                let _ = tx.send(Event::CollectionDone(key, String::new(), tracks));
            });
        }
        self.dirty = true;
    }

    fn close_detail(&mut self) {
        self.detail = None;
        self.list_scroll = 0.0;
        if self.tab == Tab::Search {
            self.open_search();
        }
        self.dirty = true;
    }

    fn save_detail(&mut self) {
        let Some(d) = self.detail.clone() else { return };
        if d.tracks.is_empty() {
            return;
        }
        let art = if d.art.is_empty() { d.tracks[0].art.clone() } else { d.art.clone() };
        let pid = if d.remote.is_empty() {
            self.db.create_playlist(&d.title)
        } else {
            self.db.upsert_remote_playlist(&d.title, &d.remote, &ytm::big_art(&art))
        };
        match pid {
            Ok(pid) => {
                let _ = self.db.set_playlist_tracks(pid, &d.tracks);
                if let Some(cur) = self.detail.as_mut() {
                    cur.local = Some(pid);
                }
                self.refresh_library();
                self.set_status(&format!("«{}» guardada en tu biblioteca", d.title));
            }
            Err(e) => self.set_status(&format!("No se pudo guardar: {e}")),
        }
    }

    /// Run `marmota-music setup` in a floating terminal.
    fn open_setup(&mut self) {
        let omarchy = "omarchy-launch-floating-terminal-with-presentation";
        let has_omarchy = std::process::Command::new("sh")
            .args(["-c", &format!("command -v {omarchy}")])
            .stdout(std::process::Stdio::null())
            .status()
            .is_ok_and(|s| s.success());
        let spawned = if has_omarchy {
            std::process::Command::new(omarchy).arg("marmota-music setup").spawn()
        } else {
            std::process::Command::new("xdg-terminal-exec")
                .args(["marmota-music", "setup"])
                .env("MM_SETUP_WAIT", "1")
                .spawn()
        };
        match spawned {
            Ok(_) => {
                self.set_status("Sigue los pasos en la terminal");
                self.collapse();
            }
            Err(e) => self.set_status(&format!("No pude abrir la terminal: {e}")),
        }
    }

    fn import_account(&mut self) {
        if self.importing {
            return;
        }
        self.importing = true;
        self.set_status("Importando tus listas de YouTube Music…");
        let tx = self.tx.clone();
        std::thread::spawn(move || {
            let result = yt::account_playlists().map_err(|e| e.to_string()).and_then(|lists| {
                let db = Db::open().map_err(|e| e.to_string())?;
                for (id, title) in &lists {
                    let _ = db.upsert_remote_playlist(title, &format!("VL{id}"), "");
                }
                Ok(lists.len())
            });
            let _ = tx.send(Event::ImportDone(result));
        });
    }

    fn list_extent(&self) -> f64 {
        let l = self.layout();
        let bottom = self
            .rows(l.content)
            .iter()
            .map(|(r, _)| r.y + r.h)
            .fold(l.content.y, f64::max);
        (bottom + self.list_scroll - l.content.y - l.content.h + 12.0).max(0.0)
    }

    fn scroll_list(&mut self, dy: f64) {
        let max = self.list_extent();
        self.list_scroll = (self.list_scroll + dy * 52.0).clamp(0.0, max);
        self.dirty = true;
    }

    fn clamp_scroll(&mut self) {
        let max = self.list_extent();
        self.list_scroll = self.list_scroll.clamp(0.0, max);
    }

    fn refresh_library(&mut self) {
        self.accounts_ok = crate::setup::accounts_configured();
        self.library = self.db.library();
        self.history = self.db.history(25);
        self.playlists = self.db.playlists();
        // A mix per recent artist, seeded with their latest song.
        let mut mixes: Vec<Collection> = Vec::new();
        for t in &self.history {
            if mixes.len() >= 4 {
                break;
            }
            let artist = lyrics::main_artist(&t.artist);
            if t.source != "youtube" || mixes.iter().any(|m| m.subtitle.ends_with(&artist)) {
                continue;
            }
            mixes.push(Collection {
                id: format!("RDAMVM{}", t.id),
                title: format!("Mix de {}", if artist.is_empty() { &t.title } else { &artist }),
                subtitle: format!("A partir de «{}» · {}", t.title, artist),
                art: t.art.clone(),
                kind: CollKind::Mix,
            });
        }
        self.mixes = mixes;
        self.dirty = true;
    }

    // --------------------------------------------------------------- events
    fn handle_event(&mut self, ev: Event) {
        match ev {
            Event::SearchDone(q, tracks, colls) => {
                if q == self.searched {
                    self.results = tracks;
                    self.colls = colls;
                    self.searching = false;
                    if self.tab == Tab::Search && !self.detail_here() {
                        self.list_scroll = 0.0;
                    }
                    self.dirty = true;
                }
            }
            Event::HomeDone(colls, tracks) => {
                self.home_state = if colls.is_empty() && tracks.is_empty() { 3 } else { 2 };
                self.home_colls = colls.into_iter().take(12).collect();
                self.home_tracks = tracks.into_iter().take(20).collect();
                self.dirty = true;
            }
            Event::CollectionDone(key, title, tracks) => {
                if let Some(pid) = key.strip_prefix("local:").and_then(|p| p.parse::<i64>().ok()) {
                    if !tracks.is_empty() {
                        let _ = self.db.set_playlist_tracks(pid, &tracks);
                        if let Some(p) = self.playlists.iter().find(|p| p.id == pid).cloned() {
                            if p.art.is_empty() && !p.remote.is_empty() {
                                let _ = self.db.upsert_remote_playlist(&p.name, &p.remote, &ytm::big_art(&tracks[0].art));
                            }
                        }
                        self.refresh_library();
                    }
                }
                if let Some(d) = self.detail.as_mut().filter(|d| d.key == key) {
                    d.loading = false;
                    if !tracks.is_empty() || d.tracks.is_empty() {
                        d.tracks = tracks;
                    }
                    if !title.is_empty() && (d.title.is_empty() || d.title == "Lista de YouTube") {
                        d.title = title;
                    }
                    if d.art.is_empty() {
                        d.art = d.tracks.first().map(|t| t.art.clone()).unwrap_or_default();
                    }
                    if d.subtitle.is_empty() {
                        d.subtitle = format!("{} canciones", d.tracks.len());
                    }
                    self.dirty = true;
                }
            }
            Event::RadioDone(seed, tracks) => {
                if seed != self.radio_seed {
                    return;
                }
                self.radio_loading = false;
                let before = self.queue.len();
                for t in tracks {
                    if !self.queue.iter().any(|x| x.id == t.id) {
                        self.queue.push(t);
                    }
                }
                if self.radio_waiting {
                    self.radio_waiting = false;
                    if self.queue.len() > before {
                        self.play_index(before);
                    } else {
                        self.set_status("No hay más canciones en la cola");
                    }
                }
                self.dirty = true;
            }
            Event::ThumbDone(url, px) => {
                let surf = px.as_ref().and_then(to_surface);
                self.thumbs.borrow_mut().insert(url, surf);
                self.dirty = true;
            }
            Event::ImportDone(result) => {
                self.importing = false;
                match result {
                    Ok(n) => {
                        self.refresh_library();
                        self.set_status(&format!("{n} listas importadas de YouTube Music"));
                    }
                    Err(e) => self.set_status(&format!("No se pudo importar: {e}")),
                }
            }
            Event::Substitute(sp_id, twin) => {
                let current = self.queue.get(self.index).is_some_and(|t| t.id == sp_id);
                match twin {
                    Some(yt) if current => {
                        self.queue[self.index] = yt.clone();
                        self.begin_track(yt);
                    }
                    None if current => self.set_status("No encontré esa canción en YouTube Music"),
                    _ => {}
                }
            }
            Event::VideoFound(id, vid) => {
                if self.video && self.track.as_ref().is_some_and(|t| t.id == id) {
                    self.video_at = Instant::now();
                    let url = vid.map(|v| format!("https://www.youtube.com/watch?v={v}"));
                    self.mpv.video(true, url);
                    self.set_status("Cargando vídeo…");
                }
            }
            Event::LyricsDone(id, lines, synced) => {
                if self.track.as_ref().map(|t| t.id.as_str()) == Some(id.as_str()) {
                    self.lyrics = lines;
                    self.lyrics_synced = synced;
                    self.lyrics_state = if self.lyrics.is_empty() { 3 } else { 2 };
                    self.lyric_idx = 0;
                    self.lyric_scroll = 0.0;
                    self.dirty = true;
                }
            }
            Event::CoverDone(id, px) => {
                self.apply_cover(&id, px);
            }
            Event::Ended => {
                if self.repeat == Repeat::One {
                    if let Some(t) = self.track.clone() {
                        self.pos = 0.0;
                        self.mpv.load(&t.url());
                    }
                } else {
                    self.advance(true);
                }
            }
            Event::Cmd(cmd) => self.handle_cmd(cmd),
            Event::Error(msg) => self.set_status(&msg),
        }
    }

    fn handle_cmd(&mut self, cmd: Cmd) {
        match cmd {
            Cmd::Toggle => {
                if self.visible {
                    self.hide();
                } else {
                    self.show();
                }
            }
            Cmd::Show => self.show(),
            Cmd::Collapse => self.collapse(),
            Cmd::Hide => self.hide(),
            Cmd::PlayPause => self.toggle_play(),
            Cmd::Next => self.advance(false),
            Cmd::Prev => self.prev(),
            Cmd::Seek { pos } => self.seek_to(pos),
            Cmd::Volume { value } => self.set_volume(value),
            Cmd::Play { id, title, artist } => self.play_id(&id, title, artist),
            Cmd::Search { query } => {
                self.show();
                self.detail = None;
                if let Some(q) = query {
                    self.search_query = q;
                    self.open_search();
                    self.searched.clear();
                    self.run_search();
                } else {
                    self.open_search();
                }
            }
            Cmd::Quit => {
                let _ = std::fs::remove_file(paths::socket());
                self.app.quit();
            }
            Cmd::ToggleVideo => self.set_video(!self.video),
            Cmd::Click { x, y } => {
                let (ox, oy) = self.origin();
                self.on_press((x + ox) * self.ui_scale, (y + oy) * self.ui_scale)
            }
            Cmd::Status => {}
            Cmd::Refresh => {
                self.refresh_library();
                self.spotify_search = spotify_api::configured();
                self.searched.clear();
                self.set_status("Cuentas actualizadas");
            }
        }
    }

    fn play_id(&mut self, id: &str, title: Option<String>, artist: Option<String>) {
        let find = |list: &Vec<Track>| list.iter().find(|t| t.id == id).cloned();
        let found = find(&self.results)
            .or_else(|| find(&self.library))
            .or_else(|| find(&self.history))
            .or_else(|| find(&self.queue));
        let track = found.unwrap_or_else(|| Track {
            id: id.to_string(),
            title: title.unwrap_or_else(|| "YouTube".into()),
            artist: artist.unwrap_or_default(),
            album: String::new(),
            duration: 0.0,
            art: format!("https://i.ytimg.com/vi/{id}/maxresdefault.jpg"),
            source: "youtube".into(),
            ..Default::default()
        });
        self.show();
        self.play_single(track);
    }

    // ------------------------------------------------------------- window
    fn start_anim(&mut self, to: f64) {
        if (self.anim - to).abs() < 0.001 {
            return;
        }
        self.anim_from = self.anim;
        self.anim_to = to;
        self.anim_t0 = Instant::now();
        self.animating = true;
        self.dirty = true;
    }

    fn sync_size(&mut self) {
        let k = self.ui_scale;
        self.window.set_default_size(
            (EXPANDED_W * k).round() as i32,
            (EXPANDED_H * k).round() as i32,
        );
        self.sync_input_region();
    }

    /// Focused output as reported by Hyprland: connector, logical origin and
    /// logical height.
    fn focused_output() -> Option<(String, f64, f64, f64)> {
        let out = std::process::Command::new("hyprctl")
            .args(["-j", "monitors"])
            .output()
            .ok()?;
        let list = serde_json::from_slice::<serde_json::Value>(&out.stdout).ok()?;
        let mon = list.as_array()?.iter().find(|m| {
            m.get("focused").and_then(|f| f.as_bool()) == Some(true)
        })?;
        let name = mon.get("name")?.as_str()?.to_string();
        let scale = mon
            .get("scale")
            .and_then(|v| v.as_f64())
            .filter(|s| *s > 0.0)
            .unwrap_or(1.0);
        let x = mon.get("x").and_then(|v| v.as_f64()).unwrap_or(0.0) / scale;
        let y = mon.get("y").and_then(|v| v.as_f64()).unwrap_or(0.0) / scale;
        let h = mon.get("height").and_then(|v| v.as_f64()).unwrap_or(0.0) / scale;
        Some((name, x, y, h))
    }

    /// Pick the GDK monitor that matches the focused Hyprland output.
    fn gdk_monitor_named(x: f64, y: f64) -> Option<gtk::gdk::Monitor> {
        let display = gtk::gdk::Display::default()?;
        let list = display.monitors();
        for i in 0..list.n_items() {
            if let Some(mon) = list.item(i).and_then(|o| o.downcast::<gtk::gdk::Monitor>().ok()) {
                let g = mon.geometry();
                if (g.x() as f64 - x).abs() < 2.0 && (g.y() as f64 - y).abs() < 2.0 {
                    return Some(mon);
                }
            }
        }
        None
    }

    /// Point the layer surface at the focused output and scale it against that
    /// output's logical height, so the notch neither swamps a 1080p laptop
    /// panel nor looks tiny on a 4K one.
    fn place(&mut self) {
        let focused = Self::focused_output();
        let mut k = 1.0;
        if let Some((_name, x, y, logical)) = focused {
            if logical > 100.0 {
                k = (logical / 1080.0).clamp(0.70, 1.0);
            }
            if let Some(mon) = Self::gdk_monitor_named(x, y) {
                let current = self
                    .window
                    .surface()
                    .and_then(|s| gtk::gdk::Display::default().map(|d| (d, s)))
                    .and_then(|(d, s)| d.monitor_at_surface(&s));
                let same = current.as_ref() == Some(&mon);
                if !same {
                    // The surface keeps the output it first mapped on; drop it
                    // and re-map on the focused one.
                    if self.window.is_realized() {
                        gtk::prelude::WidgetExt::unrealize(&self.window);
                    }
                    self.window.set_monitor(Some(&mon));
                }
            }
        }
        self.ui_scale = k;
        self.sync_size();
    }

    fn show(&mut self) {
        // If nothing is playing here but Spotify is, opening the notch should
        // show what the user is actually listening to.
        if self.source == Source::YouTube && !self.playing {
            let sp = self.spotify.snapshot();
            if sp.available && sp.playing {
                self.switch_source(Source::Spotify);
            }
        }
        self.visible = true;
        self.hiding = false;
        self.place();
        self.window.present();
        // A (re)mapped surface starts with a full input region.
        self.input_rect = (-1, -1, -1, -1);
        self.sync_input_region();
        self.expanded = true;
        self.dirty = true;
        if self.anim < 0.999 {
            self.start_anim(1.0);
        }
    }

    fn expand(&mut self) {
        if !self.visible {
            self.show();
            return;
        }
        self.expanded = true;
        self.start_anim(1.0);
    }

    fn collapse(&mut self) {
        self.expanded = false;
        if self.entry_mode == EntryMode::NewPlaylist {
            self.close_entry();
        }
        self.start_anim(0.0);
    }

    fn hide(&mut self) {
        if !self.visible || self.hiding {
            return;
        }
        self.hiding = true;
        self.expanded = false;
        if self.entry_mode == EntryMode::NewPlaylist {
            self.close_entry();
        }
        if self.anim <= 0.01 {
            self.window.set_visible(false);
            self.visible = false;
            self.hiding = false;
        } else {
            self.start_anim(0.0);
        }
    }

    fn set_status(&mut self, msg: &str) {
        self.status = msg.to_string();
        self.status_at = Instant::now();
        self.dirty = true;
    }

    fn publish(&mut self) {
        if let Ok(mut s) = self.shared.lock() {
            s.playing = self.playing;
            s.pos = self.pos;
            s.duration = self.dur;
            s.volume = self.volume;
            s.expanded = self.expanded;
            s.queue_len = self.queue.len();
            if let Some(t) = &self.track {
                s.title = t.title.clone();
                s.artist = t.artist.clone();
                s.album = t.album.clone();
                s.art = t.art.clone();
                s.source = t.source.clone();
            } else {
                s.title.clear();
                s.artist.clear();
                s.album.clear();
                s.art.clear();
                s.source.clear();
            }
        }
    }

    /// Copy the newest video frame into a Cairo surface.
    fn pull_video_frame(&mut self) {
        let Ok(frame) = self.mpv.frame.lock() else { return };
        let serial = frame.serial.load(std::sync::atomic::Ordering::Relaxed);
        if serial == self.video_serial {
            return;
        }
        self.video_serial = serial;
        if self.video_surface.is_none() {
            self.video_surface =
                cairo::ImageSurface::create(cairo::Format::Rgb24, mpv::VIDEO_W, mpv::VIDEO_H).ok();
        }
        let Some(surface) = self.video_surface.as_mut() else { return };
        let stride = surface.stride() as usize;
        let src_stride = (mpv::VIDEO_W * 4) as usize;
        surface.flush();
        if let Ok(mut data) = surface.data() {
            if stride == src_stride && data.len() >= frame.data.len() {
                data[..frame.data.len()].copy_from_slice(&frame.data);
            } else {
                for row in 0..mpv::VIDEO_H as usize {
                    let s = &frame.data[row * src_stride..(row + 1) * src_stride];
                    data[row * stride..row * stride + src_stride].copy_from_slice(s);
                }
            }
        }
        surface.mark_dirty();
        self.dirty = true;
    }

    fn tick(&mut self) {
        while let Ok(ev) = self.rx.try_recv() {
            self.handle_event(ev);
        }

        let (mut pos, mut dur, mut playing, has_video) = {
            let p = self.playback.lock().unwrap();
            (p.pos, p.dur, p.playing, p.has_video)
        };
        if self.source == Source::Spotify {
            let sp = self.spotify.snapshot();
            if !sp.available {
                self.switch_source(Source::YouTube);
            } else {
                // Our queue drives Spotify: when its song finishes, move on.
                let settled = self.sp_started.elapsed() > Duration::from_secs(5);
                let near_end = self.dur > 0.0 && self.pos >= self.dur - 3.0;
                let ended = self.spotify_queue
                    && settled
                    && near_end
                    && ((self.sp_was_playing && !sp.playing)
                        || spotify_key(&sp.track_id) != self.spotify_seen);
                self.sp_was_playing = sp.playing;
                if ended {
                    self.advance(true);
                    return;
                }
                pos = sp.pos;
                dur = sp.dur;
                playing = sp.playing;
                self.sync_spotify(&sp);
                if self.dragging != Some(Drag::Volume) && sp.volume > 0.0 {
                    self.volume = (sp.volume * 100.0).clamp(0.0, 100.0);
                }
            }
        }
        if has_video != self.has_video {
            self.has_video = has_video;
            self.dirty = true;
            if has_video && self.video {
                self.set_status("Reproduciendo vídeo");
            }
        }
        if self.video && self.has_video {
            self.pull_video_frame();
        }
        if (pos - self.pos).abs() > 0.001 {
            self.last_pos = self.pos;
            self.pos = pos;
            // Repaint only when the progress bar visibly moves (~1 px), not on
            // every 16 ms tick while a song plays.
            let step = if self.dur > 0.0 {
                (self.dur / 300.0).max(0.10)
            } else {
                0.50
            };
            if !self.playing || (pos - self.drawn_pos).abs() >= step {
                self.drawn_pos = pos;
                self.dirty = true;
            }
        }
        if dur > 0.0 {
            self.dur = dur;
        } else if self.dur <= 0.0 {
            if let Some(t) = &self.track {
                self.dur = t.duration;
            }
        }
        if playing != self.playing {
            self.playing = playing;
            self.dirty = true;
        }

        // Keep something queued after the current song (autoplay).
        if self.repeat == Repeat::Off
            && !self.radio_loading
            && self.index + 1 >= self.queue.len()
        {
            if let Some(id) = self.track.as_ref().map(|t| t.id.clone()) {
                if self.radio_seed != id {
                    self.request_radio(&id);
                }
            }
        }

        // Search as you type.
        if let Some(at) = self.search_edit {
            if at.elapsed() > Duration::from_millis(550) {
                self.search_edit = None;
                if self.search_query.trim().chars().count() >= 2 {
                    self.run_search();
                } else if self.search_query.trim().is_empty() {
                    self.results.clear();
                    self.colls.clear();
                    self.searched.clear();
                    self.dirty = true;
                }
            }
        }

        if self.animating {
            let t = (self.anim_t0.elapsed().as_secs_f64() * 1000.0 / ANIM_MS).min(1.0);
            let e = 1.0 - (1.0 - t).powi(3);
            self.anim = self.anim_from + (self.anim_to - self.anim_from) * e;
            if t >= 1.0 {
                self.anim = self.anim_to;
                self.animating = false;
                if self.hiding {
                    self.window.set_visible(false);
                    self.visible = false;
                    self.hiding = false;
                }
            }
            self.dirty = true;
        }

        let w = COLLAPSED_W + (EXPANDED_W - COLLAPSED_W) * self.anim;
        let h = COLLAPSED_H + (EXPANDED_H - COLLAPSED_H) * self.anim;
        if (w - self.width).abs() > 0.5 || (h - self.height).abs() > 0.5 {
            self.width = w;
            self.height = h;
            self.sync_size();
            self.dirty = true;
        }

        if self.video && !self.has_video && self.video_at.elapsed().as_millis() > 12000 {
            self.video = false;
            self.video_surface = None;
            self.mpv.video(false, None);
            self.set_status("Esta canción no tiene vídeo");
        }

        if self.lyrics_state == 2 && !self.lyrics.is_empty() && self.lyrics_synced {
            let idx = self
                .lyrics
                .iter()
                .rposition(|l| l.t <= self.pos + 0.12)
                .unwrap_or(0);
            if idx != self.lyric_idx {
                self.lyric_idx = idx;
                self.dirty = true;
            }
            let lr = self.layout().lyrics;
            if lr.h > 0.0 && self.lyric_manual.elapsed().as_secs() > 4 {
                // Fixed-length tween (~0.3 s) to the current line. The old
                // exponential approach took ~1.4 s per line, so the notch was
                // repainting most of the time a song played.
                let target = 12.0 + idx as f64 * 46.0 + 23.0 - lr.h / 2.0;
                if (target - self.lyric_to).abs() > 0.5 {
                    self.lyric_from = self.lyric_scroll;
                    self.lyric_to = target;
                    self.lyric_t0 = Instant::now();
                }
                if (self.lyric_scroll - self.lyric_to).abs() > 0.01 {
                    let t = (self.lyric_t0.elapsed().as_secs_f64() / 0.32).min(1.0);
                    let e = 1.0 - (1.0 - t).powi(3);
                    self.lyric_scroll = self.lyric_from + (self.lyric_to - self.lyric_from) * e;
                    self.dirty = true;
                }
            }
        } else if self.lyrics_state == 2 && !self.lyrics_synced {
            let n = self.lyrics.len().max(1) as f64;
            if self.dur > 0.0 {
                self.lyric_idx =
                    ((self.pos / self.dur) * n).floor().clamp(0.0, n - 1.0) as usize;
            }
        }

        self.sync_entry();
        self.publish();

        self.frame = self.frame.wrapping_add(1);
        if self.visible
            && (self.animating || self.dirty || self.searching || self.dragging.is_some())
        {
            // Smooth lyric scrolling runs at half the tick rate (30 fps);
            // expand/collapse and drags keep every frame.
            let smooth = !self.animating && self.dragging.is_none() && !self.searching;
            if !smooth || self.frame % 2 == 0 {
                self.area.queue_draw();
                if !self.animating {
                    self.dirty = false;
                }
            }
        }
    }
}

fn random_index(n: usize) -> usize {
    let seed = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.subsec_nanos() as usize ^ (d.as_secs() as usize).rotate_left(17))
        .unwrap_or(0);
    // xorshift to spread consecutive calls.
    let mut x = seed | 1;
    x ^= x << 13;
    x ^= x >> 7;
    x ^= x << 17;
    x % n.max(1)
}

fn shuffle_vec<T>(v: &mut [T]) {
    let mut x = random_index(usize::MAX) | 1;
    for i in (1..v.len()).rev() {
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        v.swap(i, x % (i + 1));
    }
}

/// Decode an image file into premultiplied BGRA, downsized to fit `max`.
/// Runs on worker threads.
fn decode_pixels(path: &std::path::Path, max: u32) -> Option<Pixels> {
    let reader = image::ImageReader::open(path).ok()?.with_guessed_format().ok()?;
    let mut img = reader.decode().ok()?;
    if max > 0 && (img.width() > max || img.height() > max) {
        img = img.thumbnail(max, max);
    }
    let img = img.to_rgba8();
    let (w, h) = img.dimensions();
    if w == 0 || h == 0 {
        return None;
    }
    let mut data = Vec::with_capacity((w * h * 4) as usize);
    for p in img.pixels() {
        let a = p[3] as u32;
        data.push((p[2] as u32 * a / 255) as u8);
        data.push((p[1] as u32 * a / 255) as u8);
        data.push((p[0] as u32 * a / 255) as u8);
        data.push(p[3]);
    }
    Some(Pixels { w, h, data })
}

fn to_surface(px: &Pixels) -> Option<cairo::ImageSurface> {
    let mut surface =
        cairo::ImageSurface::create(cairo::Format::ARgb32, px.w as i32, px.h as i32).ok()?;
    let stride = surface.stride() as usize;
    let row = (px.w * 4) as usize;
    {
        let mut data = surface.data().ok()?;
        for y in 0..px.h as usize {
            data[y * stride..y * stride + row].copy_from_slice(&px.data[y * row..(y + 1) * row]);
        }
    }
    Some(surface)
}

fn spotify_key(track_id: &str) -> String {
    let tail = track_id.trim_end_matches('/').rsplit('/').next().unwrap_or("");
    let clean: String = tail.chars().filter(|c| c.is_ascii_alphanumeric()).collect();
    if clean.is_empty() {
        "sp_unknown".into()
    } else {
        format!("sp_{clean}")
    }
}

/// The same song on YouTube Music (used to play or seed radio for Spotify
/// results when Spotify itself can't play).
fn youtube_twin(t: &Track) -> Option<Track> {
    let title = lyrics::clean_title(&t.title);
    let q = format!("{title} {}", lyrics::main_artist(&t.artist));
    let (tracks, _) = ytm::search(&q).ok()?;
    let mut candidates: Vec<Track> = tracks
        .into_iter()
        .filter(|x| lyrics::similar(&lyrics::clean_title(&x.title), &title))
        .collect();
    // Prefer the song (static cover, album audio) over a video upload.
    candidates.sort_by_key(|x| (x.video, x.quality == 1));
    candidates.into_iter().next()
}
