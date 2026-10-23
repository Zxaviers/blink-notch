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
    fn it_round_trips() {
        let list = vec![ModuleConfig { id: "a".into(), enabled: true, options: Value::Null }, ModuleConfig { id: "b".into(), enabled: false, options: json!({ "x": 1 }) }];
        assert_eq!(parse_modules(Some(&to_json(&list))), list);
    }
}
