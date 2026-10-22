//! Notification threshold normalization (`notifyConfig.js`).

use serde_json::Value;

use crate::jsval::js_number;

/// Hard sanity ceiling: two weeks.
const MAX_HOURS: f64 = 336.0;

pub fn default_thresholds() -> Vec<f64> {
    vec![24.0, 6.0, 1.0]
}

/// Hours sorted descending, deduplicated, finite, in (0, 336]. Anything unusable
/// falls back to the defaults.
pub fn normalize_thresholds_hours(value: Option<&Value>) -> Vec<f64> {
    let Some(Value::Array(items)) = value else {
        return default_thresholds();
    };
    let mut cleaned: Vec<f64> = Vec::new();
    for item in items {
        let n = js_number(Some(item));
        if n.is_finite() && n > 0.0 && n <= MAX_HOURS && !cleaned.contains(&n) {
            cleaned.push(n);
        }
    }
    cleaned.sort_by(|a, b| b.total_cmp(a));
    if cleaned.is_empty() { default_thresholds() } else { cleaned }
}

pub fn thresholds_to_ms(hours: &[f64]) -> Vec<f64> {
    hours.iter().map(|h| h * 3600.0 * 1000.0).collect()
}