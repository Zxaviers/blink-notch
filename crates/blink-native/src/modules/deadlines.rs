//! The deadlines module: the resting pill, peeks and the open panel (hero
//! block, rows, overdue and done sections, snooze chooser, footer), drawn with
//! the kit. Port of `src/modules/deadlines.js` and the panel CSS.
//!
//! Data comes through `DeadlineSource`: Phase 2 ships an in-memory demo; Phase 3
//! plugs in feeds, persistence and notifications behind the same trait.
//!
//! Coordinates: logical pixels in the window. While drawing, `y` is already
//! shifted by the scroll offset, so hit rectangles are recorded as drawn.

use std::collections::HashMap;

use blink_core::countdown::{compact_countdown, due_label, urgency_of, MINUTE_MS};
use blink_core::format::{pill_label, sync_summary, FeedSync};
use blink_core::ics::Task;
use blink_core::local_events::LocalEvent;
use blink_core::merge::FeedResult;
use blink_core::planner::{is_snoozed, snooze_until, SNOOZE_KINDS};
use blink_core::settings::Settings;
use blink_core::view::{build_view, View};

use crate::access::{self, key_for, Node, NodeAction, Role, ROOT};
use super::{Action, Effect, Env, Key, KeyResult, Kit, Module, PanelFrame, PeekView, PointerKind, RestView, CONTENT_H};
use crate::ui::canvas::Icon;
use crate::ui::text::Face;
use crate::ui::theme::Rgb;

// ---------------------------------------------------------------- data

/// Where deadlines come from and where the user's choices go.
pub trait DeadlineSource {
    fn feeds(&self) -> Vec<FeedResult>;
    fn local_events(&self) -> Vec<LocalEvent>;
    fn done(&self) -> HashMap<String, String>;
    fn snoozed(&self) -> HashMap<String, f64>;
    fn sync_state(&self) -> Vec<FeedSync>;
    fn has_feeds(&self) -> bool;
    fn keep_overdue_days(&self) -> f64;
    fn mark_done(&mut self, id: &str, wall_ms: i64);
    fn restore(&mut self, id: &str);
    fn snooze(&mut self, id: &str, kind: &str, wall_ms: i64, tz: &jiff::tz::TimeZone);
    fn refresh(&mut self);

    // The rest is for real sources; the demo has nothing to fetch or announce.

    /// The user's settings changed (or arrived): feeds, thresholds, refresh interval.
    fn configure(&mut self, _settings: &Settings) {}
    /// Re-read the personal events file.
    fn reload_local(&mut self) {}
    /// Start whatever syncs are due. Called on every wake.
    fn poll(&mut self, _wall_ms: i64) {}
    /// The next wall-clock moment this source needs a wake: a refresh, a threshold crossing, a snooze ending.
    fn next_wake_wall(&self, _wall_ms: i64) -> Option<i64> {
        None
    }
    /// Run the notification planner; what comes back must be announced exactly once.
    fn announce(&mut self, _wall_ms: i64) -> Vec<Announcement> {
        Vec::new()
    }
}

/// A reminder the planner says is due.
#[derive(Clone, Debug, PartialEq)]
pub enum Announcement {
    /// One task crossed a threshold.
    Task(Task),
    /// More came due at once than are worth a notification each.
    Summary(usize),
}

/// Sample data relative to the clock, for the demo and the tests.
pub struct DemoSource {
    tasks: Vec<Task>,
    done: HashMap<String, String>,
    snoozed: HashMap<String, f64>,
    feeds: bool,
    pub refreshes: u32,
}

impl DemoSource {
    pub fn new(wall_ms: i64) -> Self {
        const H: i64 = 3_600_000;
        let mk = |uid: &str, title: &str, course: &str, due: i64, url: bool| Task {
            id: format!("{uid}::{due}"),
            uid: uid.to_string(),
            title: title.to_string(),
            course: (!course.is_empty()).then(|| course.to_string()),
            phase: "due".to_string(),
            due: blink_core::ics::iso_string(due),
            due_ms: due,
            url: url.then(|| format!("https://example.edu/mod/assign/view.php?id={uid}")),
            ..Task::default()
        };
        let tasks = vec![
            mk("t1", "Problem Set 4", "CS101 Algorithms", wall_ms + 3 * H + 14 * 60_000, true),
            mk("t2", "Quiz 3", "MA201 Linear Algebra", wall_ms + 30 * H, true),
            mk("t3", "Lab report", "PH110 Physics Lab", wall_ms + 4 * 24 * H, true),
            mk("t4", "Essay draft", "EN150 Writing", wall_ms - 5 * H, true),
            mk("t5", "Project proposal", "Basis Data", wall_ms + 11 * 24 * H, false),
            mk("t6", "Analisis dan Perancangan Sistem Informasi: Laporan Akhir Praktikum Modul 7", "SI301 Sistem Informasi", wall_ms + 2 * 24 * H, true),
        ];
        DemoSource { tasks, done: HashMap::new(), snoozed: HashMap::new(), feeds: true, refreshes: 0 }
    }

    pub fn without_feeds(mut self) -> Self {
        self.feeds = false;
        self.tasks.clear();
        self
    }

    pub fn with_tasks(mut self, tasks: Vec<Task>) -> Self {
        self.tasks = tasks;
        self
    }

    /// Ids of the sample tasks in creation order (tests).
    pub fn ids(&self) -> Vec<String> {
        self.tasks.iter().map(|t| t.id.clone()).collect()
    }
}

impl DeadlineSource for DemoSource {
    fn feeds(&self) -> Vec<FeedResult> {
        vec![FeedResult { feed_id: "demo".into(), feed_kind: Some("ics".into()), tasks: self.tasks.clone() }]
    }
    fn local_events(&self) -> Vec<LocalEvent> {
        Vec::new()
    }
    fn done(&self) -> HashMap<String, String> {
        self.done.clone()
    }
    fn snoozed(&self) -> HashMap<String, f64> {
        self.snoozed.clone()
    }
    fn sync_state(&self) -> Vec<FeedSync> {
        if self.feeds {
            vec![FeedSync { fetched_at_ms: Some(1.0), syncing: false, error: None }]
        } else {
            Vec::new()
        }
    }
    fn has_feeds(&self) -> bool {
        self.feeds
    }
    fn keep_overdue_days(&self) -> f64 {
        3.0
    }
    fn mark_done(&mut self, id: &str, wall_ms: i64) {
        self.done.insert(id.to_string(), blink_core::ics::iso_string(wall_ms));
    }
    fn restore(&mut self, id: &str) {
        self.done.remove(id);
    }
    fn snooze(&mut self, id: &str, kind: &str, wall_ms: i64, tz: &jiff::tz::TimeZone) {
        if let Some(until) = snooze_until(kind, wall_ms, tz) {
            self.snoozed.insert(id.to_string(), until as f64);
        }
    }
    fn refresh(&mut self) {
        self.refreshes += 1;
    }
}

// -------------------------------------------------------------- layout

const MAX_UPCOMING_ROWS: usize = 12;
/// How close a deadline has to be to take the resting pill from the clock.
pub const DEFAULT_URGENT_HOURS: f64 = 5.0;
const MAX_OVERDUE_ROWS: usize = 4;
/// The left column: the next deadline in big numbers, and the buttons.
const LEFT_W: f32 = 150.0;
const LEFT_GAP: f32 = 20.0;
const ROW_H: f32 = 36.0;
const ROW_GAP: f32 = 5.0;
const BTN: f32 = 26.0;
const BLUE: Rgb = Rgb(0x4e, 0xa8, 0xff);
const AMBER: Rgb = Rgb(0xf5, 0xb8, 0x4a);

#[derive(Clone, Copy, Debug, Default)]
struct Rect {
    x: f32,
    y: f32,
    w: f32,
    h: f32,
}

impl Rect {
    fn contains(&self, px: f32, py: f32) -> bool {
        px >= self.x && px < self.x + self.w && py >= self.y && py < self.y + self.h
    }
}

#[derive(Clone, Debug)]
struct Hit {
    rect: Rect,
    action: Option<Action>,
    row: Option<String>,
}

#[derive(Default)]
struct PanelState {
    scroll: f32,
    selected: Option<String>,
    hover_row: Option<String>,
    pointer: Option<(f32, f32)>,
    snooze_for: Option<String>,
    done_open: bool,
    keyboard: bool,
}

pub struct DeadlinesModule {
    source: Box<dyn DeadlineSource>,
    state: PanelState,
    hits: Vec<Hit>,
    /// Row rectangles in list space (scroll removed), to keep the selection in view.
    row_rects: HashMap<String, Rect>,
    /// Height of the whole list, from the last layout.
    content_h: f32,
    peek_task: Option<String>,
    /// A deadline this close (or already missed) takes the pill from the clock (`urgentHours`, 5 by default).
    urgent_hours: f64,
}

struct Model {
    view: View,
    sync: Vec<FeedSync>,
    has_feeds: bool,
    snoozed: HashMap<String, f64>,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Kind {
    Upcoming,
    Overdue,
    Done,
}

fn mix(a: Rgb, b: Rgb, t: f32) -> Rgb {
    let m = |x: u8, y: u8| (f32::from(x) + (f32::from(y) - f32::from(x)) * t).round() as u8;
    Rgb(m(a.0, b.0), m(a.1, b.1), m(a.2, b.2))
}

/// The colour of a deadline by how close it is: red when missed, amber when soon, blue otherwise.
fn tone(frame: &PanelFrame, urgency: &str) -> Rgb {
    match urgency {
        "overdue" => frame.theme.late,
        "soon" => frame.theme.soon,
        _ => {
            if frame.theme.surface == crate::ui::theme::Surface::Dark {
                BLUE
            } else {
                Rgb(0x1f, 0x6f, 0xd1)
            }
        }
    }
}

impl DeadlinesModule {
    pub fn new(source: Box<dyn DeadlineSource>) -> Self {
        DeadlinesModule { source, state: PanelState::default(), hits: Vec::new(), row_rects: HashMap::new(), content_h: 0.0, peek_task: None, urgent_hours: DEFAULT_URGENT_HOURS }
    }

    pub fn source_mut(&mut self) -> &mut dyn DeadlineSource {
        self.source.as_mut()
    }

    /// Ask for a peek about one task (a threshold was crossed).
    pub fn request_peek(&mut self, task_id: &str) {
        self.peek_task = Some(task_id.to_string());
    }

    pub fn selected(&self) -> Option<&str> {
        self.state.selected.as_deref()
    }

    pub fn scroll(&self) -> f32 {
        self.state.scroll
    }

    /// Where the named action's button was drawn last frame (window coordinates), for tests.
    pub fn hit_center(&self, action: &Action) -> Option<(f32, f32)> {
        self.hits.iter().rev().find(|h| h.action.as_ref() == Some(action)).map(|h| (h.rect.x + h.rect.w / 2.0, h.rect.y + h.rect.h / 2.0))
    }

    /// Centre of a row's area as drawn last frame, for tests.
    pub fn row_center(&self, id: &str) -> Option<(f32, f32)> {
        self.hits.iter().find(|h| h.row.as_deref() == Some(id) && h.rect.w > BTN * 2.0).map(|h| (h.rect.x + h.rect.w / 2.0, h.rect.y + h.rect.h / 2.0))
    }

    fn model(&self, env: &Env) -> Model {
        let view = build_view(
            &self.source.feeds(),
            &self.source.local_events(),
            &self.source.done(),
            self.source.keep_overdue_days(),
            env.wall_ms,
            env.tz,
        );
        Model { view, sync: self.source.sync_state(), has_feeds: self.source.has_feeds(), snoozed: self.source.snoozed() }
    }

    /// Rows the keyboard can select, in display order: upcoming (hero first), then overdue.
    fn selectable(model: &Model) -> Vec<String> {
        model
            .view
            .upcoming
            .iter()
            .take(MAX_UPCOMING_ROWS)
            .chain(model.view.overdue.iter().take(MAX_OVERDUE_ROWS))
            .map(|t| t.id.clone())
            .collect()
    }

    /// What the list shows: upcoming then overdue, or the done ones while they are open.
    fn entries<'a>(&self, model: &'a Model) -> Vec<(&'a Task, Kind)> {
        if self.state.done_open {
            return model.view.done_list.iter().map(|t| (t, Kind::Done)).collect();
        }
        model
            .view
            .upcoming
            .iter()
            .take(MAX_UPCOMING_ROWS)
            .map(|t| (t, Kind::Upcoming))
            .chain(model.view.overdue.iter().take(MAX_OVERDUE_ROWS).map(|t| (t, Kind::Overdue)))
            .collect()
    }

    fn list_height(count: usize) -> f32 {
        if count == 0 { 0.0 } else { count as f32 * ROW_H + (count as f32 - 1.0) * ROW_GAP }
    }

    fn base(kit: &Kit, face: Face, size: f32, line_h: f32) -> f32 {
        kit.fonts.baseline(face, size, line_h)
    }

    // ------------------------------------------------------------ pieces

    /// A round icon button with a hover background and a hit region.
    #[allow(clippy::too_many_arguments)]
    fn icon_button(&mut self, kit: &mut Kit, frame: &PanelFrame, x: f32, y: f32, icon: Icon, action: Action, row: Option<&str>, tint: Rgb) {
        let rect = Rect { x, y, w: BTN, h: BTN };
        let over = self.state.pointer.is_some_and(|(px, py)| rect.contains(px, py));
        kit.canvas.circle(x + BTN / 2.0, y + BTN / 2.0, BTN / 2.0 - 1.0, frame.theme.fg, frame.alpha * if over { 0.22 } else { 0.1 });
        kit.canvas.icon(icon, x + BTN / 2.0, y + BTN / 2.0, 13.0, if over { frame.theme.fg } else { tint }, frame.alpha);
        self.hits.push(Hit { rect, action: Some(action), row: row.map(String::from) });
    }

    /// A small pill with text. Returns its width.
    #[allow(clippy::too_many_arguments)]
    fn chip(&mut self, kit: &mut Kit, frame: &PanelFrame, x: f32, y: f32, label: &str, action: Action, row: &str) -> f32 {
        let size = 11.5;
        let w = kit.canvas.measure(kit.fonts, Face::Sans, size, label, 0.0) + 20.0;
        let h = 22.0;
        let rect = Rect { x, y, w, h };
        let over = self.state.pointer.is_some_and(|(px, py)| rect.contains(px, py));
        kit.canvas.rrect(x, y, w, h, h / 2.0, frame.theme.fg, frame.alpha * if over { 0.28 } else { 0.14 });
        let base = y + Self::base(kit, Face::Sans, size, h);
        kit.canvas.text(kit.fonts, Face::Sans, size, x + 10.0, base, label, frame.theme.fg, frame.alpha, 0.0);
        self.hits.push(Hit { rect, action: Some(action), row: Some(row.to_string()) });
        w
    }

    /// Open / snooze / done buttons, right-aligned so the last one ends at `right`.
    fn row_actions(&mut self, kit: &mut Kit, frame: &PanelFrame, task: &Task, right: f32, y: f32, urgency: &str, tint: Rgb) {
        let mut x = right - BTN;
        self.icon_button(kit, frame, x, y, Icon::Check, Action::MarkDone(task.id.clone()), Some(&task.id), tint);
        if urgency != "overdue" {
            x -= BTN + 4.0;
            self.icon_button(kit, frame, x, y, Icon::Alarm, Action::ToggleSnooze(task.id.clone()), Some(&task.id), tint);
        }
        if let Some(url) = &task.url {
            x -= BTN + 4.0;
            self.icon_button(kit, frame, x, y, Icon::ArrowUpRight, Action::OpenUrl(url.clone()), Some(&task.id), tint);
        }
    }

    fn is_snoozed(env: &Env, model: &Model, task: &Task) -> bool {
        is_snoozed(&model.snoozed, &task.id, env.wall_ms as f64)
    }

    /// One task as a rounded row tinted by how close it is.
    #[allow(clippy::too_many_arguments)]
    fn task_row(&mut self, kit: &mut Kit, env: &Env, frame: &PanelFrame, model: &Model, task: &Task, kind: Kind, x: f32, y: f32, w: f32, list_y: f32, index: usize) {
        let theme = frame.theme;
        let urgency = if kind == Kind::Done { "later" } else { urgency_of(task.due_ms, env.wall_ms) };
        let color = if kind == Kind::Done { theme.dim } else { tone(frame, urgency) };
        let selected = self.state.keyboard && self.state.selected.as_deref() == Some(task.id.as_str());
        let hovered = self.state.hover_row.as_deref() == Some(task.id.as_str());
        let snoozing = self.state.snooze_for.as_deref() == Some(task.id.as_str());
        let h = ROW_H;
        let scale = kit.canvas.scale;

        self.row_rects.insert(task.id.clone(), Rect { x, y: index as f32 * (ROW_H + ROW_GAP), w, h });
        self.hits.push(Hit { rect: Rect { x, y, w, h }, action: task.url.clone().map(Action::OpenUrl), row: Some(task.id.clone()) });

        let tint = if kind == Kind::Done { 0.06 } else if hovered || selected { 0.26 } else { 0.17 };
        if selected {
            kit.canvas.rrect(x - 2.0, y - 2.0, w + 4.0, h + 4.0, 15.0, theme.fg, frame.alpha * 0.9);
            kit.canvas.rrect(x, y, w, h, 13.0, theme.bg, frame.alpha);
        }
        // A fine border in the colour, then the tinted body inside it.
        kit.canvas.rrect(x, y, w, h, 13.0, color, frame.alpha * if kind == Kind::Done { 0.12 } else { 0.30 });
        kit.canvas.rrect(x + 1.0, y + 1.0, w - 2.0, h - 2.0, 12.0, mix(theme.bg, color, tint * 0.9), frame.alpha);

        if snoozing {
            let mut cx = x + 14.0;
            let cy = y + (h - 22.0) / 2.0;
            for kind in SNOOZE_KINDS {
                let text = env.t.t(&format!("snooze.{kind}"), &[]);
                cx += self.chip(kit, frame, cx, cy, &text, Action::Snooze(task.id.clone(), kind), &task.id) + 6.0;
            }
            let text = env.t.t("snooze.cancel", &[]);
            self.chip(kit, frame, cx, cy, &text, Action::ToggleSnooze(task.id.clone()), &task.id);
            return;
        }

        // The course chip.
        let label = if task.source.as_deref() == Some("local") { env.t.t("row.local", &[]) } else { pill_label(Some(task), 12) };
        let label = kit.fonts.ellipsize(Face::SansBold, &label, 10.0 * scale, 0.0, 56.0 * scale);
        let chip_w = kit.canvas.measure(kit.fonts, Face::SansBold, 10.0, &label, 0.0) + 14.0;
        kit.canvas.rrect(x + 12.0, y + (h - 18.0) / 2.0, chip_w, 18.0, 6.0, color, frame.alpha * 0.2);
        let base = y + (h - 18.0) / 2.0 + Self::base(kit, Face::SansBold, 10.0, 18.0);
        kit.canvas.text(kit.fonts, Face::SansBold, 10.0, x + 19.0, base, &label, color, frame.alpha, 0.0);

        // The right end: the buttons while pointed at, otherwise the countdown.
        let right = x + w - 10.0;
        let show_actions = selected || hovered;
        let text_right;
        if show_actions && kind != Kind::Done {
            let count = 1 + usize::from(task.url.is_some()) + usize::from(urgency != "overdue");
            self.row_actions(kit, frame, task, right, y + (h - BTN) / 2.0, urgency, theme.fg);
            text_right = right - count as f32 * (BTN + 4.0);
        } else if kind == Kind::Done {
            self.icon_button(kit, frame, right - BTN, y + (h - BTN) / 2.0, Icon::Undo, Action::Restore(task.id.clone()), Some(&task.id), theme.fg);
            text_right = right - BTN - 4.0;
        } else {
            let count = compact_countdown(task.due_ms, env.wall_ms, &env.t, false);
            let cw = kit.canvas.measure(kit.fonts, Face::Mono, 12.0, &count, 0.0);
            let base = y + Self::base(kit, Face::Mono, 12.0, h);
            kit.canvas.text(kit.fonts, Face::Mono, 12.0, right - cw, base, &count, color, frame.alpha, 0.0);
            if Self::is_snoozed(env, model, task) {
                kit.canvas.icon(Icon::Alarm, right - cw - 10.0, y + h / 2.0, 10.0, theme.dim, frame.alpha);
                text_right = right - cw - 22.0;
            } else {
                text_right = right - cw - 10.0;
            }
        }

        // The title in what is left.
        let tx = x + 12.0 + chip_w + 9.0;
        let title = kit.fonts.ellipsize(Face::Sans, &task.title, 13.0 * scale, 0.0, (text_right - tx).max(20.0) * scale);
        let base = y + Self::base(kit, Face::Sans, 13.0, h);
        let adv = kit.canvas.text(kit.fonts, Face::Sans, 13.0, tx, base, &title, if kind == Kind::Done { theme.dim } else { theme.fg }, frame.alpha, 0.0);
        if kind == Kind::Done {
            kit.canvas.rect(tx, base - 4.5, adv, 1.0, theme.dim, frame.alpha);
        }
        let _ = list_y;
    }

    /// The left column: the next deadline in big light numbers, a line about it, the sync state and the buttons.
    fn left_column(&mut self, kit: &mut Kit, env: &Env, frame: &PanelFrame, model: &Model) {
        let theme = frame.theme;
        let x = frame.content_x();
        let y0 = frame.origin_y + 2.0;
        let scale = kit.canvas.scale;
        let lang = env.t.lang();

        match &model.view.hero {
            Some(hero) => {
                let urgency = urgency_of(hero.due_ms, env.wall_ms);
                let color = tone(frame, urgency);
                let caption = access::word(lang, "next");
                let base = y0 + Self::base(kit, Face::Sans, 11.0, 14.0);
                kit.canvas.text(kit.fonts, Face::Sans, 11.0, x, base, caption, theme.dim, frame.alpha, 0.0);
                let count = compact_countdown(hero.due_ms, env.wall_ms, &env.t, true);
                let mut size = 34.0;
                while size > 20.0 && kit.canvas.measure(kit.fonts, Face::SansLight, size, &count, 0.0) > LEFT_W {
                    size -= 2.0;
                }
                let base = y0 + 14.0 + Self::base(kit, Face::SansLight, size, 42.0);
                kit.canvas.text(kit.fonts, Face::SansLight, size, x - 1.0, base, &count, color, frame.alpha, 0.0);
                if !frame.keyboard {
                    let line = hero.course.clone().filter(|c| !c.is_empty()).map_or_else(|| hero.title.clone(), |c| format!("{c} \u{00B7} {}", hero.title));
                    let line = kit.fonts.ellipsize(Face::Sans, &line, 12.0 * scale, 0.0, LEFT_W * scale);
                    let base = y0 + 58.0 + Self::base(kit, Face::Sans, 12.0, 16.0);
                    kit.canvas.text(kit.fonts, Face::Sans, 12.0, x, base, &line, theme.fg, frame.alpha, 0.0);
                }
            }
            None => {
                let msg = if model.has_feeds { env.t.t("rest.none", &[]) } else { env.t.t("rest.noFeeds", &[]) };
                let base = y0 + 14.0 + Self::base(kit, Face::SansLight, 26.0, 42.0);
                kit.canvas.text(kit.fonts, Face::SansLight, 26.0, x - 1.0, base, &msg, theme.fg, frame.alpha, 0.0);
            }
        }

        // The sync state, or the keys while the keyboard has the panel.
        let (text, color) = if frame.keyboard {
            let key = if self.state.snooze_for.is_some() { "kbd.snooze" } else { "kbd.hint" };
            (env.t.t(key, &[]), theme.dim)
        } else {
            let s = sync_summary(&model.sync, &env.t, env.tz);
            (s.text, if s.kind == "error" { theme.late } else { theme.dim })
        };
        let lines = kit.fonts.wrap(Face::Sans, &text, 10.0 * scale, LEFT_W * scale, if frame.keyboard { 3 } else { 1 });
        let top = if frame.keyboard { y0 + 58.0 } else { y0 + 78.0 };
        for (i, l) in lines.iter().enumerate() {
            let base = top + i as f32 * 12.5 + Self::base(kit, Face::Sans, 10.0, 12.5);
            kit.canvas.text(kit.fonts, Face::Sans, 10.0, x, base, l, color, frame.alpha, 0.0);
        }

        // The buttons.
        let by = frame.origin_y + CONTENT_H - BTN - 2.0;
        let mut bx = x;
        if model.has_feeds {
            self.icon_button(kit, frame, bx, by, Icon::Refresh, Action::Refresh, None, theme.fg);
            bx += BTN + 6.0;
        }
        self.icon_button(kit, frame, bx, by, Icon::Gear, Action::OpenSettings, None, theme.fg);
        bx += BTN + 6.0;
        if !model.view.done_list.is_empty() {
            let n = model.view.done_list.len().to_string();
            let text = env.t.t("section.done", &[("n", &n)]);
            let tw = kit.canvas.measure(kit.fonts, Face::Sans, 11.5, &text, 0.0);
            let w = (tw + 24.0).min(x + LEFT_W - bx);
            let rect = Rect { x: bx, y: by, w, h: BTN };
            let over = self.state.pointer.is_some_and(|(px, py)| rect.contains(px, py));
            if self.state.done_open {
                kit.canvas.rrect(bx, by, w, BTN, BTN / 2.0, theme.fg, frame.alpha * 0.95);
            } else {
                kit.canvas.rrect(bx, by, w, BTN, BTN / 2.0, theme.fg, frame.alpha * if over { 0.22 } else { 0.1 });
            }
            let text = kit.fonts.ellipsize(Face::Sans, &text, 11.5 * scale, 0.0, (w - 20.0) * scale);
            let base = by + Self::base(kit, Face::Sans, 11.5, BTN);
            kit.canvas.text(kit.fonts, Face::Sans, 11.5, bx + 12.0, base, &text, if self.state.done_open { theme.bg } else { theme.fg }, frame.alpha, 0.0);
            self.hits.push(Hit { rect, action: Some(Action::ToggleDone), row: None });
        }
    }

    /// The right column: the list, scrolled inside its box.
    fn list(&mut self, kit: &mut Kit, env: &Env, frame: &PanelFrame, model: &Model) {
        let theme = frame.theme;
        let x = frame.content_x() + LEFT_W + LEFT_GAP;
        let w = frame.content_w() - LEFT_W - LEFT_GAP;
        let top = frame.origin_y + 2.0;
        let entries = self.entries(model);
        if entries.is_empty() {
            let msg = if model.has_feeds { env.t.t("list.empty", &[]) } else { env.t.t("list.noFeeds", &[]) };
            let lines = kit.fonts.wrap(Face::Sans, &msg, 13.0 * kit.canvas.scale, (w - 20.0) * kit.canvas.scale, 2);
            let block = lines.len() as f32 * 18.0 + if model.has_feeds { 0.0 } else { 40.0 };
            let mut ty = top + (CONTENT_H - 4.0 - block) / 2.0;
            for l in &lines {
                let lw = kit.canvas.measure(kit.fonts, Face::Sans, 13.0, l, 0.0);
                let base = ty + Self::base(kit, Face::Sans, 13.0, 18.0);
                kit.canvas.text(kit.fonts, Face::Sans, 13.0, x + (w - lw) / 2.0, base, l, theme.dim, frame.alpha, 0.0);
                ty += 18.0;
            }
            if !model.has_feeds {
                let label = env.t.t("list.addFeed", &[]);
                let bw = kit.canvas.measure(kit.fonts, Face::SansSemibold, 12.5, &label, 0.0) + 32.0;
                let (bx, by) = (x + (w - bw) / 2.0, ty + 10.0);
                let rect = Rect { x: bx, y: by, w: bw, h: 30.0 };
                let over = self.state.pointer.is_some_and(|(px, py)| rect.contains(px, py));
                kit.canvas.rrect(bx, by, bw, 30.0, 15.0, AMBER, frame.alpha * if over { 1.0 } else { 0.92 });
                let base = by + Self::base(kit, Face::SansSemibold, 12.5, 30.0);
                kit.canvas.text(kit.fonts, Face::SansSemibold, 12.5, bx + 16.0, base, &label, Rgb(0x18, 0x10, 0x04), frame.alpha, 0.0);
                self.hits.push(Hit { rect, action: Some(Action::OpenSettings), row: None });
            }
            return;
        }
        kit.canvas.push_clip(x - 4.0, top - 2.0, w + 8.0, CONTENT_H);
        for (i, (task, kind)) in entries.iter().enumerate() {
            let y = top + i as f32 * (ROW_H + ROW_GAP) - self.state.scroll;
            if y + ROW_H < top - 2.0 || y > top + CONTENT_H {
                // Still remember where it is, so the keyboard can bring it into view.
                self.row_rects.insert(task.id.clone(), Rect { x, y: i as f32 * (ROW_H + ROW_GAP), w, h: ROW_H });
                continue;
            }
            self.task_row(kit, env, frame, model, task, *kind, x, y, w, top, i);
        }
        kit.canvas.pop_clip();
        // Rows partly out of the box cannot be clicked where they are cut off.
        let (lo, hi) = (top, top + CONTENT_H - 4.0);
        for h in &mut self.hits {
            if h.rect.x >= x - 1.0 && h.rect.w > 0.0 && h.row.is_some() {
                let (y0, y1) = (h.rect.y.max(lo), (h.rect.y + h.rect.h).min(hi));
                h.rect.y = y0;
                h.rect.h = (y1 - y0).max(0.0);
            }
        }
        self.hits.retain(|h| h.rect.h > 0.0);
    }

    fn ensure_visible(&mut self, id: &str) {
        let Some(r) = self.row_rects.get(id).copied() else { return };
        let view_h = CONTENT_H - 4.0;
        if r.y < self.state.scroll {
            self.state.scroll = r.y;
        } else if r.y + r.h > self.state.scroll + view_h {
            self.state.scroll = r.y + r.h - view_h;
        }
        self.clamp_scroll();
    }

    fn clamp_scroll(&mut self) {
        self.state.scroll = self.state.scroll.clamp(0.0, (self.content_h - (CONTENT_H - 4.0)).max(0.0));
    }

    fn act(&mut self, action: Action, env: &Env) -> Vec<Effect> {
        match action {
            Action::OpenUrl(url) => vec![Effect::OpenUrl(url)],
            Action::MarkDone(id) => {
                let said = self.said(env, "marked_done", &id);
                self.source.mark_done(&id, env.wall_ms);
                if self.state.selected.as_deref() == Some(id.as_str()) {
                    self.state.selected = None;
                }
                vec![Effect::Redraw, Effect::Announce(said)]
            }
            Action::Restore(id) => {
                let said = self.said(env, "restored", &id);
                self.source.restore(&id);
                vec![Effect::Redraw, Effect::Announce(said)]
            }
            Action::ToggleSnooze(id) => {
                self.state.snooze_for = if self.state.snooze_for.as_deref() == Some(id.as_str()) { None } else { Some(id) };
                vec![Effect::Redraw]
            }
            Action::Snooze(id, kind) => {
                let said = self.said(env, "snoozed", &id);
                self.source.snooze(&id, kind, env.wall_ms, env.tz);
                self.state.snooze_for = None;
                vec![Effect::Redraw, Effect::Announce(said)]
            }
            Action::ToggleDone => {
                self.state.done_open = !self.state.done_open;
                self.state.scroll = 0.0;
                vec![Effect::Redraw]
            }
            Action::Refresh => {
                self.source.refresh();
                vec![Effect::RefreshFeeds]
            }
            Action::OpenSettings => vec![Effect::OpenSettings],
            Action::Control(_) | Action::Music(_) | Action::Project(_) | Action::Agent(_) | Action::Shelf(_) => Vec::new(),
        }
    }

    /// "Marked done: Problem Set 4": what a screen reader says after an action.
    fn said(&self, env: &Env, word_key: &str, id: &str) -> String {
        let view = self.model(env).view;
        let title = view.upcoming.iter().chain(&view.overdue).chain(&view.done_list).find(|t| t.id == id).map(|t| t.title.clone()).unwrap_or_default();
        format!("{}: {}", access::word(env.t.lang(), word_key), title)
    }

    /// One screen-reader sentence for a deadline: title, course, when, how long.
    fn row_name(task: &Task, env: &Env) -> String {
        let mut parts = vec![task.title.clone()];
        if let Some(course) = task.course.as_deref().filter(|c| !c.is_empty()) {
            parts.push(course.to_string());
        }
        parts.push(due_label(task.due_ms, env.wall_ms, &env.t, env.tz));
        parts.push(compact_countdown(task.due_ms, env.wall_ms, &env.t, false));
        parts.join(". ")
    }

    /// Window rectangle (logical px) of a row as drawn last frame; None when scrolled out of view.
    fn drawn_row(&self, id: &str) -> Option<(f32, f32, f32, f32)> {
        self.hits.iter().find(|h| h.row.as_deref() == Some(id) && h.rect.w > BTN * 2.0).map(|h| (h.rect.x, h.rect.y, h.rect.w, h.rect.h))
    }

    /// Milliseconds until the countdown text for `due` changes.
    fn change_in(due_ms: i64, now_ms: i64, seconds: bool) -> u64 {
        let delta = due_ms - now_ms;
        let ms = if seconds && delta > 0 && delta < 3_600_000 {
            delta % 1000 + 1
        } else if delta > 0 {
            delta % MINUTE_MS + 1
        } else {
            MINUTE_MS - delta.abs() % MINUTE_MS
        };
        ms.max(1) as u64
    }
}

impl Module for DeadlinesModule {
    fn id(&self) -> &'static str {
        "deadlines"
    }

    fn as_any_mut(&mut self) -> &mut dyn std::any::Any {
        self
    }

    fn title(&self, lang: blink_core::i18n::Lang) -> String {
        if lang == blink_core::i18n::Lang::Id { "Tenggat" } else { "Deadlines" }.to_string()
    }

    fn accent(&self) -> Rgb {
        AMBER
    }

    fn due_dates(&mut self, env: &Env) -> Vec<(i64, &'static str)> {
        let model = self.model(env);
        model.view.upcoming.iter().chain(model.view.overdue.iter()).map(|t| (t.due_ms, urgency_of(t.due_ms, env.wall_ms))).collect()
    }

    fn title_parts(&self, lang: blink_core::i18n::Lang) -> (String, String) {
        let (a, b) = if lang == blink_core::i18n::Lang::Id { ("Teng", "gat") } else { ("Dead", "lines") };
        (a.to_string(), b.to_string())
    }

    fn set_options(&mut self, options: &serde_json::Value) {
        self.urgent_hours = options.get("urgentHours").and_then(serde_json::Value::as_f64).filter(|h| (0.1..=720.0).contains(h)).unwrap_or(DEFAULT_URGENT_HOURS);
    }

    /// The clock rests on the pill until a deadline is close (inside `urgentHours`, or already missed):
    /// then the deadline takes it.
    fn rest_priority(&mut self, env: &Env, _settings: &Settings) -> u8 {
        let model = self.model(env);
        let Some(hero) = &model.view.hero else { return 0 };
        if hero.due_ms <= env.wall_ms + (self.urgent_hours * 3_600_000.0) as i64 { 3 } else { 0 }
    }

    fn rest_style(&mut self, _env: &Env, resting_style: &str, _due_horizon_hours: f64) -> crate::shell::RestStyle {
        use crate::shell::RestStyle;
        // Only "hover" hides the pill (an invisible strip along the top that still opens on the mouse).
        if resting_style == "hover" { RestStyle::Hot } else { RestStyle::Full }
    }

    fn debug_now(&mut self, env: &Env) -> serde_json::Value {
        let model = self.model(env);
        let titles = |tasks: &[Task]| tasks.iter().map(|t| t.title.clone()).collect::<Vec<_>>();
        let sync = sync_summary(&model.sync, &env.t, env.tz);
        let mut v = self.debug();
        if let Some(o) = v.as_object_mut() {
            o.insert("upcoming".into(), serde_json::json!(titles(&model.view.upcoming)));
            o.insert("overdue".into(), serde_json::json!(titles(&model.view.overdue)));
            o.insert("doneList".into(), serde_json::json!(titles(&model.view.done_list)));
            o.insert("upcomingIds".into(), serde_json::json!(model.view.upcoming.iter().map(|t| t.id.clone()).collect::<Vec<_>>()));
            o.insert("sync".into(), serde_json::json!({ "kind": sync.kind, "text": sync.text }));
            o.insert("hasFeeds".into(), serde_json::json!(model.has_feeds));
        }
        v
    }

    fn debug(&self) -> serde_json::Value {
        let action = |a: &Option<Action>| match a {
            Some(Action::OpenUrl(u)) => format!("OpenUrl:{u}"),
            Some(Action::MarkDone(id)) => format!("MarkDone:{id}"),
            Some(Action::Restore(id)) => format!("Restore:{id}"),
            Some(Action::ToggleSnooze(id)) => format!("ToggleSnooze:{id}"),
            Some(Action::Snooze(id, k)) => format!("Snooze:{id}:{k}"),
            Some(Action::ToggleDone) => "ToggleDone".to_string(),
            Some(Action::Refresh) => "Refresh".to_string(),
            Some(Action::OpenSettings) => "OpenSettings".to_string(),
            Some(Action::Control(_)) => "Control".to_string(),
            Some(Action::Music(_)) => "Music".to_string(),
            Some(Action::Project(_)) => "Project".to_string(),
            Some(Action::Agent(_)) => "Agent".to_string(),
            Some(Action::Shelf(_)) => "Shelf".to_string(),
            None => String::new(),
        };
        serde_json::json!({
            "selected": self.state.selected,
            "scroll": self.state.scroll,
            "snoozeFor": self.state.snooze_for,
            "doneOpen": self.state.done_open,
            "contentHeight": self.content_h,
            "hits": self.hits.iter().map(|h| serde_json::json!({
                "x": h.rect.x, "y": h.rect.y, "w": h.rect.w, "h": h.rect.h, "action": action(&h.action), "row": h.row
            })).collect::<Vec<_>>(),
        })
    }

    fn rest(&mut self, env: &Env) -> RestView {
        let model = self.model(env);
        match &model.view.hero {
            None => RestView {
                label: String::new(),
                count: if model.has_feeds { env.t.t("rest.none", &[]) } else { env.t.t("rest.noFeeds", &[]) },
                urgency: "later",
            },
            Some(h) => RestView {
                label: pill_label(Some(h), 12),
                count: compact_countdown(h.due_ms, env.wall_ms, &env.t, true),
                urgency: urgency_of(h.due_ms, env.wall_ms),
            },
        }
    }

    fn peek(&mut self, env: &Env) -> Option<PeekView> {
        let id = self.peek_task.take()?;
        let model = self.model(env);
        let task = model.view.upcoming.iter().chain(model.view.overdue.iter()).find(|t| t.id == id)?;
        Some(PeekView {
            code: task.course.clone().unwrap_or_else(|| pill_label(Some(task), 12)),
            title: task.title.clone(),
            count: compact_countdown(task.due_ms, env.wall_ms, &env.t, true),
            urgency: urgency_of(task.due_ms, env.wall_ms),
            // A deadline alert is something to look at now.
            priority: 3,
            ttl_ms: None,
            key: Some(format!("deadline:{}", task.id)),
            progress: None,
            icon: Some(Icon::Alarm),
            accent: None,
            cover: None,
        })
    }

    fn panel_height(&mut self, env: &Env, _kit: &mut Kit) -> f32 {
        let model = self.model(env);
        self.content_h = Self::list_height(self.entries(&model).len());
        CONTENT_H
    }

    fn draw_panel(&mut self, env: &Env, kit: &mut Kit, frame: &PanelFrame) {
        let mut model = self.model(env);
        // With nothing done there is no list of done ones to show (and no button to leave it): back to the deadlines.
        if self.state.done_open && model.view.done_list.is_empty() {
            self.state.done_open = false;
            self.state.scroll = 0.0;
            model = self.model(env);
        }
        self.content_h = Self::list_height(self.entries(&model).len());
        self.clamp_scroll();
        self.hits.clear();
        self.row_rects.clear();
        self.left_column(kit, env, frame, &model);
        self.list(kit, env, frame, &model);
    }

    fn pointer(&mut self, kind: PointerKind, x: f32, y: f32, env: &Env) -> Vec<Effect> {
        match kind {
            PointerKind::Leave => {
                self.state.pointer = None;
                self.state.hover_row = None;
                vec![Effect::Redraw]
            }
            PointerKind::Move => {
                self.state.pointer = Some((x, y));
                self.state.hover_row = self.hits.iter().rev().find(|h| h.rect.contains(x, y) && h.row.is_some()).and_then(|h| h.row.clone());
                vec![Effect::Redraw]
            }
            PointerKind::Down => {
                let hit = self.hits.iter().rev().find(|h| h.rect.contains(x, y) && h.action.is_some()).and_then(|h| h.action.clone());
                hit.map_or_else(Vec::new, |a| self.act(a, env))
            }
            PointerKind::Up => Vec::new(),
            PointerKind::Wheel(delta) => {
                self.state.scroll -= delta;
                self.clamp_scroll();
                vec![Effect::Redraw]
            }
        }
    }

    fn key(&mut self, key: Key, env: &Env) -> KeyResult {
        let model = self.model(env);
        let ids = Self::selectable(&model);
        let current = self.state.selected.clone().filter(|s| ids.contains(s));
        let handled = |effects: Vec<Effect>| KeyResult { handled: true, close: false, effects };

        if key == Key::Escape {
            if self.state.snooze_for.is_some() {
                self.state.snooze_for = None;
                return handled(vec![Effect::Redraw]);
            }
            return KeyResult { handled: true, close: true, effects: Vec::new() };
        }
        if let Some(id) = self.state.snooze_for.clone() {
            let kind = match key {
                Key::Char('1') => Some("30m"),
                Key::Char('2') => Some("2h"),
                Key::Char('3') => Some("tomorrow"),
                _ => None,
            };
            return match kind {
                Some(k) => handled(self.act(Action::Snooze(id, k), env)),
                None => KeyResult::default(),
            };
        }

        let at = current.as_ref().and_then(|c| ids.iter().position(|i| i == c));
        let last = ids.len().saturating_sub(1);
        let select = |this: &mut Self, idx: usize| -> KeyResult {
            if let Some(id) = ids.get(idx).cloned() {
                this.state.selected = Some(id.clone());
                this.ensure_visible(&id);
            }
            handled(vec![Effect::Redraw])
        };
        match key {
            Key::Down => select(self, at.map_or(0, |i| (i + 1).min(last))),
            Key::Up => select(self, at.map_or(last, |i| i.saturating_sub(1))),
            Key::Home => select(self, 0),
            Key::End => select(self, last),
            Key::Enter => {
                let task = current.as_ref().and_then(|id| model.view.upcoming.iter().chain(model.view.overdue.iter()).find(|t| &t.id == id));
                handled(task.and_then(|t| t.url.clone()).map(Effect::OpenUrl).into_iter().collect())
            }
            Key::Space | Key::Char('d') | Key::Char('D') => {
                let Some(id) = current else { return handled(Vec::new()) };
                // Move on before the row disappears.
                self.state.selected = at.and_then(|i| ids.get(i + 1).or_else(|| i.checked_sub(1).and_then(|p| ids.get(p)))).cloned();
                handled(self.act(Action::MarkDone(id), env))
            }
            Key::Char('s') | Key::Char('S') => {
                let Some(id) = current else { return handled(Vec::new()) };
                if model.view.overdue.iter().any(|t| t.id == id) {
                    return handled(Vec::new());
                }
                self.state.snooze_for = Some(id);
                handled(vec![Effect::Redraw])
            }
            _ => KeyResult::default(),
        }
    }

    fn keyboard_mode(&mut self, on: bool, env: &Env) {
        self.state.keyboard = on;
        self.state.snooze_for = None;
        self.state.selected = if on { Self::selectable(&self.model(env)).first().cloned() } else { None };
    }

    fn configure(&mut self, settings: &Settings) {
        self.source.configure(settings);
    }

    fn perform(&mut self, action: Action, env: &Env) -> Vec<Effect> {
        self.act(action, env)
    }

    fn accessibility(&mut self, env: &Env, open: bool) -> Vec<Node> {
        let lang = env.t.lang();
        let w = |key: &str| access::word(lang, key);
        let model = self.model(env);
        let mut out = Vec::new();

        if !open {
            let rv = self.rest(env);
            let text = if model.view.hero.is_some() { format!("{}: {} {}", w("next"), rv.label, rv.count) } else { rv.count };
            let mut pill = Node::new(1, ROOT, Role::Button, text);
            pill.action = Some(NodeAction::OpenPanel);
            out.push(pill);
            return out;
        }

        let push_rows = |out: &mut Vec<Node>, list_key: i32, list_name: &str, tasks: Vec<&Task>, done: bool, this: &Self| {
            if tasks.is_empty() {
                return;
            }
            out.push(Node::new(list_key, ROOT, Role::List, list_name));
            for task in tasks {
                let rect = this.drawn_row(&task.id);
                let row_key = key_for(&task.id, 0);
                let mut row = Node::new(row_key, list_key, Role::ListItem, Self::row_name(task, env));
                row.help = w("row_help").to_string();
                row.rect = rect.unwrap_or_default();
                row.offscreen = rect.is_none();
                row.focused = this.state.keyboard && this.state.selected.as_deref() == Some(task.id.as_str());
                row.action = task.url.clone().map(|u| NodeAction::Module(Action::OpenUrl(u)));
                out.push(row.clone());
                let button = |sub: i32, name: &str, action: Action| {
                    let mut b = Node::new(key_for(&task.id, sub), row_key, Role::Button, format!("{name}: {}", task.title));
                    b.rect = row.rect;
                    b.offscreen = row.offscreen;
                    b.action = Some(NodeAction::Module(action));
                    b
                };
                if done {
                    out.push(button(5, w("restore"), Action::Restore(task.id.clone())));
                } else {
                    out.push(button(1, w("mark_done"), Action::MarkDone(task.id.clone())));
                    out.push(button(2, w("snooze_30m"), Action::Snooze(task.id.clone(), "30m")));
                    out.push(button(3, w("snooze_2h"), Action::Snooze(task.id.clone(), "2h")));
                    out.push(button(4, w("snooze_tomorrow"), Action::Snooze(task.id.clone(), "tomorrow")));
                }
            }
        };

        let upcoming: Vec<&Task> = model.view.upcoming.iter().take(MAX_UPCOMING_ROWS).collect();
        push_rows(&mut out, 2, w("upcoming"), upcoming, false, self);
        let overdue: Vec<&Task> = model.view.overdue.iter().take(MAX_OVERDUE_ROWS).collect();
        push_rows(&mut out, 3, w("overdue"), overdue, false, self);
        if !model.view.done_list.is_empty() {
            let mut toggle = Node::new(5, ROOT, Role::Button, format!("{} ({})", w("show_done"), model.view.done_list.len()));
            toggle.action = Some(NodeAction::Module(Action::ToggleDone));
            out.push(toggle);
            if self.state.done_open {
                push_rows(&mut out, 4, w("done"), model.view.done_list.iter().collect(), true, self);
            }
        }
        if model.has_feeds {
            let mut refresh = Node::new(6, ROOT, Role::Button, w("refresh"));
            refresh.action = Some(NodeAction::Module(Action::Refresh));
            out.push(refresh);
        }
        let mut settings = Node::new(7, ROOT, Role::Button, w("settings"));
        settings.action = Some(NodeAction::Module(Action::OpenSettings));
        out.push(settings);
        out
    }

    fn refresh(&mut self) {
        self.source.refresh();
    }

    fn reload_local(&mut self) {
        self.source.reload_local();
    }

    fn tick(&mut self, env: &Env) -> Vec<Effect> {
        self.source.poll(env.wall_ms);
        let mut effects = Vec::new();
        for item in self.source.announce(env.wall_ms) {
            match item {
                Announcement::Task(task) => {
                    effects.push(Effect::Toast {
                        title: env.t.t("toast.title", &[("title", task.title.as_str())]),
                        body: env.t.t(
                            "toast.body",
                            &[("course", task.course.as_deref().unwrap_or("")), ("left", compact_countdown(task.due_ms, env.wall_ms, &env.t, false).as_str())],
                        ),
                        url: task.url.clone(),
                    });
                    // The tightest one (the planner sorts by time left) also peeks from the notch.
                    if self.peek_task.is_none() {
                        self.peek_task = Some(task.id.clone());
                        effects.push(Effect::Peek);
                    }
                }
                Announcement::Summary(n) => effects.push(Effect::Toast {
                    title: env.t.t("toast.summary.title", &[("n", n.to_string().as_str())]),
                    body: env.t.t("toast.summary.body", &[]),
                    url: None,
                }),
            }
        }
        effects
    }

    fn next_wake(&mut self, env: &Env, panel_open: bool) -> Option<u64> {
        let source_wake = self.source.next_wake_wall(env.wall_ms).map(|at| env.now_ms + (at - env.wall_ms).max(1) as u64);
        let own = self.display_wake(env, panel_open);
        [own, source_wake].into_iter().flatten().min()
    }
}

impl DeadlinesModule {
    /// When the countdown text on screen next changes.
    fn display_wake(&mut self, env: &Env, panel_open: bool) -> Option<u64> {
        let model = self.model(env);
        let hero_id = model.view.hero.as_ref().map(|h| h.id.clone());
        let mut soonest: Option<u64> = None;
        let mut consider = |due: i64, seconds: bool| {
            let ms = Self::change_in(due, env.wall_ms, seconds);
            soonest = Some(soonest.map_or(ms, |s| s.min(ms)));
        };
        if let Some(h) = &model.view.hero {
            consider(h.due_ms, true); // the pill shows the hero
        }
        if panel_open {
            for t in model.view.upcoming.iter().take(MAX_UPCOMING_ROWS).chain(model.view.overdue.iter().take(MAX_OVERDUE_ROWS)) {
                consider(t.due_ms, hero_id.as_deref() == Some(t.id.as_str()));
            }
        }
        soonest.map(|ms| env.now_ms + ms)
    }
}
