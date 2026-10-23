//! The app: shell state machine + modules + canvas, driven by inputs and time.
//! No Windows here, so the whole behavior is testable with a manual clock;
//! the window layer only feeds it events and presents its pixels.

use blink_core::i18n::{resolve_language, Lang, Translator};
use blink_core::settings::{normalize_settings, Settings};
use jiff::tz::TimeZone;
use serde_json::{json, Value};

use crate::clock::Clock;
use crate::modules::{Effect, Env, Key, Kit, Module, PanelFrame, PeekView, PointerKind, RestView};
use crate::shell::{Mode, RestStyle, Shape, Shell, ShellConfig, OPEN_MAX_H, OPEN_W, REST_FULL, WINDOW_H, WINDOW_W};
use crate::ui::canvas::Canvas;
use crate::ui::text::{Face, Fonts};
use crate::ui::theme::{panel_surface, Rgb, Surface, Theme};

/// What the window layer must do on the app's behalf.
#[derive(Clone, Debug, PartialEq)]
pub enum AppEffect {
    OpenUrl(String),
    OpenSettings,
    RefreshFeeds,
    /// Take (true) or give back (false) real keyboard focus.
    KeyboardFocus(bool),
    /// Something for a screen reader to say (only matters while one is running).
    Announce(String),
    /// A system notification; `url` is what clicking it opens.
    Toast { title: String, body: String, url: Option<String> },
}

pub struct App {
    pub clock: Clock,
    shell: Shell,
    settings: Settings,
    tz: TimeZone,
    lang: Lang,
    windows_light: bool,
    canvas: Canvas,
    fonts: Fonts,
    modules: Vec<Box<dyn Module>>,
    active: usize,
    peek: Option<PeekView>,
    pointer: Option<(f32, f32)>,
    effects: Vec<AppEffect>,
    /// The last frame's panel height, to retarget the shell when content changes.
    last_panel_h: f32,
}

fn lerp_rgb(a: Rgb, b: Rgb, t: f32) -> Rgb {
    let m = |x: u8, y: u8| (f32::from(x) + (f32::from(y) - f32::from(x)) * t).round() as u8;
    Rgb(m(a.0, b.0), m(a.1, b.1), m(a.2, b.2))
}

impl App {
    /// `scale` is the display scale (1.0 = 96 dpi); the canvas is `WINDOW_W x WINDOW_H` logical px.
    pub fn new(clock: Clock, settings: Settings, tz: TimeZone, scale: f32, system_language: &str, modules: Vec<Box<dyn Module>>) -> Result<App, String> {
        let fonts = Fonts::load()?;
        let lang = resolve_language(&settings.language, system_language);
        let cfg = ShellConfig { hover_delay_ms: settings.hover_delay_ms as u64, ..ShellConfig::default() };
        let (w, h) = ((WINDOW_W * scale).round() as u32, (WINDOW_H * scale).round() as u32);
        let mut modules = modules;
        for m in modules.iter_mut() {
            m.configure(&settings);
        }
        Ok(App {
            clock,
            shell: Shell::new(cfg),
            settings,
            tz,
            lang,
            windows_light: false,
            canvas: Canvas::new(w, h, scale),
            fonts,
            modules,
            active: 0,
            peek: None,
            pointer: None,
            effects: Vec::new(),
            last_panel_h: 0.0,
        })
    }

    // ------------------------------------------------------------ accessors

    pub fn shell(&self) -> &Shell {
        &self.shell
    }

    pub fn settings(&self) -> &Settings {
        &self.settings
    }

    pub fn canvas(&self) -> &Canvas {
        &self.canvas
    }

    pub fn scale(&self) -> f32 {
        self.canvas.scale
    }

    pub fn module_mut<T: 'static>(&mut self) -> Option<&mut T> {
        self.modules.get_mut(self.active)?.as_any_mut().downcast_mut::<T>()
    }

    /// Take the effects produced since the last call.
    pub fn take_effects(&mut self) -> Vec<AppEffect> {
        std::mem::take(&mut self.effects)
    }

    pub fn translator(&self) -> Translator {
        Translator::new(self.lang)
    }

    pub fn now(&self) -> u64 {
        self.clock.now_ms()
    }

    fn env<'a>(&self, tz: &'a TimeZone) -> Env<'a> {
        Env { now_ms: self.clock.now_ms(), wall_ms: self.clock.wall_ms(), tz, t: Translator::new(self.lang) }
    }

    // --------------------------------------------------------------- config

    pub fn apply_settings(&mut self, settings: Settings, system_language: &str) {
        self.lang = resolve_language(&settings.language, system_language);
        let mut cfg = *self.shell.config();
        cfg.hover_delay_ms = settings.hover_delay_ms as u64;
        self.shell.set_config(cfg);
        self.settings = settings;
        for m in self.modules.iter_mut() {
            m.configure(&self.settings);
        }
        self.update_rest_style();
        self.run_module_tick();
    }

    pub fn set_reduced_motion(&mut self, on: bool) {
        let mut cfg = *self.shell.config();
        cfg.reduced_motion = on;
        self.shell.set_config(cfg);
    }

    pub fn set_windows_light_theme(&mut self, light: bool) {
        self.windows_light = light;
    }

    /// The display scale changed (monitor with a different DPI).
    pub fn set_scale(&mut self, scale: f32) {
        let (w, h) = ((WINDOW_W * scale).round() as u32, (WINDOW_H * scale).round() as u32);
        self.canvas = Canvas::new(w, h, scale);
    }

    pub fn pin_open(&mut self) {
        let now = self.now();
        self.refresh_panel_height();
        self.shell.pin_open(now);
    }

    // --------------------------------------------------------------- panel

    fn refresh_panel_height(&mut self) {
        let tz = self.tz.clone();
        let env = self.env(&tz);
        let mut kit = Kit { canvas: &mut self.canvas, fonts: &mut self.fonts };
        let h = self.modules[self.active].panel_height(&env, &mut kit);
        self.last_panel_h = h;
        let now = self.clock.now_ms();
        self.shell.set_open_height(h, now);
    }

    fn update_rest_style(&mut self) {
        let tz = self.tz.clone();
        let env = self.env(&tz);
        let style = self.modules[self.active].rest_style(&env, &self.settings.resting_style, self.settings.due_horizon_hours as f64);
        self.shell.set_rest_style(style, env.now_ms);
    }

    fn apply_module_effects(&mut self, effects: Vec<Effect>) {
        let mut want_peek = false;
        for e in effects {
            match e {
                Effect::OpenUrl(u) => self.effects.push(AppEffect::OpenUrl(u)),
                Effect::OpenSettings => self.effects.push(AppEffect::OpenSettings),
                Effect::RefreshFeeds => self.effects.push(AppEffect::RefreshFeeds),
                Effect::Redraw => {}
                Effect::Announce(text) => self.effects.push(AppEffect::Announce(text)),
                Effect::Toast { title, body, url } => {
                    if self.settings.toast {
                        self.effects.push(AppEffect::Announce(format!("{title}. {body}")));
                        self.effects.push(AppEffect::Toast { title, body, url });
                    }
                }
                Effect::Peek => want_peek = true,
            }
        }
        if self.shell.mode() == Mode::Open {
            self.refresh_panel_height();
        }
        // Marking something done can change what the resting pill shows.
        self.update_rest_style();
        if want_peek {
            self.peek();
        }
    }

    // ---------------------------------------------------------------- input

    /// A pointer event in window coordinates (logical px). Moves outside the
    /// visible shape count as leaving it, exactly as Windows reports a layered
    /// window whose pixels are transparent there.
    pub fn pointer(&mut self, kind: PointerKind, x: f32, y: f32) {
        let now = self.clock.now_ms();
        let shape = self.shell.shape(now);
        let sx = (WINDOW_W - shape.w) / 2.0;
        let inside = x >= sx && x < sx + shape.w && y >= 0.0 && y < shape.h;
        let tz = self.tz.clone();

        match kind {
            PointerKind::Leave => {
                self.pointer = None;
                self.shell.pointer_leave(now);
                let env = self.env(&tz);
                let fx = self.modules[self.active].pointer(PointerKind::Leave, x, y, &env);
                self.apply_module_effects(fx);
            }
            PointerKind::Move => {
                if !inside {
                    if self.pointer.is_some() {
                        self.pointer = None;
                        self.shell.pointer_leave(now);
                        let env = self.env(&tz);
                        let fx = self.modules[self.active].pointer(PointerKind::Leave, x, y, &env);
                        self.apply_module_effects(fx);
                    }
                    return;
                }
                if self.pointer.is_none() {
                    self.shell.pointer_enter(now);
                }
                self.pointer = Some((x, y));
                if self.shell.mode() == Mode::Open {
                    let env = self.env(&tz);
                    let fx = self.modules[self.active].pointer(PointerKind::Move, x, y, &env);
                    self.apply_module_effects(fx);
                }
            }
            PointerKind::Down => {
                if !inside {
                    return;
                }
                if self.shell.mode() == Mode::Open {
                    let env = self.env(&tz);
                    let fx = self.modules[self.active].pointer(PointerKind::Down, x, y, &env);
                    self.apply_module_effects(fx);
                } else {
                    self.refresh_panel_height();
                    self.shell.click(now);
                }
            }
            PointerKind::Wheel(d) => {
                if inside && self.shell.mode() == Mode::Open {
                    let env = self.env(&tz);
                    let fx = self.modules[self.active].pointer(PointerKind::Wheel(d), x, y, &env);
                    self.apply_module_effects(fx);
                }
            }
        }
        self.sync_open_height();
    }

    /// Opening starts only once the panel height is known.
    fn sync_open_height(&mut self) {
        if self.shell.hovering() && self.shell.mode() != Mode::Open {
            self.refresh_panel_height();
        }
    }

    pub fn key(&mut self, key: Key) {
        let now = self.clock.now_ms();
        if self.shell.mode() != Mode::Open {
            return;
        }
        let tz = self.tz.clone();
        let env = self.env(&tz);
        let res = self.modules[self.active].key(key, &env);
        if res.close || (!res.handled && key == Key::Escape) {
            self.close_panel(now);
        }
        self.apply_module_effects(res.effects);
    }

    fn close_panel(&mut self, now: u64) {
        if self.shell.keyboard() {
            self.effects.push(AppEffect::KeyboardFocus(false));
            let tz = self.tz.clone();
            let env = self.env(&tz);
            self.modules[self.active].keyboard_mode(false, &env);
        }
        self.shell.close(now);
    }

    /// The global hotkey: open and take keyboard focus, or close.
    pub fn hotkey(&mut self) {
        let now = self.clock.now_ms();
        if self.shell.keyboard() {
            self.close_panel(now);
            return;
        }
        self.refresh_panel_height();
        let tz = self.tz.clone();
        let env = self.env(&tz);
        self.modules[self.active].keyboard_mode(true, &env);
        self.shell.enter_keyboard(now);
        self.effects.push(AppEffect::KeyboardFocus(true));
    }

    /// Windows took keyboard focus away (another app was activated): leave keyboard mode.
    pub fn focus_lost(&mut self) {
        if self.shell.keyboard() {
            let now = self.clock.now_ms();
            // Focus is already gone; do not ask for it to be handed back.
            let tz = self.tz.clone();
            let env = self.env(&tz);
            self.modules[self.active].keyboard_mode(false, &env);
            self.shell.close(now);
        }
    }

    /// Show a peek for something the module wants announced.
    pub fn peek(&mut self) {
        let tz = self.tz.clone();
        let env = self.env(&tz);
        if let Some(p) = self.modules[self.active].peek(&env) {
            self.peek = Some(p);
            self.shell.peek(env.now_ms);
        }
    }

    // ----------------------------------------------------------------- time

    /// Run whatever is due. Returns true if anything visible may have changed.
    pub fn tick(&mut self) -> bool {
        let now = self.clock.now_ms();
        self.run_module_tick();
        self.update_rest_style();
        let changed = self.shell.tick(now);
        if changed && self.shell.mode() == Mode::Open {
            self.refresh_panel_height();
        }
        changed
    }

    /// Let the active module sync and announce what is due.
    fn run_module_tick(&mut self) {
        let tz = self.tz.clone();
        let env = self.env(&tz);
        let effects = self.modules[self.active].tick(&env);
        if !effects.is_empty() {
            self.apply_module_effects(effects);
        }
    }

    /// New data arrived from a worker thread: redraw and re-run reminders.
    pub fn data_changed(&mut self) {
        self.run_module_tick();
        self.update_rest_style();
        if self.shell.mode() == Mode::Open {
            self.refresh_panel_height();
        }
    }

    /// The tree a screen reader sees right now: the notch, and what the active module shows in it.
    pub fn accessibility(&mut self) -> crate::access::Tree {
        use crate::access::{Node, Role, Tree, ROOT};
        let now = self.clock.now_ms();
        let shape = self.shell.shape(now);
        let open = self.shell.mode() == Mode::Open;
        let tz = self.tz.clone();
        let env = self.env(&tz);
        let mut nodes = self.modules[self.active].accessibility(&env, open);
        let rect = ((WINDOW_W - shape.w) / 2.0, 0.0, shape.w, shape.h);
        let mut root = Node::new(ROOT, ROOT, Role::Pane, crate::access::word(env.t.lang(), "notch"));
        root.rect = rect;
        root.focused = self.shell.keyboard() && !nodes.iter().any(|n| n.focused);
        // Elements without a place of their own (the resting pill) take the notch's.
        for n in nodes.iter_mut().filter(|n| n.rect.2 <= 0.0 && !n.offscreen && n.role == Role::Button && n.parent == ROOT && !open) {
            n.rect = rect;
        }
        let mut all = vec![root];
        all.extend(nodes);
        Tree { nodes: all }
    }

    /// A screen reader activated an element (by key from the last tree it saw).
    pub fn accessible_invoke(&mut self, key: i32) {
        use crate::access::NodeAction;
        let tree = self.accessibility();
        let Some(action) = tree.find(key).and_then(|n| n.action.clone()) else { return };
        match action {
            NodeAction::OpenPanel => self.hotkey(),
            NodeAction::Module(a) => {
                let tz = self.tz.clone();
                let env = self.env(&tz);
                let effects = self.modules[self.active].perform(a, &env);
                self.apply_module_effects(effects);
            }
        }
    }

    /// Fetch the feeds now (tray "Refresh now", footer button).
    pub fn refresh_sources(&mut self) {
        if let Some(m) = self.modules.get_mut(self.active) {
            m.refresh();
        }
        self.run_module_tick();
    }

    /// The personal events file changed.
    pub fn reload_local_events(&mut self) {
        if let Some(m) = self.modules.get_mut(self.active) {
            m.reload_local();
        }
        self.data_changed();
    }

    /// The next moment (app ms) something needs to run without any input.
    pub fn next_wake(&mut self) -> Option<u64> {
        let tz = self.tz.clone();
        let env = self.env(&tz);
        let open = self.shell.mode() == Mode::Open;
        let module = self.modules[self.active].next_wake(&env, open || self.shell.mode() == Mode::Peek);
        [self.shell.next_wake(), module].into_iter().flatten().min()
    }

    pub fn wants_frames(&self) -> bool {
        self.shell.is_animating(self.clock.now_ms())
    }

    // -------------------------------------------------------------- drawing

    fn panel_theme(&self) -> Theme {
        match panel_surface(&self.settings.theme, self.windows_light) {
            Surface::Light => Theme::light(),
            Surface::Dark => Theme::dark(),
        }
    }

    /// How many rows (physical px) of the window can hold non-transparent pixels right now.
    pub fn visible_rows(&self) -> usize {
        let shape = self.shell.shape(self.clock.now_ms());
        ((shape.h * self.canvas.scale).ceil() as usize + 4).min(self.canvas.height() as usize)
    }

    pub fn shape(&self) -> Shape {
        self.shell.shape(self.clock.now_ms())
    }

    /// Draw the current frame into the canvas.
    pub fn render(&mut self) {
        let now = self.clock.now_ms();
        let tz = self.tz.clone();
        let env = self.env(&tz);
        let shape = self.shell.shape(now);
        let openness = self.shell.openness(now);
        let pill = Theme::dark();
        let panel_theme = self.panel_theme();
        let reduced = self.shell.config().reduced_motion;
        let fade = if reduced { self.shell.fade_in(now) } else { 1.0 };

        self.canvas.clear();
        // The body: black, turning into the panel surface as it opens.
        let body = lerp_rgb(pill.bg, panel_theme.bg, openness);
        let hot = self.shell.mode() == Mode::Rest && self.shell.rest_style() == RestStyle::Hot;
        // A hot strip is drawn at 1/255 opacity: invisible, but not transparent to the mouse.
        self.canvas.fill_notch(shape, body, if hot { 1.0 / 255.0 } else { 1.0 });

        self.canvas.push_notch_clip(shape);
        match self.shell.mode() {
            Mode::Rest => {
                let rest_alpha = (1.0 - openness * 3.0).clamp(0.0, 1.0) * if reduced { 1.0 - fade * 0.0 } else { 1.0 };
                if self.shell.rest_style() == RestStyle::Full && rest_alpha > 0.0 {
                    let rv = self.modules[self.active].rest(&env);
                    self.draw_rest(&rv, &pill, rest_alpha);
                }
            }
            Mode::Peek => {
                if let Some(p) = self.peek.clone() {
                    self.draw_peek(&p, &pill, self.shell.fade_in(now).max(if reduced { 0.0 } else { 1.0 }));
                }
            }
            Mode::Open => {}
        }
        let open_alpha = ((openness - 0.35) / 0.5).clamp(0.0, 1.0) * fade;
        if open_alpha > 0.0 && (self.shell.mode() == Mode::Open || openness > 0.0) {
            let frame = PanelFrame {
                theme: &panel_theme,
                alpha: open_alpha,
                viewport_h: self.shell.target().h.max(REST_FULL.h),
                origin_x: (WINDOW_W - OPEN_W) / 2.0,
                keyboard: self.shell.keyboard(),
            };
            let mut kit = Kit { canvas: &mut self.canvas, fonts: &mut self.fonts };
            self.modules[self.active].draw_panel(&env, &mut kit, &frame);
        }
        self.canvas.pop_clip();
    }

    fn draw_rest(&mut self, rv: &RestView, theme: &Theme, alpha: f32) {
        let size = 12.5;
        let s = self.canvas.scale;
        let label = self.fonts.ellipsize(Face::Mono, &rv.label, size * s, 0.0, 96.0 * s);
        let label_w = if label.is_empty() { 0.0 } else { self.canvas.measure(&mut self.fonts, Face::Mono, size, &label, 0.0) };
        let count_w = self.canvas.measure(&mut self.fonts, Face::Mono, size, &rv.count, 0.0);
        let gap = if label.is_empty() { 0.0 } else { 8.0 };
        let total = label_w + gap + count_w;
        let x = (WINDOW_W - total) / 2.0;
        let base = self.fonts.baseline(Face::Mono, size, REST_FULL.h);
        if !label.is_empty() {
            self.canvas.text(&mut self.fonts, Face::Mono, size, x, base, &label, theme.dim, alpha, 0.0);
        }
        self.canvas.text(&mut self.fonts, Face::Mono, size, x + label_w + gap, base, &rv.count, theme.urgency(rv.urgency), alpha, 0.0);
    }

    fn draw_peek(&mut self, p: &PeekView, theme: &Theme, alpha: f32) {
        let s = self.canvas.scale;
        let left = (WINDOW_W - crate::shell::PEEK.w) / 2.0 + 18.0;
        let right = (WINDOW_W + crate::shell::PEEK.w) / 2.0 - 18.0;
        let base = self.fonts.baseline(Face::Mono, 12.0, crate::shell::PEEK.h);
        let code = self.fonts.ellipsize(Face::Mono, &p.code, 12.0 * s, 0.0, 110.0 * s);
        let cw = self.canvas.text(&mut self.fonts, Face::Mono, 12.0, left, base, &code, theme.dim, alpha, 0.0);
        let count_w = self.canvas.measure(&mut self.fonts, Face::Mono, 13.0, &p.count, 0.0);
        let title_x = left + cw + 10.0;
        let title_w = (right - count_w - 10.0 - title_x).max(20.0);
        let title = self.fonts.ellipsize(Face::SansSemibold, &p.title, 13.0 * s, 0.0, title_w * s);
        let base_t = self.fonts.baseline(Face::SansSemibold, 13.0, crate::shell::PEEK.h);
        self.canvas.text(&mut self.fonts, Face::SansSemibold, 13.0, title_x, base_t, &title, theme.fg, alpha, 0.0);
        let base_c = self.fonts.baseline(Face::Mono, 13.0, crate::shell::PEEK.h);
        self.canvas.text(&mut self.fonts, Face::Mono, 13.0, right - count_w, base_c, &p.count, theme.urgency(p.urgency), alpha, 0.0);
    }

    /// Render and write the frame as a PNG.
    pub fn dump_png(&mut self, path: &str) -> Result<(), String> {
        self.render();
        self.canvas.save_png(path)
    }

    // ---------------------------------------------------------------- state

    pub fn state_json(&mut self) -> Value {
        let now = self.clock.now_ms();
        let shape = self.shell.shape(now);
        let tz = self.tz.clone();
        let env = self.env(&tz);
        let rest = self.modules[self.active].rest(&env);
        let module_debug = self.modules[self.active].debug_now(&env);
        json!({
            "mode": format!("{:?}", self.shell.mode()).to_lowercase(),
            "restStyle": format!("{:?}", self.shell.rest_style()).to_lowercase(),
            "shape": { "w": shape.w, "h": shape.h, "r": shape.r },
            "target": { "w": self.shell.target().w, "h": self.shell.target().h },
            "animating": self.shell.is_animating(now),
            "hovering": self.shell.hovering(),
            "keyboard": self.shell.keyboard(),
            "nowMs": now,
            "wallMs": self.clock.wall_ms(),
            "manualClock": self.clock.is_manual(),
            "scale": self.canvas.scale,
            "canvas": { "w": self.canvas.width(), "h": self.canvas.height() },
            "rest": { "label": rest.label, "count": rest.count, "urgency": rest.urgency },
            "panelHeight": self.last_panel_h,
            "nextWake": self.next_wake(),
            "cachedGlyphs": self.fonts.cached_glyphs(),
            "module": module_debug,
        })
    }

    pub fn default_settings() -> Settings {
        normalize_settings(None)
    }

    pub fn open_max_height() -> f32 {
        OPEN_MAX_H
    }
}