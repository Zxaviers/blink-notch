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