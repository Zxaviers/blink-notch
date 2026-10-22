//! Native notch spike: no web engine, no GPU driver.
//!
//! A layered, per-pixel-alpha window at top center. tiny-skia draws the pill
//! into a pixmap on the CPU (text too: glyph outlines from ttf-parser, rasterized lazily and cached), and UpdateLayeredWindow
//! hands the result to Windows. Fully transparent pixels let the mouse through,
//! so there is no cursor polling; hover comes from WM_MOUSEMOVE / WM_MOUSELEAVE
//! and a resting notch blocks in GetMessage (zero CPU) until something happens.
//!
//! Matches the WebView2 app where it matters for a fair comparison: same
//! 640x360 logical window, same 232x32 pill, same 420-wide open panel, same
//! 150 ms hover intent / 300 ms grace / 280 ms ease-out, a progress bar.
//!
//! Flags:  --open        start expanded and stay expanded
//!         --stress      animate the progress bar every frame while expanded
//!         --seconds N   exit after N seconds
//!         --dump P      write P-rest.png and P-open.png and exit
//! Env:    DN_DEBUG=1    log milestones and frame pacing to stderr

#![windows_subsystem = "windows"]

use std::cell::RefCell;
use std::collections::HashMap;
use std::ffi::c_void;
use std::time::{Duration, Instant};

use tiny_skia::{
    FillRule, LineCap, Paint, PathBuilder, Pixmap, Stroke, Transform,
};
use windows::core::{w, PCWSTR};
use windows::Win32::Foundation::{COLORREF, HWND, LPARAM, LRESULT, POINT, SIZE, WPARAM};
use windows::Win32::Graphics::Dwm::DwmFlush;
use windows::Win32::Graphics::Gdi::{
    CreateCompatibleDC, CreateDIBSection, GetDC, SelectObject, AC_SRC_ALPHA, AC_SRC_OVER,
    BITMAPINFO, BITMAPINFOHEADER, BI_RGB, BLENDFUNCTION, DIB_RGB_COLORS, HDC, HGDIOBJ,
};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::Input::KeyboardAndMouse::{
    TrackMouseEvent, TME_LEAVE, TRACKMOUSEEVENT,
};
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, DispatchMessageW, GetMessageW, GetSystemMetrics, KillTimer,
    LoadCursorW, PeekMessageW, PostQuitMessage, RegisterClassExW, SetProcessDPIAware, SetTimer,
    ShowWindow, TranslateMessage, UpdateLayeredWindow, IDC_ARROW, MA_NOACTIVATE, MSG, PM_REMOVE,
    SM_CXSCREEN, SW_SHOWNOACTIVATE, ULW_ALPHA, WM_DESTROY, WM_MOUSEACTIVATE,
    WM_MOUSEMOVE, WM_QUIT, WM_TIMER, WNDCLASSEXW, WS_EX_LAYERED, WS_EX_NOACTIVATE,
    WS_EX_TOOLWINDOW, WS_EX_TOPMOST, WS_POPUP,
};

// Logical sizes, as in the WebView2 app.
const WIN_W: f32 = 640.0;
const WIN_H: f32 = 360.0;
const REST: Shape = Shape { w: 232.0, h: 32.0, r: 16.0 };
const OPEN: Shape = Shape { w: 420.0, h: 340.0, r: 24.0 };

/// Not exported by the windows crate under WindowsAndMessaging.
const WM_MOUSELEAVE: u32 = 0x02A3;

const INTENT_MS: u32 = 150;
const GRACE_MS: u32 = 300;
const ANIM: Duration = Duration::from_millis(280);

const T_INTENT: usize = 1;
const T_GRACE: usize = 2;
const T_SECOND: usize = 3;
const T_EXIT: usize = 4;

#[derive(Clone, Copy)]
struct Shape {
    w: f32,
    h: f32,
    r: f32,
}

fn lerp(a: f32, b: f32, t: f32) -> f32 {
    a + (b - a) * t
}

/// Ease-out cubic, close to the CSS cubic-bezier(0.2, 0.8, 0.2, 1) the web app uses.
fn ease(t: f32) -> f32 {
    1.0 - (1.0 - t).powi(3)
}

struct Anim {
    from: Shape,
    to: Shape,
    start: Instant,
    stamps: Vec<Instant>,
}

/// A glyph rasterized once, on first use, into a small coverage bitmap.
struct Glyph {
    left: i32,
    top: i32, // pixels above the baseline
    width: usize,
    height: usize,
    advance: f32,
    coverage: Vec<u8>,
}

/// Font files are read once. A glyph is turned into a bitmap only when first
/// drawn (outlines come from ttf-parser, zero-copy) and then reused, so memory
/// stays proportional to the glyphs actually shown, not to the whole font.
struct Fonts {
    sans: Vec<u8>,
    mono: Vec<u8>,
    cache: HashMap<(bool, char, u32), Glyph>,
}

impl Fonts {
    fn load() -> Self {
        let read = |name: &str| std::fs::read(format!("C:\\Windows\\Fonts\\{name}")).expect("system font");
        Fonts { sans: read("segoeui.ttf"), mono: read("consola.ttf"), cache: HashMap::new() }
    }

    fn glyph(&mut self, mono: bool, ch: char, px: f32) -> Option<&Glyph> {
        let key = (mono, ch, px.round() as u32);
        if !self.cache.contains_key(&key) {
            let data = if mono { &self.mono } else { &self.sans };
            let face = ttf_parser::Face::parse(data, 0).ok()?;
            let u = px / face.units_per_em() as f32;
            let gid = face.glyph_index(ch)?;
            let advance = face.glyph_hor_advance(gid).unwrap_or(0) as f32 * u;
            let glyph = match face.glyph_bounding_box(gid) {
                // Whitespace and the like: only an advance.
                None => Glyph { left: 0, top: 0, width: 0, height: 0, advance, coverage: Vec::new() },
                Some(bb) => {
                    let left = (bb.x_min as f32 * u).floor() as i32 - 1;
                    let top = (bb.y_max as f32 * u).ceil() as i32 + 1;
                    let right = (bb.x_max as f32 * u).ceil() as i32 + 1;
                    let bottom = (bb.y_min as f32 * u).floor() as i32 - 1;
                    let (w, h) = ((right - left) as usize, (top - bottom) as usize);
                    let mut pb = PathBuilder::new();
                    face.outline_glyph(gid, &mut Outline { pb: &mut pb, x: -(left as f32), y: top as f32, u });
                    let mut coverage = vec![0u8; w * h];
                    if let (Some(path), Some(mut pm)) = (pb.finish(), Pixmap::new(w as u32, h as u32)) {
                        let mut paint = Paint::default();
                        paint.set_color_rgba8(255, 255, 255, 255);
                        paint.anti_alias = true;
                        pm.fill_path(&path, &paint, FillRule::Winding, Transform::identity(), None);
                        for (c, px4) in coverage.iter_mut().zip(pm.data().chunks_exact(4)) {
                            *c = px4[3];
                        }
                    }
                    Glyph { left, top, width: w, height: h, advance, coverage }
                }
            };
            self.cache.insert(key, glyph);
        }
        self.cache.get(&key)
    }
}
/// Feeds a glyph outline (font units, y up) into a tiny-skia path (pixels, y down).
struct Outline<'a> {
    pb: &'a mut PathBuilder,
    x: f32,
    y: f32,
    u: f32,
}

impl ttf_parser::OutlineBuilder for Outline<'_> {
    fn move_to(&mut self, x: f32, y: f32) {
        self.pb.move_to(self.x + x * self.u, self.y - y * self.u);
    }
    fn line_to(&mut self, x: f32, y: f32) {
        self.pb.line_to(self.x + x * self.u, self.y - y * self.u);
    }
    fn quad_to(&mut self, x1: f32, y1: f32, x: f32, y: f32) {
        self.pb.quad_to(self.x + x1 * self.u, self.y - y1 * self.u, self.x + x * self.u, self.y - y * self.u);
    }
    fn curve_to(&mut self, x1: f32, y1: f32, x2: f32, y2: f32, x: f32, y: f32) {
        self.pb.cubic_to(
            self.x + x1 * self.u, self.y - y1 * self.u,
            self.x + x2 * self.u, self.y - y2 * self.u,
            self.x + x * self.u, self.y - y * self.u,
        );
    }
    fn close(&mut self) {
        self.pb.close();
    }
}
struct App {
    hwnd: HWND,
    screen_dc: HDC,
    mem_dc: HDC,
    dib: *mut u8,
    pw: usize, // physical window size in px
    ph: usize,
    scale: f32,
    pixmap: Pixmap,
    fonts: Fonts,
    shape: Shape,
    anim: Option<Anim>,
    open_target: bool,
    hovering: bool,
    tracking: bool,
    intent_pending: bool,
    grace_pending: bool,
    pin_open: bool,
    stress: bool,
    debug: bool,
    start: Instant,
    dirty_rows: usize, // rows of the DIB that may hold non-transparent pixels
}

thread_local! {
    static APP: RefCell<Option<App>> = const { RefCell::new(None) };
}

fn log(app: &App, msg: &str) {
    if app.debug {
        eprintln!("[native +{}ms] {msg}", app.start.elapsed().as_millis());
    }
}

impl App {
    fn set_open(&mut self, open: bool) {
        if self.open_target == open {
            return;
        }
        self.open_target = open;
        let to = if open { OPEN } else { REST };
        self.anim = Some(Anim { from: self.shape, to, start: Instant::now(), stamps: Vec::new() });
    }

    fn wants_frames(&self) -> bool {
        self.anim.is_some() || (self.stress && self.open_target)
    }

    /// Advance the animation, draw and present one frame.
    fn frame(&mut self) {
        if let Some(anim) = self.anim.as_mut() {
            anim.stamps.push(Instant::now());
            let t = (anim.start.elapsed().as_secs_f32() / ANIM.as_secs_f32()).min(1.0);
            let e = ease(t);
            self.shape = Shape {
                w: lerp(anim.from.w, anim.to.w, e),
                h: lerp(anim.from.h, anim.to.h, e),
                r: lerp(anim.from.r, anim.to.r, e),
            };
            if t >= 1.0 {
                let done = self.anim.take().unwrap();
                self.shape = done.to;
                if self.debug && self.open_target {
                    let mut gaps: Vec<f64> = done.stamps.windows(2)
                        .map(|p| (p[1] - p[0]).as_secs_f64() * 1000.0).collect();
                    gaps.sort_by(|a, b| a.partial_cmp(b).unwrap());
                    if !gaps.is_empty() {
                        let q = |p: f64| gaps[((p * gaps.len() as f64) as usize).min(gaps.len() - 1)];
                        log(self, &format!("frames open: n={} p50={:.1}ms p95={:.1}ms max={:.1}ms",
                            gaps.len(), q(0.5), q(0.95), gaps[gaps.len() - 1]));
                    }
                }
            }
        }
        self.render();
        self.present();
    }

    fn render(&mut self) {
        let s = self.scale;
        self.pixmap.fill(tiny_skia::Color::TRANSPARENT);

        let sh = self.shape;
        let (sw, shh, r) = (sh.w * s, sh.h * s, sh.r * s);
        let x = (self.pw as f32 - sw) / 2.0;

        // Black notch with square top corners and rounded bottom corners.
        let k = 0.5523 * r;
        let mut pb = PathBuilder::new();
        pb.move_to(x, 0.0);
        pb.line_to(x + sw, 0.0);
        pb.line_to(x + sw, shh - r);
        pb.cubic_to(x + sw, shh - r + k, x + sw - r + k, shh, x + sw - r, shh);
        pb.line_to(x + r, shh);
        pb.cubic_to(x + r - k, shh, x, shh - r + k, x, shh - r);
        pb.close();
        let path = pb.finish().expect("notch path");
        let mut paint = Paint::default();
        paint.set_color_rgba8(0, 0, 0, 255);
        paint.anti_alias = true;
        self.pixmap.fill_path(&path, &paint, FillRule::Winding, Transform::identity(), None);

        // Progress of the open animation, to fade the contents in and out.
        let openness = ((sh.h - REST.h) / (OPEN.h - REST.h)).clamp(0.0, 1.0);
        let rest_alpha = (1.0 - openness * 3.0).clamp(0.0, 1.0);
        let open_alpha = ((openness - 0.35) / 0.5).clamp(0.0, 1.0);

        if rest_alpha > 0.0 {
            self.text(false, "CS101", 12.5, x + 28.0 * s, 21.0 * s, [163, 163, 168], rest_alpha);
            self.text(true, "3h 13m", 12.5, x + 28.0 * s + 52.0 * s, 21.0 * s, [224, 178, 90], rest_alpha);
        }
        if open_alpha > 0.0 {
            let l = x + 24.0 * s;
            self.text(true, "CS101 Algorithms", 11.5, l, 34.0 * s, [163, 163, 168], open_alpha);
            self.text(false, "Problem Set 4", 15.0, l, 58.0 * s, [242, 242, 242], open_alpha);
            self.text(true, "3h 13m", 22.0, l, 90.0 * s, [224, 178, 90], open_alpha);
            let rows = [("MA201", "Quiz 3", "1d 5h"), ("PH110", "Lab report", "3d 23h"), ("BD", "Project proposal", "10d 23h")];
            for (i, (code, title, left)) in rows.iter().enumerate() {
                let y = (130.0 + 30.0 * i as f32) * s;
                self.text(true, code, 11.5, l, y, [163, 163, 168], open_alpha);
                self.text(false, title, 13.0, l + 72.0 * s, y, [242, 242, 242], open_alpha);
                self.text(true, left, 12.0, x + sw - 24.0 * s - 56.0 * s, y, [163, 163, 168], open_alpha);
            }

            // Progress bar: moves every frame under --stress, once a second otherwise.
            let secs = self.start.elapsed().as_secs_f32();
            let p = if self.stress { (secs % 4.0) / 4.0 } else { (secs.floor() % 4.0) / 4.0 };
            let (bx0, bx1, by) = (l, x + sw - 24.0 * s, 250.0 * s);
            let mut stroke = Stroke { width: 8.0 * s, line_cap: LineCap::Round, ..Stroke::default() };
            stroke.width = 8.0 * s;
            let line = |a: f32, b: f32| {
                let mut pb = PathBuilder::new();
                pb.move_to(a, by);
                pb.line_to(b.max(a + 0.1), by);
                pb.finish().unwrap()
            };
            let mut track = Paint::default();
            track.set_color_rgba8(255, 255, 255, (40.0 * open_alpha) as u8);
            self.pixmap.stroke_path(&line(bx0, bx1), &track, &stroke, Transform::identity(), None);
            let mut fill = Paint::default();
            fill.set_color_rgba8(224, 178, 90, (255.0 * open_alpha) as u8);
            self.pixmap.stroke_path(&line(bx0, bx0 + (bx1 - bx0) * p), &fill, &stroke, Transform::identity(), None);
            self.text(true, "progress", 11.0, l, 280.0 * s, [163, 163, 168], open_alpha);
        }
    }

    fn text(&mut self, mono: bool, text: &str, size: f32, x: f32, baseline: f32, rgb: [u8; 3], alpha: f32) {
        // Below the notch's bottom edge the screen is see-through: draw nothing there.
        let limit = ((self.shape.h * self.scale).floor() as i32).min(self.ph as i32);
        let px = size * self.scale;
        let (pw, base) = (self.pw as i32, baseline.round() as i32);
        let mut pen = x;
        for ch in text.chars() {
            let Some(g) = self.fonts.glyph(mono, ch, px) else {
                pen += px * 0.5;
                continue;
            };
            let (gx0, gy0, gw, gh, adv) = (pen.round() as i32 + g.left, base - g.top, g.width, g.height, g.advance);
            let data = self.pixmap.data_mut();
            for row in 0..gh {
                let y = gy0 + row as i32;
                if y < 0 || y >= limit {
                    continue;
                }
                for col in 0..gw {
                    let xx = gx0 + col as i32;
                    if xx < 0 || xx >= pw {
                        continue;
                    }
                    let cov = g.coverage[row * gw + col];
                    if cov == 0 {
                        continue;
                    }
                    let a = cov as f32 / 255.0 * alpha;
                    let i = (y as usize * self.pw + xx as usize) * 4;
                    for c in 0..3 {
                        data[i + c] = (rgb[c] as f32 * a + data[i + c] as f32 * (1.0 - a)) as u8;
                    }
                    data[i + 3] = (255.0 * a + data[i + 3] as f32 * (1.0 - a)) as u8;
                }
            }
            pen += adv;
        }
    }
    /// Copy the pixmap (premultiplied RGBA) into the DIB (premultiplied BGRA) and show it.
    fn present(&mut self) {
        let rows = ((self.shape.h * self.scale).ceil() as usize + 4).min(self.ph);
        let copy_rows = rows.max(self.dirty_rows).min(self.ph);
        let src = self.pixmap.data();
        let dst = unsafe { std::slice::from_raw_parts_mut(self.dib, self.pw * self.ph * 4) };
        let n = copy_rows * self.pw * 4;
        for (d, s) in dst[..n].chunks_exact_mut(4).zip(src[..n].chunks_exact(4)) {
            d[0] = s[2];
            d[1] = s[1];
            d[2] = s[0];
            d[3] = s[3];
        }
        self.dirty_rows = rows;

        let size = SIZE { cx: self.pw as i32, cy: self.ph as i32 };
        let origin = POINT { x: 0, y: 0 };
        let blend = BLENDFUNCTION {
            BlendOp: AC_SRC_OVER as u8,
            BlendFlags: 0,
            SourceConstantAlpha: 255,
            AlphaFormat: AC_SRC_ALPHA as u8,
        };
        unsafe {
            let _ = UpdateLayeredWindow(
                self.hwnd, Some(self.screen_dc), None, Some(&size),
                Some(self.mem_dc), Some(&origin), COLORREF(0), Some(&blend), ULW_ALPHA,
            );
        }
    }
}

fn with_app<R>(f: impl FnOnce(&mut App) -> R) -> Option<R> {
    APP.with(|a| a.borrow_mut().as_mut().map(f))
}

unsafe extern "system" fn wndproc(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    match msg {
        WM_MOUSEACTIVATE => return LRESULT(MA_NOACTIVATE as isize),
        WM_MOUSEMOVE => {
            with_app(|app| {
                if !app.tracking {
                    let mut tme = TRACKMOUSEEVENT {
                        cbSize: std::mem::size_of::<TRACKMOUSEEVENT>() as u32,
                        dwFlags: TME_LEAVE,
                        hwndTrack: hwnd,
                        dwHoverTime: 0,
                    };
                    let _ = TrackMouseEvent(&mut tme);
                    app.tracking = true;
                }
                app.hovering = true;
                if app.grace_pending {
                    let _ = KillTimer(Some(hwnd), T_GRACE);
                    app.grace_pending = false;
                }
                if !app.open_target && !app.intent_pending {
                    SetTimer(Some(hwnd), T_INTENT, INTENT_MS, None);
                    app.intent_pending = true;
                }
            });
            return LRESULT(0);
        }
        WM_MOUSELEAVE => {
            with_app(|app| {
                app.tracking = false;
                app.hovering = false;
                if app.intent_pending {
                    let _ = KillTimer(Some(hwnd), T_INTENT);
                    app.intent_pending = false;
                }
                if app.open_target && !app.pin_open && !app.grace_pending {
                    SetTimer(Some(hwnd), T_GRACE, GRACE_MS, None);
                    app.grace_pending = true;
                }
            });
            return LRESULT(0);
        }
        WM_TIMER => {
            with_app(|app| match wparam.0 {
                T_INTENT => {
                    let _ = KillTimer(Some(hwnd), T_INTENT);
                    app.intent_pending = false;
                    app.set_open(true);
                }
                T_GRACE => {
                    let _ = KillTimer(Some(hwnd), T_GRACE);
                    app.grace_pending = false;
                    if !app.hovering {
                        app.set_open(false);
                    }
                }
                T_SECOND => {
                    // Once-a-second refresh while expanded (progress, countdown).
                    if app.open_target && app.anim.is_none() && !app.stress {
                        app.frame();
                    }
                }
                T_EXIT => PostQuitMessage(0),
                _ => {}
            });
            return LRESULT(0);
        }
        WM_DESTROY => {
            PostQuitMessage(0);
            return LRESULT(0);
        }
        _ => {}
    }
    DefWindowProcW(hwnd, msg, wparam, lparam)
}

fn main() {
    let start = Instant::now();
    let args: Vec<String> = std::env::args().collect();
    let flag = |name: &str| args.iter().any(|a| a == name);
    let seconds: Option<u32> = args.iter().position(|a| a == "--seconds")
        .and_then(|i| args.get(i + 1)).and_then(|v| v.parse().ok());
    let debug = std::env::var_os("DN_DEBUG").is_some();

    unsafe {
        let _ = SetProcessDPIAware();
        let hinstance = GetModuleHandleW(None).expect("module").into();
        let class = w!("NativeNotchSpike");
        let wc = WNDCLASSEXW {
            cbSize: std::mem::size_of::<WNDCLASSEXW>() as u32,
            lpfnWndProc: Some(wndproc),
            hInstance: hinstance,
            hCursor: LoadCursorW(None, IDC_ARROW).unwrap_or_default(),
            lpszClassName: class,
            ..Default::default()
        };
        RegisterClassExW(&wc);

        let dpi = windows::Win32::UI::HiDpi::GetDpiForSystem() as f32;
        let scale = dpi / 96.0;
        let (pw, ph) = ((WIN_W * scale).round() as usize, (WIN_H * scale).round() as usize);
        let screen_w = GetSystemMetrics(SM_CXSCREEN);
        let x = (screen_w - pw as i32) / 2;

        let hwnd = CreateWindowExW(
            WS_EX_LAYERED | WS_EX_TOPMOST | WS_EX_TOOLWINDOW | WS_EX_NOACTIVATE,
            class, PCWSTR::null(), WS_POPUP, x, 0, pw as i32, ph as i32,
            None, None, Some(hinstance), None,
        ).expect("window");
        // Title is only for tooling that looks windows up by name.
        let _ = windows::Win32::UI::WindowsAndMessaging::SetWindowTextW(hwnd, w!("Native Notch Spike"));

        let screen_dc = GetDC(None);
        let mem_dc = CreateCompatibleDC(Some(screen_dc));
        let bmi = BITMAPINFO {
            bmiHeader: BITMAPINFOHEADER {
                biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
                biWidth: pw as i32,
                biHeight: -(ph as i32), // top-down
                biPlanes: 1,
                biBitCount: 32,
                biCompression: BI_RGB.0,
                ..Default::default()
            },
            ..Default::default()
        };
        let mut bits: *mut c_void = std::ptr::null_mut();
        let dib = CreateDIBSection(Some(mem_dc), &bmi, DIB_RGB_COLORS, &mut bits, None, 0).expect("dib");
        SelectObject(mem_dc, HGDIOBJ(dib.0));

        let mut app = App {
            hwnd, screen_dc, mem_dc, dib: bits as *mut u8, pw, ph, scale,
            pixmap: Pixmap::new(pw as u32, ph as u32).expect("pixmap"),
            fonts: Fonts::load(),
            shape: REST, anim: None, open_target: false, hovering: false, tracking: false,
            intent_pending: false, grace_pending: false,
            pin_open: flag("--open"), stress: flag("--stress"), debug, start, dirty_rows: 0,
        };
        // --dump <prefix>: write the rest and open frames as PNGs and exit (no window shown,
        // no cursor needed), to check how it looks.
        if let Some(prefix) = args.iter().position(|a| a == "--dump").and_then(|i| args.get(i + 1)) {
            app.render();
            let _ = app.pixmap.save_png(format!("{prefix}-rest.png"));
            app.shape = OPEN;
            app.render();
            let _ = app.pixmap.save_png(format!("{prefix}-open.png"));
            return;
        }
        app.render();
        app.present();
        let _ = ShowWindow(hwnd, SW_SHOWNOACTIVATE);
        log(&app, "window shown");
        if app.pin_open {
            app.shape = OPEN;
            app.open_target = true;
            app.render();
            app.present();
        }
        APP.with(|a| *a.borrow_mut() = Some(app));

        SetTimer(Some(hwnd), T_SECOND, 1000, None);
        if let Some(s) = seconds {
            SetTimer(Some(hwnd), T_EXIT, s * 1000, None);
        }

        let mut msg = MSG::default();
        loop {
            let animating = APP.with(|a| a.borrow().as_ref().is_some_and(|app| app.wants_frames()));
            if animating {
                while PeekMessageW(&mut msg, None, 0, 0, PM_REMOVE).as_bool() {
                    if msg.message == WM_QUIT {
                        return;
                    }
                    let _ = TranslateMessage(&msg);
                    DispatchMessageW(&msg);
                }
                with_app(|app| app.frame());
                let _ = DwmFlush(); // pace to the display refresh
            } else {
                // Resting: sleep until the mouse or a timer wakes us. Zero CPU.
                if GetMessageW(&mut msg, None, 0, 0).0 <= 0 {
                    return;
                }
                let _ = TranslateMessage(&msg);
                DispatchMessageW(&msg);
            }
        }
    }
}
