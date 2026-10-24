//! Small Windows facts and switches: fullscreen detection and the single-instance guard.

use windows::core::{w, PCWSTR};
use windows::Win32::Foundation::{CloseHandle, GetLastError, ERROR_ALREADY_EXISTS, HANDLE, LPARAM, WPARAM};
use windows::Win32::System::Threading::CreateMutexW;
use windows::Win32::UI::Shell::SHQueryUserNotificationState;
use windows::Win32::UI::WindowsAndMessaging::{FindWindowW, PostMessageW};

/// Posted by a second launch to the running instance: show yourself.
pub const WM_SHOW_ME: u32 = 0x8000 + 2;

/// What kind of fullscreen, if any, is in front.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FullscreenKind {
    None,
    /// An ordinary app covering the whole screen (Windows calls it "busy"): a browser or an
    /// app in fullscreen, a maximized borderless window, a video player.
    App,
    /// A game in exclusive fullscreen, or a presentation.
    Game,
}

pub fn fullscreen_kind() -> FullscreenKind {
    // The empty desktop (or the taskbar) in front is never a fullscreen app, whatever Windows' own check says of it.
    if desktop_in_front() {
        return FullscreenKind::None;
    }
    match unsafe { SHQueryUserNotificationState() } {
        // QUNS_RUNNING_D3D_FULL_SCREEN = 3, QUNS_PRESENTATION_MODE = 4
        Ok(state) if matches!(state.0, 3 | 4) => FullscreenKind::Game,
        // QUNS_BUSY = 2: only when the window in front really covers its whole screen.
        Ok(state) if state.0 == 2 && foreground_covers_its_screen() => FullscreenKind::App,
        _ => FullscreenKind::None,
    }
}

/// Whether the window in front covers the whole of the screen it is on, taskbar included (not just maximized).
fn foreground_covers_its_screen() -> bool {
    use windows::Win32::Foundation::RECT;
    use windows::Win32::Graphics::Gdi::{GetMonitorInfoW, MonitorFromWindow, MONITORINFO, MONITOR_DEFAULTTONEAREST};
    use windows::Win32::UI::WindowsAndMessaging::{GetForegroundWindow, GetWindowRect};
    unsafe {
        let hwnd = GetForegroundWindow();
        if hwnd.0.is_null() {
            return false;
        }
        let mut window = RECT::default();
        if GetWindowRect(hwnd, &mut window).is_err() {
            return false;
        }
        let mut info = MONITORINFO { cbSize: std::mem::size_of::<MONITORINFO>() as u32, ..Default::default() };
        if !GetMonitorInfoW(MonitorFromWindow(hwnd, MONITOR_DEFAULTTONEAREST), &mut info).as_bool() {
            return false;
        }
        let m = info.rcMonitor;
        window.left <= m.left && window.top <= m.top && window.right >= m.right && window.bottom >= m.bottom
    }
}

/// Whether no window is in front: the desktop itself (its icons) or the taskbar has the foreground, as after
/// "show desktop" or a click on the wallpaper. A window, the Start menu or a lock screen in front is not the desktop.
pub fn desktop_in_front() -> bool {
    use windows::Win32::UI::WindowsAndMessaging::{GetClassNameW, GetForegroundWindow};
    unsafe {
        let hwnd = GetForegroundWindow();
        if hwnd.0.is_null() {
            return false;
        }
        let mut buf = [0u16; 64];
        let n = GetClassNameW(hwnd, &mut buf) as usize;
        let class = String::from_utf16_lossy(&buf[..n.min(buf.len())]);
        matches!(class.as_str(), "Progman" | "WorkerW" | "Shell_TrayWnd" | "Shell_SecondaryTrayWnd")
    }
}

// ---------------------------------------------------------- single instance

/// Holds the named mutex for the life of the process.
pub struct InstanceGuard(HANDLE);

impl Drop for InstanceGuard {
    fn drop(&mut self) {
        unsafe {
            let _ = CloseHandle(self.0);
        }
    }
}

/// Some(guard) when this is the only instance. None when another is running;
/// it has then been asked to show itself.
pub fn single_instance(tag: &str) -> Option<InstanceGuard> {
    let name: Vec<u16> = format!("Local\\BlinkNotch-{tag}").encode_utf16().chain(Some(0)).collect();
    unsafe {
        let handle = CreateMutexW(None, false, PCWSTR(name.as_ptr())).ok()?;
        if GetLastError() == ERROR_ALREADY_EXISTS {
            let _ = CloseHandle(handle);
            if let Ok(existing) = FindWindowW(w!("BlinkNotch"), None) {
                let _ = PostMessageW(Some(existing), WM_SHOW_ME, WPARAM(0), LPARAM(0));
            }
            return None;
        }
        Some(InstanceGuard(handle))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_second_guard_for_the_same_tag_is_refused() {
        let first = single_instance("unit-test-tag");
        assert!(first.is_some());
        assert!(single_instance("unit-test-tag").is_none());
        drop(first);
        assert!(single_instance("unit-test-tag").is_some());
    }
}
