//! Personal ("local") events: deadlines the user types in that no calendar
//! feed knows about (`localEvents.js`). They are merged into the task list as
//! task-shaped records with `source: "local"`.

use std::collections::HashSet;
use std::hash::{BuildHasher, RandomState};

use jiff::tz::TimeZone;
use serde_json::Value;

use crate::ics::{compare_titles, iso_string, local_ms, utc_ms, Task, DEFAULT_HORIZON_MS, DEFAULT_KEEP_OVERDUE_MS};
use crate::jsval::{js_string_or_empty, js_trim};

#[derive(Debug, Clone, PartialEq)]
pub struct LocalEvent {
    pub id: String,
    pub title: String,
    pub due_ms: i64,
    pub all_day: bool,
}

fn base36(mut n: u64) -> String {
    const DIGITS: &[u8; 36] = b"0123456789abcdefghijklmnopqrstuvwxyz";
    if n == 0 {
        return "0".to_string();
    }
    let mut out = Vec::new();
    while n > 0 {
        out.push(DIGITS[(n % 36) as usize]);
        n /= 36;
    }
    out.reverse();
    String::from_utf8(out).expect("ascii")
}

/// `local-<time in base 36>-<6 random base-36 chars>`, like the JS.
fn new_event_id(now_ms: i64) -> String {
    let random = RandomState::new().hash_one(now_ms);
    let suffix: String = base36(random).chars().rev().take(6).collect();
    format!("local-{}-{:0>6}", base36(now_ms.max(0) as u64), suffix)
}

fn all_digits(s: &str, n: usize) -> bool {
    s.len() == n && s.bytes().all(|b| b.is_ascii_digit())
}

/// What V8 accepts for `new Date("YYYY-MM-DDTHH:mm:00")`: month 1 to 12, day 1
/// to 31 (rolling over into the next month), hour up to 24 (24:00 only), minute below 60.
fn wall_clock_ms(date: &str, hhmm: &str) -> Option<i64> {
    let (y, m, d) = (date[0..4].parse::<i64>().ok()?, date[5..7].parse::<i64>().ok()?, date[8..10].parse::<i64>().ok()?);
    let (h, mi) = (hhmm[0..2].parse::<i64>().ok()?, hhmm[3..5].parse::<i64>().ok()?);
    if !(1..=12).contains(&m) || !(1..=31).contains(&d) || mi > 59 || h > 24 || (h == 24 && mi != 0) {
        return None;
    }
    utc_ms(y, m - 1, d, h, mi, 0)
}

/// Validate raw form input. Missing time means all-day, due at 23:59 local.
/// Error codes: TITLE_REQUIRED, DATE_REQUIRED, TIME_INVALID, DATE_INVALID,
/// DATE_PAST, DATE_FAR.
pub fn normalize_local_event(
    title: Option<&Value>,
    date: Option<&Value>,
    time: Option<&Value>,
    now_ms: i64,
    tz: &TimeZone,
) -> Result<LocalEvent, &'static str> {
    let title = js_trim(&js_string_or_empty(title)).to_string();
    if title.is_empty() {
        return Err("TITLE_REQUIRED");
    }
    let raw_date = js_trim(&js_string_or_empty(date)).to_string();
    let date_ok = raw_date.len() == 10
        && all_digits(&raw_date[0..4], 4)
        && &raw_date[4..5] == "-"
        && all_digits(&raw_date[5..7], 2)
        && &raw_date[7..8] == "-"
        && all_digits(&raw_date[8..10], 2);
    if !date_ok {
        return Err("DATE_REQUIRED");
    }
    let raw_time = js_trim(&js_string_or_empty(time)).to_string();
    let time_ok = raw_time.len() == 5 && all_digits(&raw_time[0..2], 2) && &raw_time[2..3] == ":" && all_digits(&raw_time[3..5], 2);
    if !raw_time.is_empty() && !time_ok {
        return Err("TIME_INVALID");
    }
    let hhmm = if raw_time.is_empty() { "23:59" } else { raw_time.as_str() };
    let due_ms = wall_clock_ms(&raw_date, hhmm)
        .and_then(|wall| local_ms(tz, wall))
        .ok_or("DATE_INVALID")?;

    if due_ms < now_ms - DEFAULT_KEEP_OVERDUE_MS {
        return Err("DATE_PAST");
    }
    if due_ms > now_ms + DEFAULT_HORIZON_MS {
        return Err("DATE_FAR");
    }
    Ok(LocalEvent { id: new_event_id(now_ms), title, due_ms, all_day: raw_time.is_empty() })
}

/// Shape a persisted event like a parsed feed task.
pub fn local_task_from(ev: &LocalEvent) -> Task {
    Task {
        id: ev.id.clone(),
        uid: ev.id.clone(),
        title: ev.title.clone(),
        course: Some(String::new()),
        phase: "due".to_string(),
        due: iso_string(ev.due_ms),
        due_ms: ev.due_ms,
        all_day: ev.all_day,
        description: String::new(),
        url: None,
        source: Some("local".to_string()),
        feed_id: None,
        feed_kind: None,
    }
}

/// Feed tasks plus live local events, nearest deadline first. Events older than
/// the keep-overdue window drop out. Inputs are not mutated.
pub fn with_local_tasks(feed_tasks: &[Task], events: &[LocalEvent], now_ms: i64, keep_overdue_ms: i64) -> Vec<Task> {
    let min_ms = now_ms - keep_overdue_ms;
    let feed_ids: HashSet<&str> = feed_tasks.iter().map(|t| t.id.as_str()).collect();
    let mut all: Vec<Task> = feed_tasks.to_vec();
    all.extend(
        events
            .iter()
            .filter(|ev| ev.due_ms >= min_ms && !feed_ids.contains(ev.id.as_str()))
            .map(local_task_from),
    );
    all.sort_by(|a, b| a.due_ms.cmp(&b.due_ms).then_with(|| compare_titles(&a.title, &b.title)));
    all
}