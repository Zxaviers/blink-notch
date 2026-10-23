//! Controls: the quick settings of Windows in the notch, so the Windows panels are not needed.
//!
//! Two big sliders (volume, brightness) that fill like a progress bar, the output device, and a
//! grid of tiles: Wi-Fi, Bluetooth, microphone, cast, projection, lock, screenshot and settings.
//! A tile with more to choose from (Wi-Fi) has a small arrow that opens a page of its own; so does
//! the output row. Which tiles show, and in what order, is the `tiles` option in `host.json`.
//!
//! The panel only draws and sends commands. What actually talks to Windows is a
//! `ControlsBackend` (`platform::winctl` for real, `FakeControls` in tests and scripted runs),
//! which keeps a shared state that the panel reads each frame and never blocks the UI.

use std::collections::VecDeque;
use std::sync::{Arc, Mutex};

use blink_core::i18n::Lang;
use serde_json::Value;

use super::{Action, Effect, Env, Key, KeyResult, Kit, Module, PanelFrame, PeekView, PointerKind, RestView, CONTENT_H, PAGE_SIDE};
use crate::access::{Node, NodeAction, Role, ROOT};
use crate::events::SystemEvent;
use crate::ui::canvas::Icon;
use crate::ui::text::Face;
use crate::ui::theme::Rgb;

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
    /// Signal strength, 0 to 100.
    pub signal: u8,
    /// Needs a password.
    pub secured: bool,
    /// Windows already has this network's password (or it is open and was used before).
    pub saved: bool,
}

/// What Win+P offers.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ProjectMode {
    PcOnly,
    Duplicate,
    Extend,
    SecondOnly,
}

impl ProjectMode {
    /// The mode one press of the Project tile moves to.
    pub fn next(self) -> ProjectMode {
        match self {
            ProjectMode::PcOnly => ProjectMode::Duplicate,
            ProjectMode::Duplicate => ProjectMode::Extend,
            ProjectMode::Extend => ProjectMode::SecondOnly,
            ProjectMode::SecondOnly => ProjectMode::PcOnly,
        }
    }
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
    /// Whether the default microphone is muted. None when there is no microphone.
    pub mic_muted: Option<bool>,
    pub networks: Vec<Network>,
    pub project: Option<ProjectMode>,
    /// How many screens are connected. The projection modes only mean something with two or more.
    pub displays: u8,
}

/// The pages of the panel.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ControlsPage {
    Main,
    Output,
    Network,
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
    ToggleMic,
    /// Connect to a network: a saved one directly, an open one by making a profile for it; one that
    /// needs a password is left to Windows' own list.
    ConnectWifi(String),
    /// Look for networks in range again.
    ScanWifi,
    Project(ProjectMode),
    /// Open the cast (wireless display) list.
    Cast,
    /// Lock the computer.
    Lock,
    /// Start the screenshot tool (the one on Win+Shift+S).
    Snip,
    /// Open Windows' settings.
    OpenSettings,
    /// Open Windows' own Wi-Fi list (for a network you have not used before).
    WifiSettings,
    /// Read everything again.
    Refresh,
    /// Only the panel itself acts on this (it changes the page); a backend ignores it.
    ShowPage(ControlsPage),
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
                mic_muted: Some(false),
                networks: vec![
                    Network { name: "Campus".into(), connected: true, signal: 80, secured: true, saved: true },
                    Network { name: "Home".into(), connected: false, signal: 55, secured: true, saved: true },
                    Network { name: "Cafe".into(), connected: false, signal: 40, secured: false, saved: false },
                    Network { name: "Library".into(), connected: false, signal: 20, secured: true, saved: false },
                ],
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
            ControlCommand::ToggleMic => s.mic_muted = s.mic_muted.map(|m| !m),
            ControlCommand::ConnectWifi(name) => {
                for n in s.networks.iter_mut() {
                    n.connected = &n.name == name;
                }
            }
            ControlCommand::Project(m) => s.project = Some(*m),
            ControlCommand::Cast
            | ControlCommand::Lock
            | ControlCommand::Snip
            | ControlCommand::OpenSettings
            | ControlCommand::WifiSettings
            | ControlCommand::ScanWifi
            | ControlCommand::Refresh
            | ControlCommand::ShowPage(_) => {}
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

// --------------------------------------------------------------------- tiles

/// The tiles of the grid.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tile {
    Wifi,
    Bluetooth,
    Mic,
    Cast,
    Project,
    Lock,
    Snip,
    Settings,
}

impl Tile {
    pub const ALL: [Tile; 8] = [Tile::Wifi, Tile::Bluetooth, Tile::Mic, Tile::Cast, Tile::Project, Tile::Lock, Tile::Snip, Tile::Settings];

    pub fn id(self) -> &'static str {
        match self {
            Tile::Wifi => "wifi",
            Tile::Bluetooth => "bluetooth",
            Tile::Mic => "mic",
            Tile::Cast => "cast",
            Tile::Project => "project",
            Tile::Lock => "lock",
            Tile::Snip => "snip",
            Tile::Settings => "settings",
        }
    }

    pub fn from_id(id: &str) -> Option<Tile> {
        Tile::ALL.into_iter().find(|t| t.id() == id)
    }
}

/// A tile as it is shown right now.
struct TileView {
    on: bool,
    icon: Icon,
    label: String,
    sub: String,
    command: ControlCommand,
    /// The page the little arrow opens, if the tile has one.
    detail: Option<ControlsPage>,
    /// What a screen reader says.
    name: String,
    detail_name: String,
}

/// A toggle the user just pressed, shown as done until Windows catches up.
#[derive(Clone, Copy, Debug)]
struct Pending {
    tile: Tile,
    on: bool,
    until: u64,
}

/// How long a pressed toggle is shown as done before Windows' own answer is believed.
const PENDING_MS: u64 = 2500;
/// How long a slider keeps showing where it was put (Windows takes a moment to report it).
const LEVEL_HOLD_MS: u64 = 600;
/// Seconds between re-reading Windows while the panel is open.
const REFRESH_EVERY_MS: u64 = 2000;

// ------------------------------------------------------------------- strings

fn word(lang: Lang, key: &str) -> &'static str {
    let id = lang == Lang::Id;
    match key {
        "title" => if id { "Kontrol" } else { "Controls" },
        "volume" => "Volume",
        "brightness" => if id { "Kecerahan" } else { "Brightness" },
        "muted" => if id { "Senyap" } else { "Muted" },
        "mute" => if id { "Senyapkan" } else { "Mute" },
        "wifi" => "Wi-Fi",
        "bluetooth" => "Bluetooth",
        "mic" => if id { "Mikrofon" } else { "Microphone" },
        "mic_live" => if id { "Aktif" } else { "Live" },
        "project" => if id { "Proyeksi" } else { "Project" },
        "pc" => if id { "Hanya PC" } else { "PC screen" },
        "duplicate" => if id { "Duplikat" } else { "Duplicate" },
        "extend" => if id { "Perluas" } else { "Extend" },
        "second" => if id { "Layar kedua" } else { "Second screen" },
        "cast" => if id { "Transmisi" } else { "Cast" },
        "cast_hint" => "Win+K",
        "lock" => if id { "Kunci" } else { "Lock" },
        "lock_hint" => "Win+L",
        "snip" => if id { "Tangkapan" } else { "Screenshot" },
        "snip_hint" => "Win+Shift+S",
        "settings" => if id { "Pengaturan" } else { "Settings" },
        "settings_hint" => "Windows",
        "on" => if id { "Nyala" } else { "On" },
        "off" => if id { "Mati" } else { "Off" },
        "output" => if id { "Keluaran" } else { "Output" },
        "networks" => if id { "Jaringan Wi-Fi" } else { "Wi-Fi networks" },
        "more" => if id { "Lainnya…" } else { "More…" },
        "searching" => if id { "Mencari jaringan…" } else { "Looking for networks…" },
        "secured" => if id { "perlu kata sandi" } else { "password needed" },
        "open" => if id { "terbuka" } else { "open" },
        "back" => if id { "Kembali" } else { "Back" },
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

/// The left column (two sliders and the output row) and the gap before the round buttons.
const LEFT_W: f32 = 196.0;
const SIDE_GAP: f32 = 22.0;
const SLIDER_H: f32 = 38.0;
const OUTPUT_H: f32 = 30.0;
const V_GAP: f32 = 8.0;
const ORB_R: f32 = 19.0;
const ORB_COLS: usize = 4;
const ORB_CELL_W: f32 = (crate::shell::OPEN_W - 2.0 * PAGE_SIDE - LEFT_W - SIDE_GAP) / ORB_COLS as f32;
const CELL_H: f32 = 60.0;
const LIST_ROW_H: f32 = 34.0;
const LIST_GAP: f32 = 6.0;
/// How many choices a page can show (two columns of three).
const MAX_ROWS: usize = 6;

pub struct ControlsModule {
    backend: Arc<dyn ControlsBackend>,
    hits: Vec<Hit>,
    /// A slider being dragged.
    dragging: Option<Target>,
    selected: usize,
    keyboard: bool,
    last_refresh: Option<u64>,
    pointer: Option<(f32, f32)>,
    page: ControlsPage,
    /// Which tiles show, in order (`tiles` in host.json).
    tiles: Vec<Tile>,
    /// Whether the output row shows (`output` in host.json).
    show_output: bool,
    pending: Vec<Pending>,
    /// Where each slider was last put (volume, brightness): the value and until when it is believed.
    levels: [Option<(u8, u64)>; 2],
    /// When the panel last drew (so a refresh is only asked for while someone is looking).
    last_draw: Option<u64>,
    /// Buttons drawn so far this frame (the keyboard selection counts them).
    drawn_buttons: usize,
}

impl ControlsModule {
    pub fn new(backend: Arc<dyn ControlsBackend>) -> Self {
        ControlsModule {
            backend,
            hits: Vec::new(),
            dragging: None,
            selected: 0,
            keyboard: false,
            last_refresh: None,
            pointer: None,
            page: ControlsPage::Main,
            tiles: Tile::ALL.to_vec(),
            show_output: true,
            pending: Vec::new(),
            levels: [None, None],
            last_draw: None,
            drawn_buttons: 0,
        }
    }

    fn slider_index(target: &Target) -> usize {
        usize::from(*target == Target::Brightness)
    }

    fn slider_value(hit: &Hit, x: f32) -> u8 {
        let (a, b) = hit.track;
        (((x - a) / (b - a).max(1.0)).clamp(0.0, 1.0) * 100.0).round() as u8
    }

    fn apply_slider(&mut self, target: &Target, hit: &Hit, x: f32, now: u64) {
        let v = Self::slider_value(hit, x);
        self.levels[Self::slider_index(target)] = Some((v, now + LEVEL_HOLD_MS));
        match target {
            Target::Volume => self.backend.send(ControlCommand::SetVolume(v)),
            Target::Brightness => self.backend.send(ControlCommand::SetBrightness(v)),
            Target::Command(_) => {}
        }
    }

    /// What a slider shows: where the user put it a moment ago, otherwise what Windows says.
    fn shown_level(&self, target: &Target, actual: u8, now: u64) -> u8 {
        match self.levels[Self::slider_index(target)] {
            Some((v, until)) if self.dragging.as_ref() == Some(target) || until > now => v,
            _ => actual,
        }
    }

    /// The buttons in drawing order, for keyboard selection and screen readers.
    fn buttons(&self) -> Vec<&Hit> {
        self.hits.iter().filter(|h| matches!(h.target, Target::Command(_))).collect()
    }

    fn pointer_over(&self, x: f32, y: f32, w: f32, h: f32) -> bool {
        self.pointer.is_some_and(|(px, py)| px >= x && px < x + w && py >= y && py < y + h)
    }

    /// Do what a button asks: change the page, or tell Windows.
    fn activate(&mut self, command: ControlCommand, now: u64) -> Vec<Effect> {
        let state = self.backend.state();
        let mut set_pending = |tile: Tile, on: bool| {
            self.pending.retain(|p| p.tile != tile);
            self.pending.push(Pending { tile, on, until: now + PENDING_MS });
        };
        match &command {
            ControlCommand::ShowPage(page) => {
                self.page = *page;
                self.selected = 0;
                if *page == ControlsPage::Network {
                    self.backend.send(ControlCommand::ScanWifi);
                }
                return vec![Effect::Redraw];
            }
            ControlCommand::SetWifi(on) => set_pending(Tile::Wifi, *on),
            ControlCommand::SetBluetooth(on) => set_pending(Tile::Bluetooth, *on),
            // The tile is lit while the microphone is live, so muting makes it unlit.
            ControlCommand::ToggleMic => {
                if let Some(muted) = state.mic_muted {
                    set_pending(Tile::Mic, muted);
                }
            }
            ControlCommand::SetOutput(_) | ControlCommand::ConnectWifi(_) => {
                self.page = ControlsPage::Main;
                self.selected = 0;
            }
            _ => {}
        }
        self.backend.send(command);
        vec![Effect::Redraw]
    }

    /// Whether Windows says a tile is on (None: the tile has nothing to show on this computer).
    fn tile_state(tile: Tile, state: &ControlsState) -> Option<bool> {
        match tile {
            Tile::Wifi => state.wifi,
            Tile::Bluetooth => state.bluetooth,
            Tile::Mic => state.mic_muted.map(|m| !m),
            Tile::Project => (state.displays >= 2).then(|| state.project.is_some_and(|m| m != ProjectMode::PcOnly)),
            Tile::Cast | Tile::Lock | Tile::Snip | Tile::Settings => Some(false),
        }
    }

    /// Forget pressed toggles Windows has answered (or that took too long).
    fn settle(&mut self, state: &ControlsState, now: u64) {
        self.pending.retain(|p| p.until > now && Self::tile_state(p.tile, state) != Some(p.on));
    }

    fn tile_views(&self, state: &ControlsState, lang: Lang) -> Vec<TileView> {
        let w = |k| word(lang, k);
        let mut out = Vec::new();
        for &tile in &self.tiles {
            let Some(actual) = Self::tile_state(tile, state) else { continue };
            let on = self.pending.iter().find(|p| p.tile == tile).map_or(actual, |p| p.on);
            let state_word = |on: bool| if on { w("on") } else { w("off") };
            let view = match tile {
                Tile::Wifi => {
                    let ssid = state.networks.iter().find(|n| n.connected).map(|n| n.name.clone());
                    TileView {
                        on,
                        icon: Icon::Wifi,
                        label: w("wifi").to_string(),
                        sub: if on { ssid.unwrap_or_else(|| w("on").to_string()) } else { w("off").to_string() },
                        command: ControlCommand::SetWifi(!on),
                        detail: on.then_some(ControlsPage::Network),
                        name: format!("{}: {}", w("wifi"), state_word(on).to_lowercase()),
                        detail_name: w("networks").to_string(),
                    }
                }
                Tile::Bluetooth => TileView {
                    on,
                    icon: Icon::Bluetooth,
                    label: w("bluetooth").to_string(),
                    sub: state_word(on).to_string(),
                    command: ControlCommand::SetBluetooth(!on),
                    detail: None,
                    name: format!("{}: {}", w("bluetooth"), state_word(on).to_lowercase()),
                    detail_name: String::new(),
                },
                Tile::Mic => TileView {
                    on,
                    icon: if on { Icon::Mic } else { Icon::MicMuted },
                    label: w("mic").to_string(),
                    sub: if on { w("mic_live").to_string() } else { w("muted").to_string() },
                    command: ControlCommand::ToggleMic,
                    detail: None,
                    name: format!("{}: {}", w("mic"), if on { w("mic_live") } else { w("muted") }.to_lowercase()),
                    detail_name: String::new(),
                },
                Tile::Cast => TileView {
                    on: false,
                    icon: Icon::Cast,
                    label: w("cast").to_string(),
                    sub: w("cast_hint").to_string(),
                    command: ControlCommand::Cast,
                    detail: None,
                    name: w("cast").to_string(),
                    detail_name: String::new(),
                },
                Tile::Project => {
                    let mode = state.project.unwrap_or(ProjectMode::PcOnly);
                    TileView {
                        on,
                        icon: Icon::Monitor,
                        label: w("project").to_string(),
                        sub: w(mode_word(mode)).to_string(),
                        command: ControlCommand::Project(mode.next()),
                        detail: None,
                        name: format!("{}: {}", w("project"), w(mode_word(mode))),
                        detail_name: String::new(),
                    }
                }
                Tile::Lock => TileView {
                    on: false,
                    icon: Icon::Lock,
                    label: w("lock").to_string(),
                    sub: w("lock_hint").to_string(),
                    command: ControlCommand::Lock,
                    detail: None,
                    name: w("lock").to_string(),
                    detail_name: String::new(),
                },
                Tile::Snip => TileView {
                    on: false,
                    icon: Icon::Crop,
                    label: w("snip").to_string(),
                    sub: w("snip_hint").to_string(),
                    command: ControlCommand::Snip,
                    detail: None,
                    name: w("snip").to_string(),
                    detail_name: String::new(),
                },
                Tile::Settings => TileView {
                    on: false,
                    icon: Icon::Gear,
                    label: w("settings").to_string(),
                    sub: w("settings_hint").to_string(),
                    command: ControlCommand::OpenSettings,
                    detail: None,
                    name: w("settings").to_string(),
                    detail_name: String::new(),
                },
            };
            out.push(view);
        }
        out
    }
}

// ------------------------------------------------------------------- drawing

/// Text on one line inside a box of height `box_h` starting at `box_y`, cut with "…" at `max_w`.
#[allow(clippy::too_many_arguments)]
fn put(kit: &mut Kit, face: Face, size: f32, x: f32, box_y: f32, box_h: f32, max_w: f32, text: &str, color: Rgb, alpha: f32) -> f32 {
    let px = size * kit.canvas.scale;
    let cut = kit.fonts.ellipsize(face, text, px, 0.0, max_w.max(8.0) * kit.canvas.scale);
    let base = box_y + kit.fonts.baseline(face, size, box_h);
    kit.canvas.text(kit.fonts, face, size, x, base, &cut, color, alpha, 0.0)
}

/// The same text centred on `cx`.
#[allow(clippy::too_many_arguments)]
fn put_centered(kit: &mut Kit, face: Face, size: f32, cx: f32, box_y: f32, box_h: f32, max_w: f32, text: &str, color: Rgb, alpha: f32) {
    let px = size * kit.canvas.scale;
    let cut = kit.fonts.ellipsize(face, text, px, 0.0, max_w.max(8.0) * kit.canvas.scale);
    let w = kit.canvas.measure(kit.fonts, face, size, &cut, 0.0);
    let base = box_y + kit.fonts.baseline(face, size, box_h);
    kit.canvas.text(kit.fonts, face, size, cx - w / 2.0, base, &cut, color, alpha, 0.0);
}

/// A picture for an output: headphones for what you wear, a speaker for the rest.
fn output_icon(name: &str) -> Icon {
    let n = name.to_lowercase();
    if ["headphone", "headset", "buds", "airpods", "earphone", "earbud"].iter().any(|w| n.contains(w)) { Icon::Headphones } else { Icon::Speaker }
}

fn mix(a: Rgb, b: Rgb, t: f32) -> Rgb {
    let m = |x: u8, y: u8| (f32::from(x) + (f32::from(y) - f32::from(x)) * t).round() as u8;
    Rgb(m(a.0, b.0), m(a.1, b.1), m(a.2, b.2))
}

const CYAN: Rgb = Rgb(0x38, 0xd6, 0xe8);
const AMBER: Rgb = Rgb(0xf5, 0xb8, 0x4a);

/// The icon, name and number of a slider, in one colour (drawn twice: dark over the fill, light elsewhere).
#[allow(clippy::too_many_arguments)]
fn slider_content(kit: &mut Kit, x: f32, y: f32, w: f32, h: f32, icon: Icon, label: &str, value: &str, color: Rgb, alpha: f32) {
    kit.canvas.icon(icon, x + 20.0, y + h / 2.0, 16.0, color, alpha);
    put(kit, Face::SansSemibold, 12.0, x + 38.0, y, h, w - 100.0, label, color, alpha);
    let tw = kit.canvas.measure(kit.fonts, Face::Mono, 11.5, value, 0.0);
    let base = y + kit.fonts.baseline(Face::Mono, 11.5, h);
    kit.canvas.text(kit.fonts, Face::Mono, 11.5, x + w - 14.0 - tw, base, value, color, alpha, 0.0);
}

impl ControlsModule {
    /// A soft rounded surface; hovered it brightens, selected by the keyboard it gets an amber ring.
    #[allow(clippy::too_many_arguments)]
    fn surface(&self, kit: &mut Kit, frame: &PanelFrame, x: f32, y: f32, w: f32, h: f32, r: f32, on: bool, selected: bool) {
        let theme = frame.theme;
        let over = self.pointer_over(x, y, w, h);
        if selected {
            kit.canvas.rrect(x - 2.0, y - 2.0, w + 4.0, h + 4.0, r + 2.0, theme.soon, frame.alpha);
            kit.canvas.rrect(x, y, w, h, r, theme.bg, frame.alpha);
        }
        if on {
            kit.canvas.gradient_rrect(x, y, w, h, r, mix(CYAN, Rgb(255, 255, 255), 0.4), CYAN, true, frame.alpha * if over { 1.0 } else { 0.95 });
        } else {
            kit.canvas.rrect(x, y, w, h, r, theme.fg, frame.alpha * (0.09 + if over { 0.07 } else { 0.0 }));
        }
    }

    fn push_button(&mut self, x: f32, y: f32, w: f32, h: f32, command: ControlCommand) -> bool {
        let selected = self.keyboard && self.drawn_buttons == self.selected;
        self.drawn_buttons += 1;
        self.hits.push(Hit { x, y, w, h, target: Target::Command(command), track: (0.0, 0.0) });
        selected
    }

    /// A slider that fills with colour like a progress bar. The icon at the left is a button when `icon_command` is given.
    #[allow(clippy::too_many_arguments)]
    fn slider_tile(&mut self, kit: &mut Kit, frame: &PanelFrame, x: f32, y: f32, color: Rgb, icon: Icon, label: &str, value: u8, value_text: &str, faded: bool, target: Target, icon_command: Option<ControlCommand>) {
        let theme = frame.theme;
        let (w, h, r) = (LEFT_W, SLIDER_H, 13.0);
        self.surface(kit, frame, x, y, w, h, r, false, false);
        let fill = w * f32::from(value) / 100.0;
        let ink = Rgb(0x04, 0x10, 0x14);
        if value > 0 {
            kit.canvas.push_clip(x, y, fill, h);
            let a = frame.alpha * if faded { 0.55 } else { 1.0 };
            kit.canvas.gradient_rrect(x, y, w, h, r, mix(color, Rgb(255, 255, 255), 0.45), color, false, a);
            kit.canvas.glow(x + fill, y + h / 2.0, 34.0, mix(color, Rgb(255, 255, 255), 0.5), a * 0.55);
            slider_content(kit, x, y, w, h, icon, label, value_text, ink, frame.alpha);
            kit.canvas.pop_clip();
        }
        kit.canvas.push_clip(x + fill, y, w - fill, h);
        slider_content(kit, x, y, w, h, icon, label, value_text, if value == 0 { theme.dim } else { theme.fg }, frame.alpha);
        kit.canvas.pop_clip();
        self.hits.push(Hit { x, y, w, h, target, track: (x, x + w) });
        if let Some(cmd) = icon_command {
            if self.pointer_over(x, y, 36.0, h) {
                kit.canvas.rrect(x + 4.0, y + 4.0, 30.0, h - 8.0, 9.0, if value > 0 { ink } else { theme.fg }, frame.alpha * 0.12);
            }
            self.push_button(x, y, 36.0, h, cmd);
        }
    }

    /// The row showing which output is in use; it opens the page to choose another.
    fn output_row(&mut self, kit: &mut Kit, frame: &PanelFrame, x: f32, y: f32, state: &ControlsState) {
        let theme = frame.theme;
        let (w, h) = (LEFT_W, OUTPUT_H);
        let many = state.outputs.len() > 1;
        let selected = many && self.keyboard && self.drawn_buttons == self.selected;
        self.surface(kit, frame, x, y, w, h, 12.0, false, selected);
        let active = state.outputs.iter().find(|o| o.active).or(state.outputs.first());
        kit.canvas.icon(active.map_or(Icon::Speaker, |o| output_icon(&o.name)), x + 20.0, y + h / 2.0, 15.0, theme.dim, frame.alpha);
        let name = active.map_or(String::new(), |o| short_output_name(&o.name));
        put(kit, Face::Sans, 12.0, x + 38.0, y, h, w - 70.0, &name, theme.fg, frame.alpha);
        if many {
            kit.canvas.icon(Icon::ChevronRight, x + w - 16.0, y + h / 2.0, 12.0, theme.dim, frame.alpha);
            self.push_button(x, y, w, h, ControlCommand::ShowPage(ControlsPage::Output));
        }
    }

    /// One round button of the grid, with its name under it.
    fn orb(&mut self, kit: &mut Kit, frame: &PanelFrame, cx: f32, y: f32, tv: &TileView) {
        let theme = frame.theme;
        let cell_w = ORB_CELL_W;
        let cy = y + ORB_R + 2.0;
        let selected = self.keyboard && self.drawn_buttons == self.selected;
        let over = self.pointer_over(cx - cell_w / 2.0, y, cell_w, CELL_H);
        if tv.on {
            kit.canvas.glow(cx, cy, ORB_R + 15.0, CYAN, frame.alpha * 0.34);
            kit.canvas.orb(cx, cy, ORB_R, mix(CYAN, Rgb(255, 255, 255), 0.6), CYAN, frame.alpha);
        } else {
            kit.canvas.circle(cx, cy, ORB_R, theme.fg, frame.alpha * (0.10 + if over { 0.08 } else { 0.0 }));
        }
        if selected {
            kit.canvas.arc(cx, cy, ORB_R + 3.5, 2.0, 0.0, 360.0, theme.soon, frame.alpha);
        }
        kit.canvas.icon(tv.icon, cx, cy, 17.0, if tv.on { Rgb(0x04, 0x10, 0x14) } else { theme.fg }, frame.alpha);
        let label = if tv.on && !tv.sub.is_empty() && tv.label == "Wi-Fi" { &tv.sub } else { &tv.label };
        put_centered(kit, Face::Sans, 10.5, cx, y + 2.0 * ORB_R + 6.0, 14.0, cell_w - 4.0, label, if tv.on { theme.fg } else { theme.dim }, frame.alpha);
        self.push_button(cx - cell_w / 2.0, y, cell_w, CELL_H, tv.command.clone());
        if let Some(page) = tv.detail {
            // A small badge on the corner opens the list.
            let (bx, by) = (cx + ORB_R - 2.0, cy - ORB_R + 2.0);
            let badge_over = self.pointer_over(bx - 9.0, by - 9.0, 18.0, 18.0);
            kit.canvas.circle(bx, by, 7.5, theme.fg, frame.alpha * if badge_over { 1.0 } else { 0.9 });
            kit.canvas.icon(Icon::ChevronRight, bx, by, 8.0, theme.bg, frame.alpha);
            self.push_button(bx - 9.0, by - 9.0, 18.0, 18.0, ControlCommand::ShowPage(page));
        }
    }

    /// The round back button where the page arrow would be, at the left of a page of choices.
    fn back_button(&mut self, kit: &mut Kit, frame: &PanelFrame) {
        let theme = frame.theme;
        let (cx, cy) = (frame.origin_x + 20.0, frame.origin_y + CONTENT_H / 2.0);
        let selected = self.keyboard && self.drawn_buttons == self.selected;
        let over = self.pointer_over(cx - 15.0, cy - 15.0, 30.0, 30.0);
        if selected {
            kit.canvas.arc(cx, cy, 16.0, 2.0, 0.0, 360.0, theme.soon, frame.alpha);
        }
        kit.canvas.circle(cx, cy, 12.5, theme.fg, frame.alpha * if over { 0.4 } else { 0.28 });
        kit.canvas.icon(Icon::ChevronLeft, cx, cy, 14.0, theme.fg, frame.alpha);
        self.push_button(cx - 15.0, cy - 15.0, 30.0, 30.0, ControlCommand::ShowPage(ControlsPage::Main));
    }

    /// One choice in a page: an icon, a name, and a mark when it is the one in use.
    #[allow(clippy::too_many_arguments)]
    fn choice_row(&mut self, kit: &mut Kit, frame: &PanelFrame, x: f32, y: f32, w: f32, icon: Icon, name: &str, active: bool, command: ControlCommand) {
        let theme = frame.theme;
        let h = LIST_ROW_H;
        let selected = self.keyboard && self.drawn_buttons == self.selected;
        self.surface(kit, frame, x, y, w, h, 12.0, active, selected);
        let ink = if active { Rgb(0x04, 0x10, 0x14) } else { theme.fg };
        kit.canvas.icon(icon, x + 20.0, y + h / 2.0, 15.0, ink, frame.alpha);
        put(kit, Face::Sans, 12.0, x + 38.0, y, h, w - 72.0, name, ink, frame.alpha);
        if active {
            kit.canvas.icon(Icon::Check, x + w - 18.0, y + h / 2.0, 13.0, ink, frame.alpha);
        }
        self.push_button(x, y, w, h, command);
    }

    /// One network in range: its name, how strong it is, and whether it needs a password.
    fn network_row(&mut self, kit: &mut Kit, frame: &PanelFrame, x: f32, y: f32, w: f32, n: &Network) {
        let theme = frame.theme;
        let h = LIST_ROW_H;
        let selected = self.keyboard && self.drawn_buttons == self.selected;
        self.surface(kit, frame, x, y, w, h, 12.0, n.connected, selected);
        let ink = if n.connected { Rgb(0x04, 0x10, 0x14) } else { theme.fg };
        let quiet = if n.connected { ink } else { theme.dim };
        kit.canvas.icon(Icon::Wifi, x + 20.0, y + h / 2.0, 15.0, ink, frame.alpha);
        // The right end: a check when connected, a lock when a password is needed, the signal bars.
        let mut right = x + w - 14.0;
        if n.connected {
            kit.canvas.icon(Icon::Check, right - 2.0, y + h / 2.0, 12.0, ink, frame.alpha);
            right -= 22.0;
        } else if n.secured && !n.saved {
            kit.canvas.icon(Icon::Lock, right - 3.0, y + h / 2.0, 11.0, quiet, frame.alpha);
            right -= 22.0;
        }
        let lit = (u32::from(n.signal) + 24) / 25;
        for i in 0..4u32 {
            let bar_h = 4.0 + 2.6 * i as f32;
            let bx = right - 17.0 + 4.4 * i as f32;
            kit.canvas.rrect(bx, y + h / 2.0 + 6.0 - bar_h, 2.6, bar_h, 1.0, quiet, frame.alpha * if i < lit { 0.95 } else { 0.28 });
        }
        put(kit, Face::Sans, 12.0, x + 38.0, y, h, right - 22.0 - (x + 38.0), &n.name, ink, frame.alpha);
        self.push_button(x, y, w, h, ControlCommand::ConnectWifi(n.name.clone()));
    }

    fn layout(&mut self, env: &Env, kit: &mut Kit, frame: &PanelFrame, draw: bool) -> f32 {
        let lang = env.t.lang();
        let state = self.backend.state();
        self.settle(&state, env.now_ms);
        if draw {
            match self.page {
                ControlsPage::Main => self.layout_main(env, kit, frame, &state, lang),
                ControlsPage::Output => self.layout_output(kit, frame, &state),
                ControlsPage::Network => self.layout_network(kit, frame, &state, lang),
            }
        }
        CONTENT_H
    }

    fn layout_main(&mut self, env: &Env, kit: &mut Kit, frame: &PanelFrame, state: &ControlsState, lang: Lang) {
        let w = |k| word(lang, k);
        let now = env.now_ms;
        let x0 = frame.content_x();
        let mut y = frame.origin_y + 2.0;

        // Volume: the icon is the mute button.
        let value = self.shown_level(&Target::Volume, state.volume, now);
        let muted = state.muted || value == 0;
        let text = if state.muted { w("muted").to_string() } else { format!("{value}%") };
        let icon = if muted { Icon::SpeakerMuted } else { Icon::Speaker };
        self.slider_tile(kit, frame, x0, y, CYAN, icon, w("volume"), if state.muted { 0 } else { value }, &text, false, Target::Volume, Some(ControlCommand::ToggleMute));
        y += SLIDER_H + V_GAP;

        if let Some(b) = state.brightness {
            let value = self.shown_level(&Target::Brightness, b, now);
            self.slider_tile(kit, frame, x0, y, AMBER, Icon::Sun, w("brightness"), value, &format!("{value}%"), false, Target::Brightness, None);
            y += SLIDER_H + V_GAP;
        }
        if self.show_output && !state.outputs.is_empty() {
            self.output_row(kit, frame, x0, y, state);
        }

        // The round buttons.
        let views = self.tile_views(state, lang);
        let gx = x0 + LEFT_W + SIDE_GAP;
        for (i, tv) in views.iter().enumerate() {
            let (col, row) = (i % ORB_COLS, i / ORB_COLS);
            let cx = gx + ORB_CELL_W * (col as f32 + 0.5);
            self.orb(kit, frame, cx, frame.origin_y + 2.0 + row as f32 * (CELL_H + 2.0), tv);
        }
    }

    fn list_origin(frame: &PanelFrame) -> (f32, f32, f32) {
        let x = frame.content_x();
        let col_w = (frame.content_w() - 12.0) / 2.0;
        (x, frame.origin_y + 4.0, col_w)
    }

    fn layout_output(&mut self, kit: &mut Kit, frame: &PanelFrame, state: &ControlsState) {
        self.back_button(kit, frame);
        let (x, y0, col_w) = Self::list_origin(frame);
        // The one in use first.
        let mut ordered: Vec<&Output> = state.outputs.iter().collect();
        ordered.sort_by_key(|o| !o.active);
        for (i, o) in ordered.into_iter().take(MAX_ROWS).enumerate() {
            let (col, row) = (i / 3, i % 3);
            let rx = x + col as f32 * (col_w + 12.0);
            self.choice_row(kit, frame, rx, y0 + row as f32 * (LIST_ROW_H + LIST_GAP), col_w, output_icon(&o.name), &short_output_name(&o.name), o.active, ControlCommand::SetOutput(o.id.clone()));
        }
    }

    fn layout_network(&mut self, kit: &mut Kit, frame: &PanelFrame, state: &ControlsState, lang: Lang) {
        self.back_button(kit, frame);
        let (x, y0, col_w) = Self::list_origin(frame);
        let shown = state.networks.len().min(MAX_ROWS - 1);
        for (i, n) in state.networks.iter().take(shown).enumerate() {
            let (col, row) = (i / 3, i % 3);
            let rx = x + col as f32 * (col_w + 12.0);
            self.network_row(kit, frame, rx, y0 + row as f32 * (LIST_ROW_H + LIST_GAP), col_w, n);
        }
        // "More…" takes the last place: Windows' own list, for networks not heard here or not yet used.
        let (col, row) = (shown / 3, shown % 3);
        let rx = x + col as f32 * (col_w + 12.0);
        if shown == 0 {
            let theme = frame.theme;
            put(kit, Face::Sans, 12.0, rx + 4.0, y0 + (LIST_ROW_H + LIST_GAP), LIST_ROW_H, col_w, word(lang, "searching"), theme.dim, frame.alpha);
        }
        self.choice_row(kit, frame, rx, y0 + row as f32 * (LIST_ROW_H + LIST_GAP), col_w, Icon::ArrowUpRight, word(lang, "more"), false, ControlCommand::WifiSettings);
    }
}

impl Module for ControlsModule {
    fn id(&self) -> &'static str {
        "controls"
    }

    fn title(&self, lang: Lang) -> String {
        word(lang, "title").to_string()
    }

    fn accent(&self) -> Rgb {
        CYAN
    }

    fn page_arrows(&self) -> bool {
        self.page == ControlsPage::Main
    }

    fn title_parts(&self, lang: Lang) -> (String, String) {
        let id = lang == Lang::Id;
        let (a, b) = match (self.page, id) {
            (ControlsPage::Main, false) => ("Con", "trols"),
            (ControlsPage::Main, true) => ("Kon", "trol"),
            (ControlsPage::Output, false) => ("Out", "put"),
            (ControlsPage::Output, true) => ("Kelu", "aran"),
            (ControlsPage::Network, _) => ("Wi-", "Fi"),
        };
        (a.to_string(), b.to_string())
    }

    fn as_any_mut(&mut self) -> &mut dyn std::any::Any {
        self
    }

    fn set_options(&mut self, options: &Value) {
        self.tiles = match options.get("tiles").and_then(Value::as_array) {
            Some(list) => {
                let mut tiles: Vec<Tile> = Vec::new();
                for t in list.iter().filter_map(Value::as_str).filter_map(Tile::from_id) {
                    if !tiles.contains(&t) {
                        tiles.push(t);
                    }
                }
                tiles
            }
            None => Tile::ALL.to_vec(),
        };
        self.show_output = options.get("output").and_then(Value::as_bool).unwrap_or(true);
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
        let frame = PanelFrame { theme: &theme, alpha: 1.0, viewport_h: crate::shell::OPEN_MAX_H, origin_x: 0.0, origin_y: 0.0, width: crate::shell::OPEN_W, keyboard: false };
        self.layout(env, kit, &frame, false)
    }

    fn draw_panel(&mut self, env: &Env, kit: &mut Kit, frame: &PanelFrame) {
        self.hits.clear();
        self.drawn_buttons = 0;
        self.last_draw = Some(env.now_ms);
        self.layout(env, kit, frame, true);
        // The selection can point past the last button after the page changed.
        if self.selected >= self.drawn_buttons {
            self.selected = self.drawn_buttons.saturating_sub(1);
        }
    }

    fn pointer(&mut self, kind: PointerKind, x: f32, y: f32, env: &Env) -> Vec<Effect> {
        match kind {
            PointerKind::Leave => {
                self.pointer = None;
                // Closing the notch ends the visit: it opens on the main page next time.
                if self.dragging.is_none() {
                    self.page = ControlsPage::Main;
                }
                vec![Effect::Redraw]
            }
            PointerKind::Move => {
                self.pointer = Some((x, y));
                if let Some(target) = self.dragging.clone() {
                    if let Some(hit) = self.hits.iter().find(|h| h.target == target).cloned() {
                        self.apply_slider(&target, &hit, x, env.now_ms);
                    }
                }
                vec![Effect::Redraw]
            }
            PointerKind::Down => {
                let Some(hit) = self.hits.iter().rev().find(|h| h.contains(x, y)).cloned() else { return Vec::new() };
                match &hit.target {
                    Target::Volume | Target::Brightness => {
                        self.dragging = Some(hit.target.clone());
                        self.apply_slider(&hit.target, &hit, x, env.now_ms);
                        vec![Effect::Redraw]
                    }
                    Target::Command(cmd) => self.activate(cmd.clone(), env.now_ms),
                }
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

    fn key(&mut self, key: Key, env: &Env) -> KeyResult {
        let count = self.buttons().len().max(1);
        let handled = |effects: Vec<Effect>| KeyResult { handled: true, close: false, effects };
        match key {
            // On a page of choices Escape goes back; on the main page it closes the notch.
            Key::Escape if self.page != ControlsPage::Main => handled(self.activate(ControlCommand::ShowPage(ControlsPage::Main), env.now_ms)),
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
                let target = self.buttons().get(self.selected).map(|h| h.target.clone());
                match target {
                    Some(Target::Command(cmd)) => handled(self.activate(cmd, env.now_ms)),
                    _ => handled(vec![Effect::Redraw]),
                }
            }
            // Plus and minus (and the volume-style = and -) nudge the volume; [ and ] the brightness.
            Key::Char('+') | Key::Char('=') => {
                self.backend.send(ControlCommand::VolumeStep(5));
                handled(vec![Effect::Redraw])
            }
            Key::Char('-') => {
                self.backend.send(ControlCommand::VolumeStep(-5));
                handled(vec![Effect::Redraw])
            }
            Key::Char(c @ ('[' | ']')) => {
                if let Some(b) = self.backend.state().brightness {
                    let next = (i16::from(b) + if c == ']' { 5 } else { -5 }).clamp(0, 100) as u8;
                    self.levels[1] = Some((next, env.now_ms + LEVEL_HOLD_MS));
                    self.backend.send(ControlCommand::SetBrightness(next));
                }
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
        if !on {
            self.page = ControlsPage::Main;
        }
    }

    fn tick(&mut self, env: &Env) -> Vec<Effect> {
        // While someone is looking, keep up with changes made elsewhere (a radio switched, a mic muted).
        let looking = self.last_draw.is_some_and(|t| env.now_ms.saturating_sub(t) < 2 * REFRESH_EVERY_MS);
        if looking && self.last_refresh.is_none_or(|t| env.now_ms.saturating_sub(t) >= REFRESH_EVERY_MS) {
            self.last_refresh = Some(env.now_ms);
            self.backend.send(ControlCommand::Refresh);
            return vec![Effect::Redraw];
        }
        if self.pending.iter().any(|p| p.until <= env.now_ms) || self.levels.iter().flatten().any(|(_, until)| *until <= env.now_ms) {
            return vec![Effect::Redraw];
        }
        Vec::new()
    }

    fn next_wake(&mut self, env: &Env, panel_open: bool) -> Option<u64> {
        if !panel_open {
            return None;
        }
        let refresh = self.last_refresh.map_or(env.now_ms + REFRESH_EVERY_MS, |t| t + REFRESH_EVERY_MS);
        let expiry = self.pending.iter().map(|p| p.until).chain(self.levels.iter().flatten().map(|(_, until)| *until)).filter(|t| *t > env.now_ms).min();
        Some(expiry.map_or(refresh, |e| e.min(refresh)).max(env.now_ms + 1))
    }

    fn perform(&mut self, action: Action, env: &Env) -> Vec<Effect> {
        if let Action::Control(cmd) = action {
            let announce = match &cmd {
                ControlCommand::ToggleMute => "Mute",
                ControlCommand::ToggleMic => "Microphone",
                ControlCommand::SetWifi(_) => "Wi-Fi",
                ControlCommand::SetBluetooth(_) => "Bluetooth",
                ControlCommand::Lock => "Lock",
                _ => "",
            };
            let mut effects = self.activate(cmd, env.now_ms);
            if !announce.is_empty() {
                effects.push(Effect::Announce(announce.to_string()));
            }
            return effects;
        }
        Vec::new()
    }

    fn debug_now(&mut self, _env: &Env) -> serde_json::Value {
        serde_json::json!({
            "page": format!("{:?}", self.page),
            "hits": self.hits.iter().map(|h| serde_json::json!({ "x": h.x, "y": h.y, "w": h.w, "h": h.h, "target": format!("{:?}", h.target) })).collect::<Vec<_>>(),
            "dragging": self.dragging.is_some(),
            "tiles": self.tiles.iter().map(|t| t.id()).collect::<Vec<_>>(),
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
        let cmd_rect = |hits: &[Hit], cmd: &ControlCommand| rect_of(hits, &|h| h.target == Target::Command(cmd.clone()));
        match self.page {
            ControlsPage::Main => {
                let vol_rect = rect_of(&self.hits, &|h| h.target == Target::Volume);
                push(&mut out, format!("{} {}%{}", w("volume"), state.volume, if state.muted { format!(", {}", w("mute")) } else { String::new() }), ControlCommand::ToggleMute, vol_rect);
                push(&mut out, format!("{} +5", w("volume")), ControlCommand::VolumeStep(5), vol_rect);
                push(&mut out, format!("{} -5", w("volume")), ControlCommand::VolumeStep(-5), vol_rect);
                if self.show_output && state.outputs.len() > 1 {
                    let active = state.outputs.iter().find(|o| o.active).or(state.outputs.first()).map_or(String::new(), |o| short_output_name(&o.name));
                    let cmd = ControlCommand::ShowPage(ControlsPage::Output);
                    let r = cmd_rect(&self.hits, &cmd);
                    push(&mut out, format!("{}: {active}", w("output")), cmd, r);
                }
                if let Some(b) = state.brightness {
                    let r = rect_of(&self.hits, &|h| h.target == Target::Brightness);
                    push(&mut out, format!("{} {b}%: +10", w("brightness")), ControlCommand::SetBrightness((b + 10).min(100)), r);
                    push(&mut out, format!("{} {b}%: -10", w("brightness")), ControlCommand::SetBrightness(b.saturating_sub(10)), r);
                }
                for tv in self.tile_views(&state, lang) {
                    let r = cmd_rect(&self.hits, &tv.command);
                    push(&mut out, tv.name.clone(), tv.command.clone(), r);
                    if let Some(page) = tv.detail {
                        let cmd = ControlCommand::ShowPage(page);
                        let r = cmd_rect(&self.hits, &cmd);
                        push(&mut out, tv.detail_name.clone(), cmd, r);
                    }
                }
            }
            ControlsPage::Output => {
                let back = ControlCommand::ShowPage(ControlsPage::Main);
                let r = cmd_rect(&self.hits, &back);
                push(&mut out, w("back").to_string(), back, r);
                for o in state.outputs.iter().take(MAX_ROWS) {
                    let cmd = ControlCommand::SetOutput(o.id.clone());
                    let r = cmd_rect(&self.hits, &cmd);
                    push(&mut out, format!("{}: {}{}", w("output"), short_output_name(&o.name), if o.active { " ✓" } else { "" }), cmd, r);
                }
            }
            ControlsPage::Network => {
                let back = ControlCommand::ShowPage(ControlsPage::Main);
                let r = cmd_rect(&self.hits, &back);
                push(&mut out, w("back").to_string(), back, r);
                for n in state.networks.iter().take(MAX_ROWS) {
                    let cmd = ControlCommand::ConnectWifi(n.name.clone());
                    let r = cmd_rect(&self.hits, &cmd);
                    let note = if n.connected {
                        " ✓".to_string()
                    } else if n.secured && !n.saved {
                        format!(", {}", w("secured"))
                    } else if !n.secured && !n.saved {
                        format!(", {}", w("open"))
                    } else {
                        String::new()
                    };
                    push(&mut out, format!("{}{note}", n.name), cmd, r);
                }
                let r = cmd_rect(&self.hits, &ControlCommand::WifiSettings);
                push(&mut out, w("more").to_string(), ControlCommand::WifiSettings, r);
            }
        }
        out
    }
}
