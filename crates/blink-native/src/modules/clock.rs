//! The clock: what the resting pill shows when nothing needs your attention,
//! and a calm panel with the time and date.

use jiff::{tz::TimeZone, Timestamp};

use super::{Effect, Env, Key, KeyResult, Kit, Module, PanelFrame, PeekView, PointerKind, RestView};
use crate::access::{Node, NodeAction, Role, ROOT};
use crate::ui::text::Face;
use blink_core::i18n::Lang;
use blink_core::settings::Settings;

const PANEL_H: f32 = 132.0;

const DAYS_EN: [&str; 7] = ["Mon", "Tue", "Wed", "Thu", "Fri", "Sat", "Sun"];
const DAYS_ID: [&str; 7] = ["Sen", "Sel", "Rab", "Kam", "Jum", "Sab", "Min"];
const DAYS_LONG_EN: [&str; 7] = ["Monday", "Tuesday", "Wednesday", "Thursday", "Friday", "Saturday", "Sunday"];
const DAYS_LONG_ID: [&str; 7] = ["Senin", "Selasa", "Rabu", "Kamis", "Jumat", "Sabtu", "Minggu"];
const MONTHS_EN: [&str; 12] = ["Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec"];
const MONTHS_ID: [&str; 12] = ["Jan", "Feb", "Mar", "Apr", "Mei", "Jun", "Jul", "Agu", "Sep", "Okt", "Nov", "Des"];
const MONTHS_LONG_EN: [&str; 12] =
    ["January", "February", "March", "April", "May", "June", "July", "August", "September", "October", "November", "December"];
const MONTHS_LONG_ID: [&str; 12] =
    ["Januari", "Februari", "Maret", "April", "Mei", "Juni", "Juli", "Agustus", "September", "Oktober", "November", "Desember"];

/// Local time pieces for a wall-clock instant.
struct Now {
    hour: i8,
    minute: i8,
    weekday: usize,
    day: i8,
    month: usize,
}

fn now_in(wall_ms: i64, tz: &TimeZone) -> Now {
    let z = Timestamp::from_millisecond(wall_ms).unwrap_or_default().to_zoned(tz.clone());
    Now {
        hour: z.hour(),
        minute: z.minute(),
        weekday: z.weekday().to_monday_zero_offset() as usize,
        day: z.day(),
        month: z.month() as usize - 1,
    }
}

pub fn hhmm(wall_ms: i64, tz: &TimeZone) -> String {
    let n = now_in(wall_ms, tz);
    format!("{:02}:{:02}", n.hour, n.minute)
}

/// "Fri 23 Oct"
pub fn short_date(wall_ms: i64, tz: &TimeZone, lang: Lang) -> String {
    let n = now_in(wall_ms, tz);
    let (days, months) = if lang == Lang::Id { (&DAYS_ID, &MONTHS_ID) } else { (&DAYS_EN, &MONTHS_EN) };
    format!("{} {} {}", days[n.weekday], n.day, months[n.month])
}

/// "Friday 23 October"
pub fn long_date(wall_ms: i64, tz: &TimeZone, lang: Lang) -> String {
    let n = now_in(wall_ms, tz);
    let (days, months) = if lang == Lang::Id { (&DAYS_LONG_ID, &MONTHS_LONG_ID) } else { (&DAYS_LONG_EN, &MONTHS_LONG_EN) };
    format!("{} {} {}", days[n.weekday], n.day, months[n.month])
}

#[derive(Default)]
pub struct ClockModule;

impl ClockModule {
    pub fn new() -> Self {
        ClockModule
    }
}

impl Module for ClockModule {
    fn id(&self) -> &'static str {
        "clock"
    }

    fn title(&self, lang: Lang) -> String {
        if lang == Lang::Id { "Jam" } else { "Clock" }.to_string()
    }

    fn as_any_mut(&mut self) -> &mut dyn std::any::Any {
        self
    }

    /// Always something to show, but never more important than a deadline.
    fn rest_priority(&mut self, _env: &Env, _settings: &Settings) -> u8 {
        1
    }

    fn rest_style(&mut self, _env: &Env, _resting_style: &str, _due_horizon_hours: f64) -> crate::shell::RestStyle {
        crate::shell::RestStyle::Full
    }

    fn rest(&mut self, env: &Env) -> RestView {
        RestView { label: short_date(env.wall_ms, env.tz, env.t.lang()), count: hhmm(env.wall_ms, env.tz), urgency: "later" }
    }

    fn peek(&mut self, _env: &Env) -> Option<PeekView> {
        None
    }

    fn panel_height(&mut self, _env: &Env, _kit: &mut Kit) -> f32 {
        PANEL_H
    }

    fn draw_panel(&mut self, env: &Env, kit: &mut Kit, frame: &PanelFrame) {
        let theme = frame.theme;
        let cx = frame.origin_x + 210.0;
        let time = hhmm(env.wall_ms, env.tz);
        let size = 52.0;
        let w = kit.canvas.measure(kit.fonts, Face::Mono, size, &time, 0.0);
        let base = 20.0 + kit.fonts.baseline(Face::Mono, size, 64.0);
        kit.canvas.text(kit.fonts, Face::Mono, size, cx - w / 2.0, base, &time, theme.fg, frame.alpha, 0.0);
        let date = long_date(env.wall_ms, env.tz, env.t.lang());
        let dw = kit.canvas.measure(kit.fonts, Face::Sans, 13.0, &date, 0.0);
        let dbase = 88.0 + kit.fonts.baseline(Face::Sans, 13.0, 20.0);
        kit.canvas.text(kit.fonts, Face::Sans, 13.0, cx - dw / 2.0, dbase, &date, theme.dim, frame.alpha, 0.0);
    }

    fn pointer(&mut self, _kind: PointerKind, _x: f32, _y: f32, _env: &Env) -> Vec<Effect> {
        Vec::new()
    }

    fn key(&mut self, key: Key, _env: &Env) -> KeyResult {
        // Nothing to do here; Escape closes the notch.
        KeyResult { handled: false, close: key == Key::Escape, effects: Vec::new() }
    }

    fn keyboard_mode(&mut self, _on: bool, _env: &Env) {}

    /// The next minute boundary: the only moment the text changes.
    fn next_wake(&mut self, env: &Env, _panel_open: bool) -> Option<u64> {
        let into_minute = env.wall_ms.rem_euclid(60_000) as u64;
        Some(env.now_ms + 60_000 - into_minute + 1)
    }

    fn accessibility(&mut self, env: &Env, open: bool) -> Vec<Node> {
        let time = hhmm(env.wall_ms, env.tz);
        if open {
            vec![Node::new(2, ROOT, Role::Text, format!("{time}, {}", long_date(env.wall_ms, env.tz, env.t.lang())))]
        } else {
            let mut pill = Node::new(1, ROOT, Role::Button, format!("{} {time}", self.title(env.t.lang())));
            pill.action = Some(NodeAction::OpenPanel);
            vec![pill]
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn time_and_dates_follow_the_zone_and_the_language() {
        // 2026-10-22 12:00 UTC is 19:00 Thursday in Jakarta.
        let wall = 1_792_670_400_000;
        let tz = TimeZone::get("Asia/Jakarta").unwrap();
        assert_eq!(hhmm(wall, &tz), "19:00");
        assert_eq!(short_date(wall, &tz, Lang::En), "Thu 22 Oct");
        assert_eq!(short_date(wall, &tz, Lang::Id), "Kam 22 Okt");
        assert_eq!(long_date(wall, &tz, Lang::Id), "Kamis 22 Oktober");
        let ny = TimeZone::get("America/New_York").unwrap();
        assert_eq!(hhmm(wall, &ny), "08:00");
    }
}
