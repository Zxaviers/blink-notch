//! Differential test for the small pure modules: every call in
//! tests/diff/ops-cases.mjs must return what the JavaScript returned
//! (fixtures from tests/diff/gen-ops.mjs), under several machine zones.

mod common;

use std::collections::HashMap;

use blink_core::done::{DoneStore, LastKind};
use blink_core::ics::{Task, DEFAULT_KEEP_OVERDUE_MS};
use blink_core::local_events::{self, LocalEvent};
use blink_core::merge::{merge_feed_tasks, FeedResult};
use blink_core::planner::{self, Fired, Plan, PlanTask, Snoozed};
use blink_core::countdown;
use blink_core::format::{self, FeedSync};
use blink_core::i18n::{resolve_language, Lang, Translator};
use blink_core::view;
use blink_core::{feed_url, feeds, hotkey, jsval, settings, thresholds};
use common::{canon, read_json, zones, Op};
use jiff::tz::TimeZone;
use serde_json::{json, Value};


// ---- lenient conversions: the JS tolerated odd shapes, so the wrappers do too

fn is_string_id(v: &Value) -> bool {
    v.get("id").is_some_and(Value::is_string)
}

fn tasks_from(v: Option<&Value>) -> Vec<Task> {
    v.and_then(Value::as_array)
        .map(|a| a.iter().filter(|t| t.is_object() && is_string_id(t)).filter_map(|t| serde_json::from_value(t.clone()).ok()).collect())
        .unwrap_or_default()
}

fn feed_results_from(v: Option<&Value>) -> Vec<FeedResult> {
    let Some(Value::Array(items)) = v else { return Vec::new() };
    items
        .iter()
        .filter_map(|r| {
            let feed = r.get("feed").filter(|f| f.is_object())?;
            let id = feed.get("id").and_then(Value::as_str)?;
            let tasks = r.get("tasks").filter(|t| t.is_array())?;
            Some(FeedResult {
                feed_id: id.to_string(),
                feed_kind: feed.get("kind").and_then(Value::as_str).map(String::from),
                tasks: tasks_from(Some(tasks)),
            })
        })
        .collect()
}

fn local_events_from(v: Option<&Value>) -> Vec<LocalEvent> {
    v.and_then(Value::as_array)
        .map(|a| {
            a.iter()
                .filter_map(|e| {
                    let due = e.get("dueMs")?.as_f64().filter(|d| d.is_finite())?;
                    Some(LocalEvent {
                        id: e.get("id")?.as_str()?.to_string(),
                        title: e.get("title").and_then(Value::as_str).unwrap_or("").to_string(),
                        due_ms: due as i64,
                        all_day: matches!(e.get("allDay"), Some(Value::Bool(true))),
                    })
                })
                .collect()
        })
        .unwrap_or_default()
}

fn fired_from(v: Option<&Value>) -> Fired {
    let mut out = Fired::new();
    if let Some(Value::Object(m)) = v {
        for (k, list) in m {
            if let Value::Array(items) = list {
                out.insert(k.clone(), items.iter().filter_map(Value::as_f64).collect());
            }
        }
    }
    out
}

fn snoozed_from(v: Option<&Value>) -> Snoozed {
    let mut out = Snoozed::new();
    if let Some(Value::Object(m)) = v {
        for (k, until) in m {
            // A non-number never counts as a live snooze (NaN is "not finite").
            out.insert(k.clone(), until.as_f64().unwrap_or(f64::NAN));
        }
    }
    out
}

fn fired_json(fired: &Fired) -> Value {
    Value::Object(fired.iter().map(|(k, v)| (k.clone(), Value::Array(v.iter().copied().map(jsval::json_number).collect()))).collect())
}

fn plan_tasks_from(v: Option<&Value>) -> Vec<PlanTask> {
    v.and_then(Value::as_array)
        .map(|a| {
            a.iter()
                .filter_map(|t| Some(PlanTask { id: t.get("id")?.as_str()?.to_string(), due_ms: t.get("dueMs")?.as_f64()? }))
                .collect()
        })
        .unwrap_or_default()
}

fn num(v: Option<&Value>) -> f64 {
    v.and_then(Value::as_f64).unwrap_or(f64::NAN)
}

fn done_script(initial: &Value, steps: &Value) -> Value {
    let initial: Vec<(String, String)> = initial
        .as_object()
        .map(|m| m.iter().map(|(k, v)| (k.clone(), v.as_str().unwrap_or("").to_string())).collect())
        .unwrap_or_default();
    let mut store = DoneStore::new(initial);
    let out = steps.as_array().unwrap().iter().map(|s| {
        let id = s.get("id").and_then(Value::as_str).unwrap_or("");
        match s["op"].as_str().unwrap() {
            "isDone" => json!(store.is_done(id)),
            "mark" => {
                store.mark(id, s["iso"].as_str().unwrap());
                Value::Null
            }
            "unmark" => {
                store.unmark(id);
                Value::Null
            }
            "visible" => serde_json::to_value(store.visible(&tasks_from(s.get("tasks")))).unwrap(),
            "doneList" => serde_json::to_value(store.done_list(&tasks_from(s.get("tasks")))).unwrap(),
            "prune" => {
                let left = store.prune(&tasks_from(s.get("tasks")), s["keepMs"].as_i64().unwrap(), s["nowMs"].as_i64().unwrap());
                Value::Object(left.into_iter().map(|(k, v)| (k, Value::String(v))).collect())
            }
            "lastAction" => store.last_action().map_or(Value::Null, |(id, kind)| {
                json!({ "id": id, "type": if *kind == LastKind::Mark { "mark" } else { "unmark" } })
            }),
            "undo" => {
                store.undo("UNDO-TIMESTAMP");
                Value::Null
            }
            "toJSON" => store.to_json(),
            other => panic!("unknown step {other}"),
        }
    });
    Value::Array(out.collect())
}

fn base36(mut n: u64) -> String {
    let mut digits = Vec::new();
    loop {
        digits.push(b"0123456789abcdefghijklmnopqrstuvwxyz"[(n % 36) as usize]);
        n /= 36;
        if n == 0 {
            break;
        }
    }
    digits.reverse();
    String::from_utf8(digits).unwrap()
}
/// Run one op the way the JS did. `Err` carries an error code like the JS `throw`.
fn run(op: &Op, _tz: &TimeZone) -> Result<Value, String> {
    // {$localWall, plusMs} markers: that wall time in the zone under test, plus an offset.
    let resolved: Vec<Value> = op.args.iter().map(|v| match v.get("$localWall").and_then(Value::as_str) {
        Some(wall) => {
            let dt: jiff::civil::DateTime = wall.parse().expect("wall time");
            json!(_tz.to_timestamp(dt).unwrap().as_millisecond() + v["plusMs"].as_i64().unwrap())
        }
        None => v.clone(),
    }).collect();
    let a = &resolved;
    let arg = |i: usize| a.get(i);
    let string = |i: usize| a.get(i).and_then(Value::as_str).unwrap_or("").to_string();
    Ok(match op.func.as_str() {
        "normalizeThresholdsHours" => {
            Value::Array(thresholds::normalize_thresholds_hours(arg(0)).into_iter().map(jsval::json_number).collect())
        }
        "isValidFeedId" => json!(feeds::is_valid_feed_id(&string(0))),
        "normalizeFeedKind" => feeds::normalize_feed_kind(&string(0)).map_or(Value::Null, |k| json!(k)),
        "normalizeFeed" => feeds::normalize_feed(&a[0]),
        "feedMetaFrom" => feeds::feed_meta_from(&a[0]),
        "upsertFeedMeta" => feeds::upsert_feed_meta(&a[0], &a[1]),
        "clampInterval" => {
            let n = arg(0).and_then(Value::as_f64).unwrap_or(f64::NAN);
            jsval::json_number(feed_url::clamp_interval(n))
        }
        "describeUrl" => json!(feed_url::describe_url(&string(0))),
        "validateUrl" => feed_url::validate_url(arg(0)).map(Value::from).map_err(String::from)?,
        "nextRefreshDelayMs" => {
            let o = &a[0];
            let refresh = o.get("refreshMs").and_then(Value::as_f64).unwrap_or(f64::NAN);
            let ok = o.get("ok").and_then(Value::as_bool).unwrap_or(false);
            let failures = o.get("transportFailures").and_then(Value::as_u64).unwrap_or(0) as u32;
            jsval::json_number(feed_url::next_refresh_delay_ms(refresh, ok, failures))
        }
        "inspectMoodleUrl" => {
            let (ok, hint) = feed_url::inspect_moodle_url(&string(0));
            json!({ "ok": ok, "hint": hint })
        }
        "isValidAccelerator" => json!(hotkey::is_valid_accelerator(arg(0))),
        "normalizeSettings" => serde_json::to_value(settings::normalize_settings(arg(0))).unwrap(),
        "mergeFeedTasks" => serde_json::to_value(merge_feed_tasks(&feed_results_from(arg(0)))).unwrap(),
        "doneScript" => done_script(&a[0], &a[1]),
        "normalizeLocalEvent" => {
            let o = &a[0];
            let now = a[1].as_i64().unwrap();
            match local_events::normalize_local_event(o.get("title"), o.get("date"), o.get("time"), now, _tz) {
                Ok(ev) => {
                    // The id is random: check its shape and time part, then mask it like the JS side did.
                    let want_prefix = format!("local-{}-", base36(now as u64));
                    assert!(ev.id.starts_with(&want_prefix) && ev.id.len() == want_prefix.len() + 6, "bad id {}", ev.id);
                    assert!(ev.id[want_prefix.len()..].bytes().all(|b| b.is_ascii_digit() || b.is_ascii_lowercase()));
                    json!({ "ok": true, "event": { "id": "local-ID", "title": ev.title, "dueMs": ev.due_ms, "allDay": ev.all_day } })
                }
                Err(code) => json!({ "ok": false, "error": code }),
            }
        }
        "localTaskFrom" => {
            let ev = local_events_from(Some(&json!([a[0].clone()]))).remove(0);
            serde_json::to_value(local_events::local_task_from(&ev)).unwrap()
        }
        "withLocalTasks" => {
            let keep = a.get(3).and_then(Value::as_i64).unwrap_or(DEFAULT_KEEP_OVERDUE_MS);
            serde_json::to_value(local_events::with_local_tasks(&tasks_from(arg(0)), &local_events_from(arg(1)), a[2].as_i64().unwrap(), keep)).unwrap()
        }
        "planNotifications" => {
            let thresholds: Vec<f64> = arg(1).and_then(Value::as_array).map(|t| t.iter().filter_map(Value::as_f64).collect()).unwrap_or_default();
            if !arg(0).is_some_and(Value::is_array) || !arg(1).is_some_and(Value::is_array) {
                json!([])
            } else {
                serde_json::to_value(planner::plan_notifications(
                    &plan_tasks_from(arg(0)), &thresholds, num(arg(2)), &fired_from(arg(3)), &snoozed_from(arg(4)),
                )).unwrap()
            }
        }
        "recordFired" => {
            let plans: Vec<Plan> = arg(1).and_then(Value::as_array).unwrap().iter().map(|p| Plan {
                task_id: p["taskId"].as_str().unwrap().to_string(),
                threshold_hours: 0.0,
                crossed: p["crossed"].as_array().unwrap().iter().filter_map(Value::as_f64).collect(),
                remaining_ms: 0.0,
            }).collect();
            fired_json(&planner::record_fired(&fired_from(arg(0)), &plans))
        }
        "pruneFired" => {
            let live: Vec<String> = arg(1).and_then(Value::as_array).unwrap().iter().filter_map(|t| t["id"].as_str().map(String::from)).collect();
            fired_json(&planner::prune_fired(&fired_from(arg(0)), &live, num(arg(2)), num(arg(3))))
        }
        "groupNotifications" => {
            let plans = arg(0).and_then(Value::as_array).cloned().unwrap_or_default();
            let max = a.get(1).and_then(Value::as_u64).unwrap_or(3) as usize;
            let (individual, extra) = planner::group_notifications(&plans, max);
            json!({ "individual": individual, "extra": extra })
        }
        "snoozeUntil" => {
            let kind = arg(0).and_then(Value::as_str).unwrap_or("\u{0}none");
            planner::snooze_until(kind, a[1].as_i64().unwrap(), _tz).map_or(Value::Null, Value::from)
        }
        "isSnoozed" => json!(planner::is_snoozed(&snoozed_from(arg(0)), a[1].as_str().unwrap(), num(arg(2)))),
        "expireSnoozes" => {
            let (s, f, changed) = planner::expire_snoozes(&snoozed_from(arg(0)), &fired_from(arg(1)), num(arg(2)));
            json!({
                "snoozed": Value::Object(s.iter().map(|(k, v)| (k.clone(), jsval::json_number(*v))).collect()),
                "fired": fired_json(&f),
                "changed": changed
            })
        }
        "buildView" => {
            let o = &a[0];
            let done: HashMap<String, String> = o["done"].as_object().unwrap().iter().map(|(k, v)| (k.clone(), v.as_str().unwrap().to_string())).collect();
            let v = view::build_view(
                &feed_results_from(o.get("feedResults")), &local_events_from(o.get("localEvents")), &done,
                o["settings"]["keepOverdueDays"].as_f64().unwrap(), o["nowMs"].as_i64().unwrap(), _tz,
            );
            serde_json::to_value(v).unwrap()
        }
        "restModeFor" => {
            let o = &a[0];
            let hero: Option<Task> = o.get("hero").filter(|h| h.is_object()).and_then(|h| serde_json::from_value(h.clone()).ok());
            json!(view::rest_mode_for(o["restingStyle"].as_str().unwrap(), hero.as_ref(), o["nowMs"].as_i64().unwrap(), o["dueHorizonHours"].as_f64().unwrap()))
        }        "t" => {
            let lang = Lang::from_code(&string(0)).unwrap_or(Lang::En);
            let vars: Vec<(String, String)> = arg(2)
                .and_then(Value::as_object)
                .map(|m| m.iter().map(|(k, v)| (k.clone(), jsval::js_string_or_empty(Some(v)))).collect())
                .unwrap_or_default();
            let refs: Vec<(&str, &str)> = vars.iter().map(|(k, v)| (k.as_str(), v.as_str())).collect();
            json!(Translator::new(lang).t(&string(1), &refs))
        }
        "resolveLanguage" => json!(resolve_language(arg(0).and_then(Value::as_str).unwrap_or(""), arg(1).and_then(Value::as_str).unwrap_or("")).code()),
        "urgencyOf" => json!(countdown::urgency_of(a[0].as_i64().unwrap(), a[1].as_i64().unwrap())),
        "compactCountdown" => {
            let t = Translator::new(Lang::from_code(&string(2)).unwrap_or(Lang::En));
            let seconds = a.get(3).and_then(|o| o.get("seconds")).and_then(Value::as_bool).unwrap_or(false);
            json!(countdown::compact_countdown(a[0].as_i64().unwrap(), a[1].as_i64().unwrap(), &t, seconds))
        }
        "dueLabel" => {
            let t = Translator::new(Lang::from_code(&string(2)).unwrap_or(Lang::En));
            json!(countdown::due_label(a[0].as_i64().unwrap(), a[1].as_i64().unwrap(), &t, _tz))
        }
        "isDataStale" => json!(countdown::is_data_stale(num(arg(0)), num(arg(1)), jsval::js_number(arg(2)))),
        "nextTickMs" => {
            let live = a[2].get("live").and_then(Value::as_bool).unwrap_or(false);
            json!(countdown::next_tick_ms(a[0].as_i64().unwrap(), a[1].as_i64().unwrap(), live))
        }
        "truncate" => json!(format::truncate(&jsval::js_string_or_empty(arg(0)), a[1].as_u64().unwrap() as usize)),
        "pillLabel" => {
            let task: Option<Task> = arg(0).filter(|t| t.is_object()).and_then(|t| serde_json::from_value(t.clone()).ok());
            json!(format::pill_label(task.as_ref(), arg(1).and_then(Value::as_u64).unwrap_or(14) as usize))
        }
        "syncSummary" => {
            let t = Translator::new(Lang::from_code(&string(1)).unwrap_or(Lang::En));
            let feeds: Vec<FeedSync> = arg(0)
                .and_then(Value::as_object)
                .map(|m| {
                    m.values()
                        .map(|f| FeedSync {
                            fetched_at_ms: f.get("fetchedAtMs").and_then(Value::as_f64).filter(|x| x.is_finite()),
                            syncing: f.get("syncing").and_then(Value::as_bool).unwrap_or(false),
                            error: f.get("error").filter(|e| e.is_object()).map(|e| {
                                (e["code"].as_str().unwrap_or("undefined").to_string(), e.get("transport").and_then(Value::as_bool).unwrap_or(false))
                            }),
                        })
                        .collect()
                })
                .unwrap_or_default();
            let s = format::sync_summary(&feeds, &t, _tz);
            json!({ "kind": s.kind, "text": s.text })
        }        other => return Err(format!("unported function {other}")),
    })
}

#[test]
fn rust_matches_javascript_on_the_small_modules() {
    let ops: Vec<Op> = read_json("ops-cases.json");
    assert!(ops.len() > 1800, "expected the full corpus, got {}", ops.len());
    let mut failures = Vec::new();

    for zone in zones() {
        let expected: Vec<Value> = read_json(&format!("ops-expected.{}.json", zone.replace('/', "_")));
        let tz = TimeZone::get(&zone).unwrap();
        for (op, want) in ops.iter().zip(&expected) {
            let got = match run(op, &tz) {
                Ok(v) => json!({ "value": canon(v) }),
                Err(e) => json!({ "error": e }),
            };
            let want = canon(want.clone());
            if got != want {
                failures.push(format!(
                    "[{zone}] {}({})\n  js:   {want}\n  rust: {got}",
                    op.func,
                    serde_json::to_string(&op.args).unwrap()
                ));
            }
        }
    }
    assert!(failures.is_empty(), "{} mismatches:\n{}", failures.len(), failures.join("\n"));
}
