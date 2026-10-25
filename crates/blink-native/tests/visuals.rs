//! The bars that move with the sound, and the accent that follows the cover.

mod common;

use blink_core::settings::normalize_settings;
use blink_native::app::App;
use blink_native::clock::Clock;
use blink_native::look::parse;
use blink_native::modules::clock::ClockModule;
use blink_native::modules::music::{Art, FakeLyrics, FakeMedia, MusicModule};
use blink_native::modules::{Module, PointerKind};
use blink_native::platform::spectrum::Spectrum;
use blink_native::ui::theme::Rgb;
use common::*;
use serde_json::json;
use std::sync::Arc;

fn rig() -> (App, Arc<FakeMedia>, Arc<Spectrum>) {
    let media = FakeMedia::new(WALL_BASE);
    let modules: Vec<Box<dyn Module>> = vec![Box::new(ClockModule::new()), Box::new(MusicModule::new(media.clone(), Arc::new(FakeLyrics), Arc::new(|| {})))];
    let mut app = App::new(Clock::manual(WALL_BASE), normalize_settings(None), tz(), 1.0, "en", modules).expect("app");
    let spectrum = Spectrum::new();
    app.set_spectrum(spectrum.clone());
    app.tick();
    app.pointer(PointerKind::Leave, 0.0, 0.0);
    advance(&mut app, 600);
    settle(&mut app);
    (app, media, spectrum)
}

#[test]
fn the_bars_stand_as_high_as_the_sound_and_fall_back_to_a_beat_when_it_cannot_be_heard() {
    let (app, _media, spectrum) = rig();
    // The capture is not working: a plain beat, never all flat while a song plays.
    let beat = app.bar_levels(1_000);
    assert!(beat.iter().any(|l| *l > 0.0));
    spectrum.set_for_test([1.0, 1.0, 0.5, 0.5, 0.0, 0.0, 0.1, 0.1]);
    let l = app.bar_levels(1_000);
    assert!(l[0] > 0.95 && l[1] > 0.4 && l[1] < 0.7 && l[2] < 0.01 && l[3] < 0.2, "{l:?}");
    assert_eq!(app.bar_levels(u64::MAX), [0.0; 4], "paused: flat");
}

#[test]
fn the_speakers_are_listened_to_only_while_the_bars_are_on_screen() {
    let (mut app, media, spectrum) = rig();
    app.tick();
    assert!(spectrum.wanted(), "a song plays and the pill shows its bars");
    media.set(|s| s.playing = false);
    app.data_changed();
    assert!(!spectrum.wanted(), "paused: nothing to listen for");
    media.set(|s| s.playing = true);
    app.data_changed();
    assert!(spectrum.wanted());
    // Turned off by the person: not even asked.
    app.set_look(parse(Some(&json!({ "look": { "visualizer": false } }))));
    app.data_changed();
    assert!(!spectrum.wanted());
    spectrum.set_for_test([1.0; 8]);
    let l = app.bar_levels(1_000);
    assert!(l.iter().any(|v| *v < 0.99), "with the visualizer off the beat is used, not the sound: {l:?}");
}

#[test]
fn the_accent_can_follow_the_cover_of_the_song_that_plays() {
    let (mut app, media, _) = rig();
    app.set_look(parse(Some(&json!({ "look": { "accent": "cover" } }))));
    let mut pixmap = tiny_skia::Pixmap::new(8, 8).unwrap();
    pixmap.fill(tiny_skia::Color::from_rgba8(230, 40, 90, 255));
    media.set(|s| s.art = Some(Arc::new(Art { pixmap: pixmap.clone(), accent: Rgb(230, 40, 90) })));
    app.tick();
    assert_eq!(app.chrome_accent(), Some(Rgb(230, 40, 90)), "the colour of the cover");
    media.set(|s| s.playing = false);
    app.data_changed();
    assert_eq!(app.chrome_accent(), None, "nothing playing: every page has its own colour again");
    // A fixed accent is a fixed accent.
    app.set_look(parse(Some(&json!({ "look": { "accent": "pink" } }))));
    assert_eq!(app.chrome_accent(), Some(Rgb(0xf4, 0x72, 0xb6)));
}
