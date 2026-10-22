//! Keyboard focus on a window that never takes focus on its own.
//!
//! The notch is a tool window with WS_EX_NOACTIVATE, so hovering and clicking
//! leave the app underneath active. The hotkey needs the opposite: lift the
//! flag, take the foreground so keys arrive, and on the way out put the flag
//! back and hand the foreground to the app that had it.

use std::sync::atomic::{AtomicIsize, Ordering};
use std::time::Duration;

use windows::Win32::Foundation::{HWND, LPARAM};
use windows::Win32::System::Threading::{AttachThreadInput, GetCurrentThreadId};
use windows::Win32::UI::Input::KeyboardAndMouse::SetFocus;
use windows::Win32::UI::WindowsAndMessaging::{
    GetForegroundWindow, GetWindowLongPtrW, GetWindowThreadProcessId, SetForegroundWindow, SetWindowLongPtrW, SetWindowPos,
    GWL_EXSTYLE, HWND_TOPMOST, SWP_FRAMECHANGED, SWP_NOACTIVATE, SWP_NOMOVE, SWP_NOSIZE, WINDOW_EX_STYLE, WS_EX_NOACTIVATE,
    WS_EX_TOOLWINDOW,
};

/// The window that had the foreground just before the notch took it.
static PREVIOUS: AtomicIsize = AtomicIsize::new(0);

fn set_no_activate(hwnd: HWND, on: bool) {
    unsafe {
        let current = WINDOW_EX_STYLE(GetWindowLongPtrW(hwnd, GWL_EXSTYLE) as u32);
        let wanted = if on { current | WS_EX_NOACTIVATE | WS_EX_TOOLWINDOW } else { current & !WS_EX_NOACTIVATE };
        if wanted != current {
            SetWindowLongPtrW(hwnd, GWL_EXSTYLE, wanted.0 as isize);
            let _ = SetWindowPos(hwnd, Some(HWND_TOPMOST), 0, 0, 0, 0, SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE | SWP_FRAMECHANGED);
        }
    }
}

/// Take keyboard focus. Returns whether the window really became the foreground window.
pub fn enter(hwnd: HWND) -> bool {
    unsafe {
        let fg = GetForegroundWindow();
        if !fg.0.is_null() && fg != hwnd {
            PREVIOUS.store(fg.0 as isize, Ordering::Relaxed);
        }
        set_no_activate(hwnd, false);
        let _ = SetForegroundWindow(hwnd);
        if GetForegroundWindow() != hwnd {
            // Windows only lets the process that last received input take the foreground.
            // Briefly share input state with the current foreground thread to qualify.
            let fg_thread = GetWindowThreadProcessId(fg, None);
            let me = GetCurrentThreadId();
            if fg_thread != 0 && fg_thread != me {
                let _ = AttachThreadInput(me, fg_thread, true);
                let _ = SetForegroundWindow(hwnd);
                let _ = AttachThreadInput(me, fg_thread, false);
            }
        }
        let _ = SetFocus(Some(hwnd));
        GetForegroundWindow() == hwnd
    }
}

/// Stop taking focus again and give the foreground back to the previous app.
/// `give_back` is false when Windows already moved focus elsewhere.
pub fn leave(hwnd: HWND, give_back: bool) {
    set_no_activate(hwnd, true);
    let previous = PREVIOUS.load(Ordering::Relaxed);
    let own = hwnd.0 as isize;
    if give_back && previous != 0 && previous != own && unsafe { GetForegroundWindow() }.0 as isize == own {
        restore_foreground(previous);
    }
}

/// Give the foreground back, retrying: one attempt can lose a race with the
/// deactivation that is still in progress.
fn restore_foreground(previous: isize) {
    std::thread::spawn(move || {
        let target = HWND(previous as *mut _);
        for wait_ms in [10u64, 40, 100, 250] {
            std::thread::sleep(Duration::from_millis(wait_ms));
            unsafe {
                let _ = SetForegroundWindow(target);
                if GetForegroundWindow() == target {
                    return;
                }
            }
        }
    });
}

#[allow(dead_code)]
fn unused(_: LPARAM) {}