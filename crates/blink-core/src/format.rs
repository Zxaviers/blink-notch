//! Small text helpers for the pill and the footer (`format.js`).

use jiff::tz::TimeZone;

use crate::countdown::clock;
use crate::i18n::Translator;
use crate::ics::Task;
use crate::jsval::{is_js_space, js_trim, js_trim_end};

fn utf16_len(s: &str) -> usize {
    s.chars().map(char::len_utf16).sum()
}

/// Shorten to `max` UTF-16 units with an ellipsis, like the JS.
pub fn truncate(text: &str, max: usize) -> String {
    let s = js_trim(text);
    if utf16_len(s) <= max {
        return s.to_string();
    }
    let keep = max.saturating_sub(1);
    let mut kept = String::new();
    let mut units = 0;
    for c in s.chars() {
        if units + c.len_utf16() > keep {
            break;
        }
        units += c.len_utf16();
        kept.push(c);
    }
    format!("{}\u{2026}", js_trim_end(&kept))
}

/// A leading course code such as "CS101" or "MA 201": 2 to 6 letters, an
/// optional space and dash, 2 to 4 digits, an optional letter, then not
/// another letter or digit. Hand-rolled because the JS regex used a lookahead.
fn course_code(course: &str) -> Option<String> {
    let chars: Vec<char> = course.chars().collect();
    let n = chars.len();
    let mut i = 0;
    while i < n && chars[i].is_ascii_alphabetic() {
        i += 1;
    }
    if !(2..=6).contains(&i) {
        return None;
    }
    let letters: String = chars[..i].iter().collect();
    let mut j = i;
    if j < n && is_js_space(chars[j]) {
        j += 1;
    }
    if j < n && chars[j] == '-' {
        j += 1;
    }
    let digits_start = j;
    while j < n && chars[j].is_ascii_digit() {
        j += 1;
    }
    if !(2..=4).contains(&(j - digits_start)) {
        return None;
    }
    let digits: String = chars[digits_start..j].iter().collect();
    let mut suffix = String::new();
    if j < n && chars[j].is_ascii_alphabetic() {
        suffix.push(chars[j]);
        j += 1;
    }
    if j < n && chars[j].is_ascii_alphanumeric() {
        return None;
    }
    Some(format!("{letters}{digits}{suffix}").to_uppercase())
}

/// Short label for the resting pill: course code, else course name, else title.
pub fn pill_label(task: Option<&Task>, max: usize) -> String {
    let Some(task) = task else { return String::new() };
    let course = js_trim(task.course.as_deref().unwrap_or(""));
    if let Some(code) = course_code(course) {
        return code;
    }
    if !course.is_empty() {
        return truncate(course, max);
    }
    truncate(&task.title, max)
}

/// One feed's state, as the footer sees it.
#[derive(Debug, Clone, Default)]
pub struct FeedSync {
    pub fetched_at_ms: Option<f64>,
    pub syncing: bool,
    /// (error code, server never reached)
    pub error: Option<(String, bool)>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct SyncSummary {
    pub kind: &'static str,
    pub text: String,
}

pub fn sync_summary(feeds: &[FeedSync], t: &Translator, tz: &TimeZone) -> SyncSummary {
    let make = |kind, text: String| SyncSummary { kind, text };
    if feeds.is_empty() {
        return make("none", String::new());
    }
    if feeds.iter().any(|f| f.syncing) {
        return make("syncing", t.t("footer.syncing", &[]));
    }
    let last = feeds.iter().filter_map(|f| f.fetched_at_ms).filter(|m| m.is_finite()).fold(None, |a: Option<f64>, m| Some(a.map_or(m, |x| x.max(m))));
    if feeds.iter().any(|f| f.error.as_ref().is_some_and(|(_, transport)| *transport)) {
        return match last {
            None => make("offline", t.t("footer.never", &[])),
            Some(ms) => make("offline", t.t("footer.offline", &[("time", &clock(ms as i64, tz))])),
        };
    }
    if let Some((code, _)) = feeds.iter().find_map(|f| f.error.as_ref()) {
        return make("error", t.t(&format!("error.{code}"), &[]));
    }
    match last {
        None => make("never", t.t("footer.never", &[])),
        Some(ms) => make("ok", t.t("footer.synced", &[("time", &clock(ms as i64, tz))])),
    }
}