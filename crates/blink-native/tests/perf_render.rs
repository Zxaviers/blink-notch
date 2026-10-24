//! How long a frame takes to draw, per page. Run on release: `cargo test --release -p blink-native --test perf_render -- --ignored --nocapture`.

mod common;

use blink_core::lyrics::{Line, Lyrics, LyricsSource, Query};
use blink_core::settings::normalize_settings;
use blink_native::app::App;
use blink_native::clock::Clock;
use blink_native::modules::clock::ClockModule;
use blink_native::modules::deadlines::{DeadlinesModule, DemoSource};
use blink_native::modules::music::{FakeMedia, MusicModule};
use blink_native::modules::Module;
use common::*;
use std::sync::Arc;
use std::time::Instant;

struct Lyr;
impl LyricsSource for Lyr {
    fn fetch(&self, _q: &Query) -> Lyrics {
        Lyrics::Synced((0..30).map(|i| Line { at_ms: i * 4000, text: format!("line number {i} of the song, long enough to need some room") }).collect())
    }
}

fn time(app: &mut App, what: &str) {
    for _ in 0..5 {
        app.render();
    }
    let n = 60;
    let t = Instant::now();
    for _ in 0..n {
        app.render();
    }
    println!("{what}: {:.2} ms per frame", t.elapsed().as_secs_f64() * 1000.0 / n as f64);
}

#[test]
#[ignore]
fn how_long_each_page_takes_to_draw() {
    let media = FakeMedia::new(WALL_BASE);
    let music = MusicModule::new(media, Arc::new(Lyr), Arc::new(|| {}));
    let modules: Vec<Box<dyn Module>> = vec![Box::new(ClockModule::new()), Box::new(DeadlinesModule::new(Box::new(DemoSource::new(WALL_BASE)))), Box::new(music)];
    let mut app = App::new(Clock::manual(WALL_BASE), normalize_settings(None), tz(), 1.0, "en", modules).expect("app");
    app.tick();
    open_by_hover(&mut app);
    settle(&mut app);
    time(&mut app, "clock page");
    app.switch_to(1);
    settle(&mut app);
    time(&mut app, "deadlines page");
    app.switch_to(2);
    settle(&mut app);
    time(&mut app, "music page, cover view");
    app.key(blink_native::modules::Key::Char('l'));
    settle(&mut app);
    time(&mut app, "music page, lyrics view");
}

#[test]
#[ignore]
fn how_long_the_canvas_basics_take() {
    use blink_native::shell::{OPEN_R, REST_FULL};
    use blink_native::ui::canvas::Canvas;
    use blink_native::ui::theme::Rgb;
    let mut c = Canvas::new(640, 360, 1.0);
    let shape = blink_native::shell::Shape { w: 540.0, h: 190.0, r: OPEN_R };
    let _ = REST_FULL;
    let n = 300;
    let t = Instant::now();
    for _ in 0..n {
        c.clear();
    }
    println!("clear: {:.3} ms", t.elapsed().as_secs_f64() * 1000.0 / n as f64);
    let t = Instant::now();
    for _ in 0..n {
        c.fill_notch(shape, Rgb(0, 0, 0), 1.0);
    }
    println!("fill_notch: {:.3} ms", t.elapsed().as_secs_f64() * 1000.0 / n as f64);
    let t = Instant::now();
    for _ in 0..n {
        c.push_notch_clip(shape);
        c.pop_clip();
    }
    println!("push_notch_clip: {:.3} ms", t.elapsed().as_secs_f64() * 1000.0 / n as f64);
    c.push_notch_clip(shape);
    let t = Instant::now();
    for _ in 0..n {
        c.push_clip(60.0, 60.0, 200.0, 40.0);
        c.pop_clip();
    }
    println!("push_clip inside the notch clip: {:.3} ms", t.elapsed().as_secs_f64() * 1000.0 / n as f64);
    let t = Instant::now();
    for _ in 0..n {
        c.rrect(60.0, 60.0, 200.0, 40.0, 8.0, Rgb(255, 255, 255), 0.5);
    }
    println!("rrect with the notch mask: {:.3} ms", t.elapsed().as_secs_f64() * 1000.0 / n as f64);
    let t = Instant::now();
    for _ in 0..n {
        c.gradient_rrect_alpha(50.0, 20.0, 540.0, 190.0, 0.0, (Rgb(200, 0, 50), 0.0), (Rgb(200, 0, 50), 0.38), true);
    }
    println!("full-panel gradient wash: {:.3} ms", t.elapsed().as_secs_f64() * 1000.0 / n as f64);
    let t = Instant::now();
    for _ in 0..n {
        c.glow(120.0, 100.0, 70.0, Rgb(200, 0, 50), 0.4);
    }
    println!("glow r70: {:.3} ms", t.elapsed().as_secs_f64() * 1000.0 / n as f64);
    c.pop_clip();
    let t = Instant::now();
    for _ in 0..n {
        let _ = c.content_rows();
    }
    println!("content_rows: {:.3} ms", t.elapsed().as_secs_f64() * 1000.0 / n as f64);
}
