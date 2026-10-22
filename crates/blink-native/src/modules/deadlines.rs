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
use blink_core::view::{build_view, View};

use super::{Action, Effect, Env, Key, KeyResult, Kit, Module, PanelFrame, PeekView, PointerKind, RestView};
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
const MAX_OVERDUE_ROWS: usize = 4;
const PANEL_W: f32 = 420.0;
const PAD_X: f32 = 12.0;
const PAD_TOP: f32 = 16.0;
const CONTENT_W: f32 = PANEL_W - 2.0 * PAD_X;
const ROW_GAP: f32 = 2.0;
const HERO_GAP: f32 = 6.0;
const FOOTER_H: f32 = 39.0;
const BTN: f32 = 24.0;

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
    /// Row rectangles in content space (scroll removed), to keep the selection in view.
    row_rects: HashMap<String, Rect>,
    /// Height of everything above the footer, from the last layout.
    content_h: f32,
    /// The viewport height the last frame used.
    viewport_h: f32,
    peek_task: Option<String>,
}

struct Model {
    view: View,
    sync: Vec<FeedSync>,
    has_feeds: bool,
    snoozed: HashMap<String, f64>,
}

fn line_alpha(frame: &PanelFrame) -> f32 {
    f32::from(frame.theme.line.1) / 255.0 * frame.alpha
}

/// A 1px rounded outline made from four thin bars (the kit has no stroked rounded rect).
fn outline(kit: &mut Kit, x: f32, y: f32, w: f32, h: f32, color: Rgb, alpha: f32) {
    kit.canvas.rrect(x, y, w, 1.0, 0.5, color, alpha);
    kit.canvas.rrect(x, y + h - 1.0, w, 1.0, 0.5, color, alpha);
    kit.canvas.rrect(x, y, 1.0, h, 0.5, color, alpha);
    kit.canvas.rrect(x + w - 1.0, y, 1.0, h, 0.5, color, alpha);
}

impl DeadlinesModule {
    pub fn new(source: Box<dyn DeadlineSource>) -> Self {
        DeadlinesModule {
            source,
            state: PanelState::default(),
            hits: Vec::new(),
            row_rects: HashMap::new(),
            content_h: 0.0,
            viewport_h: 0.0,
            peek_task: None,
        }
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

    fn base(kit: &Kit, face: Face, size: f32, line_h: f32) -> f32 {
        kit.fonts.baseline(face, size, line_h)
    }

    // ------------------------------------------------------------ pieces

    /// An icon button with a hover background and a hit region.
    #[allow(clippy::too_many_arguments)]
    fn icon_button(&mut self, kit: &mut Kit, frame: &PanelFrame, x: f32, y: f32, icon: Icon, action: Action, row: Option<&str>) {
        let rect = Rect { x, y, w: BTN, h: BTN };
        let over = self.state.pointer.is_some_and(|(px, py)| rect.contains(px, py));
        if over {
            kit.canvas.rrect(x, y, BTN, BTN, 6.0, frame.theme.line.0, line_alpha(frame));
        }
        let color = if over { frame.theme.fg } else { frame.theme.dim };
        kit.canvas.icon(icon, x + BTN / 2.0, y + BTN / 2.0, 14.0, color, frame.alpha);
        self.hits.push(Hit { rect, action: Some(action), row: row.map(String::from) });
    }

    /// A small bordered button with text. Returns its width.
    fn chip(&mut self, kit: &mut Kit, frame: &PanelFrame, x: f32, y: f32, label: &str, action: Action, row: &str) -> f32 {
        let size = 11.5;
        let w = kit.canvas.measure(kit.fonts, Face::Sans, size, label, 0.0) + 20.0;
        let h = 22.0;
        let rect = Rect { x, y, w, h };
        if self.state.pointer.is_some_and(|(px, py)| rect.contains(px, py)) {
            kit.canvas.rrect(x, y, w, h, 6.0, frame.theme.line.0, line_alpha(frame));
        }
        outline(kit, x, y, w, h, frame.theme.line.0, line_alpha(frame) * 1.6);
        let base = y + Self::base(kit, Face::Sans, size, h);
        kit.canvas.text(kit.fonts, Face::Sans, size, x + 10.0, base, label, frame.theme.fg, frame.alpha, 0.0);
        self.hits.push(Hit { rect, action: Some(action), row: Some(row.to_string()) });
        w
    }

    /// Open / snooze / done buttons, right-aligned so the last one ends at `right`.
    fn row_actions(&mut self, kit: &mut Kit, frame: &PanelFrame, task: &Task, right: f32, y: f32, urgency: &str) {
        let mut x = right - BTN;
        self.icon_button(kit, frame, x, y, Icon::Check, Action::MarkDone(task.id.clone()), Some(&task.id));
        if urgency != "overdue" {
            x -= BTN;
            self.icon_button(kit, frame, x, y, Icon::Alarm, Action::ToggleSnooze(task.id.clone()), Some(&task.id));
        }
        if let Some(url) = &task.url {
            x -= BTN;
            self.icon_button(kit, frame, x, y, Icon::ArrowUpRight, Action::OpenUrl(url.clone()), Some(&task.id));
        }
    }

    /// The due label, with the snooze time appended while snoozed.
    fn when_text(env: &Env, model: &Model, task: &Task) -> String {
        let base = due_label(task.due_ms, env.wall_ms, &env.t, env.tz);
        if !is_snoozed(&model.snoozed, &task.id, env.wall_ms as f64) {
            return base;
        }
        let until = model.snoozed[&task.id] as i64;
        match jiff::Timestamp::from_millisecond(until).map(|t| t.to_zoned(env.tz.clone())) {
            Ok(z) => format!("{base} \u{00B7} {:02}:{:02}", z.hour(), z.minute()),
            Err(_) => base,
        }
    }

    fn is_snoozed(env: &Env, model: &Model, task: &Task) -> bool {
        is_snoozed(&model.snoozed, &task.id, env.wall_ms as f64)
    }

    /// One task row. Returns its height; with `draw == false` it only measures.
    #[allow(clippy::too_many_arguments)]
    fn task_row(&mut self, kit: &mut Kit, env: &Env, frame: &PanelFrame, model: &Model, task: &Task, y: f32, hero: bool, draw: bool) -> f32 {
        let theme = frame.theme;
        let scale = kit.canvas.scale;
        let x0 = frame.origin_x + PAD_X;
        let urgency = urgency_of(task.due_ms, env.wall_ms);
        // Soon and overdue are colored; a later deadline is neutral: dim in a row, full text in the hero.
        let accent = match urgency {
            "later" if hero => theme.fg,
            "later" => theme.dim,
            other => theme.urgency(other),
        };
        let selected = self.state.keyboard && self.state.selected.as_deref() == Some(task.id.as_str());
        let hovered = self.state.hover_row.as_deref() == Some(task.id.as_str());
        let snoozing = self.state.snooze_for.as_deref() == Some(task.id.as_str());
        let when = Self::when_text(env, model, task);
        let count = compact_countdown(task.due_ms, env.wall_ms, &env.t, hero);
        let label = if task.source.as_deref() == Some("local") { env.t.t("row.local", &[]) } else { pill_label(Some(task), 12) };

        // ---- measure
        let (h, lines, title_x, title_w);
        if hero {
            title_x = x0 + 3.0 + 12.0;
            title_w = CONTENT_W - 3.0 - 12.0 - 84.0;
            lines = kit.fonts.wrap(Face::SansSemibold, &task.title, 15.0 * scale, title_w * scale, 2);
            h = 10.0 + 15.5 + 2.0 + lines.len().max(1) as f32 * 18.75 + 4.0 + 29.7 + 12.0;
        } else {
            let count_w = kit.canvas.measure(kit.fonts, Face::Mono, 12.0, &count, 0.0);
            let when_w = kit.canvas.measure(kit.fonts, Face::Sans, 10.5, &when, 0.0);
            let time_w = count_w.max(when_w + if Self::is_snoozed(env, model, task) { 13.0 } else { 0.0 });
            title_x = x0 + 3.0 + 10.0 + 64.0 + 8.0;
            title_w = ((x0 + CONTENT_W - 90.0 - time_w - 8.0) - title_x).max(24.0);
            lines = kit.fonts.wrap(Face::Sans, &task.title, 13.0 * scale, title_w * scale, 2);
            h = (lines.len().max(1) as f32 * 17.55).max(16.2 + 14.2).max(BTN) + 12.0;
        }
        let total_h = h + if snoozing { 28.0 } else { 0.0 };
        if !draw {
            return total_h;
        }

        // ---- draw
        self.row_rects.insert(task.id.clone(), Rect { x: x0, y: y + self.state.scroll, w: CONTENT_W, h: total_h });
        self.hits.push(Hit { rect: Rect { x: x0, y, w: CONTENT_W, h: total_h }, action: task.url.clone().map(Action::OpenUrl), row: Some(task.id.clone()) });

        if hovered || selected {
            kit.canvas.rrect(x0, y, CONTENT_W, total_h, 6.0, theme.line.0, line_alpha(frame));
        }
        if selected {
            outline(kit, x0, y, CONTENT_W, total_h, theme.fg, 0.9 * frame.alpha);
            outline(kit, x0 + 1.0, y + 1.0, CONTENT_W - 2.0, total_h - 2.0, theme.fg, 0.9 * frame.alpha);
        }
        if urgency != "later" {
            kit.canvas.rrect(x0, y, 3.0, h, 1.5, accent, frame.alpha);
        }
        let show_actions = selected || hovered;

        if hero {
            let mut ty = y + 10.0;
            let course = task.course.clone().unwrap_or_else(|| label.clone());
            let course = kit.fonts.ellipsize(Face::Mono, &course, 11.5 * scale, 0.0, title_w * scale);
            let base = ty + Self::base(kit, Face::Mono, 11.5, 15.5);
            kit.canvas.text(kit.fonts, Face::Mono, 11.5, title_x, base, &course, theme.dim, frame.alpha, 0.0);
            ty += 15.5 + 2.0;
            for l in &lines {
                let base = ty + Self::base(kit, Face::SansSemibold, 15.0, 18.75);
                kit.canvas.text(kit.fonts, Face::SansSemibold, 15.0, title_x, base, l, theme.fg, frame.alpha, 0.0);
                ty += 18.75;
            }
            ty += 4.0;
            let base = ty + Self::base(kit, Face::Mono, 22.0, 29.7);
            let adv = kit.canvas.text(kit.fonts, Face::Mono, 22.0, title_x, base, &count, accent, frame.alpha, 0.0);
            let mut wx = title_x + adv + 10.0;
            if Self::is_snoozed(env, model, task) {
                kit.canvas.icon(Icon::Alarm, wx + 5.0, base - 4.0, 11.0, theme.dim, frame.alpha);
                wx += 14.0;
            }
            kit.canvas.text(kit.fonts, Face::Sans, 12.0, wx, base, &when, theme.dim, frame.alpha, 0.0);
            if show_actions {
                self.row_actions(kit, frame, task, x0 + CONTENT_W - 6.0, y + 8.0, urgency);
            }
        } else {
            let base = y + 6.0 + Self::base(kit, Face::Mono, 11.5, 17.55);
            let code = kit.fonts.ellipsize(Face::Mono, &label, 11.5 * scale, 0.0, 64.0 * scale);
            kit.canvas.text(kit.fonts, Face::Mono, 11.5, x0 + 13.0, base, &code, theme.dim, frame.alpha, 0.0);
            for (i, l) in lines.iter().enumerate() {
                let base = y + 6.0 + i as f32 * 17.55 + Self::base(kit, Face::Sans, 13.0, 17.55);
                kit.canvas.text(kit.fonts, Face::Sans, 13.0, title_x, base, l, theme.fg, frame.alpha, 0.0);
            }
            let right = x0 + CONTENT_W - 90.0;
            let count_w = kit.canvas.measure(kit.fonts, Face::Mono, 12.0, &count, 0.0);
            let when_w = kit.canvas.measure(kit.fonts, Face::Sans, 10.5, &when, 0.0);
            let base = y + 6.0 + Self::base(kit, Face::Mono, 12.0, 16.2);
            kit.canvas.text(kit.fonts, Face::Mono, 12.0, right - count_w, base, &count, accent, frame.alpha, 0.0);
            let base = y + 6.0 + 16.2 + Self::base(kit, Face::Sans, 10.5, 14.2);
            kit.canvas.text(kit.fonts, Face::Sans, 10.5, right - when_w, base, &when, theme.dim, frame.alpha, 0.0);
            if Self::is_snoozed(env, model, task) {
                kit.canvas.icon(Icon::Alarm, right - when_w - 8.0, base - 4.0, 10.0, theme.dim, frame.alpha);
            }
            if show_actions {
                self.row_actions(kit, frame, task, x0 + CONTENT_W - 6.0, y + (h - BTN) / 2.0, urgency);
            }
        }

        if snoozing {
            let cy = y + h - 2.0;
            let mut cx = x0 + 13.0;
            for kind in SNOOZE_KINDS {
                let text = env.t.t(&format!("snooze.{kind}"), &[]);
                cx += self.chip(kit, frame, cx, cy, &text, Action::Snooze(task.id.clone(), kind), &task.id) + 6.0;
            }
            let text = env.t.t("snooze.cancel", &[]);
            self.chip(kit, frame, cx, cy, &text, Action::ToggleSnooze(task.id.clone()), &task.id);
        }
        total_h
    }

    fn section_title(&mut self, kit: &mut Kit, frame: &PanelFrame, y: f32, text: &str, color: Rgb, action: Option<Action>, draw: bool) -> f32 {
        let size = 10.5;
        let h = 10.0 + 14.2 + 4.0;
        if draw {
            let area = Rect { x: frame.origin_x + PAD_X, y: y + 4.0, w: 200.0, h: h - 4.0 };
            let over = action.is_some() && self.state.pointer.is_some_and(|(px, py)| area.contains(px, py));
            let base = y + 10.0 + Self::base(kit, Face::Sans, size, 14.2);
            let c = if over { frame.theme.fg } else { color };
            let w = kit.canvas.text(kit.fonts, Face::Sans, size, frame.origin_x + PAD_X + 10.0, base, &text.to_uppercase(), c, frame.alpha, 0.63);
            if let Some(action) = action {
                self.hits.push(Hit { rect: Rect { w: w + 24.0, ..area }, action: Some(action), row: None });
            }
        }
        h
    }

    fn done_row(&mut self, kit: &mut Kit, env: &Env, frame: &PanelFrame, task: &Task, y: f32, draw: bool) -> f32 {
        let h = 30.4 + 12.0;
        if !draw {
            return h;
        }
        let theme = frame.theme;
        let scale = kit.canvas.scale;
        let x0 = frame.origin_x + PAD_X;
        if self.state.hover_row.as_deref() == Some(task.id.as_str()) {
            kit.canvas.rrect(x0, y, CONTENT_W, h, 6.0, theme.line.0, line_alpha(frame));
        }
        self.hits.push(Hit { rect: Rect { x: x0, y, w: CONTENT_W, h }, action: None, row: Some(task.id.clone()) });
        let label = if task.source.as_deref() == Some("local") { env.t.t("row.local", &[]) } else { pill_label(Some(task), 12) };
        let base = y + 6.0 + Self::base(kit, Face::Mono, 11.5, 17.55);
        kit.canvas.text(kit.fonts, Face::Mono, 11.5, x0 + 13.0, base, &label, theme.dim, frame.alpha, 0.0);
        let tx = x0 + 13.0 + 64.0 + 8.0;
        let title = kit.fonts.ellipsize(Face::Sans, &task.title, 13.0 * scale, 0.0, (CONTENT_W - 100.0 - 85.0) * scale);
        let base = y + 6.0 + Self::base(kit, Face::Sans, 13.0, 17.55);
        let adv = kit.canvas.text(kit.fonts, Face::Sans, 13.0, tx, base, &title, theme.dim, frame.alpha, 0.0);
        kit.canvas.rect(tx, base - 4.5, adv, 1.0, theme.dim, frame.alpha); // strikethrough
        self.icon_button(kit, frame, x0 + CONTENT_W - 6.0 - BTN, y + (h - BTN) / 2.0, Icon::Undo, Action::Restore(task.id.clone()), Some(&task.id));
        h
    }

    fn empty_state(&mut self, kit: &mut Kit, env: &Env, frame: &PanelFrame, model: &Model, y: f32, draw: bool) -> f32 {
        let theme = frame.theme;
        let msg = if model.has_feeds { env.t.t("list.empty", &[]) } else { env.t.t("list.noFeeds", &[]) };
        let mut h = 18.0 + 17.55;
        if draw {
            let w = kit.canvas.measure(kit.fonts, Face::Sans, 13.0, &msg, 0.0);
            let base = y + 18.0 + Self::base(kit, Face::Sans, 13.0, 17.55);
            kit.canvas.text(kit.fonts, Face::Sans, 13.0, frame.origin_x + (PANEL_W - w) / 2.0, base, &msg, theme.dim, frame.alpha, 0.0);
        }
        if !model.has_feeds {
            h += 10.0;
            let label = env.t.t("list.addFeed", &[]);
            let w = kit.canvas.measure(kit.fonts, Face::Sans, 13.0, &label, 0.0) + 28.0;
            if draw {
                let (x, by) = (frame.origin_x + (PANEL_W - w) / 2.0, y + h);
                let rect = Rect { x, y: by, w, h: 32.0 };
                if self.state.pointer.is_some_and(|(px, py)| rect.contains(px, py)) {
                    kit.canvas.rrect(x, by, w, 32.0, 8.0, theme.line.0, line_alpha(frame));
                }
                outline(kit, x, by, w, 32.0, theme.line.0, line_alpha(frame) * 1.6);
                let base = by + Self::base(kit, Face::Sans, 13.0, 32.0);
                kit.canvas.text(kit.fonts, Face::Sans, 13.0, x + 14.0, base, &label, theme.fg, frame.alpha, 0.0);
                self.hits.push(Hit { rect, action: Some(Action::OpenSettings), row: None });
            }
            h += 32.0;
        }
        h + 12.0
    }

    /// Walk the whole panel. With `draw` it paints and records hit regions at
    /// the current scroll; without it only measures. Returns the height of
    /// everything above the footer.
    fn layout(&mut self, env: &Env, kit: &mut Kit, frame: &PanelFrame, model: &Model, draw: bool) -> f32 {
        let top = if draw { PAD_TOP - self.state.scroll } else { PAD_TOP };
        let mut y = top;
        let theme = frame.theme;

        if model.view.upcoming.is_empty() && model.view.overdue.is_empty() {
            y += self.empty_state(kit, env, frame, model, y, draw);
        } else {
            let hero_id = model.view.hero.as_ref().map(|h| h.id.as_str());
            for (i, task) in model.view.upcoming.iter().take(MAX_UPCOMING_ROWS).enumerate() {
                let is_hero = i == 0 && hero_id == Some(task.id.as_str());
                y += self.task_row(kit, env, frame, model, task, y, is_hero, draw) + if is_hero { HERO_GAP } else { ROW_GAP };
            }
            if !model.view.overdue.is_empty() {
                y += self.section_title(kit, frame, y, &env.t.t("section.overdue", &[]), theme.late, None, draw);
                // When nothing is upcoming, the most recent miss is the hero.
                let overdue_hero = model.view.upcoming.is_empty();
                for (i, task) in model.view.overdue.iter().take(MAX_OVERDUE_ROWS).enumerate() {
                    let is_hero = overdue_hero && i == 0;
                    y += self.task_row(kit, env, frame, model, task, y, is_hero, draw) + if is_hero { HERO_GAP } else { ROW_GAP };
                }
            }
        }

        if !model.view.done_list.is_empty() {
            let n = model.view.done_list.len().to_string();
            let title = env.t.t("section.done", &[("n", &n)]);
            y += self.section_title(kit, frame, y, &title, theme.dim, Some(Action::ToggleDone), draw);
            if self.state.done_open {
                for task in &model.view.done_list {
                    y += self.done_row(kit, env, frame, task, y, draw) + ROW_GAP;
                }
            }
        }
        (y - top) + PAD_TOP + 8.0
    }

    fn footer(&mut self, kit: &mut Kit, env: &Env, frame: &PanelFrame, model: &Model) {
        let theme = frame.theme;
        let top = frame.viewport_h - FOOTER_H;
        let x = frame.origin_x;
        // Cover whatever scrolled underneath, then the divider.
        kit.canvas.rect(x, top, PANEL_W, FOOTER_H, theme.bg, frame.alpha);
        kit.canvas.rect(x + PAD_X, top, CONTENT_W, 1.0, theme.line.0, line_alpha(frame));

        let (text, color) = if frame.keyboard {
            let key = if self.state.snooze_for.is_some() { "kbd.snooze" } else { "kbd.hint" };
            (env.t.t(key, &[]), theme.dim)
        } else {
            let s = sync_summary(&model.sync, &env.t, env.tz);
            (s.text, if s.kind == "error" { theme.late } else { theme.dim })
        };
        let max_w = CONTENT_W - 2.0 * BTN - 16.0;
        let text = kit.fonts.ellipsize(Face::Sans, &text, 11.0 * kit.canvas.scale, 0.0, max_w * kit.canvas.scale);
        let base = top + 6.0 + Self::base(kit, Face::Sans, 11.0, BTN);
        kit.canvas.text(kit.fonts, Face::Sans, 11.0, x + PAD_X, base, &text, color, frame.alpha, 0.0);

        let y = top + 6.0;
        let mut bx = x + PAD_X + CONTENT_W - BTN;
        self.icon_button(kit, frame, bx, y, Icon::Gear, Action::OpenSettings, None);
        if model.has_feeds {
            bx -= BTN;
            self.icon_button(kit, frame, bx, y, Icon::Refresh, Action::Refresh, None);
        }
    }

    fn ensure_visible(&mut self, id: &str) {
        let Some(r) = self.row_rects.get(id).copied() else { return };
        let list_h = self.viewport_h - FOOTER_H;
        if r.y < self.state.scroll + 4.0 {
            self.state.scroll = (r.y - PAD_TOP).max(0.0);
        } else if r.y + r.h > self.state.scroll + list_h - 4.0 {
            self.state.scroll = r.y + r.h - list_h + 8.0;
        }
        self.clamp_scroll();
    }

    fn clamp_scroll(&mut self) {
        let natural = self.content_h + FOOTER_H;
        let shown = natural.min(crate::shell::OPEN_MAX_H);
        self.state.scroll = self.state.scroll.clamp(0.0, (natural - shown).max(0.0));
    }

    fn act(&mut self, action: Action, env: &Env) -> Vec<Effect> {
        match action {
            Action::OpenUrl(url) => vec![Effect::OpenUrl(url)],
            Action::MarkDone(id) => {
                self.source.mark_done(&id, env.wall_ms);
                if self.state.selected.as_deref() == Some(id.as_str()) {
                    self.state.selected = None;
                }
                vec![Effect::Redraw]
            }
            Action::Restore(id) => {
                self.source.restore(&id);
                vec![Effect::Redraw]
            }
            Action::ToggleSnooze(id) => {
                self.state.snooze_for = if self.state.snooze_for.as_deref() == Some(id.as_str()) { None } else { Some(id) };
                vec![Effect::Redraw]
            }
            Action::Snooze(id, kind) => {
                self.source.snooze(&id, kind, env.wall_ms, env.tz);
                self.state.snooze_for = None;
                vec![Effect::Redraw]
            }
            Action::ToggleDone => {
                self.state.done_open = !self.state.done_open;
                vec![Effect::Redraw]
            }
            Action::Refresh => {
                self.source.refresh();
                vec![Effect::RefreshFeeds]
            }
            Action::OpenSettings => vec![Effect::OpenSettings],
        }
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

    fn rest_style(&mut self, env: &Env, resting_style: &str, due_horizon_hours: f64) -> crate::shell::RestStyle {
        use crate::shell::RestStyle;
        let model = self.model(env);
        match blink_core::view::rest_mode_for(resting_style, model.view.hero.as_ref(), env.wall_ms, due_horizon_hours) {
            "hot" => RestStyle::Hot,
            "mini" => RestStyle::Mini,
            _ => RestStyle::Full,
        }
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
        })
    }

    fn panel_height(&mut self, env: &Env, kit: &mut Kit) -> f32 {
        let model = self.model(env);
        let theme = crate::ui::theme::Theme::dark();
        let frame = PanelFrame { theme: &theme, alpha: 1.0, viewport_h: crate::shell::OPEN_MAX_H, origin_x: 0.0, keyboard: false };
        let h = self.layout(env, kit, &frame, &model, false);
        self.content_h = h;
        h + FOOTER_H
    }

    fn draw_panel(&mut self, env: &Env, kit: &mut Kit, frame: &PanelFrame) {
        let model = self.model(env);
        self.content_h = self.layout(env, kit, frame, &model, false);
        self.viewport_h = frame.viewport_h;
        self.clamp_scroll();

        self.hits.clear();
        self.row_rects.clear();
        let list_h = (frame.viewport_h - FOOTER_H).max(0.0);
        kit.canvas.push_clip(frame.origin_x, 0.0, PANEL_W, list_h);
        self.layout(env, kit, frame, &model, true);
        kit.canvas.pop_clip();
        // Clip the recorded regions to the list so a row scrolled under the footer cannot be clicked.
        for h in &mut self.hits {
            h.rect.h = h.rect.h.min((list_h - h.rect.y).max(0.0));
            if h.rect.y < 0.0 {
                h.rect.h = (h.rect.h + h.rect.y).max(0.0);
                h.rect.y = 0.0;
            }
        }
        self.hits.retain(|h| h.rect.h > 0.0);
        self.footer(kit, env, frame, &model);
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

    fn next_wake(&mut self, env: &Env, panel_open: bool) -> Option<u64> {
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
