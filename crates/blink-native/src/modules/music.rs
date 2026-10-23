//! Music: what is playing on the computer (Spotify, a browser, any player that tells Windows),
//! with the cover, the progress bar, the buttons, and synced lyrics that light up line by line.
//!
//! Where the song comes from is a `MediaBackend` (`platform::media` for real, `FakeMedia` in
//! tests and scripted runs). Lyrics come from a `LyricsSource` (LRCLIB), and only after the person
//! turns lyrics on for a song: that is the one moment the title and artist leave the computer.

use std::collections::HashMap;
use std::sync::mpsc::{channel, Receiver, Sender};
use std::sync::{Arc, Mutex};

use blink_core::i18n::Lang;
use blink_core::lyrics::{current_line, Lyrics, LyricsSource, Query};
use serde_json::Value;

use super::{Action, Effect, Env, Key, KeyResult, Kit, Module, PanelFrame, PeekView, PointerKind, RestView, CONTENT_H};
use crate::access::{Node, NodeAction, Role, ROOT};
use crate::ui::canvas::Icon;
use crate::ui::text::Face;
use crate::ui::theme::Rgb;

// ----------------------------------------------------------------- the model

/// A cover picture, already small, and the colour that stands for it.
pub struct Art {
    pub pixmap: tiny_skia::Pixmap,
    pub accent: Rgb,
}

#[derive(Clone, Default)]
pub struct MediaState {
    /// Something is open in a player (playing or paused).
    pub active: bool,
    /// The player's name, when it says.
    pub app: String,
    pub title: String,
    pub artist: String,
    pub album: String,
    pub playing: bool,
    /// Where the song was at `sampled_wall_ms`.
    pub position_ms: u64,
    pub sampled_wall_ms: i64,
    pub rate: f32,
    pub duration_ms: u64,
    pub can_seek: bool,
    pub can_prev: bool,
    pub can_next: bool,
    /// Changes with every new song (title or artist).
    pub track: u64,
    pub art: Option<Arc<Art>>,
}

impl MediaState {
    /// Where the song is at `wall_ms`: the sample, moved on by the time that has passed while playing.
    pub fn position_at(&self, wall_ms: i64) -> u64 {
        let moved = if self.playing { ((wall_ms - self.sampled_wall_ms).max(0) as f32 * if self.rate > 0.0 { self.rate } else { 1.0 }) as u64 } else { 0 };
        let pos = self.position_ms + moved;
        if self.duration_ms > 0 { pos.min(self.duration_ms) } else { pos }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum MediaCommand {
    PlayPause,
    Next,
    Previous,
    Seek(u64),
}

pub trait MediaBackend: Send + Sync {
    fn state(&self) -> MediaState;
    /// Ask for something to be done. Never blocks. `wall_ms` is the time now (for backends that keep a position).
    fn send(&self, command: MediaCommand, wall_ms: i64);
}

/// A backend that only remembers: tests and scripted runs drive the page with it.
pub struct FakeMedia {
    state: Mutex<MediaState>,
    log: Mutex<Vec<MediaCommand>>,
}

impl FakeMedia {
    /// "Paper Sky" by "Lumen Coast", 3:36 long, playing, 1:00 in at wall time `wall_ms`.
    pub fn new(wall_ms: i64) -> Arc<FakeMedia> {
        Arc::new(FakeMedia {
            state: Mutex::new(MediaState {
                active: true,
                app: "Player".into(),
                title: "Paper Sky".into(),
                artist: "Lumen Coast".into(),
                album: "Summer Flows".into(),
                playing: true,
                position_ms: 60_000,
                sampled_wall_ms: wall_ms,
                rate: 1.0,
                duration_ms: 216_000,
                can_seek: true,
                can_prev: true,
                can_next: true,
                track: 1,
                art: None,
            }),
            log: Mutex::new(Vec::new()),
        })
    }

    pub fn set(&self, change: impl FnOnce(&mut MediaState)) {
        change(&mut self.state.lock().unwrap());
    }

    pub fn take_log(&self) -> Vec<MediaCommand> {
        std::mem::take(&mut *self.log.lock().unwrap())
    }
}

impl MediaBackend for FakeMedia {
    fn state(&self) -> MediaState {
        self.state.lock().unwrap().clone()
    }

    fn send(&self, command: MediaCommand, wall_ms: i64) {
        {
            let mut s = self.state.lock().unwrap();
            match &command {
                MediaCommand::PlayPause => {
                    s.position_ms = s.position_at(wall_ms);
                    s.sampled_wall_ms = wall_ms;
                    s.playing = !s.playing;
                }
                MediaCommand::Seek(ms) => {
                    s.position_ms = *ms;
                    s.sampled_wall_ms = wall_ms;
                }
                MediaCommand::Next | MediaCommand::Previous => {}
            }
        }
        self.log.lock().unwrap().push(command);
    }
}

/// What the page can be asked to do (screen readers use these).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum MusicAction {
    PlayPause,
    Next,
    Previous,
    ToggleLyrics,
}

/// Lyrics that need no network: a made-up song, a line every four seconds. For tests and scripted runs.
pub struct FakeLyrics;

impl LyricsSource for FakeLyrics {
    fn fetch(&self, query: &Query) -> Lyrics {
        if query.title.is_empty() || query.title == "Silence" {
            return Lyrics::None;
        }
        const WORDS: [&str; 12] = [
            "paper birds over the harbour",
            "carry the morning on their wings",
            "every window holds a song",
            "that the city never sings",
            "we follow the light along the water",
            "until the tide forgets our names",
            "and the lamps come on one by one",
            "like a slow and quiet flame",
            "hold the line, hold the sky",
            "we were never going far",
            "just to hear the evening hum",
            "under a thousand stars",
        ];
        Lyrics::Synced(WORDS.iter().enumerate().map(|(i, w)| blink_core::lyrics::Line { at_ms: 4_000 * (i as u64 + 1), text: (*w).to_string() }).collect())
    }
}

// ------------------------------------------------------------------- strings

fn word(lang: Lang, key: &str) -> &'static str {
    let id = lang == Lang::Id;
    match key {
        "title" => if id { "Musik" } else { "Music" },
        "nothing" => if id { "Tidak ada yang diputar" } else { "Nothing playing" },
        "hint" => if id { "Putar lagu di Spotify atau peramban." } else { "Play something in Spotify or a browser." },
        "play" => if id { "Putar" } else { "Play" },
        "pause" => if id { "Jeda" } else { "Pause" },
        "next" => if id { "Berikutnya" } else { "Next" },
        "previous" => if id { "Sebelumnya" } else { "Previous" },
        "lyrics" => if id { "Lirik" } else { "Lyrics" },
        "loading" => if id { "Mencari lirik…" } else { "Looking for lyrics…" },
        "none" => if id { "Lirik tidak ditemukan" } else { "No lyrics found" },
        "plain" => if id { "Lirik tanpa waktu" } else { "Lyrics without timing" },
        "off" => if id { "Pencarian lirik dimatikan" } else { "Lyrics lookup is off" },
        "now" => if id { "Sedang diputar" } else { "Now playing" },
        _ => "",
    }
}

fn mmss(ms: u64) -> String {
    let s = ms / 1000;
    format!("{:02}:{:02}", s / 60, s % 60)
}

// -------------------------------------------------------------------- module

const VIOLET: Rgb = Rgb(0xa7, 0x8b, 0xfa);
/// How long the lyrics take to scroll to the next line.
const SCROLL_MS: u64 = 340;
const LINE_H: f32 = 27.0;

enum LyricsState {
    Loading,
    Ready(Lyrics),
}

#[derive(Clone, Debug, PartialEq)]
enum Target {
    PlayPause,
    Next,
    Previous,
    ToggleLyrics,
    /// The progress bar: the track runs between the two x values.
    Seek,
    /// A line of the lyrics (click to jump there).
    Line(u64),
}

#[derive(Clone, Debug)]
struct Hit {
    x: f32,
    y: f32,
    w: f32,
    h: f32,
    target: Target,
    track: (f32, f32),
}

impl Hit {
    fn contains(&self, px: f32, py: f32) -> bool {
        px >= self.x && px < self.x + self.w && py >= self.y && py < self.y + self.h
    }
}

pub struct MusicModule {
    backend: Arc<dyn MediaBackend>,
    source: Arc<dyn LyricsSource>,
    wake: Arc<dyn Fn() + Send + Sync>,
    lyrics: HashMap<String, LyricsState>,
    results_tx: Sender<(String, Lyrics)>,
    results_rx: Receiver<(String, Lyrics)>,
    show_lyrics: bool,
    /// `lyrics` option: may the title and artist be sent to look lyrics up?
    lookup_allowed: bool,
    /// `peek` option: a peek when the song changes.
    peek_on_change: bool,
    seen_track: Option<u64>,
    hits: Vec<Hit>,
    pointer: Option<(f32, f32)>,
    /// A seek in progress: where the pointer is, in milliseconds.
    drag_ms: Option<u64>,
    accent: Rgb,
    /// The lyrics scroll: from this line (fractional) to that one, since this moment.
    scroll_from: f32,
    scroll_to: usize,
    scroll_start: u64,
    animating: bool,
    plain_scroll: f32,
    last_draw_ms: u64,
}

impl MusicModule {
    pub fn new(backend: Arc<dyn MediaBackend>, source: Arc<dyn LyricsSource>, wake: Arc<dyn Fn() + Send + Sync>) -> Self {
        let (results_tx, results_rx) = channel();
        MusicModule {
            backend,
            source,
            wake,
            lyrics: HashMap::new(),
            results_tx,
            results_rx,
            show_lyrics: false,
            lookup_allowed: true,
            peek_on_change: true,
            seen_track: None,
            hits: Vec::new(),
            pointer: None,
            drag_ms: None,
            accent: VIOLET,
            scroll_from: 0.0,
            scroll_to: 0,
            scroll_start: 0,
            animating: false,
            plain_scroll: 0.0,
            last_draw_ms: 0,
        }
    }

    pub fn lyrics_shown(&self) -> bool {
        self.show_lyrics
    }

    fn query(st: &MediaState) -> Query {
        Query { artist: st.artist.clone(), title: st.title.clone(), album: st.album.clone(), duration_s: (st.duration_ms > 0).then_some((st.duration_ms / 1000) as u32) }
    }

    /// Ask for this song's lyrics if lyrics are on and they have not been asked for yet.
    fn ensure_lyrics(&mut self, st: &MediaState) {
        if !self.show_lyrics || !self.lookup_allowed || !st.active || st.title.is_empty() {
            return;
        }
        let query = Self::query(st);
        let key = query.key();
        if self.lyrics.contains_key(&key) {
            return;
        }
        self.lyrics.insert(key.clone(), LyricsState::Loading);
        let (tx, source, wake) = (self.results_tx.clone(), self.source.clone(), self.wake.clone());
        let _ = std::thread::Builder::new().name("blink-lyrics".into()).spawn(move || {
            let found = source.fetch(&query);
            let _ = tx.send((key, found));
            wake();
        });
    }

    fn drain_lyrics(&mut self) -> bool {
        let mut any = false;
        while let Ok((key, found)) = self.results_rx.try_recv() {
            self.lyrics.insert(key, LyricsState::Ready(found));
            any = true;
        }
        any
    }

    fn lyrics_for(&self, st: &MediaState) -> Option<&LyricsState> {
        self.lyrics.get(&Self::query(st).key())
    }

    fn command(&self, c: MediaCommand, env: &Env) {
        self.backend.send(c, env.wall_ms);
    }

    fn do_action(&mut self, action: &MusicAction, env: &Env) -> Vec<Effect> {
        match action {
            MusicAction::PlayPause => self.command(MediaCommand::PlayPause, env),
            MusicAction::Next => self.command(MediaCommand::Next, env),
            MusicAction::Previous => self.command(MediaCommand::Previous, env),
            MusicAction::ToggleLyrics => {
                self.show_lyrics = !self.show_lyrics;
                let st = self.backend.state();
                self.ensure_lyrics(&st);
            }
        }
        vec![Effect::Redraw]
    }

    fn seek_value(hit: &Hit, x: f32, duration_ms: u64) -> u64 {
        let (a, b) = hit.track;
        (((x - a) / (b - a).max(1.0)).clamp(0.0, 1.0) * duration_ms as f32) as u64
    }

    fn over(&self, x: f32, y: f32, w: f32, h: f32) -> bool {
        self.pointer.is_some_and(|(px, py)| px >= x && px < x + w && py >= y && py < y + h)
    }
}

// ------------------------------------------------------------------- drawing

#[allow(clippy::too_many_arguments)]
fn put(kit: &mut Kit, face: Face, size: f32, x: f32, box_y: f32, box_h: f32, max_w: f32, text: &str, color: Rgb, alpha: f32) -> f32 {
    let px = size * kit.canvas.scale;
    let cut = kit.fonts.ellipsize(face, text, px, 0.0, max_w.max(8.0) * kit.canvas.scale);
    let base = box_y + kit.fonts.baseline(face, size, box_h);
    kit.canvas.text(kit.fonts, face, size, x, base, &cut, color, alpha, 0.0)
}

fn put_centered(kit: &mut Kit, face: Face, size: f32, cx: f32, box_y: f32, box_h: f32, max_w: f32, text: &str, color: Rgb, alpha: f32) {
    let px = size * kit.canvas.scale;
    let cut = kit.fonts.ellipsize(face, text, px, 0.0, max_w.max(8.0) * kit.canvas.scale);
    let w = kit.canvas.measure(kit.fonts, face, size, &cut, 0.0);
    let base = box_y + kit.fonts.baseline(face, size, box_h);
    kit.canvas.text(kit.fonts, face, size, cx - w / 2.0, base, &cut, color, alpha, 0.0);
}

fn ease_out(t: f32) -> f32 {
    1.0 - (1.0 - t.clamp(0.0, 1.0)).powi(3)
}

impl MusicModule {
    /// The cover, or a coloured tile with a note when there is none.
    #[allow(clippy::too_many_arguments)]
    fn cover(&self, kit: &mut Kit, frame: &PanelFrame, st: &MediaState, x: f32, y: f32, size: f32) {
        let r = size * 0.19;
        kit.canvas.glow(x + size / 2.0, y + size / 2.0 + 4.0, size * 0.95, self.accent, frame.alpha * 0.28);
        match &st.art {
            Some(art) => kit.canvas.image_rrect(x, y, size, size, r, &art.pixmap, frame.alpha),
            None => {
                kit.canvas.gradient_rrect(x, y, size, size, r, Rgb(0x5b, 0x7c, 0xff), Rgb(0xb3, 0x5b, 0xff), true, frame.alpha);
                kit.canvas.icon(Icon::Music, x + size / 2.0, y + size / 2.0, size * 0.34, Rgb(255, 255, 255), frame.alpha);
            }
        }
    }

    /// The thin progress bar with the elapsed and remaining times. Records the seek area.
    #[allow(clippy::too_many_arguments)]
    fn progress(&mut self, kit: &mut Kit, frame: &PanelFrame, st: &MediaState, pos: u64, x: f32, y: f32, w: f32) {
        let theme = frame.theme;
        let time_w = 38.0;
        let (ta, tb) = (x + time_w + 6.0, x + w - time_w - 6.0);
        let shown = self.drag_ms.unwrap_or(pos);
        let size = 10.5;
        let base = y - 8.0 + kit.fonts.baseline(Face::Mono, size, 16.0);
        kit.canvas.text(kit.fonts, Face::Mono, size, x, base, &mmss(shown), theme.dim, frame.alpha, 0.0);
        let remaining = format!("-{}", mmss(st.duration_ms.saturating_sub(shown)));
        let rw = kit.canvas.measure(kit.fonts, Face::Mono, size, &remaining, 0.0);
        kit.canvas.text(kit.fonts, Face::Mono, size, x + w - rw, base, &remaining, theme.dim, frame.alpha, 0.0);
        let fraction = if st.duration_ms > 0 { (shown as f32 / st.duration_ms as f32).clamp(0.0, 1.0) } else { 0.0 };
        let over = self.over(ta - 4.0, y - 9.0, tb - ta + 8.0, 18.0) || self.drag_ms.is_some();
        kit.canvas.bar(ta, tb, y, if over { 5.0 } else { 4.0 }, theme.fg, frame.alpha * 0.2);
        let fx = ta + (tb - ta) * fraction;
        if fraction > 0.0 {
            kit.canvas.bar(ta, fx, y, if over { 5.0 } else { 4.0 }, theme.fg, frame.alpha);
        }
        if over {
            kit.canvas.circle(fx, y, 5.5, theme.fg, frame.alpha);
        }
        if st.can_seek && st.duration_ms > 0 {
            self.hits.push(Hit { x: ta - 4.0, y: y - 9.0, w: tb - ta + 8.0, h: 18.0, target: Target::Seek, track: (ta, tb) });
        }
    }

    /// Previous, play or pause, next; and the lyrics toggle at the left of them.
    #[allow(clippy::too_many_arguments)]
    fn controls(&mut self, kit: &mut Kit, frame: &PanelFrame, st: &MediaState, left: f32, right: f32, y: f32, lang: Lang) {
        let theme = frame.theme;
        let mid = (left + right) / 2.0;
        let button = |this: &mut Self, kit: &mut Kit, cx: f32, r: f32, icon: Icon, target: Target, enabled: bool, filled: bool| {
            let over = enabled && this.over(cx - r - 2.0, y - r - 2.0, 2.0 * r + 4.0, 2.0 * r + 4.0);
            if filled {
                kit.canvas.circle(cx, y, r, theme.fg, frame.alpha * if over { 1.0 } else { 0.94 });
                kit.canvas.icon(icon, cx + if icon == Icon::Play { 1.0 } else { 0.0 }, y, r * 0.95, theme.bg, frame.alpha);
            } else {
                if over {
                    kit.canvas.circle(cx, y, r + 2.0, theme.fg, frame.alpha * 0.14);
                }
                kit.canvas.icon(icon, cx, y, r * 1.1, theme.fg, frame.alpha * if enabled { 1.0 } else { 0.3 });
            }
            this.hits.push(Hit { x: cx - r - 4.0, y: y - r - 4.0, w: 2.0 * r + 8.0, h: 2.0 * r + 8.0, target, track: (0.0, 0.0) });
        };
        let _ = lang;
        button(self, kit, mid - 44.0, 9.0, Icon::SkipBack, Target::Previous, st.can_prev, false);
        button(self, kit, mid, 17.0, if st.playing { Icon::Pause } else { Icon::Play }, Target::PlayPause, true, true);
        button(self, kit, mid + 44.0, 9.0, Icon::SkipForward, Target::Next, st.can_next, false);
        // The lyrics toggle, lit while lyrics are showing.
        let lx = left + 12.0;
        let over = self.over(lx - 14.0, y - 14.0, 28.0, 28.0);
        if self.show_lyrics {
            kit.canvas.circle(lx, y, 13.0, self.accent, frame.alpha * 0.9);
            kit.canvas.icon(Icon::Message, lx, y, 14.0, Rgb(0x10, 0x0a, 0x20), frame.alpha);
        } else {
            if over {
                kit.canvas.circle(lx, y, 13.0, theme.fg, frame.alpha * 0.14);
            }
            kit.canvas.icon(Icon::Message, lx, y, 14.0, theme.dim, frame.alpha);
        }
        self.hits.push(Hit { x: lx - 14.0, y: y - 14.0, w: 28.0, h: 28.0, target: Target::ToggleLyrics, track: (0.0, 0.0) });
    }

    /// The synced lyrics: the line being sung in the middle, bright, the others fading away from it.
    #[allow(clippy::too_many_arguments)]
    fn lyrics_view(&mut self, kit: &mut Kit, env: &Env, frame: &PanelFrame, st: &MediaState, pos: u64, x: f32, y0: f32, w: f32) {
        let theme = frame.theme;
        let lang = env.t.lang();
        let cy = y0 + CONTENT_H / 2.0;
        let centered = |kit: &mut Kit, text: &str| put_centered(kit, Face::Sans, 12.5, x + w / 2.0, cy - 10.0, 20.0, w - 8.0, text, theme.dim, frame.alpha);
        if !self.lookup_allowed {
            centered(kit, word(lang, "off"));
            return;
        }
        let key = Self::query(st).key();
        match self.lyrics.get(&key) {
            None | Some(LyricsState::Loading) => centered(kit, word(lang, "loading")),
            Some(LyricsState::Ready(Lyrics::None)) => centered(kit, word(lang, "none")),
            Some(LyricsState::Ready(Lyrics::Plain(lines))) => {
                // Words without times: shown from the top, moved with the wheel.
                let lines = lines.clone();
                kit.canvas.push_clip(x - 4.0, y0, w + 8.0, CONTENT_H);
                put(kit, Face::Sans, 10.5, x, y0 + 2.0, 14.0, w, word(lang, "plain"), theme.dim, frame.alpha * 0.8);
                for (i, l) in lines.iter().enumerate() {
                    let y = y0 + 20.0 + i as f32 * 20.0 - self.plain_scroll;
                    if y > y0 + CONTENT_H || y < y0 + 8.0 {
                        continue;
                    }
                    put(kit, Face::Sans, 12.5, x, y, 20.0, w, l, theme.fg, frame.alpha * 0.9);
                }
                kit.canvas.pop_clip();
            }
            Some(LyricsState::Ready(Lyrics::Synced(lines))) => {
                let lines = lines.clone();
                let now_idx = current_line(&lines, pos).unwrap_or(0);
                // Glide to the new line instead of jumping.
                let shown_at = |from: f32, to: usize, start: u64| {
                    let t = ((env.now_ms.saturating_sub(start)) as f32 / SCROLL_MS as f32).clamp(0.0, 1.0);
                    (from + (to as f32 - from) * ease_out(t), t)
                };
                if self.scroll_start == 0 {
                    self.scroll_from = now_idx as f32;
                    self.scroll_to = now_idx;
                    self.scroll_start = env.now_ms.max(1);
                } else if now_idx != self.scroll_to {
                    let (current, _) = shown_at(self.scroll_from, self.scroll_to, self.scroll_start);
                    self.scroll_from = current;
                    self.scroll_to = now_idx;
                    self.scroll_start = env.now_ms.max(1);
                }
                let (at, t) = shown_at(self.scroll_from, self.scroll_to, self.scroll_start);
                if t < 1.0 {
                    self.animating = true;
                }
                let started = current_line(&lines, pos).is_some();
                kit.canvas.push_clip(x - 6.0, y0, w + 12.0, CONTENT_H);
                for (i, line) in lines.iter().enumerate() {
                    let d = i as f32 - at;
                    if d.abs() > 2.6 {
                        continue;
                    }
                    let near = (1.0 - d.abs()).clamp(0.0, 1.0);
                    let fade = (1.0 - (d.abs() - 1.0).max(0.0) / 1.6).clamp(0.0, 1.0);
                    let size = 13.0 + 2.0 * near;
                    let (face, color, a) = if near > 0.5 && started { (Face::SansBold, theme.fg, 1.0) } else { (Face::Sans, theme.fg, 0.42 * fade.max(0.2)) };
                    let a = if near > 0.5 && started { 1.0 } else { a * (0.55 + 0.45 * near) };
                    let text = if line.text.is_empty() { "♪" } else { &line.text };
                    // A long line shrinks a little before it is cut.
                    let mut s = size;
                    while s > 11.0 && kit.canvas.measure(kit.fonts, face, s, text, 0.0) > w {
                        s -= 0.5;
                    }
                    let ly = cy + d * LINE_H;
                    put(kit, face, s, x, ly - 11.0, 22.0, w, text, color, frame.alpha * a * fade.max(0.15));
                    self.hits.push(Hit { x, y: ly - 12.0, w, h: 24.0, target: Target::Line(line.at_ms), track: (0.0, 0.0) });
                }
                kit.canvas.pop_clip();
            }
        }
    }
}

impl Module for MusicModule {
    fn id(&self) -> &'static str {
        "music"
    }

    fn title(&self, lang: Lang) -> String {
        word(lang, "title").to_string()
    }

    fn accent(&self) -> Rgb {
        self.accent
    }

    fn title_parts(&self, lang: Lang) -> (String, String) {
        let (a, b) = if lang == Lang::Id { ("Mu", "sik") } else { ("Mu", "sic") };
        (a.to_string(), b.to_string())
    }

    fn as_any_mut(&mut self) -> &mut dyn std::any::Any {
        self
    }

    fn set_options(&mut self, options: &Value) {
        self.lookup_allowed = options.get("lyrics").and_then(Value::as_bool).unwrap_or(true);
        self.peek_on_change = options.get("peek").and_then(Value::as_bool).unwrap_or(true);
    }

    fn playing(&self) -> bool {
        let st = self.backend.state();
        st.active && st.playing
    }

    fn wants_frames(&self) -> bool {
        self.animating
    }

    fn rest_priority(&mut self, _env: &Env, _settings: &blink_core::settings::Settings) -> u8 {
        0
    }

    fn rest_style(&mut self, _env: &Env, _resting_style: &str, _due_horizon_hours: f64) -> crate::shell::RestStyle {
        crate::shell::RestStyle::Full
    }

    fn rest(&mut self, _env: &Env) -> RestView {
        RestView { label: String::new(), count: String::new(), urgency: "later" }
    }

    fn peek(&mut self, _env: &Env) -> Option<PeekView> {
        None
    }

    fn panel_height(&mut self, _env: &Env, _kit: &mut Kit) -> f32 {
        CONTENT_H
    }

    fn draw_panel(&mut self, env: &Env, kit: &mut Kit, frame: &PanelFrame) {
        let theme = frame.theme;
        let lang = env.t.lang();
        self.hits.clear();
        self.animating = self.drag_ms.is_some();
        self.last_draw_ms = env.now_ms;
        let st = self.backend.state();
        self.accent = st.art.as_ref().map_or(VIOLET, |a| a.accent);
        let x0 = frame.content_x();
        let w = frame.content_w();
        let y0 = frame.origin_y;

        // The cover's colour washes up from the bottom of the page.
        if st.active {
            kit.canvas.gradient_rrect_alpha(frame.origin_x, y0 + 10.0, frame.width, frame.viewport_h + 24.0, 0.0, (self.accent, 0.0), (self.accent, 0.38 * frame.alpha), true);
        }

        if !st.active {
            let cx = frame.origin_x + frame.width / 2.0;
            kit.canvas.circle(cx, y0 + 40.0, 22.0, theme.fg, frame.alpha * 0.08);
            kit.canvas.icon(Icon::Music, cx, y0 + 40.0, 22.0, theme.dim, frame.alpha);
            put_centered(kit, Face::SansBold, 14.0, cx, y0 + 70.0, 20.0, w, word(lang, "nothing"), theme.fg, frame.alpha);
            put_centered(kit, Face::Sans, 12.0, cx, y0 + 92.0, 18.0, w, word(lang, "hint"), theme.dim, frame.alpha);
            return;
        }
        let pos = st.position_at(env.wall_ms);

        if !self.show_lyrics {
            let art = 84.0;
            self.cover(kit, frame, &st, x0, y0 + 10.0, art);
            let rx = x0 + art + 18.0;
            let rw = w - art - 18.0;
            put(kit, Face::SansBold, 16.0, rx, y0 + 6.0, 24.0, rw, &st.title, theme.fg, frame.alpha);
            put(kit, Face::Sans, 12.5, rx, y0 + 30.0, 18.0, rw, &st.artist, theme.dim, frame.alpha);
            self.progress(kit, frame, &st, pos, rx, y0 + 66.0, rw);
            self.controls(kit, frame, &st, rx, rx + rw, y0 + 98.0, lang);
        } else {
            let col_w = 214.0;
            let art = 60.0;
            self.cover(kit, frame, &st, x0, y0 + 8.0, art);
            put(kit, Face::SansBold, 13.5, x0 + art + 12.0, y0 + 8.0, 22.0, col_w - art - 12.0, &st.title, theme.fg, frame.alpha);
            put(kit, Face::Sans, 11.5, x0 + art + 12.0, y0 + 30.0, 18.0, col_w - art - 12.0, &st.artist, theme.dim, frame.alpha);
            self.progress(kit, frame, &st, pos, x0, y0 + 82.0, col_w);
            self.controls(kit, frame, &st, x0, x0 + col_w, y0 + 106.0, lang);
            kit.canvas.rect(x0 + col_w + 10.0, y0 + 8.0, 1.0, CONTENT_H - 16.0, theme.fg, frame.alpha * 0.12);
            self.lyrics_view(kit, env, frame, &st, pos, x0 + col_w + 24.0, y0, w - col_w - 24.0);
        }
    }

    fn pointer(&mut self, kind: PointerKind, x: f32, y: f32, env: &Env) -> Vec<Effect> {
        let st = self.backend.state();
        match kind {
            PointerKind::Leave => {
                self.pointer = None;
                if self.drag_ms.is_none() {
                    return vec![Effect::Redraw];
                }
                vec![Effect::Redraw]
            }
            PointerKind::Move => {
                self.pointer = Some((x, y));
                if self.drag_ms.is_some() {
                    if let Some(hit) = self.hits.iter().find(|h| h.target == Target::Seek).cloned() {
                        self.drag_ms = Some(Self::seek_value(&hit, x, st.duration_ms));
                    }
                }
                vec![Effect::Redraw]
            }
            PointerKind::Down => {
                let Some(hit) = self.hits.iter().rev().find(|h| h.contains(x, y)).cloned() else { return Vec::new() };
                match hit.target {
                    Target::PlayPause => return self.do_action(&MusicAction::PlayPause, env),
                    Target::Next => return self.do_action(&MusicAction::Next, env),
                    Target::Previous => return self.do_action(&MusicAction::Previous, env),
                    Target::ToggleLyrics => return self.do_action(&MusicAction::ToggleLyrics, env),
                    Target::Seek => self.drag_ms = Some(Self::seek_value(&hit, x, st.duration_ms)),
                    Target::Line(at) => {
                        if st.can_seek {
                            self.command(MediaCommand::Seek(at), env);
                        }
                    }
                }
                vec![Effect::Redraw]
            }
            PointerKind::Up => {
                if let Some(ms) = self.drag_ms.take() {
                    self.command(MediaCommand::Seek(ms), env);
                }
                vec![Effect::Redraw]
            }
            PointerKind::Wheel(delta) => {
                if self.show_lyrics {
                    self.plain_scroll = (self.plain_scroll - delta).max(0.0);
                }
                vec![Effect::Redraw]
            }
        }
    }

    fn captures_pointer(&self) -> bool {
        self.drag_ms.is_some()
    }

    fn key(&mut self, key: Key, env: &Env) -> KeyResult {
        let handled = |effects: Vec<Effect>| KeyResult { handled: true, close: false, effects };
        let st = self.backend.state();
        match key {
            Key::Space | Key::Enter => handled(self.do_action(&MusicAction::PlayPause, env)),
            Key::Char('n') | Key::Char('N') => handled(self.do_action(&MusicAction::Next, env)),
            Key::Char('p') | Key::Char('P') => handled(self.do_action(&MusicAction::Previous, env)),
            Key::Char('l') | Key::Char('L') => handled(self.do_action(&MusicAction::ToggleLyrics, env)),
            // Up and down jump ten seconds.
            Key::Up | Key::Down if st.active && st.can_seek => {
                let pos = st.position_at(env.wall_ms);
                let target = if key == Key::Up { pos.saturating_sub(10_000) } else { (pos + 10_000).min(st.duration_ms.max(pos)) };
                self.command(MediaCommand::Seek(target), env);
                handled(vec![Effect::Redraw])
            }
            _ => KeyResult { handled: false, close: key == Key::Escape, effects: Vec::new() },
        }
    }

    fn keyboard_mode(&mut self, _on: bool, _env: &Env) {}

    fn tick(&mut self, env: &Env) -> Vec<Effect> {
        let mut effects = Vec::new();
        if self.drain_lyrics() {
            effects.push(Effect::Redraw);
        }
        let st = self.backend.state();
        if st.active {
            self.ensure_lyrics(&st);
            if self.seen_track != Some(st.track) {
                // A new song: say so (not for the one already playing when the notch started).
                if self.seen_track.is_some() && self.peek_on_change && st.playing {
                    effects.push(Effect::ShowPeek(PeekView {
                        code: st.artist.clone(),
                        title: st.title.clone(),
                        count: String::new(),
                        urgency: "later",
                        priority: 1,
                        ttl_ms: Some(3200),
                        key: Some("music:track".into()),
                        progress: None,
                        icon: Some(Icon::Music),
                        accent: Some(st.art.as_ref().map_or(VIOLET, |a| a.accent)),
                    }));
                }
                self.seen_track = Some(st.track);
                self.plain_scroll = 0.0;
                self.scroll_start = 0;
                effects.push(Effect::Redraw);
            }
        }
        let _ = env;
        effects
    }

    /// While the page is open and a song plays: the next whole second, and the next lyric line.
    fn next_wake(&mut self, env: &Env, panel_open: bool) -> Option<u64> {
        if !panel_open {
            return None;
        }
        let st = self.backend.state();
        if !st.active {
            return None;
        }
        let mut wake: Option<u64> = None;
        let mut consider = |ms: u64| wake = Some(wake.map_or(ms, |w| w.min(ms)));
        if self.show_lyrics && matches!(self.lyrics_for(&st), None | Some(LyricsState::Loading)) && self.lookup_allowed {
            consider(300);
        }
        if st.playing {
            let pos = st.position_at(env.wall_ms);
            consider(1000 - pos % 1000 + 1);
            if self.show_lyrics {
                if let Some(LyricsState::Ready(Lyrics::Synced(lines))) = self.lyrics_for(&st) {
                    let next = lines.iter().map(|l| l.at_ms).find(|&at| at > pos);
                    if let Some(at) = next {
                        consider(at - pos + 1);
                    }
                }
            }
        }
        wake.map(|ms| env.now_ms + ms.max(1))
    }

    fn perform(&mut self, action: Action, env: &Env) -> Vec<Effect> {
        match action {
            Action::Music(a) => self.do_action(&a, env),
            _ => Vec::new(),
        }
    }

    fn debug_now(&mut self, env: &Env) -> Value {
        let st = self.backend.state();
        serde_json::json!({
            "showLyrics": self.show_lyrics,
            "track": st.track,
            "active": st.active,
            "title": st.title,
            "artist": st.artist,
            "app": st.app,
            "hasArt": st.art.is_some(),
            "position": st.position_at(env.wall_ms),
            "playing": st.playing,
            "lyrics": match self.lyrics_for(&st) { None => "none-asked", Some(LyricsState::Loading) => "loading", Some(LyricsState::Ready(Lyrics::Synced(l))) => if l.is_empty() { "empty" } else { "synced" }, Some(LyricsState::Ready(Lyrics::Plain(_))) => "plain", Some(LyricsState::Ready(Lyrics::None)) => "not-found" },
            "scrollTo": self.scroll_to,
            "hits": self.hits.iter().map(|h| serde_json::json!({ "x": h.x, "y": h.y, "w": h.w, "h": h.h, "target": format!("{:?}", h.target) })).collect::<Vec<_>>(),
        })
    }

    fn accessibility(&mut self, env: &Env, open: bool) -> Vec<Node> {
        if !open {
            return Vec::new();
        }
        let lang = env.t.lang();
        let st = self.backend.state();
        let mut out = Vec::new();
        if !st.active {
            out.push(Node::new(300, ROOT, Role::Text, word(lang, "nothing")));
            return out;
        }
        out.push(Node::new(300, ROOT, Role::Text, format!("{}: {}, {}", word(lang, "now"), st.title, st.artist)));
        let rect_of = |hits: &[Hit], target: &Target| hits.iter().find(|h| &h.target == target).map_or((0.0, 0.0, 0.0, 0.0), |h| (h.x, h.y, h.w, h.h));
        let button = |out: &mut Vec<Node>, key: i32, name: String, target: Target, action: MusicAction| {
            let mut n = Node::new(key, ROOT, Role::Button, name);
            n.rect = rect_of(&self.hits, &target);
            n.action = Some(NodeAction::Module(Action::Music(action)));
            out.push(n);
        };
        button(&mut out, 301, word(lang, "previous").to_string(), Target::Previous, MusicAction::Previous);
        button(&mut out, 302, word(lang, if st.playing { "pause" } else { "play" }).to_string(), Target::PlayPause, MusicAction::PlayPause);
        button(&mut out, 303, word(lang, "next").to_string(), Target::Next, MusicAction::Next);
        button(&mut out, 304, format!("{}: {}", word(lang, "lyrics"), if self.show_lyrics { "on" } else { "off" }), Target::ToggleLyrics, MusicAction::ToggleLyrics);
        out
    }
}
