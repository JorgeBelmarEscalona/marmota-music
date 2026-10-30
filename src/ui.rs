use gtk4 as gtk;
use std::cell::RefCell;
use std::rc::Rc;
use std::sync::mpsc::{Receiver, Sender};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use gtk::prelude::*;
use gtk4_layer_shell::{Edge, KeyboardMode, Layer, LayerShell};

use crate::db::Db;
use crate::lyrics;
use crate::model::*;
use crate::mpv::Mpv;
use crate::paths;
use crate::spotify::{self, Cmd as SpotifyCmd, Spotify};
use crate::theme::{self, Rgb};
use crate::yt;

pub const COLLAPSED_W: f64 = 664.0;
pub const COLLAPSED_H: f64 = 74.0;
pub const EXPANDED_W: f64 = 1040.0;
pub const EXPANDED_H: f64 = 612.0;
const ANIM_MS: f64 = 320.0;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Tab {
    Lyrics,
    Library,
    Search,
    Queue,
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
    Playlist(usize),
    AddTo(i64),
    Track(usize),
    Result(usize),
    QueueTrack(usize),
    NewPlaylist,
    SaveCurrent,
    Lyric(usize),
    Close,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum RowItem {
    Header(&'static str),
    Playlist(usize),
    AddTo(i64),
    Track(usize),
    Result(usize),
    QueueTrack(usize),
    NewPlaylist,
    Lyric(usize),
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
    pub new_playlist: Rect,
    pub close: Rect,
}

impl Layout {
    pub fn hit(&self, x: f64, y: f64) -> Option<Hit> {
        if self.chevron.contains(x, y) {
            return Some(Hit::Chevron);
        }
        if self.play.contains(x, y) {
            return Some(Hit::Play);
        }
        if self.prev.contains(x, y) {
            return Some(Hit::Prev);
        }
        if self.next.contains(x, y) {
            return Some(Hit::Next);
        }
        if self.shuffle.contains(x, y) {
            return Some(Hit::Shuffle);
        }
        if self.repeat.contains(x, y) {
            return Some(Hit::Repeat);
        }
        if self.seek.contains(x, y) {
            return Some(Hit::Seek);
        }
        if self.volume.contains(x, y) {
            return Some(Hit::Volume);
        }
        if self.video.contains(x, y) {
            return Some(Hit::Video);
        }
        if self.spotify.contains(x, y) {
            return Some(Hit::Spotify);
        }
        if self.close.contains(x, y) {
            return Some(Hit::Close);
        }
        if self.save.contains(x, y) {
            return Some(Hit::SaveCurrent);
        }
        if self.new_playlist.contains(x, y) {
            return Some(Hit::NewPlaylist);
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
            RowItem::Playlist(i) => Some(Hit::Playlist(i)),
            RowItem::AddTo(id) => Some(Hit::AddTo(id)),
            RowItem::Track(i) => Some(Hit::Track(i)),
            RowItem::Result(i) => Some(Hit::Result(i)),
            RowItem::QueueTrack(i) => Some(Hit::QueueTrack(i)),
            RowItem::NewPlaylist => Some(Hit::NewPlaylist),
            RowItem::Lyric(i) => Some(Hit::Lyric(i)),
            RowItem::Header(_) => None,
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

pub struct App {
    pub app: gtk::Application,
    pub window: gtk::Window,
    pub area: gtk::DrawingArea,
    pub fixed: gtk::Fixed,
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
    pub video: bool,
    pub video_at: Instant,
    pub has_video: bool,

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
    pub list_scroll: f64,
    pub lyric_idx: usize,
    #[allow(dead_code)]
    pub lyric_moved: Instant,

    pub library: Vec<Track>,
    pub playlists: Vec<Playlist>,
    #[allow(dead_code)]
    pub queued: Vec<(usize, Track)>, // (1-based position, track) mirror for drawing

    pub search_query: String,
    pub results: Vec<Track>,
    pub searching: bool,

    pub accent: Rgb,
    pub cover: Option<cairo::ImageSurface>,
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

fn build(
    app: &gtk::Application,
    rx: Receiver<Event>,
    tx: Sender<Event>,
    shared: Arc<Mutex<Snapshot>>,
) -> anyhow::Result<()> {    paths::ensure_dirs();

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

    let fixed = gtk::Fixed::new();
    fixed.set_hexpand(true);
    fixed.set_vexpand(true);
    fixed.set_can_target(false);

    let entry = gtk::Entry::new();
    entry.add_css_class("mm-entry");
    entry.set_visible(false);
    entry.set_size_request(430, 38);
    fixed.put(&entry, 430.0, 36.0);

    let overlay = gtk::Overlay::new();
    overlay.set_child(Some(&area));
    overlay.add_overlay(&fixed);
    window.set_child(Some(&overlay));

    let playback = Arc::new(Mutex::new(Playback::default()));
    let mpv = Mpv::new(playback.clone(), tx.clone());
    let db = Db::open()?;
    let library = db.library();
    let playlists = db.playlists();

    let mut state = App {
        app: app.clone(),
        window: window.clone(),
        area: area.clone(),
        fixed: fixed.clone(),
        entry: entry.clone(),
        playback,
        shared,
        mpv,
        tx: tx.clone(),
        rx,
        db,
        track: None,
        queued: Vec::new(),
        queue: Vec::new(),
        index: 0,
        shuffle: false,
        repeat: Repeat::Off,
        volume: 70.0,
        drawn_pos: -1.0,
        frame: 0,
        video: false,
        video_at: Instant::now(),
        has_video: false,
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
        list_scroll: 0.0,
        lyric_idx: 0,
        lyric_moved: Instant::now(),
        library,
        playlists,
        search_query: String::new(),
        results: Vec::new(),
        searching: false,
        accent: theme::ACCENT_FALLBACK,
        cover: None,
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
        dirty: true,
        last_pos: -1.0,
    };
    state.width = COLLAPSED_W;
    state.height = COLLAPSED_H;

    let app_cell = Rc::new(RefCell::new(state));

    // --- draw -------------------------------------------------------------
    {
        let a = app_cell.clone();
        area.set_draw_func(move |_area, cr, _w, _h| {
            let s = a.borrow();
            s.draw(cr, s.width, s.height);
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
            if s.volume_rect().contains(s.mouse.0, s.mouse.1) {
                let target = s.volume - dy * 4.0;
                s.set_volume(target);
            } else if l.content.contains(s.mouse.0, s.mouse.1) {
                if s.tab == Tab::Lyrics {
                    s.lyric_scroll -= dy * 40.0;
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
        keys.connect_key_pressed(move |_, key, _code, mods| {
            let mut s = a.borrow_mut();
            if s.entry.has_focus() {
                if key == gtk::gdk::Key::Escape {
                    s.close_entry();
                    return glib::Propagation::Stop;
                }
                return glib::Propagation::Proceed;
            }
            match key {
                gtk::gdk::Key::Escape => {
                    if s.expanded {
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
                k if k == gtk::gdk::Key::_1 && mods.contains(gtk::gdk::ModifierType::ALT_MASK) => {
                    s.tab = Tab::Lyrics;
                    s.dirty = true;
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
                    s.search_query = e.text().to_string();
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
                    EntryMode::Search => s.run_search(),
                    EntryMode::NewPlaylist => s.create_playlist_from_entry(),
                    EntryMode::Hidden => {}
                }
            }
        });
    }

    // --- ticker -----------------------------------------------------------
    {
        let a = app_cell.clone();
        glib::timeout_add_local(Duration::from_millis(16), move || {
            let mut s = a.borrow_mut();
            s.tick();
            glib::ControlFlow::Continue
        });
    }

    let _ = tx;
    let _ = window;
    Ok(())
}

impl App {
    // ------------------------------------------------------------- geometry
    pub fn volume_rect(&self) -> Rect {
        self.layout().volume
    }

    pub fn layout(&self) -> Layout {
        if self.anim < 0.55 {
            self.collapsed_layout()
        } else {
            self.expanded_layout()
        }
    }

    pub(crate) fn collapsed_layout(&self) -> Layout {
        let w = self.width;
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

    pub(crate) fn expanded_layout(&self) -> Layout {
        let w = self.width;
        let h = self.height;
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
        l.entry = Rect::new(430.0, 84.0, 470.0, 38.0);
        l.content = Rect::new(430.0, 84.0, w - 458.0, h - 112.0);
        if self.tab == Tab::Search {
            l.content = Rect::new(430.0, 134.0, w - 458.0, h - 162.0);
        } else if self.entry_mode == EntryMode::NewPlaylist {
            l.content = Rect::new(430.0, 134.0, w - 458.0, h - 162.0);
        }
        l
    }

    // ---------------------------------------------------------------- rows
    pub fn rows(&self, content: Rect) -> Vec<(Rect, RowItem)> {
        let mut out: Vec<(Rect, RowItem)> = Vec::new();
        let off = if self.tab == Tab::Lyrics { 0.0 } else { self.list_scroll };
        match self.tab {
            Tab::Lyrics => {
                if self.lyrics.is_empty() {
                    return out;
                }
                let rowh = 46.0;
                for (i, _line) in self.lyrics.iter().enumerate() {
                    let y = content.y + 12.0 + i as f64 * rowh - self.lyric_scroll;
                    if y + rowh < content.y || y > content.y + content.h {
                        continue;
                    }
                    out.push((Rect::new(content.x, y, content.w, rowh), RowItem::Lyric(i)));
                }
            }
            Tab::Library => {
                let mut y = content.y + 4.0 - off;
                out.push((
                    Rect::new(content.x, y, content.w, 22.0),
                    RowItem::Header("LISTAS"),
                ));
                y += 26.0;
                for (i, p) in self.playlists.iter().enumerate() {
                    let r = Rect::new(content.x, y, content.w, 40.0);
                    if r.y + r.h > content.y && r.y < content.y + content.h {
                    out.push((
                        Rect::new(r.x + r.w - 38.0, r.y + 5.0, 30.0, 30.0),
                        RowItem::AddTo(p.id),
                    ));
                    out.push((r, RowItem::Playlist(i)));
                    }
                    y += 44.0;
                }
                out.push((
                    Rect::new(content.x, y, content.w, 36.0),
                    RowItem::NewPlaylist,
                ));
                y += 44.0;
                out.push((
                    Rect::new(content.x, y, content.w, 22.0),
                    RowItem::Header("GUARDADAS"),
                ));
                y += 26.0;
                for (i, _t) in self.library.iter().enumerate() {
                    if y > content.y + content.h {
                        break;
                    }
                    out.push((
                        Rect::new(content.x, y, content.w, 46.0),
                        RowItem::Track(i),
                    ));
                    y += 50.0;
                }
            }
            Tab::Search => {
                let mut y = content.y + 4.0 - off;
                if self.searching {
                    out.push((
                        Rect::new(content.x, y, content.w, 40.0),
                        RowItem::Header("Buscando…"),
                    ));
                } else if self.results.is_empty() {
                    let msg = if self.search_query.trim().is_empty() {
                        "Escribe para buscar en YouTube Music"
                    } else {
                        "Sin resultados"
                    };
                    out.push((
                        Rect::new(content.x, y, content.w, 40.0),
                        RowItem::Header(msg),
                    ));
                } else {
                    for (i, _) in self.results.iter().enumerate() {
                        if y > content.y + content.h {
                            break;
                        }
                        out.push((
                            Rect::new(content.x, y, content.w, 48.0),
                            RowItem::Result(i),
                        ));
                        y += 52.0;
                    }
                }
            }
            Tab::Queue => {
                let mut y = content.y + 4.0 - off;
                if self.queue.is_empty() {
                    out.push((
                        Rect::new(content.x, y, content.w, 40.0),
                        RowItem::Header("Nada en cola"),
                    ));
                }
                for (i, _) in self.queue.iter().enumerate() {
                    if y > content.y + content.h {
                        break;
                    }
                    out.push((
                        Rect::new(content.x, y, content.w, 46.0),
                        RowItem::QueueTrack(i),
                    ));
                    y += 50.0;
                }
            }
        }
        out
    }

    // ------------------------------------------------------------- input
    fn on_motion(&mut self, x: f64, y: f64) {
        let k = self.ui_scale.max(0.1);
        let (x, y) = (x / k, y / k);
        self.mouse = (x, y);
        let l = self.layout();
        let mut hit = l.hit(x, y);
        if hit.is_none() {
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
                        let f = ((x - l.volume.x) / l.volume.w).clamp(0.0, 1.0);
                        self.set_volume(f * 100.0);
                    }
                }
            }
        }
    }

    fn on_press(&mut self, x: f64, y: f64) {
        let k = self.ui_scale.max(0.1);
        let (x, y) = (x / k, y / k);
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
        for (r, item) in self.rows(l.content) {
            if r.contains(x, y) {
                self.act_row(item);
                return;
            }
        }
        if self.entry_mode != EntryMode::Hidden {
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
                self.dirty = true;
            }
            Hit::Repeat => {
                self.repeat = match self.repeat {
                    Repeat::Off => Repeat::All,
                    Repeat::All => Repeat::One,
                    Repeat::One => Repeat::Off,
                };
                self.dirty = true;
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
                    let f = ((x - l.volume.x) / l.volume.w).clamp(0.0, 1.0);
                    self.set_volume(f * 100.0);
                }
            }
            Hit::Tab(t) => {
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
                    let _ = self.db.save_track(&t);
                    self.refresh_library();
                    self.set_status("Guardada en tu biblioteca");
                }
            }
            Hit::NewPlaylist => self.new_playlist(),
            _ => {}
        }
    }

    fn act_row(&mut self, item: RowItem) {
        match item {
            RowItem::Playlist(i) => {
                if let Some(p) = self.playlists.get(i).cloned() {
                    let tracks = self.db.playlist_tracks(p.id);
                    if !tracks.is_empty() {
                        self.tab = Tab::Queue;
                        self.play_tracks(tracks, 0);
                    }
                }
            }
            RowItem::AddTo(pid) => {
                if let Some(t) = self.track.clone() {
                    let _ = self.db.add_to_playlist(pid, &t);
                    self.refresh_library();
                    self.set_status("Añadida a la lista");
                }
            }
            RowItem::Track(i) => {
                if self.library.get(i).is_some() {
                    let list = self.library.clone();
                    self.tab = Tab::Queue;
                    self.play_tracks(list, i);
                }
            }
            RowItem::Result(i) => {
                if self.results.get(i).is_some() {
                    let list = self.results.clone();
                    self.tab = Tab::Lyrics;
                    self.close_entry();
                    self.play_tracks(list, i);
                }
            }
            RowItem::QueueTrack(i) => {
                if let Some(t) = self.queue.get(i).cloned() {
                    self.index = i;
                    self.begin_track(t);
                }
            }
            RowItem::Lyric(i) => {
                if let Some(line) = self.lyrics.get(i) {
                    self.seek_to(line.t);
                }
            }
            RowItem::NewPlaylist => self.new_playlist(),
            RowItem::Header(_) => {}
        }
        self.dirty = true;
    }

    // ----------------------------------------------------------- playback
    fn toggle_play(&mut self) {
        if self.track.is_none() {
            self.tab = Tab::Search;
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
        self.video = on;
        self.video_at = Instant::now();
        self.mpv.video(on);
        if on {
            self.set_status("Buscando vídeo…");
            // Get the notch out of the way so the video window has room.
            self.collapse();
        } else {
            self.set_status("Modo audio");
        }
        self.dirty = true;
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
        if self.source == Source::Spotify {
            self.spotify.send(SpotifyCmd::Next);
            return;
        }
        let n = self.queue.len();
        if n == 0 {
            return;
        }
        let next = if self.shuffle && n > 1 {
            let seed = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.subsec_nanos() as usize)
                .unwrap_or(0);
            let mut i = seed % n;
            if i == self.index {
                i = (i + 1) % n;
            }
            i
        } else {
            self.index + 1
        };
        if next >= n {
            if auto && self.repeat == Repeat::Off {
                self.playing = false;
                self.mpv.pause(true);
                self.dirty = true;
                return;
            }
            self.play_index(0);
        } else {
            self.play_index(next);
        }
    }

    fn prev(&mut self) {
        if self.pos > 3.0 {
            self.seek_to(0.0);
            return;
        }
        if self.source == Source::Spotify {
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

    fn play_tracks(&mut self, list: Vec<Track>, i: usize) {
        if list.is_empty() {
            return;
        }
        self.queue = list;
        self.index = i.min(self.queue.len() - 1);
        if let Some(t) = self.queue.get(self.index).cloned() {
            self.begin_track(t);
        }
    }

    fn begin_track(&mut self, t: Track) {
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
        self.mpv.load(&t.url());
        let _ = self.db.save_track(&t);
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
                    if let Ok(db) = Db::open() {
                        let _ = db.save_lyrics(&id, l.synced_text.as_deref(), &l.plain_text);
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
        let path = paths::art_path(&t.id);
        if path.exists() {
            self.apply_cover(&t.id, &path);
            return;
        }
        let tx = self.tx.clone();
        let id = t.id.clone();
        let art = t.art.clone();
        std::thread::spawn(move || {
            if let Ok(path) = yt::download_cover(&id, &art) {
                let _ = tx.send(Event::CoverDone(id, path));
            }
        });
    }

    fn apply_cover(&mut self, id: &str, path: &std::path::Path) {
        if self.track.as_ref().map(|t| t.id.as_str()) != Some(id) {
            return;
        }
        if let Some(mut surface) = decode_surface(path) {
            self.accent = theme::accent_from_surface(&mut surface);
            self.cover = Some(surface);
            self.dirty = true;
        }
    }

    // -------------------------------------------------------------- search
    fn open_search(&mut self) {
        self.tab = Tab::Search;
        self.entry_mode = EntryMode::Search;
        self.entry.set_placeholder_text(Some("Buscar en YouTube Music…"));
        if self.entry.text().to_string() != self.search_query {
            self.entry.set_text(&self.search_query);
        }
        self.entry.set_visible(true);
        self.entry.grab_focus();
        self.entry_shown = true;
        self.dirty = true;
    }

    fn new_playlist(&mut self) {
        self.tab = Tab::Library;
        self.entry_mode = EntryMode::NewPlaylist;
        self.entry.set_placeholder_text(Some("Nombre de la lista…"));
        self.entry.set_text("");
        self.entry.set_visible(true);
        self.entry.grab_focus();
        self.entry_shown = true;
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
        let want = self.visible && self.entry_mode != EntryMode::Hidden && self.expanded;
        if want != self.entry_shown {
            self.entry_shown = want;
            self.entry.set_visible(want);
            if want {
                self.entry.grab_focus();
            }
        }
        if want {
            let l = self.layout();
            let k = self.ui_scale;
            self.fixed.move_(&self.entry, l.entry.x * k, l.entry.y * k);
            self.entry
                .set_size_request((l.entry.w * k) as i32, (l.entry.h * k) as i32);
        }
    }

    fn run_search(&mut self) {
        let q = self.search_query.trim().to_string();
        if q.is_empty() {
            return;
        }
        self.searching = true;
        self.results.clear();
        self.dirty = true;
        let tx = self.tx.clone();
        std::thread::spawn(move || match yt::search(&q, 24) {
            Ok(tracks) => {
                let _ = tx.send(Event::SearchDone(q, tracks));
            }
            Err(e) => {
                let _ = tx.send(Event::Error(format!("Búsqueda: {e}")));
                let _ = tx.send(Event::SearchDone(q, Vec::new()));
            }
        });
    }

    fn create_playlist_from_entry(&mut self) {
        let name = self.entry.text().trim().to_string();
        if name.is_empty() {
            return;
        }
        match self.db.create_playlist(&name) {
            Ok(_) => {
                self.refresh_library();
                self.set_status("Lista creada");
            }
            Err(e) => self.set_status(&format!("No se pudo crear: {e}")),
        }
        self.close_entry();
    }

    fn list_extent(&self) -> f64 {
        let l = self.layout();
        let total = match self.tab {
            Tab::Library => {
                4.0 + 26.0
                    + self.playlists.len() as f64 * 44.0
                    + 44.0
                    + 26.0
                    + self.library.len() as f64 * 50.0
            }
            Tab::Search => 4.0 + self.results.len() as f64 * 52.0,
            Tab::Queue => 4.0 + self.queue.len() as f64 * 50.0,
            Tab::Lyrics => 0.0,
        };
        (total - l.content.h).max(0.0)
    }

    fn scroll_list(&mut self, dy: f64) {
        let max = self.list_extent();
        self.list_scroll = (self.list_scroll - dy * 52.0).clamp(0.0, max);
        self.dirty = true;
    }

    fn refresh_library(&mut self) {
        self.library = self.db.library();
        self.playlists = self.db.playlists();
        self.dirty = true;
    }

    // --------------------------------------------------------------- events
    fn handle_event(&mut self, ev: Event) {
        match ev {
            Event::SearchDone(q, tracks) => {
                if q == self.search_query {
                    self.results = tracks;
                    self.searching = false;
                    self.list_scroll = 0.0;
                    self.tab = Tab::Search;
                    self.dirty = true;
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
            Event::CoverDone(id, path) => {
                self.apply_cover(&id, &path);
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
                if let Some(q) = query {
                    self.search_query = q;
                    self.open_search();
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
            Cmd::Click { x, y } => self.on_press(x, y),
            Cmd::Status => {}
        }
    }

    fn play_id(&mut self, id: &str, title: Option<String>, artist: Option<String>) {
        let find = |list: &Vec<Track>| list.iter().find(|t| t.id == id).cloned();
        let found = find(&self.results)
            .or_else(|| find(&self.library))
            .or_else(|| find(&self.queue));
        let track = found.unwrap_or_else(|| Track {
            id: id.to_string(),
            title: title.unwrap_or_else(|| "YouTube".into()),
            artist: artist.unwrap_or_default(),
            album: String::new(),
            duration: 0.0,
            art: format!("https://i.ytimg.com/vi/{id}/maxresdefault.jpg"),
            source: "youtube".into(),
        });
        self.show();
        self.queue = vec![track.clone()];
        self.index = 0;
        self.begin_track(track);
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
            (self.width * k).round() as i32,
            (self.height * k).round() as i32,
        );
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
        self.close_entry();
        self.start_anim(0.0);
    }

    fn hide(&mut self) {
        if !self.visible || self.hiding {
            return;
        }
        self.hiding = true;
        self.expanded = false;
        self.close_entry();
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
            s.playing = self.playing;
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

        if self.video && !self.has_video && self.video_at.elapsed().as_millis() > 9000 {
            self.video = false;
            self.mpv.video(false);
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
                self.lyric_manual = Instant::now() - Duration::from_secs(30);
                self.dirty = true;
            }
            let l = self.layout();
            if l.content.h > 0.0 && self.lyric_manual.elapsed().as_secs() > 3 {
                let target =
                    l.content.y + 12.0 + idx as f64 * 46.0 + 23.0 - l.content.cy();
                let delta = target - self.lyric_scroll;
                if delta.abs() > 0.5 {
                    self.lyric_scroll += delta * 0.10;
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

fn decode_surface(path: &std::path::Path) -> Option<cairo::ImageSurface> {
    let reader = image::ImageReader::open(path).ok()?.with_guessed_format().ok()?;
    let img = reader.decode().ok()?.to_rgba8();
    let (w, h) = img.dimensions();
    if w == 0 || h == 0 {
        return None;
    }
    let mut surface = cairo::ImageSurface::create(
        cairo::Format::ARgb32,
        w as i32,
        h as i32,
    )
    .ok()?;
    let stride = surface.stride() as usize;
    {
        let mut data = surface.data().ok()?;
        for y in 0..h as usize {
            for x in 0..w as usize {
                let p = img.get_pixel(x as u32, y as u32);
                let i = y * stride + x * 4;
                data[i] = p[2];
                data[i + 1] = p[1];
                data[i + 2] = p[0];
                data[i + 3] = p[3];
            }
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
