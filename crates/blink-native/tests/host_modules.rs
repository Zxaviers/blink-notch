//! The host with more than one module: which one owns the pill, tabs and switching, and the
//! priority rules for peeks.

mod common;

use blink_core::settings::normalize_settings;
use blink_native::access::NodeAction;
use blink_native::app::App;
use blink_native::clock::Clock;
use blink_native::host::ModuleConfig;
use blink_native::events::{AudioKind, SystemEvent};
use blink_native::modules::clock::ClockModule;
use blink_native::modules::system::SystemModule;
use blink_native::modules::deadlines::{DeadlinesModule, DemoSource};
use blink_native::modules::{Key, Module, PeekView, PointerKind};
use blink_native::shell::Mode;
use common::*;
use serde_json::json;

fn two_modules(settings: serde_json::Value) -> App {
    let modules: Vec<Box<dyn Module>> = vec![Box::new(DeadlinesModule::new(Box::new(DemoSource::new(WALL_BASE)))), Box::new(ClockModule::new())];
    App::new(Clock::manual(WALL_BASE), normalize_settings(Some(&settings)), tz(), 1.0, "en", modules).expect("app")
}

/// Two modules where a deadline takes the pill only when it is due within `hours` (the deadlines' `urgentHours` option).
fn two_modules_urgent(hours: f64) -> App {
    let mut app = two_modules(json!({}));
    app.configure_modules(&[ModuleConfig { id: "deadlines".into(), enabled: true, options: json!({ "urgentHours": hours }) }]);
    app
}

fn cfg(list: &[(&str, bool)]) -> Vec<ModuleConfig> {
    list.iter().map(|(id, on)| ModuleConfig { id: id.to_string(), enabled: *on, options: serde_json::Value::Null }).collect()
}

fn peek(text: &str, priority: u8, key: Option<&str>, ttl: Option<u64>) -> PeekView {
    PeekView { code: "X".into(), title: text.into(), count: "1".into(), urgency: "later", priority, ttl_ms: ttl, key: key.map(String::from), progress: None, icon: None, accent: None }
}

fn shown(app: &App) -> Option<String> {
    app.current_peek().map(|p| p.title.clone())
}

#[test]
fn the_clock_rests_on_the_pill_until_a_deadline_is_close() {
    // The nearest deadline is 3 h 13 m away. By default "close" is five hours.
    let mut app = two_modules(json!({}));
    app.tick();
    assert_eq!(app.rest_owner_module(), "deadlines", "inside the default five hours the deadline takes the pill");

    let mut app = two_modules_urgent(1.0);
    app.tick();
    assert_eq!(app.rest_owner_module(), "clock", "hours away is not close: the calm clock rests there");
    let rest = app.state_json()["rest"].clone();
    assert_eq!(rest["count"], "19:00", "Jakarta time of the test clock");

    let mut app = two_modules_urgent(4.0);
    app.tick();
    assert_eq!(app.rest_owner_module(), "deadlines", "inside the chosen hours the deadline wins");
}

#[test]
fn opening_shows_the_module_that_owns_the_pill() {
    let mut app = two_modules_urgent(1.0);
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
    let mut app = two_modules_urgent(1.0);
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

fn with_system() -> App {
    let modules: Vec<Box<dyn Module>> =
        vec![Box::new(DeadlinesModule::new(Box::new(DemoSource::new(WALL_BASE)))), Box::new(ClockModule::new()), Box::new(SystemModule::new())];
    let mut app = App::new(Clock::manual(WALL_BASE), normalize_settings(None), tz(), 1.0, "en", modules).expect("app");
    app.tick();
    app
}

#[test]
fn a_module_without_a_panel_gets_no_tab_but_can_still_peek() {
    let mut app = with_system();
    assert_eq!(app.module_list().len(), 3);
    open_by_hover(&mut app);
    // Two modules have panels, so there are two tabs; the system module is not one of them.
    app.render();
    let tabs: Vec<String> = app.accessibility().nodes.iter().filter(|n| matches!(n.action, Some(NodeAction::SwitchTab(_)))).map(|n| n.name.clone()).collect();
    assert_eq!(tabs, ["Deadlines", "Clock"]);
    app.key(Key::Right);
    app.key(Key::Right);
    assert_eq!(app.active_module(), "deadlines", "arrows cycle through panels only");
    app.pointer(PointerKind::Leave, 0.0, 0.0);
    advance(&mut app, 600);
    settle(&mut app);

    app.system_event(SystemEvent::Volume { percent: 42, muted: false });
    let p = app.current_peek().expect("volume peeks");
    assert_eq!((p.count.as_str(), p.progress), ("42%", Some(0.42)));
}

#[test]
fn opening_from_a_system_peek_shows_a_panel_not_nothing() {
    let mut app = with_system();
    app.system_event(SystemEvent::AudioDevice { name: "AirBuds Pro".into(), kind: AudioKind::Headphones, connected: true });
    assert_eq!(app.current_peek().map(|p| p.code.clone()).as_deref(), Some("Headphones"));
    settle(&mut app);
    app.pointer(PointerKind::Move, 320.0, 20.0);
    advance(&mut app, 250);
    settle(&mut app);
    assert_eq!(app.state_json()["mode"], "open");
    assert_eq!(app.active_module(), "deadlines", "the system module has no panel; the pill's module opens");
}

#[test]
fn volume_steps_collapse_and_a_deadline_alert_outranks_them() {
    let mut app = with_system();
    for v in [30, 32, 34, 36] {
        app.system_event(SystemEvent::Volume { percent: v, muted: false });
    }
    assert_eq!(app.state_json()["queuedPeeks"], 0);
    assert_eq!(app.current_peek().unwrap().count, "36%");
    app.offer_peek(peek("Quiz in 1h", 3, None, None), None);
    assert_eq!(app.current_peek().unwrap().title, "Quiz in 1h");
}

#[test]
fn the_system_options_come_from_host_json() {
    let mut app = with_system();
    app.configure_modules(&[ModuleConfig { id: "system".into(), enabled: true, options: json!({ "volume": false }) }]);
    app.system_event(SystemEvent::Volume { percent: 50, muted: false });
    assert!(app.current_peek().is_none(), "volume peeks are switched off");
    app.system_event(SystemEvent::CapsLock(true));
    assert_eq!(app.current_peek().unwrap().title, "Caps Lock on");
}

#[test]
fn the_dots_at_the_top_right_open_blinks_own_settings() {
    let mut app = two_modules(json!({}));
    app.tick();
    open_by_hover(&mut app);
    app.render();
    // The settings button sits at the right end of the header.
    let node = app.accessibility().nodes.into_iter().find(|n| n.action == Some(NodeAction::AppSettings)).expect("a Blink settings button");
    assert_eq!(node.name, "Blink settings");
    let (x, y) = (node.rect.0 + node.rect.2 / 2.0, node.rect.1 + node.rect.3 / 2.0);
    app.take_effects();
    app.pointer(PointerKind::Move, x, y);
    app.pointer(PointerKind::Down, x, y);
    assert_eq!(app.take_effects(), vec![blink_native::app::AppEffect::OpenAppSettings]);
}

#[test]
fn the_calendar_page_browses_months_and_marks_nothing_it_was_not_told_about() {
    let mut app = two_modules(json!({}));
    app.tick();
    open_by_hover(&mut app);
    app.switch_to(1);
    settle(&mut app);
    app.render();
    assert_eq!(app.active_module(), "clock");
    let month = |app: &mut App| app.accessibility().nodes.iter().map(|n| n.name.clone()).find(|n| n.contains("20")).unwrap_or_default();
    assert_eq!(month(&mut app), "October 2026");
    app.key(Key::Down);
    app.render();
    assert_eq!(month(&mut app), "November 2026");
    assert_eq!(app.module_mut::<ClockModule>().unwrap().month_offset(), 1);
    app.key(Key::Home);
    app.render();
    assert_eq!(app.module_mut::<ClockModule>().unwrap().month_offset(), 0);
}

fn weather_app(source: std::sync::Arc<blink_native::modules::clock::FakeWeather>) -> App {
    let clock = ClockModule::new().with_weather(source, std::sync::Arc::new(|| {}));
    let modules: Vec<Box<dyn Module>> = vec![Box::new(clock), Box::new(DeadlinesModule::new(Box::new(DemoSource::new(WALL_BASE))))];
    let mut app = App::new(Clock::manual(WALL_BASE), normalize_settings(Some(&json!({}))), tz(), 1.0, "en", modules).expect("app");
    // The demo deadline is hours away: not close, so the calm clock owns the pill and opens first.
    app.configure_modules(&[ModuleConfig { id: "deadlines".into(), enabled: true, options: json!({ "urgentHours": 1.0 }) }]);
    app
}

/// Let the weather worker answer: it runs on its own thread.
fn wait_for_weather(app: &mut App) {
    for _ in 0..200 {
        app.tick();
        app.render();
        if !app.state_json()["module"]["place"].is_null() {
            return;
        }
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
}

#[test]
fn the_clock_page_shows_the_weather_first_and_swaps_to_the_calendar() {
    let source = blink_native::modules::clock::FakeWeather::new();
    let mut app = weather_app(source.clone());
    app.tick();
    open_by_hover(&mut app);
    settle(&mut app);
    app.render();
    assert_eq!(app.active_module(), "clock");
    assert_eq!(app.state_json()["module"]["pane"], "Weather", "the weather is what sits beside the time");
    wait_for_weather(&mut app);
    let state = app.state_json();
    assert_eq!(state["module"]["place"], "Jakarta (time zone)", "with no city chosen the time zone names one");
    assert_eq!(state["module"]["temp"].as_f64().map(|t| t.round()), Some(25.0));
    let asked = source.asked.lock().unwrap().clone();
    assert_eq!(asked, vec!["locate:Jakarta".to_string(), "forecast:-7.98,112.63".to_string()]);

    // The switch at the top right turns it into the calendar, and back.
    let click = |app: &mut App, target: &str| {
        let hit = app.state_json()["module"]["hits"].as_array().unwrap().iter().find(|h| h["target"] == target).cloned().expect(target);
        let (x, y) = (hit["x"].as_f64().unwrap() as f32 + 4.0, hit["y"].as_f64().unwrap() as f32 + 4.0);
        app.pointer(PointerKind::Move, x, y);
        app.pointer(PointerKind::Down, x, y);
        app.pointer(PointerKind::Up, x, y);
        app.render();
    };
    click(&mut app, "ShowCalendar");
    assert_eq!(app.state_json()["module"]["pane"], "Calendar");
    assert!(app.state_json()["module"]["hits"].as_array().unwrap().iter().any(|h| h["target"] == "NextMonth"));
    click(&mut app, "ShowWeather");
    assert_eq!(app.state_json()["module"]["pane"], "Weather");
    assert_eq!(source.asked.lock().unwrap().len(), 2, "swapping does not fetch again");
}

#[test]
fn a_city_from_the_settings_is_used_and_the_default_pane_can_be_the_calendar() {
    let source = blink_native::modules::clock::FakeWeather::new();
    let mut app = weather_app(source.clone());
    app.configure_modules(&[
        ModuleConfig { id: "clock".into(), enabled: true, options: json!({ "pane": "calendar", "weather": { "name": "Malang", "lat": -7.98, "lon": 112.63, "units": "f" } }) },
        ModuleConfig { id: "deadlines".into(), enabled: true, options: json!({ "urgentHours": 1.0 }) },
    ]);
    app.tick();
    open_by_hover(&mut app);
    settle(&mut app);
    app.render();
    assert_eq!(app.state_json()["module"]["pane"], "Calendar");
    let hit = app.state_json()["module"]["hits"].as_array().unwrap().iter().find(|h| h["target"] == "ShowWeather").cloned().unwrap();
    let (x, y) = (hit["x"].as_f64().unwrap() as f32 + 4.0, hit["y"].as_f64().unwrap() as f32 + 4.0);
    app.pointer(PointerKind::Move, x, y);
    app.pointer(PointerKind::Down, x, y);
    app.render();
    wait_for_weather(&mut app);
    assert_eq!(app.state_json()["module"]["place"], "Malang");
    assert_eq!(source.asked.lock().unwrap().clone(), vec!["forecast:-7.98,112.63".to_string()], "no guessing when a city is chosen");
}

#[test]
fn a_failed_weather_says_so_and_a_click_asks_again() {
    let source = blink_native::modules::clock::FakeWeather::new();
    source.fail.store(true, std::sync::atomic::Ordering::SeqCst);
    let mut app = weather_app(source.clone());
    app.tick();
    open_by_hover(&mut app);
    settle(&mut app);
    for _ in 0..200 {
        app.tick();
        app.render();
        if app.state_json()["module"]["failed"] == true {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    assert_eq!(app.state_json()["module"]["failed"], true);
    source.fail.store(false, std::sync::atomic::Ordering::SeqCst);
    let hit = app.state_json()["module"]["hits"].as_array().unwrap().iter().find(|h| h["target"] == "RetryWeather").cloned().expect("a retry area");
    let (x, y) = (hit["x"].as_f64().unwrap() as f32 + 4.0, hit["y"].as_f64().unwrap() as f32 + 4.0);
    app.pointer(PointerKind::Move, x, y);
    app.pointer(PointerKind::Down, x, y);
    wait_for_weather(&mut app);
    assert!(!app.state_json()["module"]["place"].is_null(), "the second try worked");
}

#[test]
fn the_place_under_the_weather_opens_the_settings_on_the_weather() {
    let source = blink_native::modules::clock::FakeWeather::new();
    let mut app = weather_app(source);
    app.tick();
    open_by_hover(&mut app);
    settle(&mut app);
    wait_for_weather(&mut app);
    let hit = app.state_json()["module"]["hits"].as_array().unwrap().iter().find(|h| h["target"] == "ChoosePlace").cloned().expect("the place is a button");
    let (x, y) = (hit["x"].as_f64().unwrap() as f32 + 4.0, hit["y"].as_f64().unwrap() as f32 + 4.0);
    app.take_effects();
    app.pointer(PointerKind::Move, x, y);
    app.pointer(PointerKind::Down, x, y);
    assert_eq!(app.take_effects(), vec![blink_native::app::AppEffect::OpenSettingsAt("weather".into())]);
}

#[test]
fn the_calendar_button_opens_the_chosen_calendar_in_the_browser() {
    let mut app = two_modules(json!({}));
    app.tick();
    open_by_hover(&mut app);
    settle(&mut app);
    app.render();
    assert_eq!(app.active_module(), "deadlines");
    let click = |app: &mut App, url: &str| {
        let (x, y) = app.module_mut::<DeadlinesModule>().unwrap().hit_center(&blink_native::modules::Action::OpenUrl(url.into())).expect("a calendar button");
        app.take_effects();
        app.pointer(PointerKind::Move, x, y);
        app.pointer(PointerKind::Down, x, y);
        app.take_effects()
    };
    assert_eq!(click(&mut app, "https://calendar.google.com/calendar/"), vec![blink_native::app::AppEffect::OpenUrl("https://calendar.google.com/calendar/".into())]);
    app.configure_modules(&[
        ModuleConfig { id: "deadlines".into(), enabled: true, options: json!({ "calendarUrl": "https://moodle.example.edu/calendar/view.php" }) },
        ModuleConfig { id: "clock".into(), enabled: true, options: serde_json::Value::Null },
    ]);
    app.render();
    assert_eq!(click(&mut app, "https://moodle.example.edu/calendar/view.php"), vec![blink_native::app::AppEffect::OpenUrl("https://moodle.example.edu/calendar/view.php".into())]);
    app.configure_modules(&[ModuleConfig { id: "deadlines".into(), enabled: true, options: json!({ "calendarUrl": "http://insecure" }) }]);
    app.render();
    assert_eq!(click(&mut app, "https://calendar.google.com/calendar/"), vec![blink_native::app::AppEffect::OpenUrl("https://calendar.google.com/calendar/".into())], "only https is taken");
}
