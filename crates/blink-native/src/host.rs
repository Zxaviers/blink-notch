//! Host-level configuration: which modules are on and in what order.
//!
//! Lives in its own file (`host.json`) so it never collides with the settings
//! file, whose shape the settings page and the WebView2 app both validate.
//!
//! ```json
//! { "modules": [ { "id": "deadlines", "enabled": true }, { "id": "clock", "enabled": true } ] }
//! ```
//!
//! A module the file does not mention keeps its default (on) and its place after
//! the ones that are listed, so a new version's new module shows up by itself.

use serde_json::Value;

/// The shortcut that shows or hides the whole notch (`visibilityHotkey` in `host.json`).
/// Empty turns it off.
pub const DEFAULT_VISIBILITY_HOTKEY: &str = "Ctrl+Alt+H";

pub fn parse_visibility_hotkey(value: Option<&Value>) -> String {
    match value.and_then(|v| v.get("visibilityHotkey")).and_then(Value::as_str) {
        Some(s) => s.trim().to_string(),
        None => DEFAULT_VISIBILITY_HOTKEY.to_string(),
    }
}

/// When the notch steps aside for a fullscreen app (`hideInFullscreen` in `host.json`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FullscreenMode {
    /// Only for games and presentations (the default): a browser, a chat app or a maximized
    /// window that covers the screen does not hide it.
    Games,
    /// For anything fullscreen, videos and browsers included.
    All,
    /// Never.
    Never,
}

pub fn parse_fullscreen_mode(value: Option<&Value>) -> FullscreenMode {
    match value.and_then(|v| v.get("hideInFullscreen")).and_then(Value::as_str) {
        Some("all") => FullscreenMode::All,
        Some("never") => FullscreenMode::Never,
        _ => FullscreenMode::Games,
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ModuleConfig {
    pub id: String,
    pub enabled: bool,
    /// Per-module settings (for the system peeks: which kinds to show). Null when none.
    pub options: Value,
}

/// Parse `host.json`'s content. Anything unusable gives an empty list (all defaults).
pub fn parse_modules(value: Option<&Value>) -> Vec<ModuleConfig> {
    let Some(Value::Array(items)) = value.and_then(|v| v.get("modules")) else { return Vec::new() };
    let mut out: Vec<ModuleConfig> = Vec::new();
    for item in items {
        let Some(id) = item.get("id").and_then(Value::as_str).map(str::trim).filter(|s| !s.is_empty()) else { continue };
        if out.iter().any(|m| m.id == id) {
            continue;
        }
        out.push(ModuleConfig {
            id: id.to_string(),
            enabled: item.get("enabled").and_then(Value::as_bool).unwrap_or(true),
            options: item.get("options").filter(|o| o.is_object()).cloned().unwrap_or(Value::Null),
        });
    }
    out
}

pub fn to_json(modules: &[ModuleConfig]) -> Value {
    serde_json::json!({ "modules": modules.iter().map(|m| {
        let mut o = serde_json::json!({ "id": m.id, "enabled": m.enabled });
        if !m.options.is_null() {
            o["options"] = m.options.clone();
        }
        o
    }).collect::<Vec<_>>() })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn order_and_flags_are_kept_and_junk_is_skipped() {
        let v = json!({ "modules": [ { "id": "clock", "enabled": false }, { "id": " deadlines " }, { "id": "clock" }, { "enabled": true }, 7 ] });
        assert_eq!(
            parse_modules(Some(&v)),
            vec![ModuleConfig { id: "clock".into(), enabled: false, options: Value::Null }, ModuleConfig { id: "deadlines".into(), enabled: true, options: Value::Null }]
        );
    }

    #[test]
    fn a_missing_or_broken_file_means_all_defaults() {
        assert!(parse_modules(None).is_empty());
        assert!(parse_modules(Some(&json!({ "modules": "no" }))).is_empty());
        assert!(parse_modules(Some(&json!([]))).is_empty());
    }

    #[test]
    fn the_visibility_shortcut_has_a_default_and_can_be_turned_off() {
        assert_eq!(parse_visibility_hotkey(None), "Ctrl+Alt+H");
        assert_eq!(parse_visibility_hotkey(Some(&json!({}))), "Ctrl+Alt+H");
        assert_eq!(parse_visibility_hotkey(Some(&json!({ "visibilityHotkey": " Ctrl+Shift+N " }))), "Ctrl+Shift+N");
        assert_eq!(parse_visibility_hotkey(Some(&json!({ "visibilityHotkey": "" }))), "", "empty means off");
    }

    #[test]
    fn fullscreen_hiding_defaults_to_games_only() {
        assert_eq!(parse_fullscreen_mode(None), FullscreenMode::Games);
        assert_eq!(parse_fullscreen_mode(Some(&json!({ "hideInFullscreen": "all" }))), FullscreenMode::All);
        assert_eq!(parse_fullscreen_mode(Some(&json!({ "hideInFullscreen": "never" }))), FullscreenMode::Never);
        assert_eq!(parse_fullscreen_mode(Some(&json!({ "hideInFullscreen": "banana" }))), FullscreenMode::Games);
    }

    #[test]
    fn it_round_trips() {
        let list = vec![ModuleConfig { id: "a".into(), enabled: true, options: Value::Null }, ModuleConfig { id: "b".into(), enabled: false, options: json!({ "x": 1 }) }];
        assert_eq!(parse_modules(Some(&to_json(&list))), list);
    }
}
