//! Gestures on the notch: the wheel over the resting pill, a middle click, a sideways swipe.

mod common;

use blink_core::settings::normalize_settings;
use blink_native::app::App;
use blink_native::clock::Clock;
use blink_native::modules::clock::ClockModule;
use blink_native::modules::controls::{ControlsBackend, ControlsModule, FakeControls};
use blink_native::modules::deadlines::{DeadlinesModule, DemoSource};
use blink_native::modules::music::{FakeLyrics, FakeMedia, MediaCommand, MusicModule};
use blink_native::modules::{Module, PointerKind};
use common::*;
use std::sync::Arc;

fn rig() -> (App, Arc<FakeControls>, Arc<FakeMedia>) {
    let controls = FakeControls::new();
    let media = FakeMedia::new(WALL_BASE);
    let modules: Vec<Box<dyn Module>> = vec![
        Box::new(ClockModule::new()),
        Box::new(DeadlinesModule::new(Box::new(DemoSource::new(WALL_BASE)))),
        Box::new(ControlsModule::new(controls.clone())),
        Box::new(MusicModule::new(media.clone(), Arc::new(FakeLyrics), Arc::new(|| {}))),
    ];
    let mut app = App::new(Clock::manual(WALL_BASE), normalize_settings(None), tz(), 1.0, "en", modules).expect("app");
    // The demo deadline is hours away: not close, so the clock owns the pill and the notch opens on it.
    app.configure_modules(&[
        blink_native::host::ModuleConfig { id: "clock".into(), enabled: true, options: serde_json::Value::Null },
        blink_native::host::ModuleConfig { id: "deadlines".into(), enabled: true, options: serde_json::json!({ "urgentHours": 1.0 }) },
    ]);
    app.tick();
    app.pointer(PointerKind::Leave, 0.0, 0.0);
    advance(&mut app, 600);
    settle(&mut app);
    (app, controls, media)
}

#[test]
fn the_wheel_over_the_resting_pill_changes_the_volume_and_with_ctrl_the_brightness() {
    let (mut app, controls, _) = rig();
    assert_eq!(controls.state().volume, 40);
    assert!(app.quick_wheel(1.0, false));
    assert_eq!(controls.state().volume, 44);
    assert!(app.quick_wheel(-1.0, false));
    assert!(app.quick_wheel(-1.0, false));
    assert_eq!(controls.state().volume, 36);
    assert!(app.quick_wheel(1.0, true));
    assert_eq!(controls.state().brightness, Some(75));
    assert!(!app.quick_wheel(0.0, false), "no movement, nothing taken");
}

#[test]
fn an_open_notch_keeps_its_wheel_for_the_page() {
    let (mut app, controls, _) = rig();
    open_by_hover(&mut app);
    assert!(!app.quick_wheel(1.0, false), "the page scrolls, not the volume");
    assert_eq!(controls.state().volume, 40);
}

#[test]
fn a_middle_click_plays_or_pauses_only_when_something_is_open_in_a_player() {
    let (mut app, _, media) = rig();
    media.take_log();
    assert!(app.middle_click());
    assert!(media.take_log().contains(&MediaCommand::PlayPause));
    media.set(|s| s.active = false);
    assert!(!app.middle_click(), "nothing playing, nothing done");
    assert!(media.take_log().is_empty());
}

#[test]
fn a_swipe_turns_one_page_at_a_time_and_small_steps_add_up() {
    let (mut app, _, _) = rig();
    // Closed: a swipe does nothing.
    app.swipe(1.0);
    assert_eq!(app.active_module(), "clock");
    open_by_hover(&mut app);
    assert_eq!(app.active_module(), "clock");
    for _ in 0..3 {
        app.swipe(0.3);
    }
    assert_eq!(app.active_module(), "clock", "not enough yet");
    app.swipe(0.3);
    assert_eq!(app.active_module(), "deadlines", "enough: one page forward");
    // The rest of the same swipe arrives at once: it does not run on through the pages.
    app.swipe(1.0);
    app.swipe(1.0);
    assert_eq!(app.active_module(), "deadlines");
    advance(&mut app, 700);
    app.swipe(-1.0);
    assert_eq!(app.active_module(), "clock", "the other way, a new swipe");
}
