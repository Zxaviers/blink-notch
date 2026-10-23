//! The clock: what the resting pill shows when nothing needs your attention,
//! and the first page of the notch: the time, the date and a month calendar that marks the
//! days with deadlines.

use jiff::civil::Date;
use jiff::{tz::TimeZone, Timestamp};

use super::{Effect, Env, Key, KeyResult, Kit, Module, PanelFrame, PeekView, PointerKind, RestView, CONTENT_H};
use crate::access::{Node, NodeAction, Role, ROOT};
use crate::ui::canvas::Icon;
use crate::ui::text::Face;
use crate::ui::theme::Rgb;
use blink_core::i18n::Lang;
use blink_core::settings::Settings;

const DAYS_EN: [&str; 7] = ["Mon", "Tue", "Wed", "Thu", "Fri", "Sat", "Sun"];
const DAYS_ID: [&str; 7] = ["Sen", "Sel", "Rab", "Kam", "Jum", "Sab", "Min"];
const INITIALS_EN: [&str; 7] = ["M", "T", "W", "T", "F", "S", "S"];
const INITIALS_ID: [&str; 7] = ["S", "S", "R", "K", "J", "S", "M"];
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
    year: i16,
}

fn now_in(wall_ms: i64, tz: &TimeZone) -> Now {
    let z = Timestamp::from_millisecond(wall_ms).unwrap_or_default().to_zoned(tz.clone());
    Now {
        hour: z.hour(),
        minute: z.minute(),
        weekday: z.weekday().to_monday_zero_offset() as usize,
        day: z.day(),
        month: z.month() as usize - 1,
        year: z.year(),
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

/// A month on the calendar: which weekday it starts on (0 = Monday) and how many days it has.
pub fn month_layout(year: i16, month: i8) -> Option<(usize, i8)> {
    let first = Date::new(year, month, 1).ok()?;
    Some((first.weekday().to_monday_zero_offset() as usize, first.days_in_month()))
}

/// The month `offset` months from the one holding `wall_ms`: (year, month 1..=12).
pub fn shifted_month(wall_ms: i64, tz: &TimeZone, offset: i32) -> (i16, i8) {
    let n = now_in(wall_ms, tz);
    let total = i32::from(n.year) * 12 + n.month as i32 + offset;
    (total.div_euclid(12) as i16, (total.rem_euclid(12) + 1) as i8)
}

fn urgency_rank(urgency: &str) -> u8 {
    match urgency {
        "overdue" => 3,
        "soon" => 2,
        _ => 1,
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ClockHit {
    PrevMonth,
    NextMonth,
    ThisMonth,
}

pub struct ClockModule {
    /// Whether the notch shows eyes after a while of quiet (`eyes` in the clock's options).
    pub eyes: bool,
    /// Deadlines to mark on the calendar: when, and how pressing (set by the host from the modules that have them).
    marks: Vec<(i64, &'static str)>,
    /// Which month the calendar shows, in months from the current one.
    month_offset: i32,
    hits: Vec<(f32, f32, f32, f32, ClockHit)>,
    pointer: Option<(f32, f32)>,
}

impl Default for ClockModule {
    fn default() -> Self {
        ClockModule { eyes: true, marks: Vec::new(), month_offset: 0, hits: Vec::new(), pointer: None }
    }
}

impl ClockModule {
    pub fn new() -> Self {
        ClockModule::default()
    }

    /// The deadlines the calendar should mark.
    pub fn set_marks(&mut self, marks: Vec<(i64, &'static str)>) {
        self.marks = marks;
    }

    pub fn month_offset(&self) -> i32 {
        self.month_offset
    }

    fn over(&self, x: f32, y: f32, w: f32, h: f32) -> bool {
        self.pointer.is_some_and(|(px, py)| px >= x && px < x + w && py >= y && py < y + h)
    }
}

impl Module for ClockModule {
    fn id(&self) -> &'static str {
        "clock"
    }

    fn title(&self, lang: Lang) -> String {
        if lang == Lang::Id { "Jam" } else { "Clock" }.to_string()
    }

    fn accent(&self) -> Rgb {
        Rgb(0x4e, 0xa8, 0xff)
    }

    fn title_parts(&self, lang: Lang) -> (String, String) {
        let (a, b) = if lang == Lang::Id { ("J", "am") } else { ("Cl", "ock") };
        (a.to_string(), b.to_string())
    }

    fn as_any_mut(&mut self) -> &mut dyn std::any::Any {
        self
    }

    fn set_options(&mut self, options: &serde_json::Value) {
        self.eyes = options.get("eyes").and_then(serde_json::Value::as_bool).unwrap_or(true);
    }

    /// Always something to show, but never more important than a deadline.
    fn rest_priority(&mut self, _env: &Env, _settings: &Settings) -> u8 {
        1
    }

    fn rest_style(&mut self, _env: &Env, _resting_style: &str, _due_horizon_hours: f64) -> crate::shell::RestStyle {
        crate::shell::RestStyle::Full
    }

    fn rest(&mut self, env: &Env) -> RestView {
        RestView { label: String::new(), count: hhmm(env.wall_ms, env.tz), urgency: "later" }
    }

    fn peek(&mut self, _env: &Env) -> Option<PeekView> {
        None
    }

    fn panel_height(&mut self, _env: &Env, _kit: &mut Kit) -> f32 {
        CONTENT_H
    }

    fn draw_panel(&mut self, env: &Env, kit: &mut Kit, frame: &PanelFrame) {
        let theme = frame.theme;
        let lang = env.t.lang();
        self.hits.clear();

        // The left: the time, big and light, with the date under it.
        let left_w = 200.0;
        let cx = frame.content_x() + left_w / 2.0;
        let time = hhmm(env.wall_ms, env.tz);
        let size = 54.0;
        let w = kit.canvas.measure(kit.fonts, Face::SansLight, size, &time, 0.0);
        let base = frame.origin_y + 8.0 + kit.fonts.baseline(Face::SansLight, size, 70.0);
        kit.canvas.text(kit.fonts, Face::SansLight, size, cx - w / 2.0, base, &time, theme.fg, frame.alpha, 0.0);
        let date = long_date(env.wall_ms, env.tz, lang);
        let dw = kit.canvas.measure(kit.fonts, Face::Sans, 13.0, &date, 0.0);
        let dbase = frame.origin_y + 80.0 + kit.fonts.baseline(Face::Sans, 13.0, 20.0);
        kit.canvas.text(kit.fonts, Face::Sans, 13.0, cx - dw / 2.0, dbase, &date, theme.dim, frame.alpha, 0.0);

        // The right: the month.
        let cal_w = 238.0;
        let x1 = frame.content_x() + frame.content_w() - cal_w;
        let top = frame.origin_y;
        let (year, month) = shifted_month(env.wall_ms, env.tz, self.month_offset);
        let today = now_in(env.wall_ms, env.tz);
        let accent = self.accent();
        let months = if lang == Lang::Id { &MONTHS_LONG_ID } else { &MONTHS_LONG_EN };
        let title = format!("{} {year}", months[(month - 1) as usize]);
        // Title, with arrows to browse and a click on the name to come back.
        let tw = kit.canvas.measure(kit.fonts, Face::SansBold, 11.5, &title, 0.0);
        let tbase = top + 1.0 + kit.fonts.baseline(Face::SansBold, 11.5, 16.0);
        let title_hover = self.over(x1, top, tw + 6.0, 17.0);
        kit.canvas.text(kit.fonts, Face::SansBold, 11.5, x1 + 2.0, tbase, &title, if title_hover && self.month_offset != 0 { accent } else { theme.fg }, frame.alpha, 0.0);
        self.hits.push((x1, top, tw + 6.0, 17.0, ClockHit::ThisMonth));
        for (i, (icon, hit)) in [(Icon::ChevronLeft, ClockHit::PrevMonth), (Icon::ChevronRight, ClockHit::NextMonth)].into_iter().enumerate() {
            let bx = x1 + cal_w - 40.0 + i as f32 * 20.0;
            let over = self.over(bx, top - 1.0, 20.0, 19.0);
            kit.canvas.circle(bx + 10.0, top + 8.5, 8.5, theme.fg, frame.alpha * if over { 0.2 } else { 0.08 });
            kit.canvas.icon(icon, bx + 10.0, top + 8.5, 10.0, theme.fg, frame.alpha);
            self.hits.push((bx, top - 1.0, 20.0, 19.0, hit));
        }
        // Weekday initials.
        let initials = if lang == Lang::Id { &INITIALS_ID } else { &INITIALS_EN };
        let col_w = cal_w / 7.0;
        for (i, ch) in initials.iter().enumerate() {
            let cxm = x1 + col_w * (i as f32 + 0.5);
            let base = top + 19.0 + kit.fonts.baseline(Face::Sans, 10.0, 13.0);
            let w = kit.canvas.measure(kit.fonts, Face::Sans, 10.0, ch, 0.0);
            kit.canvas.text(kit.fonts, Face::Sans, 10.0, cxm - w / 2.0, base, ch, if i >= 5 { theme.late } else { theme.dim }, frame.alpha * 0.9, 0.0);
        }
        // The days.
        let Some((first, days)) = month_layout(year, month) else { return };
        // The most pressing deadline on each day of this month.
        let mut mark = [0u8; 32];
        for (due_ms, urgency) in &self.marks {
            let z = Timestamp::from_millisecond(*due_ms).unwrap_or_default().to_zoned(env.tz.clone());
            if z.year() == year && z.month() == month {
                let d = z.day() as usize;
                mark[d] = mark[d].max(urgency_rank(urgency));
            }
        }
        let row_h = 15.0;
        let grid_top = top + 33.0;
        for d in 1..=days {
            let idx = first + (d as usize - 1);
            let (row, col) = (idx / 7, idx % 7);
            let cxm = x1 + col_w * (col as f32 + 0.5);
            let cym = grid_top + row as f32 * row_h + row_h / 2.0;
            let is_today = self.month_offset == 0 && today.day == d;
            let rank = mark[d as usize];
            let tone = match rank {
                3 => theme.late,
                2 => theme.soon,
                1 => Rgb(0x4e, 0xa8, 0xff),
                _ => theme.fg,
            };
            if is_today {
                kit.canvas.circle(cxm, cym, 7.4, if rank > 0 { tone } else { accent }, frame.alpha * 0.95);
            }
            let label = d.to_string();
            let w = kit.canvas.measure(kit.fonts, Face::Sans, 10.5, &label, 0.0);
            let base = cym - 8.0 + kit.fonts.baseline(Face::Sans, 10.5, 16.0);
            let color = if is_today {
                theme.bg
            } else if rank > 0 {
                tone
            } else if col >= 5 {
                mix_dim(theme.fg, theme.late)
            } else {
                theme.fg
            };
            kit.canvas.text(kit.fonts, Face::Sans, 10.5, cxm - w / 2.0, base, &label, color, frame.alpha * if is_today || rank > 0 { 1.0 } else { 0.85 }, 0.0);
            if rank > 0 && !is_today {
                kit.canvas.circle(cxm, cym + 6.6, 1.25, tone, frame.alpha);
            }
        }
    }

    fn pointer(&mut self, kind: PointerKind, x: f32, y: f32, _env: &Env) -> Vec<Effect> {
        match kind {
            PointerKind::Leave => {
                self.pointer = None;
                vec![Effect::Redraw]
            }
            PointerKind::Move => {
                self.pointer = Some((x, y));
                vec![Effect::Redraw]
            }
            PointerKind::Down => {
                let hit = self.hits.iter().rev().find(|(hx, hy, hw, hh, _)| x >= *hx && x < hx + hw && y >= *hy && y < hy + hh).map(|h| h.4);
                match hit {
                    Some(ClockHit::PrevMonth) => self.month_offset -= 1,
                    Some(ClockHit::NextMonth) => self.month_offset += 1,
                    Some(ClockHit::ThisMonth) => self.month_offset = 0,
                    None => return Vec::new(),
                }
                vec![Effect::Redraw]
            }
            PointerKind::Wheel(delta) => {
                // The wheel browses months (up for earlier).
                self.month_offset += if delta > 0.0 { -1 } else { 1 };
                vec![Effect::Redraw]
            }
            PointerKind::Up => Vec::new(),
        }
    }

    fn key(&mut self, key: Key, _env: &Env) -> KeyResult {
        let handled = |effects: Vec<Effect>| KeyResult { handled: true, close: false, effects };
        match key {
            Key::Up => {
                self.month_offset -= 1;
                handled(vec![Effect::Redraw])
            }
            Key::Down => {
                self.month_offset += 1;
                handled(vec![Effect::Redraw])
            }
            Key::Home => {
                self.month_offset = 0;
                handled(vec![Effect::Redraw])
            }
            // Escape closes the notch.
            _ => KeyResult { handled: false, close: key == Key::Escape, effects: Vec::new() },
        }
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
            let lang = env.t.lang();
            let (year, month) = shifted_month(env.wall_ms, env.tz, self.month_offset);
            let months = if lang == Lang::Id { &MONTHS_LONG_ID } else { &MONTHS_LONG_EN };
            vec![
                Node::new(2, ROOT, Role::Text, format!("{time}, {}", long_date(env.wall_ms, env.tz, lang))),
                Node::new(3, ROOT, Role::Text, format!("{} {year}", months[(month - 1) as usize])),
            ]
        } else {
            let mut pill = Node::new(1, ROOT, Role::Button, format!("{} {time}", self.title(env.t.lang())));
            pill.action = Some(NodeAction::OpenPanel);
            vec![pill]
        }
    }
}

/// A weekday number a little toward the weekend colour, so Saturday and Sunday read as such.
fn mix_dim(a: Rgb, b: Rgb) -> Rgb {
    let m = |x: u8, y: u8| (f32::from(x) * 0.6 + f32::from(y) * 0.4).round() as u8;
    Rgb(m(a.0, b.0), m(a.1, b.1), m(a.2, b.2))
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

    #[test]
    fn the_calendar_knows_where_a_month_starts_and_how_long_it_is() {
        // October 2026 starts on a Thursday (Monday = 0) and has 31 days; February 2028 has 29.
        assert_eq!(month_layout(2026, 10), Some((3, 31)));
        assert_eq!(month_layout(2028, 2), Some((1, 29)));
        let wall = 1_792_670_400_000;
        let tz = TimeZone::get("Asia/Jakarta").unwrap();
        assert_eq!(shifted_month(wall, &tz, 0), (2026, 10));
        assert_eq!(shifted_month(wall, &tz, 3), (2027, 1));
        assert_eq!(shifted_month(wall, &tz, -10), (2025, 12));
    }
}
