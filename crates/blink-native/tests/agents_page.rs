//! The Agents page: what programs tell the notch, what it shows, and what it does about it.

mod common;

use blink_core::settings::normalize_settings;
use blink_native::app::{App, AppEffect};
use blink_native::clock::Clock;
use blink_native::modules::agents::AgentsModule;
use blink_native::modules::clock::ClockModule;
use blink_native::modules::deadlines::{DeadlinesModule, DemoSource};
use blink_native::modules::{Module, PointerKind};
use common::*;
use serde_json::json;

fn rig() -> App {
    let modules: Vec<Box<dyn Module>> =
        vec![Box::new(ClockModule::new()), Box::new(DeadlinesModule::new(Box::new(DemoSource::new(WALL_BASE)))), Box::new(AgentsModule::new())];
    let mut app = App::new(Clock::manual(WALL_BASE), normalize_settings(None), tz(), 1.0, "en", modules).expect("app");
    // The demo deadline is hours away: not close, so the clock owns the pill.
    app.configure_modules(&[blink_native::host::ModuleConfig { id: "deadlines".into(), enabled: true, options: json!({ "urgentHours": 1.0 }) }]);
    app.tick();
    app.pointer(PointerKind::Leave, 0.0, 0.0);
    advance(&mut app, 600);
    settle(&mut app);
    app
}

fn names(app: &mut App) -> Vec<String> {
    app.render();
    app.accessibility().nodes.iter().map(|n| n.name.clone()).collect()
}

fn open_agents(app: &mut App) {
    open_by_hover(app);
    app.switch_to(2);
    settle(app);
    app.render();
}

#[test]
fn a_program_reports_and_the_page_lists_it_with_its_state() {
    let mut app = rig();
    app.agent_update(&json!({ "id": "claude-1", "name": "Claude Code", "state": "running", "title": "Refactor the parser", "progress": 0.4 })).unwrap();
    app.agent_update(&json!({ "id": "build", "name": "Nightly build", "state": "done", "title": "All green" })).unwrap();
    app.agent_update(&json!({ "id": "gemini", "name": "Gemini CLI", "state": "input", "title": "Apply the migration?", "detail": "Waiting for you to approve the migration" })).unwrap();
    open_agents(&mut app);
    let n = names(&mut app);
    assert!(n.iter().any(|n| n == "Claude Code: Running, Refactor the parser"), "{n:?}");
    assert!(n.iter().any(|n| n == "Nightly build: Finished, All green"), "{n:?}");
    let listed = app.agent_list().unwrap();
    assert_eq!(listed["agents"].as_array().unwrap().len(), 3);
    // Finished ones can be cleared.
    assert!(n.iter().any(|n| n == "Clear finished"));
    dump(&mut app, "agents-page");
}

#[test]
fn a_bad_update_is_refused_and_nothing_changes() {
    let mut app = rig();
    assert_eq!(app.agent_update(&json!({ "id": "bad id" })), Err("BAD_ID".to_string()));
    assert_eq!(app.agent_update(&json!({ "id": "a", "state": "dancing" })), Err("BAD_STATE".to_string()));
    assert_eq!(app.agent_update(&json!({ "id": "a", "url": "http://insecure" })), Err("BAD_URL".to_string()));
    assert!(app.agent_list().unwrap()["agents"].as_array().unwrap().is_empty());
}

#[test]
fn an_agent_that_needs_you_peeks_and_takes_the_pill_until_it_is_answered() {
    let mut app = rig();
    assert_eq!(app.rest_owner_module(), "clock");
    // Starting to run is not news.
    app.agent_update(&json!({ "id": "a", "name": "Claude Code", "state": "running" })).unwrap();
    assert!(app.current_peek().is_none());
    assert_eq!(app.rest_owner_module(), "clock");
    app.agent_update(&json!({ "id": "a", "state": "input", "title": "Run the tests?" })).unwrap();
    let peek = app.current_peek().expect("it needs you: a peek");
    assert_eq!((peek.code.as_str(), peek.title.as_str(), peek.priority), ("Claude Code", "Run the tests?", 3));
    assert_eq!(app.rest_owner_module(), "agents", "and the pill shows it");
    assert_eq!(app.state_json()["rest"]["label"], "Claude Code");
    // Answered: back to running, the clock has the pill again.
    app.agent_update(&json!({ "id": "a", "state": "running" })).unwrap();
    assert_eq!(app.rest_owner_module(), "clock");
}

#[test]
fn finishing_peeks_and_the_peek_can_be_turned_off() {
    let mut app = rig();
    app.agent_update(&json!({ "id": "a", "name": "Build", "state": "running" })).unwrap();
    app.agent_update(&json!({ "id": "a", "state": "done", "title": "Built in 41s" })).unwrap();
    assert_eq!(app.current_peek().map(|p| p.title.clone()), Some("Built in 41s".to_string()));

    let mut quiet = rig();
    quiet.configure_modules(&[blink_native::host::ModuleConfig { id: "agents".into(), enabled: true, options: json!({ "peek": false }) }]);
    quiet.agent_update(&json!({ "id": "a", "state": "done" })).unwrap();
    assert!(quiet.current_peek().is_none());
    assert_eq!(quiet.agent_list().unwrap()["agents"].as_array().unwrap().len(), 1, "it is still listed");
}

#[test]
fn updates_can_be_refused_altogether() {
    let mut app = rig();
    app.configure_modules(&[blink_native::host::ModuleConfig { id: "agents".into(), enabled: true, options: json!({ "accept": false }) }]);
    assert_eq!(app.agent_update(&json!({ "id": "a" })), Err("AGENTS_OFF".to_string()));
}

#[test]
fn a_row_opens_its_link_and_the_buttons_remove_and_clear() {
    let mut app = rig();
    app.agent_update(&json!({ "id": "a", "name": "PR review", "state": "done", "url": "https://example.com/pr/7" })).unwrap();
    app.agent_update(&json!({ "id": "b", "name": "Other", "state": "running" })).unwrap();
    open_agents(&mut app);
    let hits = app.state_json()["module"]["hits"].as_array().cloned().unwrap();
    let hit = |target: &str| hits.iter().find(|h| h["target"].as_str().is_some_and(|t| t.contains(target))).cloned().unwrap_or_else(|| panic!("no {target}: {hits:?}"));
    let click = |app: &mut App, h: &serde_json::Value| {
        let (x, y) = (h["x"].as_f64().unwrap() as f32 + 4.0, h["y"].as_f64().unwrap() as f32 + 4.0);
        app.take_effects();
        app.pointer(PointerKind::Move, x, y);
        app.pointer(PointerKind::Down, x, y);
        app.take_effects()
    };
    assert_eq!(click(&mut app, &hit("Row(\"a\")")), vec![AppEffect::OpenUrl("https://example.com/pr/7".into())]);
    assert!(click(&mut app, &hit("Row(\"b\")")).is_empty(), "no link, nothing to open");
    // Clear finished takes the done one away and keeps the running one.
    click(&mut app, &hit("Clear"));
    app.render();
    let left: Vec<String> = app.agent_list().unwrap()["agents"].as_array().unwrap().iter().map(|a| a["id"].as_str().unwrap().to_string()).collect();
    assert_eq!(left, vec!["b".to_string()]);
    assert_eq!(app.agent_remove("b").unwrap()["removed"], true);
    assert_eq!(app.agent_remove("b").unwrap()["removed"], false);
}

#[test]
fn the_page_says_how_to_connect_when_it_is_empty() {
    let mut app = rig();
    open_agents(&mut app);
    let n = names(&mut app);
    assert!(n.iter().any(|n| n.starts_with("Agent: 0")), "{n:?}");
    dump(&mut app, "agents-empty");
}

#[test]
fn a_screen_reader_can_remove_an_agent() {
    let mut app = rig();
    app.agent_update(&json!({ "id": "a", "name": "Claude Code", "state": "running" })).unwrap();
    open_agents(&mut app);
    app.render();
    let node = app.accessibility().nodes.into_iter().find(|n| n.name == "Remove Claude Code").expect("a remove button");
    assert!(node.action.is_some());
}
