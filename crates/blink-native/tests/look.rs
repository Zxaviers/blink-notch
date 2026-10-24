//! Palettes, accents and pill shapes: the notch looks the way it was told to.

mod common;

use blink_native::look::{parse, Look, PILLS};
use blink_native::modules::PointerKind;
use common::*;
use serde_json::json;

fn rest_width(app: &mut blink_native::app::App) -> f32 {
    app.state_json()["shape"]["w"].as_f64().unwrap() as f32
}

#[test]
fn a_pill_shape_changes_the_resting_pill_and_back() {
    let mut app = app();
    app.tick();
    app.pointer(PointerKind::Leave, 0.0, 0.0);
    advance(&mut app, 600);
    settle(&mut app);
    assert_eq!(rest_width(&mut app), 232.0);
    app.set_look(parse(Some(&json!({ "look": { "pill": "wide" } }))));
    advance(&mut app, 600);
    assert_eq!(rest_width(&mut app), 300.0);
    app.render();
    // The wider pill really is drawn wider: opaque pixels well outside the classic pill, none beyond the new one.
    assert_eq!(alpha(&app, 320 - 140, 4), 255);
    assert_eq!(alpha(&app, 320 - 170, 4), 0);
    app.set_look(parse(Some(&json!({ "look": { "pill": "compact" } }))));
    advance(&mut app, 600);
    assert_eq!(rest_width(&mut app), 184.0);
    app.set_look(Look::default());
    advance(&mut app, 600);
    assert_eq!(rest_width(&mut app), 232.0);
}

#[test]
fn a_palette_colours_the_body_of_the_pill_and_of_the_open_panel() {
    let mut app = app();
    app.tick();
    app.pointer(PointerKind::Leave, 0.0, 0.0);
    advance(&mut app, 600);
    settle(&mut app);
    app.render();
    let px = app.canvas().pm.pixel(320 - 100, 4).unwrap();
    assert_eq!((px.red(), px.green(), px.blue()), (0, 0, 0), "midnight is plain black");
    app.set_look(parse(Some(&json!({ "look": { "palette": "aurora" } }))));
    app.render();
    let px = app.canvas().pm.pixel(320 - 100, 4).unwrap();
    assert_eq!((px.red(), px.green(), px.blue()), (0x0a, 0x0e, 0x20), "the pill takes the palette's body");
    open_by_hover(&mut app);
    app.render();
    // Inside the open panel, away from the corner light, the body is the palette's too.
    let px = app.canvas().pm.pixel(320 + 200, 150).unwrap();
    assert!(px.blue() > px.red() && px.blue() >= 0x10, "{px:?}");
    dump(&mut app, "look-aurora");
}

#[test]
fn an_accent_colours_the_title_and_leaves_the_rest_alone() {
    let mut plain = app();
    open_by_hover(&mut plain);
    plain.render();
    let mut pink = app();
    pink.set_look(parse(Some(&json!({ "look": { "accent": "pink" } }))));
    open_by_hover(&mut pink);
    pink.render();
    // The second half of the title is drawn in the accent: find a pink-ish pixel in the title row that the plain one lacks.
    let has_pink = |app: &blink_native::app::App| (60..220).any(|x| (10..34).any(|y| app.canvas().pm.pixel(x, y).is_some_and(|p| p.red() > 200 && p.green() < 140 && p.blue() > 140 && p.alpha() == 255)));
    assert!(has_pink(&pink), "the accent is in the title");
    assert!(!has_pink(&plain), "and the default is not pink");
    dump(&mut pink, "look-pink");
}

#[test]
fn every_pill_shape_and_palette_can_be_chosen_by_name() {
    for (name, shape) in PILLS {
        let look = parse(Some(&json!({ "look": { "pill": name } })));
        assert_eq!(look.pill(), shape, "{name}");
    }
    for id in ["midnight", "graphite", "aurora", "sunset", "forest", "ocean"] {
        assert_eq!(parse(Some(&json!({ "look": { "palette": id } }))).palette().id, id);
    }
}
