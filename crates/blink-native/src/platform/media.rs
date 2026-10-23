//! What is playing, from Windows itself: the same "now playing" information the volume flyout and
//! the lock screen show, for Spotify, a browser tab, or any player that reports to Windows.
//!
//! One worker thread does everything, so a slow cover or a stuck player never freezes the notch.
//! Windows tells the worker when something changed (no polling); it reads the song, the position
//! and the cover into a shared state and wakes the window.

use std::sync::mpsc::{channel, Receiver, Sender};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use tiny_skia::{IntSize, Pixmap};
use windows::Foundation::TypedEventHandler;
use windows::Graphics::Imaging::{BitmapAlphaMode, BitmapBounds, BitmapDecoder, BitmapInterpolationMode, BitmapPixelFormat, BitmapTransform, ColorManagementMode, ExifOrientationMode};
use windows::Media::Control::{GlobalSystemMediaTransportControlsSession as Session, GlobalSystemMediaTransportControlsSessionManager as Manager, GlobalSystemMediaTransportControlsSessionPlaybackStatus as Status};
use windows::Win32::System::WinRT::{RoInitialize, RO_INIT_MULTITHREADED};

use crate::modules::music::{Art, MediaBackend, MediaCommand, MediaState};
use crate::ui::theme::Rgb;

/// The cover is kept this many pixels square: plenty for the largest place it is drawn.
const ART_PX: u32 = 144;
/// 100-nanosecond ticks between 1601 (Windows' epoch) and 1970 (Unix'), in milliseconds.
const EPOCH_GAP_MS: i64 = 11_644_473_600_000;

enum Msg {
    Refresh,
    Command(MediaCommand),
}

pub struct WinMedia {
    shared: Arc<Mutex<MediaState>>,
    tx: Sender<Msg>,
}

impl WinMedia {
    pub fn start(wake: Arc<dyn Fn() + Send + Sync>) -> Arc<WinMedia> {
        let shared = Arc::new(Mutex::new(MediaState::default()));
        let (tx, rx) = channel();
        let worker_shared = shared.clone();
        let worker_tx = tx.clone();
        let _ = std::thread::Builder::new().name("blink-media".into()).spawn(move || worker(rx, worker_tx, worker_shared, wake));
        Arc::new(WinMedia { shared, tx })
    }
}

impl MediaBackend for WinMedia {
    fn state(&self) -> MediaState {
        self.shared.lock().unwrap_or_else(|p| p.into_inner()).clone()
    }

    fn send(&self, command: MediaCommand, _wall_ms: i64) {
        let _ = self.tx.send(Msg::Command(command));
    }
}

/// What the worker keeps between messages.
struct Hooks {
    session_id: String,
    session: Session,
    tokens: [i64; 3],
}

fn worker(rx: Receiver<Msg>, tx: Sender<Msg>, shared: Arc<Mutex<MediaState>>, wake: Arc<dyn Fn() + Send + Sync>) {
    unsafe {
        let _ = RoInitialize(RO_INIT_MULTITHREADED);
    }
    let Some(manager) = Manager::RequestAsync().ok().and_then(|op| op.join().ok()) else { return };
    {
        // The player in front changed (or went away).
        let tx = tx.clone();
        let _ = manager.CurrentSessionChanged(&TypedEventHandler::new(move |_, _| {
            let _ = tx.send(Msg::Refresh);
            Ok(())
        }));
    }
    let mut hooks: Option<Hooks> = None;
    let mut track = 0u64;
    let mut last_key = String::new();
    let mut art: Option<Arc<Art>> = None;
    let _ = tx.send(Msg::Refresh);

    while let Ok(first) = rx.recv() {
        let mut messages = vec![first];
        // A burst of changes needs one read.
        while let Ok(next) = rx.try_recv() {
            messages.push(next);
        }
        let session = manager.GetCurrentSession().ok();
        for m in &messages {
            if let (Msg::Command(c), Some(s)) = (m, &session) {
                run(c, s);
            }
        }
        // Follow whichever session is current: hook its changes once.
        match &session {
            Some(s) => {
                let id = s.SourceAppUserModelId().map(|h| h.to_string()).unwrap_or_default();
                if hooks.as_ref().is_none_or(|h| h.session_id != id) {
                    if let Some(old) = hooks.take() {
                        let _ = old.session.RemoveMediaPropertiesChanged(old.tokens[0]);
                        let _ = old.session.RemovePlaybackInfoChanged(old.tokens[1]);
                        let _ = old.session.RemoveTimelinePropertiesChanged(old.tokens[2]);
                    }
                    // Each kind of change has its own handler type; all of them just ask for a fresh read.
                    let (a, b, c) = (tx.clone(), tx.clone(), tx.clone());
                    let tokens = [
                        s.MediaPropertiesChanged(&TypedEventHandler::new(move |_, _| {
                            let _ = a.send(Msg::Refresh);
                            Ok(())
                        }))
                        .unwrap_or(0),
                        s.PlaybackInfoChanged(&TypedEventHandler::new(move |_, _| {
                            let _ = b.send(Msg::Refresh);
                            Ok(())
                        }))
                        .unwrap_or(0),
                        s.TimelinePropertiesChanged(&TypedEventHandler::new(move |_, _| {
                            let _ = c.send(Msg::Refresh);
                            Ok(())
                        }))
                        .unwrap_or(0),
                    ];
                    hooks = Some(Hooks { session_id: id, session: s.clone(), tokens });
                }
            }
            None => {
                hooks = None;
            }
        }

        let next = match &session {
            Some(s) => read(s, &mut track, &mut last_key, &mut art),
            None => MediaState { track, ..MediaState::default() },
        };
        *shared.lock().unwrap_or_else(|p| p.into_inner()) = next;
        wake();
        // Commands change what the player reports a moment later; ask again once it has.
        if messages.iter().any(|m| matches!(m, Msg::Command(_))) {
            let tx = tx.clone();
            std::thread::spawn(move || {
                std::thread::sleep(Duration::from_millis(350));
                let _ = tx.send(Msg::Refresh);
            });
        }
    }
}

fn run(command: &MediaCommand, session: &Session) {
    match command {
        MediaCommand::PlayPause => {
            let _ = session.TryTogglePlayPauseAsync().and_then(|a| a.join());
        }
        MediaCommand::Next => {
            let _ = session.TrySkipNextAsync().and_then(|a| a.join());
        }
        MediaCommand::Previous => {
            let _ = session.TrySkipPreviousAsync().and_then(|a| a.join());
        }
        MediaCommand::Seek(ms) => {
            let _ = session.TryChangePlaybackPositionAsync(*ms as i64 * 10_000).and_then(|a| a.join());
        }
    }
}

/// Everything about the current song, read fresh.
fn read(session: &Session, track: &mut u64, last_key: &mut String, art: &mut Option<Arc<Art>>) -> MediaState {
    let mut st = MediaState { active: true, rate: 1.0, ..MediaState::default() };
    st.app = session.SourceAppUserModelId().map(|h| app_name(&h.to_string())).unwrap_or_default();
    let props = session.TryGetMediaPropertiesAsync().ok().and_then(|op| op.join().ok());
    if let Some(p) = &props {
        st.title = p.Title().map(|h| h.to_string()).unwrap_or_default();
        st.artist = p.Artist().map(|h| h.to_string()).unwrap_or_default();
        st.album = p.AlbumTitle().map(|h| h.to_string()).unwrap_or_default();
    }
    let key = format!("{}\u{1}{}\u{1}{}", st.title, st.artist, st.album);
    if key != *last_key {
        *last_key = key;
        *track += 1;
        *art = None;
    }
    st.track = *track;
    // The cover can arrive a moment after the words: keep asking until there is one.
    if art.is_none() {
        if let Some(p) = &props {
            *art = trace("get the thumbnail", p.Thumbnail()).and_then(|t| read_art(&t));
        }
    }
    st.art = art.clone();

    if let Ok(info) = session.GetPlaybackInfo() {
        st.playing = info.PlaybackStatus().is_ok_and(|s| s == Status::Playing);
        if let Ok(controls) = info.Controls() {
            st.can_next = controls.IsNextEnabled().unwrap_or(false);
            st.can_prev = controls.IsPreviousEnabled().unwrap_or(false);
            st.can_seek = controls.IsPlaybackPositionEnabled().unwrap_or(false);
        }
        if let Some(rate) = info.PlaybackRate().ok().and_then(|r| r.Value().ok()) {
            if rate > 0.0 {
                st.rate = rate as f32;
            }
        }
    }
    if let Ok(t) = session.GetTimelineProperties() {
        let ms = |d: windows::Foundation::TimeSpan| (d.Duration / 10_000).max(0) as u64;
        let start = t.StartTime().map(ms).unwrap_or(0);
        st.position_ms = t.Position().map(ms).unwrap_or(0).saturating_sub(start);
        st.duration_ms = t.EndTime().map(ms).unwrap_or(0).saturating_sub(start);
        st.sampled_wall_ms = t.LastUpdatedTime().map(|d| d.UniversalTime / 10_000 - EPOCH_GAP_MS).unwrap_or(0);
        if st.sampled_wall_ms <= 0 {
            // Some players never say when: take now.
            st.sampled_wall_ms = jiff::Timestamp::now().as_millisecond();
        }
    }
    st
}

/// "Spotify.exe" -> "Spotify"; "Chrome" stays; a package id keeps its first word.
fn app_name(id: &str) -> String {
    let base = id.rsplit(['\\', '!']).next().unwrap_or(id);
    let base = base.strip_suffix(".exe").unwrap_or(base);
    base.split('_').next().unwrap_or(base).to_string()
}

/// Decode the cover to a small, square, premultiplied picture, and find the colour that stands for it.
fn read_art(reference: &windows::Storage::Streams::IRandomAccessStreamReference) -> Option<Arc<Art>> {
    let stream = trace("open the cover", reference.OpenReadAsync().and_then(|a| a.join()))?;
    let decoder = trace("decode the cover", BitmapDecoder::CreateAsync(&stream).and_then(|a| a.join()))?;
    let (w, h) = (decoder.PixelWidth().ok()?, decoder.PixelHeight().ok()?);
    if w == 0 || h == 0 {
        return None;
    }
    // Scale so the short side is ART_PX, then keep the middle square of that (a video thumbnail is wide).
    // The crop is in the scaled picture's own pixels.
    let (scaled_w, scaled_h) = if w >= h { ((w as u64 * ART_PX as u64 / h as u64).max(ART_PX as u64) as u32, ART_PX) } else { (ART_PX, (h as u64 * ART_PX as u64 / w as u64).max(ART_PX as u64) as u32) };
    let transform = BitmapTransform::new().ok()?;
    transform.SetScaledWidth(scaled_w).ok()?;
    transform.SetScaledHeight(scaled_h).ok()?;
    transform.SetBounds(BitmapBounds { X: (scaled_w - ART_PX) / 2, Y: (scaled_h - ART_PX) / 2, Width: ART_PX, Height: ART_PX }).ok()?;
    transform.SetInterpolationMode(BitmapInterpolationMode::Fant).ok()?;
    let provider = trace(
        "read the pixels",
        decoder
            .GetPixelDataTransformedAsync(BitmapPixelFormat::Bgra8, BitmapAlphaMode::Premultiplied, &transform, ExifOrientationMode::IgnoreExifOrientation, ColorManagementMode::DoNotColorManage)
            .and_then(|a| a.join()),
    )?;
    let bgra = provider.DetachPixelData().ok()?;
    if bgra.len() < (ART_PX * ART_PX * 4) as usize {
        return None;
    }
    // BGRA to RGBA (tiny-skia's order); the alpha is already multiplied in.
    let mut rgba = Vec::with_capacity(bgra.len());
    for px in bgra.chunks_exact(4) {
        rgba.extend_from_slice(&[px[2], px[1], px[0], px[3]]);
    }
    let accent = accent_of(&rgba);
    let pixmap = Pixmap::from_vec(rgba, IntSize::from_wh(ART_PX, ART_PX)?)?;
    Some(Arc::new(Art { pixmap, accent }))
}

/// Keep the value, or say (when `BLINK_MEDIA_DEBUG` is set) which step of reading the cover failed.
fn trace<T>(step: &str, result: windows::core::Result<T>) -> Option<T> {
    match result {
        Ok(v) => Some(v),
        Err(e) => {
            if std::env::var_os("BLINK_MEDIA_DEBUG").is_some() {
                use std::io::Write;
                if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(std::env::temp_dir().join("blink-media-debug.txt")) {
                    let _ = writeln!(f, "{step}: {e}");
                }
            }
            None
        }
    }
}

/// A colour for the page from the cover: the most vivid hue in it (not the muddy average of all of it),
/// kept bright enough to read on black.
pub fn accent_of(rgba: &[u8]) -> Rgb {
    const BINS: usize = 18;
    let mut weight = [0.0f64; BINS];
    let mut sum = [[0.0f64; 3]; BINS];
    for px in rgba.chunks_exact(4).step_by(3) {
        let (r, g, b) = (f64::from(px[0]), f64::from(px[1]), f64::from(px[2]));
        let (max, min) = (r.max(g).max(b), r.min(g).min(b));
        let chroma = max - min;
        if chroma < 24.0 || max < 60.0 {
            continue; // grey, white and near-black say nothing about the colour
        }
        let hue = if max == r {
            ((g - b) / chroma).rem_euclid(6.0)
        } else if max == g {
            (b - r) / chroma + 2.0
        } else {
            (r - g) / chroma + 4.0
        };
        let bin = ((hue / 6.0 * BINS as f64) as usize).min(BINS - 1);
        let w = (chroma / 255.0) * (max / 255.0);
        weight[bin] += w;
        sum[bin][0] += r * w;
        sum[bin][1] += g * w;
        sum[bin][2] += b * w;
    }
    let Some((best, _)) = weight.iter().enumerate().max_by(|a, b| a.1.partial_cmp(b.1).unwrap_or(std::cmp::Ordering::Equal)).filter(|(_, w)| **w > 0.0) else {
        return Rgb(0xa7, 0x8b, 0xfa);
    };
    let (mut r, mut g, mut b) = (sum[best][0] / weight[best], sum[best][1] / weight[best], sum[best][2] / weight[best]);
    let brightest = r.max(g).max(b);
    if brightest < 170.0 && brightest > 0.0 {
        let k = 170.0 / brightest;
        r = (r * k).min(255.0);
        g = (g * k).min(255.0);
        b = (b * k).min(255.0);
    }
    Rgb(r as u8, g as u8, b as u8)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn picture(parts: &[([u8; 3], usize)]) -> Vec<u8> {
        parts.iter().flat_map(|(c, n)| std::iter::repeat_n([c[0], c[1], c[2], 255], *n)).flatten().collect()
    }

    #[test]
    fn the_accent_is_the_dominant_vivid_colour_not_the_average() {
        // A lot of red, a little blue, and grey and white around: red wins, and it stays red.
        let rgba = picture(&[([200, 30, 30], 600), ([30, 30, 220], 150), ([128, 128, 128], 800), ([250, 250, 250], 800)]);
        let Rgb(r, g, b) = accent_of(&rgba);
        assert!(r > 180 && g < 90 && b < 90, "{r} {g} {b}");
    }

    #[test]
    fn a_dark_cover_gets_a_colour_light_enough_to_read() {
        let Rgb(r, g, b) = accent_of(&picture(&[([90, 20, 20], 1000)]));
        assert!(r >= 160 && g < 80, "{r} {g} {b}");
    }

    #[test]
    fn a_grey_cover_gets_the_default_violet() {
        assert_eq!(accent_of(&picture(&[([120, 120, 120], 1000)])), Rgb(0xa7, 0x8b, 0xfa));
    }
}
