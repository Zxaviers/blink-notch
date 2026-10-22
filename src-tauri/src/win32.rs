//! Win32 helpers for the notch overlay window.
//!
//! - `make_overlay`: tool window (no taskbar / Alt+Tab entry) that never takes
//!   activation, so hovering or clicking the notch leaves focus in the app below.
//! - `fullscreen_app_active`: SHQueryUserNotificationState, the PRD's proposed
//!   fullscreen / presentation detector (Spike B).

#![cfg(windows)]

use std::sync::atomic::{AtomicBool, AtomicIsize, Ordering};

use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, WPARAM};
use windows::Win32::UI::Shell::{DefSubclassProc, SHQueryUserNotificationState, SetWindowSubclass};
use windows::Win32::UI::WindowsAndMessaging::{
    GetForegroundWindow, GetWindowLongPtrW, SetForegroundWindow, SetWindowLongPtrW, SetWindowPos,
    GWL_EXSTYLE, HWND_TOPMOST, MA_NOACTIVATE, SET_WINDOW_POS_FLAGS, SWP_FRAMECHANGED,
    SWP_NOACTIVATE, SWP_NOMOVE, SWP_NOSIZE, WA_INACTIVE, WINDOW_EX_STYLE, WM_ACTIVATE,
    WM_MOUSEACTIVATE, WS_EX_APPWINDOW, WS_EX_NOACTIVATE, WS_EX_TOOLWINDOW,
};

fn hwnd_of(win: &tauri::WebviewWindow) -> Option<HWND> {
    // Go through the raw handle so `windows` minor-version drift between this
    // crate and Tauri cannot break the build.
    let raw = win.hwnd().ok()?;
    Some(HWND(raw.0))
}

pub fn make_overlay(win: &tauri::WebviewWindow) -> bool {
    let Some(hwnd) = hwnd_of(win) else {
        return false;
    };
    unsafe {
        let current = WINDOW_EX_STYLE(GetWindowLongPtrW(hwnd, GWL_EXSTYLE) as u32);
        let updated = (current | WS_EX_TOOLWINDOW | WS_EX_NOACTIVATE) & !WS_EX_APPWINDOW;
        if current != updated {
            SetWindowLongPtrW(hwnd, GWL_EXSTYLE, updated.0 as isize);
        }
        let flags: SET_WINDOW_POS_FLAGS =
            SWP_NOSIZE | SWP_NOMOVE | SWP_NOACTIVATE | SWP_FRAMECHANGED;
        SetWindowPos(hwnd, Some(HWND_TOPMOST), 0, 0, 0, 0, flags).is_ok()
    }
}

unsafe extern "system" fn no_activate_proc(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
    _id: usize,
    _data: usize,
) -> LRESULT {
    // WS_EX_NOACTIVATE alone does not stop a click inside the WebView2 child
    // from activating the top-level window; answering WM_MOUSEACTIVATE does.
    if std::env::var_os("DN_DEBUG").is_some() && (msg == WM_MOUSEACTIVATE || msg == WM_ACTIVATE) {
        eprintln!(
            "[notch] msg=0x{msg:X} wparam=0x{:X} prev=0x{:X}",
            wparam.0,
            PREV_FOREGROUND.load(Ordering::Relaxed)
        );
    }
    if msg == WM_MOUSEACTIVATE {
        return LRESULT(MA_NOACTIVATE as isize);
    }
    // WebView2 still calls SetFocus on click, which activates the top-level
    // window anyway. Hand the foreground straight back to the app that had it,
    // unless the user asked for keyboard focus with the hotkey.
    if msg == WM_ACTIVATE
        && (wparam.0 & 0xFFFF) as u32 != WA_INACTIVE
        && !KEYBOARD_MODE.load(Ordering::Relaxed)
    {
        let prev = PREV_FOREGROUND.load(Ordering::Relaxed);
        if prev != 0 && prev != hwnd.0 as isize {
            restore_foreground(prev);
        }
        return LRESULT(0);
    }
    DefSubclassProc(hwnd, msg, wparam, lparam)
}

/// Give the foreground back to `prev` shortly after, retrying until it sticks.
/// Restoring inside the activation message is overridden by the activation
/// still in progress, and one attempt can lose the race, hence the retries.
fn restore_foreground(prev: isize) {
    std::thread::spawn(move || {
        let target = HWND(prev as *mut _);
        let mut ok = false;
        for wait_ms in [10u64, 40, 100, 250] {
            std::thread::sleep(std::time::Duration::from_millis(wait_ms));
            let _ = unsafe { SetForegroundWindow(target) };
            if unsafe { GetForegroundWindow() } == target {
                ok = true;
                break;
            }
        }
        if std::env::var_os("DN_DEBUG").is_some() {
            eprintln!("[notch] restore foreground ok={ok}");
        }
    });
}

/// True from the hotkey until the notch is dismissed: the window is allowed
/// to take focus so the keyboard works inside the panel.
static KEYBOARD_MODE: AtomicBool = AtomicBool::new(false);

/// Let the notch take keyboard focus (hotkey). Remembers the app that had it.
/// Returns whether the notch really became the foreground window.
pub fn enter_keyboard_mode(win: &tauri::WebviewWindow) -> bool {
    let Some(hwnd) = hwnd_of(win) else {
        return false;
    };
    remember_foreground(win);
    KEYBOARD_MODE.store(true, Ordering::Relaxed);
    unsafe {
        let current = WINDOW_EX_STYLE(GetWindowLongPtrW(hwnd, GWL_EXSTYLE) as u32);
        let updated = current & !WS_EX_NOACTIVATE;
        if current != updated {
            SetWindowLongPtrW(hwnd, GWL_EXSTYLE, updated.0 as isize);
        }
        let _ = SetForegroundWindow(hwnd);
    }
    let _ = win.set_focus();
    unsafe { GetForegroundWindow() == hwnd }
}

/// Dismiss keyboard mode: back to a window that never takes focus, and give
/// the foreground back to the app that had it.
pub fn exit_keyboard_mode(win: &tauri::WebviewWindow) {
    if !KEYBOARD_MODE.swap(false, Ordering::Relaxed) {
        return;
    }
    make_overlay(win);
    let prev = PREV_FOREGROUND.load(Ordering::Relaxed);
    let own = hwnd_of(win).map_or(0, |h| h.0 as isize);
    if prev != 0 && prev != own && unsafe { GetForegroundWindow() }.0 as isize == own {
        restore_foreground(prev);
    }
}

/// Foreground window recorded just before the cursor entered the notch.
static PREV_FOREGROUND: AtomicIsize = AtomicIsize::new(0);

/// Call when the cursor enters the notch, while the previous app still owns focus.
pub fn remember_foreground(win: &tauri::WebviewWindow) {
    let Some(own) = hwnd_of(win) else {
        return;
    };
    let fg = unsafe { GetForegroundWindow() };
    if !fg.0.is_null() && fg != own {
        PREV_FOREGROUND.store(fg.0 as isize, Ordering::Relaxed);
    }
}

/// Must be called on the thread that owns the window (Tauri's setup does).
pub fn install_no_activate(win: &tauri::WebviewWindow) -> bool {
    let Some(hwnd) = hwnd_of(win) else {
        return false;
    };
    unsafe { SetWindowSubclass(hwnd, Some(no_activate_proc), 1, 0).as_bool() }
}

/// True while a fullscreen app, D3D fullscreen game or presentation is active.
pub fn fullscreen_app_active() -> bool {
    // QUNS_BUSY = 2, QUNS_RUNNING_D3D_FULL_SCREEN = 3, QUNS_PRESENTATION_MODE = 4
    match unsafe { SHQueryUserNotificationState() } {
        Ok(state) => matches!(state.0, 2 | 3 | 4),
        Err(_) => false,
    }
}
