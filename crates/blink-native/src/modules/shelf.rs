//! The shelf: a place to leave files for a moment. Drag files onto the notch and drop them on the shelf, then drag them out again
//! onto another app (Discord, a mail, a folder). Only the paths are kept (in `shelf.json`); nothing is copied, and taking a file off the
//! shelf never touches the file.

use blink_core::i18n::Lang;
use blink_core::store::Store;
use serde_json::{json, Value};

use super::{Action, Effect, Env, Key, KeyResult, Kit, Module, PanelFrame, PeekView, PointerKind, RestView, CONTENT_H};
use crate::access::{Node, NodeAction, Role, ROOT};
use crate::ui::canvas::Icon;
use crate::ui::text::Face;
use crate::ui::theme::Rgb;

const CYAN: Rgb = Rgb(0x38, 0xd6, 0xe8);
const FILE: &str = "shelf.json";
const MAX_ITEMS: usize = 60;
const LEFT_W: f32 = 130.0;
const LEFT_GAP: f32 = 20.0;
const ROW_H: f32 = 34.0;
const ROW_GAP: f32 = 4.0;
const LIST_H: f32 = CONTENT_H - 4.0;
/// How far the pointer moves with the button down before it is a drag and not a click.
const DRAG_START: f32 = 6.0;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ShelfAction {
    Open(String),
    Remove(String),
    Clear,
}

#[derive(Clone, Debug, PartialEq)]
enum Target {
    Item(String),
    Remove(String),
    DragAll,
    Clear,
}

#[derive(Clone, Debug)]
struct Hit {
    x: f32,
    y: f32,
    w: f32,
    h: f32,
    target: Target,
}

pub struct ShelfModule {
    store: Store,
    items: Vec<String>,
    scroll: f32,
    hits: Vec<Hit>,
    pointer: Option<(f32, f32)>,
    /// A button went down on something: where, and on what. It becomes a drag if the pointer moves far enough.
    press: Option<(f32, f32, Target)>,
    /// Take a file off the shelf once it has been dragged out and somebody took it (`autoClear`, on by default).
    auto_clear: bool,
}

fn word(lang: Lang, key: &str) -> &'static str {
    let id = lang == Lang::Id;
    match key {
        "title" => if id { "Rak" } else { "Shelf" },
        "on_shelf" => if id { "di rak" } else { "on the shelf" },
        "drag_all" => if id { "Seret semua" } else { "Drag all" },
        "clear" => if id { "Kosongkan" } else { "Clear" },
        "empty" => if id { "Rak masih kosong" } else { "The shelf is empty" },
        "hint" => if id { "Seret file ke notch, lepas di Rak, lalu seret keluar ke aplikasi lain." } else { "Drag files onto the notch, drop them on the shelf, then drag them out to another app." },
        "open" => if id { "Buka" } else { "Open" },
        "remove" => if id { "Hapus dari rak" } else { "Take off the shelf" },
        "now" => if id { "Rak" } else { "Shelf" },
        _ => "",
    }
}

fn file_name(path: &str) -> String {
    path.trim_end_matches(['\\', '/']).rsplit(['\\', '/']).next().unwrap_or(path).to_string()
}

fn parent_name(path: &str) -> String {
    let trimmed = path.trim_end_matches(['\\', '/']);
    let parent = trimmed.rsplit_once(['\\', '/']).map_or("", |(p, _)| p);
    file_name(parent)
}

#[allow(clippy::too_many_arguments)]
fn put(kit: &mut Kit, face: Face, size: f32, x: f32, box_y: f32, box_h: f32, max_w: f32, text: &str, color: Rgb, alpha: f32) -> f32 {
    let px = size * kit.canvas.scale;
    let cut = kit.fonts.ellipsize(face, text, px, 0.0, max_w.max(8.0) * kit.canvas.scale);
    let base = box_y + kit.fonts.baseline(face, size, box_h);
    kit.canvas.text(kit.fonts, face, size, x, base, &cut, color, alpha, 0.0)
}

#[allow(clippy::too_many_arguments)]
fn put_centered(kit: &mut Kit, face: Face, size: f32, cx: f32, box_y: f32, box_h: f32, max_w: f32, text: &str, color: Rgb, alpha: f32) {
    let px = size * kit.canvas.scale;
    let cut = kit.fonts.ellipsize(face, text, px, 0.0, max_w.max(8.0) * kit.canvas.scale);
    let w = kit.canvas.measure(kit.fonts, face, size, &cut, 0.0);
    let base = box_y + kit.fonts.baseline(face, size, box_h);
    kit.canvas.text(kit.fonts, face, size, cx - w / 2.0, base, &cut, color, alpha, 0.0);
}

impl ShelfModule {
    pub fn new(store: Store) -> ShelfModule {
        let items = store
            .read(FILE)
            .and_then(|b| serde_json::from_slice::<Value>(&b).ok())
            .and_then(|v| v.get("items").and_then(Value::as_array).map(|a| a.iter().filter_map(|i| i.get("path").and_then(Value::as_str).map(str::to_string)).collect::<Vec<_>>()))
            .unwrap_or_default()
            .into_iter()
            // A file that has gone since last time is not on the shelf any more.
            .filter(|p| std::path::Path::new(p).exists())
            .collect();
        ShelfModule { store, items, scroll: 0.0, hits: Vec::new(), pointer: None, press: None, auto_clear: true }
    }

    pub fn items(&self) -> &[String] {
        &self.items
    }

    fn save(&self) {
        let value = json!({ "items": self.items.iter().map(|p| json!({ "path": p })).collect::<Vec<_>>() });
        let _ = self.store.write_atomic(FILE, value.to_string().as_bytes());
    }

    /// Put files on the shelf (ones already there stay where they are; ones that do not exist are skipped).
    pub fn add(&mut self, paths: &[String]) -> usize {
        let mut added = 0;
        for p in paths {
            if !std::path::Path::new(p).exists() || self.items.iter().any(|i| i == p) {
                continue;
            }
            if self.items.len() >= MAX_ITEMS {
                self.items.remove(0);
            }
            self.items.push(p.clone());
            added += 1;
        }
        if added > 0 {
            self.save();
        }
        added
    }

    pub fn remove(&mut self, path: &str) {
        self.items.retain(|i| i != path);
        self.clamp_scroll();
        self.save();
    }

    fn over(&self, x: f32, y: f32, w: f32, h: f32) -> bool {
        self.pointer.is_some_and(|(px, py)| px >= x && px < x + w && py >= y && py < y + h)
    }

    fn clamp_scroll(&mut self) {
        let content = (self.items.len() as f32 * (ROW_H + ROW_GAP) - ROW_GAP).max(0.0);
        self.scroll = self.scroll.clamp(0.0, (content - LIST_H).max(0.0));
    }

    fn do_action(&mut self, action: &ShelfAction) -> Vec<Effect> {
        match action {
            ShelfAction::Open(path) => vec![Effect::OpenPath(path.clone())],
            ShelfAction::Remove(path) => {
                self.remove(path);
                vec![Effect::Redraw]
            }
            ShelfAction::Clear => {
                self.items.clear();
                self.scroll = 0.0;
                self.save();
                vec![Effect::Redraw]
            }
        }
    }

    fn left_column(&mut self, kit: &mut Kit, env: &Env, frame: &PanelFrame) {
        let theme = frame.theme;
        let lang = env.t.lang();
        let x = frame.content_x();
        let y0 = frame.origin_y;
        let n = self.items.len();
        let text = n.to_string();
        let base = y0 + 6.0 + kit.fonts.baseline(Face::SansLight, 44.0, 52.0);
        kit.canvas.text(kit.fonts, Face::SansLight, 44.0, x - 1.0, base, &text, if n > 0 { theme.fg } else { theme.dim }, frame.alpha, 0.0);
        put(kit, Face::Sans, 12.0, x, y0 + 60.0, 16.0, LEFT_W, word(lang, "on_shelf"), theme.dim, frame.alpha);
        if n == 0 {
            return;
        }
        // Two small buttons: carry everything at once, or empty the shelf.
        let by = y0 + CONTENT_H - 26.0;
        let all = word(lang, "drag_all");
        let aw = kit.canvas.measure(kit.fonts, Face::SansSemibold, 12.0, all, 0.0) + 36.0;
        let over = self.over(x, by, aw, 26.0) || self.press.as_ref().is_some_and(|p| p.2 == Target::DragAll);
        kit.canvas.rrect(x, by, aw, 26.0, 13.0, CYAN, frame.alpha * if over { 1.0 } else { 0.92 });
        kit.canvas.icon(Icon::File, x + 14.0, by + 13.0, 11.0, Rgb(0x04, 0x16, 0x1a), frame.alpha);
        put(kit, Face::SansSemibold, 12.0, x + 25.0, by, 26.0, aw - 28.0, all, Rgb(0x04, 0x16, 0x1a), frame.alpha);
        self.hits.push(Hit { x, y: by, w: aw, h: 26.0, target: Target::DragAll });
        let clear = word(lang, "clear");
        let cw = kit.canvas.measure(kit.fonts, Face::Sans, 12.0, clear, 0.0) + 22.0;
        let cx = x + aw + 6.0;
        if cx + cw <= x + LEFT_W + 8.0 {
            let over = self.over(cx, by, cw, 26.0);
            kit.canvas.rrect(cx, by, cw, 26.0, 13.0, theme.fg, frame.alpha * if over { 0.22 } else { 0.1 });
            put(kit, Face::Sans, 12.0, cx + 11.0, by, 26.0, cw - 14.0, clear, theme.fg, frame.alpha);
            self.hits.push(Hit { x: cx, y: by, w: cw, h: 26.0, target: Target::Clear });
        }
    }

    fn list(&mut self, kit: &mut Kit, env: &Env, frame: &PanelFrame) {
        let theme = frame.theme;
        let lang = env.t.lang();
        let lx = frame.content_x() + LEFT_W + LEFT_GAP;
        let lw = frame.content_w() - LEFT_W - LEFT_GAP;
        let top = frame.origin_y;
        if self.items.is_empty() {
            let cx = lx + lw / 2.0;
            kit.canvas.circle(cx, top + 34.0, 18.0, theme.fg, frame.alpha * 0.08);
            kit.canvas.icon(Icon::Download, cx, top + 34.0, 17.0, theme.dim, frame.alpha);
            put_centered(kit, Face::SansBold, 13.0, cx, top + 58.0, 18.0, lw, word(lang, "empty"), theme.fg, frame.alpha);
            put_centered(kit, Face::Sans, 11.0, cx, top + 78.0, 32.0, lw - 20.0, word(lang, "hint"), theme.dim, frame.alpha);
            return;
        }
        self.clamp_scroll();
        kit.canvas.push_clip(lx - 4.0, top, lw + 8.0, LIST_H);
        let items = self.items.clone();
        for (i, path) in items.iter().enumerate() {
            let y = top + i as f32 * (ROW_H + ROW_GAP) - self.scroll;
            if y + ROW_H < top || y > top + LIST_H {
                continue;
            }
            let folder = std::path::Path::new(path).is_dir();
            let hover = self.over(lx, y, lw, ROW_H) && self.pointer.is_some_and(|(_, py)| py < top + LIST_H);
            let pressed = self.press.as_ref().is_some_and(|p| p.2 == Target::Item(path.clone()));
            kit.canvas.rrect(lx, y, lw, ROW_H, 11.0, theme.fg, frame.alpha * if hover || pressed { 0.12 } else { 0.06 });
            let (ix, iy) = (lx + 18.0, y + ROW_H / 2.0);
            kit.canvas.circle(ix, iy, 11.0, CYAN, frame.alpha * 0.18);
            kit.canvas.icon(if folder { Icon::Folder } else { Icon::File }, ix, iy, 12.0, CYAN, frame.alpha);
            let tx = lx + 38.0;
            let right = lx + lw - if hover { 28.0 } else { 10.0 };
            put(kit, Face::SansSemibold, 12.0, tx, y + 3.0, 16.0, right - tx, &file_name(path), theme.fg, frame.alpha);
            put(kit, Face::Sans, 10.5, tx, y + 18.0, 14.0, right - tx, &parent_name(path), theme.dim, frame.alpha);
            // The row first, the × after it: the last hit under the pointer wins, and the × lies on the row.
            self.hits.push(Hit { x: lx, y, w: lw, h: ROW_H, target: Target::Item(path.clone()) });
            if hover {
                let (rx, ry) = (lx + lw - 20.0, y + ROW_H / 2.0 - 8.0);
                let over = self.over(rx - 4.0, ry - 4.0, 24.0, 24.0);
                kit.canvas.icon(Icon::Close, rx + 8.0, ry + 8.0, 9.0, if over { theme.fg } else { theme.dim }, frame.alpha);
                self.hits.push(Hit { x: rx - 4.0, y: ry - 4.0, w: 24.0, h: 24.0, target: Target::Remove(path.clone()) });
            }
        }
        kit.canvas.pop_clip();
    }
}

impl Module for ShelfModule {
    fn id(&self) -> &'static str {
        "shelf"
    }

    fn title(&self, lang: Lang) -> String {
        word(lang, "title").to_string()
    }

    fn title_parts(&self, lang: Lang) -> (String, String) {
        let (a, b) = if lang == Lang::Id { ("R", "ak") } else { ("Sh", "elf") };
        (a.to_string(), b.to_string())
    }

    fn accent(&self) -> Rgb {
        CYAN
    }

    fn as_any_mut(&mut self) -> &mut dyn std::any::Any {
        self
    }

    fn set_options(&mut self, options: &Value) {
        self.auto_clear = options.get("autoClear").and_then(Value::as_bool).unwrap_or(true);
    }

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

    fn panel_height(&mut self, _env: &Env, _kit: &mut Kit) -> f32 {
        CONTENT_H
    }

    fn draw_panel(&mut self, env: &Env, kit: &mut Kit, frame: &PanelFrame) {
        self.hits.clear();
        self.left_column(kit, env, frame);
        self.list(kit, env, frame);
    }

    fn captures_pointer(&self) -> bool {
        self.press.is_some()
    }

    fn pointer(&mut self, kind: PointerKind, x: f32, y: f32, _env: &Env) -> Vec<Effect> {
        match kind {
            PointerKind::Leave => {
                self.pointer = None;
                self.press = None;
                vec![Effect::Redraw]
            }
            PointerKind::Move => {
                self.pointer = Some((x, y));
                // The button is down on a file and the pointer has gone a little way: carry it.
                if let Some((px, py, target)) = self.press.clone() {
                    if ((x - px).powi(2) + (y - py).powi(2)).sqrt() >= DRAG_START {
                        self.press = None;
                        let paths = match target {
                            Target::Item(p) => vec![p],
                            Target::DragAll => self.items.clone(),
                            _ => Vec::new(),
                        };
                        if !paths.is_empty() {
                            return vec![Effect::StartDrag(paths)];
                        }
                    }
                }
                vec![Effect::Redraw]
            }
            PointerKind::Down => {
                let Some(hit) = self.hits.iter().rev().find(|h| x >= h.x && x < h.x + h.w && y >= h.y && y < h.y + h.h).cloned() else { return Vec::new() };
                match hit.target {
                    Target::Remove(path) => self.do_action(&ShelfAction::Remove(path)),
                    Target::Clear => self.do_action(&ShelfAction::Clear),
                    target @ (Target::Item(_) | Target::DragAll) => {
                        self.press = Some((x, y, target));
                        vec![Effect::Redraw]
                    }
                }
            }
            PointerKind::Up => match self.press.take() {
                // Let go without moving: a click, which opens the file.
                Some((_, _, Target::Item(path))) => self.do_action(&ShelfAction::Open(path)),
                _ => vec![Effect::Redraw],
            },
            PointerKind::Wheel(delta) => {
                self.scroll -= delta;
                self.clamp_scroll();
                vec![Effect::Redraw]
            }
        }
    }

    fn key(&mut self, key: Key, _env: &Env) -> KeyResult {
        match key {
            Key::Up | Key::Down => {
                self.scroll += if key == Key::Down { ROW_H } else { -ROW_H };
                self.clamp_scroll();
                KeyResult { handled: true, close: false, effects: vec![Effect::Redraw] }
            }
            _ => KeyResult { handled: false, close: key == Key::Escape, effects: Vec::new() },
        }
    }

    fn keyboard_mode(&mut self, _on: bool, _env: &Env) {}

    fn next_wake(&mut self, _env: &Env, _panel_open: bool) -> Option<u64> {
        None
    }

    /// Files dropped on the shelf's zone.
    fn drop_files(&mut self, paths: &[String]) -> Vec<Effect> {
        let added = self.add(paths);
        let mut effects = vec![Effect::Redraw];
        if added > 0 {
            effects.push(Effect::ShowPeek(PeekView {
                code: String::new(),
                title: if added == 1 { "Added to the shelf".to_string() } else { format!("{added} files added to the shelf") },
                count: String::new(),
                urgency: "later",
                priority: 1,
                ttl_ms: Some(2600),
                key: Some("shelf:add".into()),
                progress: None,
                icon: Some(Icon::Download),
                accent: Some(CYAN),
                cover: None,
            }));
        }
        effects
    }

    /// Carried out and somebody took them: they leave the shelf (the files themselves are theirs now).
    fn drag_finished(&mut self, paths: &[String], accepted: bool) -> Vec<Effect> {
        if accepted && self.auto_clear {
            for p in paths {
                self.items.retain(|i| i != p);
            }
            self.clamp_scroll();
            self.save();
        }
        vec![Effect::Redraw]
    }

    fn accessibility(&mut self, env: &Env, open: bool) -> Vec<Node> {
        let lang = env.t.lang();
        if !open {
            return Vec::new();
        }
        let mut out = vec![Node::new(600, ROOT, Role::Text, format!("{}: {}", word(lang, "now"), self.items.len()))];
        for (i, path) in self.items.iter().enumerate() {
            let key = 610 + i as i32 * 3;
            out.push(Node::new(key, ROOT, Role::Text, file_name(path)));
            let mut open_button = Node::new(key + 1, ROOT, Role::Button, format!("{} {}", word(lang, "open"), file_name(path)));
            open_button.action = Some(NodeAction::Module(Action::Shelf(ShelfAction::Open(path.clone()))));
            out.push(open_button);
            let mut remove = Node::new(key + 2, ROOT, Role::Button, format!("{} {}", word(lang, "remove"), file_name(path)));
            remove.action = Some(NodeAction::Module(Action::Shelf(ShelfAction::Remove(path.clone()))));
            out.push(remove);
        }
        if !self.items.is_empty() {
            let mut clear = Node::new(599, ROOT, Role::Button, word(lang, "clear").to_string());
            clear.action = Some(NodeAction::Module(Action::Shelf(ShelfAction::Clear)));
            out.push(clear);
        }
        out
    }

    fn perform(&mut self, action: Action, _env: &Env) -> Vec<Effect> {
        match action {
            Action::Shelf(a) => self.do_action(&a),
            _ => Vec::new(),
        }
    }

    fn debug_now(&mut self, _env: &Env) -> Value {
        json!({
            "items": self.items,
            "autoClear": self.auto_clear,
            "hits": self.hits.iter().map(|h| json!({ "x": h.x, "y": h.y, "w": h.w, "h": h.h, "target": format!("{:?}", h.target) })).collect::<Vec<_>>(),
        })
    }
}
