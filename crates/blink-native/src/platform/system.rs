//! Small Windows facts and switches: fullscreen detection and the single-instance guard.

use windows::core::{w, PCWSTR};
use windows::Win32::Foundation::{CloseHandle, GetLastError, ERROR_ALREADY_EXISTS, HANDLE, LPARAM, WPARAM};
use windows::Win32::System::Threading::CreateMutexW;
use windows::Win32::UI::Shell::SHQueryUserNotificationState;
use windows::Win32::UI::WindowsAndMessaging::{FindWindowW, PostMessageW};

/// Posted by a second launch to the running instance: show yourself.
pub const WM_SHOW_ME: u32 = 0x8000 + 2;

/// True while a fullscreen app, D3D fullscreen game or presentation is active
/// (QUNS_BUSY, QUNS_RUNNING_D3D_FULL_SCREEN, QUNS_PRESENTATION_MODE).
pub fn fullscreen_app_active() -> bool {
    match unsafe { SHQueryUserNotificationState() } {
        Ok(state) => matches!(state.0, 2..=4),
        Err(_) => false,
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
