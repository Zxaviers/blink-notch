//! Renders the main states to PNGs in scripts/out so they can be looked at, and
//! checks the properties that must hold in every one of them.

mod common;

use blink_core::settings::normalize_settings;
use blink_native::modules::deadlines::{DeadlinesModule, DemoSource};
use blink_native::modules::{Action, Key, PointerKind};
use common::*;
use serde_json::json;

fn lit(app: &blink_native::app::App) -> usize {
    app.canvas().pm.data().chunks_exact(4).filter(|p| p[3] > 0).count()
}

#[test]
fn rest_and_peek_and_open_render() {
    let mut a = app();
    a.tick();
    dump(&mut a, "snap-rest");
    assert!(lit(&a) > 3000);

    let id = a.module_mut::<DeadlinesModule>().unwrap().source_mut().feeds()[0].tasks[0].id.clone();
    a.module_mut::<DeadlinesModule>().unwrap().request_peek(&id);
    a.peek();
    settle(&mut a);
    dump(&mut a, "snap-peek");

    let mut o = app();
    open_by_hover(&mut o);
    dump(&mut o, "snap-open");
    assert!(lit(&o) > 60_000, "an open panel is large: {}", lit(&o));
}

#[test]
fn hover_keyboard_snooze_done_and_scrolling_states_render() {
    let mut a = app();
    open_by_hover(&mut a);
    a.render();
    let id = a.module_mut::<DeadlinesModule>().unwrap().source_mut().feeds()[0].tasks[1].id.clone();
    let (x, y) = a.module_mut::<DeadlinesModule>().unwrap().row_center(&id).unwrap();
    a.pointer(PointerKind::Move, x, y);
    dump(&mut a, "snap-row-hover");

    let mut k = app();
    k.hotkey();
    settle(&mut k);
    k.key(Key::Down);
    dump(&mut k, "snap-keyboard");
    k.key(Key::Char('s'));
    dump(&mut k, "snap-snooze-chooser");
    k.key(Key::Char('2'));
    k.key(Key::Down);
    k.key(Key::Char('d'));
    dump(&mut k, "snap-after-snooze-and-done");
    // The Done heading sits below the fold in a 340 px panel: scroll to the bottom first.
    k.pointer(PointerKind::Wheel(-100_000.0), 320.0, 150.0);
    let (dx, dy) = {
        k.render();
        k.module_mut::<DeadlinesModule>().unwrap().hit_center(&Action::ToggleDone).expect("done section")
    };
    k.pointer(PointerKind::Move, dx, dy);
    k.pointer(PointerKind::Down, dx, dy);
    k.pointer(PointerKind::Wheel(-100_000.0), 320.0, 150.0);
    dump(&mut k, "snap-done-section-open");
}

#[test]
fn empty_overdue_only_light_theme_and_long_lists_render() {
    let mut e = app_with(DemoSource::new(WALL_BASE).without_feeds(), normalize_settings(None), 1.0);
    open_by_hover(&mut e);
    dump(&mut e, "snap-empty-no-feeds");

    let mut l = app_with_settings(json!({ "theme": "light" }));
    open_by_hover(&mut l);
    dump(&mut l, "snap-light");

    // Many deadlines: the panel is capped at 340 px and scrolls.
    let many: Vec<_> = (0..20)
        .map(|i| {
            let mut t = blink_core::ics::Task::default();
            t.uid = format!("m{i}");
            t.title = format!("Tugas nomor {i} dengan judul yang cukup panjang untuk membungkus");
            t.course = Some(format!("MK{:03} Mata kuliah", 100 + i));
            t.due_ms = WALL_BASE + (i as i64 + 1) * 5 * 3_600_000;
            t.id = format!("m{i}::{}", t.due_ms);
            t
        })
        .collect();
    let mut m = app_with(DemoSource::new(WALL_BASE).with_tasks(many), normalize_settings(None), 1.0);
    open_by_hover(&mut m);
    let s = m.state_json();
    assert_eq!(s["shape"]["h"], 340.0);
    assert!(s["panelHeight"].as_f64().unwrap() > 340.0);
    dump(&mut m, "snap-long-top");
    m.pointer(PointerKind::Wheel(-120.0), 320.0, 150.0);
    m.pointer(PointerKind::Wheel(-120.0), 320.0, 150.0);
    assert!(m.state_json()["module"]["scroll"].as_f64().unwrap() > 100.0);
    dump(&mut m, "snap-long-scrolled");
    m.pointer(PointerKind::Wheel(-100_000.0), 320.0, 150.0);
    let max = m.state_json()["module"]["scroll"].as_f64().unwrap();
    assert!((max - (m.state_json()["panelHeight"].as_f64().unwrap() - 340.0)).abs() < 0.6, "scroll stops at the end: {max}");
    dump(&mut m, "snap-long-bottom");
}

#[test]
fn high_dpi_renders_the_same_layout_larger() {
    let mut a = app_with(DemoSource::new(WALL_BASE), normalize_settings(None), 1.5);
    open_by_hover(&mut a);
    assert_eq!(a.state_json()["canvas"]["w"], 960);
    assert_eq!(a.state_json()["canvas"]["h"], 540);
    dump(&mut a, "snap-open-150pct");
    assert_eq!(alpha(&a, 480, 3), 255);
}