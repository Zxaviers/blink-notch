//! The clock: what the resting pill shows when nothing needs your attention,
//! and the first page of the notch: the time, the date and a month calendar that marks the
//! days with deadlines.

use std::sync::mpsc::{channel, Receiver, Sender};
use std::sync::Arc;

use jiff::civil::Date;
use jiff::{tz::TimeZone, Timestamp};

use super::{Effect, Env, Key, KeyResult, Kit, Module, PanelFrame, PeekView, PointerKind, RestView, CONTENT_H};
use crate::access::{Node, NodeAction, Role, ROOT};
use crate::ui::canvas::Icon;
use crate::ui::text::Face;
use crate::ui::theme::Rgb;
use blink_core::i18n::Lang;
use blink_core::settings::Settings;
use blink_core::weather::{c_to_f, describe, sky_of, Place, Sky, Weather, WeatherSource};

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
    ShowWeather,
    ShowCalendar,
    /// The message that the weather could not be loaded (click to ask again).
    RetryWeather,
    /// The place under the weather: opens the settings where the city is chosen.
    ChoosePlace,
}

/// What the right half of the page shows.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Pane {
    Weather,
    Calendar,
}

/// The weather is refreshed when it is older than this and the page is looked at.
const WEATHER_MAX_AGE_MS: i64 = 30 * 60_000;
/// After a failure, wait this long before asking again by itself.
const WEATHER_RETRY_MS: i64 = 5 * 60_000;

/// A place chosen in the settings.
#[derive(Clone, Debug, PartialEq)]
struct ChosenPlace {
    name: String,
    lat: f64,
    lon: f64,
}

type WeatherResult = Result<(String, Weather), String>;

/// A stand-in for tests and scripted runs: the same forecast every time, and it remembers what it was asked.
pub struct FakeWeather {
    pub asked: std::sync::Mutex<Vec<String>>,
    pub fail: std::sync::atomic::AtomicBool,
}

impl FakeWeather {
    pub fn new() -> Arc<FakeWeather> {
        Arc::new(FakeWeather { asked: std::sync::Mutex::new(Vec::new()), fail: std::sync::atomic::AtomicBool::new(false) })
    }
}

impl WeatherSource for FakeWeather {
    fn locate(&self, name: &str, _lang: Lang) -> Result<Place, String> {
        self.asked.lock().unwrap().push(format!("locate:{name}"));
        Ok(Place { name: name.to_string(), admin1: String::new(), country: "Testland".into(), lat: -7.98, lon: 112.63 })
    }

    fn forecast(&self, lat: f64, lon: f64) -> Result<Weather, String> {
        self.asked.lock().unwrap().push(format!("forecast:{lat:.2},{lon:.2}"));
        if self.fail.load(std::sync::atomic::Ordering::SeqCst) {
            return Err("offline".into());
        }
        let day = |date: &str, code: i64, hi: f32, lo: f32| blink_core::weather::Day { date: date.into(), code, hi_c: hi, lo_c: lo, rain_pct: 30 };
        Ok(Weather {
            current: blink_core::weather::Current { temp_c: 24.6, feels_c: 27.0, humidity: 83, wind_kmh: 7.0, code: 2, is_day: true },
            days: vec![day("2026-10-22", 2, 29.5, 21.0), day("2026-10-23", 61, 28.0, 21.5), day("2026-10-24", 95, 27.0, 21.0), day("2026-10-25", 3, 29.0, 22.0), day("2026-10-26", 0, 31.0, 22.0)],
        })
    }
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
    pane: Pane,
    /// The accent the person chose for the chrome, from the last frame (None: this page's own blue).
    accent_override: Option<Rgb>,
    weather_source: Option<Arc<dyn WeatherSource>>,
    wake: Option<Arc<dyn Fn() + Send + Sync>>,
    chosen: Option<ChosenPlace>,
    fahrenheit: bool,
    /// What was last read: the place's name and the forecast, and when.
    weather: Option<(String, Weather, i64)>,
    fetching: bool,
    failed_at: Option<i64>,
    results_tx: Sender<WeatherResult>,
    results_rx: Receiver<WeatherResult>,
}

impl Default for ClockModule {
    fn default() -> Self {
        let (results_tx, results_rx) = channel();
        ClockModule {
            eyes: true,
            marks: Vec::new(),
            month_offset: 0,
            hits: Vec::new(),
            pointer: None,
            pane: Pane::Calendar,
            accent_override: None,
            weather_source: None,
            wake: None,
            chosen: None,
            fahrenheit: false,
            weather: None,
            fetching: false,
            failed_at: None,
            results_tx,
            results_rx,
        }
    }
}

impl ClockModule {
    pub fn new() -> Self {
        ClockModule::default()
    }

    /// Show the weather (the page then opens on it): where it comes from, and how to wake the app when it arrives.
    pub fn with_weather(mut self, source: Arc<dyn WeatherSource>, wake: Arc<dyn Fn() + Send + Sync>) -> Self {
        self.weather_source = Some(source);
        self.wake = Some(wake);
        self.pane = Pane::Weather;
        self
    }

    pub fn pane(&self) -> Pane {
        self.pane
    }

    /// Ask for the weather when there is none or it is old, unless a request is out or one failed a moment ago.
    fn ensure_weather(&mut self, env: &Env) {
        let Some(source) = self.weather_source.clone() else { return };
        let stale = self.weather.as_ref().is_none_or(|(_, _, at)| env.wall_ms - at > WEATHER_MAX_AGE_MS);
        let cooling = self.failed_at.is_some_and(|at| env.wall_ms - at < WEATHER_RETRY_MS);
        if !stale || self.fetching || cooling {
            return;
        }
        self.fetching = true;
        let (chosen, lang, tx, wake) = (self.chosen.clone(), env.t.lang(), self.results_tx.clone(), self.wake.clone());
        // Until a city is chosen, use the one the time zone names ("Asia/Jakarta" is Jakarta).
        let guess = env.tz.iana_name().and_then(|n| n.rsplit('/').next()).map(|c| c.replace('_', " "));
        let _ = std::thread::Builder::new().name("blink-weather".into()).spawn(move || {
            let result = (|| {
                let (name, lat, lon) = match chosen {
                    Some(c) => (c.name, c.lat, c.lon),
                    None => {
                        let city = guess.ok_or("no place")?;
                        let place = source.locate(&city, lang)?;
                        (format!("{} (time zone)", place.name), place.lat, place.lon)
                    }
                };
                source.forecast(lat, lon).map(|w| (name, w))
            })();
            let _ = tx.send(result);
            if let Some(wake) = wake {
                wake();
            }
        });
    }

    fn drain_weather(&mut self, env: &Env) -> bool {
        let mut any = false;
        while let Ok(result) = self.results_rx.try_recv() {
            self.fetching = false;
            any = true;
            match result {
                Ok((name, w)) => {
                    self.weather = Some((name, w, env.wall_ms));
                    self.failed_at = None;
                }
                Err(_) => self.failed_at = Some(env.wall_ms),
            }
        }
        any
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

impl ClockModule {
    /// The two-way switch at the top right of the page: weather or calendar.
    fn draw_toggle(&mut self, kit: &mut Kit, frame: &PanelFrame, x: f32, y: f32) {
        let theme = frame.theme;
        let (w, h) = (54.0, 22.0);
        kit.canvas.rrect(x, y, w, h, h / 2.0, theme.fg, frame.alpha * 0.1);
        for (i, (icon, hit)) in [(Icon::Cloud, ClockHit::ShowWeather), (Icon::Calendar, ClockHit::ShowCalendar)].into_iter().enumerate() {
            let cx = x + 14.0 + i as f32 * 26.0;
            let active = (hit == ClockHit::ShowWeather) == (self.pane == Pane::Weather);
            let over = self.over(cx - 13.0, y, 26.0, h);
            if active {
                kit.canvas.rrect(cx - 12.0, y + 2.0, 24.0, h - 4.0, (h - 4.0) / 2.0, self.accent(), frame.alpha * 0.95);
            } else if over {
                kit.canvas.rrect(cx - 12.0, y + 2.0, 24.0, h - 4.0, (h - 4.0) / 2.0, theme.fg, frame.alpha * 0.12);
            }
            kit.canvas.icon(icon, cx, y + h / 2.0, 12.0, if active { Rgb(0x06, 0x12, 0x20) } else { theme.fg }, frame.alpha);
            self.hits.push((cx - 13.0, y, 26.0, h, hit));
        }
    }

    /// The weather: now (with a picture, the temperature, how it feels) and the next days.
    fn draw_weather(&mut self, kit: &mut Kit, env: &Env, frame: &PanelFrame) {
        let theme = frame.theme;
        let lang = env.t.lang();
        let id = lang == Lang::Id;
        let w = 238.0;
        let x1 = frame.content_x() + frame.content_w() - w;
        let top = frame.origin_y;
        let cx = x1 + w / 2.0;
        let Some((place, wx, _)) = self.weather.clone() else {
            // Nothing yet: loading, or it did not work (a click asks again).
            let (msg, retry) = if self.failed_at.is_some() {
                (if id { "Cuaca tidak bisa dimuat. Klik untuk coba lagi." } else { "Couldn't load the weather. Click to try again." }, true)
            } else {
                (if id { "Memuat cuaca\u{2026}" } else { "Loading weather\u{2026}" }, false)
            };
            kit.canvas.icon(Icon::Cloud, cx, top + 52.0, 30.0, theme.dim, frame.alpha);
            put_centered(kit, Face::Sans, 12.0, cx, top + 70.0, 18.0, msg, theme.dim, frame.alpha);
            if retry {
                self.hits.push((x1, top + 24.0, w, 90.0, ClockHit::RetryWeather));
            }
            return;
        };
        let temp = |c: f32| {
            let v = if self.fahrenheit { c_to_f(c) } else { c };
            format!("{}\u{00B0}", v.round() as i32)
        };
        let sky_icon = |sky: Sky, day: bool| match sky {
            Sky::Clear => if day { Icon::Sun } else { Icon::Moon },
            Sky::PartlyCloudy => Icon::CloudSun,
            Sky::Cloudy => Icon::Cloud,
            Sky::Fog => Icon::Fog,
            Sky::Drizzle | Sky::Rain => Icon::Rain,
            Sky::Snow => Icon::Snow,
            Sky::Storm => Icon::Storm,
        };
        let sky_color = |sky: Sky, day: bool| match sky {
            Sky::Clear => if day { Rgb(0xf5, 0xb8, 0x4a) } else { Rgb(0xc9, 0xd3, 0xff) },
            Sky::PartlyCloudy => Rgb(0xf0, 0xc8, 0x7a),
            Sky::Cloudy | Sky::Fog => Rgb(0xb4, 0xbe, 0xcf),
            Sky::Drizzle | Sky::Rain => Rgb(0x6c, 0xb6, 0xff),
            Sky::Snow => Rgb(0xdf, 0xee, 0xff),
            Sky::Storm => Rgb(0xb7, 0x9c, 0xff),
        };

        // Now: picture and temperature at the left, the words at the right.
        let cur = &wx.current;
        let sky = sky_of(cur.code);
        let color = sky_color(sky, cur.is_day);
        kit.canvas.glow(x1 + 26.0, top + 40.0, 34.0, color, frame.alpha * 0.22);
        kit.canvas.icon(sky_icon(sky, cur.is_day), x1 + 26.0, top + 40.0, 34.0, color, frame.alpha);
        let t = temp(cur.temp_c);
        let tbase = top + 14.0 + kit.fonts.baseline(Face::SansLight, 40.0, 50.0);
        kit.canvas.text(kit.fonts, Face::SansLight, 40.0, x1 + 52.0, tbase, &t, theme.fg, frame.alpha, 0.0);
        let rx = x1 + 118.0;
        let rw = w - 118.0;
        put(kit, Face::SansBold, 12.5, rx, top + 22.0, 18.0, rw, describe(cur.code, lang), theme.fg, frame.alpha);
        let feels = format!("{} {}", if id { "Terasa" } else { "Feels" }, temp(cur.feels_c));
        put(kit, Face::Sans, 10.5, rx, top + 40.0, 15.0, rw, &feels, theme.dim, frame.alpha);
        let wind = if self.fahrenheit { format!("{} mph", (cur.wind_kmh * 0.621).round() as i32) } else { format!("{} km/j", cur.wind_kmh.round() as i32) };
        let wind = if id { wind } else { wind.replace("km/j", "km/h") };
        put(kit, Face::Sans, 10.5, rx, top + 55.0, 15.0, rw, &format!("{}% \u{00B7} {wind}", cur.humidity), theme.dim, frame.alpha);

        // The place, small, under the temperature.
        let over_place = self.over(x1, top + 63.0, w, 18.0);
        put(kit, Face::Sans, 10.0, x1, top + 65.0, 14.0, w - 4.0, &place, if over_place { theme.fg } else { theme.dim }, frame.alpha * if over_place { 1.0 } else { 0.8 });
        self.hits.push((x1, top + 63.0, w, 18.0, ClockHit::ChoosePlace));

        // The next days.
        let names = if id { ["Sen", "Sel", "Rab", "Kam", "Jum", "Sab", "Min"] } else { ["Mon", "Tue", "Wed", "Thu", "Fri", "Sat", "Sun"] };
        let col = w / 5.0;
        for (i, d) in wx.days.iter().take(5).enumerate() {
            let dx = x1 + col * (i as f32 + 0.5);
            let dsky = sky_of(d.code);
            if i == 0 {
                kit.canvas.rrect(dx - col / 2.0 + 2.0, top + 83.0, col - 4.0, 43.0, 10.0, theme.fg, frame.alpha * 0.08);
            }
            let name = d.weekday().map_or("", |wd| names[wd]);
            put_centered(kit, Face::Sans, 10.0, dx, top + 85.0, 12.0, name, theme.dim, frame.alpha);
            kit.canvas.icon(sky_icon(dsky, true), dx, top + 106.0, 14.0, sky_color(dsky, true), frame.alpha);
            let (hi, lo) = (temp(d.hi_c), temp(d.lo_c));
            let hi_w = kit.canvas.measure(kit.fonts, Face::SansSemibold, 10.5, &hi, 0.0);
            let lo_w = kit.canvas.measure(kit.fonts, Face::Sans, 10.5, &lo, 0.0);
            let x0 = dx - (hi_w + 3.0 + lo_w) / 2.0;
            let base = top + 113.0 + kit.fonts.baseline(Face::Sans, 10.5, 13.0);
            kit.canvas.text(kit.fonts, Face::SansSemibold, 10.5, x0, base, &hi, theme.fg, frame.alpha, 0.0);
            kit.canvas.text(kit.fonts, Face::Sans, 10.5, x0 + hi_w + 3.0, base, &lo, theme.dim, frame.alpha, 0.0);
        }
    }

    /// The month, with arrows to browse and a click on its name to come back.
    fn draw_calendar(&mut self, kit: &mut Kit, env: &Env, frame: &PanelFrame) {
        let theme = frame.theme;
        let lang = env.t.lang();
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
            let bx = x1 + cal_w - 40.0 - 62.0 + i as f32 * 20.0;
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

}

impl Module for ClockModule {
    fn id(&self) -> &'static str {
        "clock"
    }

    fn title(&self, lang: Lang) -> String {
        if lang == Lang::Id { "Jam" } else { "Clock" }.to_string()
    }

    fn accent(&self) -> Rgb {
        self.accent_override.unwrap_or(Rgb(0x4e, 0xa8, 0xff))
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
        let place = options.get("weather").and_then(|w| {
            let (lat, lon) = (w.get("lat")?.as_f64()?, w.get("lon")?.as_f64()?);
            Some(ChosenPlace { name: w.get("name").and_then(serde_json::Value::as_str).unwrap_or("").to_string(), lat, lon })
        });
        if place != self.chosen {
            // A different place: what was read belongs to the old one.
            self.chosen = place;
            self.weather = None;
            self.failed_at = None;
        }
        self.fahrenheit = options.get("weather").and_then(|w| w.get("units")).and_then(serde_json::Value::as_str) == Some("f");
        if self.weather_source.is_some() {
            self.pane = if options.get("pane").and_then(serde_json::Value::as_str) == Some("calendar") { Pane::Calendar } else { Pane::Weather };
        }
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
        self.accent_override = frame.accent;

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

        // The right: the weather or the month, with a little switch for the two.
        let toggle_x = frame.content_x() + frame.content_w() - 54.0;
        self.draw_toggle(kit, frame, toggle_x, frame.origin_y - 1.0);
        if self.pane == Pane::Weather && self.weather_source.is_some() {
            self.ensure_weather(env);
            self.draw_weather(kit, env, frame);
        } else {
            self.draw_calendar(kit, env, frame);
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
                    Some(ClockHit::ShowWeather) => self.pane = Pane::Weather,
                    Some(ClockHit::ShowCalendar) => self.pane = Pane::Calendar,
                    Some(ClockHit::RetryWeather) => {
                        self.failed_at = None;
                    }
                    Some(ClockHit::ChoosePlace) => return vec![Effect::OpenSettingsAt("weather".into())],
                    None => return Vec::new(),
                }
                vec![Effect::Redraw]
            }
            PointerKind::Wheel(delta) => {
                // The wheel browses months (up for earlier), when the month is showing.
                if self.pane == Pane::Calendar || self.weather_source.is_none() {
                    self.month_offset += if delta > 0.0 { -1 } else { 1 };
                }
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

    fn tick(&mut self, env: &Env) -> Vec<Effect> {
        if self.drain_weather(env) {
            return vec![Effect::Redraw];
        }
        Vec::new()
    }

    fn debug_now(&mut self, _env: &Env) -> serde_json::Value {
        serde_json::json!({
            "pane": format!("{:?}", self.pane),
            "monthOffset": self.month_offset,
            "fetching": self.fetching,
            "failed": self.failed_at.is_some(),
            "place": self.weather.as_ref().map(|(p, _, _)| p.clone()),
            "temp": self.weather.as_ref().map(|(_, w, _)| w.current.temp_c),
            "hits": self.hits.iter().map(|h| serde_json::json!({ "x": h.0, "y": h.1, "w": h.2, "h": h.3, "target": format!("{:?}", h.4) })).collect::<Vec<_>>(),
        })
    }

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

/// Text at `x`, in a line box of `line_h` starting at `y`, cut with an ellipsis to fit `max_w`.
#[allow(clippy::too_many_arguments)]
fn put(kit: &mut Kit, face: Face, size: f32, x: f32, y: f32, line_h: f32, max_w: f32, text: &str, color: Rgb, alpha: f32) {
    let fitted = kit.fonts.ellipsize(face, text, size * kit.canvas.scale, 0.0, max_w * kit.canvas.scale);
    let base = y + kit.fonts.baseline(face, size, line_h);
    kit.canvas.text(kit.fonts, face, size, x, base, &fitted, color, alpha, 0.0);
}

/// Text centred on `cx`.
#[allow(clippy::too_many_arguments)]
fn put_centered(kit: &mut Kit, face: Face, size: f32, cx: f32, y: f32, line_h: f32, text: &str, color: Rgb, alpha: f32) {
    let w = kit.canvas.measure(kit.fonts, face, size, text, 0.0);
    let base = y + kit.fonts.baseline(face, size, line_h);
    kit.canvas.text(kit.fonts, face, size, cx - w / 2.0, base, text, color, alpha, 0.0);
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
