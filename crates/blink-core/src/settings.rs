//! Defaults and validation for settings.json (`settings.js`). Whatever is read
//! from disk or a form goes through `normalize_settings`, so the rest of the app
//! only sees clean values. Feed URLs are never part of settings.

use serde::Serialize;
use serde_json::Value;

use crate::feed_url::{clamp_interval, DEFAULT_REFRESH_MS, MINUTE_MS};
use crate::feeds::{is_valid_feed_id, FeedMeta, FEED_KINDS};
use crate::hotkey::{is_valid_accelerator, DEFAULT_HOTKEY};
use crate::jsval::{js_number, js_round, js_trim};
use crate::thresholds::normalize_thresholds_hours;

pub const THEMES: [&str; 3] = ["dark", "light", "auto"];
pub const LANGUAGE_SETTINGS: [&str; 3] = ["auto", "en", "id"];
pub const RESTING_STYLES: [&str; 3] = ["always", "when-due", "hover"];

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Settings {
    pub version: u32,
    pub language: String,
    pub theme: String,
    pub resting_style: String,
    pub due_horizon_hours: i64,
    pub hover_delay_ms: i64,
    pub refresh_minutes: f64,
    pub thresholds_hours: Vec<f64>,
    pub toast: bool,
    pub hotkey: String,
    pub keep_overdue_days: i64,
    pub monitor: i64,
    pub offset_x: i64,
    pub onboarded: bool,
    pub feeds: Vec<FeedMeta>,
}

impl Default for Settings {
    fn default() -> Self {
        normalize_settings(None)
    }
}

fn pick(value: Option<&Value>, allowed: &[&str], fallback: &str) -> String {
    match value {
        Some(Value::String(s)) if allowed.contains(&s.as_str()) => s.clone(),
        _ => fallback.to_string(),
    }
}

fn int(value: Option<&Value>, min: i64, max: i64, fallback: i64) -> i64 {
    let n = js_round(js_number(value));
    if !n.is_finite() {
        return fallback;
    }
    (n.clamp(min as f64, max as f64)) as i64
}

fn normalize_feed_rows(rows: Option<&Value>) -> Vec<FeedMeta> {
    let Some(Value::Array(rows)) = rows else { return Vec::new() };
    let mut out: Vec<FeedMeta> = Vec::new();
    for row in rows {
        let Some(id) = row.get("id").and_then(Value::as_str) else { continue };
        let Some(kind) = row.get("kind").and_then(Value::as_str) else { continue };
        if !is_valid_feed_id(id) || !FEED_KINDS.contains(&kind) || out.iter().any(|f| f.id == id) {
            continue;
        }
        let label = js_trim(&crate::jsval::js_string_or_empty(row.get("label"))).chars().take(60).collect::<String>();
        let label = js_trim(&label).to_string();
        out.push(FeedMeta {
            id: id.to_string(),
            kind: kind.to_string(),
            label: if label.is_empty() { id.to_string() } else { label },
            enabled: !matches!(row.get("enabled"), Some(Value::Bool(false))),
        });
    }
    out
}

pub fn normalize_settings(raw: Option<&Value>) -> Settings {
    let empty = Value::Object(Default::default());
    let s = match raw {
        Some(v @ Value::Object(_)) => v,
        _ => &empty,
    };
    let get = |k: &str| s.get(k);

    let refresh_ms = js_number(get("refreshMinutes")) * MINUTE_MS;
    Settings {
        version: 1,
        language: pick(get("language"), &LANGUAGE_SETTINGS, "auto"),
        theme: pick(get("theme"), &THEMES, "dark"),
        resting_style: pick(get("restingStyle"), &RESTING_STYLES, "always"),
        due_horizon_hours: int(get("dueHorizonHours"), 1, 336, 72),
        hover_delay_ms: int(get("hoverDelayMs"), 0, 1000, 150),
        refresh_minutes: if refresh_ms.is_nan() { DEFAULT_REFRESH_MS / MINUTE_MS } else { clamp_interval(refresh_ms) / MINUTE_MS },
        thresholds_hours: normalize_thresholds_hours(get("thresholdsHours")),
        toast: !matches!(get("toast"), Some(Value::Bool(false))),
        hotkey: if is_valid_accelerator(get("hotkey")) {
            get("hotkey").and_then(Value::as_str).unwrap_or(DEFAULT_HOTKEY).to_string()
        } else {
            DEFAULT_HOTKEY.to_string()
        },
        keep_overdue_days: int(get("keepOverdueDays"), 0, 30, 3),
        monitor: int(get("monitor"), 0, 15, 0),
        offset_x: int(get("offsetX"), -2000, 2000, 0),
        onboarded: matches!(get("onboarded"), Some(Value::Bool(true))),
        feeds: normalize_feed_rows(get("feeds")),
    }
}