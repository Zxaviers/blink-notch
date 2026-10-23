//! The real data source: feeds, persistence, reminders. The network is replaced
//! by a scripted fetcher; the files are real, in a temp folder.

mod common;

use std::collections::VecDeque;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{Arc, Mutex};

use blink_core::feed::{FeedError, FeedResult as Fetched};
use blink_core::feed_url::next_refresh_delay_ms;
use blink_core::settings::{normalize_settings, Settings};
use blink_core::store::Store;
use blink_native::app::{App, AppEffect};
use blink_native::clock::Clock;
use blink_native::live::{FeedFetcher, LiveSource};
use blink_native::modules::deadlines::{Announcement, DeadlineSource, DeadlinesModule};
use blink_native::modules::Module;
use common::{tz, WALL_BASE};
use serde_json::json;

struct Scripted(Mutex<VecDeque<Result<Fetched, &'static str>>>);

impl FeedFetcher for Scripted {
    fn fetch(&self, _id: &str, _force: bool) -> Result<Fetched, &'static str> {
        self.0.lock().unwrap().pop_front().unwrap_or(Err("NO_SCRIPT"))
    }
}

fn scripted(items: Vec<Result<Fetched, &'static str>>) -> Arc<Scripted> {
    Arc::new(Scripted(Mutex::new(items.into())))
}

fn dir(tag: &str) -> std::path::PathBuf {
    static N: AtomicU32 = AtomicU32::new(0);
    let d = std::env::temp_dir().join(format!("blink-live-{}-{}-{tag}", std::process::id(), N.fetch_add(1, Ordering::Relaxed)));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

fn settings() -> Settings {
    normalize_settings(Some(&json!({
        "feeds": [{ "id": "f1", "kind": "ics", "label": "Feed", "enabled": true }],
        "thresholdsHours": [24, 6, 1],
        "refreshMinutes": 720,
    })))
}

/// A calendar with one event per (uid, title, due in ms from `WALL_BASE`).
fn ics(events: &[(&str, &str, i64)]) -> String {
    let mut out = String::from("BEGIN:VCALENDAR\r\nVERSION:2.0\r\n");
    for (uid, title, due) in events {
        let t = blink_core::ics::iso_string(WALL_BASE + due).replace(['-', ':'], "");
        let stamp = format!("{}T{}Z", &t[0..8], &t[9..15]);
        out.push_str(&format!("BEGIN:VEVENT\r\nUID:{uid}\r\nSUMMARY:{title}\r\nDTSTART:{stamp}\r\nEND:VEVENT\r\n"));
    }
    out.push_str("END:VCALENDAR\r\n");
    out
}

fn ok(text: String) -> Result<Fetched, &'static str> {
    Ok(Fetched { ics: Some(text), from_cache: false, fetched_at_ms: Some(WALL_BASE), error: None })
}

fn source(d: &std::path::Path, fetcher: Arc<dyn FeedFetcher>) -> LiveSource {
    LiveSource::new(Store::new(d), fetcher, &settings(), tz(), Arc::new(|| {}), false)
}

fn refresh_ms() -> i64 {
    (settings().refresh_minutes * 60_000.0) as i64
}

const H: i64 = 3_600_000;
const MIN: i64 = 60_000;

#[test]
fn a_feed_is_fetched_parsed_and_scheduled_again() {
    let d = dir("fetch");
    let mut s = source(&d, scripted(vec![ok(ics(&[("a", "Quiz 1 is due", 30 * H), ("b", "Essay is due", 2 * H)]))]));
    assert!(s.has_feeds());
    s.poll(WALL_BASE);
    let titles: Vec<String> = s.feeds()[0].tasks.iter().map(|t| t.title.clone()).collect();
    assert_eq!(titles, ["Essay", "Quiz 1"], "sorted nearest first, Moodle suffix split off");
    let sync = &s.sync_state()[0];
    assert!(!sync.syncing && sync.error.is_none() && sync.fetched_at_ms.is_some());
    assert_eq!(s.next_wake_wall(WALL_BASE), Some(WALL_BASE + 1), "new data makes the planner due at once");
    s.announce(WALL_BASE);
    let next = s.next_wake_wall(WALL_BASE).unwrap();
    assert!(next <= WALL_BASE + refresh_ms(), "the next wake is no later than the next refresh");
}

#[test]
fn polling_does_not_refetch_before_the_interval() {
    let d = dir("interval");
    let mut s = source(&d, scripted(vec![ok(ics(&[("a", "Quiz is due", 30 * H)]))]));
    s.poll(WALL_BASE);
    // A second fetch would hit the empty script and report an error.
    s.poll(WALL_BASE + 5 * MIN);
    assert!(s.sync_state()[0].error.is_none());
    s.poll(WALL_BASE + refresh_ms() + 1);
    assert!(s.sync_state()[0].error.is_some(), "after the interval it fetches again (and the script is empty)");
}

#[test]
fn offline_keeps_the_cached_tasks_and_backs_off() {
    let d = dir("offline");
    let cached = ics(&[("a", "Quiz is due", 30 * H)]);
    let offline = Ok(Fetched {
        ics: Some(cached.clone()),
        from_cache: true,
        fetched_at_ms: Some(WALL_BASE),
        error: Some(FeedError { code: "NETWORK", transport: true }),
    });
    let mut s = source(&d, scripted(vec![ok(cached), offline]));
    s.poll(WALL_BASE);
    s.refresh();
    assert_eq!(s.feeds()[0].tasks.len(), 1, "cached tasks stay");
    assert_eq!(s.sync_state()[0].error, Some(("NETWORK".to_string(), true)));
    let ladder = next_refresh_delay_ms(settings().refresh_minutes * 60_000.0, false, 1) as i64;
    s.announce(WALL_BASE);
    assert_eq!(s.next_wake_wall(WALL_BASE), Some(WALL_BASE + ladder), "first offline retry follows the ladder");
}

#[test]
fn a_failing_fetch_with_nothing_cached_shows_the_error_and_no_tasks() {
    let d = dir("nothing");
    let failed = Ok(Fetched { ics: None, from_cache: false, fetched_at_ms: None, error: Some(FeedError { code: "UNAUTHORIZED", transport: false }) });
    let mut s = source(&d, scripted(vec![failed]));
    s.poll(WALL_BASE);
    assert!(s.feeds()[0].tasks.is_empty());
    assert_eq!(s.sync_state()[0].error, Some(("UNAUTHORIZED".to_string(), false)));
}

#[test]
fn done_and_snooze_survive_a_restart_in_the_old_file_format() {
    let d = dir("persist");
    let mut s = source(&d, scripted(vec![]));
    s.mark_done("a::1", WALL_BASE);
    s.snooze("b::2", "30m", WALL_BASE, &tz());
    drop(s);

    let raw: serde_json::Value = serde_json::from_slice(&std::fs::read(d.join("done.json")).unwrap()).unwrap();
    assert!(raw["a::1"].as_str().unwrap().starts_with("2026-10-22T12:00:00"), "done.json is a map of id to ISO time: {raw}");
    let raw: serde_json::Value = serde_json::from_slice(&std::fs::read(d.join("snoozed.json")).unwrap()).unwrap();
    assert_eq!(raw["b::2"].as_f64(), Some((WALL_BASE + 30 * MIN) as f64));

    let mut again = source(&d, scripted(vec![]));
    assert!(again.done().contains_key("a::1"));
    assert_eq!(again.snoozed().get("b::2").copied(), Some((WALL_BASE + 30 * MIN) as f64));
    again.restore("a::1");
    assert!(!source(&d, scripted(vec![])).done().contains_key("a::1"));
}

#[test]
fn a_reminder_fires_once_per_threshold() {
    let d = dir("fire");
    let mut s = source(&d, scripted(vec![ok(ics(&[("a", "Quiz is due", 5 * H)]))]));
    s.poll(WALL_BASE);
    let first = s.announce(WALL_BASE);
    assert_eq!(first.len(), 1, "5h left: the 24h and 6h thresholds are crossed, one toast");
    assert!(matches!(&first[0], Announcement::Task(t) if t.title == "Quiz"));
    s.mark_done("nothing", WALL_BASE); // anything that dirties the planner
    assert!(s.announce(WALL_BASE).is_empty(), "never repeated");

    // The 1 hour threshold is crossed later.
    let wake = s.planner_due().unwrap();
    assert_eq!(wake, WALL_BASE + 4 * H, "the planner wakes exactly when the 1h threshold is crossed");
    assert!(s.announce(wake - 1).is_empty());
    assert_eq!(s.announce(wake).len(), 1);
    let fired: serde_json::Value = serde_json::from_slice(&std::fs::read(d.join("fired.json")).unwrap()).unwrap();
    assert_eq!(fired.as_object().unwrap().len(), 1, "fired.json records it: {fired}");

    // A restart does not replay it.
    let mut again = source(&d, scripted(vec![ok(ics(&[("a", "Quiz is due", 5 * H)]))]));
    again.poll(wake);
    assert!(again.announce(wake).is_empty());
}

#[test]
fn many_reminders_at_once_become_three_plus_a_summary() {
    let d = dir("many");
    let events: Vec<(String, String, i64)> = (1..=5).map(|i| (format!("u{i}"), format!("Task {i} is due"), i * 10 * MIN)).collect();
    let refs: Vec<(&str, &str, i64)> = events.iter().map(|(a, b, c)| (a.as_str(), b.as_str(), *c)).collect();
    let mut s = source(&d, scripted(vec![ok(ics(&refs))]));
    s.poll(WALL_BASE);
    let out = s.announce(WALL_BASE);
    assert_eq!(out.len(), 4);
    assert!(matches!(out[3], Announcement::Summary(5)));
    assert!(matches!(&out[0], Announcement::Task(t) if t.title == "Task 1"), "tightest first");
}

#[test]
fn a_snoozed_task_is_quiet_and_comes_back() {
    let d = dir("snooze");
    let mut s = source(&d, scripted(vec![ok(ics(&[("a", "Quiz is due", 5 * H)]))]));
    s.poll(WALL_BASE);
    let id = s.feeds()[0].tasks[0].id.clone();
    s.snooze(&id, "30m", WALL_BASE, &tz());
    assert!(s.announce(WALL_BASE).is_empty(), "snoozed: nothing");
    let wake = s.planner_due().unwrap();
    assert_eq!(wake, WALL_BASE + 30 * MIN, "the planner wakes when the snooze ends");
    assert_eq!(s.announce(wake).len(), 1, "announced again afterwards");
    assert!(s.next_wake_wall(WALL_BASE).unwrap() <= WALL_BASE + refresh_ms(), "and the overall wake is never later than a refresh");
}

#[test]
fn removing_a_feed_in_settings_drops_its_tasks() {
    let d = dir("remove");
    let mut s = source(&d, scripted(vec![ok(ics(&[("a", "Quiz is due", 5 * H)]))]));
    s.poll(WALL_BASE);
    assert_eq!(s.feeds().len(), 1);
    s.configure(&normalize_settings(Some(&json!({ "feeds": [] }))));
    assert!(s.feeds().is_empty());
    assert!(!s.has_feeds());
}

#[test]
fn personal_events_come_from_the_events_file() {
    let d = dir("local");
    let due = WALL_BASE + 3 * H;
    std::fs::write(d.join("local_events.json"), json!([{ "id": "local-x-1", "title": "Dentist", "dueMs": due, "allDay": false }]).to_string()).unwrap();
    let s = source(&d, scripted(vec![]));
    assert_eq!(s.local_events().len(), 1);
    assert_eq!(s.local_events()[0].title, "Dentist");
}

fn app_with(source: LiveSource, st: Settings) -> App {
    let modules: Vec<Box<dyn Module>> = vec![Box::new(DeadlinesModule::new(Box::new(source)))];
    App::new(Clock::manual(WALL_BASE), st, tz(), 1.0, "en", modules).unwrap()
}

#[test]
fn the_app_turns_a_crossed_threshold_into_a_toast_and_a_peek() {
    let d = dir("app");
    let mut app = app_with(source(&d, scripted(vec![ok(ics(&[("a", "Quiz is due", 5 * H)]))])), settings());
    app.tick();
    let effects = app.take_effects();
    let toast = effects.iter().find_map(|e| match e {
        AppEffect::Toast { title, body, .. } => Some((title.clone(), body.clone())),
        _ => None,
    });
    let (title, body) = toast.expect("a toast was requested");
    assert_eq!(title, "Deadline approaching: Quiz");
    assert!(body.contains("4h") || body.contains("5h"), "body says how long is left: {body}");
    assert_eq!(app.shell().mode(), blink_native::shell::Mode::Peek, "the notch peeks too");
}

#[test]
fn toasts_off_still_records_the_reminder_but_sends_none() {
    let d = dir("notoast");
    let mut st = settings();
    st.toast = false;
    let live = LiveSource::new(Store::new(&d), scripted(vec![ok(ics(&[("a", "Quiz is due", 5 * H)]))]), &st, tz(), Arc::new(|| {}), false);
    let mut app = app_with(live, st);
    app.tick();
    assert!(!app.take_effects().iter().any(|e| matches!(e, AppEffect::Toast { .. })));
    assert!(d.join("fired.json").exists(), "the threshold is still marked as fired");
}

#[test]
fn the_next_refresh_counts_from_when_the_data_was_fetched() {
    // Fetched a little after the poll that asked for it: the feed service will not touch the
    // network again until 15 minutes after that moment, so the schedule must not be earlier.
    let d = dir("fetched-at");
    let fetched = Ok(Fetched { ics: Some(ics(&[("a", "Quiz is due", 30 * H)])), from_cache: false, fetched_at_ms: Some(WALL_BASE + 4_000), error: None });
    let mut s = source(&d, scripted(vec![fetched]));
    s.poll(WALL_BASE);
    s.announce(WALL_BASE);
    let refresh = s.next_wake_wall(WALL_BASE).unwrap();
    assert_eq!(refresh, WALL_BASE + 4_000 + refresh_ms());
}
