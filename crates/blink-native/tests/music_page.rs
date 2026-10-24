//! The music page with a stand-in player and stand-in lyrics: what a click, a key, a drag and the
//! passing of time do.

mod common;

use blink_core::lyrics::{Line, Lyrics, LyricsSource, Query};
use blink_core::settings::normalize_settings;
use blink_native::app::App;
use blink_native::clock::Clock;
use blink_native::modules::deadlines::{DeadlinesModule, DemoSource};
use blink_native::modules::music::{FakeMedia, MediaCommand, MusicModule};
use blink_native::modules::{Key, Module, PointerKind};
use common::*;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::Duration;

/// Answers at once and counts how many times it was asked.
struct CountingLyrics {
    asked: AtomicUsize,
    lyrics: Lyrics,
}

impl LyricsSource for CountingLyrics {
    fn fetch(&self, _query: &Query) -> Lyrics {
        self.asked.fetch_add(1, Ordering::SeqCst);
        self.lyrics.clone()
    }
}

fn synced() -> Lyrics {
    Lyrics::Synced(vec![
        Line { at_ms: 10_000, text: "first".into() },
        Line { at_ms: 20_000, text: "second".into() },
        Line { at_ms: 30_000, text: "third".into() },
        Line { at_ms: 40_000, text: "fourth".into() },
    ])
}

struct Rig {
    app: App,
    media: Arc<FakeMedia>,
    lyrics: Arc<CountingLyrics>,
}

fn rig(options: Option<serde_json::Value>) -> Rig {
    let media = FakeMedia::new(WALL_BASE);
    let lyrics = Arc::new(CountingLyrics { asked: AtomicUsize::new(0), lyrics: synced() });
    let mut music = MusicModule::new(media.clone(), lyrics.clone(), Arc::new(|| {}));
    if let Some(o) = options {
        music.set_options(&o);
    }
    let modules: Vec<Box<dyn Module>> = vec![Box::new(DeadlinesModule::new(Box::new(DemoSource::new(WALL_BASE)))), Box::new(music)];
    let mut app = App::new(Clock::manual(WALL_BASE), normalize_settings(None), tz(), 1.0, "en", modules).expect("app");
    app.tick();
    open_by_hover(&mut app);
    app.switch_to(1);
    settle(&mut app);
    app.render();
    media.take_log();
    Rig { app, media, lyrics }
}

fn names(app: &mut App) -> Vec<String> {
    app.render();
    app.accessibility().nodes.iter().map(|n| n.name.clone()).collect()
}

fn rect_of(app: &mut App, name: &str) -> (f32, f32, f32, f32) {
    app.render();
    app.accessibility().nodes.iter().find(|n| n.name == name || n.name.starts_with(name)).unwrap_or_else(|| panic!("no control named {name}")).rect
}

fn click(app: &mut App, name: &str) {
    let r = rect_of(app, name);
    let (x, y) = (r.0 + r.2 / 2.0, r.1 + r.3 / 2.0);
    app.pointer(PointerKind::Move, x, y);
    app.pointer(PointerKind::Down, x, y);
    app.pointer(PointerKind::Up, x, y);
    app.render();
}

/// Let the lookup thread answer (it runs on its own thread).
fn wait_for_lyrics(app: &mut App) {
    for _ in 0..200 {
        app.tick();
        app.render();
        if app.state_json()["module"]["lyrics"] == "synced" || app.state_json()["module"]["lyrics"] == "not-found" || app.state_json()["module"]["lyrics"] == "plain" {
            return;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    panic!("the lyrics never came: {}", app.state_json()["module"]);
}

#[test]
fn the_page_shows_the_song_and_its_buttons() {
    let mut r = rig(None);
    let n = names(&mut r.app);
    assert!(n.iter().any(|n| n == "Now playing: Paper Sky, Lumen Coast"), "{n:?}");
    for want in ["Previous", "Pause", "Next"] {
        assert!(n.iter().any(|x| x == want), "{want} in {n:?}");
    }
    assert_eq!(r.app.active_module(), "music");
}

#[test]
fn the_buttons_and_keys_control_the_player() {
    let mut r = rig(None);
    click(&mut r.app, "Pause");
    click(&mut r.app, "Next");
    click(&mut r.app, "Previous");
    assert_eq!(r.media.take_log(), vec![MediaCommand::PlayPause, MediaCommand::Next, MediaCommand::Previous]);
    assert!(names(&mut r.app).iter().any(|n| n == "Play"), "the button follows the player: it is paused now");
    r.app.hotkey();
    settle(&mut r.app);
    r.app.switch_to(1);
    r.media.take_log();
    r.app.key(Key::Space);
    r.app.key(Key::Char('n'));
    r.app.key(Key::Char('p'));
    assert_eq!(r.media.take_log(), vec![MediaCommand::PlayPause, MediaCommand::Next, MediaCommand::Previous]);
}

#[test]
fn the_progress_follows_time_and_a_click_or_drag_on_the_bar_seeks() {
    let mut r = rig(None);
    // 1:00 in when the player said so; opening the notch took about a second of the test clock.
    let p0 = r.app.state_json()["module"]["position"].as_u64().unwrap();
    assert!((60_000..62_000).contains(&p0), "{p0}");
    advance(&mut r.app, 5_000);
    assert_eq!(r.app.state_json()["module"]["position"].as_u64().unwrap(), p0 + 5_000, "playing: it moves with the clock");
    let seek = r.app.state_json()["module"]["hits"].as_array().unwrap().iter().find(|h| h["target"] == "Seek").cloned().expect("a seek bar");
    let (x, y, w, h) = (seek["x"].as_f64().unwrap() as f32, seek["y"].as_f64().unwrap() as f32, seek["w"].as_f64().unwrap() as f32, seek["h"].as_f64().unwrap() as f32);
    let cy = y + h / 2.0;
    // The track is 4 px shorter than the hit area at each end.
    let (a, b) = (x + 4.0, x + w - 4.0);
    r.app.pointer(PointerKind::Move, a + (b - a) * 0.5, cy);
    r.app.pointer(PointerKind::Down, a + (b - a) * 0.5, cy);
    assert!(r.app.pointer_captured(), "a drag keeps the mouse");
    assert!(r.media.take_log().is_empty(), "nothing is sent until the button is let go");
    r.app.pointer(PointerKind::Move, a + (b - a) * 0.25, cy);
    r.app.pointer(PointerKind::Up, a + (b - a) * 0.25, cy);
    assert_eq!(r.media.take_log(), vec![MediaCommand::Seek(54_000)]);
}

#[test]
fn lyrics_are_only_asked_for_when_turned_on_and_follow_the_song() {
    let mut r = rig(None);
    assert_eq!(r.lyrics.asked.load(Ordering::SeqCst), 0, "nothing leaves the computer until lyrics are turned on");
    click(&mut r.app, "Lyrics");
    wait_for_lyrics(&mut r.app);
    assert_eq!(r.lyrics.asked.load(Ordering::SeqCst), 1);
    // 1:00 is after every line: the last one is current.
    assert_eq!(r.app.state_json()["module"]["scrollTo"], 3);
    // Jump back to 0:25: the second line.
    r.media.set(|s| s.position_ms = 25_000);
    advance(&mut r.app, 400);
    r.app.render();
    assert_eq!(r.app.state_json()["module"]["scrollTo"], 1);
    // Clicking a line seeks to it.
    let hit = r.app.state_json()["module"]["hits"].as_array().unwrap().iter().find(|h| h["target"] == "Line(30000)").cloned().expect("the third line is on screen");
    let (x, y) = (hit["x"].as_f64().unwrap() as f32 + 20.0, hit["y"].as_f64().unwrap() as f32 + 10.0);
    r.app.pointer(PointerKind::Move, x, y);
    r.app.pointer(PointerKind::Down, x, y);
    assert_eq!(r.media.take_log(), vec![MediaCommand::Seek(30_000)]);
    // Turning lyrics off and on again does not ask a second time.
    click(&mut r.app, "Lyrics");
    click(&mut r.app, "Lyrics");
    assert_eq!(r.lyrics.asked.load(Ordering::SeqCst), 1, "remembered for the song");
}

#[test]
fn the_lyrics_option_can_forbid_the_lookup_altogether() {
    let mut r = rig(Some(serde_json::json!({ "lyrics": false })));
    click(&mut r.app, "Lyrics");
    for _ in 0..20 {
        r.app.tick();
        std::thread::sleep(Duration::from_millis(5));
    }
    assert_eq!(r.lyrics.asked.load(Ordering::SeqCst), 0, "no lookup, ever");
}

#[test]
fn a_new_song_peeks_but_the_first_one_does_not() {
    let mut r = rig(None);
    r.app.configure_modules(&[blink_native::host::ModuleConfig { id: "music".into(), enabled: true, options: serde_json::json!({ "autoOpen": 0 }) }]);
    r.app.pointer(PointerKind::Leave, 0.0, 0.0);
    advance(&mut r.app, 600);
    settle(&mut r.app);
    assert!(r.app.current_peek().is_none(), "the song already playing at the start is not news");
    r.media.set(|s| {
        s.title = "Harbour Lights".into();
        s.artist = "Someone Else".into();
        s.track += 1;
    });
    r.app.data_changed();
    let p = r.app.current_peek().expect("a new song peeks");
    assert_eq!((p.title.as_str(), p.code.as_str()), ("Harbour Lights", "Someone Else"));
}

#[test]
fn with_nothing_playing_the_page_says_so() {
    let mut r = rig(None);
    r.media.set(|s| s.active = false);
    let n = names(&mut r.app);
    assert!(n.iter().any(|n| n == "Nothing playing"), "{n:?}");
    assert!(!n.iter().any(|n| n == "Pause" || n == "Play"));
}

/// Fails the first time (the service cannot be reached), then answers.
struct Flaky(AtomicUsize);

impl LyricsSource for Flaky {
    fn fetch(&self, _query: &Query) -> Lyrics {
        if self.0.fetch_add(1, Ordering::SeqCst) == 0 { Lyrics::Unavailable } else { synced() }
    }
}

#[test]
fn when_the_lyrics_service_cannot_be_reached_a_click_asks_again() {
    let media = FakeMedia::new(WALL_BASE);
    let source = Arc::new(Flaky(AtomicUsize::new(0)));
    let modules: Vec<Box<dyn Module>> = vec![Box::new(DeadlinesModule::new(Box::new(DemoSource::new(WALL_BASE)))), Box::new(MusicModule::new(media, source.clone(), Arc::new(|| {})))];
    let mut app = App::new(Clock::manual(WALL_BASE), normalize_settings(None), tz(), 1.0, "en", modules).expect("app");
    app.tick();
    open_by_hover(&mut app);
    app.switch_to(1);
    settle(&mut app);
    click(&mut app, "Lyrics");
    for _ in 0..200 {
        app.tick();
        app.render();
        if app.state_json()["module"]["lyrics"] == "unavailable" {
            break;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    assert_eq!(app.state_json()["module"]["lyrics"], "unavailable", "the service was down");
    // A click anywhere on the message tries again.
    let hit = app.state_json()["module"]["hits"].as_array().unwrap().iter().find(|h| h["target"] == "RetryLyrics").cloned().expect("a retry area");
    let (x, y) = (hit["x"].as_f64().unwrap() as f32 + 30.0, hit["y"].as_f64().unwrap() as f32 + 30.0);
    app.pointer(PointerKind::Move, x, y);
    app.pointer(PointerKind::Down, x, y);
    wait_for_lyrics(&mut app);
    assert_eq!(app.state_json()["module"]["lyrics"], "synced");
    assert_eq!(source.0.load(Ordering::SeqCst), 2);
}

/// A resting notch (no hover, nothing open) with a player: music is the second page.
fn resting_rig() -> Rig {
    let media = FakeMedia::new(WALL_BASE);
    let lyrics = Arc::new(CountingLyrics { asked: AtomicUsize::new(0), lyrics: synced() });
    let music = MusicModule::new(media.clone(), lyrics.clone(), Arc::new(|| {}));
    let modules: Vec<Box<dyn Module>> = vec![Box::new(DeadlinesModule::new(Box::new(DemoSource::new(WALL_BASE)))), Box::new(music)];
    let mut app = App::new(Clock::manual(WALL_BASE), normalize_settings(None), tz(), 1.0, "en", modules).expect("app");
    app.tick();
    app.pointer(PointerKind::Leave, 0.0, 0.0);
    advance(&mut app, 600);
    settle(&mut app);
    Rig { app, media, lyrics }
}

#[test]
fn a_song_that_starts_opens_the_notch_by_itself_on_the_music_and_its_lyrics() {
    let mut r = resting_rig();
    assert_eq!(r.app.state_json()["mode"], "rest");
    r.media.set(|s| {
        s.title = "Harbour Lights".into();
        s.track += 1;
    });
    r.app.data_changed();
    settle(&mut r.app);
    r.app.render();
    let state = r.app.state_json();
    assert_eq!(state["mode"], "open", "no hover needed");
    assert_eq!(r.app.active_module(), "music");
    assert_eq!(state["module"]["showLyrics"], true, "with the lyrics");
    assert_eq!(r.lyrics.asked.load(Ordering::SeqCst), 1, "the lyrics were asked for at once");
    // It goes away by itself after a while.
    advance(&mut r.app, 11_000);
    settle(&mut r.app);
    assert_eq!(r.app.state_json()["mode"], "rest");
}

#[test]
fn the_notch_stays_open_while_the_pointer_is_on_it_and_a_song_that_was_already_playing_does_not_open_it() {
    let mut r = resting_rig();
    // The song playing since the start did not open anything (the rig started with it playing).
    assert_eq!(r.app.state_json()["mode"], "rest");
    r.media.set(|s| s.track += 1);
    r.app.data_changed();
    settle(&mut r.app);
    r.app.render();
    assert_eq!(r.app.state_json()["mode"], "open");
    let (cx, cy) = (320.0, 40.0);
    r.app.pointer(PointerKind::Move, cx, cy);
    advance(&mut r.app, 15_000);
    assert_eq!(r.app.state_json()["mode"], "open", "the pointer on it keeps it open");
    r.app.pointer(PointerKind::Leave, 0.0, 0.0);
    advance(&mut r.app, 2_000);
    settle(&mut r.app);
    assert_eq!(r.app.state_json()["mode"], "rest", "and it closes once the pointer is gone");
}

#[test]
fn the_auto_open_can_be_turned_off_or_made_longer() {
    let mut r = resting_rig();
    r.app.configure_modules(&[blink_native::host::ModuleConfig { id: "music".into(), enabled: true, options: serde_json::json!({ "autoOpen": 0, "peek": false }) }]);
    r.media.set(|s| s.track += 1);
    r.app.data_changed();
    settle(&mut r.app);
    assert_eq!(r.app.state_json()["mode"], "rest", "autoOpen 0 keeps the notch where it was");
}
