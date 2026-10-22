//! Urgency buckets and countdown text (`countdown.js`). Every urgency state
//! carries a word or a number as well as a color: overdue is never a bare
//! label, "soon" always shows the time left.

use jiff::civil::Weekday;
use jiff::tz::TimeZone;
use jiff::Timestamp;

use crate::i18n::{Lang, Translator};
use crate::jsval::js_round;

pub const MINUTE_MS: i64 = 60 * 1000;
pub const HOUR_MS: i64 = 60 * MINUTE_MS;
pub const DAY_MS: i64 = 24 * HOUR_MS;
/// Due within this window counts as "soon" (amber).
pub const SOON_MS: i64 = DAY_MS;

/// "overdue", "soon" or "later". Callers leave "later" uncolored.
pub fn urgency_of(due_ms: i64, now_ms: i64) -> &'static str {
    let delta = due_ms - now_ms;
    if delta < 0 {
        "overdue"
    } else if delta <= SOON_MS {
        "soon"
    } else {
        "later"
    }
}

/// Compact countdown for the pill and list rows:
/// "3d 2h", "2h 14m", "42m" (or "42:07" with `seconds`), "Overdue 3h".
pub fn compact_countdown(due_ms: i64, now_ms: i64, t: &Translator, seconds: bool) -> String {
    let delta = due_ms - now_ms;
    let abs = delta.abs();
    let minutes = abs / MINUTE_MS;
    let hours = minutes / 60;
    let days = hours / 24;
    let (m, h, d) = (t.t("unit.m", &[]), t.t("unit.h", &[]), t.t("unit.d", &[]));

    if delta < 0 {
        let value = if minutes < 60 {
            format!("{}{m}", minutes.max(1))
        } else if hours < 24 {
            format!("{hours}{h}")
        } else {
            format!("{days}{d}")
        };
        return t.t("overdue.ago", &[("value", &value)]);
    }
    if minutes < 60 {
        if seconds {
            return format!("{:02}:{:02}", minutes, (abs / 1000) % 60);
        }
        return format!("{}{m}", minutes.max(1));
    }
    if hours < 24 {
        return format!("{hours}{h} {}{m}", minutes % 60);
    }
    format!("{days}{d} {}{h}", hours % 24)
}

fn start_of_day(tz: &TimeZone, ms: i64) -> i64 {
    let date = Timestamp::from_millisecond(ms).expect("in range").to_zoned(tz.clone()).date();
    tz.to_timestamp(date.at(0, 0, 0, 0)).expect("valid").as_millisecond()
}

/// "HH:MM" in the zone, as `Intl` formats it with hour cycle h23 (the JS then
/// turns Indonesian "09.05" into "09:05").
pub fn clock(ms: i64, tz: &TimeZone) -> String {
    let z = Timestamp::from_millisecond(ms).expect("in range").to_zoned(tz.clone());
    format!("{:02}:{:02}", z.hour(), z.minute())
}

fn weekday_short(lang: Lang, d: Weekday) -> &'static str {
    const EN: [&str; 7] = ["Mon", "Tue", "Wed", "Thu", "Fri", "Sat", "Sun"];
    const ID: [&str; 7] = ["Sen", "Sel", "Rab", "Kam", "Jum", "Sab", "Min"];
    let i = d.to_monday_zero_offset() as usize;
    if lang == Lang::Id { ID[i] } else { EN[i] }
}

fn month_short(lang: Lang, month: i8) -> &'static str {
    const EN: [&str; 12] = ["Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sept", "Oct", "Nov", "Dec"];
    const ID: [&str; 12] = ["Jan", "Feb", "Mar", "Apr", "Mei", "Jun", "Jul", "Agu", "Sep", "Okt", "Nov", "Des"];
    let i = (month - 1) as usize;
    if lang == Lang::Id { ID[i] } else { EN[i] }
}

/// "Today 14:00", "Tomorrow 09:30", "Fri 23:59" (within a week), or "22 Aug".
pub fn due_label(due_ms: i64, now_ms: i64, t: &Translator, tz: &TimeZone) -> String {
    let lang = t.lang();
    let time = clock(due_ms, tz);
    let day_delta = js_round((start_of_day(tz, due_ms) - start_of_day(tz, now_ms)) as f64 / DAY_MS as f64) as i64;
    let due = Timestamp::from_millisecond(due_ms).expect("in range").to_zoned(tz.clone());
    match day_delta {
        0 => t.t("when.today", &[("time", &time)]),
        1 => t.t("when.tomorrow", &[("time", &time)]),
        2..=6 => format!("{} {time}", weekday_short(lang, due.weekday())),
        _ => format!("{} {}", due.day(), month_short(lang, due.month())),
    }
}

/// True when synced data is older than a refresh interval plus a grace minute.
pub fn is_data_stale(last_fetched_at_ms: f64, now_ms: f64, refresh_minutes: f64) -> bool {
    if !last_fetched_at_ms.is_finite() {
        return true;
    }
    let minutes = if refresh_minutes.is_finite() && refresh_minutes != 0.0 { refresh_minutes } else { 20.0 };
    now_ms - last_fetched_at_ms > minutes * MINUTE_MS as f64 + MINUTE_MS as f64
}

/// How long until the displayed countdown can change. Seconds only matter in
/// the last hour, and only while something is live on screen.
pub fn next_tick_ms(due_ms: i64, now_ms: i64, live: bool) -> i64 {
    if !live {
        return MINUTE_MS;
    }
    let delta = due_ms - now_ms;
    if delta > 0 && delta < HOUR_MS { 1000 } else { 15_000 }
}