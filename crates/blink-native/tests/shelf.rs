//! The shelf, and the two places a file can be dropped on the notch.

mod common;

use blink_core::settings::normalize_settings;
use blink_core::store::Store;
use blink_native::app::{App, AppEffect, DropZone};
use blink_native::clock::Clock;
use blink_native::modules::clock::ClockModule;
use blink_native::modules::deadlines::{DeadlinesModule, DemoSource};
use blink_native::modules::projects::ProjectsModule;
use blink_native::modules::shelf::ShelfModule;
use blink_native::modules::{Module, PointerKind};
use common::*;

struct Rig {
    app: App,
    dir: std::path::PathBuf,
}

fn dir(name: &str) -> std::path::PathBuf {
    let d = std::env::temp_dir().join(format!("blink-shelf-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(d.join("data")).unwrap();
    d
}

fn file(dir: &std::path::Path, name: &str) -> String {
    let p = dir.join(name);
    std::fs::write(&p, b"x").unwrap();
    p.to_string_lossy().to_string()
}

fn rig(name: &str) -> Rig {
    let d = dir(name);
    let modules: Vec<Box<dyn Module>> = vec![
        Box::new(ClockModule::new()),
        Box::new(DeadlinesModule::new(Box::new(DemoSource::new(WALL_BASE)))),
        Box::new(ProjectsModule::new(Store::new(d.join("data")), WALL_BASE)),
        Box::new(ShelfModule::new(Store::new(d.join("data")))),
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
    Rig { app, dir: d }
}

fn shelf(app: &mut App) -> Vec<String> {
    app.module_mut::<ShelfModule>().unwrap().items().to_vec()
}

#[test]
fn dragging_files_over_the_notch_opens_it_on_two_places_and_the_pointer_picks_one() {
    let mut r = rig("zones");
    r.app.drag_files(true);
    settle(&mut r.app);
    assert_eq!(r.app.state_json()["mode"], "open");
    assert_eq!(r.app.drag_zone(), Some(DropZone::Projects), "Projects is lit first: that is where dragged files used to go");
    r.app.drag_point(250.0, 10.0);
    assert_eq!(r.app.drag_zone(), Some(DropZone::Shelf), "moving to the left half lights the shelf");
    r.app.drag_point(400.0, 10.0);
    assert_eq!(r.app.drag_zone(), Some(DropZone::Projects));
    r.app.drag_point(250.0, 10.0);
    r.app.render();
    dump(&mut r.app, "dropzones");
    // The notch stays open for as long as the files are over it, however long.
    advance(&mut r.app, 30_000);
    assert_eq!(r.app.state_json()["mode"], "open");
    // They leave: it closes like after any hover.
    r.app.drag_files(false);
    advance(&mut r.app, 2_000);
    settle(&mut r.app);
    assert_eq!(r.app.state_json()["mode"], "rest");
    assert_eq!(r.app.drag_zone(), None);
}

#[test]
fn a_drop_on_the_lit_place_goes_there_and_that_page_shows_it() {
    let mut r = rig("drop");
    let a = file(&r.dir, "report.pdf");
    r.app.drag_files(true);
    r.app.drag_point(250.0, 10.0);
    r.app.drop_files(&[a.clone()]);
    settle(&mut r.app);
    assert_eq!(shelf(&mut r.app), vec![a.clone()]);
    assert_eq!(r.app.active_module(), "shelf");
    assert_eq!(r.app.drag_zone(), None, "the drag is over");
    // The same file again is not a second one; a file that is not there is skipped.
    r.app.drag_files(true);
    r.app.drag_point(250.0, 10.0);
    r.app.drop_files(&[a.clone(), r.dir.join("missing.txt").to_string_lossy().to_string()]);
    assert_eq!(shelf(&mut r.app).len(), 1);
    // The right half is Projects.
    let b = file(&r.dir, "notes.md");
    r.app.drag_files(true);
    r.app.drag_point(400.0, 10.0);
    r.app.drop_files(&[b]);
    assert_eq!(r.app.active_module(), "projects");
    assert_eq!(shelf(&mut r.app).len(), 1, "that one went to the project, not the shelf");
}

#[test]
fn the_shelf_is_remembered_and_forgets_files_that_have_gone() {
    let mut r = rig("persist");
    let a = file(&r.dir, "one.txt");
    let b = file(&r.dir, "two.txt");
    r.app.module_mut::<ShelfModule>().unwrap().add(&[a.clone(), b.clone()]);
    std::fs::remove_file(&b).unwrap();
    let again = ShelfModule::new(Store::new(r.dir.join("data")));
    assert_eq!(again.items(), [a].as_slice(), "the one that was deleted is not on the shelf any more");
}

fn open_shelf(r: &mut Rig) {
    open_by_hover(&mut r.app);
    let found = (0..8).any(|i| {
        r.app.switch_to(i);
        r.app.active_module() == "shelf"
    });
    assert!(found);
    settle(&mut r.app);
    r.app.render();
}

fn hit(app: &mut App, target: &str) -> (f32, f32) {
    let hits = app.state_json()["module"]["hits"].as_array().cloned().unwrap();
    let h = hits.iter().find(|h| h["target"].as_str().is_some_and(|t| t.starts_with(target))).cloned().unwrap_or_else(|| panic!("no {target}: {hits:?}"));
    (h["x"].as_f64().unwrap() as f32 + 6.0, h["y"].as_f64().unwrap() as f32 + 6.0)
}

#[test]
fn pressing_on_a_file_and_moving_carries_it_out_and_letting_go_in_place_opens_it() {
    let mut r = rig("drag");
    let a = file(&r.dir, "slides.pdf");
    r.app.module_mut::<ShelfModule>().unwrap().add(&[a.clone()]);
    open_shelf(&mut r);
    let (x, y) = hit(&mut r.app, "Item(");
    // A click: press and release without moving opens the file.
    r.app.take_effects();
    r.app.pointer(PointerKind::Move, x, y);
    r.app.pointer(PointerKind::Down, x, y);
    r.app.pointer(PointerKind::Up, x, y);
    assert_eq!(r.app.take_effects(), vec![AppEffect::OpenPath(a.clone())]);
    // A drag: press, then move far enough.
    r.app.pointer(PointerKind::Down, x, y);
    r.app.pointer(PointerKind::Move, x + 3.0, y);
    assert!(r.app.take_effects().is_empty(), "a small wobble is still a click");
    r.app.pointer(PointerKind::Move, x + 12.0, y + 4.0);
    assert_eq!(r.app.take_effects(), vec![AppEffect::StartDrag(vec![a.clone()])]);
    // Taken by somebody: it leaves the shelf. Refused: it stays.
    r.app.drag_out_begin();
    r.app.drag_out_end(&[a.clone()], false);
    assert_eq!(shelf(&mut r.app), vec![a.clone()]);
    r.app.drag_out_begin();
    r.app.drag_out_end(&[a.clone()], true);
    assert!(shelf(&mut r.app).is_empty());
    // The file itself is untouched.
    assert!(std::path::Path::new(&a).exists());
}

#[test]
fn drag_all_carries_everything_and_clear_empties_the_shelf() {
    let mut r = rig("all");
    let (a, b) = (file(&r.dir, "a.txt"), file(&r.dir, "b.txt"));
    r.app.module_mut::<ShelfModule>().unwrap().add(&[a.clone(), b.clone()]);
    open_shelf(&mut r);
    let (x, y) = hit(&mut r.app, "DragAll");
    r.app.take_effects();
    r.app.pointer(PointerKind::Move, x, y);
    r.app.pointer(PointerKind::Down, x, y);
    r.app.pointer(PointerKind::Move, x + 20.0, y);
    assert_eq!(r.app.take_effects(), vec![AppEffect::StartDrag(vec![a, b])]);
    r.app.pointer(PointerKind::Up, x, y);
    r.app.render();
    let (cx, cy) = hit(&mut r.app, "Clear");
    r.app.pointer(PointerKind::Move, cx, cy);
    r.app.pointer(PointerKind::Down, cx, cy);
    r.app.pointer(PointerKind::Up, cx, cy);
    assert!(shelf(&mut r.app).is_empty());
}

#[test]
fn while_files_are_carried_out_the_notch_does_not_take_them_back_in() {
    let mut r = rig("loop");
    let a = file(&r.dir, "loop.txt");
    r.app.drag_out_begin();
    r.app.drag_files(true);
    assert_eq!(r.app.drag_zone(), None, "its own drop target stays quiet");
    r.app.drop_files(&[a]);
    assert!(shelf(&mut r.app).is_empty());
    r.app.drag_out_end(&[], false);
}

#[test]
fn with_the_shelf_off_only_projects_is_offered() {
    let mut r = rig("off");
    r.app.configure_modules(&[blink_native::host::ModuleConfig { id: "shelf".into(), enabled: false, options: serde_json::Value::Null }]);
    r.app.drag_files(true);
    r.app.drag_point(250.0, 10.0);
    assert_eq!(r.app.drag_zone(), Some(DropZone::Projects), "nothing else to light");
}

#[test]
fn the_cross_on_a_row_takes_the_file_off_the_shelf_and_does_not_open_it() {
    let mut r = rig("cross");
    let (a, b) = (file(&r.dir, "keep.txt"), file(&r.dir, "drop.txt"));
    r.app.module_mut::<ShelfModule>().unwrap().add(&[a.clone(), b.clone()]);
    open_shelf(&mut r);
    // The cross shows while the pointer is on the row: move there, draw, then press on it.
    let (rx, ry) = hit(&mut r.app, "Item(");
    r.app.pointer(PointerKind::Move, rx, ry);
    r.app.render();
    let (cx, cy) = hit(&mut r.app, "Remove(");
    r.app.take_effects();
    r.app.pointer(PointerKind::Move, cx, cy);
    r.app.pointer(PointerKind::Down, cx, cy);
    r.app.pointer(PointerKind::Up, cx, cy);
    assert!(r.app.take_effects().iter().all(|e| !matches!(e, AppEffect::OpenPath(_))), "the file is not opened");
    assert_eq!(shelf(&mut r.app).len(), 1, "one file is off the shelf");
    assert!(std::path::Path::new(&a).exists() && std::path::Path::new(&b).exists(), "and nothing is deleted");
}

#[test]
fn the_plus_asks_for_the_clipboard() {
    let mut r = rig("paste");
    open_shelf(&mut r);
    let (x, y) = hit(&mut r.app, "Paste");
    r.app.take_effects();
    r.app.pointer(PointerKind::Move, x, y);
    r.app.pointer(PointerKind::Down, x, y);
    r.app.pointer(PointerKind::Up, x, y);
    assert_eq!(r.app.take_effects(), vec![AppEffect::PasteClipboard]);
}

#[test]
fn what_the_notch_made_from_the_clipboard_goes_when_it_leaves_the_shelf_and_nothing_else_does() {
    let mut r = rig("made");
    let dir = r.app.shelf_files_dir().unwrap();
    std::fs::create_dir_all(&dir).unwrap();
    let made = dir.join("clip-20261025-143007.png");
    std::fs::write(&made, b"png").unwrap();
    let mine = file(&r.dir, "notes.txt");
    r.app.add_to_shelf(&[made.to_string_lossy().to_string(), mine.clone()]);
    assert_eq!(shelf(&mut r.app).len(), 2);
    r.app.module_mut::<ShelfModule>().unwrap().remove(&made.to_string_lossy());
    assert!(!made.exists(), "the copy made from the clipboard is deleted with its entry");
    r.app.module_mut::<ShelfModule>().unwrap().remove(&mine);
    assert!(std::path::Path::new(&mine).exists(), "a file of the person's own never is");
}

#[test]
fn copied_pictures_are_taken_only_when_asked_and_only_with_a_shelf() {
    let mut r = rig("copied");
    assert!(!r.app.shelf_takes_copied_images(), "off by default");
    let on = |enabled: bool| {
        vec![blink_native::host::ModuleConfig { id: "shelf".into(), enabled, options: serde_json::json!({ "clipboardImages": true }) }]
    };
    r.app.configure_modules(&on(true));
    assert!(r.app.shelf_takes_copied_images());
    r.app.configure_modules(&on(false));
    assert!(!r.app.shelf_takes_copied_images(), "a shelf that is switched off takes nothing");
}

#[test]
fn clip_names_carry_the_date_and_time() {
    let r = rig("name");
    let name = r.app.clip_name("png");
    assert!(name.starts_with("clip-") && name.ends_with(".png") && name.len() == "clip-20261025-143007.png".len(), "{name}");
}

#[test]
fn the_notch_says_why_it_is_open() {
    let mut r = rig("why");
    assert!(r.app.why_open().contains("hold=false") && r.app.why_open().contains("dragging=false"), "{}", r.app.why_open());
    r.app.drag_files(true);
    let open = r.app.why_open();
    assert!(open.contains("hold=true") && open.contains("dragging=true"), "a drag over the notch is why it stays open: {open}");
    r.app.drag_files(false);
    advance(&mut r.app, 2_000);
    assert!(r.app.why_open().contains("hold=false"));
    assert!(r.app.state_json()["whyOpen"].as_str().is_some_and(|w| w.contains("mode=")));
}
