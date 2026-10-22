//! What the notch shows, from raw state (`view.js`): feed tasks, personal
//! events, the done map, the clock.

use std::collections::HashMap;

use jiff::tz::TimeZone;
use serde::Serialize;

use crate::done::DoneStore;
use crate::ics::{Task, DAY_MS};
use crate::local_events::{with_local_tasks, LocalEvent};
use crate::merge::{merge_feed_tasks, FeedResult};

const HOUR_MS: i64 = 3_600_000;

#[derive(Debug, Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct View {
    pub upcoming: Vec<Task>,
    pub overdue: Vec<Task>,
    pub done_list: Vec<Task>,
    pub hero: Option<Task>,
}

pub fn build_view(
    feed_results: &[FeedResult],
    local_events: &[LocalEvent],
    done: &HashMap<String, String>,
    keep_overdue_days: f64,
    now_ms: i64,
    _tz: &TimeZone,
) -> View {
    let keep_ms = (keep_overdue_days * DAY_MS as f64) as i64;
    let merged = with_local_tasks(&merge_feed_tasks(feed_results), local_events, now_ms, keep_ms);
    let all: Vec<Task> = merged.into_iter().filter(|t| t.due_ms >= now_ms - keep_ms).collect();

    let store = DoneStore::new(done.iter().map(|(k, v)| (k.clone(), v.clone())));
    let visible: Vec<Task> = store.visible(&all).into_iter().cloned().collect();
    let done_list: Vec<Task> = store.done_list(&all).into_iter().cloned().collect();

    let upcoming: Vec<Task> = visible.iter().filter(|t| t.due_ms >= now_ms).cloned().collect();
    let mut overdue: Vec<Task> = visible.iter().filter(|t| t.due_ms < now_ms).cloned().collect();
    overdue.sort_by(|a, b| b.due_ms.cmp(&a.due_ms)); // most recently missed first

    let hero = upcoming.first().or(overdue.first()).cloned();
    View { upcoming, overdue, done_list, hero }
}

/// Resting pill form: "full" (course code plus countdown), "mini", or "hot"
/// (an invisible hover strip).
pub fn rest_mode_for(resting_style: &str, hero: Option<&Task>, now_ms: i64, due_horizon_hours: f64) -> &'static str {
    match resting_style {
        "hover" => "hot",
        "always" => "full",
        _ => match hero {
            Some(h) if (h.due_ms < now_ms) || (h.due_ms - now_ms) as f64 <= due_horizon_hours * HOUR_MS as f64 => "full",
            _ => "mini",
        },
    }
}