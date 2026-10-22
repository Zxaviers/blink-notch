//! iCalendar text to a sorted task list.
//!
//! Port of `src/lib/parseTasks.js`. Same shape of data, same rules: VEVENT
//! blocks only, TZID / UTC / floating / DATE starts, RFC 5545 line folding and
//! text escapes, Moodle's "... is due" summary suffixes, no RRULE expansion.
//!
//! Known, accepted differences from the JavaScript version (see the plan):
//! - Titles that tie on the due time are ordered by a simple case-insensitive
//!   comparison, not ICU collation.
//! - A description cut inside a surrogate pair drops the half pair; JS would
//!   keep a lone surrogate, which a Rust `String` cannot hold.
//! - Years 0 to 99 follow `Date`'s 1900s quirk, as JS does.

use std::collections::HashMap;
use std::sync::OnceLock;

use jiff::civil::DateTime;
use jiff::tz::TimeZone;
use jiff::Timestamp;
use regex_lite::Regex;
use serde::{Deserialize, Serialize};

use crate::jsval::{is_js_space, js_trim, js_trim_end};

pub const DAY_MS: i64 = 24 * 60 * 60 * 1000;
/// How far into the past an already-passed deadline stays visible.
pub const DEFAULT_KEEP_OVERDUE_MS: i64 = 3 * DAY_MS;
/// How far into the future to look.
pub const DEFAULT_HORIZON_MS: i64 = 120 * DAY_MS;

const DESCRIPTION_MAX_LEN: usize = 240;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase", default)]
pub struct Task {
    pub id: String,
    pub uid: String,
    pub title: String,
    pub course: Option<String>,
    pub phase: String,
    /// ISO 8601 in UTC with milliseconds, like JavaScript's `toISOString()`.
    pub due: String,
    pub due_ms: i64,
    pub all_day: bool,
    pub description: String,
    pub url: Option<String>,
    /// "local" for personal events; absent for feed tasks.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source: Option<String>,
    /// Which feed a merged task came from.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub feed_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub feed_kind: Option<String>,
}

#[derive(Debug, Clone)]
pub struct ParseOptions {
    pub now_ms: i64,
    pub keep_overdue_ms: i64,
    pub horizon_ms: i64,
    pub limit: Option<usize>,
}

impl ParseOptions {
    pub fn new(now_ms: i64) -> Self {
        ParseOptions {
            now_ms,
            keep_overdue_ms: DEFAULT_KEEP_OVERDUE_MS,
            horizon_ms: DEFAULT_HORIZON_MS,
            limit: None,
        }
    }
}

// ------------------------------------------------------------------ text

/// Collapse runs of whitespace to one space and trim, like
/// `.replace(/\s+/g, ' ').trim()`.
fn collapse_whitespace(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut in_space = false;
    for c in s.chars() {
        if is_js_space(c) {
            in_space = true;
        } else {
            if in_space {
                out.push(' ');
                in_space = false;
            }
            out.push(c);
        }
    }
    // A trailing run was never flushed, a leading run is trimmed below.
    js_trim(&out).to_string()
}

fn re(cell: &'static OnceLock<Regex>, pattern: &str) -> &'static Regex {
    cell.get_or_init(|| Regex::new(pattern).expect("valid regex"))
}

fn html_entity(name: &str) -> &'static str {
    match name {
        "&amp;" => "&",
        "&lt;" => "<",
        "&gt;" => ">",
        "&quot;" => "\"",
        "&#39;" | "&apos;" => "'",
        "&nbsp;" => " ",
        "&hellip;" => "\u{2026}",
        "&ndash;" => "\u{2013}",
        "&mdash;" => "\u{2014}",
        _ => "",
    }
}

/// Strip the HTML tags and entities Moodle puts in DESCRIPTION and normalize space.
pub fn to_plain_text(value: &str) -> String {
    if value.is_empty() {
        return String::new();
    }
    static BR: OnceLock<Regex> = OnceLock::new();
    static BLOCK: OnceLock<Regex> = OnceLock::new();
    static TAG: OnceLock<Regex> = OnceLock::new();
    static NUMERIC: OnceLock<Regex> = OnceLock::new();
    static ENTITY: OnceLock<Regex> = OnceLock::new();

    let s = re(&BR, r"(?i)<br\s*/?>").replace_all(value, " ");
    let s = re(&BLOCK, r"(?i)</(?:p|div|li|h[1-6])>").replace_all(&s, " ");
    let s = re(&TAG, r"<[^>]*>").replace_all(&s, "");
    let s = re(&NUMERIC, r"&#(\d+);").replace_all(&s, |caps: &regex_lite::Captures| {
        let n: u64 = caps[1].parse().unwrap_or(0);
        if n > 0 && n < 0x10ffff {
            char::from_u32(n as u32).map(String::from).unwrap_or_default()
        } else {
            String::new()
        }
    });
    let s = re(&ENTITY, r"(?i)&[a-z]+;|&#\d+;").replace_all(&s, |caps: &regex_lite::Captures| {
        html_entity(&caps[0].to_ascii_lowercase()).to_string()
    });
    collapse_whitespace(&s)
}

fn utf16_len(s: &str) -> usize {
    s.chars().map(char::len_utf16).sum()
}

fn truncate(value: &str, max: usize) -> String {
    if utf16_len(value) <= max {
        return value.to_string();
    }
    let mut kept = String::new();
    let mut units = 0;
    for c in value.chars() {
        if units + c.len_utf16() > max - 1 {
            break;
        }
        units += c.len_utf16();
        kept.push(c);
    }
    format!("{}\u{2026}", js_trim_end(&kept))
}

/// Moodle encodes the meaning of an event in a suffix on SUMMARY, e.g.
/// "Tugas 1 is due". Longest patterns first so "is due for grading" wins.
fn summary_suffixes() -> &'static [(Regex, &'static str)] {
    static CELL: OnceLock<Vec<(Regex, &'static str)>> = OnceLock::new();
    CELL.get_or_init(|| {
        [
            (r"(?i)\s+is\s+due\s+for\s+grading$", "grading"),
            (r"(?i)\s+should\s+be\s+submitted$", "due"),
            (r"(?i)\s+harus\s+diserahkan$", "due"),
            (r"(?i)\s+is\s+due$", "due"),
            (r"(?i)\s+tenggat$", "due"),
            (r"(?i)\s+closes$", "closes"),
            (r"(?i)\s+ditutup$", "closes"),
            (r"(?i)\s+opens$", "opens"),
            (r"(?i)\s+dibuka$", "opens"),
        ]
        .into_iter()
        .map(|(p, phase)| (Regex::new(p).expect("valid regex"), phase))
        .collect()
    })
}

/// "Tugas 1 is due" becomes ("Tugas 1", "due").
pub fn split_summary(raw: &str) -> (String, String) {
    let summary = to_plain_text(raw);
    for (re, phase) in summary_suffixes() {
        if re.is_match(&summary) {
            let title = js_trim(&re.replace(&summary, "")).to_string();
            // Never return an empty title just because the suffix ate everything.
            return if title.is_empty() { (summary, phase.to_string()) } else { (title, phase.to_string()) };
        }
    }
    (summary, "event".to_string())
}

// ------------------------------------------------------------- ics parse

/// Unfold per RFC 5545: a CRLF followed by a space or tab is a continuation.
fn unfold_lines(text: &str) -> Vec<String> {
    let t = text
        .replace("\r\n ", "")
        .replace("\r\n\t", "")
        .replace("\n ", "")
        .replace("\n\t", "");
    let parts: Vec<&str> = t.split('\n').collect();
    let last = parts.len() - 1;
    parts
        .iter()
        .enumerate()
        .map(|(i, p)| if i < last { p.strip_suffix('\r').unwrap_or(p).to_string() } else { p.to_string() })
        .collect()
}

struct Prop {
    params: HashMap<String, String>,
    value: String,
}

/// Split one content line into (name, params, value). The value separator is
/// the first ':' that is not inside a double-quoted parameter.
fn parse_content_line(line: &str) -> Option<(String, HashMap<String, String>, String)> {
    let mut in_quotes = false;
    let mut colon_at = None;
    for (i, ch) in line.char_indices() {
        if ch == '"' {
            in_quotes = !in_quotes;
        } else if ch == ':' && !in_quotes {
            colon_at = Some(i);
            break;
        }
    }
    let colon_at = colon_at?;
    let head = &line[..colon_at];
    let value = &line[colon_at + 1..];

    let mut parts = head.split(';');
    let name = parts.next().unwrap_or("").to_uppercase();
    let mut params = HashMap::new();
    for raw in parts {
        let Some(eq) = raw.find('=') else { continue };
        let mut v = raw[eq + 1..].to_string();
        if v.starts_with('"') {
            v.remove(0);
        }
        if v.ends_with('"') {
            v.pop();
        }
        params.insert(raw[..eq].to_uppercase(), v);
    }
    Some((name, params, value.to_string()))
}

/// Decode TEXT escapes per RFC 5545 section 3.3.11.
fn unescape_text(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    let mut chars = value.chars();
    while let Some(ch) = chars.next() {
        if ch == '\\' {
            match chars.next() {
                Some('n') | Some('N') => out.push('\n'),
                Some(next) => out.push(next),
                None => out.push('\\'),
            }
        } else {
            out.push(ch);
        }
    }
    out
}

type Event = HashMap<String, Vec<Prop>>;

fn starts_with_ignore_case(line: &str, prefix: &str) -> bool {
    line.get(..prefix.len()).is_some_and(|p| p.eq_ignore_ascii_case(prefix))
}

fn extract_events(text: &str) -> Vec<Event> {
    let mut events = Vec::new();
    let mut current: Option<Event> = None;
    for line in unfold_lines(text) {
        if starts_with_ignore_case(&line, "BEGIN:VEVENT") {
            current = Some(Event::new());
            continue;
        }
        if starts_with_ignore_case(&line, "END:VEVENT") {
            if let Some(ev) = current.take() {
                events.push(ev);
            }
            continue;
        }
        let Some(ev) = current.as_mut() else { continue };
        let Some((name, params, value)) = parse_content_line(&line) else { continue };
        ev.entry(name).or_default().push(Prop { params, value });
    }
    events
}

fn first_prop<'a>(ev: &'a Event, name: &str) -> Option<&'a Prop> {
    ev.get(name).and_then(|v| v.first())
}

// ---------------------------------------------------------------- time

/// `Date.UTC` semantics: month and day overflow roll over, years 0 to 99 mean 1900s.
pub(crate) fn utc_ms(year: i64, month0: i64, day: i64, hour: i64, minute: i64, second: i64) -> Option<i64> {
    let mut year = year;
    if (0..=99).contains(&year) {
        year += 1900;
    }
    let year = year + month0.div_euclid(12);
    let month = month0.rem_euclid(12) + 1;
    let first = DateTime::new(i16::try_from(year).ok()?, month as i8, 1, 0, 0, 0, 0).ok()?;
    let base = TimeZone::UTC.to_timestamp(first).ok()?.as_millisecond();
    Some(base + (day - 1) * DAY_MS + hour * 3_600_000 + minute * 60_000 + second * 1000)
}

fn civil_from_wall_ms(wall_ms: i64) -> Option<DateTime> {
    let ts = Timestamp::from_millisecond(wall_ms).ok()?;
    Some(ts.to_zoned(TimeZone::UTC).datetime())
}

/// A wall-clock time in `tz`, resolved like JavaScript: a gap moves forward, an
/// ambiguous time takes the earlier instant.
pub(crate) fn local_ms(tz: &TimeZone, wall_ms: i64) -> Option<i64> {
    let dt = civil_from_wall_ms(wall_ms)?;
    Some(tz.to_timestamp(dt).ok()?.as_millisecond())
}

fn offset_ms(tz: &TimeZone, utc: i64) -> Option<i64> {
    let ts = Timestamp::from_millisecond(utc).ok()?;
    Some(i64::from(tz.to_offset(ts).seconds()) * 1000)
}

struct ParsedDate {
    ms: i64,
    date_only: bool,
}

fn parse_ics_date(
    value: &str,
    params: &HashMap<String, String>,
    local: &TimeZone,
    zones: &mut HashMap<String, Option<TimeZone>>,
) -> Option<ParsedDate> {
    static DATE: OnceLock<Regex> = OnceLock::new();
    static ALL_DIGITS: OnceLock<Regex> = OnceLock::new();
    let raw = js_trim(value);
    let date_only = params.get("VALUE").is_some_and(|v| v == "DATE") || re(&ALL_DIGITS, r"^\d{8}$").is_match(raw);

    let caps = re(&DATE, r"^(\d{4})(\d{2})(\d{2})(?:T(\d{2})(\d{2})(\d{2})(Z)?)?").captures(raw)?;
    let num = |i: usize| caps.get(i).map_or(0, |m| m.as_str().parse::<i64>().unwrap_or(0));
    let (year, month0, day) = (num(1), num(2) - 1, num(3));
    let (hour, minute, second) = (num(4), num(5), num(6));
    let z = caps.get(7).is_some();

    if date_only {
        let wall = utc_ms(year, month0, day, 0, 0, 0)?;
        return Some(ParsedDate { ms: local_ms(local, wall)?, date_only: true });
    }

    let wall = utc_ms(year, month0, day, hour, minute, second)?;
    let ms = if z {
        wall
    } else if let Some(tzid) = params.get("TZID") {
        let tz = zones
            .entry(tzid.clone())
            .or_insert_with(|| TimeZone::get(tzid).ok())
            .as_ref()?;
        // Two passes, as in the JS: guess the offset at the wall time read as UTC,
        // then correct it at the guessed instant (handles daylight-saving edges).
        let offset = offset_ms(tz, wall)?;
        let mut ms = wall - offset;
        let refined = offset_ms(tz, ms)?;
        if refined != offset {
            ms = wall - refined;
        }
        ms
    } else {
        local_ms(local, wall)?
    };
    Some(ParsedDate { ms, date_only: false })
}

/// For an all-day event the deadline is the end of that day, not 00:00.
fn end_of_local_day(tz: &TimeZone, midnight_ms: i64) -> Option<i64> {
    let date = Timestamp::from_millisecond(midnight_ms).ok()?.to_zoned(tz.clone()).date();
    let dt = date.at(23, 59, 59, 999_000_000);
    Some(tz.to_timestamp(dt).ok()?.as_millisecond())
}

/// `Date.prototype.toISOString` for an in-range instant.
pub fn iso_string(ms: i64) -> String {
    let z = Timestamp::from_millisecond(ms)
        .map(|t| t.to_zoned(TimeZone::UTC))
        .expect("instant in range");
    format!(
        "{:04}-{:02}-{:02}T{:02}:{:02}:{:02}.{:03}Z",
        z.year(),
        z.month(),
        z.day(),
        z.hour(),
        z.minute(),
        z.second(),
        ms.rem_euclid(1000)
    )
}

// ---------------------------------------------------------------- tasks

/// Course name. Moodle puts it in CATEGORIES; odd feeds mention it as "Course: X".
fn extract_course(ev: &Event) -> Option<String> {
    static SUFFIX_YEAR: OnceLock<Regex> = OnceLock::new();
    static IN_TEXT: OnceLock<Regex> = OnceLock::new();
    for entry in ev.get("CATEGORIES").into_iter().flatten() {
        let mut text = to_plain_text(&unescape_text(&entry.value));
        if let Some(i) = text.find('|') {
            text.truncate(i);
        }
        let text = re(&SUFFIX_YEAR, r"_\d{4}$").replace(&text, "");
        let text = js_trim(&text);
        if !text.is_empty() {
            return Some(text.to_string());
        }
    }
    let description = first_prop(ev, "DESCRIPTION").map(|d| to_plain_text(&unescape_text(&d.value))).unwrap_or_default();
    re(&IN_TEXT, r"(?i)(?:course|mata\s*kuliah|kelas)\s*[:\-]\s*([^.;|]{2,80})")
        .captures(&description)
        .map(|c| js_trim(&c[1]).to_string())
}

/// Only the event's own URL property, and only https. Never links in the description.
fn extract_url(ev: &Event) -> Option<String> {
    static HTTPS: OnceLock<Regex> = OnceLock::new();
    let url = first_prop(ev, "URL")?;
    let trimmed = js_trim(&url.value);
    re(&HTTPS, r"^(?i:https)://\S+$").is_match(trimmed).then(|| trimmed.to_string())
}

/// Locale-aware title order is approximated: case-insensitive, lowercase first.
pub(crate) fn compare_titles(a: &str, b: &str) -> std::cmp::Ordering {
    a.to_lowercase().cmp(&b.to_lowercase()).then_with(|| b.cmp(a))
}

/// Parse calendar text into tasks, nearest deadline first.
///
/// `local` is the zone used for floating times and all-day events (the user's
/// time zone in the app; explicit here so tests do not depend on the machine).
pub fn parse_tasks(ics_text: &str, opts: &ParseOptions, local: &TimeZone) -> Vec<Task> {
    if ics_text.trim().is_empty() || !ics_text.contains("BEGIN:VCALENDAR") {
        return Vec::new();
    }
    let min_ms = opts.now_ms - opts.keep_overdue_ms;
    let max_ms = opts.now_ms + opts.horizon_ms;

    let mut zones: HashMap<String, Option<TimeZone>> = HashMap::new();
    let mut by_id: HashMap<String, Task> = HashMap::new();
    let mut order: Vec<String> = Vec::new();

    for ev in extract_events(ics_text) {
        let Some(start_prop) = first_prop(&ev, "DTSTART") else { continue };
        let Some(start) = parse_ics_date(&start_prop.value, &start_prop.params, local, &mut zones) else { continue };

        let summary = first_prop(&ev, "SUMMARY").map(|p| unescape_text(&p.value)).unwrap_or_default();
        let (title, phase) = split_summary(&summary);
        if title.is_empty() {
            continue;
        }

        let course = extract_course(&ev);
        let url = extract_url(&ev);
        let description = truncate(
            &first_prop(&ev, "DESCRIPTION").map(|d| to_plain_text(&unescape_text(&d.value))).unwrap_or_default(),
            DESCRIPTION_MAX_LEN,
        );
        let uid = match first_prop(&ev, "UID") {
            Some(p) if !js_trim(&p.value).is_empty() => js_trim(&p.value).to_string(),
            _ => format!("{title}|{phase}"),
        };

        let all_day = start.date_only;
        let due_ms = if all_day {
            match end_of_local_day(local, start.ms) {
                Some(ms) => ms,
                None => continue,
            }
        } else {
            start.ms
        };
        if due_ms < min_ms || due_ms > max_ms {
            continue;
        }

        let id = format!("{uid}::{due_ms}");
        if by_id.contains_key(&id) {
            continue;
        }
        order.push(id.clone());
        by_id.insert(
            id.clone(),
            Task {
                id, uid, title, course, phase, due: iso_string(due_ms), due_ms, all_day, description, url,
                source: None, feed_id: None, feed_kind: None,
            },
        );
    }

    let mut tasks: Vec<Task> = order.into_iter().filter_map(|id| by_id.remove(&id)).collect();
    tasks.sort_by(|a, b| a.due_ms.cmp(&b.due_ms).then_with(|| compare_titles(&a.title, &b.title)));
    if let Some(limit) = opts.limit {
        tasks.truncate(limit);
    }
    tasks
}

/// True when `task` falls inside the next `window_ms` and has not passed yet.
pub fn is_due_within(task: &Task, window_ms: i64, now_ms: i64) -> bool {
    let delta = task.due_ms - now_ms;
    (0..=window_ms).contains(&delta)
}

pub fn is_overdue(task: &Task, now_ms: i64) -> bool {
    task.due_ms < now_ms
}

#[cfg(test)]
mod tests {
    use super::*;

    fn jakarta() -> TimeZone {
        TimeZone::get("Asia/Jakarta").unwrap()
    }

    #[test]
    fn plain_text_strips_markup_and_entities() {
        assert_eq!(to_plain_text("<p>Kuis&nbsp;online &amp; <b>bab</b> 4</p>"), "Kuis online & bab 4");
        assert_eq!(to_plain_text("a<br/>b<BR>c"), "a b c");
        assert_eq!(to_plain_text("&#8212; &#0; &unknown; x"), "\u{2014} x");
    }

    #[test]
    fn summary_suffix_sets_the_phase() {
        assert_eq!(split_summary("Tugas 1 is due"), ("Tugas 1".into(), "due".into()));
        assert_eq!(split_summary("Kuis closes"), ("Kuis".into(), "closes".into()));
        assert_eq!(split_summary("Praktikum harus diserahkan"), ("Praktikum".into(), "due".into()));
        assert_eq!(split_summary("Rapat"), ("Rapat".into(), "event".into()));
        // A summary that is only the suffix keeps itself as the title.
        assert_eq!(split_summary(" is due").1, "event".to_string());
    }

    #[test]
    fn utc_tzid_floating_and_date_only_starts() {
        let ics = "BEGIN:VCALENDAR\r\nBEGIN:VEVENT\r\nUID:a\r\nSUMMARY:A is due\r\nDTSTART:20260822T170000Z\r\nEND:VEVENT\r\n\
BEGIN:VEVENT\r\nUID:b\r\nSUMMARY:B is due\r\nDTSTART;TZID=Asia/Jakarta:20260821T235900\r\nEND:VEVENT\r\n\
BEGIN:VEVENT\r\nUID:c\r\nSUMMARY:C closes\r\nDTSTART;VALUE=DATE:20260825\r\nEND:VEVENT\r\nEND:VCALENDAR\r\n";
        let now = 1_787_000_000_000; // 2026-08-17
        let tasks = parse_tasks(ics, &ParseOptions::new(now), &jakarta());
        let by_uid = |u: &str| tasks.iter().find(|t| t.uid == u).unwrap();
        assert_eq!(by_uid("a").due, "2026-08-22T17:00:00.000Z");
        assert_eq!(by_uid("b").due, "2026-08-21T16:59:00.000Z"); // 23:59 in UTC+7
        assert!(by_uid("c").all_day);
        assert_eq!(by_uid("c").due, "2026-08-25T16:59:59.999Z"); // end of the local day
        assert_eq!(tasks.iter().map(|t| t.uid.as_str()).collect::<Vec<_>>(), ["b", "a", "c"]);
    }

    #[test]
    fn unfolding_and_escapes() {
        let ics = "BEGIN:VCALENDAR\nBEGIN:VEVENT\nUID:f\nSUMMARY:Long\n  title\\, with comma is due\nDTSTART:20260822T170000Z\nEND:VEVENT\nEND:VCALENDAR";
        let tasks = parse_tasks(ics, &ParseOptions::new(1_787_000_000_000), &jakarta());
        assert_eq!(tasks[0].title, "Long title, with comma");
    }

    #[test]
    fn not_a_calendar_gives_nothing() {
        assert!(parse_tasks("", &ParseOptions::new(0), &jakarta()).is_empty());
        assert!(parse_tasks("hello", &ParseOptions::new(0), &jakarta()).is_empty());
    }

    #[test]
    fn iso_string_matches_javascript() {
        assert_eq!(iso_string(0), "1970-01-01T00:00:00.000Z");
        assert_eq!(iso_string(1_787_000_000_123), "2026-08-17T20:53:20.123Z");
        assert_eq!(iso_string(-1), "1969-12-31T23:59:59.999Z");
    }
}
