//! Shared setup for the app-level tests: a manual clock, a fixed time zone,
//! sample deadlines relative to the clock.
#![allow(dead_code)]

use blink_core::settings::{normalize_settings, Settings};
use blink_native::app::App;
use blink_native::clock::Clock;
use blink_native::modules::deadlines::{DeadlinesModule, DemoSource};
use blink_native::modules::Module;
use jiff::tz::TimeZone;
use serde_json::json;

/// 2026-10-22 12:00 UTC.
pub const WALL_BASE: i64 = 1_792_670_400_000;

pub fn tz() -> TimeZone {
    TimeZone::get("Asia/Jakarta").unwrap()
}

pub fn app_with(source: DemoSource, settings: Settings, scale: f32) -> App {
    let modules: Vec<Box<dyn Module>> = vec![Box::new(DeadlinesModule::new(Box::new(source)))];
    App::new(Clock::manual(WALL_BASE), settings, tz(), scale, "en", modules).expect("app")
}

pub fn app() -> App {
    app_with(DemoSource::new(WALL_BASE), normalize_settings(None), 1.0)
}

pub fn app_with_settings(patch: serde_json::Value) -> App {
    app_with(DemoSource::new(WALL_BASE), normalize_settings(Some(&patch)), 1.0)
}

pub fn alpha(app: &App, x: u32, y: u32) -> u8 {
    app.canvas().pm.pixel(x, y).map_or(0, |p| p.alpha())
}

/// Move the clock forward in 8 ms steps, ticking the app like the window loop would.
pub fn advance(app: &mut App, ms: u64) {
    let target = app.now() + ms;
    while app.now() < target {
        let step = (target - app.now()).min(8);
        let t = app.now() + step;
        app.clock.set_manual_ms(t);
        app.tick();
    }
}

pub fn settle(app: &mut App) {
    advance(app, 400);
}

/// Hover the pill and let it open.
pub fn open_by_hover(app: &mut App) {
    app.pointer(blink_native::modules::PointerKind::Move, 320.0, 10.0);
    advance(app, 160);
    settle(app);
}

pub fn dump(app: &mut App, name: &str) {
    let dir = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("..").join("..").join("scripts").join("out");
    std::fs::create_dir_all(&dir).unwrap();
    app.dump_png(dir.join(format!("{name}.png")).to_str().unwrap()).unwrap();
}

pub fn default_settings_json() -> serde_json::Value {
    json!({})
}