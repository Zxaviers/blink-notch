//! Agents: what AI agents (Claude Code, a script, anything) say about themselves over the notch's pipe.
//!
//! A program sends `blinkctl agent --id x --state running --title "..."` (see `docs/AGENTS.md`); the page lists the agents with
//! what each is doing, a peek says when one needs the person or finishes, and the resting pill shows one that is waiting.
//! Nothing is stored on disk: agents come and go with the notch.

use blink_core::agents::{ago, Agent, AgentState, Agents, Change, Update};
use blink_core::i18n::Lang;
use serde_json::{json, Value};

use super::{Action, Effect, Env, Key, KeyResult, Kit, Module, PanelFrame, PeekView, PointerKind, RestView, CONTENT_H};
use crate::access::{Node, NodeAction, Role, ROOT};
use crate::ui::canvas::Icon;
use crate::ui::text::Face;
use crate::ui::theme::Rgb;

const TEAL: Rgb = Rgb(0x2d, 0xd4, 0xbf);
const BLUE: Rgb = Rgb(0x4e, 0xa8, 0xff);
const AMBER: Rgb = Rgb(0xfb, 0xbf, 0x24);
const GREEN: Rgb = Rgb(0x34, 0xd3, 0x99);
const RED: Rgb = Rgb(0xf8, 0x71, 0x71);

const LEFT_W: f32 = 150.0;
const LEFT_GAP: f32 = 20.0;
const ROW_H: f32 = 36.0;
const ROW_GAP: f32 = 4.0;
const LIST_H: f32 = CONTENT_H - 4.0;

/// What a click or a screen reader can ask of the page.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum AgentAction {
    /// Open the agent's link (a run page, a pull request).
    Open(String),
    Remove(String),
    ClearFinished,
}

#[derive(Clone, Debug, PartialEq)]
enum Target {
    Row(String),
    Remove(String),
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

pub struct AgentsModule {
    agents: Agents,
    /// Show a peek when an agent needs the person, finishes or fails (`peek` option, on by default).
    peek_on_change: bool,
    /// Accept updates at all (`enabled` option under the module; the page can also just be turned off in the pages list).
    accept: bool,
    scroll: f32,
    hits: Vec<Hit>,
    pointer: Option<(f32, f32)>,
}

impl Default for AgentsModule {
    fn default() -> Self {
        AgentsModule { agents: Agents::new(), peek_on_change: true, accept: true, scroll: 0.0, hits: Vec::new(), pointer: None }
    }
}

fn word(lang: Lang, key: &str) -> &'static str {
    let id = lang == Lang::Id;
    match key {
        "title" => if id { "Agen" } else { "Agents" },
        "running" => if id { "berjalan" } else { "running" },
        "needs_you" => if id { "butuh kamu" } else { "need you" },
        "done" => if id { "selesai" } else { "done" },
        "failed" => if id { "gagal" } else { "failed" },
        "idle" => if id { "diam" } else { "idle" },
        "clear" => if id { "Bersihkan selesai" } else { "Clear finished" },
        "none" => if id { "Belum ada agen" } else { "No agents yet" },
        "hint" => if id { "Agen AI melapor lewat blinkctl:" } else { "AI agents report through blinkctl:" },
        "example" => "blinkctl agent --id a1 --state running",
        "state_running" => if id { "Berjalan" } else { "Running" },
        "state_input" => if id { "Butuh jawabanmu" } else { "Needs your input" },
        "state_done" => if id { "Selesai" } else { "Finished" },
        "state_error" => if id { "Gagal" } else { "Failed" },
        "state_idle" => if id { "Diam" } else { "Idle" },
        "open" => if id { "Buka" } else { "Open" },
        "remove" => if id { "Hapus" } else { "Remove" },
        "now" => if id { "Agen" } else { "Agent" },
        _ => "",
    }
}

fn state_word(lang: Lang, s: AgentState) -> &'static str {
    word(
        lang,
        match s {
            AgentState::Running => "state_running",
            AgentState::Input => "state_input",
            AgentState::Done => "state_done",
            AgentState::Error => "state_error",
            AgentState::Idle => "state_idle",
        },
    )
}

fn state_color(s: AgentState, dim: Rgb) -> Rgb {
    match s {
        AgentState::Running => BLUE,
        AgentState::Input => AMBER,
        AgentState::Done => GREEN,
        AgentState::Error => RED,
        AgentState::Idle => dim,
    }
}

fn state_icon(s: AgentState) -> Icon {
    match s {
        AgentState::Running => Icon::Play,
        AgentState::Input => Icon::Message,
        AgentState::Done => Icon::Check,
        AgentState::Error => Icon::Alarm,
        AgentState::Idle => Icon::Pause,
    }
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

impl AgentsModule {
    pub fn new() -> AgentsModule {
        AgentsModule::default()
    }

    pub fn agents(&self) -> &Agents {
        &self.agents
    }

    /// An update from a program: apply it and say whether it is worth a peek.
    pub fn apply_update(&mut self, v: &Value, wall_ms: i64) -> Result<Vec<Effect>, String> {
        if !self.accept {
            return Err("AGENTS_OFF".to_string());
        }
        let update = Update::parse(v)?;
        let id = update.id.clone();
        let change = self.agents.apply(update, wall_ms);
        let Some(agent) = self.agents.get(&id) else { return Ok(Vec::new()) };
        let worth = match change {
            Change::New => !matches!(agent.state, AgentState::Running | AgentState::Idle),
            Change::State(s) => matches!(s, AgentState::Input | AgentState::Done | AgentState::Error),
            Change::Same => false,
        };
        let mut effects = vec![Effect::Redraw];
        if worth && self.peek_on_change {
            effects.push(Effect::ShowPeek(Self::peek_for(agent)));
        }
        Ok(effects)
    }

    fn peek_for(a: &Agent) -> PeekView {
        let urgent = matches!(a.state, AgentState::Input | AgentState::Error);
        let say = if !a.title.is_empty() { a.title.clone() } else { state_word(Lang::En, a.state).to_string() };
        PeekView {
            code: a.name.clone(),
            title: say,
            count: String::new(),
            urgency: if a.state == AgentState::Error { "late" } else if urgent { "soon" } else { "later" },
            priority: if urgent { 3 } else { 2 },
            ttl_ms: Some(if urgent { 9_000 } else { 6_000 }),
            key: Some(format!("agent:{}", a.id)),
            progress: None,
            icon: Some(state_icon(a.state)),
            accent: Some(state_color(a.state, TEAL)),
            cover: None,
        }
    }

    pub fn remove(&mut self, id: &str) -> bool {
        self.agents.remove(id)
    }

    /// The agents as the pipe reports them.
    pub fn list_json(&self, wall_ms: i64) -> Value {
        Value::Array(
            self.agents
                .sorted()
                .iter()
                .map(|a| {
                    json!({ "id": a.id, "name": a.name, "state": a.state.word(), "title": a.title, "detail": a.detail, "progress": a.progress, "url": a.url, "sinceMs": a.since_ms, "updatedMs": a.updated_ms, "ageS": (wall_ms - a.updated_ms).max(0) / 1000 })
                })
                .collect(),
        )
    }

    fn over(&self, x: f32, y: f32, w: f32, h: f32) -> bool {
        self.pointer.is_some_and(|(px, py)| px >= x && px < x + w && py >= y && py < y + h)
    }

    fn content_h(&self) -> f32 {
        let n = self.agents.len() as f32;
        (n * (ROW_H + ROW_GAP) - ROW_GAP).max(0.0)
    }

    fn clamp_scroll(&mut self) {
        self.scroll = self.scroll.clamp(0.0, (self.content_h() - LIST_H).max(0.0));
    }

    fn do_action(&mut self, action: &AgentAction) -> Vec<Effect> {
        match action {
            AgentAction::Open(id) => match self.agents.get(id).and_then(|a| a.url.clone()) {
                Some(url) => vec![Effect::OpenUrl(url)],
                None => Vec::new(),
            },
            AgentAction::Remove(id) => {
                self.agents.remove(id);
                self.clamp_scroll();
                vec![Effect::Redraw]
            }
            AgentAction::ClearFinished => {
                self.agents.clear_finished();
                self.clamp_scroll();
                vec![Effect::Redraw]
            }
        }
    }

    fn left_column(&mut self, kit: &mut Kit, env: &Env, frame: &PanelFrame) {
        let theme = frame.theme;
        let lang = env.t.lang();
        let x = frame.content_x();
        let y0 = frame.origin_y;
        let counts = self.agents.counts();
        let n = |k: &str| counts.get(k).copied().unwrap_or(0);
        let waiting = n("input") + n("error");

        // The number that matters: how many want the person, else how many are running.
        let (big, label, color) = if waiting > 0 {
            (waiting, word(lang, "needs_you"), AMBER)
        } else {
            (n("running"), word(lang, "running"), BLUE)
        };
        let text = big.to_string();
        let base = y0 + 6.0 + kit.fonts.baseline(Face::SansLight, 44.0, 52.0);
        kit.canvas.text(kit.fonts, Face::SansLight, 44.0, x - 1.0, base, &text, if big > 0 { color } else { theme.dim }, frame.alpha, 0.0);
        put(kit, Face::Sans, 12.0, x, y0 + 60.0, 16.0, LEFT_W, label, theme.dim, frame.alpha);
        // The rest, in one quiet line.
        let mut rest = Vec::new();
        if waiting > 0 && n("running") > 0 {
            rest.push(format!("{} {}", n("running"), word(lang, "running")));
        }
        if n("done") > 0 {
            rest.push(format!("{} {}", n("done"), word(lang, "done")));
        }
        if n("idle") > 0 {
            rest.push(format!("{} {}", n("idle"), word(lang, "idle")));
        }
        if !rest.is_empty() {
            put(kit, Face::Sans, 10.5, x, y0 + 78.0, 14.0, LEFT_W, &rest.join(" · "), theme.dim, frame.alpha * 0.9);
        }

        // Clear what is finished.
        if n("done") + n("error") > 0 {
            let by = y0 + CONTENT_H - 26.0;
            let label = word(lang, "clear");
            let w = kit.canvas.measure(kit.fonts, Face::Sans, 12.0, label, 0.0) + 24.0;
            let over = self.over(x, by, w, 26.0);
            kit.canvas.rrect(x, by, w, 26.0, 13.0, theme.fg, frame.alpha * if over { 0.22 } else { 0.1 });
            put(kit, Face::Sans, 12.0, x + 12.0, by, 26.0, w - 16.0, label, theme.fg, frame.alpha);
            self.hits.push(Hit { x, y: by, w, h: 26.0, target: Target::Clear });
        }
    }

    fn list(&mut self, kit: &mut Kit, env: &Env, frame: &PanelFrame) {
        let theme = frame.theme;
        let lang = env.t.lang();
        let lx = frame.content_x() + LEFT_W + LEFT_GAP;
        let lw = frame.content_w() - LEFT_W - LEFT_GAP;
        let top = frame.origin_y;
        if self.agents.is_empty() {
            let cx = lx + lw / 2.0;
            kit.canvas.circle(cx, top + 34.0, 18.0, theme.fg, frame.alpha * 0.08);
            kit.canvas.icon(Icon::Play, cx + 1.0, top + 34.0, 16.0, theme.dim, frame.alpha);
            put_centered(kit, Face::SansBold, 13.0, cx, top + 58.0, 18.0, lw, word(lang, "none"), theme.fg, frame.alpha);
            put_centered(kit, Face::Sans, 11.0, cx, top + 78.0, 16.0, lw, word(lang, "hint"), theme.dim, frame.alpha);
            put_centered(kit, Face::Mono, 10.5, cx, top + 96.0, 16.0, lw, word(lang, "example"), theme.dim, frame.alpha);
            return;
        }
        self.clamp_scroll();
        kit.canvas.push_clip(lx - 4.0, top, lw + 8.0, LIST_H);
        let agents: Vec<Agent> = self.agents.sorted().into_iter().cloned().collect();
        for (i, a) in agents.iter().enumerate() {
            let y = top + i as f32 * (ROW_H + ROW_GAP) - self.scroll;
            if y + ROW_H < top || y > top + LIST_H {
                continue;
            }
            let color = state_color(a.state, theme.dim);
            let hover = self.over(lx, y, lw, ROW_H) && self.pointer.is_some_and(|(_, py)| py < top + LIST_H);
            kit.canvas.rrect(lx, y, lw, ROW_H, 11.0, theme.fg, frame.alpha * if hover { 0.1 } else { 0.05 });
            if matches!(a.state, AgentState::Input | AgentState::Error) {
                kit.canvas.rrect(lx, y, lw, ROW_H, 11.0, color, frame.alpha * 0.12);
            }
            // The state, as a small coloured disc.
            let (ix, iy) = (lx + 18.0, y + ROW_H / 2.0);
            kit.canvas.circle(ix, iy, 11.0, color, frame.alpha * 0.2);
            kit.canvas.icon(state_icon(a.state), ix + if a.state == AgentState::Running { 0.8 } else { 0.0 }, iy, 11.0, color, frame.alpha);
            // The words: the name and, under it, what it is doing.
            let tx = lx + 36.0;
            let right = lx + lw - 10.0;
            let age = ago(a.since_ms, env.wall_ms);
            let age_w = kit.canvas.measure(kit.fonts, Face::Sans, 10.5, &age, 0.0);
            let remove_w = if hover { 18.0 } else { 0.0 };
            put(kit, Face::SansBold, 12.0, tx, y + 3.0, 16.0, right - tx - age_w - remove_w - 10.0, &a.name, theme.fg, frame.alpha);
            put(kit, Face::Sans, 10.5, right - age_w - remove_w, y + 4.0, 14.0, age_w + 2.0, &age, theme.dim, frame.alpha);
            let what = if !a.detail.is_empty() {
                a.detail.clone()
            } else if !a.title.is_empty() {
                a.title.clone()
            } else {
                state_word(lang, a.state).to_string()
            };
            put(kit, Face::Sans, 10.5, tx, y + 19.0, 14.0, right - tx - remove_w, &what, theme.dim, frame.alpha);
            // How far along, as a thin line along the bottom of the row.
            if let Some(p) = a.progress {
                kit.canvas.bar(tx, right, y + ROW_H - 3.0, 2.0, theme.fg, frame.alpha * 0.14);
                kit.canvas.bar(tx, tx + (right - tx) * p.clamp(0.02, 1.0), y + ROW_H - 3.0, 2.0, color, frame.alpha);
            }
            if hover {
                let (rx, ry) = (lx + lw - 16.0, y + 11.0);
                let over = self.over(rx - 7.0, ry - 7.0, 18.0, 18.0);
                kit.canvas.icon(Icon::Close, rx + 2.0, ry + 2.0, 9.0, if over { theme.fg } else { theme.dim }, frame.alpha);
                self.hits.push(Hit { x: rx - 7.0, y: ry - 7.0, w: 18.0, h: 18.0, target: Target::Remove(a.id.clone()) });
            }
            self.hits.push(Hit { x: lx, y, w: lw, h: ROW_H, target: Target::Row(a.id.clone()) });
        }
        kit.canvas.pop_clip();
    }
}

impl Module for AgentsModule {
    fn id(&self) -> &'static str {
        "agents"
    }

    fn title(&self, lang: Lang) -> String {
        word(lang, "title").to_string()
    }

    fn title_parts(&self, lang: Lang) -> (String, String) {
        let (a, b) = if lang == Lang::Id { ("Ag", "en") } else { ("Ag", "ents") };
        (a.to_string(), b.to_string())
    }

    fn accent(&self) -> Rgb {
        TEAL
    }

    /// Off until the person connects an app (Blink settings, Agents does that and switches it on): most people never use it.
    fn default_enabled(&self) -> bool {
        false
    }

    fn as_any_mut(&mut self) -> &mut dyn std::any::Any {
        self
    }

    fn set_options(&mut self, options: &Value) {
        self.peek_on_change = options.get("peek").and_then(Value::as_bool).unwrap_or(true);
        self.accept = options.get("accept").and_then(Value::as_bool).unwrap_or(true);
    }

    /// An agent waiting for the person (or one that failed) deserves the pill; otherwise the clock keeps it.
    fn rest_priority(&mut self, _env: &Env, _settings: &blink_core::settings::Settings) -> u8 {
        if self.agents.needs_you().is_some() { 2 } else { 0 }
    }

    fn rest_style(&mut self, _env: &Env, _resting_style: &str, _due_horizon_hours: f64) -> crate::shell::RestStyle {
        crate::shell::RestStyle::Full
    }

    fn rest(&mut self, env: &Env) -> RestView {
        match self.agents.needs_you() {
            Some(a) => RestView {
                label: a.name.chars().take(14).collect(),
                count: if a.state == AgentState::Error { word(env.t.lang(), "failed") } else { word(env.t.lang(), "needs_you") }.to_string(),
                urgency: if a.state == AgentState::Error { "overdue" } else { "soon" },
            },
            None => RestView { label: String::new(), count: String::new(), urgency: "later" },
        }
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
                let Some(hit) = self.hits.iter().rev().find(|h| x >= h.x && x < h.x + h.w && y >= h.y && y < h.y + h.h).cloned() else { return Vec::new() };
                match hit.target {
                    Target::Row(id) => self.do_action(&AgentAction::Open(id)),
                    Target::Remove(id) => self.do_action(&AgentAction::Remove(id)),
                    Target::Clear => self.do_action(&AgentAction::ClearFinished),
                }
            }
            PointerKind::Up => Vec::new(),
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

    /// Forget agents that went quiet long ago.
    fn tick(&mut self, env: &Env) -> Vec<Effect> {
        if self.agents.forget_old(env.wall_ms) {
            self.clamp_scroll();
            return vec![Effect::Redraw];
        }
        Vec::new()
    }

    /// With the page open, the ages in the rows change once a minute.
    fn next_wake(&mut self, env: &Env, panel_open: bool) -> Option<u64> {
        if !panel_open || self.agents.is_empty() {
            return None;
        }
        let into_minute = env.wall_ms.rem_euclid(60_000) as u64;
        Some(env.now_ms + 60_000 - into_minute + 1)
    }

    fn accessibility(&mut self, env: &Env, open: bool) -> Vec<Node> {
        let lang = env.t.lang();
        if !open {
            return Vec::new();
        }
        let mut out = vec![Node::new(500, ROOT, Role::Text, format!("{}: {}", word(lang, "now"), self.agents.len()))];
        for (i, a) in self.agents.sorted().into_iter().enumerate() {
            let key = 510 + i as i32 * 3;
            let what = if a.title.is_empty() { String::new() } else { format!(", {}", a.title) };
            out.push(Node::new(key, ROOT, Role::Text, format!("{}: {}{}", a.name, state_word(lang, a.state), what)));
            if a.url.is_some() {
                let mut open_button = Node::new(key + 1, ROOT, Role::Button, format!("{} {}", word(lang, "open"), a.name));
                open_button.action = Some(NodeAction::Module(Action::Agent(AgentAction::Open(a.id.clone()))));
                out.push(open_button);
            }
            let mut remove = Node::new(key + 2, ROOT, Role::Button, format!("{} {}", word(lang, "remove"), a.name));
            remove.action = Some(NodeAction::Module(Action::Agent(AgentAction::Remove(a.id.clone()))));
            out.push(remove);
        }
        if self.agents.counts().get("done").copied().unwrap_or(0) + self.agents.counts().get("error").copied().unwrap_or(0) > 0 {
            let mut clear = Node::new(499, ROOT, Role::Button, word(lang, "clear").to_string());
            clear.action = Some(NodeAction::Module(Action::Agent(AgentAction::ClearFinished)));
            out.push(clear);
        }
        out
    }

    fn perform(&mut self, action: Action, _env: &Env) -> Vec<Effect> {
        match action {
            Action::Agent(a) => self.do_action(&a),
            _ => Vec::new(),
        }
    }

    fn debug_now(&mut self, env: &Env) -> Value {
        json!({
            "agents": self.list_json(env.wall_ms),
            "acceptUpdates": self.accept,
            "hits": self.hits.iter().map(|h| json!({ "x": h.x, "y": h.y, "w": h.w, "h": h.h, "target": format!("{:?}", h.target) })).collect::<Vec<_>>(),
        })
    }
}
