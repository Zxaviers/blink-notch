//! Projects: tasks with a status and a due date, and the files and folders that belong to a
//! project (the shelf). For an IT student: the repo folder, the spec, the thing due Friday.
//!
//! Tasks are typed into a small field drawn here (the notch takes the keyboard for it): a title,
//! and optionally `@fri`, `@tomorrow`, `@25/10` for the due date. Files and folders are only
//! paths: dropping them on the notch attaches them, a click opens them, nothing is copied.

use std::collections::HashMap;

use blink_core::countdown::urgency_of;
use blink_core::i18n::Lang;
use blink_core::projects::{parse_task_text, Project, Projects, Status, Task};
use blink_core::store::Store;
use serde_json::Value;

use super::{Action, Effect, Env, Key, KeyResult, Kit, Module, PanelFrame, PeekView, PointerKind, RestView, CONTENT_H};
use crate::access::{Node, NodeAction, Role, ROOT};
use crate::ui::canvas::Icon;
use crate::ui::text::Face;
use crate::ui::theme::Rgb;

const VIOLET: Rgb = Rgb(0xa7, 0x8b, 0xfa);
const GREEN: Rgb = Rgb(0x34, 0xd3, 0x99);
const BLUE: Rgb = Rgb(0x4e, 0xa8, 0xff);

const LEFT_W: f32 = 150.0;
const LEFT_GAP: f32 = 20.0;
const ROW_H: f32 = 32.0;
const ROW_GAP: f32 = 4.0;
const LIST_H: f32 = CONTENT_H - 4.0;

/// What a click, a key or a screen reader can ask of the page.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ProjectAction {
    AddTask,
    CycleStatus(String),
    DeleteTask(String),
    ToggleFiles,
    OpenFile(String),
    RevealFile(String),
    RemoveFile(String),
    NextProject,
    NewProject,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum View {
    Tasks,
    Files,
}

#[derive(Clone, Debug, PartialEq, Eq)]
enum EditTarget {
    NewTask,
    Task(String),
    ProjectName,
    NewProject,
}

/// A line of text being typed.
#[derive(Clone, Debug)]
struct Edit {
    target: EditTarget,
    text: Vec<char>,
    cursor: usize,
}

impl Edit {
    fn new(target: EditTarget, text: &str) -> Edit {
        let text: Vec<char> = text.chars().collect();
        Edit { target, cursor: text.len(), text }
    }

    fn insert(&mut self, s: &str) {
        for c in s.chars().filter(|c| !c.is_control()) {
            if self.text.len() < 300 {
                self.text.insert(self.cursor, c);
                self.cursor += 1;
            }
        }
    }

    fn string(&self) -> String {
        self.text.iter().collect()
    }
}

#[derive(Clone, Debug, PartialEq)]
enum Target {
    Add,
    ToggleFiles,
    NextProject,
    NewProject,
    Status(String),
    Delete(String),
    EditTask(String),
    OpenFile(String),
    RevealFile(String),
    RemoveFile(String),
    /// The text field itself (a click in it does nothing).
    Field,
}

#[derive(Clone, Debug)]
struct Hit {
    x: f32,
    y: f32,
    w: f32,
    h: f32,
    target: Target,
}

impl Hit {
    fn contains(&self, px: f32, py: f32) -> bool {
        px >= self.x && px < self.x + self.w && py >= self.y && py < self.y + self.h
    }
}

// ------------------------------------------------------------------- strings

fn word(lang: Lang, key: &str) -> &'static str {
    let id = lang == Lang::Id;
    match key {
        "title" => if id { "Proyek" } else { "Projects" },
        "done" => if id { "tugas selesai" } else { "tasks done" },
        "add" => if id { "Tugas" } else { "Task" },
        "files" => if id { "File" } else { "Files" },
        "no_tasks" => if id { "Belum ada tugas" } else { "No tasks yet" },
        "tasks_hint" => if id { "Tekan + Tugas lalu ketik judulnya." } else { "Press + Task and type a title." },
        "date_hint" => if id { "Tambah tanggal dengan @jumat atau @25/10" } else { "Add a date with @fri or @25/10" },
        "new_task_hint" => if id { "Judul tugas… (@jumat untuk tanggal)" } else { "Task title… (@fri for a date)" },
        "project_hint" => if id { "Nama proyek…" } else { "Project name…" },
        "drop" => if id { "Seret file atau folder ke sini" } else { "Drop files or folders here" },
        "drop_over" => if id { "Lepas untuk menambahkan" } else { "Release to add" },
        "overdue" => if id { "Terlewat" } else { "Overdue" },
        "today" => if id { "Hari ini" } else { "Today" },
        "tomorrow" => if id { "Besok" } else { "Tomorrow" },
        "delete" => if id { "Hapus" } else { "Delete" },
        "delete_q" => if id { "Hapus?" } else { "Delete?" },
        "open" => if id { "Buka" } else { "Open" },
        "reveal" => if id { "Tampilkan di folder" } else { "Show in folder" },
        "remove" => if id { "Lepas dari proyek" } else { "Remove from project" },
        "next_project" => if id { "Proyek berikutnya" } else { "Next project" },
        "new_project" => if id { "Proyek baru" } else { "New project" },
        "project" => if id { "Proyek" } else { "Project" },
        "todo" => if id { "belum" } else { "to do" },
        "doing" => if id { "dikerjakan" } else { "in progress" },
        "finished" => if id { "selesai" } else { "done" },
        "cycle" => if id { "Ganti status" } else { "Change status" },
        "added" => if id { "Ditambahkan" } else { "Added" },
        _ => "",
    }
}

const DAYS_EN: [&str; 7] = ["Mon", "Tue", "Wed", "Thu", "Fri", "Sat", "Sun"];
const DAYS_ID: [&str; 7] = ["Sen", "Sel", "Rab", "Kam", "Jum", "Sab", "Min"];
const MONTHS_EN: [&str; 12] = ["Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec"];
const MONTHS_ID: [&str; 12] = ["Jan", "Feb", "Mar", "Apr", "Mei", "Jun", "Jul", "Agu", "Sep", "Okt", "Nov", "Des"];

/// "Overdue", "Today", "Tomorrow", a weekday within the week, otherwise "23 Oct".
fn due_label(due_ms: i64, env: &Env) -> (String, Rgb) {
    let lang = env.t.lang();
    let zone = |ms: i64| jiff::Timestamp::from_millisecond(ms).unwrap_or_default().to_zoned(env.tz.clone());
    let (due, now) = (zone(due_ms), zone(env.wall_ms));
    let days = (due.date() - now.date()).get_days();
    if due_ms < env.wall_ms {
        return (word(lang, "overdue").to_string(), Rgb(0xec, 0x7f, 0x72));
    }
    let (days_names, months) = if lang == Lang::Id { (&DAYS_ID, &MONTHS_ID) } else { (&DAYS_EN, &MONTHS_EN) };
    match days {
        0 => (word(lang, "today").to_string(), Rgb(0xe0, 0xb2, 0x5a)),
        1 => (word(lang, "tomorrow").to_string(), Rgb(0xe0, 0xb2, 0x5a)),
        2..=6 => (days_names[due.weekday().to_monday_zero_offset() as usize].to_string(), BLUE),
        _ => (format!("{} {}", due.day(), months[due.month() as usize - 1]), BLUE),
    }
}

// -------------------------------------------------------------------- module

pub struct ProjectsModule {
    store: Store,
    data: Projects,
    view: View,
    edit: Option<Edit>,
    /// The row or file the keyboard is on.
    selected: Option<String>,
    scroll: f32,
    hits: Vec<Hit>,
    pointer: Option<(f32, f32)>,
    /// A delete waiting for its second click.
    confirm: Option<String>,
    /// Files are being dragged over the notch.
    drag_over: bool,
    keyboard: bool,
    /// The page took the keyboard for typing (so it gives it back afterwards).
    kb_taken: bool,
    /// Where each row is in list space, to keep the selection in view.
    row_tops: HashMap<String, f32>,
}

impl ProjectsModule {
    pub fn new(store: Store, now_ms: i64) -> Self {
        let data = Projects::load(&store, now_ms);
        ProjectsModule { store, data, view: View::Tasks, edit: None, selected: None, scroll: 0.0, hits: Vec::new(), pointer: None, confirm: None, drag_over: false, keyboard: false, kb_taken: false, row_tops: HashMap::new() }
    }

    /// For tests and the host: what is saved.
    pub fn data(&self) -> &Projects {
        &self.data
    }

    /// Files are being dragged over the notch (highlight the drop zone).
    pub fn set_drag_over(&mut self, over: bool) {
        self.drag_over = over;
        if over {
            self.view = View::Files;
        }
    }

    fn save(&self) {
        self.data.save(&self.store);
    }

    fn project(&self) -> Option<Project> {
        self.data.selected_project().cloned()
    }

    fn pid(&self) -> Option<String> {
        self.data.selected.clone()
    }

    fn over(&self, x: f32, y: f32, w: f32, h: f32) -> bool {
        self.pointer.is_some_and(|(px, py)| px >= x && px < x + w && py >= y && py < y + h)
    }

    /// How many rows the list has (the new-task field counts as one).
    fn row_count(&self) -> usize {
        let Some(p) = self.data.selected_project() else { return 0 };
        let extra = usize::from(self.edit.as_ref().is_some_and(|e| e.target == EditTarget::NewTask));
        extra + if self.view == View::Tasks { p.tasks.len() } else { p.files.len() }
    }

    fn clamp_scroll(&mut self) {
        let rows = self.row_count();
        let content = if rows == 0 { 0.0 } else { rows as f32 * (ROW_H + ROW_GAP) - ROW_GAP };
        self.scroll = self.scroll.clamp(0.0, (content - LIST_H).max(0.0));
    }

    fn ensure_visible(&mut self, id: &str) {
        if let Some(top) = self.row_tops.get(id).copied() {
            if top < self.scroll {
                self.scroll = top;
            } else if top + ROW_H > self.scroll + LIST_H {
                self.scroll = top + ROW_H - LIST_H;
            }
            self.clamp_scroll();
        }
    }

    fn start_edit(&mut self, target: EditTarget, text: &str) -> Vec<Effect> {
        if !self.keyboard {
            self.kb_taken = true;
        }
        self.edit = Some(Edit::new(target, text));
        self.view = if matches!(self.edit.as_ref().map(|e| &e.target), Some(EditTarget::NewTask | EditTarget::Task(_))) { View::Tasks } else { self.view };
        self.scroll = 0.0;
        vec![Effect::TakeKeyboard, Effect::Redraw]
    }

    /// Effects that end a typing session: the redraw, and the keyboard back if this page took it.
    fn finish_typing(&mut self) -> Vec<Effect> {
        let mut effects = vec![Effect::Redraw];
        if self.kb_taken {
            self.kb_taken = false;
            effects.push(Effect::ReleaseKeyboard);
        }
        effects
    }

    fn commit(&mut self, env: &Env) -> Vec<Effect> {
        let Some(edit) = self.edit.take() else { return Vec::new() };
        let text = edit.string();
        let Some(pid) = self.pid() else { return self.finish_typing() };
        match edit.target {
            EditTarget::NewTask => {
                let (title, due) = parse_task_text(&text, env.wall_ms, env.tz);
                if let Some(id) = self.data.add_task(&pid, &title, due, env.wall_ms) {
                    self.selected = Some(id);
                }
            }
            EditTarget::Task(id) => {
                let (title, due) = parse_task_text(&text, env.wall_ms, env.tz);
                self.data.edit_task(&pid, &id, &title, due);
            }
            EditTarget::ProjectName => self.data.rename_project(&pid, &text),
            EditTarget::NewProject => {
                if !text.trim().is_empty() {
                    let id = self.data.add_project(&text, env.wall_ms);
                    self.data.select(&id);
                    self.scroll = 0.0;
                }
            }
        }
        self.save();
        self.finish_typing()
    }

    fn do_action(&mut self, action: &ProjectAction, env: &Env) -> Vec<Effect> {
        let Some(pid) = self.pid() else { return Vec::new() };
        match action {
            ProjectAction::AddTask => return self.start_edit(EditTarget::NewTask, ""),
            ProjectAction::CycleStatus(id) => {
                self.data.cycle_status(&pid, id, env.wall_ms);
                self.save();
            }
            ProjectAction::DeleteTask(id) => {
                self.data.delete_task(&pid, id);
                if self.selected.as_deref() == Some(id.as_str()) {
                    self.selected = None;
                }
                self.confirm = None;
                self.save();
                self.clamp_scroll();
            }
            ProjectAction::ToggleFiles => {
                self.view = if self.view == View::Tasks { View::Files } else { View::Tasks };
                self.scroll = 0.0;
                self.selected = None;
            }
            ProjectAction::OpenFile(id) => {
                if let Some(f) = self.project().and_then(|p| p.files.into_iter().find(|f| &f.id == id)) {
                    return vec![Effect::OpenPath(f.path)];
                }
            }
            ProjectAction::RevealFile(id) => {
                if let Some(f) = self.project().and_then(|p| p.files.into_iter().find(|f| &f.id == id)) {
                    return vec![Effect::RevealPath(f.path)];
                }
            }
            ProjectAction::RemoveFile(id) => {
                self.data.remove_attachment(&pid, id);
                self.confirm = None;
                self.save();
                self.clamp_scroll();
            }
            ProjectAction::NextProject => {
                self.data.select_step(1);
                self.scroll = 0.0;
                self.selected = None;
                self.save();
            }
            ProjectAction::NewProject => return self.start_edit(EditTarget::NewProject, ""),
        }
        vec![Effect::Redraw]
    }

    /// The rows the keyboard can move over, in order.
    fn selectable(&self) -> Vec<String> {
        let Some(p) = self.data.selected_project() else { return Vec::new() };
        match self.view {
            View::Tasks => p.ordered_tasks().iter().map(|t| t.id.clone()).collect(),
            View::Files => p.files.iter().map(|f| f.id.clone()).collect(),
        }
    }
}

// ------------------------------------------------------------------- drawing

#[allow(clippy::too_many_arguments)]
fn put(kit: &mut Kit, face: Face, size: f32, x: f32, box_y: f32, box_h: f32, max_w: f32, text: &str, color: Rgb, alpha: f32) -> f32 {
    let px = size * kit.canvas.scale;
    let cut = kit.fonts.ellipsize(face, text, px, 0.0, max_w.max(8.0) * kit.canvas.scale);
    let base = box_y + kit.fonts.baseline(face, size, box_h);
    kit.canvas.text(kit.fonts, face, size, x, base, &cut, color, alpha, 0.0)
}

fn put_centered(kit: &mut Kit, face: Face, size: f32, cx: f32, box_y: f32, box_h: f32, max_w: f32, text: &str, color: Rgb, alpha: f32) {
    let px = size * kit.canvas.scale;
    let cut = kit.fonts.ellipsize(face, text, px, 0.0, max_w.max(8.0) * kit.canvas.scale);
    let w = kit.canvas.measure(kit.fonts, face, size, &cut, 0.0);
    let base = box_y + kit.fonts.baseline(face, size, box_h);
    kit.canvas.text(kit.fonts, face, size, cx - w / 2.0, base, &cut, color, alpha, 0.0);
}

fn mix(a: Rgb, b: Rgb, t: f32) -> Rgb {
    let m = |x: u8, y: u8| (f32::from(x) + (f32::from(y) - f32::from(x)) * t).round() as u8;
    Rgb(m(a.0, b.0), m(a.1, b.1), m(a.2, b.2))
}

/// A dashed outline (a drop zone), made of short bars.
fn dashed(kit: &mut Kit, x: f32, y: f32, w: f32, h: f32, color: Rgb, alpha: f32) {
    let (dash, gap): (f32, f32) = (5.0, 4.0);
    let mut at = x + 8.0;
    while at < x + w - 8.0 {
        let len = dash.min(x + w - 8.0 - at);
        kit.canvas.rect(at, y, len, 1.0, color, alpha);
        kit.canvas.rect(at, y + h - 1.0, len, 1.0, color, alpha);
        at += dash + gap;
    }
    let mut at = y + 8.0;
    while at < y + h - 8.0 {
        let len = dash.min(y + h - 8.0 - at);
        kit.canvas.rect(x, at, 1.0, len, color, alpha);
        kit.canvas.rect(x + w - 1.0, at, 1.0, len, color, alpha);
        at += dash + gap;
    }
}

impl ProjectsModule {
    /// A one-line text field with a caret. Shows the end of the text when it is longer than the field.
    #[allow(clippy::too_many_arguments)]
    fn field(&mut self, kit: &mut Kit, frame: &PanelFrame, x: f32, y: f32, w: f32, h: f32, hint: &str) {
        let theme = frame.theme;
        kit.canvas.rrect(x - 1.0, y - 1.0, w + 2.0, h + 2.0, 12.0, VIOLET, frame.alpha * 0.8);
        kit.canvas.rrect(x, y, w, h, 11.0, mix(theme.bg, theme.fg, 0.1), frame.alpha);
        let Some(edit) = &self.edit else { return };
        let text = edit.string();
        let size = 12.5;
        let room = w - 24.0;
        if text.is_empty() {
            put(kit, Face::Sans, size, x + 12.0, y, h, room, hint, theme.dim, frame.alpha);
            kit.canvas.rect(x + 12.0, y + 8.0, 1.5, h - 16.0, theme.fg, frame.alpha);
            return;
        }
        // Drop characters from the left until the caret fits.
        let chars: Vec<char> = text.chars().collect();
        let mut start = 0;
        let width_of = |kit: &mut Kit, from: usize, to: usize| kit.canvas.measure(kit.fonts, Face::Sans, size, &chars[from..to].iter().collect::<String>(), 0.0);
        while start < edit.cursor && width_of(kit, start, edit.cursor) > room {
            start += 1;
        }
        let shown: String = chars[start..].iter().collect();
        let px = size * kit.canvas.scale;
        let cut = kit.fonts.ellipsize(Face::Sans, &shown, px, 0.0, room * kit.canvas.scale);
        let base = y + kit.fonts.baseline(Face::Sans, size, h);
        kit.canvas.text(kit.fonts, Face::Sans, size, x + 12.0, base, &cut, theme.fg, frame.alpha, 0.0);
        let caret_x = x + 12.0 + width_of(kit, start, edit.cursor);
        kit.canvas.rect(caret_x, y + 8.0, 1.5, h - 16.0, theme.fg, frame.alpha);
    }

    fn left_column(&mut self, kit: &mut Kit, env: &Env, frame: &PanelFrame, project: &Project) {
        let theme = frame.theme;
        let lang = env.t.lang();
        let x = frame.content_x();
        let y0 = frame.origin_y;

        // The project's name, with a button for a new one.
        let editing_name = self.edit.as_ref().is_some_and(|e| matches!(e.target, EditTarget::ProjectName | EditTarget::NewProject));
        if editing_name {
            let hint = word(lang, "project_hint");
            self.field(kit, frame, x, y0 + 2.0, LEFT_W, 24.0, hint);
            self.hits.push(Hit { x, y: y0 + 2.0, w: LEFT_W, h: 24.0, target: Target::Field });
        } else {
            let over = self.over(x - 2.0, y0, LEFT_W - 24.0, 26.0);
            put(kit, Face::SansBold, 13.0, x, y0 + 2.0, 24.0, LEFT_W - 28.0, &project.name, if over { theme.fg } else { VIOLET }, frame.alpha);
            kit.canvas.icon(Icon::ChevronRight, x + LEFT_W - 36.0, y0 + 14.0, 10.0, theme.dim, frame.alpha * if over { 1.0 } else { 0.0 });
            self.hits.push(Hit { x: x - 2.0, y: y0, w: LEFT_W - 24.0, h: 26.0, target: Target::NextProject });
            let plus_over = self.over(x + LEFT_W - 24.0, y0 + 2.0, 24.0, 24.0);
            kit.canvas.circle(x + LEFT_W - 12.0, y0 + 14.0, 10.0, theme.fg, frame.alpha * if plus_over { 0.22 } else { 0.1 });
            kit.canvas.icon(Icon::Plus, x + LEFT_W - 12.0, y0 + 14.0, 10.0, theme.fg, frame.alpha);
            self.hits.push(Hit { x: x + LEFT_W - 24.0, y: y0 + 2.0, w: 24.0, h: 24.0, target: Target::NewProject });
        }

        // The count, big and light, and how far along.
        let (done, all) = project.counts();
        let big = format!("{done}");
        let bw = kit.canvas.measure(kit.fonts, Face::SansLight, 40.0, &big, 0.0);
        let base = y0 + 24.0 + kit.fonts.baseline(Face::SansLight, 40.0, 46.0);
        kit.canvas.text(kit.fonts, Face::SansLight, 40.0, x - 1.0, base, &big, theme.fg, frame.alpha, 0.0);
        let total = format!("/ {all}");
        let tbase = y0 + 24.0 + kit.fonts.baseline(Face::SansLight, 18.0, 46.0) + 8.0;
        kit.canvas.text(kit.fonts, Face::SansLight, 18.0, x + bw + 4.0, tbase, &total, theme.dim, frame.alpha, 0.0);
        put(kit, Face::Sans, 11.0, x, y0 + 68.0, 14.0, LEFT_W, word(lang, "done"), theme.dim, frame.alpha);
        let fraction = if all > 0 { done as f32 / all as f32 } else { 0.0 };
        kit.canvas.bar(x + 1.0, x + LEFT_W - 1.0, y0 + 88.0, 3.0, theme.fg, frame.alpha * 0.16);
        if fraction > 0.0 {
            kit.canvas.bar(x + 1.0, x + 1.0 + (LEFT_W - 2.0) * fraction, y0 + 88.0, 3.0, GREEN, frame.alpha);
        }

        // The two buttons.
        let by = y0 + CONTENT_H - 26.0;
        let add_label = word(lang, "add");
        let aw = kit.canvas.measure(kit.fonts, Face::SansSemibold, 12.0, add_label, 0.0) + 38.0;
        let over = self.over(x, by, aw, 26.0);
        kit.canvas.rrect(x, by, aw, 26.0, 13.0, VIOLET, frame.alpha * if over { 1.0 } else { 0.94 });
        kit.canvas.icon(Icon::Plus, x + 15.0, by + 13.0, 10.0, Rgb(0x14, 0x0e, 0x2a), frame.alpha);
        put(kit, Face::SansSemibold, 12.0, x + 26.0, by, 26.0, aw - 30.0, add_label, Rgb(0x14, 0x0e, 0x2a), frame.alpha);
        self.hits.push(Hit { x, y: by, w: aw, h: 26.0, target: Target::Add });

        let fx = x + aw + 8.0;
        let count = project.files.len();
        let flabel = if count > 0 { format!("{count}") } else { word(lang, "files").to_string() };
        let fw = (kit.canvas.measure(kit.fonts, Face::Sans, 12.0, &flabel, 0.0) + 38.0).min(LEFT_W - aw - 8.0);
        let lit = self.view == View::Files;
        let over = self.over(fx, by, fw, 26.0);
        if lit {
            kit.canvas.rrect(fx, by, fw, 26.0, 13.0, theme.fg, frame.alpha * 0.95);
        } else {
            kit.canvas.rrect(fx, by, fw, 26.0, 13.0, theme.fg, frame.alpha * if over { 0.22 } else { 0.1 });
        }
        let ink = if lit { theme.bg } else { theme.fg };
        kit.canvas.icon(Icon::Folder, fx + 15.0, by + 13.0, 11.0, ink, frame.alpha);
        put(kit, Face::Sans, 12.0, fx + 26.0, by, 26.0, fw - 28.0, &flabel, ink, frame.alpha);
        self.hits.push(Hit { x: fx, y: by, w: fw, h: 26.0, target: Target::ToggleFiles });
    }

    fn status_mark(&self, kit: &mut Kit, frame: &PanelFrame, cx: f32, cy: f32, status: Status) {
        let theme = frame.theme;
        match status {
            Status::Todo => kit.canvas.arc(cx, cy, 7.0, 1.6, 0.0, 360.0, theme.fg, frame.alpha * 0.4),
            Status::Doing => {
                kit.canvas.arc(cx, cy, 7.0, 1.6, 0.0, 360.0, VIOLET, frame.alpha * 0.45);
                kit.canvas.arc(cx, cy, 7.0, 1.6, -90.0, 180.0, VIOLET, frame.alpha);
                kit.canvas.circle(cx, cy, 2.2, VIOLET, frame.alpha);
            }
            Status::Done => {
                kit.canvas.circle(cx, cy, 8.0, GREEN, frame.alpha);
                kit.canvas.icon(Icon::Check, cx, cy, 9.0, Rgb(0x04, 0x20, 0x14), frame.alpha);
            }
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn task_row(&mut self, kit: &mut Kit, env: &Env, frame: &PanelFrame, task: &Task, x: f32, y: f32, w: f32, top_in_list: f32) {
        let theme = frame.theme;
        let lang = env.t.lang();
        self.row_tops.insert(task.id.clone(), top_in_list);
        let hovered = self.over(x, y, w, ROW_H);
        let selected = self.keyboard && self.selected.as_deref() == Some(task.id.as_str());
        let done = task.status == Status::Done;
        let editing = self.edit.as_ref().is_some_and(|e| e.target == EditTarget::Task(task.id.clone()));
        if editing {
            self.field(kit, frame, x, y, w, ROW_H, word(lang, "new_task_hint"));
            self.hits.push(Hit { x, y, w, h: ROW_H, target: Target::Field });
            return;
        }
        if selected {
            kit.canvas.rrect(x - 2.0, y - 2.0, w + 4.0, ROW_H + 4.0, 13.0, theme.fg, frame.alpha * 0.9);
            kit.canvas.rrect(x, y, w, ROW_H, 11.0, theme.bg, frame.alpha);
        }
        let tone = if done { GREEN } else { VIOLET };
        kit.canvas.rrect(x, y, w, ROW_H, 11.0, mix(theme.bg, tone, if done { 0.10 } else if hovered { 0.24 } else { 0.15 }), frame.alpha);
        self.status_mark(kit, frame, x + 18.0, y + ROW_H / 2.0, task.status);
        self.hits.push(Hit { x, y, w: 36.0, h: ROW_H, target: Target::Status(task.id.clone()) });

        // The right end: a delete button while pointed at, otherwise when it is due.
        let mut right = x + w - 10.0;
        if hovered || selected {
            let confirming = self.confirm.as_deref() == Some(task.id.as_str());
            let label = if confirming { word(lang, "delete_q") } else { "" };
            let lw = if confirming { kit.canvas.measure(kit.fonts, Face::SansSemibold, 11.0, label, 0.0) + 8.0 } else { 0.0 };
            let bw = 22.0 + lw;
            let (bx, over) = (right - bw, self.over(right - bw, y + 5.0, bw, ROW_H - 10.0));
            kit.canvas.rrect(bx, y + 5.0, bw, ROW_H - 10.0, 11.0, if confirming { theme.late } else { theme.fg }, frame.alpha * if confirming { 0.9 } else if over { 0.25 } else { 0.1 });
            if confirming {
                put(kit, Face::SansSemibold, 11.0, bx + 8.0, y, ROW_H, lw, label, Rgb(0x2a, 0x08, 0x06), frame.alpha);
            }
            kit.canvas.icon(Icon::Close, bx + bw - 11.0, y + ROW_H / 2.0, 9.0, if confirming { Rgb(0x2a, 0x08, 0x06) } else { theme.fg }, frame.alpha);
            self.hits.push(Hit { x: bx, y: y + 5.0, w: bw, h: ROW_H - 10.0, target: Target::Delete(task.id.clone()) });
            right = bx - 6.0;
        } else if let (Some(due), false) = (task.due_ms, done) {
            let (text, color) = due_label(due, env);
            let tw = kit.canvas.measure(kit.fonts, Face::SansSemibold, 10.5, &text, 0.0) + 14.0;
            kit.canvas.rrect(right - tw, y + 7.0, tw, ROW_H - 14.0, 9.0, color, frame.alpha * 0.2);
            put(kit, Face::SansSemibold, 10.5, right - tw + 7.0, y + 7.0, ROW_H - 14.0, tw, &text, color, frame.alpha);
            right -= tw + 6.0;
        }
        let title_color = if done { theme.dim } else { theme.fg };
        let adv = put(kit, Face::Sans, 12.5, x + 36.0, y, ROW_H, right - (x + 36.0), &task.title, title_color, frame.alpha);
        if done {
            kit.canvas.rect(x + 36.0, y + ROW_H / 2.0 + 0.5, adv.min(right - x - 36.0), 1.0, theme.dim, frame.alpha);
        }
        self.hits.push(Hit { x: x + 36.0, y, w: (right - x - 36.0).max(10.0), h: ROW_H, target: Target::EditTask(task.id.clone()) });
    }

    #[allow(clippy::too_many_arguments)]
    fn file_row(&mut self, kit: &mut Kit, env: &Env, frame: &PanelFrame, file: &blink_core::projects::Attachment, x: f32, y: f32, w: f32, top_in_list: f32) {
        let theme = frame.theme;
        let lang = env.t.lang();
        self.row_tops.insert(file.id.clone(), top_in_list);
        let hovered = self.over(x, y, w, ROW_H);
        let selected = self.keyboard && self.selected.as_deref() == Some(file.id.as_str());
        if selected {
            kit.canvas.rrect(x - 2.0, y - 2.0, w + 4.0, ROW_H + 4.0, 13.0, theme.fg, frame.alpha * 0.9);
            kit.canvas.rrect(x, y, w, ROW_H, 11.0, theme.bg, frame.alpha);
        }
        kit.canvas.rrect(x, y, w, ROW_H, 11.0, mix(theme.bg, if file.folder { BLUE } else { VIOLET }, if hovered { 0.24 } else { 0.14 }), frame.alpha);
        kit.canvas.icon(if file.folder { Icon::Folder } else { Icon::File }, x + 18.0, y + ROW_H / 2.0, 14.0, if file.folder { BLUE } else { VIOLET }, frame.alpha);
        self.hits.push(Hit { x, y, w, h: ROW_H, target: Target::OpenFile(file.id.clone()) });
        let mut right = x + w - 10.0;
        if hovered || selected {
            for (icon, target, tip) in [(Icon::Close, Target::RemoveFile(file.id.clone()), "remove"), (Icon::ArrowUpRight, Target::RevealFile(file.id.clone()), "reveal")] {
                let bx = right - 22.0;
                let over = self.over(bx, y + 5.0, 22.0, ROW_H - 10.0);
                kit.canvas.rrect(bx, y + 5.0, 22.0, ROW_H - 10.0, 11.0, theme.fg, frame.alpha * if over { 0.25 } else { 0.1 });
                kit.canvas.icon(icon, bx + 11.0, y + ROW_H / 2.0, 9.0, theme.fg, frame.alpha);
                self.hits.push(Hit { x: bx, y: y + 5.0, w: 22.0, h: ROW_H - 10.0, target });
                right = bx - 4.0;
                let _ = (tip, lang);
            }
        } else {
            let parent = file.parent();
            if !parent.is_empty() {
                let pw = (right - x - 150.0).clamp(40.0, 110.0);
                let shown = kit.fonts.ellipsize(Face::Sans, &parent, 10.0 * kit.canvas.scale, 0.0, pw * kit.canvas.scale);
                let sw = kit.canvas.measure(kit.fonts, Face::Sans, 10.0, &shown, 0.0);
                let base = y + kit.fonts.baseline(Face::Sans, 10.0, ROW_H);
                kit.canvas.text(kit.fonts, Face::Sans, 10.0, right - sw, base, &shown, theme.dim, frame.alpha * 0.8, 0.0);
                right -= sw + 8.0;
            }
        }
        put(kit, Face::Sans, 12.5, x + 36.0, y, ROW_H, right - (x + 36.0), &file.name(), theme.fg, frame.alpha);
    }

    fn list(&mut self, kit: &mut Kit, env: &Env, frame: &PanelFrame, project: &Project) {
        let theme = frame.theme;
        let lang = env.t.lang();
        let x = frame.content_x() + LEFT_W + LEFT_GAP;
        let w = frame.content_w() - LEFT_W - LEFT_GAP;
        let top = frame.origin_y + 2.0;
        self.row_tops.clear();
        let adding = self.edit.as_ref().is_some_and(|e| e.target == EditTarget::NewTask);
        let empty = if self.view == View::Tasks { project.tasks.is_empty() && !adding } else { project.files.is_empty() };

        if self.view == View::Files && (empty || self.drag_over) {
            // The drop zone: always there when nothing is attached, and lit while files are over the notch.
            let lit = self.drag_over;
            if lit {
                kit.canvas.rrect(x, top, w, LIST_H, 14.0, VIOLET, frame.alpha * 0.16);
            }
            dashed(kit, x, top, w, LIST_H, if lit { VIOLET } else { theme.fg }, frame.alpha * if lit { 1.0 } else { 0.3 });
            if empty {
                let cx = x + w / 2.0;
                kit.canvas.icon(Icon::Download, cx, top + LIST_H / 2.0 - 14.0, 20.0, if lit { VIOLET } else { theme.dim }, frame.alpha);
                put_centered(kit, Face::Sans, 12.5, cx, top + LIST_H / 2.0 + 2.0, 20.0, w - 20.0, word(lang, if lit { "drop_over" } else { "drop" }), if lit { theme.fg } else { theme.dim }, frame.alpha);
                return;
            }
        }
        if empty {
            let cx = x + w / 2.0;
            put_centered(kit, Face::SansSemibold, 13.0, cx, top + 30.0, 20.0, w, word(lang, "no_tasks"), theme.fg, frame.alpha);
            put_centered(kit, Face::Sans, 11.5, cx, top + 54.0, 18.0, w - 10.0, word(lang, "tasks_hint"), theme.dim, frame.alpha);
            put_centered(kit, Face::Sans, 11.5, cx, top + 72.0, 18.0, w - 10.0, word(lang, "date_hint"), theme.dim, frame.alpha * 0.8);
            return;
        }

        kit.canvas.push_clip(x - 4.0, top - 2.0, w + 8.0, LIST_H + 4.0);
        let mut row = 0usize;
        let place = |row: usize, scroll: f32| top + row as f32 * (ROW_H + ROW_GAP) - scroll;
        if adding && self.view == View::Tasks {
            let y = place(row, self.scroll);
            self.field(kit, frame, x, y, w, ROW_H, word(lang, "new_task_hint"));
            self.hits.push(Hit { x, y, w, h: ROW_H, target: Target::Field });
            row += 1;
        }
        match self.view {
            View::Tasks => {
                for task in project.ordered_tasks() {
                    let y = place(row, self.scroll);
                    let tl = row as f32 * (ROW_H + ROW_GAP);
                    if y + ROW_H >= top - 2.0 && y <= top + LIST_H + 2.0 {
                        self.task_row(kit, env, frame, task, x, y, w, tl);
                    } else {
                        self.row_tops.insert(task.id.clone(), tl);
                    }
                    row += 1;
                }
            }
            View::Files => {
                for file in &project.files {
                    let y = place(row, self.scroll);
                    let tl = row as f32 * (ROW_H + ROW_GAP);
                    if y + ROW_H >= top - 2.0 && y <= top + LIST_H + 2.0 {
                        self.file_row(kit, env, frame, file, x, y, w, tl);
                    } else {
                        self.row_tops.insert(file.id.clone(), tl);
                    }
                    row += 1;
                }
            }
        }
        kit.canvas.pop_clip();
        // A row cut by the edge of the list cannot be clicked where it is cut.
        let (lo, hi) = (top, top + LIST_H);
        for h in &mut self.hits {
            if h.x >= x - 1.0 && !matches!(h.target, Target::Field) {
                let (y0, y1) = (h.y.max(lo), (h.y + h.h).min(hi));
                h.y = y0;
                h.h = (y1 - y0).max(0.0);
            }
        }
        self.hits.retain(|h| h.h > 0.0);
    }
}

impl Module for ProjectsModule {
    fn id(&self) -> &'static str {
        "projects"
    }

    fn title(&self, lang: Lang) -> String {
        word(lang, "title").to_string()
    }

    fn accent(&self) -> Rgb {
        VIOLET
    }

    fn title_parts(&self, lang: Lang) -> (String, String) {
        let (a, b) = if lang == Lang::Id { ("Pro", "yek") } else { ("Pro", "jects") };
        (a.to_string(), b.to_string())
    }

    fn as_any_mut(&mut self) -> &mut dyn std::any::Any {
        self
    }

    fn set_options(&mut self, _options: &Value) {}

    fn rest_priority(&mut self, _env: &Env, _settings: &blink_core::settings::Settings) -> u8 {
        0
    }

    fn rest_style(&mut self, _env: &Env, _resting_style: &str, _due_horizon_hours: f64) -> crate::shell::RestStyle {
        crate::shell::RestStyle::Full
    }

    fn rest(&mut self, _env: &Env) -> RestView {
        RestView { label: String::new(), count: String::new(), urgency: "later" }
    }

    fn peek(&mut self, _env: &Env) -> Option<PeekView> {
        None
    }

    fn due_dates(&mut self, env: &Env) -> Vec<(i64, &'static str)> {
        self.data.projects.iter().flat_map(|p| p.due_dates()).map(|d| (d, urgency_of(d, env.wall_ms))).collect()
    }

    fn captures_keys(&self) -> bool {
        self.edit.is_some()
    }

    fn panel_height(&mut self, _env: &Env, _kit: &mut Kit) -> f32 {
        CONTENT_H
    }

    fn draw_panel(&mut self, env: &Env, kit: &mut Kit, frame: &PanelFrame) {
        self.hits.clear();
        let Some(project) = self.project() else { return };
        self.clamp_scroll();
        self.left_column(kit, env, frame, &project);
        self.list(kit, env, frame, &project);
    }

    fn pointer(&mut self, kind: PointerKind, x: f32, y: f32, env: &Env) -> Vec<Effect> {
        match kind {
            PointerKind::Leave => {
                self.pointer = None;
                self.confirm = None;
                vec![Effect::Redraw]
            }
            PointerKind::Move => {
                self.pointer = Some((x, y));
                if self.confirm.is_some() && !self.hits.iter().any(|h| h.contains(x, y) && matches!(&h.target, Target::Delete(id) | Target::RemoveFile(id) if self.confirm.as_deref() == Some(id.as_str()))) {
                    self.confirm = None;
                }
                vec![Effect::Redraw]
            }
            PointerKind::Down => {
                let Some(hit) = self.hits.iter().rev().find(|h| h.contains(x, y)).cloned() else {
                    // A click away from a field ends the typing (what was typed is kept).
                    return if self.edit.is_some() { self.commit(env) } else { Vec::new() };
                };
                if self.edit.is_some() && hit.target != Target::Field {
                    let mut effects = self.commit(env);
                    // The click still does its own thing after the field is closed, except a second "add".
                    if matches!(hit.target, Target::Add | Target::NewProject) {
                        return effects;
                    }
                    effects.extend(self.click(&hit.target, env));
                    return effects;
                }
                self.click(&hit.target, env)
            }
            PointerKind::Up => Vec::new(),
            PointerKind::Wheel(delta) => {
                self.scroll -= delta;
                self.clamp_scroll();
                vec![Effect::Redraw]
            }
        }
    }

    fn key(&mut self, key: Key, env: &Env) -> KeyResult {
        let handled = |effects: Vec<Effect>| KeyResult { handled: true, close: false, effects };
        if let Some(edit) = self.edit.as_mut() {
            match key {
                Key::Char(c) => edit.insert(&c.to_string()),
                Key::Space => edit.insert(" "),
                Key::Backspace => {
                    if edit.cursor > 0 {
                        edit.cursor -= 1;
                        edit.text.remove(edit.cursor);
                    }
                }
                Key::Delete => {
                    if edit.cursor < edit.text.len() {
                        edit.text.remove(edit.cursor);
                    }
                }
                Key::Left => edit.cursor = edit.cursor.saturating_sub(1),
                Key::Right => edit.cursor = (edit.cursor + 1).min(edit.text.len()),
                Key::Home => edit.cursor = 0,
                Key::End => edit.cursor = edit.text.len(),
                Key::Enter => return handled(self.commit(env)),
                Key::Escape => {
                    self.edit = None;
                    return handled(self.finish_typing());
                }
                _ => {}
            }
            return handled(vec![Effect::Redraw]);
        }
        let ids = self.selectable();
        let at = self.selected.as_ref().and_then(|s| ids.iter().position(|i| i == s));
        let last = ids.len().saturating_sub(1);
        let select = |this: &mut Self, idx: usize| -> KeyResult {
            if let Some(id) = ids.get(idx).cloned() {
                this.selected = Some(id.clone());
                this.ensure_visible(&id);
            }
            KeyResult { handled: true, close: false, effects: vec![Effect::Redraw] }
        };
        match key {
            Key::Down => select(self, at.map_or(0, |i| (i + 1).min(last))),
            Key::Up => select(self, at.map_or(last, |i| i.saturating_sub(1))),
            Key::Home => select(self, 0),
            Key::End => select(self, last),
            Key::Char('a') | Key::Char('A') | Key::Char('+') => handled(self.do_action(&ProjectAction::AddTask, env)),
            Key::Char('f') | Key::Char('F') => handled(self.do_action(&ProjectAction::ToggleFiles, env)),
            Key::Char('p') | Key::Char('P') => handled(self.do_action(&ProjectAction::NextProject, env)),
            Key::Char('b') | Key::Char('B') => handled(self.do_action(&ProjectAction::NewProject, env)),
            Key::Char('r') | Key::Char('R') => {
                let name = self.project().map(|p| p.name).unwrap_or_default();
                handled(self.start_edit(EditTarget::ProjectName, &name))
            }
            Key::Space | Key::Enter => {
                let Some(id) = self.selected.clone() else { return handled(Vec::new()) };
                match self.view {
                    View::Tasks if key == Key::Space => handled(self.do_action(&ProjectAction::CycleStatus(id), env)),
                    View::Tasks => {
                        let title = self.project().and_then(|p| p.tasks.into_iter().find(|t| t.id == id)).map(|t| t.title).unwrap_or_default();
                        handled(self.start_edit(EditTarget::Task(id), &title))
                    }
                    View::Files => handled(self.do_action(&ProjectAction::OpenFile(id), env)),
                }
            }
            Key::Delete => {
                let Some(id) = self.selected.clone() else { return handled(Vec::new()) };
                match self.view {
                    View::Tasks => handled(self.do_action(&ProjectAction::DeleteTask(id), env)),
                    View::Files => handled(self.do_action(&ProjectAction::RemoveFile(id), env)),
                }
            }
            _ => KeyResult { handled: false, close: key == Key::Escape, effects: Vec::new() },
        }
    }

    fn keyboard_mode(&mut self, on: bool, _env: &Env) {
        self.keyboard = on;
        if !on {
            // Losing the keyboard ends the typing; what was typed is dropped (nothing was committed).
            self.edit = None;
            self.kb_taken = false;
        }
    }

    fn paste(&mut self, text: &str) -> Vec<Effect> {
        if let Some(edit) = self.edit.as_mut() {
            // One line: a pasted block of text becomes its first line.
            edit.insert(text.lines().next().unwrap_or(""));
            return vec![Effect::Redraw];
        }
        Vec::new()
    }

    fn drop_files(&mut self, paths: &[String]) -> Vec<Effect> {
        self.drag_over = false;
        let Some(pid) = self.pid() else { return Vec::new() };
        let now = jiff::Timestamp::now().as_millisecond();
        let added = paths.iter().filter(|p| self.data.add_attachment(&pid, p, now)).count();
        self.view = View::Files;
        self.scroll = 0.0;
        if added == 0 {
            return vec![Effect::Redraw];
        }
        self.save();
        let name = self.project().map(|p| p.name).unwrap_or_default();
        vec![
            Effect::Redraw,
            Effect::ShowPeek(PeekView {
                code: name,
                title: format!("{added} {}", if added == 1 { "item added" } else { "items added" }),
                count: String::new(),
                urgency: "later",
                priority: 1,
                ttl_ms: Some(2600),
                key: Some("projects:drop".into()),
                progress: None,
                icon: Some(Icon::Folder),
                accent: Some(VIOLET),
                cover: None,
            }),
        ]
    }

    fn next_wake(&mut self, _env: &Env, _panel_open: bool) -> Option<u64> {
        None
    }

    fn perform(&mut self, action: Action, env: &Env) -> Vec<Effect> {
        match action {
            Action::Project(a) => self.do_action(&a, env),
            _ => Vec::new(),
        }
    }

    fn debug_now(&mut self, _env: &Env) -> Value {
        let project = self.project();
        serde_json::json!({
            "view": format!("{:?}", self.view),
            "editing": self.edit.as_ref().map(|e| e.string()),
            "project": project.as_ref().map(|p| p.name.clone()),
            "projects": self.data.projects.len(),
            "tasks": project.as_ref().map(|p| p.tasks.iter().map(|t| serde_json::json!({ "id": t.id, "title": t.title, "status": format!("{:?}", t.status), "due": t.due_ms })).collect::<Vec<_>>()),
            "files": project.as_ref().map(|p| p.files.iter().map(|f| f.path.clone()).collect::<Vec<_>>()),
            "hits": self.hits.iter().map(|h| serde_json::json!({ "x": h.x, "y": h.y, "w": h.w, "h": h.h, "target": format!("{:?}", h.target) })).collect::<Vec<_>>(),
        })
    }

    fn accessibility(&mut self, env: &Env, open: bool) -> Vec<Node> {
        if !open {
            return Vec::new();
        }
        let lang = env.t.lang();
        let Some(project) = self.project() else { return Vec::new() };
        let mut out = Vec::new();
        let (done, all) = project.counts();
        out.push(Node::new(300, ROOT, Role::Text, format!("{}: {}, {done} / {all} {}", word(lang, "project"), project.name, word(lang, "done"))));
        let rect_of = |hits: &[Hit], target: &Target| hits.iter().find(|h| &h.target == target).map_or((0.0, 0.0, 0.0, 0.0), |h| (h.x, h.y, h.w, h.h));
        let button = |out: &mut Vec<Node>, key: i32, name: String, target: Target, action: ProjectAction| {
            let mut n = Node::new(key, ROOT, Role::Button, name);
            n.rect = rect_of(&self.hits, &target);
            n.action = Some(NodeAction::Module(Action::Project(action)));
            out.push(n);
        };
        button(&mut out, 301, format!("{} {}", word(lang, "add"), "+"), Target::Add, ProjectAction::AddTask);
        button(&mut out, 302, word(lang, "files").to_string(), Target::ToggleFiles, ProjectAction::ToggleFiles);
        button(&mut out, 303, word(lang, "next_project").to_string(), Target::NextProject, ProjectAction::NextProject);
        button(&mut out, 304, word(lang, "new_project").to_string(), Target::NewProject, ProjectAction::NewProject);
        let mut key = 310;
        match self.view {
            View::Tasks => {
                for t in project.ordered_tasks() {
                    let state = match t.status {
                        Status::Todo => word(lang, "todo"),
                        Status::Doing => word(lang, "doing"),
                        Status::Done => word(lang, "finished"),
                    };
                    button(&mut out, key, format!("{}: {}", t.title, state), Target::Status(t.id.clone()), ProjectAction::CycleStatus(t.id.clone()));
                    button(&mut out, key + 1, format!("{}: {}", word(lang, "delete"), t.title), Target::Delete(t.id.clone()), ProjectAction::DeleteTask(t.id.clone()));
                    key += 2;
                }
            }
            View::Files => {
                for f in &project.files {
                    button(&mut out, key, format!("{}: {}", word(lang, "open"), f.name()), Target::OpenFile(f.id.clone()), ProjectAction::OpenFile(f.id.clone()));
                    button(&mut out, key + 1, format!("{}: {}", word(lang, "remove"), f.name()), Target::RemoveFile(f.id.clone()), ProjectAction::RemoveFile(f.id.clone()));
                    key += 2;
                }
            }
        }
        out
    }
}

impl ProjectsModule {
    /// What a click on a control does.
    fn click(&mut self, target: &Target, env: &Env) -> Vec<Effect> {
        match target {
            Target::Add => self.do_action(&ProjectAction::AddTask, env),
            Target::ToggleFiles => self.do_action(&ProjectAction::ToggleFiles, env),
            Target::NextProject => self.do_action(&ProjectAction::NextProject, env),
            Target::NewProject => self.do_action(&ProjectAction::NewProject, env),
            Target::Status(id) => self.do_action(&ProjectAction::CycleStatus(id.clone()), env),
            Target::Delete(id) => {
                // Two clicks: the first asks, the second deletes.
                if self.confirm.as_deref() == Some(id.as_str()) {
                    self.do_action(&ProjectAction::DeleteTask(id.clone()), env)
                } else {
                    self.confirm = Some(id.clone());
                    vec![Effect::Redraw]
                }
            }
            Target::EditTask(id) => {
                let title = self.project().and_then(|p| p.tasks.into_iter().find(|t| &t.id == id)).map(|t| t.title).unwrap_or_default();
                self.selected = Some(id.clone());
                self.start_edit(EditTarget::Task(id.clone()), &title)
            }
            Target::OpenFile(id) => self.do_action(&ProjectAction::OpenFile(id.clone()), env),
            Target::RevealFile(id) => self.do_action(&ProjectAction::RevealFile(id.clone()), env),
            Target::RemoveFile(id) => {
                if self.confirm.as_deref() == Some(id.as_str()) {
                    self.do_action(&ProjectAction::RemoveFile(id.clone()), env)
                } else {
                    self.confirm = Some(id.clone());
                    vec![Effect::Redraw]
                }
            }
            Target::Field => Vec::new(),
        }
    }
}
