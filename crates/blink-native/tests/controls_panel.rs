//! The controls panel with a fake backend: what a click, a drag, a key or the wheel asks Windows to do.

mod common;

use blink_core::settings::normalize_settings;
use blink_native::app::App;
use blink_native::clock::Clock;
use blink_native::modules::controls::{ControlCommand, ControlsModule, FakeControls, ProjectMode};
use blink_native::modules::deadlines::{DeadlinesModule, DemoSource};
use blink_native::modules::{Key, Module, PointerKind};
use common::*;
use std::sync::Arc;

fn app_with_controls() -> (App, Arc<FakeControls>) {
    let fake = FakeControls::new();
    let modules: Vec<Box<dyn Module>> = vec![Box::new(DeadlinesModule::new(Box::new(DemoSource::new(WALL_BASE)))), Box::new(ControlsModule::new(fake.clone()))];
    let mut app = App::new(Clock::manual(WALL_BASE), normalize_settings(None), tz(), 1.0, "en", modules).expect("app");
    app.tick();
    open_by_hover(&mut app);
    app.switch_to(1);
    settle(&mut app);
    app.render();
    fake.take_log();
    (app, fake)
}

/// Where a control is drawn (window coordinates), found the way a screen reader would find it.
fn rect_of(app: &mut App, name_starts: &str) -> (f32, f32, f32, f32) {
    app.render();
    app.accessibility().nodes.iter().find(|n| n.name.starts_with(name_starts)).unwrap_or_else(|| panic!("no control named {name_starts}")).rect
}

fn center(r: (f32, f32, f32, f32)) -> (f32, f32) {
    (r.0 + r.2 / 2.0, r.1 + r.3 / 2.0)
}

#[test]
fn the_panel_shows_what_windows_reports() {
    let (mut app, _fake) = app_with_controls();
    let names: Vec<String> = app.accessibility().nodes.iter().map(|n| n.name.clone()).collect();
    assert!(names.iter().any(|n| n.starts_with("Volume 40%")), "{names:?}");
    assert!(names.iter().any(|n| n == "Output: Speakers ✓"));
    assert!(names.iter().any(|n| n == "Output: AirBuds Pro"));
    assert!(names.iter().any(|n| n.starts_with("Brightness 70%")));
    assert!(names.iter().any(|n| n == "Wi-Fi: on"));
    assert!(names.iter().any(|n| n == "Bluetooth: on"));
    assert!(names.iter().any(|n| n == "Campus ✓"));
    assert!(names.iter().any(|n| n == "Project: Extend ✓"));
    assert_eq!(app.active_module(), "controls");
}

#[test]
fn a_click_on_the_volume_track_sets_the_volume_there_and_a_drag_follows() {
    let (mut app, fake) = app_with_controls();
    let r = rect_of(&mut app, "Volume 40%");
    // The track runs from x+8 (the hit area has 8 px of slack) to x+w-8.
    let (a, b) = (r.0 + 8.0, r.0 + r.2 - 8.0);
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
    app.pointer(PointerKind::Move, r.0 + 10.0, r.1 + 10.0);
    app.pointer(PointerKind::Wheel(48.0), r.0 + 40.0, r.1 + r.3 / 2.0);
    assert_eq!(fake.take_log(), vec![ControlCommand::VolumeStep(5)]);
    let icon_x = r.0 - 40.0 + 14.0; // the speaker button sits left of the track
    app.pointer(PointerKind::Down, icon_x, r.1 + 14.0);
    assert_eq!(fake.take_log(), vec![ControlCommand::ToggleMute]);
}

#[test]
fn chips_switch_the_output_toggle_radios_connect_networks_and_project() {
    let (mut app, fake) = app_with_controls();
    let click = |app: &mut App, name: &str| {
        let r = rect_of(app, name);
        let (x, y) = center(r);
        app.pointer(PointerKind::Move, x, y);
        app.pointer(PointerKind::Down, x, y);
    };
    click(&mut app, "Output: AirBuds Pro");
    click(&mut app, "Home");
    click(&mut app, "Project: Duplicate");
    click(&mut app, "Bluetooth: on");
    click(&mut app, "Wi-Fi: on");
    assert_eq!(
        fake.take_log(),
        vec![
            ControlCommand::SetOutput("bt".into()),
            ControlCommand::ConnectWifi("Home".into()),
            ControlCommand::Project(ProjectMode::Duplicate),
            ControlCommand::SetBluetooth(false),
            ControlCommand::SetWifi(false),
        ]
    );
    let names: Vec<String> = app.accessibility().nodes.iter().map(|n| n.name.clone()).collect();
    assert!(names.contains(&"Output: AirBuds Pro ✓".to_string()), "the panel follows the new state: {names:?}");
    assert!(names.contains(&"Wi-Fi: off".to_string()));
    assert!(names.contains(&"Project: Duplicate ✓".to_string()));
    assert!(!names.iter().any(|n| n == "Home"), "saved networks are only listed while Wi-Fi is on");
}

#[test]
fn the_keyboard_walks_the_buttons_and_adjusts_the_volume() {
    let (mut app, fake) = app_with_controls();
    app.hotkey();
    settle(&mut app);
    app.switch_to(1);
    app.render();
    fake.take_log();
    app.key(Key::Char('+'));
    app.key(Key::Char('-'));
    app.key(Key::Char('m'));
    assert_eq!(fake.take_log(), vec![ControlCommand::VolumeStep(5), ControlCommand::VolumeStep(-5), ControlCommand::ToggleMute]);
    // Buttons in order: the speaker (mute), then the output chips. Two Downs reach the second chip.
    app.key(Key::Down);
    app.key(Key::Down);
    app.render();
    app.key(Key::Enter);
    assert_eq!(fake.take_log(), vec![ControlCommand::SetOutput("bt".into())]);
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
}

#[test]
fn a_screen_without_brightness_control_shows_none() {
    use blink_native::modules::controls::ControlsState;
    let fake = FakeControls::with_state(ControlsState { volume: 10, brightness: None, wifi: None, bluetooth: None, ..ControlsState::default() });
    let modules: Vec<Box<dyn Module>> = vec![Box::new(DeadlinesModule::new(Box::new(DemoSource::new(WALL_BASE)))), Box::new(ControlsModule::new(fake))];
    let mut app = App::new(Clock::manual(WALL_BASE), normalize_settings(None), tz(), 1.0, "en", modules).expect("app");
    app.tick();
    open_by_hover(&mut app);
    app.switch_to(1);
    app.render();
    let names: Vec<String> = app.accessibility().nodes.iter().map(|n| n.name.clone()).collect();
    assert!(!names.iter().any(|n| n.starts_with("Brightness") || n.starts_with("Wi-Fi") || n.starts_with("Bluetooth")), "{names:?}");
    assert!(names.iter().any(|n| n.starts_with("Volume 10%")));
}
