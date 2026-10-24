//! The projects page: tasks typed in with the keyboard, their statuses, files dropped on the notch.

mod common;

use blink_core::projects::{Projects, Status};
use blink_core::settings::normalize_settings;
use blink_core::store::Store;
use blink_native::app::{App, AppEffect};
use blink_native::clock::Clock;
use blink_native::modules::deadlines::{DeadlinesModule, DemoSource};
use blink_native::modules::projects::ProjectsModule;
use blink_native::modules::{Key, Module, PointerKind};
use common::*;
use std::path::PathBuf;

struct Rig {
    app: App,
    dir: PathBuf,
}

impl Drop for Rig {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

fn rig(name: &str) -> Rig {
    let dir = std::env::temp_dir().join(format!("blink-projects-page-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let modules: Vec<Box<dyn Module>> =
        vec![Box::new(DeadlinesModule::new(Box::new(DemoSource::new(WALL_BASE)))), Box::new(ProjectsModule::new(Store::new(dir.join("data")), WALL_BASE))];
    let mut app = App::new(Clock::manual(WALL_BASE), normalize_settings(None), tz(), 1.0, "en", modules).expect("app");
    app.tick();
    open_by_hover(&mut app);
    app.switch_to(1);
    settle(&mut app);
    app.render();
    app.take_effects();
    Rig { app, dir }
}

fn names(app: &mut App) -> Vec<String> {
    app.render();
    app.accessibility().nodes.iter().map(|n| n.name.clone()).collect()
}

fn click(app: &mut App, name: &str) {
    app.render();
    let r = app.accessibility().nodes.iter().find(|n| n.name == name || n.name.starts_with(name)).unwrap_or_else(|| panic!("no control named {name}")).rect;
    let (x, y) = (r.0 + r.2 / 2.0, r.1 + r.3 / 2.0);
    app.pointer(PointerKind::Move, x, y);
    app.pointer(PointerKind::Down, x, y);
    app.pointer(PointerKind::Up, x, y);
    app.render();
}

fn type_text(app: &mut App, text: &str) {
    for c in text.chars() {
        if c == ' ' {
            app.key(Key::Space);
        } else {
            app.key(Key::Char(c));
        }
    }
}

fn saved(rig: &Rig) -> Projects {
    Projects::load(&Store::new(rig.dir.join("data")), WALL_BASE)
}

fn module(app: &mut App) -> serde_json::Value {
    app.render();
    app.state_json()["module"].clone()
}

#[test]
fn a_new_page_has_one_empty_project_and_says_how_to_start() {
    let mut r = rig("empty");
    let n = names(&mut r.app);
    assert!(n.iter().any(|n| n.starts_with("Project: My project, 0 / 0")), "{n:?}");
    assert_eq!(r.app.active_module(), "projects");
    assert_eq!(module(&mut r.app)["tasks"].as_array().unwrap().len(), 0);
}

#[test]
fn typing_a_task_takes_the_keyboard_and_saves_it_on_enter() {
    let mut r = rig("typing");
    click(&mut r.app, "Task +");
    assert!(r.app.take_effects().contains(&AppEffect::KeyboardFocus(true)), "a click on add takes the keyboard");
    assert_eq!(module(&mut r.app)["editing"], "");
    type_text(&mut r.app, "Write API tests");
    assert_eq!(module(&mut r.app)["editing"], "Write API tests");
    r.app.key(Key::Backspace);
    r.app.key(Key::Char('s'));
    r.app.key(Key::Enter);
    assert!(r.app.take_effects().contains(&AppEffect::KeyboardFocus(false)), "the keyboard goes back when the typing is done");
    assert!(!r.app.state_json()["keyboard"].as_bool().unwrap(), "and the notch follows the pointer again");
    let m = module(&mut r.app);
    assert_eq!(m["editing"], serde_json::Value::Null);
    assert_eq!(m["tasks"][0]["title"], "Write API tests");
    assert_eq!(saved(&r).selected_project().unwrap().tasks[0].title, "Write API tests", "it is on disk at once");
    assert!(names(&mut r.app).iter().any(|n| n == "Write API tests: to do"));
}

#[test]
fn a_date_typed_with_at_becomes_the_due_date_and_is_not_part_of_the_title() {
    let mut r = rig("date");
    click(&mut r.app, "Task +");
    type_text(&mut r.app, "Ship it @fri");
    r.app.key(Key::Enter);
    let m = module(&mut r.app);
    assert_eq!(m["tasks"][0]["title"], "Ship it");
    assert!(m["tasks"][0]["due"].as_i64().is_some(), "{m}");
    // Friday is the day after the test clock's Thursday.
    let due = m["tasks"][0]["due"].as_i64().unwrap();
    assert!(due > WALL_BASE + 3_600_000 && due < WALL_BASE + 2 * 86_400_000, "{due}");
}

#[test]
fn escape_cancels_and_the_arrow_keys_stay_in_the_field_instead_of_turning_the_page() {
    let mut r = rig("escape");
    click(&mut r.app, "Task +");
    type_text(&mut r.app, "abc");
    r.app.key(Key::Left);
    r.app.key(Key::Left);
    r.app.key(Key::Char('X'));
    assert_eq!(r.app.active_module(), "projects", "Left did not turn the page while typing");
    assert_eq!(module(&mut r.app)["editing"], "aXbc", "it moved the caret");
    r.app.key(Key::Home);
    r.app.key(Key::Delete);
    assert_eq!(module(&mut r.app)["editing"], "XbC".replace('C', "c"));
    r.app.key(Key::Escape);
    assert_eq!(module(&mut r.app)["editing"], serde_json::Value::Null);
    assert_eq!(module(&mut r.app)["tasks"].as_array().unwrap().len(), 0, "nothing was saved");
    assert_eq!(r.app.active_module(), "projects", "the first Escape only cancelled the typing");
}

#[test]
fn pasting_goes_into_the_field_as_one_line() {
    let mut r = rig("paste");
    click(&mut r.app, "Task +");
    r.app.paste("first line\nsecond line");
    assert_eq!(module(&mut r.app)["editing"], "first line");
    r.app.key(Key::Enter);
    assert_eq!(module(&mut r.app)["tasks"][0]["title"], "first line");
}

#[test]
fn the_circle_moves_a_task_through_its_statuses_and_the_count_follows() {
    let mut r = rig("status");
    click(&mut r.app, "Task +");
    type_text(&mut r.app, "one");
    r.app.key(Key::Enter);
    click(&mut r.app, "one: to do");
    assert_eq!(saved(&r).selected_project().unwrap().tasks[0].status, Status::Doing);
    click(&mut r.app, "one: in progress");
    assert_eq!(saved(&r).selected_project().unwrap().tasks[0].status, Status::Done);
    assert!(names(&mut r.app).iter().any(|n| n.starts_with("Project: My project, 1 / 1")), "the count follows");
    click(&mut r.app, "one: done");
    assert_eq!(saved(&r).selected_project().unwrap().tasks[0].status, Status::Todo);
}

#[test]
fn deleting_a_task_takes_two_clicks() {
    let mut r = rig("delete");
    click(&mut r.app, "Task +");
    type_text(&mut r.app, "doomed");
    r.app.key(Key::Enter);
    // The delete button shows while the row is pointed at (the pointer is on it after the click above).
    r.app.render();
    let row = r.app.accessibility().nodes.iter().find(|n| n.name == "doomed: to do").unwrap().rect;
    r.app.pointer(PointerKind::Move, row.0 + 100.0, row.1 + row.3 / 2.0);
    r.app.render();
    click(&mut r.app, "Delete: doomed");
    assert_eq!(saved(&r).selected_project().unwrap().tasks.len(), 1, "the first click only asks");
    click(&mut r.app, "Delete: doomed");
    assert_eq!(saved(&r).selected_project().unwrap().tasks.len(), 0, "the second deletes");
}

#[test]
fn projects_can_be_added_named_and_switched() {
    let mut r = rig("projects");
    click(&mut r.app, "New project");
    type_text(&mut r.app, "Thesis");
    r.app.key(Key::Enter);
    let m = module(&mut r.app);
    assert_eq!((m["project"].as_str(), m["projects"].as_i64()), (Some("Thesis"), Some(2)));
    click(&mut r.app, "Next project");
    assert_eq!(module(&mut r.app)["project"], "My project");
    click(&mut r.app, "Next project");
    assert_eq!(module(&mut r.app)["project"], "Thesis", "wraps around");
    assert_eq!(saved(&r).projects.len(), 2);
}

#[test]
fn dropped_files_and_folders_are_attached_and_open_with_a_click() {
    let mut r = rig("files");
    let file = r.dir.join("spec.pdf");
    let folder = r.dir.join("repo");
    std::fs::create_dir_all(&folder).unwrap();
    std::fs::write(&file, b"x").unwrap();
    let paths = vec![file.to_string_lossy().to_string(), folder.to_string_lossy().to_string(), r.dir.join("missing").to_string_lossy().to_string()];
    r.app.drop_files(&paths);
    let m = module(&mut r.app);
    assert_eq!(m["view"], "Files");
    assert_eq!(m["files"].as_array().unwrap().len(), 2, "the missing one is skipped: {m}");
    assert_eq!(r.app.state_json()["queuedPeeks"], 1, "the drop is confirmed with a peek, which waits while the panel is open");
    // A click on a file opens it.
    click(&mut r.app, "Open: spec.pdf");
    assert_eq!(r.app.take_effects().last(), Some(&AppEffect::OpenPath(file.to_string_lossy().to_string())));
    // Removing takes two clicks and never touches the file.
    // (its buttons show while the row is pointed at)
    let row = r.app.accessibility().nodes.iter().find(|n| n.name == "Open: repo").unwrap().rect;
    r.app.pointer(PointerKind::Move, row.0 + 100.0, row.1 + row.3 / 2.0);
    r.app.render();
    click(&mut r.app, "Remove from project: repo");
    assert_eq!(saved(&r).selected_project().unwrap().files.len(), 2);
    click(&mut r.app, "Remove from project: repo");
    assert_eq!(saved(&r).selected_project().unwrap().files.len(), 1);
    assert!(folder.exists(), "only the link is removed");
}

#[test]
fn open_tasks_with_dates_mark_the_calendar() {
    let mut r = rig("calendar");
    click(&mut r.app, "Task +");
    type_text(&mut r.app, "demo day @fri");
    r.app.key(Key::Enter);
    let marks = r.app.module_mut::<ProjectsModule>().unwrap().due_dates(&blink_native::modules::Env {
        now_ms: 0,
        wall_ms: WALL_BASE,
        tz: &tz(),
        t: blink_core::i18n::Translator::new(blink_core::i18n::Lang::En),
    });
    assert_eq!(marks.len(), 1);
}

#[test]
fn a_notch_opened_with_the_hotkey_keeps_the_keyboard_after_typing() {
    let mut r = rig("hotkey");
    r.app.pointer(PointerKind::Leave, 0.0, 0.0);
    advance(&mut r.app, 600);
    r.app.hotkey();
    settle(&mut r.app);
    r.app.switch_to(1);
    r.app.take_effects();
    r.app.key(Key::Char('a'));
    type_text(&mut r.app, "by keyboard");
    r.app.key(Key::Enter);
    assert_eq!(module(&mut r.app)["tasks"][0]["title"], "by keyboard");
    assert!(r.app.state_json()["keyboard"].as_bool().unwrap(), "the person opened it with the keyboard, so it stays that way");
    assert!(!r.app.take_effects().contains(&AppEffect::KeyboardFocus(false)));
}
