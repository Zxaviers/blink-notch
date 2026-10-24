//! The app end to end without Windows: hover, click, keys, time.

mod common;

use blink_native::app::AppEffect;
use blink_native::modules::deadlines::DeadlinesModule;
use blink_native::modules::{Action, Key, PointerKind};
use blink_native::shell::Mode;
use common::*;
use serde_json::json;

fn mode(app: &mut blink_native::app::App) -> String {
    app.state_json()["mode"].as_str().unwrap().to_string()
}

#[test]
fn the_resting_pill_hangs_from_the_top_with_text_inside_and_nothing_around_it() {
    let mut app = app();
    app.render();
    assert_eq!(alpha(&app, 320, 2), 255);
    assert_eq!(alpha(&app, 100, 10), 0);
    assert_eq!(alpha(&app, 320, 50), 0);
    let lit = app.canvas().pm.data().chunks_exact(4).filter(|p| p[0] > 40 || p[1] > 40).count();
    assert!(lit > 150, "the code and countdown are drawn ({lit} light pixels)");
    let rest = &app.state_json()["rest"];
    assert_eq!(rest["label"], "CS101");
    assert_eq!(rest["count"], "03:14:00".replace("03:14:00", "3h 14m"));
    assert_eq!(rest["urgency"], "soon");
}

#[test]
fn hovering_opens_after_the_delay_and_the_panel_shows_content() {
    let mut app = app();
    app.pointer(PointerKind::Move, 320.0, 10.0);
    advance(&mut app, 149);
    assert_eq!(mode(&mut app), "rest");
    advance(&mut app, 2);
    assert_eq!(mode(&mut app), "open");
    assert!(app.wants_frames());
    settle(&mut app);
    assert!(!app.wants_frames());
    app.render();
    let s = app.state_json();
    assert_eq!(s["shape"]["w"], 540.0);
    assert!(s["shape"]["h"].as_f64().unwrap() > 150.0, "{}", s["shape"]);
    assert!(s["module"]["hits"].as_array().unwrap().len() > 3, "rows and buttons were recorded");
    assert_eq!(alpha(&app, 110 + 30, 150), 255, "inside the open panel");
}

#[test]
fn leaving_closes_after_the_grace_period_and_the_panel_goes_away() {
    let mut app = app();
    open_by_hover(&mut app);
    app.pointer(PointerKind::Leave, 0.0, 0.0);
    advance(&mut app, 299);
    assert_eq!(mode(&mut app), "open");
    advance(&mut app, 400);
    assert_eq!(mode(&mut app), "rest");
    app.render();
    assert_eq!(alpha(&app, 320, 100), 0, "the panel area is see-through again");
}

#[test]
fn moving_outside_the_shape_counts_as_leaving() {
    let mut app = app();
    app.pointer(PointerKind::Move, 320.0, 10.0);
    assert!(app.state_json()["hovering"].as_bool().unwrap());
    app.pointer(PointerKind::Move, 50.0, 300.0);
    assert!(!app.state_json()["hovering"].as_bool().unwrap());
    advance(&mut app, 500);
    assert_eq!(mode(&mut app), "rest", "the open timer was cancelled");
}

#[test]
fn clicking_the_pill_opens_at_once() {
    let mut app = app();
    app.pointer(PointerKind::Move, 320.0, 10.0);
    app.pointer(PointerKind::Down, 320.0, 10.0);
    assert_eq!(mode(&mut app), "open");
}

#[test]
fn the_done_button_on_a_hovered_row_marks_it_done_and_the_hero_moves_on() {
    let mut app = app();
    open_by_hover(&mut app);
    let hero = "t1";
    let id = {
        let m = app.module_mut::<DeadlinesModule>().unwrap();
        m.source_mut().feeds()[0].tasks.iter().find(|t| t.uid == hero).unwrap().id.clone()
    };
    // Hover the hero row so its buttons appear, draw, then press the done button.
    app.render();
    let (rx, ry) = app.module_mut::<DeadlinesModule>().unwrap().row_center(&id).expect("hero row");
    app.pointer(PointerKind::Move, rx, ry);
    app.render();
    let (bx, by) = app.module_mut::<DeadlinesModule>().unwrap().hit_center(&Action::MarkDone(id.clone())).expect("done button");
    app.pointer(PointerKind::Move, bx, by);
    app.pointer(PointerKind::Down, bx, by);
    app.render();
    assert!(app.module_mut::<DeadlinesModule>().unwrap().source_mut().done().contains_key(&id));
    let rest = &app.state_json()["rest"];
    assert_eq!(rest["label"], "MA201", "the next deadline (30 hours) is now the hero");
}

#[test]
fn opening_a_link_and_the_footer_buttons_become_effects_for_the_window_layer() {
    let mut app = app();
    open_by_hover(&mut app);
    app.render();
    let (gx, gy) = app.module_mut::<DeadlinesModule>().unwrap().hit_center(&Action::OpenSettings).expect("gear");
    app.pointer(PointerKind::Move, gx, gy);
    app.pointer(PointerKind::Down, gx, gy);
    let (rx, ry) = app.module_mut::<DeadlinesModule>().unwrap().hit_center(&Action::Refresh).expect("refresh");
    app.pointer(PointerKind::Down, rx, ry);
    assert_eq!(app.take_effects(), vec![AppEffect::OpenSettings, AppEffect::RefreshFeeds]);
    assert!(app.take_effects().is_empty());
}

#[test]
fn the_hotkey_opens_with_keyboard_focus_and_keys_navigate_mark_done_snooze_and_close() {
    let mut app = app();
    app.hotkey();
    assert_eq!(app.take_effects(), vec![AppEffect::KeyboardFocus(true)]);
    settle(&mut app);
    assert_eq!(mode(&mut app), "open");
    assert!(app.state_json()["keyboard"].as_bool().unwrap());
    app.render();
    let selected = |app: &mut blink_native::app::App| app.state_json()["module"]["selected"].as_str().map(String::from);
    let first = selected(&mut app).expect("first row is selected");
    assert!(first.starts_with("t1::"));

    app.key(Key::Down);
    app.key(Key::Down);
    let third = selected(&mut app).unwrap();
    assert!(third != first);
    app.key(Key::Up);
    app.key(Key::End);
    let last = selected(&mut app).unwrap();
    assert!(last.starts_with("t4::"), "the overdue row is last: {last}");
    app.key(Key::Home);
    assert_eq!(selected(&mut app).unwrap(), first);

    // Snooze the hero for 2 hours.
    app.key(Key::Char('s'));
    app.render();
    assert_eq!(app.state_json()["module"]["snoozeFor"], json!(first));
    app.key(Key::Char('2'));
    let snoozed = app.module_mut::<DeadlinesModule>().unwrap().source_mut().snoozed();
    assert!(snoozed.contains_key(&first));
    assert!((snoozed[&first] as i64 - (WALL_BASE + app.now() as i64 + 2 * 3_600_000)).abs() < 5000);

    // Mark the selection done; the selection moves on.
    app.key(Key::Char('d'));
    assert!(app.module_mut::<DeadlinesModule>().unwrap().source_mut().done().contains_key(&first));

    app.key(Key::Escape);
    let effects = app.take_effects();
    let announced: Vec<&str> = effects.iter().filter_map(|e| if let AppEffect::Announce(t) = e { Some(t.as_str()) } else { None }).collect();
    assert_eq!(announced, ["Snoozed: Problem Set 4", "Marked done: Problem Set 4"], "each action is spoken for a screen reader");
    let rest: Vec<&AppEffect> = effects.iter().filter(|e| !matches!(e, AppEffect::Announce(_))).collect();
    assert_eq!(rest, vec![&AppEffect::KeyboardFocus(false)]);
    settle(&mut app);
    assert_eq!(mode(&mut app), "rest");
    assert!(!app.state_json()["keyboard"].as_bool().unwrap());
}

#[test]
fn escape_closes_the_snooze_chooser_first_and_the_panel_second() {
    let mut app = app();
    app.hotkey();
    settle(&mut app);
    app.key(Key::Char('s'));
    app.key(Key::Escape);
    assert_eq!(mode(&mut app), "open");
    assert!(app.state_json()["module"]["snoozeFor"].is_null());
    app.key(Key::Escape);
    advance(&mut app, 300);
    assert_eq!(mode(&mut app), "rest");
}

#[test]
fn pressing_the_hotkey_again_and_losing_focus_both_leave_keyboard_mode() {
    let mut app = app();
    app.hotkey();
    app.take_effects();
    app.hotkey();
    assert_eq!(app.take_effects(), vec![AppEffect::KeyboardFocus(false)]);
    assert_eq!(mode(&mut app), "rest");

    app.hotkey();
    app.take_effects();
    app.focus_lost();
    assert_eq!(mode(&mut app), "rest");
    assert!(app.take_effects().is_empty(), "focus already moved on, nothing to hand back");
}

#[test]
fn a_peek_shows_the_announced_deadline_then_fades_out_after_four_seconds() {
    let mut app = app();
    let id = app.module_mut::<DeadlinesModule>().unwrap().source_mut().feeds()[0].tasks[0].id.clone();
    app.module_mut::<DeadlinesModule>().unwrap().request_peek(&id);
    app.peek();
    assert_eq!(mode(&mut app), "peek");
    settle(&mut app);
    app.render();
    assert_eq!(alpha(&app, 320, 40), 255, "the peek is 56 px tall");
    advance(&mut app, 4100);
    assert_eq!(mode(&mut app), "rest");
}

#[test]
fn rest_styles_follow_the_setting_and_the_data() {
    let mut hover = app_with_settings(json!({ "restingStyle": "hover" }));
    hover.tick();
    settle(&mut hover); // the pill shrinks to the strip over 280 ms
    hover.render();
    assert_eq!(hover.state_json()["restStyle"], "hot");
    assert_eq!(alpha(&hover, 320, 2), 1, "the hot strip is invisible but not see-through");
    assert_eq!(alpha(&hover, 320, 20), 0);

    // Only "hover" hides the pill; the others keep it (the clock, or a close deadline, rests there).
    let mut due = app_with_settings(json!({ "restingStyle": "when-due", "dueHorizonHours": 1 }));
    due.tick();
    assert_eq!(due.state_json()["restStyle"], "full");
}

#[test]
fn the_countdown_wakes_the_app_exactly_when_its_text_changes() {
    let mut app = app();
    let wake = app.next_wake().expect("a deadline is counting down");
    let base = app.state_json()["rest"]["count"].as_str().unwrap().to_string();
    assert_eq!(base, "3h 14m");
    // The hero is 3h14m away: the minute text changes after 0 s past the boundary + 1 ms.
    app.clock.set_manual_ms(wake - 1);
    assert_eq!(app.state_json()["rest"]["count"], "3h 14m");
    app.clock.set_manual_ms(wake);
    assert_eq!(app.state_json()["rest"]["count"], "3h 13m");
}

#[test]
fn an_idle_notch_with_no_deadlines_only_wakes_for_its_eyes_and_then_goes_silent() {
    let mut app = app_with(blink_native::modules::deadlines::DemoSource::new(WALL_BASE).without_feeds(), blink_core::settings::normalize_settings(None), 1.0);
    assert!(!app.wants_frames());
    assert_eq!(app.state_json()["rest"]["count"], "Add a feed");
    // Quiet for 45 s: the eyes open (that is the one thing the notch wakes for).
    assert_eq!(app.next_wake(), Some(45_000));
    // Blinks come every few seconds for a while, then the eyes doze and nothing is scheduled again.
    app.clock.set_manual_ms(45_000);
    let blink = app.next_wake().expect("the next blink");
    assert!(blink > 45_000 && blink < 50_000, "{blink}");
    app.clock.set_manual_ms(151_000);
    assert_eq!(app.next_wake(), None, "dozing eyes never wake the notch");
}

#[test]
fn shell_modes_are_reachable_through_the_public_api() {
    let app = app();
    assert_eq!(app.shell().mode(), Mode::Rest);
}
#[test]
fn changing_the_resting_style_takes_effect_without_waiting_for_a_timer() {
    let mut app = common::app();
    common::settle(&mut app);
    assert_eq!(app.shell().rest_style(), blink_native::shell::RestStyle::Full);
    let settings = blink_core::settings::normalize_settings(Some(&serde_json::json!({ "restingStyle": "hover" })));
    app.apply_settings(settings, "en");
    // No tick in between: the shell must already be heading for the hot strip.
    assert_eq!(app.shell().rest_style(), blink_native::shell::RestStyle::Hot);
    assert!(app.wants_frames(), "the shrink animation must be asked for right away");
}
