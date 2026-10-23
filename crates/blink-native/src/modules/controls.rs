//! Controls: the quick settings of Windows in the notch, so the Windows panels are not needed.
//! Volume and the output device, brightness, Wi-Fi and Bluetooth, saved networks, and the
//! projection modes (what Win+P does).
//!
//! The panel only draws and sends commands. What actually talks to Windows is a
//! `ControlsBackend` (`platform::winctl` for real, `FakeControls` in tests and scripted runs),
//! which keeps a shared state that the panel reads each frame and never blocks the UI.

use std::collections::VecDeque;
use std::sync::{Arc, Mutex};

use blink_core::i18n::Lang;
use serde_json::Value;

use super::{Action, Effect, Env, Key, KeyResult, Kit, Module, PanelFrame, PeekView, PointerKind, RestView};
use crate::access::{Node, NodeAction, Role, ROOT};
use crate::events::SystemEvent;
use crate::ui::canvas::Icon;
use crate::ui::text::Face;

// ----------------------------------------------------------------- the model

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Output {
    pub id: String,
    pub name: String,
    pub active: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Network {
    pub name: String,
    pub connected: bool,
}

/// What Win+P offers.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ProjectMode {
    PcOnly,
    Duplicate,
    Extend,
    SecondOnly,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct ControlsState {
    pub volume: u8,
    pub muted: bool,
    pub outputs: Vec<Output>,
    /// None when this computer's screen has no brightness control (a desktop monitor, say).
    pub brightness: Option<u8>,
    /// None when there is no such radio.
    pub wifi: Option<bool>,
    pub bluetooth: Option<bool>,
    pub networks: Vec<Network>,
    pub project: Option<ProjectMode>,
    /// How many screens are connected. The projection modes only mean something with two or more.
    pub displays: u8,
}

#[derive(Clone, Debug, PartialEq)]
pub enum ControlCommand {
    SetVolume(u8),
    VolumeStep(i8),
    ToggleMute,
    SetOutput(String),
    SetBrightness(u8),
    SetWifi(bool),
    SetBluetooth(bool),
    ConnectWifi(String),
    Project(ProjectMode),
    /// Open the cast (wireless display) list.
    Cast,
    /// Open Windows' own Wi-Fi list (for a network you have not used before).
    WifiSettings,
    /// Read everything again.
    Refresh,
}

pub trait ControlsBackend: Send + Sync {
    fn state(&self) -> ControlsState;
    /// Ask for something to be done. Never blocks: the change shows up in `state` when it happened.
    fn send(&self, command: ControlCommand);
}

/// A backend that only remembers: scripted runs and tests drive the panel with it.
pub struct FakeControls {
    state: Mutex<ControlsState>,
    log: Mutex<VecDeque<ControlCommand>>,
}

impl FakeControls {
    pub fn new() -> Arc<FakeControls> {
        Arc::new(FakeControls {
            state: Mutex::new(ControlsState {
                volume: 40,
                muted: false,
                outputs: vec![
                    Output { id: "spk".into(), name: "Speakers".into(), active: true },
                    Output { id: "bt".into(), name: "AirBuds Pro".into(), active: false },
                ],
                brightness: Some(70),
                wifi: Some(true),
                bluetooth: Some(true),
                networks: vec![Network { name: "Campus".into(), connected: true }, Network { name: "Home".into(), connected: false }],
                project: Some(ProjectMode::Extend),
                displays: 2,
            }),
            log: Mutex::new(VecDeque::new()),
        })
    }

    pub fn with_state(state: ControlsState) -> Arc<FakeControls> {
        let f = FakeControls::new();
        *f.state.lock().unwrap() = state;
        f
    }

    /// The commands received so far, oldest first (and forgotten).
    pub fn take_log(&self) -> Vec<ControlCommand> {
        self.log.lock().unwrap().drain(..).collect()
    }
}

impl ControlsBackend for FakeControls {
    fn state(&self) -> ControlsState {
        self.state.lock().unwrap().clone()
    }

    fn send(&self, command: ControlCommand) {
        let mut s = self.state.lock().unwrap();
        match &command {
            ControlCommand::SetVolume(v) => s.volume = (*v).min(100),
            ControlCommand::VolumeStep(d) => s.volume = (i16::from(s.volume) + i16::from(*d)).clamp(0, 100) as u8,
            ControlCommand::ToggleMute => s.muted = !s.muted,
            ControlCommand::SetOutput(id) => {
                for o in s.outputs.iter_mut() {
                    o.active = &o.id == id;
                }
            }
            ControlCommand::SetBrightness(b) => s.brightness = Some((*b).min(100)),
            ControlCommand::SetWifi(on) => s.wifi = Some(*on),
            ControlCommand::SetBluetooth(on) => s.bluetooth = Some(*on),
            ControlCommand::ConnectWifi(name) => {
                for n in s.networks.iter_mut() {
                    n.connected = &n.name == name;
                }
            }
            ControlCommand::Project(m) => s.project = Some(*m),
            ControlCommand::Cast | ControlCommand::WifiSettings | ControlCommand::Refresh => {}
        }
        drop(s);
        self.log.lock().unwrap().push_back(command);
    }
}

/// A name that fits on a chip. Windows names outputs "Speakers (Realtek(R) Audio)" or
/// "Headphones (AirBuds Pro Stereo)": a generic word first, the device in brackets.
pub fn short_output_name(name: &str) -> String {
    let Some(open) = name.find(" (") else { return name.to_string() };
    let (head, rest) = (name[..open].trim(), name[open + 2..].trim_end_matches(')').trim());
    const GENERIC: [&str; 6] = ["speakers", "headphones", "headset", "line out", "digital audio", "output"];
    let generic = GENERIC.iter().any(|g| head.to_lowercase().starts_with(g));
    // "Headphones (AirBuds Pro Stereo)" names the device inside; "Speakers (Realtek(R) Audio)" names a chip.
    let chip = ["realtek", "high definition", "nvidia", "intel", "amd", "usb audio"].iter().any(|c| rest.to_lowercase().starts_with(c));
    if generic && !chip && !rest.is_empty() { rest.to_string() } else { head.to_string() }
}

// ------------------------------------------------------------------- strings

fn word(lang: Lang, key: &str) -> &'static str {
    let id = lang == Lang::Id;
    match key {
        "title" => if id { "Kontrol" } else { "Controls" },
        "volume" => if id { "Volume" } else { "Volume" },
        "brightness" => if id { "Kecerahan" } else { "Brightness" },
        "mute" => if id { "Senyapkan" } else { "Mute" },
        "unmute" => if id { "Bunyikan" } else { "Unmute" },
        "wifi" => "Wi-Fi",
        "bluetooth" => "Bluetooth",
        "more" => if id { "Lainnya…" } else { "More…" },
        "project" => if id { "Proyeksikan" } else { "Project" },
        "pc" => if id { "Hanya PC" } else { "PC screen" },
        "duplicate" => if id { "Duplikat" } else { "Duplicate" },
        "extend" => if id { "Perluas" } else { "Extend" },
        "second" => if id { "Layar kedua" } else { "Second screen" },
        "cast" => if id { "Transmisikan" } else { "Cast" },
        "on" => if id { "nyala" } else { "on" },
        "off" => if id { "mati" } else { "off" },
        "output" => if id { "Keluaran" } else { "Output" },
        _ => "",
    }
}

fn mode_word(m: ProjectMode) -> &'static str {
    match m {
        ProjectMode::PcOnly => "pc",
        ProjectMode::Duplicate => "duplicate",
        ProjectMode::Extend => "extend",
        ProjectMode::SecondOnly => "second",
    }
}

// -------------------------------------------------------------------- module

#[derive(Clone, Debug, PartialEq)]
enum Target {
    Volume,
    Brightness,
    Command(ControlCommand),
}

#[derive(Clone, Debug)]
struct Hit {
    x: f32,
    y: f32,
    w: f32,
    h: f32,
    target: Target,
    /// For sliders: the track's left and right ends.
    track: (f32, f32),
}

impl Hit {
    fn contains(&self, px: f32, py: f32) -> bool {
        px >= self.x && px < self.x + self.w && py >= self.y && py < self.y + self.h
    }
}

const PAD_X: f32 = 16.0;
const PAD_TOP: f32 = 16.0;
const CONTENT_W: f32 = 420.0 - 2.0 * PAD_X;
const ROW: f32 = 34.0;
const CHIP_H: f32 = 26.0;
/// Seconds between re-reading Windows when the panel is reopened.
const REFRESH_EVERY_MS: u64 = 2000;

pub struct ControlsModule {
    backend: Arc<dyn ControlsBackend>,
    hits: Vec<Hit>,
    /// A slider being dragged.
    dragging: Option<Target>,
    selected: usize,
    keyboard: bool,
    last_refresh: Option<u64>,
    pointer: Option<(f32, f32)>,
}

impl ControlsModule {
    pub fn new(backend: Arc<dyn ControlsBackend>) -> Self {
        ControlsModule { backend, hits: Vec::new(), dragging: None, selected: 0, keyboard: false, last_refresh: None, pointer: None }
    }

    fn slider_value(hit: &Hit, x: f32) -> u8 {
        let (a, b) = hit.track;
        (((x - a) / (b - a).max(1.0)).clamp(0.0, 1.0) * 100.0).round() as u8
    }

    fn apply_slider(&self, target: &Target, hit: &Hit, x: f32) {
        let v = Self::slider_value(hit, x);
        match target {
            Target::Volume => self.backend.send(ControlCommand::SetVolume(v)),
            Target::Brightness => self.backend.send(ControlCommand::SetBrightness(v)),
            Target::Command(_) => {}
        }
    }

    /// The buttons in drawing order, for keyboard selection and screen readers.
    fn buttons(&self) -> Vec<&Hit> {
        self.hits.iter().filter(|h| matches!(h.target, Target::Command(_))).collect()
    }
}

/// The result of drawing one chip.
struct ChipLook {
    on: bool,
    selected: bool,
}

impl ControlsModule {
    #[allow(clippy::too_many_arguments)]
    fn chip(&mut self, kit: &mut Kit, frame: &PanelFrame, x: f32, y: f32, label: &str, look: ChipLook, command: ControlCommand, max_w: f32) -> f32 {
        let theme = frame.theme;
        let size = 11.5;
        let px = size * kit.canvas.scale;
        let text = kit.fonts.ellipsize(Face::Sans, label, px, 0.0, (max_w - 20.0).max(20.0) * kit.canvas.scale);
        let w = kit.canvas.measure(kit.fonts, Face::Sans, size, &text, 0.0) + 20.0;
        let over = self.pointer_over(x, y, w, CHIP_H);
        let base = f32::from(theme.line.1) / 255.0 * frame.alpha;
        if look.on {
            kit.canvas.rrect(x, y, w, CHIP_H, CHIP_H / 2.0, theme.fg, 0.16 * frame.alpha + if over { 0.06 } else { 0.0 });
        } else if over {
            kit.canvas.rrect(x, y, w, CHIP_H, CHIP_H / 2.0, theme.line.0, base * 1.5);
        } else {
            kit.canvas.rrect(x, y, w, CHIP_H, CHIP_H / 2.0, theme.line.0, base * 0.7);
        }
        if look.selected {
            // Keyboard selection: an amber underline.
            kit.canvas.rrect(x + 6.0, y + CHIP_H - 3.0, w - 12.0, 2.0, 1.0, theme.soon, frame.alpha);
        }
        let color = if look.on { theme.fg } else { theme.dim };
        let tb = y + kit.fonts.baseline(Face::Sans, size, CHIP_H);
        kit.canvas.text(kit.fonts, Face::Sans, size, x + 10.0, tb, &text, color, frame.alpha, 0.0);
        self.hits.push(Hit { x, y, w, h: CHIP_H, target: Target::Command(command), track: (0.0, 0.0) });
        w
    }

    fn pointer_over(&self, x: f32, y: f32, w: f32, h: f32) -> bool {
        self.pointer.is_some_and(|(px, py)| px >= x && px < x + w && py >= y && py < y + h)
    }

    #[allow(clippy::too_many_arguments)]
    fn slider(&mut self, kit: &mut Kit, frame: &PanelFrame, y: f32, icon: Icon, icon_hit: Option<ControlCommand>, value: u8, target: Target) {
        let theme = frame.theme;
        let x0 = frame.origin_x + PAD_X;
        // The icon (a button when it does something, like mute).
        let over = icon_hit.is_some() && self.pointer_over(x0, y, 28.0, ROW - 6.0);
        if over {
            kit.canvas.rrect(x0, y, 28.0, 28.0, 8.0, theme.line.0, f32::from(theme.line.1) / 255.0 * frame.alpha * 1.5);
        }
        kit.canvas.icon(icon, x0 + 14.0, y + 14.0, 16.0, if value == 0 { theme.dim } else { theme.fg }, frame.alpha);
        if let Some(cmd) = icon_hit {
            self.hits.push(Hit { x: x0, y, w: 28.0, h: 28.0, target: Target::Command(cmd), track: (0.0, 0.0) });
        }
        // The track.
        let (ta, tb) = (x0 + 40.0, x0 + CONTENT_W - 46.0);
        let cy = y + 14.0;
        kit.canvas.bar(ta, tb, cy, 4.0, theme.line.0, f32::from(theme.line.1) / 255.0 * frame.alpha * 1.6);
        let fx = ta + (tb - ta) * f32::from(value) / 100.0;
        if value > 0 {
            kit.canvas.bar(ta, fx, cy, 4.0, theme.fg, frame.alpha);
        }
        let dragging = self.dragging.as_ref() == Some(&target);
        let r = if dragging { 7.0 } else { 6.0 };
        kit.canvas.rrect(fx - r, cy - r, 2.0 * r, 2.0 * r, r, theme.fg, frame.alpha);
        // The number.
        let text = format!("{value}%");
        let tw = kit.canvas.measure(kit.fonts, Face::Mono, 12.0, &text, 0.0);
        let base = y + kit.fonts.baseline(Face::Mono, 12.0, 28.0);
        kit.canvas.text(kit.fonts, Face::Mono, 12.0, x0 + CONTENT_W - tw, base, &text, theme.dim, frame.alpha, 0.0);
        // The hit area is generous: the whole row between the icon and the number.
        self.hits.push(Hit { x: ta - 8.0, y, w: tb - ta + 16.0, h: 28.0, target, track: (ta, tb) });
    }

    fn layout(&mut self, env: &Env, kit: &mut Kit, frame: &PanelFrame, draw: bool) -> f32 {
        let lang = env.t.lang();
        let w = |k| word(lang, k);
        let state = self.backend.state();
        let theme = frame.theme;
        let x0 = frame.origin_x + PAD_X;
        let mut y = PAD_TOP;
        let mut button_index = 0usize;
        let sel = self.selected;
        let keyboard = self.keyboard;
        let mut look = |on: bool| {
            let l = ChipLook { on, selected: keyboard && button_index == sel };
            button_index += 1;
            l
        };

        // Volume: the icon is the mute button.
        if draw {
            let icon = if state.muted || state.volume == 0 { Icon::SpeakerMuted } else { Icon::Speaker };
            self.slider(kit, frame, y, icon, Some(ControlCommand::ToggleMute), if state.muted { 0 } else { state.volume }, Target::Volume);
        }
        y += ROW + 2.0;

        // Where the sound goes.
        if state.outputs.len() > 1 {
            if draw {
                let mut x = x0 + 40.0;
                let right = x0 + CONTENT_W;
                // The one in use first, then the others: choosing one makes it the first, so
                // pressing through them walks the whole list.
                let mut ordered: Vec<&Output> = state.outputs.iter().collect();
                ordered.sort_by_key(|o| !o.active);
                for o in ordered.into_iter().take(3) {
                    let avail = (right - x).max(40.0);
                    let l = look(o.active);
                    x += self.chip(kit, frame, x, y, &short_output_name(&o.name), l, ControlCommand::SetOutput(o.id.clone()), avail.min(170.0)) + 6.0;
                    if x > right - 40.0 {
                        break;
                    }
                }
            }
            y += CHIP_H + 10.0;
        }

        if let Some(b) = state.brightness {
            if draw {
                self.slider(kit, frame, y, Icon::Sun, None, b, Target::Brightness);
            }
            y += ROW + 4.0;
        }

        // Radios.
        if state.wifi.is_some() || state.bluetooth.is_some() {
            if draw {
                let mut x = x0;
                if let Some(on) = state.wifi {
                    let name = state.networks.iter().find(|n| n.connected).map(|n| n.name.as_str());
                    let label = if on { name.map_or(w("wifi").to_string(), |n| format!("{} · {n}", w("wifi"))) } else { format!("{} · {}", w("wifi"), w("off")) };
                    let l = look(on);
                    x += self.chip(kit, frame, x, y, &label, l, ControlCommand::SetWifi(!on), 200.0) + 6.0;
                }
                if let Some(on) = state.bluetooth {
                    let label = if on { w("bluetooth").to_string() } else { format!("{} · {}", w("bluetooth"), w("off")) };
                    let l = look(on);
                    x += self.chip(kit, frame, x, y, &label, l, ControlCommand::SetBluetooth(!on), 160.0) + 6.0;
                }
                let l = look(false);
                self.chip(kit, frame, x, y, w("cast"), l, ControlCommand::Cast, 100.0);
            }
            y += CHIP_H + 10.0;
        }

        // Saved networks, only while Wi-Fi is on.
        if state.wifi == Some(true) && !state.networks.is_empty() {
            if draw {
                let mut x = x0;
                let right = x0 + CONTENT_W;
                for n in state.networks.iter().take(3) {
                    let l = look(n.connected);
                    let avail = (right - x - 70.0).max(60.0);
                    x += self.chip(kit, frame, x, y, &n.name, l, ControlCommand::ConnectWifi(n.name.clone()), avail.min(130.0)) + 6.0;
                }
                let l = look(false);
                self.chip(kit, frame, x, y, w("more"), l, ControlCommand::WifiSettings, 80.0);
            }
            y += CHIP_H + 12.0;
        }

        // Projection: only with a second screen to project to.
        if state.displays >= 2 {
            if draw {
                let base = y + kit.fonts.baseline(Face::Sans, 11.0, 16.0);
                kit.canvas.text(kit.fonts, Face::Sans, 11.0, x0, base, w("project"), theme.dim, frame.alpha, 0.0);
            }
            y += 18.0;
            if draw {
                let mut x = x0;
                for m in [ProjectMode::PcOnly, ProjectMode::Duplicate, ProjectMode::Extend, ProjectMode::SecondOnly] {
                    let l = look(state.project == Some(m));
                    x += self.chip(kit, frame, x, y, w(mode_word(m)), l, ControlCommand::Project(m), 130.0) + 6.0;
                }
            }
            y += CHIP_H + PAD_TOP;
        } else {
            y += PAD_TOP - 6.0;
        }
        y
    }
}

impl Module for ControlsModule {
    fn id(&self) -> &'static str {
        "controls"
    }

    fn title(&self, lang: Lang) -> String {
        word(lang, "title").to_string()
    }

    fn as_any_mut(&mut self) -> &mut dyn std::any::Any {
        self
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

    fn on_event(&mut self, _event: &SystemEvent, _env: &Env) -> Vec<Effect> {
        Vec::new()
    }

    fn panel_height(&mut self, env: &Env, kit: &mut Kit) -> f32 {
        // About to be shown: read Windows again, unless it was just read.
        if self.last_refresh.is_none_or(|t| env.now_ms.saturating_sub(t) > REFRESH_EVERY_MS) {
            self.last_refresh = Some(env.now_ms);
            self.backend.send(ControlCommand::Refresh);
        }
        let theme = crate::ui::theme::Theme::dark();
        let frame = PanelFrame { theme: &theme, alpha: 1.0, viewport_h: crate::shell::OPEN_MAX_H, origin_x: 0.0, keyboard: false };
        self.layout(env, kit, &frame, false)
    }

    fn draw_panel(&mut self, env: &Env, kit: &mut Kit, frame: &PanelFrame) {
        self.hits.clear();
        self.layout(env, kit, frame, true);
    }

    fn pointer(&mut self, kind: PointerKind, x: f32, y: f32, _env: &Env) -> Vec<Effect> {
        match kind {
            PointerKind::Leave => {
                self.pointer = None;
                vec![Effect::Redraw]
            }
            PointerKind::Move => {
                self.pointer = Some((x, y));
                if let Some(target) = self.dragging.clone() {
                    if let Some(hit) = self.hits.iter().find(|h| h.target == target).cloned() {
                        self.apply_slider(&target, &hit, x);
                    }
                }
                vec![Effect::Redraw]
            }
            PointerKind::Down => {
                let Some(hit) = self.hits.iter().rev().find(|h| h.contains(x, y)).cloned() else { return Vec::new() };
                match &hit.target {
                    Target::Volume | Target::Brightness => {
                        self.dragging = Some(hit.target.clone());
                        self.apply_slider(&hit.target, &hit, x);
                    }
                    Target::Command(cmd) => self.backend.send(cmd.clone()),
                }
                vec![Effect::Redraw]
            }
            PointerKind::Up => {
                self.dragging = None;
                vec![Effect::Redraw]
            }
            PointerKind::Wheel(delta) => {
                // The wheel over a slider nudges it (delta is in pixels, positive scrolls up).
                let over = self.hits.iter().find(|h| h.contains(x, y) && matches!(h.target, Target::Volume | Target::Brightness)).cloned();
                if let Some(hit) = over {
                    let step = if delta > 0.0 { 5 } else { -5 };
                    match hit.target {
                        Target::Volume => self.backend.send(ControlCommand::VolumeStep(step)),
                        Target::Brightness => {
                            let now = self.backend.state().brightness.unwrap_or(50) as i16;
                            self.backend.send(ControlCommand::SetBrightness((now + i16::from(step)).clamp(0, 100) as u8));
                        }
                        Target::Command(_) => {}
                    }
                }
                vec![Effect::Redraw]
            }
        }
    }

    fn captures_pointer(&self) -> bool {
        self.dragging.is_some()
    }

    fn key(&mut self, key: Key, _env: &Env) -> KeyResult {
        let count = self.buttons().len().max(1);
        let handled = |effects: Vec<Effect>| KeyResult { handled: true, close: false, effects };
        match key {
            Key::Escape => KeyResult { handled: false, close: true, effects: Vec::new() },
            Key::Down => {
                self.selected = (self.selected + 1) % count;
                handled(vec![Effect::Redraw])
            }
            Key::Up => {
                self.selected = (self.selected + count - 1) % count;
                handled(vec![Effect::Redraw])
            }
            Key::Home => {
                self.selected = 0;
                handled(vec![Effect::Redraw])
            }
            Key::End => {
                self.selected = count - 1;
                handled(vec![Effect::Redraw])
            }
            Key::Enter | Key::Space => {
                if let Some(Target::Command(cmd)) = self.buttons().get(self.selected).map(|h| h.target.clone()) {
                    self.backend.send(cmd);
                }
                handled(vec![Effect::Redraw])
            }
            // Plus and minus (and the volume-style = and -) nudge the volume.
            Key::Char('+') | Key::Char('=') => {
                self.backend.send(ControlCommand::VolumeStep(5));
                handled(vec![Effect::Redraw])
            }
            Key::Char('-') => {
                self.backend.send(ControlCommand::VolumeStep(-5));
                handled(vec![Effect::Redraw])
            }
            Key::Char('m') | Key::Char('M') => {
                self.backend.send(ControlCommand::ToggleMute);
                handled(vec![Effect::Redraw])
            }
            _ => KeyResult::default(),
        }
    }

    fn keyboard_mode(&mut self, on: bool, _env: &Env) {
        self.keyboard = on;
        self.selected = 0;
    }

    fn next_wake(&mut self, _env: &Env, _panel_open: bool) -> Option<u64> {
        None
    }

    fn perform(&mut self, action: Action, _env: &Env) -> Vec<Effect> {
        if let Action::Control(cmd) = action {
            self.backend.send(cmd.clone());
            let lang_free = match cmd {
                ControlCommand::ToggleMute => "Mute",
                ControlCommand::SetWifi(_) => "Wi-Fi",
                ControlCommand::SetBluetooth(_) => "Bluetooth",
                _ => "",
            };
            if !lang_free.is_empty() {
                return vec![Effect::Announce(lang_free.to_string())];
            }
        }
        Vec::new()
    }

    fn debug_now(&mut self, _env: &Env) -> serde_json::Value {
        serde_json::json!({
            "hits": self.hits.iter().map(|h| serde_json::json!({ "x": h.x, "y": h.y, "w": h.w, "h": h.h, "target": format!("{:?}", h.target) })).collect::<Vec<_>>(),
            "dragging": self.dragging.is_some(),
        })
    }

    fn accessibility(&mut self, env: &Env, open: bool) -> Vec<Node> {
        if !open {
            return Vec::new();
        }
        let lang = env.t.lang();
        let w = |k| word(lang, k);
        let state = self.backend.state();
        let mut out = Vec::new();
        let mut key = 300;
        let mut push = |out: &mut Vec<Node>, name: String, cmd: ControlCommand, rect: (f32, f32, f32, f32)| {
            let mut n = Node::new(key, ROOT, Role::Button, name);
            n.action = Some(NodeAction::Module(Action::Control(cmd)));
            n.rect = rect;
            out.push(n);
            key += 1;
        };
        let rect_of = |hits: &[Hit], pred: &dyn Fn(&Hit) -> bool| hits.iter().find(|h| pred(h)).map_or((0.0, 0.0, 0.0, 0.0), |h| (h.x, h.y, h.w, h.h));
        let vol_rect = rect_of(&self.hits, &|h| h.target == Target::Volume);
        push(&mut out, format!("{} {}%{}", w("volume"), state.volume, if state.muted { format!(", {}", w("mute")) } else { String::new() }), ControlCommand::ToggleMute, vol_rect);
        push(&mut out, format!("{} +5", w("volume")), ControlCommand::VolumeStep(5), vol_rect);
        push(&mut out, format!("{} -5", w("volume")), ControlCommand::VolumeStep(-5), vol_rect);
        for o in &state.outputs {
            let r = rect_of(&self.hits, &|h| h.target == Target::Command(ControlCommand::SetOutput(o.id.clone())));
            push(&mut out, format!("{}: {}{}", w("output"), short_output_name(&o.name), if o.active { " ✓" } else { "" }), ControlCommand::SetOutput(o.id.clone()), r);
        }
        if let Some(b) = state.brightness {
            let r = rect_of(&self.hits, &|h| h.target == Target::Brightness);
            push(&mut out, format!("{} {b}%: +10", w("brightness")), ControlCommand::SetBrightness((b + 10).min(100)), r);
            push(&mut out, format!("{} {b}%: -10", w("brightness")), ControlCommand::SetBrightness(b.saturating_sub(10)), r);
        }
        if let Some(on) = state.wifi {
            let r = rect_of(&self.hits, &|h| h.target == Target::Command(ControlCommand::SetWifi(!on)));
            push(&mut out, format!("{}: {}", w("wifi"), if on { w("on") } else { w("off") }), ControlCommand::SetWifi(!on), r);
        }
        if let Some(on) = state.bluetooth {
            let r = rect_of(&self.hits, &|h| h.target == Target::Command(ControlCommand::SetBluetooth(!on)));
            push(&mut out, format!("{}: {}", w("bluetooth"), if on { w("on") } else { w("off") }), ControlCommand::SetBluetooth(!on), r);
        }
        for n in state.networks.iter().take(3) {
            let r = rect_of(&self.hits, &|h| h.target == Target::Command(ControlCommand::ConnectWifi(n.name.clone())));
            push(&mut out, format!("{}{}", n.name, if n.connected { " ✓" } else { "" }), ControlCommand::ConnectWifi(n.name.clone()), r);
        }
        for m in [ProjectMode::PcOnly, ProjectMode::Duplicate, ProjectMode::Extend, ProjectMode::SecondOnly].into_iter().filter(|_| state.displays >= 2) {
            let r = rect_of(&self.hits, &|h| h.target == Target::Command(ControlCommand::Project(m)));
            push(&mut out, format!("{}: {}{}", w("project"), w(mode_word(m)), if state.project == Some(m) { " ✓" } else { "" }), ControlCommand::Project(m), r);
        }
        let _ = Value::Null;
        out
    }
}
