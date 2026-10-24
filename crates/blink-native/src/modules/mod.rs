//! The module interface. The notch is a host: it owns the window, layout,
//! animation and which module is on top; a module owns one kind of content
//! (deadlines first; a timer, media, system peeks later).

pub mod clock;
pub mod controls;
pub mod deadlines;
pub mod music;
pub mod projects;
pub mod system;

use blink_core::i18n::Translator;
use jiff::tz::TimeZone;

use crate::ui::canvas::{Canvas, Icon};
use crate::ui::text::Fonts;
use crate::ui::theme::{Rgb, Theme};

/// The header row of the panel (title, date and time) that the host draws above a page.
pub const HEADER_H: f32 = 42.0;
/// The row of page dots along the bottom of the panel (only with more than one page).
pub const PAGER_H: f32 = 26.0;
/// Room kept free at each side of a page for the buttons that turn the page.
pub const PAGE_SIDE: f32 = 36.0;
/// How tall a page is, between the header and the pager.
pub const CONTENT_H: f32 = 124.0;

/// Everything a module needs to know about "now" and the user's settings.
pub struct Env<'a> {
    /// Monotonic app time in ms (animation, hover timing).
    pub now_ms: u64,
    /// Unix ms: what deadlines are compared against.
    pub wall_ms: i64,
    pub tz: &'a TimeZone,
    pub t: Translator,
}

/// Canvas plus fonts, passed together to drawing code.
pub struct Kit<'a> {
    pub canvas: &'a mut Canvas,
    pub fonts: &'a mut Fonts,
}

/// What the resting pill shows.
#[derive(Clone, Debug, PartialEq)]
pub struct RestView {
    pub label: String,
    pub count: String,
    /// "later", "soon" or "overdue" (colors the count; the text carries the meaning too).
    pub urgency: &'static str,
}

/// A short automatic expansion about one item.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct PeekView {
    pub code: String,
    pub title: String,
    pub count: String,
    pub urgency: &'static str,
    /// Higher replaces lower; equal waits its turn. 1 = ordinary, 3 = needs attention now.
    pub priority: u8,
    /// How long it stays; None means the standard peek time.
    pub ttl_ms: Option<u64>,
    /// A peek with the same key replaces the one showing (or waiting) instead of queueing.
    pub key: Option<String>,
    /// A ring around the picture, 0.0 to 1.0 (volume, battery).
    pub progress: Option<f32>,
    /// The picture in the ring at the left of the peek.
    pub icon: Option<Icon>,
    /// The colour of the ring; None takes it from `urgency`.
    pub accent: Option<Rgb>,
}

/// Something a click or a key asks for.
#[derive(Clone, Debug, PartialEq)]
pub enum Action {
    OpenUrl(String),
    MarkDone(String),
    Restore(String),
    ToggleSnooze(String),
    Snooze(String, &'static str),
    ToggleDone,
    Refresh,
    OpenSettings,
    Control(controls::ControlCommand),
    Music(music::MusicAction),
    Project(projects::ProjectAction),
}

/// What the host must do for a module.
#[derive(Clone, Debug, PartialEq)]
pub enum Effect {
    OpenUrl(String),
    OpenSettings,
    /// Blink's own settings, on one of their sections ("weather").
    OpenSettingsAt(String),
    RefreshFeeds,
    /// The panel's content changed size or data; redraw.
    Redraw,
    /// Something a screen reader should say (an action was carried out).
    Announce(String),
    /// Offer this peek now (the module built it from an event).
    ShowPeek(PeekView),
    /// Open this module's page by itself and close it again after this many ms (a pointer on it keeps it open).
    AutoOpen(u64),
    /// A reminder: a system notification (when the user wants them).
    Toast { title: String, body: String, url: Option<String> },
    /// The module has something to peek about (see `Module::peek`).
    Peek,
    /// The module wants to be typed into: take the keyboard (what the hotkey does).
    TakeKeyboard,
    /// The typing is over: give the keyboard back (only if the module took it, not if the person opened the notch with the hotkey).
    ReleaseKeyboard,
    /// Open a file or folder the way a double-click would.
    OpenPath(String),
    /// Show a file in its folder.
    RevealPath(String),
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum PointerKind {
    Move,
    Leave,
    Down,
    /// The button was released (a slider drag ends).
    Up,
    Wheel(f32),
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Key {
    Left,
    Right,
    Tab,
    Up,
    Down,
    Home,
    End,
    Enter,
    Space,
    Escape,
    Backspace,
    Delete,
    Char(char),
}

/// How a module answered a key press.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct KeyResult {
    pub handled: bool,
    /// Escape with nothing left to dismiss inside the module: close the notch.
    pub close: bool,
    pub effects: Vec<Effect>,
}

/// The visible part of the panel for one frame.
pub struct PanelFrame<'a> {
    pub theme: &'a Theme,
    /// Fades the contents in and out with the notch.
    pub alpha: f32,
    /// Height of the page in logical px (the viewport).
    pub viewport_h: f32,
    /// Left edge of the panel in window coordinates (logical px).
    pub origin_x: f32,
    /// Top of the page, below the header (window coordinates).
    pub origin_y: f32,
    /// Width of the panel.
    pub width: f32,
    pub keyboard: bool,
}

impl PanelFrame<'_> {
    /// Where a page's own content starts and how wide it is (the sides are left for the page buttons).
    pub fn content_x(&self) -> f32 {
        self.origin_x + PAGE_SIDE
    }

    pub fn content_w(&self) -> f32 {
        self.width - 2.0 * PAGE_SIDE
    }
}

pub trait Module {
    fn id(&self) -> &'static str;

    /// The module is in the middle of a drag: it wants pointer moves and the release even
    /// when the pointer leaves the notch.
    fn captures_pointer(&self) -> bool {
        false
    }

    /// Whether the module has a panel (and so a tab). A module without one only speaks in peeks.
    fn has_panel(&self) -> bool {
        true
    }

    /// The module's options from `host.json`.
    fn set_options(&mut self, _options: &serde_json::Value) {}

    /// Something happened on the computer (power, network, audio, keys).
    fn on_event(&mut self, _event: &crate::events::SystemEvent, _env: &Env) -> Vec<Effect> {
        Vec::new()
    }

    /// The name on the module's tab.
    /// The module is being typed into: the host leaves Left, Right and Tab to it instead of turning the page.
    fn captures_keys(&self) -> bool {
        false
    }

    /// Text pasted from the clipboard.
    fn paste(&mut self, _text: &str) -> Vec<Effect> {
        Vec::new()
    }

    /// Files or folders were dropped on the notch (full paths).
    fn drop_files(&mut self, _paths: &[String]) -> Vec<Effect> {
        Vec::new()
    }

    /// Something is playing (the header's bars move while it is).
    fn playing(&self) -> bool {
        false
    }

    /// The page is in the middle of an animation and wants a frame as soon as the last one is done.
    fn wants_frames(&self) -> bool {
        false
    }

    /// Whether the host draws the round buttons that turn the page. A page showing a sub-page of its own
    /// (with a back button where the arrow would be) says no.
    fn page_arrows(&self) -> bool {
        true
    }

    /// Deadlines this module knows of, for the calendar to mark: when, and how pressing ("later", "soon", "overdue").
    fn due_dates(&mut self, _env: &Env) -> Vec<(i64, &'static str)> {
        Vec::new()
    }

    /// The colour of this page: its title, its highlights.
    fn accent(&self) -> Rgb {
        Rgb(0xf2, 0xf2, 0xf2)
    }

    /// The title in two parts: the second is drawn in the accent colour.
    fn title_parts(&self, lang: blink_core::i18n::Lang) -> (String, String) {
        (self.title(lang), String::new())
    }

    fn title(&self, _lang: blink_core::i18n::Lang) -> String {
        self.id().to_string()
    }

    /// Whether the module is on when the user has not said.
    fn default_enabled(&self) -> bool {
        true
    }

    /// How much this module deserves the resting pill right now: 0 nothing to say, 1 something
    /// calm, 2 or more something worth a glance. The highest wins; ties go to the earlier module.
    fn rest_priority(&mut self, _env: &Env, _settings: &blink_core::settings::Settings) -> u8 {
        1
    }

    /// For tests and tools that need the concrete module.
    fn as_any_mut(&mut self) -> &mut dyn std::any::Any;

    /// The resting pill form for the user's setting and this module's data.
    fn rest_style(&mut self, env: &Env, resting_style: &str, due_horizon_hours: f64) -> crate::shell::RestStyle;

    /// Like `debug`, with the clock available (what the lists hold right now).
    fn debug_now(&mut self, _env: &Env) -> serde_json::Value {
        self.debug()
    }

    /// A snapshot of internal state for the control pipe and tests.
    fn debug(&self) -> serde_json::Value {
        serde_json::Value::Null
    }

    /// Resting pill content.
    fn rest(&mut self, env: &Env) -> RestView;

    /// Content for a peek, if the module wants one right now.
    fn peek(&mut self, env: &Env) -> Option<PeekView>;

    /// Natural height of the open panel (all content plus footer), in logical px.
    fn panel_height(&mut self, env: &Env, kit: &mut Kit) -> f32;

    /// Draw the panel and record where things can be clicked.
    fn draw_panel(&mut self, env: &Env, kit: &mut Kit, frame: &PanelFrame);

    /// A pointer event in window coordinates (logical px).
    fn pointer(&mut self, kind: PointerKind, x: f32, y: f32, env: &Env) -> Vec<Effect>;

    fn key(&mut self, key: Key, env: &Env) -> KeyResult;

    /// Keyboard mode started or ended.
    fn keyboard_mode(&mut self, on: bool, env: &Env);

    /// Fetch fresh data now.
    fn refresh(&mut self) {}

    /// What a screen reader sees, without the root: elements whose parent is `access::ROOT`
    /// or one of the others. `open` says whether the panel is showing.
    fn accessibility(&mut self, _env: &Env, _open: bool) -> Vec<crate::access::Node> {
        Vec::new()
    }

    /// Carry out an action chosen through a screen reader (the same as a click or key would).
    fn perform(&mut self, _action: Action, _env: &Env) -> Vec<Effect> {
        Vec::new()
    }

    /// The personal events file was rewritten.
    fn reload_local(&mut self) {}

    /// The user's settings changed (or arrived).
    fn configure(&mut self, _settings: &blink_core::settings::Settings) {}

    /// Called on every wake: sync what is due, run reminders.
    fn tick(&mut self, _env: &Env) -> Vec<Effect> {
        Vec::new()
    }

    /// When this module next needs to run without any input (countdown ticks).
    fn next_wake(&mut self, env: &Env, panel_open: bool) -> Option<u64>;
}