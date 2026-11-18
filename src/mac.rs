//! macOS glue: what the layer shell, Hyprland and the session give the
//! notch on Linux, done with AppKit (window level, position, Dock, click
//! through) and Carbon (global shortcut, no Accessibility permission needed).
//!
//! Everything here runs on the main thread, where GTK runs NSApp's loop.

use gtk4 as gtk;
use std::cell::{Cell, RefCell};
use std::ffi::c_void;
use std::process::Command;
use std::sync::mpsc::Sender;
use std::sync::Mutex;
use std::time::Duration;

use gtk::prelude::*;
use objc2::encode::{Encode, Encoding};
use objc2::runtime::{AnyObject, Bool};
use objc2::{class, msg_send};

use crate::model::{Cmd, Event};

/// Gap between the notch and the Dock (or the screen edge), in points.
const MARGIN: f64 = 14.0;

#[repr(C)]
#[derive(Clone, Copy, Default)]
struct Point {
    x: f64,
    y: f64,
}

#[repr(C)]
#[derive(Clone, Copy, Default)]
struct Size {
    w: f64,
    h: f64,
}

#[repr(C)]
#[derive(Clone, Copy, Default)]
struct Rect {
    origin: Point,
    size: Size,
}

unsafe impl Encode for Point {
    const ENCODING: Encoding = Encoding::Struct("CGPoint", &[f64::ENCODING, f64::ENCODING]);
}
unsafe impl Encode for Size {
    const ENCODING: Encoding = Encoding::Struct("CGSize", &[f64::ENCODING, f64::ENCODING]);
}
unsafe impl Encode for Rect {
    const ENCODING: Encoding = Encoding::Struct("CGRect", &[Point::ENCODING, Size::ENCODING]);
}

impl Rect {
    fn contains(&self, p: Point) -> bool {
        p.x >= self.origin.x
            && p.x < self.origin.x + self.size.w
            && p.y >= self.origin.y
            && p.y < self.origin.y + self.size.h
    }
}

type Id = *mut AnyObject;

fn ns_app() -> Id {
    unsafe { msg_send![class!(NSApplication), sharedApplication] }
}

fn ns_window(window: &gtk::Window) -> Option<Id> {
    let surface = window.surface()?.downcast::<gdk4_macos::MacosSurface>().ok()?;
    let ptr = surface.native() as Id;
    (!ptr.is_null()).then_some(ptr)
}

/// Run a shell command in a new Terminal.app window.
pub fn open_terminal(shell_cmd: &str) -> std::io::Result<()> {
    // AppleScript string: escape backslashes and quotes.
    let script_cmd = shell_cmd.replace('\\', "\\\\").replace('"', "\\\"");
    Command::new("osascript")
        .args(["-e", "tell application \"Terminal\" to activate"])
        .args(["-e", &format!("tell application \"Terminal\" to do script \"{script_cmd}\"")])
        .spawn()
        .map(|_| ())
}

/// Quote a path for `sh`.
pub fn sh_quote(s: &str) -> String {
    format!("'{}'", s.replace('\'', "'\\''"))
}

// ---------------------------------------------------------------- startup

/// Put Homebrew's bin directories in front of a bare PATH.
pub fn homebrew_path() {
    let current = std::env::var_os("PATH").unwrap_or_default();
    let mut dirs: Vec<std::path::PathBuf> = std::env::split_paths(&current).collect();
    for extra in ["/usr/local/bin", "/opt/homebrew/bin"] {
        let p = std::path::PathBuf::from(extra);
        if p.is_dir() && !dirs.contains(&p) {
            dirs.insert(0, p);
        }
    }
    if let Ok(joined) = std::env::join_paths(dirs) {
        std::env::set_var("PATH", joined);
    }
}

/// No Dock icon and no menu bar of its own: the notch is an accessory, like
/// a menu bar extra. (The .app bundle also says so with LSUIElement.)
pub fn init_app(tx: Sender<Event>) {
    unsafe {
        // NSApplicationActivationPolicyAccessory
        let _: Bool = msg_send![ns_app(), setActivationPolicy: 1isize];
    }
    hotkey(tx);
}

// ---------------------------------------------------------------- shortcut

/// Shortcut used when the config has no `hotkey`.
pub const DEFAULT_HOTKEY: &str = "Cmd + Shift + M";

/// The shortcut that opens the notch (config key `hotkey`).
pub fn hotkey_label() -> String {
    crate::paths::config()
        .get("hotkey")
        .and_then(|v| v.as_str())
        .map(str::to_string)
        .unwrap_or_else(|| DEFAULT_HOTKEY.into())
}

// Carbon modifier flags.
const CMD_KEY: u32 = 1 << 8;
const SHIFT_KEY: u32 = 1 << 9;
const OPTION_KEY: u32 = 1 << 11;
const CONTROL_KEY: u32 = 1 << 12;

/// Virtual key code of a letter or digit on the ANSI layout (kVK_ANSI_*).
fn ansi_key(c: char) -> Option<u32> {
    let code = match c {
        'a' => 0x00, 's' => 0x01, 'd' => 0x02, 'f' => 0x03, 'h' => 0x04, 'g' => 0x05,
        'z' => 0x06, 'x' => 0x07, 'c' => 0x08, 'v' => 0x09, 'b' => 0x0B, 'q' => 0x0C,
        'w' => 0x0D, 'e' => 0x0E, 'r' => 0x0F, 'y' => 0x10, 't' => 0x11, '1' => 0x12,
        '2' => 0x13, '3' => 0x14, '4' => 0x15, '6' => 0x16, '5' => 0x17, '9' => 0x19,
        '7' => 0x1A, '8' => 0x1C, '0' => 0x1D, 'o' => 0x1F, 'u' => 0x20, 'i' => 0x22,
        'p' => 0x23, 'l' => 0x25, 'j' => 0x26, 'k' => 0x28, 'n' => 0x2D, 'm' => 0x2E,
        _ => return None,
    };
    Some(code)
}

const F_KEYS: [u32; 20] = [
    0x7A, 0x78, 0x63, 0x76, 0x60, 0x61, 0x62, 0x64, 0x65, 0x6D, 0x67, 0x6F, 0x69, 0x6B, 0x71,
    0x6A, 0x40, 0x4F, 0x50, 0x5A,
];

/// "Cmd + Shift + M" → (Carbon modifiers, virtual key code).
fn parse_hotkey(spec: &str) -> Option<(u32, u32)> {
    let mut mods = 0;
    let mut key = None;
    for part in spec.split('+').map(|p| p.trim().to_ascii_lowercase()) {
        match part.as_str() {
            "cmd" | "command" | "⌘" | "super" | "win" => mods |= CMD_KEY,
            "shift" | "⇧" => mods |= SHIFT_KEY,
            "alt" | "option" | "opt" | "⌥" => mods |= OPTION_KEY,
            "ctrl" | "control" | "⌃" => mods |= CONTROL_KEY,
            "space" => key = Some(0x31),
            p if p.chars().count() == 1 => key = Some(ansi_key(p.chars().next()?)?),
            p if p.starts_with('f') => {
                let n: usize = p[1..].parse().ok().filter(|n| (1..=20).contains(n))?;
                key = Some(F_KEYS[n - 1]);
            }
            _ => return None,
        }
    }
    Some((mods, key?))
}

#[repr(C)]
#[derive(Clone, Copy)]
struct EventTypeSpec {
    event_class: u32,
    event_kind: u32,
}

#[repr(C)]
#[derive(Clone, Copy)]
struct EventHotKeyId {
    signature: u32,
    id: u32,
}

type EventHandler = extern "C" fn(*mut c_void, *mut c_void, *mut c_void) -> i32;

#[link(name = "Carbon", kind = "framework")]
extern "C" {
    fn GetApplicationEventTarget() -> *mut c_void;
    fn InstallEventHandler(
        target: *mut c_void,
        handler: EventHandler,
        num_types: u32,
        list: *const EventTypeSpec,
        user_data: *mut c_void,
        out_ref: *mut *mut c_void,
    ) -> i32;
    fn RegisterEventHotKey(
        key_code: u32,
        modifiers: u32,
        id: EventHotKeyId,
        target: *mut c_void,
        options: u32,
        out_ref: *mut *mut c_void,
    ) -> i32;
}

const fn four_cc(s: &[u8; 4]) -> u32 {
    (s[0] as u32) << 24 | (s[1] as u32) << 16 | (s[2] as u32) << 8 | s[3] as u32
}

static HOTKEY_TX: Mutex<Option<Sender<Event>>> = Mutex::new(None);

extern "C" fn on_hotkey(_next: *mut c_void, _event: *mut c_void, _data: *mut c_void) -> i32 {
    if let Ok(tx) = HOTKEY_TX.lock() {
        if let Some(tx) = tx.as_ref() {
            let _ = tx.send(Event::Cmd(Cmd::Toggle));
        }
    }
    0 // noErr
}

/// Global shortcut that toggles the notch, from the config key `hotkey`.
/// Carbon hot keys reach the app through NSApp's event loop (which GTK runs)
/// and, unlike an event tap, need no Accessibility permission.
fn hotkey(tx: Sender<Event>) {
    let wanted = hotkey_label();
    let Some((mods, key)) = parse_hotkey(&wanted) else {
        let _ = tx.send(Event::Error(format!("Atajo «{wanted}» no válido: revisa «hotkey» en config.json")));
        return;
    };
    if let Ok(mut slot) = HOTKEY_TX.lock() {
        *slot = Some(tx.clone());
    }
    unsafe {
        let target = GetApplicationEventTarget();
        let spec = EventTypeSpec { event_class: four_cc(b"keyb"), event_kind: 5 }; // kEventHotKeyPressed
        let mut handler = std::ptr::null_mut();
        if InstallEventHandler(target, on_hotkey, 1, &spec, std::ptr::null_mut(), &mut handler) != 0 {
            let _ = tx.send(Event::Error(format!("No pude usar el atajo {wanted}")));
            return;
        }
        let id = EventHotKeyId { signature: four_cc(b"mmus"), id: 1 };
        let mut hk = std::ptr::null_mut();
        if RegisterEventHotKey(key, mods, id, target, 0, &mut hk) != 0 {
            let _ = tx.send(Event::Error(format!(
                "El atajo {wanted} ya lo usa otro programa: cambia «hotkey» en config.json"
            )));
        }
    }
}

// ---------------------------------------------------------------- window

/// Screen under the pointer (where the notch opens), else the main one.
fn pointer_screen() -> Option<Id> {
    unsafe {
        let mouse: Point = msg_send![class!(NSEvent), mouseLocation];
        let screens: Id = msg_send![class!(NSScreen), screens];
        if !screens.is_null() {
            let n: usize = msg_send![screens, count];
            for i in 0..n {
                let s: Id = msg_send![screens, objectAtIndex: i];
                let frame: Rect = msg_send![s, frame];
                if frame.contains(mouse) {
                    return Some(s);
                }
            }
        }
        let main: Id = msg_send![class!(NSScreen), mainScreen];
        (!main.is_null()).then_some(main)
    }
}

/// Logical height (points) of the screen under the pointer.
pub fn pointer_monitor_height() -> Option<f64> {
    let screen = pointer_screen()?;
    let frame: Rect = unsafe { msg_send![screen, frame] };
    Some(frame.size.h)
}

/// Float above other windows (and full-screen apps), on every Space, out of
/// Cmd+Tab and Mission Control, with a transparent background. Cheap, so it
/// runs on every show in case GDK reset anything.
pub fn prepare(window: &gtk::Window) {
    let Some(win) = ns_window(window) else { return };
    unsafe {
        // NSStatusWindowLevel: above normal and floating windows.
        let _: () = msg_send![win, setLevel: 25isize];
        // canJoinAllSpaces | stationary | ignoresCycle | fullScreenAuxiliary
        let behavior: usize = (1 << 0) | (1 << 4) | (1 << 6) | (1 << 8);
        let _: () = msg_send![win, setCollectionBehavior: behavior];
        let _: () = msg_send![win, setHasShadow: Bool::NO];
        let _: () = msg_send![win, setOpaque: Bool::NO];
        let clear: Id = msg_send![class!(NSColor), clearColor];
        let _: () = msg_send![win, setBackgroundColor: clear];
        let _: () = msg_send![win, setHidesOnDeactivate: Bool::NO];
    }
}

/// Bring the notch forward and take the keyboard (for search and shortcuts).
pub fn activate() {
    unsafe {
        let app = ns_app();
        let _: () = msg_send![app, unhide: std::ptr::null::<AnyObject>()];
        let _: () = msg_send![app, activateIgnoringOtherApps: Bool::YES];
    }
}

/// The notch is gone: give the focus back to the app the user was in.
pub fn release_focus() {
    unsafe {
        let _: () = msg_send![ns_app(), hide: std::ptr::null::<AnyObject>()];
    }
}

/// Bottom-center of the screen under the pointer, above the Dock.
pub fn place(window: &gtk::Window) {
    let Some(win) = ns_window(window) else { return };
    let Some(screen) = pointer_screen() else { return };
    unsafe {
        // Cocoa coordinates: origin at the bottom-left of the main screen.
        let visible: Rect = msg_send![screen, visibleFrame];
        let frame: Rect = msg_send![win, frame];
        let x = visible.origin.x + ((visible.size.w - frame.size.w) / 2.0).round();
        let y = visible.origin.y + MARGIN;
        let _: () = msg_send![win, setFrameOrigin: Point { x, y }];
    }
}

thread_local! {
    /// Window and visible-notch rectangle (window coordinates, top-left
    /// origin) the click-through watcher checks the pointer against.
    static REGION: RefCell<Option<(gtk::Window, (i32, i32, i32, i32))>> = const { RefCell::new(None) };
    static WATCHING: Cell<bool> = const { Cell::new(false) };
    static IGNORING: Cell<Option<bool>> = const { Cell::new(None) };
}

/// Pass clicks on the transparent rest of the window through to what is
/// behind. AppKit has no input regions, only a per-window "ignore the
/// mouse", so while the notch is on screen a light timer (about 20 checks a
/// second, nothing while hidden) flips it as the pointer enters and leaves
/// the visible notch.
pub fn set_region(window: &gtk::Window, rect: (i32, i32, i32, i32)) {
    REGION.with(|r| *r.borrow_mut() = Some((window.clone(), rect)));
    IGNORING.with(|i| i.set(None));
    if WATCHING.with(|w| w.replace(true)) {
        return;
    }
    glib::timeout_add_local(Duration::from_millis(50), || {
        let keep = REGION.with(|r| {
            let r = r.borrow();
            let Some((window, (rx, ry, rw, rh))) = r.as_ref() else { return false };
            if !window.is_visible() {
                return false;
            }
            let Some(win) = ns_window(window) else { return false };
            unsafe {
                // Never drop a drag (seek, volume) that left the notch.
                let buttons: usize = msg_send![class!(NSEvent), pressedMouseButtons];
                if buttons != 0 {
                    return true;
                }
                let mouse: Point = msg_send![class!(NSEvent), mouseLocation];
                let frame: Rect = msg_send![win, frame];
                let lx = mouse.x - frame.origin.x;
                let ly = frame.size.h - (mouse.y - frame.origin.y);
                let inside = lx >= *rx as f64
                    && lx < (*rx + *rw) as f64
                    && ly >= *ry as f64
                    && ly < (*ry + *rh) as f64;
                let ignore = !inside;
                if IGNORING.with(|i| i.replace(Some(ignore))) != Some(ignore) {
                    let _: () = msg_send![win, setIgnoresMouseEvents: Bool::new(ignore)];
                }
            }
            true
        });
        if keep {
            glib::ControlFlow::Continue
        } else {
            WATCHING.with(|w| w.set(false));
            glib::ControlFlow::Break
        }
    });
}
