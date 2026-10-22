//! Feed URL checks and refresh timing (`feedUrl.js`).
//!
//! The URL carries a private access token: nothing here returns more than host
//! and path except `validate_url`, which hands the cleaned value back.

use serde_json::Value;
use url::Url;

pub const MINUTE_MS: f64 = 60.0 * 1000.0;
/// Hard floor: never poll a campus server faster than this.
pub const MIN_REFRESH_MS: f64 = 15.0 * MINUTE_MS;
pub const DEFAULT_REFRESH_MS: f64 = 20.0 * MINUTE_MS;
pub const MAX_REFRESH_MS: f64 = 30.0 * MINUTE_MS;
/// Retry ladder for failures where the request never reached the server.
pub const OFFLINE_RETRY_LADDER_MS: [f64; 4] = [MINUTE_MS, 2.0 * MINUTE_MS, 4.0 * MINUTE_MS, 8.0 * MINUTE_MS];

pub fn clamp_interval(value: f64) -> f64 {
    if !value.is_finite() {
        return DEFAULT_REFRESH_MS;
    }
    value.clamp(MIN_REFRESH_MS, MAX_REFRESH_MS)
}

/// `scheme://host[:port]/path`, never the query string (it holds the token).
pub fn describe_url(url: &str) -> String {
    match Url::parse(url) {
        Ok(u) => {
            let host = u.host_str().unwrap_or("");
            let port = u.port().map(|p| format!(":{p}")).unwrap_or_default();
            format!("{}://{}{}{}", u.scheme(), host, port, u.path())
        }
        Err(_) => "<invalid url>".to_string(),
    }
}

/// Error codes: URL_MISSING, URL_INVALID, URL_PROTOCOL.
pub fn validate_url(raw: Option<&Value>) -> Result<String, &'static str> {
    let Some(Value::String(s)) = raw else { return Err("URL_MISSING") };
    let trimmed = crate::jsval::js_trim(s);
    if trimmed.is_empty() {
        return Err("URL_MISSING");
    }
    let parsed = Url::parse(trimmed).map_err(|_| "URL_INVALID")?;
    if !matches!(parsed.scheme(), "http" | "https") {
        return Err("URL_PROTOCOL");
    }
    Ok(parsed.to_string())
}

/// Delay before the next refresh attempt.
pub fn next_refresh_delay_ms(refresh_ms: f64, ok: bool, transport_failures: u32) -> f64 {
    let interval = clamp_interval(refresh_ms);
    if ok || transport_failures == 0 || transport_failures as usize > OFFLINE_RETRY_LADDER_MS.len() {
        return interval;
    }
    OFFLINE_RETRY_LADDER_MS[transport_failures as usize - 1]
}

/// Sanity-check the shape of a Moodle export URL before spending a request.
/// Advisory only. Returns (ok, hint code for the UI to translate).
pub fn inspect_moodle_url(raw: &str) -> (bool, Option<&'static str>) {
    let Ok(parsed) = Url::parse(crate::jsval::js_trim(raw)) else { return (false, None) };
    let path = parsed.path().to_lowercase();
    let is_export = path.contains("export_execute.php");
    let has_token = parsed.query_pairs().any(|(k, _)| k == "authtoken");
    if is_export && has_token {
        return (true, None);
    }
    let known: [(&dyn Fn(&str) -> bool, &str); 5] = [
        (&|p| p.ends_with("/my") || p.ends_with("/my/"), "moodle_dashboard"),
        (&|p| p.contains("/calendar/view.php"), "moodle_calendar_page"),
        (&|p| p.contains("/course/view.php"), "moodle_course_page"),
        (&|p| p.contains("/mod/"), "moodle_activity_page"),
        (&|p| p == "/" || p.is_empty(), "moodle_home"),
    ];
    for (matches, hint) in known {
        if matches(&path) {
            return (false, Some(hint));
        }
    }
    (false, Some(if is_export { "moodle_missing_token" } else { "moodle_not_export" }))
}