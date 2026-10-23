//! Real data for the deadlines module: feeds fetched on short-lived worker
//! threads, the user's choices persisted in the same files the WebView2 app
//! used, and the notification planner.
//!
//! One mutex guards all state. The UI thread reads and writes it in short
//! bursts; a worker only holds it to publish a finished fetch. When a fetch
//! finishes the worker calls `wake`, which the window turns into a message so
//! the UI thread redraws and runs the planner.

use std::collections::HashMap;
use std::sync::atomic::{AtomicI64, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};

use blink_core::feed::{FeedResult as Fetched, FeedService};
use blink_core::feed_url::next_refresh_delay_ms;
use blink_core::feeds::FeedMeta;
use blink_core::format::FeedSync;
use blink_core::done::DoneStore;
use blink_core::ics::{iso_string, parse_tasks, ParseOptions, Task, DAY_MS};
use blink_core::local_events::LocalEvent;
use blink_core::merge::FeedResult;
use blink_core::planner::{
    expire_snoozes, group_notifications, plan_notifications, prune_fired, record_fired, snooze_until, Fired, PlanTask, Snoozed,
};
use blink_core::secret::Secrets;
use blink_core::settings::Settings;
use blink_core::store::Store;
use blink_core::view::build_view;
use jiff::tz::TimeZone;
use serde_json::Value;

use crate::modules::deadlines::{Announcement, DeadlineSource};

const HOUR_MS: f64 = 3_600_000.0;
const MINUTE_MS: f64 = 60_000.0;
/// Parse generously; the view applies the user's keep-overdue window.
const PARSE_KEEP_MS: i64 = 30 * DAY_MS;
const PARSE_HORIZON_MS: i64 = 365 * DAY_MS;
const MAX_INDIVIDUAL_TOASTS: usize = 3;

/// Where feed text comes from. The real one is `FeedService`; tests script it.
pub trait FeedFetcher: Send + Sync {
    fn fetch(&self, id: &str, force: bool) -> Result<Fetched, &'static str>;
}

impl<S: Secrets> FeedFetcher for FeedService<S> {
    fn fetch(&self, id: &str, force: bool) -> Result<Fetched, &'static str> {
        FeedService::fetch(self, id, force)
    }
}

#[derive(Clone, Default)]
struct Entry {
    tasks: Vec<Task>,
    fetched_at_ms: Option<i64>,
    /// (code, the server was never reached)
    error: Option<(String, bool)>,
    syncing: bool,
    failures: u32,
    /// Wall time of the next scheduled refresh.
    next_at: Option<i64>,
}

struct Inner {
    metas: Vec<FeedMeta>,
    entries: HashMap<String, Entry>,
    local: Vec<LocalEvent>,
    done: HashMap<String, String>,
    fired: Fired,
    snoozed: Snoozed,
    thresholds_hours: Vec<f64>,
    keep_overdue_days: f64,
    refresh_minutes: f64,
    /// Something changed that the planner should look at.
    dirty: bool,
    /// Wall time at which the planner must run again by itself.
    planner_due: Option<i64>,
}

pub struct LiveSource {
    inner: Arc<Mutex<Inner>>,
    fetcher: Arc<dyn FeedFetcher>,
    store: Arc<Store>,
    wake: Arc<dyn Fn() + Send + Sync>,
    tz: TimeZone,
    /// Run fetches on a worker thread (always, except in tests).
    threaded: bool,
    /// The latest wall time the UI thread saw, so workers parse against the app's clock.
    wall_hint: Arc<AtomicI64>,
}

fn lock(inner: &Mutex<Inner>) -> MutexGuard<'_, Inner> {
    inner.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
}

// ------------------------------------------------------------ persistence

fn read_done(store: &Store) -> HashMap<String, String> {
    match store.get_json("done") {
        Ok(Some(Value::Object(map))) => map.into_iter().filter_map(|(k, v)| v.as_str().map(|s| (k, s.to_string()))).collect(),
        _ => HashMap::new(),
    }
}

fn read_fired(store: &Store) -> Fired {
    match store.get_json("fired") {
        Ok(Some(Value::Object(map))) => map
            .into_iter()
            .map(|(k, v)| (k, v.as_array().map(|a| a.iter().filter_map(Value::as_f64).collect()).unwrap_or_default()))
            .collect(),
        _ => Fired::new(),
    }
}

fn read_snoozed(store: &Store) -> Snoozed {
    match store.get_json("snoozed") {
        Ok(Some(Value::Object(map))) => map.into_iter().filter_map(|(k, v)| v.as_f64().map(|n| (k, n))).collect(),
        _ => Snoozed::new(),
    }
}

/// Personal events as the settings page saves them: `[{ id, title, dueMs, allDay }]`.
fn read_local_events(store: &Store) -> Vec<LocalEvent> {
    let Ok(Some(Value::Array(items))) = store.get_json("local_events") else { return Vec::new() };
    items
        .iter()
        .filter_map(|v| {
            Some(LocalEvent {
                id: v.get("id")?.as_str()?.to_string(),
                title: v.get("title")?.as_str()?.to_string(),
                due_ms: v.get("dueMs")?.as_f64()? as i64,
                all_day: v.get("allDay").and_then(Value::as_bool).unwrap_or(false),
            })
        })
        .collect()
}

fn write_map<T: serde::Serialize>(store: &Store, name: &str, value: &T) {
    if let Ok(v) = serde_json::to_value(value) {
        let _ = store.set_json(name, &v);
    }
}

impl Inner {
    fn enabled(&self) -> impl Iterator<Item = &FeedMeta> {
        self.metas.iter().filter(|m| m.enabled)
    }

    fn feed_results(&self) -> Vec<FeedResult> {
        self.enabled()
            .map(|m| FeedResult {
                feed_id: m.id.clone(),
                feed_kind: Some(m.kind.clone()),
                tasks: self.entries.get(&m.id).map(|e| e.tasks.clone()).unwrap_or_default(),
            })
            .collect()
    }

    /// Drop what belongs to feeds that were removed or switched off.
    fn reconcile(&mut self) {
        let wanted: Vec<String> = self.enabled().map(|m| m.id.clone()).collect();
        self.entries.retain(|id, _| wanted.contains(id));
    }
}

impl LiveSource {
    pub fn new(
        store: Store,
        fetcher: Arc<dyn FeedFetcher>,
        settings: &Settings,
        tz: TimeZone,
        wake: Arc<dyn Fn() + Send + Sync>,
        threaded: bool,
    ) -> LiveSource {
        let inner = Inner {
            metas: settings.feeds.clone(),
            entries: HashMap::new(),
            local: read_local_events(&store),
            done: read_done(&store),
            fired: read_fired(&store),
            snoozed: read_snoozed(&store),
            thresholds_hours: settings.thresholds_hours.clone(),
            keep_overdue_days: settings.keep_overdue_days as f64,
            refresh_minutes: settings.refresh_minutes,
            dirty: true,
            planner_due: None,
        };
        LiveSource { inner: Arc::new(Mutex::new(inner)), fetcher, store: Arc::new(store), wake, tz, threaded, wall_hint: Arc::new(AtomicI64::new(0)) }
    }

    /// Re-read personal events, done, snooze and reminder state (the settings program changed them).
    pub fn reload_local(&mut self) {
        let events = read_local_events(&self.store);
        let (done, fired, snoozed) = (read_done(&self.store), read_fired(&self.store), read_snoozed(&self.store));
        let mut g = lock(&self.inner);
        g.local = events;
        g.done = done;
        g.fired = fired;
        g.snoozed = snoozed;
        g.dirty = true;
    }

    /// Start a fetch for one feed unless one is already running.
    fn start_sync(&self, id: &str, force: bool, wall_ms: i64) {
        {
            let mut g = lock(&self.inner);
            let entry = g.entries.entry(id.to_string()).or_default();
            if entry.syncing {
                return;
            }
            entry.syncing = true;
        }
        let inner = self.inner.clone();
        let fetcher = self.fetcher.clone();
        let wake = self.wake.clone();
        let hint = self.wall_hint.clone();
        let tz = self.tz.clone();
        let id = id.to_string();
        let job = move || {
            let result = fetcher.fetch(&id, force);
            let now = match hint.load(Ordering::Relaxed) {
                0 => wall_ms,
                v => v,
            };
            finish_sync(&inner, &id, result, now, &tz);
            wake();
        };
        if self.threaded {
            std::thread::spawn(job);
        } else {
            job();
        }
    }

    /// When the planner next runs on its own (a threshold or a snooze), ignoring refreshes.
    pub fn planner_due(&self) -> Option<i64> {
        lock(&self.inner).planner_due
    }

    /// Whether any feed is being fetched right now.
    pub fn syncing(&self) -> bool {
        lock(&self.inner).entries.values().any(|e| e.syncing)
    }
}

/// Turn a finished fetch into the entry's new state and schedule the next one.
fn finish_sync(inner: &Mutex<Inner>, id: &str, result: Result<Fetched, &'static str>, now: i64, tz: &TimeZone) {
    let fetched = result.unwrap_or(Fetched {
        ics: None,
        from_cache: false,
        fetched_at_ms: None,
        error: Some(blink_core::feed::FeedError { code: "NETWORK", transport: true }),
    });
    let parsed = fetched.ics.as_deref().map(|text| {
        let mut opts = ParseOptions::new(now);
        opts.keep_overdue_ms = PARSE_KEEP_MS;
        opts.horizon_ms = PARSE_HORIZON_MS;
        parse_tasks(text, &opts, tz)
    });

    let mut g = lock(inner);
    let refresh_ms = g.refresh_minutes * MINUTE_MS;
    let Some(entry) = g.entries.get_mut(id) else { return };
    let error = fetched.error.as_ref().map(|e| (e.code.to_string(), e.transport));
    if let Some(tasks) = parsed {
        entry.tasks = tasks;
    }
    if fetched.fetched_at_ms.is_some() {
        entry.fetched_at_ms = fetched.fetched_at_ms;
    }
    let transport_failed = error.as_ref().is_some_and(|(_, transport)| *transport);
    entry.failures = if transport_failed { entry.failures + 1 } else { 0 };
    entry.error = error;
    entry.syncing = false;
    let delay = next_refresh_delay_ms(refresh_ms, entry.error.is_none(), entry.failures);
    // Count from when the data was fetched, not from when the fetch was asked for: the feed
    // service refuses to hit the network within 15 minutes of its last fetch, so a schedule that
    // starts a moment earlier would be answered from the cache and double the real interval.
    // Failures count from now (the cached time is old and would retry in a tight loop).
    let base = if entry.error.is_none() { fetched.fetched_at_ms.unwrap_or(now) } else { now };
    entry.next_at = Some(base + delay as i64);
    g.dirty = true;
}

impl DeadlineSource for LiveSource {
    fn feeds(&self) -> Vec<FeedResult> {
        lock(&self.inner).feed_results()
    }

    fn local_events(&self) -> Vec<LocalEvent> {
        lock(&self.inner).local.clone()
    }

    fn done(&self) -> HashMap<String, String> {
        lock(&self.inner).done.clone()
    }

    fn snoozed(&self) -> HashMap<String, f64> {
        lock(&self.inner).snoozed.clone()
    }

    fn sync_state(&self) -> Vec<FeedSync> {
        let g = lock(&self.inner);
        g.enabled()
            .map(|m| {
                let e = g.entries.get(&m.id);
                FeedSync {
                    fetched_at_ms: e.and_then(|e| e.fetched_at_ms).map(|ms| ms as f64),
                    // A feed that has not started yet counts as syncing, so the footer never flashes "no data".
                    syncing: e.is_none_or(|e| e.syncing),
                    error: e.and_then(|e| e.error.clone()),
                }
            })
            .collect()
    }

    fn has_feeds(&self) -> bool {
        lock(&self.inner).enabled().next().is_some()
    }

    fn keep_overdue_days(&self) -> f64 {
        lock(&self.inner).keep_overdue_days
    }

    fn mark_done(&mut self, id: &str, wall_ms: i64) {
        let mut g = lock(&self.inner);
        let mut store = DoneStore::new(g.done.clone());
        store.mark(id, &iso_string(wall_ms));
        g.done = json_map(&store.to_json());
        write_map(&self.store, "done", &g.done);
        g.dirty = true;
    }

    fn restore(&mut self, id: &str) {
        let mut g = lock(&self.inner);
        let mut store = DoneStore::new(g.done.clone());
        store.unmark(id);
        g.done = json_map(&store.to_json());
        write_map(&self.store, "done", &g.done);
        g.dirty = true;
    }

    fn snooze(&mut self, id: &str, kind: &str, wall_ms: i64, tz: &TimeZone) {
        if let Some(until) = snooze_until(kind, wall_ms, tz) {
            let mut g = lock(&self.inner);
            g.snoozed.insert(id.to_string(), until as f64);
            write_map(&self.store, "snoozed", &g.snoozed);
            g.dirty = true;
        }
    }

    /// Fetch every enabled feed now, ignoring the 15 minute floor.
    fn refresh(&mut self) {
        let ids: Vec<String> = lock(&self.inner).enabled().map(|m| m.id.clone()).collect();
        let wall = self.wall_hint.load(Ordering::Relaxed);
        for id in ids {
            self.start_sync(&id, true, wall);
        }
    }

    fn reload_local(&mut self) {
        LiveSource::reload_local(self)
    }

    fn configure(&mut self, settings: &Settings) {
        let mut g = lock(&self.inner);
        g.metas = settings.feeds.clone();
        g.thresholds_hours = settings.thresholds_hours.clone();
        g.keep_overdue_days = settings.keep_overdue_days as f64;
        g.refresh_minutes = settings.refresh_minutes;
        g.reconcile();
        g.dirty = true;
    }

    fn poll(&mut self, wall_ms: i64) {
        self.wall_hint.store(wall_ms, Ordering::Relaxed);
        let due: Vec<String> = {
            let g = lock(&self.inner);
            g.enabled()
                .filter(|m| match g.entries.get(&m.id) {
                    None => true,
                    Some(e) => !e.syncing && e.next_at.is_some_and(|at| at <= wall_ms),
                })
                .map(|m| m.id.clone())
                .collect()
        };
        for id in due {
            self.start_sync(&id, false, wall_ms);
        }
    }

    fn next_wake_wall(&self, wall_ms: i64) -> Option<i64> {
        let g = lock(&self.inner);
        let refresh = g.enabled().filter_map(|m| g.entries.get(&m.id)).filter(|e| !e.syncing).filter_map(|e| e.next_at).min();
        // Something changed since the planner last ran: run it now.
        let planner = if g.dirty { Some(wall_ms + 1) } else { g.planner_due };
        [refresh, planner].into_iter().flatten().min()
    }

    fn announce(&mut self, wall_ms: i64) -> Vec<Announcement> {
        let mut g = lock(&self.inner);
        if !g.dirty && g.planner_due.is_none_or(|due| wall_ms < due) {
            return Vec::new();
        }
        g.dirty = false;
        let now = wall_ms as f64;

        let (snoozed, fired, changed) = expire_snoozes(&g.snoozed, &g.fired, now);
        if changed {
            g.snoozed = snoozed;
            g.fired = fired;
            write_map(&self.store, "snoozed", &g.snoozed);
            write_map(&self.store, "fired", &g.fired);
        }

        let view = build_view(&g.feed_results(), &g.local, &g.done, g.keep_overdue_days, wall_ms, &self.tz);
        let live: Vec<String> = view.upcoming.iter().chain(&view.overdue).chain(&view.done_list).map(|t| t.id.clone()).collect();
        let pruned = prune_fired(&g.fired, &live, g.keep_overdue_days * DAY_MS as f64, now);
        if pruned.len() != g.fired.len() {
            g.fired = pruned;
            write_map(&self.store, "fired", &g.fired);
        }

        let plan_tasks: Vec<PlanTask> = view.upcoming.iter().map(|t| PlanTask { id: t.id.clone(), due_ms: t.due_ms as f64 }).collect();
        let plans = plan_notifications(&plan_tasks, &g.thresholds_hours, now, &g.fired, &g.snoozed);
        let mut out = Vec::new();
        if !plans.is_empty() {
            let (individual, extra) = group_notifications(&plans, MAX_INDIVIDUAL_TOASTS);
            for plan in &individual {
                if let Some(task) = view.upcoming.iter().find(|t| t.id == plan.task_id) {
                    out.push(Announcement::Task(task.clone()));
                }
            }
            if extra > 0 {
                out.push(Announcement::Summary(plans.len()));
            }
            g.fired = record_fired(&g.fired, &plans);
            write_map(&self.store, "fired", &g.fired);
        }

        // When must the planner run next without any other event?
        let mut next: Option<f64> = None;
        let mut consider = |at: f64| {
            if at > now {
                next = Some(next.map_or(at, |n| n.min(at)));
            }
        };
        for until in g.snoozed.values() {
            consider(*until);
        }
        for task in &view.upcoming {
            if g.snoozed.get(&task.id).is_some_and(|u| *u > now) {
                continue; // its snooze end is already in the list
            }
            let already = g.fired.get(&task.id).map(Vec::as_slice).unwrap_or(&[]);
            for h in g.thresholds_hours.iter().filter(|h| !already.contains(h)) {
                consider(task.due_ms as f64 - h * HOUR_MS);
            }
        }
        g.planner_due = next.map(|n| n.ceil() as i64);
        out
    }
}

fn json_map(v: &Value) -> HashMap<String, String> {
    v.as_object().map(|m| m.iter().filter_map(|(k, v)| v.as_str().map(|s| (k.clone(), s.to_string()))).collect()).unwrap_or_default()
}
