//! The app: shell state machine + modules + canvas, driven by inputs and time.
//! No Windows here, so the whole behavior is testable with a manual clock;
//! the window layer only feeds it events and presents its pixels.

use blink_core::i18n::{resolve_language, Lang, Translator};
use blink_core::settings::{normalize_settings, Settings};
use jiff::tz::TimeZone;
use serde_json::{json, Value};

use crate::clock::Clock;
use crate::host::ModuleConfig;
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

/// Height of the tab strip along the bottom of the panel when more than one module is on.
const TAB_H: f32 = 28.0;
/// Peeks waiting for the notch to be free (a long queue is a bug in a module, not a feature).
const MAX_QUEUED_PEEKS: usize = 6;

/// A module and whether the user has it switched on.
pub struct Slot {
    pub module: Box<dyn Module>,
    pub enabled: bool,
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
    slots: Vec<Slot>,
    /// The module whose panel is (or will be) shown.
    active: usize,
    /// The module that owns the resting pill right now.
    rest_owner: usize,
    peek: Option<PeekView>,
    /// Which module the showing peek came from, so a click on it opens that module.
    peek_from: Option<usize>,
    peek_queue: Vec<(PeekView, Option<usize>)>,
    /// Tabs as drawn last frame: (slot index, x, y, w, h) in window coordinates.
    tab_rects: Vec<(usize, f32, f32, f32, f32)>,
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
        let mut slots: Vec<Slot> = modules.into_iter().map(|m| Slot { enabled: m.default_enabled(), module: m }).collect();
        for slot in slots.iter_mut() {
            slot.module.configure(&settings);
        }
        let first = slots.iter().position(|s| s.enabled && s.module.has_panel()).unwrap_or(0);
        Ok(App {
            clock,
            shell: Shell::new(cfg),
            settings,
            tz,
            lang,
            windows_light: false,
            canvas: Canvas::new(w, h, scale),
            fonts,
            slots,
            active: first,
            rest_owner: first,
            peek: None,
            peek_from: None,
            peek_queue: Vec::new(),
            tab_rects: Vec::new(),
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

    /// The module of this type, whether or not it is on (tests and the window layer).
    pub fn module_mut<T: 'static>(&mut self) -> Option<&mut T> {
        self.slots.iter_mut().find_map(|s| s.module.as_any_mut().downcast_mut::<T>())
    }

    /// Something happened on the computer: tell every module that is on.
    pub fn system_event(&mut self, event: crate::events::SystemEvent) {
        let tz = self.tz.clone();
        let env = self.env(&tz);
        for i in self.enabled_indices() {
            let effects = self.slots[i].module.on_event(&event, &env);
            if !effects.is_empty() {
                self.apply_module_effects(i, effects);
            }
        }
    }

    /// The peek showing right now, if any.
    pub fn current_peek(&self) -> Option<&PeekView> {
        if self.shell.mode() == Mode::Peek { self.peek.as_ref() } else { None }
    }

    pub fn active_module(&self) -> &'static str {
        self.slots[self.active].module.id()
    }

    pub fn rest_owner_module(&self) -> &'static str {
        self.slots[self.rest_owner].module.id()
    }

    /// (id, enabled) in the user's order.
    pub fn module_list(&self) -> Vec<(&'static str, bool)> {
        self.slots.iter().map(|s| (s.module.id(), s.enabled)).collect()
    }

    fn enabled_indices(&self) -> Vec<usize> {
        self.slots.iter().enumerate().filter(|(_, s)| s.enabled).map(|(i, _)| i).collect()
    }

    /// The modules that are on and have a panel: the ones with tabs, the ones that can own the pill.
    fn panel_indices(&self) -> Vec<usize> {
        self.slots.iter().enumerate().filter(|(_, s)| s.enabled && s.module.has_panel()).map(|(i, _)| i).collect()
    }

    fn tabs_height(&self) -> f32 {
        if self.panel_indices().len() > 1 { TAB_H } else { 0.0 }
    }

    /// Apply the user's choice of modules: order and on/off. Ids the app does not know are
    /// ignored; modules the list does not mention keep their state and follow the listed ones.
    pub fn configure_modules(&mut self, config: &[ModuleConfig]) {
        let active_id = self.slots[self.active].module.id();
        let mut rest = std::mem::take(&mut self.slots);
        let mut ordered: Vec<Slot> = Vec::with_capacity(rest.len());
        for c in config {
            if let Some(at) = rest.iter().position(|s| s.module.id() == c.id) {
                let mut slot = rest.remove(at);
                slot.enabled = c.enabled;
                ordered.push(slot);
            }
        }
        ordered.extend(rest);
        // Never leave the notch with nothing to show: one module with a panel stays on.
        if !ordered.iter().any(|s| s.enabled && s.module.has_panel()) {
            if let Some(first) = ordered.iter_mut().find(|s| s.module.has_panel()) {
                first.enabled = true;
            }
        }
        for slot in ordered.iter_mut() {
            let options = config.iter().find(|c| c.id == slot.module.id()).map(|c| c.options.clone()).unwrap_or(serde_json::Value::Null);
            slot.module.set_options(&options);
        }
        self.slots = ordered;
        self.active = self
            .slots
            .iter()
            .position(|s| s.enabled && s.module.has_panel() && s.module.id() == active_id)
            .or_else(|| self.slots.iter().position(|s| s.enabled && s.module.has_panel()))
            .unwrap_or(0);
        self.peek_queue.clear();
        self.update_rest_style();
        if self.shell.mode() == Mode::Open {
            self.refresh_panel_height();
        }
    }

    /// Show another module's panel (index into the module list; it must be on).
    pub fn switch_to(&mut self, slot: usize) {
        if slot >= self.slots.len() || !self.slots[slot].enabled || !self.slots[slot].module.has_panel() || slot == self.active {
            return;
        }
        let tz = self.tz.clone();
        let env = self.env(&tz);
        let keyboard = self.shell.keyboard();
        self.slots[self.active].module.pointer(PointerKind::Leave, 0.0, 0.0, &env);
        if keyboard {
            self.slots[self.active].module.keyboard_mode(false, &env);
            self.slots[slot].module.keyboard_mode(true, &env);
        }
        self.active = slot;
        if self.shell.mode() == Mode::Open {
            self.refresh_panel_height();
        }
    }

    /// The next (+1) or previous (-1) module that is on, wrapping around.
    pub fn switch_by(&mut self, delta: i32) {
        let on = self.panel_indices();
        let Some(at) = on.iter().position(|i| *i == self.active) else { return };
        let next = (at as i32 + delta).rem_euclid(on.len() as i32) as usize;
        self.switch_to(on[next]);
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
        for slot in self.slots.iter_mut() {
            slot.module.configure(&self.settings);
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

    /// Outside the open panel the active module follows what the pill or the peek shows, so
    /// opening it shows the thing you were looking at. `force` also does it mid-animation.
    fn sync_active(&mut self, force: bool) {
        if self.shell.mode() == Mode::Open {
            return;
        }
        if !force && self.shell.is_animating(self.clock.now_ms()) {
            return;
        }
        let want = if self.shell.mode() == Mode::Peek { self.peek_from.unwrap_or(self.rest_owner) } else { self.rest_owner };
        // A module that only peeks has no panel to open: fall back to the one on the pill.
        let want = if want < self.slots.len() && self.slots[want].module.has_panel() { want } else { self.rest_owner };
        if want < self.slots.len() && self.slots[want].enabled {
            self.active = want;
        }
    }

    fn refresh_panel_height(&mut self) {
        self.sync_active(true);
        let tz = self.tz.clone();
        let env = self.env(&tz);
        let tabs = self.tabs_height();
        let mut kit = Kit { canvas: &mut self.canvas, fonts: &mut self.fonts };
        let h = self.slots[self.active].module.panel_height(&env, &mut kit) + tabs;
        self.last_panel_h = h;
        let now = self.clock.now_ms();
        self.shell.set_open_height(h, now);
    }

    fn update_rest_style(&mut self) {
        let tz = self.tz.clone();
        let env = self.env(&tz);
        // The module with the most to say owns the pill; ties go to the earlier one.
        let mut best: Option<(usize, u8)> = None;
        for i in self.panel_indices() {
            let p = self.slots[i].module.rest_priority(&env, &self.settings);
            if best.is_none_or(|(_, bp)| p > bp) {
                best = Some((i, p));
            }
        }
        self.rest_owner = best.map_or(self.active, |(i, _)| i);
        self.sync_active(false);
        let style = if self.settings.resting_style == "hover" {
            RestStyle::Hot
        } else {
            self.slots[self.rest_owner].module.rest_style(&env, &self.settings.resting_style, self.settings.due_horizon_hours as f64)
        };
        self.shell.set_rest_style(style, env.now_ms);
    }

    fn apply_module_effects(&mut self, from: usize, effects: Vec<Effect>) {
        let mut want_peek = false;
        let mut peeks: Vec<PeekView> = Vec::new();
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
                Effect::ShowPeek(view) => peeks.push(view),
            }
        }
        if self.shell.mode() == Mode::Open {
            self.refresh_panel_height();
        }
        // Marking something done can change what the resting pill shows.
        self.update_rest_style();
        if want_peek {
            self.request_peek(from);
        }
        for view in peeks {
            self.offer_peek(view, Some(from));
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
                let fx = self.slots[self.active].module.pointer(PointerKind::Leave, x, y, &env);
                self.apply_module_effects(self.active, fx);
            }
            PointerKind::Move => {
                if !inside {
                    if self.pointer.is_some() {
                        self.pointer = None;
                        self.shell.pointer_leave(now);
                        let env = self.env(&tz);
                        let fx = self.slots[self.active].module.pointer(PointerKind::Leave, x, y, &env);
                        self.apply_module_effects(self.active, fx);
                    }
                    return;
                }
                if self.pointer.is_none() {
                    self.shell.pointer_enter(now);
                }
                self.pointer = Some((x, y));
                if self.shell.mode() == Mode::Open {
                    let env = self.env(&tz);
                    let fx = self.slots[self.active].module.pointer(PointerKind::Move, x, y, &env);
                    self.apply_module_effects(self.active, fx);
                }
            }
            PointerKind::Down => {
                if !inside {
                    return;
                }
                if self.shell.mode() == Mode::Open {
                    if let Some(tab) = self.tab_at(x, y) {
                        self.switch_to(tab);
                        return;
                    }
                    let env = self.env(&tz);
                    let fx = self.slots[self.active].module.pointer(PointerKind::Down, x, y, &env);
                    self.apply_module_effects(self.active, fx);
                } else {
                    self.refresh_panel_height();
                    self.shell.click(now);
                }
            }
            PointerKind::Wheel(d) => {
                if inside && self.shell.mode() == Mode::Open {
                    let env = self.env(&tz);
                    let fx = self.slots[self.active].module.pointer(PointerKind::Wheel(d), x, y, &env);
                    self.apply_module_effects(self.active, fx);
                }
            }
        }
        self.sync_open_height();
    }

    fn tab_at(&self, x: f32, y: f32) -> Option<usize> {
        self.tab_rects.iter().find(|(_, rx, ry, rw, rh)| x >= *rx && x < rx + rw && y >= *ry && y < ry + rh).map(|t| t.0)
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
        if matches!(key, Key::Left | Key::Right | Key::Tab) && self.panel_indices().len() > 1 {
            self.switch_by(if key == Key::Left { -1 } else { 1 });
            return;
        }
        let tz = self.tz.clone();
        let env = self.env(&tz);
        let res = self.slots[self.active].module.key(key, &env);
        if res.close || (!res.handled && key == Key::Escape) {
            self.close_panel(now);
        }
        self.apply_module_effects(self.active, res.effects);
    }

    fn close_panel(&mut self, now: u64) {
        if self.shell.keyboard() {
            self.effects.push(AppEffect::KeyboardFocus(false));
            let tz = self.tz.clone();
            let env = self.env(&tz);
            self.slots[self.active].module.keyboard_mode(false, &env);
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
        self.slots[self.active].module.keyboard_mode(true, &env);
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
            self.slots[self.active].module.keyboard_mode(false, &env);
            self.shell.close(now);
        }
    }

    /// Show whatever the first module with something to announce has (tray click, second launch).
    pub fn peek(&mut self) {
        for i in self.enabled_indices() {
            if self.request_peek(i) {
                return;
            }
        }
    }

    /// Ask one module for its pending peek and offer it. Returns whether it had one.
    fn request_peek(&mut self, from: usize) -> bool {
        let tz = self.tz.clone();
        let env = self.env(&tz);
        match self.slots.get_mut(from).and_then(|s| s.module.peek(&env)) {
            Some(view) => {
                self.offer_peek(view, Some(from));
                true
            }
            None => false,
        }
    }

    /// Offer a peek (a module's, later a plugin's). A more important one replaces what shows, an
    /// equal or less important one waits, one with the same key replaces its older self, and
    /// anything offered while the panel is open waits until it closes.
    pub fn offer_peek(&mut self, view: PeekView, from: Option<usize>) {
        let showing = self.shell.mode() == Mode::Peek;
        let same_key = |a: &PeekView, b: &PeekView| a.key.is_some() && a.key == b.key;
        if showing && self.peek.as_ref().is_some_and(|cur| same_key(cur, &view)) {
            self.start_peek(view, from);
            return;
        }
        self.peek_queue.retain(|(q, _)| !same_key(q, &view));
        let free = self.shell.mode() == Mode::Rest;
        let outranks = showing && self.peek.as_ref().is_some_and(|cur| view.priority > cur.priority);
        if free || outranks {
            self.start_peek(view, from);
        } else if self.peek_queue.len() < MAX_QUEUED_PEEKS {
            self.peek_queue.push((view, from));
        }
    }

    fn start_peek(&mut self, view: PeekView, from: Option<usize>) {
        let now = self.clock.now_ms();
        let ms = view.ttl_ms.unwrap_or(self.shell.config().peek_ms);
        self.peek = Some(view);
        self.peek_from = from;
        self.shell.peek_for(now, ms);
        self.sync_active(true);
    }

    /// When the notch is free again, show the most important waiting peek.
    fn next_queued_peek(&mut self) {
        if self.shell.mode() != Mode::Rest || self.shell.hovering() || self.peek_queue.is_empty() {
            return;
        }
        let mut at = 0;
        for (i, (v, _)) in self.peek_queue.iter().enumerate() {
            if v.priority > self.peek_queue[at].0.priority {
                at = i;
            }
        }
        let (view, from) = self.peek_queue.remove(at);
        self.start_peek(view, from);
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
        self.next_queued_peek();
        changed
    }

    /// Let every module that is on sync and announce what is due.
    fn run_module_tick(&mut self) {
        let tz = self.tz.clone();
        let env = self.env(&tz);
        for i in self.enabled_indices() {
            let effects = self.slots[i].module.tick(&env);
            if !effects.is_empty() {
                self.apply_module_effects(i, effects);
            }
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
        let shown = if open { self.active } else { self.rest_owner };
        let mut nodes = self.slots[shown].module.accessibility(&env, open);
        if open && self.tabs_height() > 0.0 {
            for (slot, x, y, w, h) in self.tab_rects.clone() {
                let mut tab = Node::new(200 + slot as i32, ROOT, Role::Button, self.slots[slot].module.title(env.t.lang()));
                tab.rect = (x, y, w, h);
                tab.action = Some(crate::access::NodeAction::SwitchTab(slot));
                nodes.push(tab);
            }
        }
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
                let effects = self.slots[self.active].module.perform(a, &env);
                self.apply_module_effects(self.active, effects);
            }
            NodeAction::SwitchTab(i) => self.switch_to(i),
        }
    }

    /// Fetch the feeds now (tray "Refresh now", footer button).
    pub fn refresh_sources(&mut self) {
        for slot in self.slots.iter_mut().filter(|s| s.enabled) {
            slot.module.refresh();
        }
        self.run_module_tick();
    }

    /// The personal events file changed.
    pub fn reload_local_events(&mut self) {
        for slot in self.slots.iter_mut().filter(|s| s.enabled) {
            slot.module.reload_local();
        }
        self.data_changed();
    }

    /// The next moment (app ms) something needs to run without any input.
    pub fn next_wake(&mut self) -> Option<u64> {
        let tz = self.tz.clone();
        let env = self.env(&tz);
        let showing = matches!(self.shell.mode(), Mode::Open | Mode::Peek);
        let mut wake = self.shell.next_wake();
        for i in self.enabled_indices() {
            let w = self.slots[i].module.next_wake(&env, showing && i == self.active);
            wake = [wake, w].into_iter().flatten().min();
        }
        wake
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
                    let rv = self.slots[self.rest_owner].module.rest(&env);
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
                // The module sees the panel without the tab strip along the bottom.
                viewport_h: self.shell.target().h.max(REST_FULL.h) - self.tabs_height(),
                origin_x: (WINDOW_W - OPEN_W) / 2.0,
                keyboard: self.shell.keyboard(),
            };
            let mut kit = Kit { canvas: &mut self.canvas, fonts: &mut self.fonts };
            self.slots[self.active].module.draw_panel(&env, &mut kit, &frame);
            self.draw_tabs(&panel_theme, open_alpha, &env);
        } else {
            self.tab_rects.clear();
        }
        self.canvas.pop_clip();
    }

    /// The strip of module tabs along the bottom of the panel (only with more than one module on).
    fn draw_tabs(&mut self, theme: &Theme, alpha: f32, env: &Env) {
        self.tab_rects.clear();
        let tabs_h = self.tabs_height();
        if tabs_h <= 0.0 {
            return;
        }
        let top = self.shell.target().h.max(REST_FULL.h) - tabs_h;
        let size = 11.0;
        let pad = 11.0;
        let gap = 4.0;
        let items: Vec<(usize, String)> = self.panel_indices().into_iter().map(|i| (i, self.slots[i].module.title(env.t.lang()))).collect();
        let widths: Vec<f32> = items.iter().map(|(_, t)| self.canvas.measure(&mut self.fonts, Face::Sans, size, t, 0.0) + 2.0 * pad).collect();
        let total: f32 = widths.iter().sum::<f32>() + gap * (items.len() as f32 - 1.0);
        let mut x = (WINDOW_W - total) / 2.0;
        let h = tabs_h - 8.0;
        let y = top + 3.0;
        let faint = f32::from(theme.line.1) / 255.0 * alpha;
        for ((slot, title), w) in items.iter().zip(&widths) {
            let active = *slot == self.active;
            let over = self.pointer.is_some_and(|(px, py)| px >= x && px < x + w && py >= y && py < y + h);
            if active {
                self.canvas.rrect(x, y, *w, h, h / 2.0, theme.line.0, faint * 1.6);
            } else if over {
                self.canvas.rrect(x, y, *w, h, h / 2.0, theme.line.0, faint);
            }
            let base = y + self.fonts.baseline(Face::Sans, size, h);
            let color = if active { theme.fg } else { theme.dim };
            self.canvas.text(&mut self.fonts, Face::Sans, size, x + pad, base, title, color, alpha, 0.0);
            self.tab_rects.push((*slot, x, top, *w, tabs_h));
            x += w + gap;
        }
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
        // With a bar underneath, the text sits a little higher so the pair is centred.
        let line_h = crate::shell::PEEK.h - if p.progress.is_some() { 14.0 } else { 0.0 };
        let base = self.fonts.baseline(Face::Mono, 12.0, line_h);
        let code = self.fonts.ellipsize(Face::Mono, &p.code, 12.0 * s, 0.0, 110.0 * s);
        let cw = self.canvas.text(&mut self.fonts, Face::Mono, 12.0, left, base, &code, theme.dim, alpha, 0.0);
        let count_w = self.canvas.measure(&mut self.fonts, Face::Mono, 13.0, &p.count, 0.0);
        let title_x = left + cw + 10.0;
        let title_w = (right - count_w - 10.0 - title_x).max(20.0);
        let title = self.fonts.ellipsize(Face::SansSemibold, &p.title, 13.0 * s, 0.0, title_w * s);
        let base_t = self.fonts.baseline(Face::SansSemibold, 13.0, line_h);
        self.canvas.text(&mut self.fonts, Face::SansSemibold, 13.0, title_x, base_t, &title, theme.fg, alpha, 0.0);
        let base_c = self.fonts.baseline(Face::Mono, 13.0, line_h);
        self.canvas.text(&mut self.fonts, Face::Mono, 13.0, right - count_w, base_c, &p.count, theme.urgency(p.urgency), alpha, 0.0);
        if let Some(progress) = p.progress {
            // A thin bar along the bottom: the track, then the filled part.
            let (y, h) = (crate::shell::PEEK.h - 18.0, 3.0);
            let width = right - left;
            self.canvas.rrect(left, y, width, h, h / 2.0, theme.line.0, f32::from(theme.line.1) / 255.0 * alpha * 1.4);
            let filled = (width * progress.clamp(0.0, 1.0)).max(if progress > 0.0 { h } else { 0.0 });
            if filled > 0.0 {
                self.canvas.rrect(left, y, filled, h, h / 2.0, theme.fg, alpha);
            }
        }
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
        let rest = self.slots[self.rest_owner].module.rest(&env);
        let module_debug = self.slots[self.active].module.debug_now(&env);
        let modules: Vec<Value> = self
            .slots
            .iter()
            .enumerate()
            .map(|(i, s)| json!({ "id": s.module.id(), "enabled": s.enabled, "active": i == self.active, "restOwner": i == self.rest_owner }))
            .collect();
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
            "modules": modules,
            "tabs": self.tabs_height() > 0.0,
            "queuedPeeks": self.peek_queue.len(),
            "peek": self.current_peek().map(|p| json!({ "code": p.code, "title": p.title, "count": p.count, "progress": p.progress, "priority": p.priority })),
        })
    }

    pub fn default_settings() -> Settings {
        normalize_settings(None)
    }

    pub fn open_max_height() -> f32 {
        OPEN_MAX_H
    }
}