//! Small Windows facts and switches: fullscreen detection, autostart and the
//! single-instance guard.

use windows::core::{w, PCWSTR};
use windows::Win32::Foundation::{CloseHandle, GetLastError, ERROR_ALREADY_EXISTS, HANDLE, LPARAM, WPARAM};
use windows::Win32::System::Registry::{
    RegCloseKey, RegCreateKeyExW, RegDeleteValueW, RegGetValueW, RegSetValueExW, HKEY, HKEY_CURRENT_USER, KEY_SET_VALUE, REG_OPTION_NON_VOLATILE,
    REG_SZ, RRF_RT_REG_SZ,
};
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

// ---------------------------------------------------------------- autostart

pub const RUN_KEY: &str = r"Software\Microsoft\Windows\CurrentVersion\Run";
pub const AUTOSTART_NAME: &str = "Blink";
/// What the WebView2 app registered itself as.
pub const LEGACY_AUTOSTART_NAME: &str = "Deadline Notch";

fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(Some(0)).collect()
}

pub fn autostart_value(run_key: &str, name: &str) -> Option<String> {
    let (key, name) = (wide(run_key), wide(name));
    let mut buf = vec![0u16; 1024];
    let mut size = (buf.len() * 2) as u32;
    let rc = unsafe {
        RegGetValueW(HKEY_CURRENT_USER, PCWSTR(key.as_ptr()), PCWSTR(name.as_ptr()), RRF_RT_REG_SZ, None, Some(buf.as_mut_ptr().cast()), Some(&mut size))
    };
    if rc.is_err() {
        return None;
    }
    let chars = (size as usize / 2).saturating_sub(1);
    Some(String::from_utf16_lossy(&buf[..chars]))
}

pub fn set_autostart_in(run_key: &str, name: &str, command: Option<&str>) -> Result<(), String> {
    let key_name = wide(run_key);
    let value_name = wide(name);
    unsafe {
        let mut key = HKEY::default();
        let rc = RegCreateKeyExW(HKEY_CURRENT_USER, PCWSTR(key_name.as_ptr()), None, PCWSTR::null(), REG_OPTION_NON_VOLATILE, KEY_SET_VALUE, None, &mut key, None);
        if rc.is_err() {
            return Err(format!("registry: {rc:?}"));
        }
        let result = match command {
            Some(cmd) => {
                let data = wide(cmd);
                let bytes = std::slice::from_raw_parts(data.as_ptr().cast::<u8>(), data.len() * 2);
                RegSetValueExW(key, PCWSTR(value_name.as_ptr()), None, REG_SZ, Some(bytes))
            }
            None => {
                let rc = RegDeleteValueW(key, PCWSTR(value_name.as_ptr()));
                // Deleting what is not there (ERROR_FILE_NOT_FOUND = 2) is fine.
                if rc.is_err() && rc.0 != 2 {
                    rc
                } else {
                    Default::default()
                }
            }
        };
        let _ = RegCloseKey(key);
        if result.is_err() {
            return Err(format!("registry: {result:?}"));
        }
    }
    Ok(())
}

/// Start with Windows (or stop). Also clears the old app's entry so only one launches.
pub fn set_autostart(enabled: bool) -> Result<(), String> {
    let exe = std::env::current_exe().map_err(|e| e.to_string())?;
    let command = format!("\"{}\"", exe.display());
    set_autostart_in(RUN_KEY, LEGACY_AUTOSTART_NAME, None)?;
    set_autostart_in(RUN_KEY, AUTOSTART_NAME, enabled.then_some(command.as_str()))
}

pub fn autostart_enabled() -> bool {
    autostart_value(RUN_KEY, AUTOSTART_NAME).is_some()
}

/// If the old app had autostart on, turn it on for this one and remove the old entry.
pub fn migrate_legacy_autostart() {
    if autostart_value(RUN_KEY, LEGACY_AUTOSTART_NAME).is_some() {
        let _ = set_autostart(true);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const TEST_KEY: &str = r"Software\BlinkTest\Run";

    #[test]
    fn autostart_value_round_trips_and_deletes() {
        let command = r#""C:\Program Files\Blink\blink.exe""#;
        set_autostart_in(TEST_KEY, "Blink", Some(command)).unwrap();
        assert_eq!(autostart_value(TEST_KEY, "Blink").as_deref(), Some(command));
        set_autostart_in(TEST_KEY, "Blink", None).unwrap();
        assert_eq!(autostart_value(TEST_KEY, "Blink"), None);
        // Removing again is not an error.
        set_autostart_in(TEST_KEY, "Blink", None).unwrap();
    }

    #[test]
    fn a_second_guard_for_the_same_tag_is_refused() {
        let first = single_instance("unit-test-tag");
        assert!(first.is_some());
        assert!(single_instance("unit-test-tag").is_none());
        drop(first);
        assert!(single_instance("unit-test-tag").is_some());
    }
}
