//! The controls panel with a fake backend: what a click, a drag, a key or the wheel asks Windows to do.

mod common;

use blink_core::settings::normalize_settings;
use blink_native::app::App;
use blink_native::clock::Clock;
use blink_native::modules::controls::{ControlCommand, ControlsBackend, ControlsModule, ControlsState, FakeControls, ProjectMode};
use blink_native::modules::deadlines::{DeadlinesModule, DemoSource};
use blink_native::modules::{Key, Module, PointerKind};
use common::*;
use std::sync::{Arc, Mutex};

fn app_over(backend: Arc<dyn ControlsBackend>, options: Option<serde_json::Value>) -> App {
    let mut controls = ControlsModule::new(backend);
    if let Some(o) = options {
        controls.set_options(&o);
    }
    let modules: Vec<Box<dyn Module>> = vec![Box::new(DeadlinesModule::new(Box::new(DemoSource::new(WALL_BASE)))), Box::new(controls)];
    let mut app = App::new(Clock::manual(WALL_BASE), normalize_settings(None), tz(), 1.0, "en", modules).expect("app");
    app.tick();
    open_by_hover(&mut app);
    app.switch_to(1);
    settle(&mut app);
    app.render();
    app
}

fn app_with_controls() -> (App, Arc<FakeControls>) {
    let fake = FakeControls::new();
    let app = app_over(fake.clone(), None);
    fake.take_log();
    (app, fake)
}

fn names(app: &mut App) -> Vec<String> {
    app.render();
    app.accessibility().nodes.iter().map(|n| n.name.clone()).collect()
}

/// Where a control is drawn (window coordinates), found the way a screen reader would find it.
fn rect_of(app: &mut App, name: &str) -> (f32, f32, f32, f32) {
    app.render();
    app.accessibility().nodes.iter().find(|n| n.name == name || n.name.starts_with(&format!("{name} "))).unwrap_or_else(|| panic!("no control named {name}")).rect
}

fn center(r: (f32, f32, f32, f32)) -> (f32, f32) {
    (r.0 + r.2 / 2.0, r.1 + r.3 / 2.0)
}

fn click(app: &mut App, name: &str) {
    let (x, y) = center(rect_of(app, name));
    app.pointer(PointerKind::Move, x, y);
    app.pointer(PointerKind::Down, x, y);
    app.pointer(PointerKind::Up, x, y);
    app.render();
}

#[test]
fn the_panel_shows_what_windows_reports() {
    let (mut app, _fake) = app_with_controls();
    let names = names(&mut app);
    for want in [
        "Output: Speakers", "Brightness 70%: +10", "Wi-Fi: on", "Wi-Fi networks", "Bluetooth: on", "Microphone: live", "Cast", "Project: Extend", "Lock", "Screenshot",
        "Settings",
    ] {
        assert!(names.iter().any(|n| n == want), "no {want:?} in {names:?}");
    }
    assert!(names.iter().any(|n| n.starts_with("Volume 40%")), "{names:?}");
    assert_eq!(blink_native::modules::controls::short_output_name("Headphones (AirBuds Pro Stereo)"), "AirBuds Pro Stereo");
    assert_eq!(blink_native::modules::controls::short_output_name("Speakers (Realtek(R) Audio)"), "Speakers");
    assert_eq!(blink_native::modules::controls::short_output_name("SteelSeries Sonar - Gaming (SteelSeries Sonar Virtual Audio Device)"), "SteelSeries Sonar - Gaming");
    assert_eq!(app.active_module(), "controls");
}

#[test]
fn a_click_on_the_volume_track_sets_the_volume_there_and_a_drag_follows() {
    let (mut app, fake) = app_with_controls();
    let r = rect_of(&mut app, "Volume 40%");
    // The whole tile is the track.
    let (a, b) = (r.0, r.0 + r.2);
    let y = r.1 + r.3 / 2.0;
    app.pointer(PointerKind::Move, a + (b - a) * 0.5, y);
    app.pointer(PointerKind::Down, a + (b - a) * 0.5, y);
    assert_eq!(fake.take_log(), vec![ControlCommand::SetVolume(50)]);
    assert!(app.pointer_captured(), "the window layer is told to keep the mouse during a drag");
    app.pointer(PointerKind::Move, a + (b - a) * 0.75, y);
    app.pointer(PointerKind::Move, b + 300.0, y + 200.0); // far outside the notch: still follows, clamped
    app.pointer(PointerKind::Up, b + 300.0, y + 200.0);
    assert_eq!(fake.take_log(), vec![ControlCommand::SetVolume(75), ControlCommand::SetVolume(100)]);
    assert!(!app.pointer_captured());
    app.pointer(PointerKind::Move, a + 5.0, y);
    assert!(fake.take_log().is_empty(), "moving without the button does nothing");
}

#[test]
fn the_speaker_button_mutes_and_the_wheel_nudges() {
    let (mut app, fake) = app_with_controls();
    let r = rect_of(&mut app, "Volume 40%");
    app.pointer(PointerKind::Move, r.0 + 100.0, r.1 + 10.0);
    app.pointer(PointerKind::Wheel(48.0), r.0 + 100.0, r.1 + r.3 / 2.0);
    assert_eq!(fake.take_log(), vec![ControlCommand::VolumeStep(5)]);
    app.pointer(PointerKind::Down, r.0 + 20.0, r.1 + r.3 / 2.0); // the speaker icon at the left of the tile
    assert_eq!(fake.take_log(), vec![ControlCommand::ToggleMute]);
}

#[test]
fn tiles_toggle_radios_the_microphone_and_run_actions() {
    let (mut app, fake) = app_with_controls();
    for name in ["Bluetooth: on", "Wi-Fi: on", "Microphone: live", "Project: Extend", "Cast", "Lock", "Screenshot", "Settings"] {
        click(&mut app, name);
    }
    assert_eq!(
        fake.take_log(),
        vec![
            ControlCommand::SetBluetooth(false),
            ControlCommand::SetWifi(false),
            ControlCommand::ToggleMic,
            ControlCommand::Project(ProjectMode::SecondOnly),
            ControlCommand::Cast,
            ControlCommand::Lock,
            ControlCommand::Snip,
            ControlCommand::OpenSettings,
        ]
    );
    let names = names(&mut app);
    for want in ["Bluetooth: off", "Wi-Fi: off", "Microphone: muted", "Project: Second screen"] {
        assert!(names.iter().any(|n| n == want), "the panel follows the new state: {want:?} in {names:?}");
    }
    assert!(!names.iter().any(|n| n == "Wi-Fi networks"), "the networks page is only there while Wi-Fi is on");
}

#[test]
fn the_output_page_lists_the_devices_and_choosing_one_goes_back() {
    let (mut app, fake) = app_with_controls();
    click(&mut app, "Output: Speakers");
    assert!(fake.take_log().is_empty(), "opening the page asks Windows for nothing");
    let on_page = names(&mut app);
    assert!(on_page.iter().any(|n| n == "Back") && on_page.iter().any(|n| n == "Output: Speakers ✓") && on_page.iter().any(|n| n == "Output: AirBuds Pro"), "{on_page:?}");
    assert!(!on_page.iter().any(|n| n.starts_with("Volume")), "the page replaces the panel");
    click(&mut app, "Output: AirBuds Pro");
    assert_eq!(fake.take_log(), vec![ControlCommand::SetOutput("bt".into())]);
    let back = names(&mut app);
    assert!(back.iter().any(|n| n.starts_with("Volume 40%")), "choosing returns to the main page: {back:?}");
    assert!(back.iter().any(|n| n == "Output: AirBuds Pro"));
}

#[test]
fn the_network_page_lists_what_is_in_range_and_connects() {
    let (mut app, fake) = app_with_controls();
    click(&mut app, "Wi-Fi networks");
    assert!(fake.take_log().contains(&ControlCommand::ScanWifi), "opening the page asks the card to look again");
    let on_page = names(&mut app);
    for want in ["Campus ✓", "Home", "Cafe, open", "Library, password needed", "More…"] {
        assert!(on_page.iter().any(|n| n == want), "no {want:?} in {on_page:?}");
    }
    click(&mut app, "More…");
    assert_eq!(fake.take_log(), vec![ControlCommand::WifiSettings]);
    click(&mut app, "Cafe, open");
    assert_eq!(fake.take_log(), vec![ControlCommand::ConnectWifi("Cafe".into())]);
    assert!(names(&mut app).iter().any(|n| n.starts_with("Volume 40%")), "back on the main page");
    click(&mut app, "Wi-Fi networks");
    click(&mut app, "Home");
    assert_eq!(fake.take_log().last(), Some(&ControlCommand::ConnectWifi("Home".into())));
}

#[test]
fn every_page_is_as_tall_as_the_main_one_so_opening_a_page_never_leaves_the_pointer_outside() {
    let (mut app, _fake) = app_with_controls();
    let main = app.state_json()["target"]["h"].as_f64().unwrap();
    click(&mut app, "Wi-Fi networks");
    advance(&mut app, 400);
    assert_eq!(app.state_json()["target"]["h"].as_f64().unwrap(), main, "network page");
    click(&mut app, "Back");
    advance(&mut app, 400);
    click(&mut app, "Output: Speakers");
    advance(&mut app, 400);
    assert_eq!(app.state_json()["target"]["h"].as_f64().unwrap(), main, "output page");
    // The pointer stayed where the click was: still inside the notch, which is still open.
    assert_eq!(app.state_json()["mode"], "open");
}

#[test]
fn the_keyboard_walks_the_buttons_adjusts_levels_and_goes_back_with_escape() {
    let (mut app, fake) = app_with_controls();
    app.hotkey();
    settle(&mut app);
    app.switch_to(1);
    app.render();
    fake.take_log();
    app.key(Key::Char('+'));
    app.key(Key::Char('-'));
    app.key(Key::Char('m'));
    app.key(Key::Char(']'));
    assert_eq!(fake.take_log(), vec![ControlCommand::VolumeStep(5), ControlCommand::VolumeStep(-5), ControlCommand::ToggleMute, ControlCommand::SetBrightness(75)]);
    // Buttons in order: the speaker (mute), the output row, then the tiles. Two Downs reach the Wi-Fi tile.
    app.key(Key::Down);
    app.key(Key::Down);
    app.render();
    app.key(Key::Enter);
    assert_eq!(fake.take_log(), vec![ControlCommand::SetWifi(false)]);
    // The output row opens its page; Escape comes back instead of closing the notch.
    app.key(Key::Home);
    app.key(Key::Down);
    app.render();
    app.key(Key::Enter);
    assert!(names(&mut app).iter().any(|n| n == "Back"));
    app.key(Key::Escape);
    assert!(names(&mut app).iter().any(|n| n.starts_with("Volume")), "Escape on a page goes back");
    assert_eq!(app.active_module(), "controls");
}

#[test]
fn opening_the_panel_asks_windows_to_re_read_everything_but_not_over_and_over() {
    let fake = FakeControls::new();
    let modules: Vec<Box<dyn Module>> = vec![Box::new(DeadlinesModule::new(Box::new(DemoSource::new(WALL_BASE)))), Box::new(ControlsModule::new(fake.clone()))];
    let mut app = App::new(Clock::manual(WALL_BASE), normalize_settings(None), tz(), 1.0, "en", modules).expect("app");
    app.tick();
    open_by_hover(&mut app);
    app.switch_to(1);
    app.render();
    let first = fake.take_log().iter().filter(|c| **c == ControlCommand::Refresh).count();
    assert_eq!(first, 1);
    for _ in 0..20 {
        app.render();
    }
    assert_eq!(fake.take_log().iter().filter(|c| **c == ControlCommand::Refresh).count(), 0, "drawing frames never reads Windows");
    // While the panel stays open it keeps up with changes made elsewhere, every couple of seconds.
    advance(&mut app, 2_100);
    app.render();
    assert!(fake.take_log().iter().filter(|c| **c == ControlCommand::Refresh).count() >= 1, "an open panel refreshes itself");
}

#[test]
fn a_screen_without_brightness_control_and_no_radios_shows_none_of_them() {
    let fake = FakeControls::with_state(ControlsState { volume: 10, brightness: None, wifi: None, bluetooth: None, mic_muted: None, displays: 1, ..ControlsState::default() });
    let mut app = app_over(fake, None);
    let names = names(&mut app);
    assert!(!names.iter().any(|n| n.starts_with("Brightness") || n.starts_with("Wi-Fi") || n.starts_with("Bluetooth") || n.starts_with("Microphone")), "{names:?}");
    assert!(names.iter().any(|n| n.starts_with("Volume 10%")));
    assert!(!names.iter().any(|n| n.starts_with("Project")), "no second screen: nothing to project to");
    assert!(names.iter().any(|n| n == "Lock") && names.iter().any(|n| n == "Screenshot"), "the actions are always there: {names:?}");
}

#[test]
fn the_tiles_option_chooses_which_tiles_show_and_in_what_order() {
    let fake = FakeControls::new();
    let mut app = app_over(fake, Some(serde_json::json!({ "tiles": ["lock", "wifi", "nonsense", "lock"], "output": false })));
    let names = names(&mut app);
    let tiles: Vec<&String> = names.iter().filter(|n| matches!(n.as_str(), "Lock" | "Wi-Fi: on" | "Screenshot" | "Bluetooth: on")).collect();
    assert_eq!(tiles, vec!["Lock", "Wi-Fi: on"], "only the chosen ones, in the chosen order: {names:?}");
    assert!(!names.iter().any(|n| n.starts_with("Output")), "the output row can be switched off");
}

/// A backend that never changes its answer, like Windows when a radio is slow to switch.
struct Stubborn(Mutex<ControlsState>);

impl ControlsBackend for Stubborn {
    fn state(&self) -> ControlsState {
        self.0.lock().unwrap().clone()
    }
    fn send(&self, _command: ControlCommand) {}
}

#[test]
fn a_pressed_toggle_shows_as_done_until_windows_answers_or_gives_up() {
    let backend = Arc::new(Stubborn(Mutex::new(FakeControls::new().state())));
    let mut app = app_over(backend, None);
    click(&mut app, "Wi-Fi: on");
    assert!(names(&mut app).iter().any(|n| n == "Wi-Fi: off"), "shown as pressed at once");
    advance(&mut app, 3_000);
    app.tick();
    assert!(names(&mut app).iter().any(|n| n == "Wi-Fi: on"), "after a few seconds Windows' own answer wins");
}
