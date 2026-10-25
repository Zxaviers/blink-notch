//! The deadlines by course: a click on a course shows only that course, and a deadline can become a project.

mod common;

use blink_core::settings::normalize_settings;
use blink_core::store::Store;
use blink_native::app::App;
use blink_native::clock::Clock;
use blink_native::modules::clock::ClockModule;
use blink_native::modules::deadlines::{DeadlinesModule, DemoSource};
use blink_native::modules::projects::ProjectsModule;
use blink_native::modules::PointerKind;
use blink_native::modules::Module;
use common::*;

fn rig(name: &str) -> (App, std::path::PathBuf) {
    let dir = std::env::temp_dir().join(format!("blink-courses-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join("data")).unwrap();
    let modules: Vec<Box<dyn Module>> = vec![
        Box::new(ClockModule::new()),
        Box::new(DeadlinesModule::new(Box::new(DemoSource::new(WALL_BASE)))),
        Box::new(ProjectsModule::new(Store::new(dir.join("data")), WALL_BASE)),
    ];
    let mut app = App::new(Clock::manual(WALL_BASE), normalize_settings(None), tz(), 1.0, "en", modules).expect("app");
    app.configure_modules(&[
        blink_native::host::ModuleConfig { id: "clock".into(), enabled: true, options: serde_json::Value::Null },
        blink_native::host::ModuleConfig { id: "deadlines".into(), enabled: true, options: serde_json::json!({ "urgentHours": 1 }) },
    ]);
    app.tick();
    app.pointer(PointerKind::Leave, 0.0, 0.0);
    advance(&mut app, 600);
    settle(&mut app);
    (app, dir)
}

fn open_deadlines(app: &mut App) {
    open_by_hover(app);
    let found = (0..6).any(|i| {
        app.switch_to(i);
        app.active_module() == "deadlines"
    });
    assert!(found);
    settle(app);
    app.render();
}

fn hits(app: &mut App) -> Vec<serde_json::Value> {
    app.state_json()["module"]["hits"].as_array().cloned().unwrap_or_default()
}

fn hit_with(app: &mut App, action_starts: &str) -> (f32, f32) {
    let all = hits(app);
    let h = all.iter().find(|h| h["action"].as_str().is_some_and(|a| a.starts_with(action_starts))).unwrap_or_else(|| panic!("no {action_starts}: {all:?}"));
    (h["x"].as_f64().unwrap() as f32 + h["w"].as_f64().unwrap() as f32 / 2.0, h["y"].as_f64().unwrap() as f32 + h["h"].as_f64().unwrap() as f32 / 2.0)
}

fn click(app: &mut App, (x, y): (f32, f32)) {
    app.pointer(PointerKind::Move, x, y);
    app.pointer(PointerKind::Down, x, y);
    app.pointer(PointerKind::Up, x, y);
    app.render();
}

fn click_on(app: &mut App, action_starts: &str) {
    let at = hit_with(app, action_starts);
    click(app, at);
}

/// The courses of the rows that are drawn.
fn rows(app: &mut App) -> Vec<String> {
    let mut ids: Vec<String> = hits(app).iter().filter_map(|h| h["row"].as_str().map(str::to_string)).collect();
    ids.dedup();
    ids
}

#[test]
fn a_click_on_a_course_shows_only_that_course_and_the_cross_brings_the_rest_back() {
    let (mut app, _dir) = rig("filter");
    open_deadlines(&mut app);
    let all = rows(&mut app);
    assert!(all.len() >= 3, "the demo has several deadlines: {all:?}");
    click_on(&mut app, "FilterCourse:CS101");
    let only = rows(&mut app);
    assert_eq!(only.len(), 1, "one deadline is of that course: {only:?}");
    assert!(only[0].starts_with("t1::"), "{only:?}");
    // The cross where the sync line was takes the filter off.
    click_on(&mut app, "FilterCourse:");
    assert_eq!(rows(&mut app), all);
}

#[test]
fn a_deadline_becomes_a_project_with_itself_as_the_first_task_and_only_once() {
    let (mut app, dir) = rig("project");
    open_deadlines(&mut app);
    // The buttons show while the pointer is on the row.
    let row = app.state_json()["module"]["hits"].as_array().unwrap().iter().find(|h| h["row"].as_str().is_some_and(|r| r.starts_with("t1::")) && h["w"].as_f64().unwrap() > 60.0).cloned().unwrap();
    app.pointer(PointerKind::Move, row["x"].as_f64().unwrap() as f32 + 140.0, row["y"].as_f64().unwrap() as f32 + 10.0);
    app.render();
    click_on(&mut app, "BreakDown:t1::");
    assert_eq!(app.active_module(), "projects", "the Projects page shows the new project");
    let made = app.module_mut::<ProjectsModule>().unwrap().data().clone();
    let project = made.projects.iter().find(|p| p.name == "CS101 Algorithms \u{00B7} Problem Set 4").expect("a project named for the deadline");
    assert_eq!(project.tasks.len(), 1);
    assert_eq!(project.tasks[0].title, "Problem Set 4");
    assert_eq!(project.tasks[0].due_ms, Some(WALL_BASE + 3 * 3_600_000 + 14 * 60_000), "due when the deadline is");
    assert_eq!(made.selected.as_deref(), Some(project.id.as_str()));
    // Again: the same project, not a second one.
    let before = made.projects.len();
    open_deadlines(&mut app);
    let row = app.state_json()["module"]["hits"].as_array().unwrap().iter().find(|h| h["row"].as_str().is_some_and(|r| r.starts_with("t1::")) && h["w"].as_f64().unwrap() > 60.0).cloned().unwrap();
    app.pointer(PointerKind::Move, row["x"].as_f64().unwrap() as f32 + 140.0, row["y"].as_f64().unwrap() as f32 + 10.0);
    app.render();
    click_on(&mut app, "BreakDown:t1::");
    assert_eq!(app.module_mut::<ProjectsModule>().unwrap().data().projects.len(), before);
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn without_the_projects_page_it_says_so_instead_of_doing_nothing() {
    let (mut app, _dir) = rig("noprojects");
    app.configure_modules(&[blink_native::host::ModuleConfig { id: "projects".into(), enabled: false, options: serde_json::Value::Null }]);
    open_deadlines(&mut app);
    let row = app.state_json()["module"]["hits"].as_array().unwrap().iter().find(|h| h["row"].as_str().is_some_and(|r| r.starts_with("t1::")) && h["w"].as_f64().unwrap() > 60.0).cloned().unwrap();
    app.pointer(PointerKind::Move, row["x"].as_f64().unwrap() as f32 + 140.0, row["y"].as_f64().unwrap() as f32 + 10.0);
    app.render();
    click_on(&mut app, "BreakDown:t1::");
    // The notch is open, so the peek waits in the queue: it is not lost.
    assert_eq!(app.active_module(), "deadlines");
    assert!(app.state_json()["queuedPeeks"].as_u64().unwrap_or(0) + u64::from(!app.state_json()["peek"].is_null()) >= 1, "it says the page is off");
}
