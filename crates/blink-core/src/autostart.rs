//! Start with Windows: one value under the user's Run key. Shared by the notch
//! and the settings program, so both agree on the name and the target.
//!
//! The WebView2 app registered itself as "Deadline Notch"; turning autostart on
//! or off here also removes that entry, so an upgraded install never launches
//! both programs.

use windows::core::PCWSTR;
use windows::Win32::System::Registry::{
    RegCloseKey, RegCreateKeyExW, RegDeleteValueW, RegGetValueW, RegSetValueExW, HKEY, HKEY_CURRENT_USER, KEY_SET_VALUE,
    REG_OPTION_NON_VOLATILE, REG_SZ, RRF_RT_REG_SZ,
};

pub const RUN_KEY: &str = r"Software\Microsoft\Windows\CurrentVersion\Run";
pub const AUTOSTART_NAME: &str = "Blink";
/// What the WebView2 app registered itself as.
pub const LEGACY_AUTOSTART_NAME: &str = "Deadline Notch";

/// The key to use: tests point `BLINK_RUN_KEY` at a scratch key so they never touch the real one.
fn run_key() -> String {
    std::env::var("BLINK_RUN_KEY").ok().filter(|k| !k.is_empty()).unwrap_or_else(|| RUN_KEY.to_string())
}

fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(Some(0)).collect()
}

pub fn value_in(run_key: &str, name: &str) -> Option<String> {
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

/// Set (`Some(command)`) or delete (`None`) one value.
pub fn set_in(run_key: &str, name: &str, command: Option<&str>) -> Result<(), String> {
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

/// Where Windows should start the notch from: `blink.exe` next to the running program.
fn notch_exe() -> Result<std::path::PathBuf, String> {
    let me = std::env::current_exe().map_err(|e| e.to_string())?;
    let name = me.file_name().and_then(|n| n.to_str()).unwrap_or("");
    // The settings program starts the notch, not itself.
    Ok(if name.eq_ignore_ascii_case("blink-settings.exe") { me.with_file_name("blink.exe") } else { me })
}

/// Start with Windows (or stop). Also clears the old app's entry.
pub fn set_enabled(enabled: bool) -> Result<(), String> {
    let key = run_key();
    let command = format!("\"{}\"", notch_exe()?.display());
    set_in(&key, LEGACY_AUTOSTART_NAME, None)?;
    set_in(&key, AUTOSTART_NAME, enabled.then_some(command.as_str()))
}

pub fn is_enabled() -> bool {
    value_in(&run_key(), AUTOSTART_NAME).is_some()
}

/// If the old app had autostart on, turn it on for this one and remove the old entry.
pub fn migrate_legacy() {
    if value_in(&run_key(), LEGACY_AUTOSTART_NAME).is_some() {
        let _ = set_enabled(true);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const TEST_KEY: &str = r"Software\BlinkTest\RunCore";

    #[test]
    fn a_value_round_trips_and_deleting_twice_is_fine() {
        let command = r#""C:\Program Files\Blink\blink.exe""#;
        set_in(TEST_KEY, "Blink", Some(command)).unwrap();
        assert_eq!(value_in(TEST_KEY, "Blink").as_deref(), Some(command));
        set_in(TEST_KEY, "Blink", None).unwrap();
        assert_eq!(value_in(TEST_KEY, "Blink"), None);
        set_in(TEST_KEY, "Blink", None).unwrap();
    }
}
