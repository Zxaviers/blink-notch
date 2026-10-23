//! Taking over the volume keys, so Windows' own volume bar does not appear (the notch
//! shows its own).
//!
//! A low-level keyboard hook on its own thread looks at one thing: whether a key is
//! Volume Up, Volume Down or Mute. Those are swallowed and posted to the window thread,
//! which changes the volume. Every other key goes straight on, untouched and unread. If the
//! notch is not running (or has hidden itself) the keys work as Windows always made them
//! work, because the hook only exists while the notch does.
//!
//! The hook callback does nothing but compare a number and post a message, so typing never waits.

use std::sync::atomic::{AtomicBool, AtomicIsize, Ordering};

use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, WPARAM};
use windows::Win32::UI::WindowsAndMessaging::{
    CallNextHookEx, DispatchMessageW, GetMessageW, PostMessageW, SetWindowsHookExW, TranslateMessage, KBDLLHOOKSTRUCT, MSG, WH_KEYBOARD_LL, WM_KEYDOWN,
    WM_SYSKEYDOWN,
};

/// Posted to the window; `wparam` is the key (0xAF up, 0xAE down, 0xAD mute).
pub const WM_MEDIAKEY: u32 = 0x8000 + 9;
pub const VK_VOLUME_MUTE: u32 = 0xAD;
pub const VK_VOLUME_DOWN: u32 = 0xAE;
pub const VK_VOLUME_UP: u32 = 0xAF;

static ENABLED: AtomicBool = AtomicBool::new(false);
static TARGET: AtomicIsize = AtomicIsize::new(0);

/// Whether the keys are being taken over right now.
pub fn set_enabled(on: bool) {
    ENABLED.store(on, Ordering::Relaxed);
}

unsafe extern "system" fn hook(code: i32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    if code >= 0 && ENABLED.load(Ordering::Relaxed) {
        let key = (*(lparam.0 as *const KBDLLHOOKSTRUCT)).vkCode;
        if matches!(key, VK_VOLUME_MUTE | VK_VOLUME_DOWN | VK_VOLUME_UP) {
            let target = TARGET.load(Ordering::Relaxed);
            // A press (and each auto-repeat) is posted; the release is swallowed too so Windows
            // never sees half of a keystroke.
            if target != 0 && (wparam.0 as u32 == WM_KEYDOWN || wparam.0 as u32 == WM_SYSKEYDOWN) {
                let _ = PostMessageW(Some(HWND(target as *mut _)), WM_MEDIAKEY, WPARAM(key as usize), LPARAM(0));
            }
            return LRESULT(1);
        }
    }
    CallNextHookEx(None, code, wparam, lparam)
}

/// Install the hook on a thread of its own. Returns whether it is in place.
pub fn start(hwnd: HWND) -> bool {
    TARGET.store(hwnd.0 as isize, Ordering::Relaxed);
    let (tx, rx) = std::sync::mpsc::channel::<bool>();
    let spawned = std::thread::Builder::new().name("blink-mediakeys".into()).spawn(move || unsafe {
        let installed = SetWindowsHookExW(WH_KEYBOARD_LL, Some(hook), None, 0).is_ok();
        let _ = tx.send(installed);
        if !installed {
            return;
        }
        // A low-level hook is called through this thread's message queue.
        let mut msg = MSG::default();
        while GetMessageW(&mut msg, None, 0, 0).0 > 0 {
            let _ = TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }
    });
    spawned.is_ok() && rx.recv().unwrap_or(false)
}
