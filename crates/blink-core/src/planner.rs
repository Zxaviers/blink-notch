//! Notification planner (`planner.js`). A pure function says which deadline
//! thresholds are due right now; each fires once per task. If several were
//! crossed while the app was closed, only the tightest is announced and the
//! rest are marked fired, so a restart never replays old reminders.

use std::collections::{HashMap, HashSet};

use jiff::tz::TimeZone;
use jiff::Timestamp;
use serde::Serialize;

use crate::jsval::number_from_str;

const HOUR_MS: f64 = 3_600_000.0;

pub type Fired = HashMap<String, Vec<f64>>;
pub type Snoozed = HashMap<String, f64>;

/// What the planner needs to know about a task.
#[derive(Debug, Clone)]
pub struct PlanTask {
    pub id: String,
    pub due_ms: f64,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Plan {
    pub task_id: String,
    pub threshold_hours: f64,
    pub crossed: Vec<f64>,
    pub remaining_ms: f64,
}

pub fn is_snoozed(snoozed: &Snoozed, task_id: &str, now_ms: f64) -> bool {
    snoozed.get(task_id).is_some_and(|u| u.is_finite() && *u > now_ms)
}

pub fn plan_notifications(
    tasks: &[PlanTask],
    thresholds_hours: &[f64],
    now_ms: f64,
    fired: &Fired,
    snoozed: &Snoozed,
) -> Vec<Plan> {
    let mut thresholds: Vec<f64> = Vec::new();
    for h in thresholds_hours {
        if h.is_finite() && *h > 0.0 && !thresholds.contains(h) {
            thresholds.push(*h);
        }
    }
    thresholds.sort_by(|a, b| b.total_cmp(a));

    let mut plans = Vec::new();
    for task in tasks {
        if !task.due_ms.is_finite() {
            continue;
        }
        let remaining = task.due_ms - now_ms;
        if remaining <= 0.0 || is_snoozed(snoozed, &task.id, now_ms) {
            continue; // overdue items get no new alarms
        }
        let already = fired.get(&task.id).map(Vec::as_slice).unwrap_or(&[]);
        let crossed: Vec<f64> = thresholds
            .iter()
            .copied()
            .filter(|h| remaining <= h * HOUR_MS && !already.contains(h))
            .collect();
        if crossed.is_empty() {
            continue;
        }
        let tightest = crossed.iter().copied().fold(f64::INFINITY, f64::min);
        plans.push(Plan { task_id: task.id.clone(), threshold_hours: tightest, crossed, remaining_ms: remaining });
    }
    plans.sort_by(|a, b| a.remaining_ms.total_cmp(&b.remaining_ms));
    plans
}

/// Fired map with every crossed threshold of every plan recorded.
pub fn record_fired(fired: &Fired, plans: &[Plan]) -> Fired {
    let mut next = fired.clone();
    for plan in plans {
        let mut set: Vec<f64> = next.get(&plan.task_id).cloned().unwrap_or_default();
        for h in &plan.crossed {
            if !set.contains(h) {
                set.push(*h);
            }
        }
        set.sort_by(|a, b| b.total_cmp(a));
        next.insert(plan.task_id.clone(), set);
    }
    next
}

/// Drop entries for tasks that left the list and whose deadline is outside the
/// keep-overdue window: they can never fire again.
pub fn prune_fired(fired: &Fired, live_ids: &[String], keep_overdue_ms: f64, now_ms: f64) -> Fired {
    let live: HashSet<&str> = live_ids.iter().map(String::as_str).collect();
    let min_due = now_ms - keep_overdue_ms;
    fired
        .iter()
        .filter(|(id, _)| {
            if live.contains(id.as_str()) {
                return true;
            }
            let due = id.split("::").nth(1).map_or(f64::NAN, number_from_str);
            due.is_finite() && due >= min_due
        })
        .map(|(k, v)| (k.clone(), v.clone()))
        .collect()
}

/// Split plans into individual toasts and a count of the rest, so the first
/// sync of a big feed does not produce a wall of notifications.
pub fn group_notifications<T: Clone>(plans: &[T], max_individual: usize) -> (Vec<T>, usize) {
    (plans.iter().take(max_individual).cloned().collect(), plans.len().saturating_sub(max_individual))
}

pub const SNOOZE_KINDS: [&str; 3] = ["30m", "2h", "tomorrow"];
const TOMORROW_HOUR: i8 = 8;

/// When a snooze of this kind ends ("tomorrow" is 08:00 local). None for an unknown kind.
pub fn snooze_until(kind: &str, now_ms: i64, tz: &TimeZone) -> Option<i64> {
    match kind {
        "30m" => Some(now_ms + 30 * 60 * 1000),
        "2h" => Some(now_ms + 2 * 3_600_000),
        "tomorrow" => {
            let local = Timestamp::from_millisecond(now_ms).ok()?.to_zoned(tz.clone());
            let next = local.date().tomorrow().ok()?.at(TOMORROW_HOUR, 0, 0, 0);
            Some(tz.to_timestamp(next).ok()?.as_millisecond())
        }
        _ => None,
    }
}

/// Drop snoozes that ran out. A task coming back from snooze is announced
/// again, so its fired thresholds are cleared. Returns (snoozed, fired, changed).
pub fn expire_snoozes(snoozed: &Snoozed, fired: &Fired, now_ms: f64) -> (Snoozed, Fired, bool) {
    let mut next_snoozed = Snoozed::new();
    let mut next_fired = fired.clone();
    let mut changed = false;
    for (id, until) in snoozed {
        if until.is_finite() && *until > now_ms {
            next_snoozed.insert(id.clone(), *until);
        } else {
            changed = true;
            next_fired.remove(id);
        }
    }
    (next_snoozed, next_fired, changed)
}