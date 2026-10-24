//! The first-run tour: it opens the notch by itself, four cards, and it is remembered when it ends.

mod common;

use blink_native::app::{App, AppEffect};
use blink_native::modules::{Key, PointerKind};
use blink_native::tour::STEPS;
use common::*;

fn mode(app: &mut App) -> String {
    app.state_json()["mode"].as_str().unwrap_or("").to_string()
}

fn button(app: &mut App, name: &str) -> (f32, f32) {
    // The buttons are found on the drawn frame: render, then look for their words in the accessibility-free way, by clicking where they are drawn.
    app.render();
    let (_, hits) = app.tour_buttons();
    let hit = hits.iter().find(|(_, label)| label == name).unwrap_or_else(|| panic!("no {name} button in {hits:?}"));
    hit.0
}

fn click(app: &mut App, at: (f32, f32)) {
    app.pointer(PointerKind::Move, at.0, at.1);
    app.pointer(PointerKind::Down, at.0, at.1);
}

fn started() -> App {
    let mut app = app();
    app.tick();
    app.pointer(PointerKind::Leave, 0.0, 0.0);
    advance(&mut app, 600);
    settle(&mut app);
    app.start_tour();
    advance(&mut app, 700);
    settle(&mut app);
    app.render();
    app
}

#[test]
fn the_tour_opens_the_notch_by_itself_and_stays_open() {
    let mut app = started();
    assert_eq!(mode(&mut app), "open", "no hover needed");
    assert_eq!(app.tour_step(), Some(0));
    advance(&mut app, 60_000);
    assert_eq!(mode(&mut app), "open", "it waits for the person");
    dump(&mut app, "tour-1");
}

#[test]
fn next_goes_through_the_four_cards_and_done_remembers_it() {
    let mut app = started();
    for step in 1..STEPS {
        let at = button(&mut app, "Next");
        click(&mut app, at);
        assert_eq!(app.tour_step(), Some(step));
    }
    app.render();
    dump(&mut app, "tour-4");
    app.take_effects();
    let at = button(&mut app, "Done");
    click(&mut app, at);
    assert_eq!(app.tour_step(), None);
    assert!(app.take_effects().contains(&AppEffect::TourDone));
    // The pointer is away: the notch closes like any other.
    app.pointer(PointerKind::Leave, 0.0, 0.0);
    advance(&mut app, 3_000);
    settle(&mut app);
    assert_eq!(mode(&mut app), "rest");
}

#[test]
fn back_and_skip_work_and_stray_clicks_do_nothing() {
    let mut app = started();
    let at = button(&mut app, "Next");
    click(&mut app, at);
    assert_eq!(app.tour_step(), Some(1));
    let at = button(&mut app, "Back");
    click(&mut app, at);
    assert_eq!(app.tour_step(), Some(0));
    // A click on empty panel is swallowed: no page below it is touched, the step stays.
    click(&mut app, (320.0, 80.0));
    assert_eq!(app.tour_step(), Some(0));
    app.take_effects();
    let at = button(&mut app, "Skip");
    click(&mut app, at);
    assert_eq!(app.tour_step(), None);
    assert!(app.take_effects().contains(&AppEffect::TourDone), "skipping is remembered too");
}

#[test]
fn the_keys_drive_it_too() {
    let mut app = started();
    app.key(Key::Right);
    assert_eq!(app.tour_step(), Some(1));
    app.key(Key::Left);
    assert_eq!(app.tour_step(), Some(0));
    app.take_effects();
    app.key(Key::Escape);
    assert_eq!(app.tour_step(), None);
    assert!(app.take_effects().contains(&AppEffect::TourDone));
}
