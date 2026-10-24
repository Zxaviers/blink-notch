//! The app: shell state machine + modules + canvas, driven by inputs and time.
//! No Windows here, so the whole behavior is testable with a manual clock;
//! the window layer only feeds it events and presents its pixels.

use blink_core::i18n::{resolve_language, Lang, Translator};
use blink_core::settings::{normalize_settings, Settings};
use jiff::tz::TimeZone;
use serde_json::{json, Value};

use crate::clock::Clock;
use crate::host::ModuleConfig;
use crate::modules::{Effect, Env, Key, Kit, Module, PanelFrame, PeekView, PointerKind, RestView, HEADER_H, PAGER_H};
use crate::shell::{Mode, RestStyle, Shape, Shell, ShellConfig, OPEN_MAX_H, OPEN_W, REST_FULL, WINDOW_H, WINDOW_W};
use crate::ui::canvas::{Canvas, Icon};
use crate::ui::text::{Face, Fonts};
use crate::ui::theme::{panel_surface, Rgb, Surface, Theme};

/// What the window layer must do on the app's behalf.
#[derive(Clone, Debug, PartialEq)]
pub enum AppEffect {
    OpenUrl(String),
    /// The settings of the deadlines (feeds, notifications, my events).
    OpenSettings,
    /// Blink's own settings (the button at the top right).
    OpenAppSettings,
    /// Blink's own settings, on this section.
    OpenSettingsAt(String),
    /// Open a file or folder.
    OpenPath(String),
    /// Show a file in its folder.
    RevealPath(String),
    RefreshFeeds,
    /// Take (true) or give back (false) real keyboard focus.
    KeyboardFocus(bool),
    /// Something for a screen reader to say (only matters while one is running).
    Announce(String),
    /// A system notification; `url` is what clicking it opens.
    Toast { title: String, body: String, url: Option<String> },
}

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
    /// The page dots and arrows as drawn last frame: (slot index, x, y, w, h) in window coordinates.
    tab_rects: Vec<(usize, f32, f32, f32, f32)>,
    /// Charge and whether it is charging, from the last power event (the header ring shows it).
    battery: Option<(u8, bool)>,
    /// When the person last did anything here (the eyes wake up from it).
    last_activity: u64,
    /// The panel was open and is still on its way out (its page may still be drawn, fading).
    panel_shown: bool,
    /// The eyes are moving (set by the last frame; the window keeps drawing frames while it is true).
    eyes_moving: bool,
    pointer: Option<(f32, f32)>,
    effects: Vec<AppEffect>,
    /// The last frame's panel height, to retarget the shell when content changes.
    last_panel_h: f32,
    /// No window is in front: the desktop (or the taskbar) is.
    desktop: bool,
    /// When the desktop came to the front (the notch waits a moment before opening for it).
    desktop_since: u64,
    /// The notch is being held open for a song (see `update_live`).
    live_on: bool,
    /// The person closed it while it was held open: leave it closed until the reason ends.
    live_dismissed: bool,
}

/// How long the empty desktop has to be in front before a song opens the notch.
const DESKTOP_SETTLE_MS: u64 = 1_000;

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
            battery: None,
            last_activity: 0,
            panel_shown: false,
            eyes_moving: false,
            pointer: None,
            effects: Vec::new(),
            last_panel_h: 0.0,
            desktop: false,
            desktop_since: 0,
            live_on: false,
            live_dismissed: false,
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
        if let crate::events::SystemEvent::Power { percent, charging, .. } = &event {
            self.battery = percent.map(|p| (p, *charging));
        }
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
        if self.panel_indices().len() > 1 { PAGER_H } else { 0.0 }
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
        let h = HEADER_H + self.slots[self.active].module.panel_height(&env, &mut kit) + tabs;
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
                Effect::OpenSettingsAt(section) => self.effects.push(AppEffect::OpenSettingsAt(section)),
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
                Effect::TakeKeyboard => self.ensure_keyboard(),
                Effect::ReleaseKeyboard => self.release_keyboard(),
                Effect::OpenPath(p) => self.effects.push(AppEffect::OpenPath(p)),
                Effect::RevealPath(p) => self.effects.push(AppEffect::RevealPath(p)),
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
        self.update_live();
    }

    // ---------------------------------------------------------------- input

    /// A pointer event in window coordinates (logical px). Moves outside the
    /// visible shape count as leaving it, exactly as Windows reports a layered
    /// window whose pixels are transparent there.
    pub fn pointer(&mut self, kind: PointerKind, x: f32, y: f32) {
        let now = self.clock.now_ms();
        self.last_activity = now;
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
            PointerKind::Up => {
                if self.shell.mode() == Mode::Open {
                    let env = self.env(&tz);
                    let fx = self.slots[self.active].module.pointer(PointerKind::Up, x, y, &env);
                    self.apply_module_effects(self.active, fx);
                }
            }
            PointerKind::Move => {
                // A drag in progress keeps following the pointer outside the notch.
                if self.shell.mode() == Mode::Open && self.slots[self.active].module.captures_pointer() {
                    self.pointer = Some((x, y));
                    let env = self.env(&tz);
                    let fx = self.slots[self.active].module.pointer(PointerKind::Move, x, y, &env);
                    self.apply_module_effects(self.active, fx);
                    return;
                }
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
                    if self.in_menu(x, y) {
                        self.effects.push(AppEffect::OpenAppSettings);
                        return;
                    }
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

    /// The button at the right end of the header: Blink's settings.
    fn menu_rect() -> (f32, f32, f32, f32) {
        let right = (WINDOW_W + OPEN_W) / 2.0 - 16.0;
        (right - 28.0, 23.0 - 14.0, 28.0, 28.0)
    }

    fn in_menu(&self, x: f32, y: f32) -> bool {
        let (mx, my, mw, mh) = Self::menu_rect();
        x >= mx && x < mx + mw && y >= my && y < my + mh
    }

    fn tab_at(&self, x: f32, y: f32) -> Option<usize> {
        self.tab_rects.iter().find(|(_, rx, ry, rw, rh)| x >= *rx && x < rx + rw && y >= *ry && y < ry + rh).map(|t| t.0)
    }

    /// The open panel is in the middle of a drag (the window layer then captures the mouse).
    pub fn pointer_captured(&self) -> bool {
        self.shell.mode() == Mode::Open && self.slots[self.active].module.captures_pointer()
    }

    /// Opening starts only once the panel height is known.
    fn sync_open_height(&mut self) {
        if self.shell.hovering() && self.shell.mode() != Mode::Open {
            self.refresh_panel_height();
        }
    }

    pub fn key(&mut self, key: Key) {
        let now = self.clock.now_ms();
        self.last_activity = now;
        if self.shell.mode() != Mode::Open {
            return;
        }
        if matches!(key, Key::Left | Key::Right | Key::Tab) && self.panel_indices().len() > 1 && !self.slots[self.active].module.captures_keys() {
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

    /// Take the keyboard for a page that is about to be typed into (a click on "add"), unless it has it.
    fn ensure_keyboard(&mut self) {
        if self.shell.keyboard() || self.shell.mode() != Mode::Open {
            return;
        }
        let now = self.clock.now_ms();
        let tz = self.tz.clone();
        let env = self.env(&tz);
        self.slots[self.active].module.keyboard_mode(true, &env);
        self.shell.enter_keyboard(now);
        self.effects.push(AppEffect::KeyboardFocus(true));
    }

    /// Give the keyboard back after a page took it for typing; the notch stays open while the pointer is on it.
    fn release_keyboard(&mut self) {
        if !self.shell.keyboard() {
            return;
        }
        let now = self.clock.now_ms();
        let tz = self.tz.clone();
        let env = self.env(&tz);
        self.slots[self.active].module.keyboard_mode(false, &env);
        self.shell.release_keyboard(now);
        self.effects.push(AppEffect::KeyboardFocus(false));
    }

    /// Text pasted from the clipboard goes to the page being typed into.
    pub fn paste(&mut self, text: &str) {
        if self.shell.mode() != Mode::Open {
            return;
        }
        let effects = self.slots[self.active].module.paste(text);
        self.apply_module_effects(self.active, effects);
    }

    /// Files are being dragged over the notch (or have left): open it on the Projects page with its drop zone lit.
    pub fn drag_files(&mut self, over: bool) {
        let now = self.clock.now_ms();
        self.last_activity = now;
        let Some(i) = self.slots.iter().position(|s| s.enabled && s.module.id() == "projects") else { return };
        if let Some(p) = self.module_mut::<crate::modules::projects::ProjectsModule>() {
            p.set_drag_over(over);
        }
        if over {
            if self.shell.mode() != Mode::Open {
                // Open first, then point the open panel at the Projects page (while closed, the page follows what
                // the pill shows, which is not what a dragged file is for).
                self.refresh_panel_height();
                self.shell.click(now);
                self.active = i;
                self.refresh_panel_height();
            } else {
                self.switch_to(i);
            }
        } else if !self.shell.keyboard() {
            self.shell.pointer_leave(now);
        }
    }

    /// Files or folders dropped on the notch: the Projects page takes them (and shows them).
    pub fn drop_files(&mut self, paths: &[String]) {
        let Some(i) = self.slots.iter().position(|s| s.enabled && s.module.id() == "projects") else { return };
        let effects = self.slots[i].module.drop_files(paths);
        self.apply_module_effects(i, effects);
        if self.shell.mode() == Mode::Open {
            self.switch_to(i);
        }
    }

    /// The global hotkey: open and take keyboard focus, or close.
    pub fn hotkey(&mut self) {
        let now = self.clock.now_ms();
        self.last_activity = now;
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
        self.last_activity = now;
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
        self.update_live();
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
        self.update_live();
        if self.shell.mode() == Mode::Open {
            self.refresh_panel_height();
        }
    }

    /// Whether no window is in front (the desktop with its icons, or the taskbar): a song playing then shows
    /// on the notch with its lyrics until a window comes up.
    pub fn set_desktop(&mut self, showing: bool) {
        if self.desktop != showing {
            self.desktop = showing;
            self.desktop_since = self.clock.now_ms();
            self.update_live();
        }
    }

    pub fn desktop(&self) -> bool {
        self.desktop
    }

    /// Hold the notch open on the page of a song that is playing, when the desktop is empty (and the page
    /// likes that) or when the person pinned it open. Let go when the reason ends or the song stops.
    fn update_live(&mut self) {
        let now = self.clock.now_ms();
        let song = self.slots.iter().position(|s| s.enabled && s.module.has_panel() && s.module.playing());
        let pinned = song.is_some_and(|i| self.slots[i].module.holds_open());
        // A click on the taskbar to start something also passes through "desktop in front": wait a moment.
        let settled = self.desktop && now >= self.desktop_since + DESKTOP_SETTLE_MS;
        let want = pinned || song.is_some_and(|i| settled && self.slots[i].module.live_on_desktop());
        if !want {
            self.live_dismissed = false;
            if self.live_on {
                self.live_on = false;
                self.shell.set_hold(false, now);
            }
            return;
        }
        let Some(i) = song else { return };
        if self.live_on {
            if self.shell.mode() != Mode::Open {
                // Closed by the person (Escape, a click on the menu): respect that until the reason ends.
                self.live_on = false;
                self.live_dismissed = true;
                self.shell.set_hold(false, now);
            }
            return;
        }
        // Pressing the pin is a fresh request, so it overrides an earlier close.
        if (self.live_dismissed && !pinned) || self.shell.keyboard() {
            return;
        }
        self.live_dismissed = false;
        self.live_on = true;
        self.shell.set_hold(true, now);
        self.active = i;
        self.slots[i].module.live_view();
        self.peek_queue.clear();
        self.refresh_panel_height();
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
        if open {
            let (mx, my, mw, mh) = Self::menu_rect();
            let mut menu = Node::new(250, ROOT, Role::Button, crate::access::word(env.t.lang(), "app_settings"));
            menu.rect = (mx, my, mw, mh);
            menu.action = Some(crate::access::NodeAction::AppSettings);
            nodes.push(menu);
        }
        if open && self.tabs_height() > 0.0 {
            let mut seen: Vec<usize> = Vec::new();
            for (slot, x, y, w, h) in self.tab_rects.clone() {
                // The dots come first; the arrows point at the same pages and add nothing a reader needs.
                if seen.contains(&slot) {
                    continue;
                }
                seen.push(slot);
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
            NodeAction::AppSettings => self.effects.push(AppEffect::OpenAppSettings),
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
        let now = self.clock.now_ms();
        wake = [wake, self.eyes_wake(now, &env)].into_iter().flatten().min();
        // The empty desktop has just come to the front with a song playing: look again once it has settled.
        if self.desktop && now < self.desktop_since + DESKTOP_SETTLE_MS && self.music_playing() {
            wake = [wake, Some(self.desktop_since + DESKTOP_SETTLE_MS)].into_iter().flatten().min();
        }
        // The bars move with the music: redraw a few times a second, but only while it plays and the notch shows them.
        if self.music_playing() && (self.shell.mode() == Mode::Open || (self.shell.mode() == Mode::Rest && self.shell.rest_style() == RestStyle::Full)) {
            wake = [wake, Some(now + 120)].into_iter().flatten().min();
        }
        for i in self.enabled_indices() {
            let w = self.slots[i].module.next_wake(&env, showing && i == self.active);
            wake = [wake, w].into_iter().flatten().min();
        }
        wake
    }

    pub fn wants_frames(&self) -> bool {
        self.shell.is_animating(self.clock.now_ms()) || self.eyes_moving || (self.shell.mode() == Mode::Open && self.slots[self.active].module.wants_frames())
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
        self.eyes_moving = false;
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
                    let step = self.eyes_step(now, &env);
                    self.eyes_moving = step.is_some_and(|s| s.animating);
                    // The clock gives way to the eyes with a quick cross-fade.
                    let clock_alpha = step.map_or(1.0, |s| (1.0 - s.frame.appear * 2.2).clamp(0.0, 1.0));
                    if clock_alpha > 0.01 {
                        let rv = self.slots[self.rest_owner].module.rest(&env);
                        self.draw_rest(&rv, &pill, rest_alpha * clock_alpha, &env);
                    }
                    if let Some(step) = step {
                        self.draw_eyes(&step.frame, &pill, rest_alpha);
                    }
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
        // A peek is as tall as a quarter of a page, but it is not one: only draw the page while it is open or closing.
        match self.shell.mode() {
            Mode::Open => self.panel_shown = true,
            Mode::Peek => self.panel_shown = false,
            Mode::Rest => {
                if !self.shell.is_animating(now) {
                    self.panel_shown = false;
                }
            }
        }
        if open_alpha > 0.0 && self.panel_shown {
            let frame = PanelFrame {
                theme: &panel_theme,
                alpha: open_alpha,
                // The module sees its page: the panel without the header above and the pager below.
                viewport_h: (self.shell.target().h.max(REST_FULL.h) - HEADER_H - self.tabs_height()).max(0.0),
                origin_x: (WINDOW_W - OPEN_W) / 2.0,
                origin_y: HEADER_H,
                width: OPEN_W,
                keyboard: self.shell.keyboard(),
            };
            if self.slots[self.active].module.id() == "clock" {
                let marks: Vec<(i64, &'static str)> = self.slots.iter_mut().filter(|s| s.enabled).flat_map(|s| s.module.due_dates(&env)).collect();
                if let Some(clock) = self.module_mut::<crate::modules::clock::ClockModule>() {
                    clock.set_marks(marks);
                }
            }
            let mut kit = Kit { canvas: &mut self.canvas, fonts: &mut self.fonts };
            self.slots[self.active].module.draw_panel(&env, &mut kit, &frame);
            self.draw_header(&panel_theme, open_alpha, &env);
            self.draw_pager(&panel_theme, open_alpha, &env, frame.viewport_h);
        } else {
            self.tab_rects.clear();
        }
        self.canvas.pop_clip();
    }

    /// The row above a page: its name (the second half in its colour), the date and time with a
    /// small ring (the battery, or how much of the day has gone), and the activity bars.
    fn draw_header(&mut self, theme: &Theme, alpha: f32, env: &Env) {
        let left = (WINDOW_W - OPEN_W) / 2.0 + 24.0;
        let right = (WINDOW_W + OPEN_W) / 2.0 - 24.0;
        let cy = 23.0;
        let lang = env.t.lang();
        let (first, second) = self.slots[self.active].module.title_parts(lang);
        let accent = self.slots[self.active].module.accent();
        let size = 15.0;
        let base = cy - 10.0 + self.fonts.baseline(Face::SansBold, size, 20.0);
        let adv = self.canvas.text(&mut self.fonts, Face::SansBold, size, left, base, &first, theme.fg, alpha, 0.0);
        if !second.is_empty() {
            self.canvas.text(&mut self.fonts, Face::SansBold, size, left + adv, base, &second, accent, alpha, 0.0);
        }

        // The middle: ring, date, time.
        let date = crate::modules::clock::short_date(env.wall_ms, env.tz, lang);
        let time = crate::modules::clock::hhmm(env.wall_ms, env.tz);
        let dw = self.canvas.measure(&mut self.fonts, Face::Sans, 13.0, &date, 0.0);
        let tw = self.canvas.measure(&mut self.fonts, Face::SansBold, 13.0, &time, 0.0);
        let total = 14.0 + 8.0 + dw + 7.0 + tw;
        let x = WINDOW_W / 2.0 - total / 2.0;
        let (fraction, ring) = match self.battery {
            Some((p, charging)) => (f32::from(p) / 100.0, if charging || p > 20 { Rgb(0x34, 0xd3, 0x99) } else if p > 10 { theme.soon } else { theme.late }),
            None => (((env.wall_ms.rem_euclid(86_400_000)) as f32 / 86_400_000.0).clamp(0.0, 1.0), accent),
        };
        let faint = f32::from(theme.line.1) / 255.0 * alpha * 1.6;
        self.canvas.arc(x + 7.0, cy, 5.8, 2.2, -90.0, 359.0, theme.line.0, faint);
        self.canvas.arc(x + 7.0, cy, 5.8, 2.2, -90.0, 360.0 * fraction.max(0.03), ring, alpha);
        let tb = cy - 10.0 + self.fonts.baseline(Face::Sans, 13.0, 20.0);
        self.canvas.text(&mut self.fonts, Face::Sans, 13.0, x + 22.0, tb, &date, theme.dim, alpha, 0.0);
        self.canvas.text(&mut self.fonts, Face::SansBold, 13.0, x + 22.0 + dw + 7.0, tb, &time, theme.fg, alpha, 0.0);

        // The right: the button for Blink's own settings, three dots.
        let (mx, my, mw, mh) = Self::menu_rect();
        let over = self.pointer.is_some_and(|(px, py)| px >= mx && px < mx + mw && py >= my && py < my + mh);
        if over {
            self.canvas.circle(mx + mw / 2.0, my + mh / 2.0, 13.0, theme.fg, alpha * 0.18);
        }
        for i in 0..3 {
            self.canvas.circle(mx + mw / 2.0 + (i as f32 - 1.0) * 5.5, my + mh / 2.0, 1.7, theme.fg, alpha * if over { 1.0 } else { 0.75 });
        }
        // While a song plays, bars that move with it sit to the left of the button.
        if self.music_playing() {
            self.draw_bars(mx - 22.0, cy, theme.fg, alpha * 0.9, env.now_ms);
        }
        let _ = right;
    }

    /// The dots along the bottom and the round buttons that turn the page (only with more than one page).
    fn draw_pager(&mut self, theme: &Theme, alpha: f32, _env: &Env, page_h: f32) {
        self.tab_rects.clear();
        let order = self.panel_indices();
        if order.len() < 2 {
            return;
        }
        let notch_h = self.shell.target().h.max(REST_FULL.h);
        let dot_y = notch_h - PAGER_H / 2.0 - 1.0;
        let (small, wide, gap) = (5.0_f32, 20.0_f32, 6.0_f32);
        let total: f32 = order.iter().map(|i| if *i == self.active { wide } else { small }).sum::<f32>() + gap * (order.len() as f32 - 1.0);
        let mut x = WINDOW_W / 2.0 - total / 2.0;
        for &slot in &order {
            let active = slot == self.active;
            let w = if active { wide } else { small };
            self.canvas.rrect(x, dot_y - small / 2.0, w, small, small / 2.0, theme.fg, alpha * if active { 1.0 } else { 0.28 });
            self.tab_rects.push((slot, x - 4.0, dot_y - 9.0, w + 8.0, 18.0));
            x += w + gap;
        }
        if !self.slots[self.active].module.page_arrows() {
            return;
        }
        let at = order.iter().position(|i| *i == self.active).unwrap_or(0);
        let mid_y = HEADER_H + page_h / 2.0;
        let cx_left = (WINDOW_W - OPEN_W) / 2.0 + 20.0;
        let cx_right = (WINDOW_W + OPEN_W) / 2.0 - 20.0;
        let arrow = |this: &mut Self, slot: usize, cx: f32, icon: crate::ui::canvas::Icon| {
            let over = this.pointer.is_some_and(|(px, py)| (px - cx).abs() <= 15.0 && (py - mid_y).abs() <= 15.0);
            this.canvas.circle(cx, mid_y, 12.5, theme.line.0, f32::from(theme.line.1) / 255.0 * alpha * if over { 4.0 } else { 2.6 });
            this.canvas.icon(icon, cx, mid_y, 14.0, theme.fg, alpha);
            this.tab_rects.push((slot, cx - 15.0, mid_y - 15.0, 30.0, 30.0));
        };
        if at > 0 {
            arrow(self, order[at - 1], cx_left, crate::ui::canvas::Icon::ChevronLeft);
        }
        if at + 1 < order.len() {
            arrow(self, order[at + 1], cx_right, crate::ui::canvas::Icon::ChevronRight);
        }
    }

    // ------------------------------------------------------------ eyes

    /// What the eyes look like now, if they are showing (see `eyes`): after a while of quiet, over a
    /// calm pill.
    fn eyes_step(&mut self, now: u64, env: &Env) -> Option<crate::eyes::Step> {
        if !self.eyes_possible(env) {
            return None;
        }
        crate::eyes::at(now.saturating_sub(self.last_activity))
    }

    /// Eyes need a quiet pill: the clock, or a deadline that is not close yet (never one that is
    /// near or missed, which the pill must keep showing).
    fn eyes_possible(&mut self, env: &Env) -> bool {
        if self.shell.mode() != Mode::Rest || self.shell.rest_style() != RestStyle::Full {
            return false;
        }
        let calm = self.slots[self.rest_owner].module.id() == "clock" || self.slots[self.rest_owner].module.rest(env).urgency == "later";
        calm && self.module_mut::<crate::modules::clock::ClockModule>().is_none_or(|c| c.eyes)
    }

    /// The next moment the eyes change by themselves (they are drawn frame by frame while they move).
    fn eyes_wake(&mut self, now: u64, env: &Env) -> Option<u64> {
        if !self.eyes_possible(env) {
            return None;
        }
        let idle = now.saturating_sub(self.last_activity);
        if idle < crate::eyes::EYES_AFTER_MS {
            return Some(self.last_activity + crate::eyes::EYES_AFTER_MS);
        }
        crate::eyes::at(idle)?.next_change_ms.map(|at| self.last_activity + at)
    }

    fn draw_eyes(&mut self, f: &crate::eyes::EyeFrame, theme: &Theme, alpha: f32) {
        let cx = WINDOW_W / 2.0;
        let cy = REST_FULL.h / 2.0 - 1.0;
        let glow = Rgb(0x9a, 0xe0, 0xff);
        let a = alpha * f.appear;
        // They grow into place, and look a few pixels to the side.
        let (w, full_h) = (9.0 * (0.7 + 0.3 * f.appear), 15.0 * (0.55 + 0.45 * f.appear));
        let h = (full_h * f.open).max(2.4);
        for dx in [-15.0_f32, 15.0] {
            let x = cx + dx + f.look * 3.4;
            self.canvas.glow(x, cy, 13.0, glow, a * 0.35 * f.open);
            self.canvas.rrect(x - w / 2.0, cy - h / 2.0, w, h, (w / 2.0).min(h / 2.0), theme.fg, a);
        }
        // A soft light under the notch, like a screen glowing in the dark; dimmer when drowsy.
        let line_w = 40.0 + 50.0 * f.open;
        let (c0, c1) = (Rgb(0x38, 0xd6, 0xe8), Rgb(0xa7, 0x8b, 0xfa));
        self.canvas.gradient_rrect(cx - line_w / 2.0, REST_FULL.h - 2.5, line_w, 2.0, 1.0, c0, c1, false, a * (0.4 + 0.45 * f.open));
    }

    // --------------------------------------------------------- pill and peek

    /// Whether any page that is on has a song playing.
    fn music_playing(&self) -> bool {
        self.slots.iter().any(|s| s.enabled && s.module.playing())
    }

    /// Four small bars: flat when `now_ms` is `u64::MAX`, otherwise rising and falling with the time.
    fn draw_bars(&mut self, x: f32, cy: f32, color: Rgb, alpha: f32, now_ms: u64) {
        for i in 0..4 {
            let h = if now_ms == u64::MAX { 4.0 } else { 4.0 + 9.0 * ((now_ms as f32 / 170.0 + i as f32 * 1.7).sin().abs()) };
            self.canvas.rrect(x + i as f32 * 4.0, cy - h / 2.0, 2.0, h, 1.0, color, alpha);
        }
    }

    /// The ring in the header and the pill: the battery when there is one, otherwise how much of the day has gone.
    fn ring_state(&self, theme: &Theme, wall_ms: i64, accent: Rgb) -> (f32, Rgb) {
        match self.battery {
            Some((p, charging)) => (f32::from(p) / 100.0, if charging || p > 20 { Rgb(0x34, 0xd3, 0x99) } else if p > 10 { theme.soon } else { theme.late }),
            None => ((wall_ms.rem_euclid(86_400_000) as f32 / 86_400_000.0).clamp(0.0, 1.0), accent),
        }
    }

    /// The resting pill: a ring at the left, the text in the middle, quiet bars at the right.
    fn draw_rest(&mut self, rv: &RestView, theme: &Theme, alpha: f32, env: &Env) {
        let s = self.canvas.scale;
        let left = (WINDOW_W - REST_FULL.w) / 2.0 + 17.0;
        let right = (WINDOW_W + REST_FULL.w) / 2.0 - 17.0;
        let cy = REST_FULL.h / 2.0;
        let accent = self.slots[self.rest_owner].module.accent();
        let (fraction, color) = self.ring_state(theme, env.wall_ms, accent);
        let faint = f32::from(theme.line.1) / 255.0 * alpha * 1.6;
        self.canvas.arc(left, cy, 5.8, 2.2, -90.0, 359.0, theme.line.0, faint);
        self.canvas.arc(left, cy, 5.8, 2.2, -90.0, 360.0 * fraction.max(0.03), color, alpha);
        let playing = self.music_playing();
        self.draw_bars(right - 12.0, cy, theme.fg, alpha * if playing { 0.9 } else { 0.6 }, if playing { env.now_ms } else { u64::MAX });

        let room = right - left - 40.0;
        let label = self.fonts.ellipsize(Face::Mono, &rv.label, 11.5 * s, 0.0, (room - 50.0).max(30.0).min(96.0) * s);
        let label_w = if label.is_empty() { 0.0 } else { self.canvas.measure(&mut self.fonts, Face::Mono, 11.5, &label, 0.0) };
        let size = if rv.label.is_empty() { 15.0 } else { 13.5 };
        let count = self.fonts.ellipsize(Face::SansBold, &rv.count, size * s, 0.0, (room - label_w - 8.0).max(30.0) * s);
        let count_w = self.canvas.measure(&mut self.fonts, Face::SansBold, size, &count, 0.0);
        let gap = if label.is_empty() { 0.0 } else { 8.0 };
        let x = WINDOW_W / 2.0 - (label_w + gap + count_w) / 2.0;
        if !label.is_empty() {
            let base = self.fonts.baseline(Face::Mono, 11.5, REST_FULL.h);
            self.canvas.text(&mut self.fonts, Face::Mono, 11.5, x, base, &label, theme.dim, alpha, 0.0);
        }
        let base = self.fonts.baseline(Face::SansBold, size, REST_FULL.h);
        self.canvas.text(&mut self.fonts, Face::SansBold, size, x + label_w + gap, base, &count, theme.urgency(rv.urgency), alpha, 0.0);
    }

    /// A peek: a ring with a picture at the left (it fills with the level, when there is one), the
    /// words beside it, and the big number at the right.
    fn draw_peek(&mut self, p: &PeekView, theme: &Theme, alpha: f32) {
        let s = self.canvas.scale;
        let shape = crate::shell::PEEK;
        let left = (WINDOW_W - shape.w) / 2.0 + 22.0;
        let right = (WINDOW_W + shape.w) / 2.0 - 22.0;
        let cy = shape.h / 2.0;
        let accent = p.accent.unwrap_or(match p.urgency {
            "late" => theme.late,
            "soon" => theme.soon,
            _ => Rgb(0x4e, 0xa8, 0xff),
        });

        // The ring.
        let (rx, rr) = (left + 19.0, 16.5);
        self.canvas.glow(rx, cy, 36.0, accent, alpha * 0.30);
        self.canvas.arc(rx, cy, rr, 3.0, -90.0, 359.0, theme.fg, alpha * 0.16);
        self.canvas.arc(rx, cy, rr, 3.0, -90.0, 360.0 * p.progress.unwrap_or(1.0).clamp(0.02, 1.0), accent, alpha);
        self.canvas.icon(p.icon.unwrap_or(Icon::Alarm), rx, cy, 16.0, accent, alpha);

        // The big number.
        let count_w = if p.count.is_empty() { 0.0 } else { self.canvas.measure(&mut self.fonts, Face::SansLight, 28.0, &p.count, 0.0) };
        if count_w > 0.0 {
            let base = cy - 18.0 + self.fonts.baseline(Face::SansLight, 28.0, 36.0);
            let color = match p.urgency {
                "late" => theme.late,
                "soon" => theme.soon,
                _ => theme.fg,
            };
            self.canvas.text(&mut self.fonts, Face::SansLight, 28.0, right - count_w, base, &p.count, color, alpha, 0.0);
        }

        // The words: a bold line and a quiet one under it.
        let (bold, quiet) = if p.title.is_empty() { (p.code.as_str(), "") } else { (p.title.as_str(), p.code.as_str()) };
        let tx = left + 50.0;
        let max_w = (right - tx - if count_w > 0.0 { count_w + 14.0 } else { 0.0 }).max(40.0);
        let bold = self.fonts.ellipsize(Face::SansBold, bold, 15.0 * s, 0.0, max_w * s);
        if quiet.is_empty() {
            let base = cy - 12.0 + self.fonts.baseline(Face::SansBold, 15.0, 24.0);
            self.canvas.text(&mut self.fonts, Face::SansBold, 15.0, tx, base, &bold, theme.fg, alpha, 0.0);
        } else {
            let base = cy - 21.0 + self.fonts.baseline(Face::SansBold, 15.0, 22.0);
            self.canvas.text(&mut self.fonts, Face::SansBold, 15.0, tx, base, &bold, theme.fg, alpha, 0.0);
            let quiet = self.fonts.ellipsize(Face::Sans, quiet, 12.5 * s, 0.0, max_w * s);
            let base = cy + 1.0 + self.fonts.baseline(Face::Sans, 12.5, 18.0);
            self.canvas.text(&mut self.fonts, Face::Sans, 12.5, tx, base, &quiet, theme.dim, alpha, 0.0);
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