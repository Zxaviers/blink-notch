//! Differential test for the small pure modules: every call in
//! tests/diff/ops-cases.mjs must return what the JavaScript returned
//! (fixtures from tests/diff/gen-ops.mjs), under several machine zones.

mod common;

use blink_core::{feed_url, feeds, hotkey, jsval, settings, thresholds};
use common::{canon, read_json, zones, Op};
use jiff::tz::TimeZone;
use serde_json::{json, Value};

/// Run one op the way the JS did. `Err` carries an error code like the JS `throw`.
fn run(op: &Op, _tz: &TimeZone) -> Result<Value, String> {
    let a = &op.args;
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
        other => return Err(format!("unported function {other}")),
    })
}

#[test]
fn rust_matches_javascript_on_the_small_modules() {
    let ops: Vec<Op> = read_json("ops-cases.json");
    assert!(ops.len() > 150, "expected the full corpus, got {}", ops.len());
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
