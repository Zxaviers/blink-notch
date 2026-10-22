//! Feed metadata (`feeds.js`). The URL itself never lives here: it goes to
//! Windows Credential Manager; this module only validates ids and kinds and
//! shapes the non-secret row kept in settings.json.

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use url::Url;

use crate::jsval::{js_string_or_empty, js_trim};

pub const FEED_KINDS: [&str; 3] = ["moodle", "google", "ics"];

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FeedMeta {
    pub id: String,
    pub kind: String,
    pub label: String,
    pub enabled: bool,
}

/// `^[A-Za-z0-9][A-Za-z0-9._-]{0,63}$` on the trimmed id.
pub fn is_valid_feed_id(id: &str) -> bool {
    let t = js_trim(id);
    let mut chars = t.chars();
    let Some(first) = chars.next() else { return false };
    t.chars().count() <= 64
        && first.is_ascii_alphanumeric()
        && chars.all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-'))
}

pub fn normalize_feed_kind(kind: &str) -> Option<&'static str> {
    let clean = js_trim(kind);
    FEED_KINDS.iter().copied().find(|k| *k == clean)
}

/// The trimmed text if it is an http(s) URL, else None.
fn normalize_url(raw: &str) -> Option<String> {
    let trimmed = js_trim(raw);
    if trimmed.is_empty() {
        return None;
    }
    let parsed = Url::parse(trimmed).ok()?;
    matches!(parsed.scheme(), "http" | "https").then(|| trimmed.to_string())
}

/// Result shape matches the JS: `{ok: true, feed}` or `{ok: false, error}`.
pub fn normalize_feed(input: &Value) -> Value {
    let field = |k: &str| input.get(k);
    let id = js_trim(&js_string_or_empty(field("id"))).to_string();
    if !is_valid_feed_id(&id) {
        return json!({ "ok": false, "error": "ID_INVALID" });
    }
    let Some(kind) = normalize_feed_kind(&js_string_or_empty(field("kind"))) else {
        return json!({ "ok": false, "error": "KIND_INVALID" });
    };
    let Some(url) = normalize_url(&js_string_or_empty(field("url"))) else {
        return json!({ "ok": false, "error": "URL_INVALID" });
    };
    let label = js_trim(&js_string_or_empty(field("label"))).to_string();
    let label = if label.is_empty() { id.clone() } else { label };
    json!({
        "ok": true,
        "feed": {
            "id": id, "kind": kind, "label": label, "url": url,
            "enabled": !matches!(field("enabled"), Some(Value::Bool(false)))
        }
    })
}

/// Strip the secret URL, leaving the row safe for settings.json.
pub fn feed_meta_from(feed: &Value) -> Value {
    json!({
        "id": feed.get("id").cloned().unwrap_or(Value::Null),
        "kind": feed.get("kind").cloned().unwrap_or(Value::Null),
        "label": feed.get("label").cloned().unwrap_or(Value::Null),
        "enabled": !matches!(feed.get("enabled"), Some(Value::Bool(false)))
    })
}

/// Upsert a metadata row by id. Inputs are not mutated.
pub fn upsert_feed_meta(metas: &Value, meta: &Value) -> Value {
    let mut list: Vec<Value> = metas.as_array().cloned().unwrap_or_default();
    let id = meta.get("id");
    match list.iter().position(|m| m.get("id") == id && m.is_object()) {
        Some(at) => {
            if let (Some(base), Some(extra)) = (list[at].as_object_mut(), meta.as_object()) {
                for (k, v) in extra {
                    base.insert(k.clone(), v.clone());
                }
            }
        }
        None => list.push(meta.clone()),
    }
    Value::Array(list)
}