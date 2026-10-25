//! What the music sounds like right now, for the bars that move with it: eight levels, from the low notes to the high ones.
//!
//! Windows lets a program listen to what the default speakers are playing (a loopback capture; nothing is recorded or kept, the
//! samples go straight into a small analysis and are thrown away). It runs only while something is playing and the bars are
//! on screen, and stops the moment they are not, so a quiet notch costs nothing.

use std::sync::atomic::{AtomicBool, AtomicU8, Ordering};
use std::sync::Arc;
use std::time::Duration;

pub const BANDS: usize = 8;
const FFT_SIZE: usize = 1024;

/// The levels the analysis found, shared with whoever draws them.
pub struct Spectrum {
    levels: [AtomicU8; BANDS],
    wanted: AtomicBool,
    /// The capture is running and delivering (false when Windows would not give it, and the bars fall back to a plain motion).
    working: AtomicBool,
    /// Why the capture could not be had, when it could not (for diagnostics).
    why: std::sync::Mutex<String>,
    /// For diagnostics: how many packets came, how many were flagged silent, and the loudest sample seen (in thousandths).
    packets: std::sync::atomic::AtomicU32,
    silent: std::sync::atomic::AtomicU32,
    loudest: std::sync::atomic::AtomicU32,
}

impl Spectrum {
    pub fn new() -> Arc<Spectrum> {
        Arc::new(Spectrum { levels: Default::default(), wanted: AtomicBool::new(false), working: AtomicBool::new(false), why: std::sync::Mutex::new(String::new()), packets: Default::default(), silent: Default::default(), loudest: Default::default() })
    }

    /// The levels, 0.0 to 1.0.
    pub fn levels(&self) -> [f32; BANDS] {
        let mut out = [0.0; BANDS];
        for (o, l) in out.iter_mut().zip(&self.levels) {
            *o = f32::from(l.load(Ordering::Relaxed)) / 255.0;
        }
        out
    }

    /// Ask for the capture to run (or stop).
    pub fn want(&self, on: bool) {
        self.wanted.store(on, Ordering::Relaxed);
    }

    /// Whether the capture is being asked for.
    pub fn wanted(&self) -> bool {
        self.wanted.load(Ordering::Relaxed)
    }

    /// (packets, silent packets, loudest sample in thousandths) since the capture started.
    pub fn heard(&self) -> (u32, u32, u32) {
        (self.packets.load(Ordering::Relaxed), self.silent.load(Ordering::Relaxed), self.loudest.load(Ordering::Relaxed))
    }

    pub fn why(&self) -> String {
        self.why.lock().map(|w| w.clone()).unwrap_or_default()
    }

    pub fn working(&self) -> bool {
        self.working.load(Ordering::Relaxed)
    }

    /// For tests and scripted runs: set the levels by hand and say the capture works.
    pub fn set_for_test(&self, levels: [f32; BANDS]) {
        for (l, v) in self.levels.iter().zip(levels) {
            l.store((v.clamp(0.0, 1.0) * 255.0) as u8, Ordering::Relaxed);
        }
        self.working.store(true, Ordering::Relaxed);
    }

    fn publish(&self, levels: &[f32; BANDS]) {
        for (l, v) in self.levels.iter().zip(levels) {
            l.store((v.clamp(0.0, 1.0) * 255.0) as u8, Ordering::Relaxed);
        }
    }
}

// ------------------------------------------------------------------------ the analysis (no Windows in it)

/// An in-place FFT of `re` and `im` (their length a power of two).
pub fn fft(re: &mut [f32], im: &mut [f32]) {
    let n = re.len();
    let mut j = 0;
    for i in 1..n {
        let mut bit = n >> 1;
        while j & bit != 0 {
            j ^= bit;
            bit >>= 1;
        }
        j ^= bit;
        if i < j {
            re.swap(i, j);
            im.swap(i, j);
        }
    }
    let mut len = 2;
    while len <= n {
        let angle = -2.0 * std::f32::consts::PI / len as f32;
        let (wr, wi) = (angle.cos(), angle.sin());
        for start in (0..n).step_by(len) {
            let (mut cr, mut ci) = (1.0_f32, 0.0_f32);
            for k in 0..len / 2 {
                let (a, b) = (start + k, start + k + len / 2);
                let (tr, ti) = (re[b] * cr - im[b] * ci, re[b] * ci + im[b] * cr);
                re[b] = re[a] - tr;
                im[b] = im[a] - ti;
                re[a] += tr;
                im[a] += ti;
                let next = cr * wr - ci * wi;
                ci = cr * wi + ci * wr;
                cr = next;
            }
        }
        len <<= 1;
    }
}

/// The edges of the bands in Hz: log-spaced from the bass to the treble.
fn band_edges() -> [f32; BANDS + 1] {
    let (lo, hi) = (50.0_f32, 12_000.0_f32);
    let mut edges = [0.0; BANDS + 1];
    for (i, e) in edges.iter_mut().enumerate() {
        *e = lo * (hi / lo).powf(i as f32 / BANDS as f32);
    }
    edges
}

/// Levels (0 to 1) for a block of `FFT_SIZE` mono samples at `rate` Hz.
pub fn analyse(samples: &[f32], rate: f32) -> [f32; BANDS] {
    let mut re = vec![0.0_f32; FFT_SIZE];
    let mut im = vec![0.0_f32; FFT_SIZE];
    for (i, s) in samples.iter().take(FFT_SIZE).enumerate() {
        // A Hann window keeps one note from smearing over the whole range.
        let w = 0.5 - 0.5 * (2.0 * std::f32::consts::PI * i as f32 / (FFT_SIZE as f32 - 1.0)).cos();
        re[i] = s * w;
    }
    fft(&mut re, &mut im);
    let edges = band_edges();
    let bin_hz = rate / FFT_SIZE as f32;
    let mut out = [0.0; BANDS];
    for (b, level) in out.iter_mut().enumerate() {
        let (from, to) = ((edges[b] / bin_hz).floor() as usize, ((edges[b + 1] / bin_hz).ceil() as usize).max((edges[b] / bin_hz) as usize + 1));
        let to = to.min(FFT_SIZE / 2);
        let mut peak = 0.0_f32;
        for k in from.max(1)..to {
            peak = peak.max((re[k] * re[k] + im[k] * im[k]).sqrt());
        }
        // Normalised to the window and the block, then in decibels: -60 dB is nothing, -6 dB is full.
        let amplitude = peak / (FFT_SIZE as f32 / 4.0);
        let db = 20.0 * amplitude.max(1e-6).log10();
        *level = ((db + 60.0) / 54.0).clamp(0.0, 1.0);
    }
    out
}

/// One step of smoothing: rise quickly, fall slowly, so the bars move like bars and not like noise.
pub fn smooth(previous: &[f32; BANDS], now: &[f32; BANDS]) -> [f32; BANDS] {
    let mut out = [0.0; BANDS];
    for i in 0..BANDS {
        out[i] = if now[i] > previous[i] { previous[i] + (now[i] - previous[i]) * 0.7 } else { previous[i] * 0.86 + now[i] * 0.14 };
    }
    out
}

// ------------------------------------------------------------------------ the capture

/// Start the worker. It waits until the levels are wanted, listens while they are, and lets go of the device when they are not.
pub fn start() -> Arc<Spectrum> {
    let spectrum = Spectrum::new();
    let shared = spectrum.clone();
    let _ = std::thread::Builder::new().name("blink-spectrum".into()).spawn(move || worker(&shared));
    spectrum
}

fn worker(spectrum: &Spectrum) {
    use windows::Win32::System::Com::{CoInitializeEx, COINIT_MULTITHREADED};
    unsafe {
        let _ = CoInitializeEx(None, COINIT_MULTITHREADED);
    }
    loop {
        if !spectrum.wanted.load(Ordering::Relaxed) {
            spectrum.working.store(false, Ordering::Relaxed);
            spectrum.publish(&[0.0; BANDS]);
            std::thread::sleep(Duration::from_millis(250));
            continue;
        }
        // Listen until the levels are not wanted any more or the device goes away; then look again.
        if capture(spectrum).is_none() {
            spectrum.working.store(false, Ordering::Relaxed);
            std::thread::sleep(Duration::from_millis(1500));
        }
    }
}

/// Say why the capture could not be had (shown by `state`, for diagnostics).
fn trace(spectrum: &Spectrum, step: &str, error: &windows::core::Error) {
    if let Ok(mut why) = spectrum.why.lock() {
        *why = format!("{step}: {error}");
    }
}

/// One speaker (or virtual output) being listened to.
struct Stream {
    client: windows::Win32::Media::Audio::IAudioClient,
    capture: windows::Win32::Media::Audio::IAudioCaptureClient,
    rate: f32,
    channels: usize,
    bits: u16,
    mono: Vec<f32>,
}

/// Every output that is switched on, each listened to: music can go to an output that is not the default one (a router such as
/// Sonar sends each app to its own virtual output), and the bars should still move.
fn open_streams(spectrum: &Spectrum) -> Vec<Stream> {
    use windows::Win32::Media::Audio::{
        eRender, IAudioCaptureClient, IAudioClient, IMMDeviceEnumerator, MMDeviceEnumerator, AUDCLNT_SHAREMODE_SHARED, AUDCLNT_STREAMFLAGS_LOOPBACK, DEVICE_STATE_ACTIVE,
    };
    use windows::Win32::System::Com::{CoCreateInstance, CoTaskMemFree, CLSCTX_ALL};
    let mut streams = Vec::new();
    unsafe {
        let Ok(enumerator) = CoCreateInstance::<_, IMMDeviceEnumerator>(&MMDeviceEnumerator, None, CLSCTX_ALL).map_err(|e| trace(spectrum, "enumerator", &e)) else { return streams };
        let Ok(devices) = enumerator.EnumAudioEndpoints(eRender, DEVICE_STATE_ACTIVE).map_err(|e| trace(spectrum, "endpoints", &e)) else { return streams };
        let count = devices.GetCount().unwrap_or(0);
        for i in 0..count.min(8) {
            let Ok(device) = devices.Item(i) else { continue };
            let Ok(client) = device.Activate::<IAudioClient>(CLSCTX_ALL, None) else { continue };
            let Ok(format) = client.GetMixFormat() else { continue };
            let (rate, channels, bits) = ((*format).nSamplesPerSec as f32, usize::from((*format).nChannels), (*format).wBitsPerSample);
            let init = client.Initialize(AUDCLNT_SHAREMODE_SHARED, AUDCLNT_STREAMFLAGS_LOOPBACK, 1_000_000, 0, format, None);
            CoTaskMemFree(Some(format as *const std::ffi::c_void));
            if init.map_err(|e| trace(spectrum, "initialize", &e)).is_err() {
                continue;
            }
            let Ok(capture) = client.GetService::<IAudioCaptureClient>() else { continue };
            if client.Start().is_err() {
                continue;
            }
            streams.push(Stream { client, capture, rate, channels, bits, mono: Vec::with_capacity(FFT_SIZE * 2) });
        }
    }
    streams
}

impl Stream {
    /// Take what has arrived. False when the output went away.
    fn read(&mut self, spectrum: &Spectrum) -> bool {
        unsafe {
            loop {
                let Ok(size) = self.capture.GetNextPacketSize() else { return false };
                if size == 0 {
                    return true;
                }
                let (mut data, mut frames, mut flags) = (std::ptr::null_mut::<u8>(), 0u32, 0u32);
                if self.capture.GetBuffer(&mut data, &mut frames, &mut flags, None, None).is_err() {
                    return false;
                }
                spectrum.packets.fetch_add(1, Ordering::Relaxed);
                let silent = flags & 0x2 != 0; // AUDCLNT_BUFFERFLAGS_SILENT
                if silent {
                    spectrum.silent.fetch_add(1, Ordering::Relaxed);
                }
                for f in 0..frames as usize {
                    let mut sum = 0.0_f32;
                    if !silent && !data.is_null() && self.channels > 0 {
                        for c in 0..self.channels {
                            sum += match self.bits {
                                32 => *(data as *const f32).add(f * self.channels + c),
                                16 => f32::from(*(data as *const i16).add(f * self.channels + c)) / 32768.0,
                                _ => 0.0,
                            };
                        }
                    }
                    let m = sum / self.channels.max(1) as f32;
                    spectrum.loudest.fetch_max((m.abs() * 1000.0) as u32, Ordering::Relaxed);
                    self.mono.push(m);
                }
                let _ = self.capture.ReleaseBuffer(frames);
                // Keep only the freshest block.
                if self.mono.len() > FFT_SIZE * 2 {
                    let drop = self.mono.len() - FFT_SIZE;
                    self.mono.drain(..drop);
                }
            }
        }
    }
}

/// Listen to the outputs while the levels are wanted (looking for outputs again every so often). `None` when none could be had.
fn capture(spectrum: &Spectrum) -> Option<()> {
    let mut streams = open_streams(spectrum);
    if streams.is_empty() {
        return None;
    }
    spectrum.working.store(true, Ordering::Relaxed);
    let started = std::time::Instant::now();
    let mut levels = [0.0_f32; BANDS];
    while spectrum.wanted.load(Ordering::Relaxed) && started.elapsed() < Duration::from_secs(20) {
        std::thread::sleep(Duration::from_millis(16));
        let mut now = [0.0_f32; BANDS];
        let mut alive = Vec::with_capacity(streams.len());
        for mut stream in streams {
            if !stream.read(spectrum) {
                let _ = unsafe { stream.client.Stop() };
                continue;
            }
            if stream.mono.len() >= FFT_SIZE {
                let found = analyse(&stream.mono[stream.mono.len() - FFT_SIZE..], stream.rate);
                for (n, f) in now.iter_mut().zip(found) {
                    *n = n.max(f);
                }
            }
            alive.push(stream);
        }
        streams = alive;
        if streams.is_empty() {
            break;
        }
        levels = smooth(&levels, &now);
        spectrum.publish(&levels);
    }
    for stream in &streams {
        let _ = unsafe { stream.client.Stop() };
    }
    Some(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tone(hz: f32, rate: f32, amplitude: f32) -> Vec<f32> {
        (0..FFT_SIZE).map(|i| amplitude * (2.0 * std::f32::consts::PI * hz * i as f32 / rate).sin()).collect()
    }

    #[test]
    fn the_fft_finds_the_frequency_of_a_tone() {
        let rate = 48_000.0;
        let samples = tone(3000.0, rate, 0.5);
        let mut re = samples.clone();
        let mut im = vec![0.0; FFT_SIZE];
        fft(&mut re, &mut im);
        let peak = (1..FFT_SIZE / 2).max_by(|a, b| (re[*a].hypot(im[*a])).total_cmp(&re[*b].hypot(im[*b]))).unwrap();
        let found = peak as f32 * rate / FFT_SIZE as f32;
        assert!((found - 3000.0).abs() < rate / FFT_SIZE as f32, "{found}");
    }

    #[test]
    fn a_bass_note_lights_the_low_bands_and_a_high_one_the_high_bands() {
        let rate = 48_000.0;
        let low = analyse(&tone(110.0, rate, 0.6), rate);
        let high = analyse(&tone(8_000.0, rate, 0.6), rate);
        let loudest = |l: &[f32; BANDS]| (0..BANDS).max_by(|a, b| l[*a].total_cmp(&l[*b])).unwrap();
        assert!(loudest(&low) <= 1, "{low:?}");
        assert!(loudest(&high) >= BANDS - 2, "{high:?}");
        assert!(low[loudest(&low)] > 0.6 && high[loudest(&high)] > 0.6);
    }

    #[test]
    fn silence_is_flat_and_the_bars_rise_fast_and_fall_slowly() {
        let silent = analyse(&vec![0.0; FFT_SIZE], 48_000.0);
        assert!(silent.iter().all(|l| *l == 0.0));
        let mut levels = [0.0; BANDS];
        levels = smooth(&levels, &[1.0; BANDS]);
        assert!(levels[0] > 0.6, "rises fast: {}", levels[0]);
        let high = levels[0];
        levels = smooth(&levels, &[0.0; BANDS]);
        assert!(levels[0] < high && levels[0] > high * 0.7, "falls slowly: {}", levels[0]);
    }

    #[test]
    fn the_levels_are_shared_as_whole_numbers() {
        let s = Spectrum::new();
        assert!(!s.working());
        s.set_for_test([1.0, 0.5, 0.0, 0.0, 0.0, 0.0, 0.0, 0.25]);
        let l = s.levels();
        assert!((l[0] - 1.0).abs() < 0.01 && (l[1] - 0.5).abs() < 0.01 && (l[7] - 0.25).abs() < 0.01 && s.working());
    }
}
