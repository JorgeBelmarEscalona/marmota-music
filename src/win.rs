//! Windows glue: what the layer shell, Hyprland and the session give the
//! notch on Linux, done with the Win32 API.

use gtk4 as gtk;
use std::os::windows::process::CommandExt;
use std::process::Command;
use std::sync::mpsc::Sender;

use gtk::prelude::*;
use windows_sys::Win32::Foundation::{SetHandleInformation, HANDLE_FLAG_INHERIT, HWND, POINT, RECT};
use windows_sys::Win32::Graphics::Gdi::{
    CreateRectRgn, GetMonitorInfoW, MonitorFromPoint, MonitorFromWindow, SetWindowRgn, MONITORINFO,
    MONITOR_DEFAULTTONEAREST,
};
use windows_sys::Win32::System::Console::{
    FreeConsole, GetConsoleMode, GetStdHandle, SetConsoleMode, ENABLE_VIRTUAL_TERMINAL_PROCESSING,
    STD_ERROR_HANDLE, STD_INPUT_HANDLE, STD_OUTPUT_HANDLE,
};
use windows_sys::Win32::System::Threading::CREATE_NO_WINDOW;
use std::sync::atomic::Ordering;
use windows_sys::w;
use windows_sys::Win32::Foundation::{LPARAM, LRESULT, WPARAM};
use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
use windows_sys::Win32::UI::Input::KeyboardAndMouse::{
    keybd_event, GetAsyncKeyState, RegisterHotKey, KEYEVENTF_KEYUP, MOD_ALT, MOD_CONTROL,
    MOD_NOREPEAT, MOD_SHIFT, MOD_WIN, VK_CONTROL, VK_LWIN, VK_MENU, VK_RWIN, VK_SHIFT,
};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    CallNextHookEx, CreateWindowExW, GetCursorPos, GetMessageW, GetWindowLongPtrW, GetWindowRect,
    SetWindowLongPtrW, SetWindowPos, SetWindowsHookExW, GWLP_HWNDPARENT, HC_ACTION, HWND_TOPMOST,
    KBDLLHOOKSTRUCT, MSG, SWP_NOACTIVATE, SWP_NOSIZE, WH_KEYBOARD_LL, WM_HOTKEY, WM_KEYDOWN,
    WM_SYSKEYDOWN,
};

use crate::model::{Cmd, Event};

/// Gap between the notch and the taskbar, in logical pixels.
const MARGIN: i32 = 14;

/// Keep console tools (yt-dlp, the daemon itself) from flashing a window.
pub fn no_console(cmd: &mut Command) {
    cmd.creation_flags(CREATE_NO_WINDOW);
}

/// Children inherit every inheritable handle, and the daemon outlives the
/// CLI call that starts it: holding the caller's stdout pipe would keep
/// `marmota-music toggle | …` waiting forever.
pub fn keep_stdio_private() {
    unsafe {
        for std in [STD_INPUT_HANDLE, STD_OUTPUT_HANDLE, STD_ERROR_HANDLE] {
            let h = GetStdHandle(std);
            if !h.is_null() {
                SetHandleInformation(h, HANDLE_FLAG_INHERIT, 0);
            }
        }
    }
}

pub fn detach_console() {
    unsafe {
        FreeConsole();
    }
}

/// Let the classic console render the assistant's colors.
pub fn enable_ansi() {
    unsafe {
        let out = GetStdHandle(STD_OUTPUT_HANDLE);
        let mut mode = 0;
        if GetConsoleMode(out, &mut mode) != 0 {
            SetConsoleMode(out, mode | ENABLE_VIRTUAL_TERMINAL_PROCESSING);
        }
    }
}

/// Shortcut used when the config has no `hotkey` (Windows owns `Win + Shift + M`).
pub const DEFAULT_HOTKEY: &str = "Win + Alt + M";

/// The shortcut that opens the notch (config key `hotkey`).
pub fn hotkey_label() -> String {
    crate::paths::config()
        .get("hotkey")
        .and_then(|v| v.as_str())
        .map(str::to_string)
        .unwrap_or_else(|| DEFAULT_HOTKEY.into())
}

/// "Ctrl + Alt + M" → (modifiers, virtual key).
fn parse_hotkey(spec: &str) -> Option<(u32, u32)> {
    let mut mods = MOD_NOREPEAT;
    let mut key = None;
    for part in spec.split('+').map(|p| p.trim().to_ascii_lowercase()) {
        match part.as_str() {
            "ctrl" | "control" => mods |= MOD_CONTROL,
            "alt" => mods |= MOD_ALT,
            "shift" => mods |= MOD_SHIFT,
            "win" | "super" => mods |= MOD_WIN,
            "space" => key = Some(0x20),
            p if p.len() == 1 && p.chars().all(|c| c.is_ascii_alphanumeric()) => {
                key = Some(p.to_ascii_uppercase().as_bytes()[0] as u32)
            }
            p if p.starts_with('f') => {
                let n: u32 = p[1..].parse().ok().filter(|n| (1..=24).contains(n))?;
                key = Some(0x70 + n - 1);
            }
            _ => return None,
        }
    }
    Some((mods, key?))
}

/// What the keyboard hook watches for, when the shortcut is taken.
struct Hook {
    mods: u32,
    vk: u32,
    tx: std::sync::Mutex<Sender<Event>>,
    /// The key is held down: ignore auto-repeat until it is released.
    down: std::sync::atomic::AtomicBool,
}

static HOOK: std::sync::OnceLock<Hook> = std::sync::OnceLock::new();

fn pressed(vk: u16) -> bool {
    unsafe { GetAsyncKeyState(vk as i32) as u16 & 0x8000 != 0 }
}

/// Modifiers held right now, as MOD_* flags.
fn held_mods() -> u32 {
    let mut m = 0;
    if pressed(VK_CONTROL) {
        m |= MOD_CONTROL;
    }
    if pressed(VK_MENU) {
        m |= MOD_ALT;
    }
    if pressed(VK_SHIFT) {
        m |= MOD_SHIFT;
    }
    if pressed(VK_LWIN) || pressed(VK_RWIN) {
        m |= MOD_WIN;
    }
    m
}

unsafe extern "system" fn keyboard_hook(code: i32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    if code == HC_ACTION as i32 {
        if let Some(hook) = HOOK.get() {
            let key = &*(lparam as *const KBDLLHOOKSTRUCT);
            if key.vkCode == hook.vk {
                let msg = wparam as u32;
                if msg == WM_KEYDOWN || msg == WM_SYSKEYDOWN {
                    if held_mods() == hook.mods & !MOD_NOREPEAT {
                        if !hook.down.swap(true, Ordering::Relaxed) {
                            if let Ok(tx) = hook.tx.lock() {
                                let _ = tx.send(Event::Cmd(Cmd::Toggle));
                            }
                            // Releasing Win (or Alt) alone right after would
                            // open the Start menu (or the app's menu bar); an
                            // unassigned key in between tells Windows a
                            // shortcut was used.
                            keybd_event(0xE8, 0, 0, 0);
                            keybd_event(0xE8, 0, KEYEVENTF_KEYUP, 0);
                        }
                        // Swallow it so the program that owns the shortcut,
                        // and the focused window, never see it.
                        return 1;
                    }
                } else if hook.down.swap(false, Ordering::Relaxed) {
                    return 1;
                }
            }
        }
    }
    CallNextHookEx(std::ptr::null_mut(), code, wparam, lparam)
}

/// Global shortcut that toggles the notch, from the config key `hotkey`.
/// When another program already registered it, a low-level keyboard hook
/// (which sees keys before registered shortcuts do) takes it over.
pub fn hotkey(tx: Sender<Event>) {
    let wanted = hotkey_label();
    std::thread::Builder::new()
        .name("hotkey".into())
        .spawn(move || unsafe {
            let Some((mods, vk)) = parse_hotkey(&wanted) else {
                let _ = tx.send(Event::Error(format!("Atajo «{wanted}» no válido: revisa «hotkey» en config.json")));
                return;
            };
            if RegisterHotKey(std::ptr::null_mut(), 1, mods, vk) == 0 {
                let _ = HOOK.set(Hook {
                    mods,
                    vk,
                    tx: std::sync::Mutex::new(tx.clone()),
                    down: false.into(),
                });
                let module = GetModuleHandleW(std::ptr::null());
                if SetWindowsHookExW(WH_KEYBOARD_LL, Some(keyboard_hook), module, 0).is_null() {
                    let _ = tx.send(Event::Error(format!("No pude usar el atajo {wanted}")));
                    return;
                }
            }
            // Delivers WM_HOTKEY, and runs the hook (it is called from this
            // thread's message loop).
            let mut msg: MSG = std::mem::zeroed();
            while GetMessageW(&mut msg, std::ptr::null_mut(), 0, 0) > 0 {
                if msg.message == WM_HOTKEY && tx.send(Event::Cmd(Cmd::Toggle)).is_err() {
                    break;
                }
            }
        })
        .ok();
}

fn hwnd(window: &gtk::Window) -> Option<HWND> {
    let surface = window.surface()?.downcast::<gdk4_win32::Win32Surface>().ok()?;
    Some(surface.handle().0 as HWND)
}

fn monitor_info(mon: windows_sys::Win32::Graphics::Gdi::HMONITOR) -> Option<MONITORINFO> {
    let mut info: MONITORINFO = unsafe { std::mem::zeroed() };
    info.cbSize = std::mem::size_of::<MONITORINFO>() as u32;
    (unsafe { GetMonitorInfoW(mon, &mut info) } != 0).then_some(info)
}

/// Logical height of the monitor under the pointer (where the notch opens),
/// in the window's GDK units.
pub fn pointer_monitor_height(window: &gtk::Window) -> Option<f64> {
    let mut pt = POINT { x: 0, y: 0 };
    unsafe { GetCursorPos(&mut pt) };
    let info = monitor_info(unsafe { MonitorFromPoint(pt, MONITOR_DEFAULTTONEAREST) })?;
    let h = (info.rcMonitor.bottom - info.rcMonitor.top) as f64;
    Some(h / window.scale_factor().max(1) as f64)
}

thread_local! {
    static OWNER: HWND = unsafe {
        CreateWindowExW(0, w!("STATIC"), std::ptr::null(), 0, 0, 0, 0, 0,
            std::ptr::null_mut(), std::ptr::null_mut(), std::ptr::null_mut(), std::ptr::null())
    };
}

/// Keep the notch out of the taskbar and Alt+Tab. GDK rewrites the extended
/// style on every show (dropping WS_EX_TOOLWINDOW), but leaves the owner
/// alone, and owned windows get no taskbar button. Call before showing.
pub fn hide_from_taskbar(window: &gtk::Window) {
    let Some(h) = hwnd(window) else { return };
    OWNER.with(|owner| unsafe {
        if !owner.is_null() && GetWindowLongPtrW(h, GWLP_HWNDPARENT) != *owner as isize {
            SetWindowLongPtrW(h, GWLP_HWNDPARENT, *owner as isize);
        }
    });
}

/// Bottom-center of the monitor under the pointer, above the taskbar and on
/// top of other windows.
pub fn place(window: &gtk::Window) {
    let Some(h) = hwnd(window) else { return };
    let mut pt = POINT { x: 0, y: 0 };
    let mut rect = RECT { left: 0, top: 0, right: 0, bottom: 0 };
    unsafe {
        GetCursorPos(&mut pt);
        let mut mon = MonitorFromPoint(pt, MONITOR_DEFAULTTONEAREST);
        if mon.is_null() {
            mon = MonitorFromWindow(h, MONITOR_DEFAULTTONEAREST);
        }
        let Some(info) = monitor_info(mon) else { return };
        GetWindowRect(h, &mut rect);
        let work = info.rcWork;
        let (w, wh) = (rect.right - rect.left, rect.bottom - rect.top);
        let x = work.left + (work.right - work.left - w) / 2;
        let y = work.bottom - wh - MARGIN * window.scale_factor().max(1);
        SetWindowPos(h, HWND_TOPMOST, x, y, 0, 0, SWP_NOSIZE | SWP_NOACTIVATE);
    }
}

/// Clip the window to the visible notch: Windows ignores GDK input regions,
/// so without this the transparent rest of the surface would eat clicks.
pub fn set_region(window: &gtk::Window, (x, y, w, h): (i32, i32, i32, i32)) {
    let Some(hwnd) = hwnd(window) else { return };
    let s = window.scale_factor().max(1);
    unsafe {
        let rgn = CreateRectRgn(x * s, y * s, (x + w) * s, (y + h) * s);
        // On success the system owns the region.
        SetWindowRgn(hwnd, rgn, 1);
    }
}
