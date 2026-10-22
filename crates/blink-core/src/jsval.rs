//! JavaScript-compatible conversions, so the ports behave like the code they
//! replace on odd input (strings that look like numbers, `null`, missing keys).

use serde_json::Value;

/// The characters JavaScript's `\s` and `trim()` treat as whitespace.
pub fn is_js_space(c: char) -> bool {
    matches!(
        c,
        '\t' | '\n' | '\u{b}' | '\u{c}' | '\r' | ' ' | '\u{a0}' | '\u{1680}'
            | '\u{2000}'..='\u{200a}'
            | '\u{2028}' | '\u{2029}' | '\u{202f}' | '\u{205f}' | '\u{3000}' | '\u{feff}'
    )
}

pub fn js_trim(s: &str) -> &str {
    s.trim_matches(is_js_space)
}

pub fn js_trim_end(s: &str) -> &str {
    s.trim_end_matches(is_js_space)
}

/// `Number(string)`.
pub fn number_from_str(s: &str) -> f64 {
    let t = js_trim(s);
    if t.is_empty() {
        return 0.0;
    }
    let radix = |digits: &str, radix: u32| u64::from_str_radix(digits, radix).map_or(f64::NAN, |n| n as f64);
    if let Some(h) = t.strip_prefix("0x").or_else(|| t.strip_prefix("0X")) {
        return radix(h, 16);
    }
    if let Some(o) = t.strip_prefix("0o").or_else(|| t.strip_prefix("0O")) {
        return radix(o, 8);
    }
    if let Some(b) = t.strip_prefix("0b").or_else(|| t.strip_prefix("0B")) {
        return radix(b, 2);
    }
    match t {
        "Infinity" | "+Infinity" => return f64::INFINITY,
        "-Infinity" => return f64::NEG_INFINITY,
        _ => {}
    }
    // Rust also accepts "inf" and "nan"; JavaScript does not.
    let body = t.trim_start_matches(['+', '-']);
    if !body.chars().next().is_some_and(|c| c.is_ascii_digit() || c == '.') {
        return f64::NAN;
    }
    t.parse::<f64>().unwrap_or(f64::NAN)
}

/// `Number(value)` for a JSON value; a missing key (`undefined`) is NaN.
pub fn js_number(v: Option<&Value>) -> f64 {
    match v {
        None => f64::NAN,
        Some(Value::Null) => 0.0,
        Some(Value::Bool(b)) => f64::from(u8::from(*b)),
        Some(Value::Number(n)) => n.as_f64().unwrap_or(f64::NAN),
        Some(Value::String(s)) => number_from_str(s),
        Some(Value::Array(a)) if a.is_empty() => 0.0,
        Some(Value::Array(a)) if a.len() == 1 => js_number(a.first()),
        Some(_) => f64::NAN,
    }
}

/// `Math.round`: halves round toward +infinity.
pub fn js_round(x: f64) -> f64 {
    if x.is_finite() { (x + 0.5).floor() } else { x }
}

/// `String(value ?? '')`.
pub fn js_string_or_empty(v: Option<&Value>) -> String {
    match v {
        None | Some(Value::Null) => String::new(),
        Some(Value::String(s)) => s.clone(),
        Some(Value::Bool(b)) => b.to_string(),
        Some(Value::Number(n)) => n.to_string(),
        Some(other) => other.to_string(),
    }
}

/// An f64 as a JSON number: integral values become integers, like JS `JSON.stringify`.
pub fn json_number(x: f64) -> Value {
    if x.is_finite() && x.fract() == 0.0 && x.abs() < 9.0e15 {
        Value::from(x as i64)
    } else {
        serde_json::Number::from_f64(x).map_or(Value::Null, Value::Number)
    }
}