//! The host with more than one module: which one owns the pill, tabs and switching, and the
//! priority rules for peeks.

mod common;

use blink_core::settings::normalize_settings;
use blink_native::access::NodeAction;
use blink_native::app::App;
use blink_native::clock::Clock;
use blink_native::host::ModuleConfig;
use blink_native::modules::clock::ClockModule;
use blink_native::modules::deadlines::{DeadlinesModule, DemoSource};
use blink_native::modules::{Key, Module, PeekView, PointerKind};
use blink_native::shell::Mode;
use common::*;
use serde_json::json;

fn two_modules(settings: serde_json::Value) -> App {
    let modules: Vec<Box<dyn Module>> = vec![Box::new(DeadlinesModule::new(Box::new(DemoSource::new(WALL_BASE)))), Box::new(ClockModule::new())];
    App::new(Clock::manual(WALL_BASE), normalize_settings(Some(&settings)), tz(), 1.0, "en", modules).expect("app")
}

fn cfg(list: &[(&str, bool)]) -> Vec<ModuleConfig> {
    list.iter().map(|(id, on)| ModuleConfig { id: id.to_string(), enabled: *on }).collect()
}

fn peek(text: &str, priority: u8, key: Option<&str>, ttl: Option<u64>) -> PeekView {
    PeekView { code: "X".into(), title: text.into(), count: "1".into(), urgency: "later", priority, ttl_ms: ttl, key: key.map(String::from) }
}

fn shown(app: &App) -> Option<String> {
    app.current_peek().map(|p| p.title.clone())
}

#[test]
fn a_deadline_keeps_the_pill_by_default_and_the_clock_takes_it_when_nothing_is_near() {
    let mut app = two_modules(json!({}));
    app.tick();
    assert_eq!(app.rest_owner_module(), "deadlines", "resting style always: the nearest deadline is always shown");

    let mut app = two_modules(json!({ "restingStyle": "when-due", "dueHorizonHours": 1 }));
    app.tick();
    assert_eq!(app.rest_owner_module(), "clock", "the nearest deadline is hours away, so the calm clock rests there");
    let rest = app.state_json()["rest"].clone();
    assert_eq!(rest["count"], "19:00", "Jakarta time of the test clock");

    let mut app = two_modules(json!({ "restingStyle": "when-due", "dueHorizonHours": 4 }));
    app.tick();
    assert_eq!(app.rest_owner_module(), "deadlines", "inside the horizon the deadline wins");
}

#[test]
fn opening_shows_the_module_that_owns_the_pill() {
    let mut app = two_modules(json!({ "restingStyle": "when-due", "dueHorizonHours": 1 }));
    app.tick();
    open_by_hover(&mut app);
    assert_eq!(app.active_module(), "clock");
    let j = app.state_json();
    assert_eq!(j["mode"], "open");
    assert_eq!(j["tabs"], true);
    // The clock panel is short; the tab strip adds to it.
    assert!(j["panelHeight"].as_f64().unwrap() < 200.0);
}

#[test]
fn tabs_switch_by_key_and_by_click() {
    let mut app = two_modules(json!({}));
    app.tick();
    open_by_hover(&mut app);
    assert_eq!(app.active_module(), "deadlines");
    app.key(Key::Right);
    assert_eq!(app.active_module(), "clock");
    app.key(Key::Right);
    assert_eq!(app.active_module(), "deadlines", "wraps around");
    app.key(Key::Left);
    assert_eq!(app.active_module(), "clock");

    // Click the Deadlines tab: find where it is drawn from what a screen reader would be told.
    app.render();
    let tree = app.accessibility();
    let tab = tree.nodes.iter().find(|n| n.action == Some(NodeAction::SwitchTab(0))).expect("a tab for the first module");
    assert_eq!(tab.name, "Deadlines");
    app.pointer(PointerKind::Down, tab.rect.0 + tab.rect.2 / 2.0, tab.rect.1 + tab.rect.3 / 2.0);
    assert_eq!(app.active_module(), "deadlines");
}

#[test]
fn switching_does_not_lose_keyboard_mode() {
    let mut app = two_modules(json!({}));
    app.tick();
    app.hotkey();
    settle(&mut app);
    app.key(Key::Right);
    assert!(app.state_json()["keyboard"].as_bool().unwrap());
    assert_eq!(app.active_module(), "clock");
    app.key(Key::Escape);
    settle(&mut app);
    assert_eq!(app.state_json()["mode"], "rest");
}

#[test]
fn modules_can_be_turned_off_and_reordered_but_never_all_off() {
    let mut app = two_modules(json!({}));
    app.configure_modules(&cfg(&[("clock", true), ("deadlines", false)]));
    assert_eq!(app.module_list(), vec![("clock", true), ("deadlines", false)]);
    assert_eq!(app.active_module(), "clock");
    assert_eq!(app.state_json()["tabs"], false, "one module on: no tab strip");

    app.configure_modules(&cfg(&[("clock", false), ("deadlines", false)]));
    assert_eq!(app.module_list().iter().filter(|(_, on)| *on).count(), 1, "the notch is never left with nothing to show");

    app.configure_modules(&cfg(&[("deadlines", true), ("nonsense", true)]));
    assert_eq!(app.module_list(), vec![("deadlines", true), ("clock", true)], "unknown ids are ignored; unmentioned modules keep their state");
}

#[test]
fn a_more_important_peek_replaces_a_lesser_one_and_the_lesser_one_waits() {
    let mut app = two_modules(json!({}));
    app.tick();
    app.offer_peek(peek("charger", 1, Some("power"), None), None);
    assert_eq!(app.state_json()["mode"], "peek");
    assert_eq!(shown(&app).as_deref(), Some("charger"));

    app.offer_peek(peek("deadline", 3, None, None), None);
    assert_eq!(shown(&app).as_deref(), Some("deadline"), "higher priority replaces");

    app.offer_peek(peek("wifi", 1, Some("net"), None), None);
    assert_eq!(shown(&app).as_deref(), Some("deadline"), "lower priority does not interrupt");
    assert_eq!(app.state_json()["queuedPeeks"], 1);

    advance(&mut app, 4200);
    settle(&mut app);
    assert_eq!(shown(&app).as_deref(), Some("wifi"), "it gets its turn when the notch is free");
    advance(&mut app, 4200);
    settle(&mut app);
    assert_eq!(app.state_json()["mode"], "rest");
    assert_eq!(app.state_json()["queuedPeeks"], 0);
}

#[test]
fn a_peek_with_the_same_key_updates_in_place() {
    let mut app = two_modules(json!({}));
    app.tick();
    app.offer_peek(peek("volume 30", 1, Some("volume"), None), None);
    advance(&mut app, 500);
    app.offer_peek(peek("volume 35", 1, Some("volume"), None), None);
    app.offer_peek(peek("volume 40", 1, Some("volume"), None), None);
    assert_eq!(shown(&app).as_deref(), Some("volume 40"));
    assert_eq!(app.state_json()["queuedPeeks"], 0, "ten volume steps are not ten peeks");
}

#[test]
fn a_peek_stays_for_its_own_time() {
    let mut app = two_modules(json!({}));
    app.tick();
    app.offer_peek(peek("quick", 1, None, Some(1000)), None);
    advance(&mut app, 600);
    assert!(shown(&app).is_some());
    advance(&mut app, 900);
    settle(&mut app);
    assert_eq!(app.state_json()["mode"], "rest", "a 1 s peek is gone well before the standard 4 s");
}

#[test]
fn a_peek_offered_while_the_panel_is_open_waits_for_it_to_close() {
    let mut app = two_modules(json!({}));
    app.tick();
    open_by_hover(&mut app);
    app.offer_peek(peek("later", 1, None, None), None);
    assert_eq!(app.state_json()["mode"], "open", "an open panel is not interrupted");
    assert_eq!(app.state_json()["queuedPeeks"], 1);
    app.pointer(PointerKind::Leave, 0.0, 0.0);
    advance(&mut app, 600);
    settle(&mut app);
    assert_eq!(shown(&app).as_deref(), Some("later"));
    assert_eq!(app.shell().mode(), Mode::Peek);
}

#[test]
fn clicking_a_peek_from_a_module_opens_that_module() {
    let mut app = two_modules(json!({ "restingStyle": "when-due", "dueHorizonHours": 1 }));
    app.tick();
    assert_eq!(app.rest_owner_module(), "clock");
    // A peek that came from the deadlines module (slot 0) while the clock owns the pill.
    app.offer_peek(peek("Problem Set 4", 3, None, None), Some(0));
    settle(&mut app);
    app.pointer(PointerKind::Move, 320.0, 20.0);
    advance(&mut app, 200);
    settle(&mut app);
    assert_eq!(app.state_json()["mode"], "open");
    assert_eq!(app.active_module(), "deadlines", "opening from that peek shows what it was about");
}
