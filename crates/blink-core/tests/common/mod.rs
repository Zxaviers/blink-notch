//! Shared helpers for the differential tests.
#![allow(dead_code)]

use std::fs;
use std::path::PathBuf;

use serde::de::DeserializeOwned;
use serde::Deserialize;
use serde_json::Value;

#[derive(Deserialize)]
pub struct Op {
    #[serde(rename = "fn")]
    pub func: String,
    pub args: Vec<Value>,
}

pub fn fixtures() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests").join("fixtures")
}

pub fn read_json<T: DeserializeOwned>(name: &str) -> T {
    let text = fs::read_to_string(fixtures().join(name))
        .unwrap_or_else(|e| panic!("{name}: {e} (run the generators in tests/diff)"));
    serde_json::from_str(&text).unwrap_or_else(|e| panic!("{name}: {e}"))
}

/// The machine zones the JS fixtures were generated under.
pub fn zones() -> Vec<String> {
    read_json("ics-zones.json")
}

/// Numbers that are whole become integers, so `24` and `24.0` compare equal
/// (JSON from JS never prints a trailing `.0`).
pub fn canon(v: Value) -> Value {
    match v {
        Value::Number(n) => match n.as_f64() {
            Some(f) if f.is_finite() && f.fract() == 0.0 && f.abs() < 9.0e15 => Value::from(f as i64),
            _ => Value::Number(n),
        },
        Value::Array(items) => Value::Array(items.into_iter().map(canon).collect()),
        Value::Object(map) => Value::Object(map.into_iter().map(|(k, v)| (k, canon(v))).collect()),
        other => other,
    }
}
