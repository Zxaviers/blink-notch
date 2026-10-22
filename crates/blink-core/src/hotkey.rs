//! Global hotkey accelerators (`hotkey.js`, the validation half). Building an
//! accelerator from a key press stays in the settings page; the native side
//! only needs to judge and later parse the stored text.

use serde_json::Value;

pub const DEFAULT_HOTKEY: &str = "Ctrl+Alt+D";

const MODIFIERS: [&str; 4] = ["Ctrl", "Alt", "Shift", "Super"];
const STRONG: [&str; 3] = ["Ctrl", "Alt", "Super"];
const NAMED_KEYS: [&str; 14] = [
    "Space", "Enter", "Tab", "Backspace", "Delete", "Insert", "Home", "End", "PageUp", "PageDown",
    "ArrowUp", "ArrowDown", "ArrowLeft", "ArrowRight",
];

fn is_function_key(key: &str) -> bool {
    key.strip_prefix('F')
        .and_then(|n| n.parse::<u8>().ok())
        .is_some_and(|n| (1..=24).contains(&n) && n.to_string() == key[1..])
}

/// '' means "hotkey off" and is valid. Needs Ctrl, Alt or Windows plus a key.
pub fn is_valid_accelerator(text: Option<&Value>) -> bool {
    let Some(Value::String(s)) = text else { return false };
    if s.is_empty() {
        return true;
    }
    let mut parts: Vec<&str> = s.split('+').collect();
    let key = parts.pop().unwrap_or("");
    if parts.is_empty() || parts.len() > MODIFIERS.len() {
        return false;
    }
    let mut seen = std::collections::HashSet::new();
    if !parts.iter().all(|m| seen.insert(*m)) {
        return false;
    }
    if !parts.iter().all(|m| MODIFIERS.contains(m)) || !parts.iter().any(|m| STRONG.contains(m)) {
        return false;
    }
    let single = key.len() == 1 && key.chars().all(|c| c.is_ascii_uppercase() || c.is_ascii_digit());
    single || is_function_key(key) || NAMED_KEYS.contains(&key)
}

/// Modifier flags in the values `RegisterHotKey` expects.
pub const MOD_ALT: u32 = 0x0001;
pub const MOD_CONTROL: u32 = 0x0002;
pub const MOD_SHIFT: u32 = 0x0004;
pub const MOD_WIN: u32 = 0x0008;

/// A stored accelerator turned into what Windows needs: (modifier flags, virtual-key code).
/// None for "" (hotkey off) or anything `is_valid_accelerator` would reject.
pub fn parse_accelerator(text: &str) -> Option<(u32, u32)> {
    if text.is_empty() || !is_valid_accelerator(Some(&Value::String(text.to_string()))) {
        return None;
    }
    let mut parts: Vec<&str> = text.split('+').collect();
    let key = parts.pop()?;
    let mods = parts.iter().fold(0, |acc, m| {
        acc | match *m {
            "Ctrl" => MOD_CONTROL,
            "Alt" => MOD_ALT,
            "Shift" => MOD_SHIFT,
            "Super" => MOD_WIN,
            _ => 0,
        }
    });
    Some((mods, virtual_key(key)?))
}

fn virtual_key(key: &str) -> Option<u32> {
    if key.len() == 1 {
        let c = key.as_bytes()[0];
        return (c.is_ascii_uppercase() || c.is_ascii_digit()).then_some(u32::from(c)); // VK_A..VK_Z, VK_0..VK_9 equal ASCII
    }
    if let Some(n) = key.strip_prefix('F').and_then(|n| n.parse::<u32>().ok()) {
        return (1..=24).contains(&n).then_some(0x6F + n); // VK_F1 = 0x70
    }
    Some(match key {
        "Backspace" => 0x08,
        "Tab" => 0x09,
        "Enter" => 0x0D,
        "Space" => 0x20,
        "PageUp" => 0x21,
        "PageDown" => 0x22,
        "End" => 0x23,
        "Home" => 0x24,
        "ArrowLeft" => 0x25,
        "ArrowUp" => 0x26,
        "ArrowRight" => 0x27,
        "ArrowDown" => 0x28,
        "Insert" => 0x2D,
        "Delete" => 0x2E,
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_the_default_and_common_forms() {
        assert_eq!(parse_accelerator("Ctrl+Alt+D"), Some((MOD_CONTROL | MOD_ALT, 0x44)));
        assert_eq!(parse_accelerator("Super+F12"), Some((MOD_WIN, 0x7B)));
        assert_eq!(parse_accelerator("Ctrl+Shift+ArrowUp"), Some((MOD_CONTROL | MOD_SHIFT, 0x26)));
        assert_eq!(parse_accelerator("Alt+Space"), Some((MOD_ALT, 0x20)));
        assert_eq!(parse_accelerator("Ctrl+9"), Some((MOD_CONTROL, 0x39)));
        assert_eq!(parse_accelerator("Ctrl+F1"), Some((MOD_CONTROL, 0x70)));
        assert_eq!(parse_accelerator("Ctrl+F24"), Some((MOD_CONTROL, 0x87)));
    }

    #[test]
    fn off_and_invalid_give_none() {
        for bad in ["", "D", "Shift+D", "Ctrl+", "Win+D", "Ctrl+F25", "Ctrl+dd"] {
            assert_eq!(parse_accelerator(bad), None, "{bad:?}");
        }
    }

    #[test]
    fn every_valid_key_name_has_a_virtual_key() {
        for name in ["Space", "Enter", "Tab", "Backspace", "Delete", "Insert", "Home", "End", "PageUp", "PageDown", "ArrowUp", "ArrowDown", "ArrowLeft", "ArrowRight"] {
            assert!(parse_accelerator(&format!("Ctrl+{name}")).is_some(), "{name}");
        }
        for c in ('A'..='Z').chain('0'..='9') {
            assert_eq!(parse_accelerator(&format!("Alt+{c}")).unwrap().1, c as u32);
        }
    }
}