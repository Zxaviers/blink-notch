//! The notch window: a layered, click-through-where-transparent popup that
//! never takes focus, plus the message loop that feeds the pure `App`.
//!
//! All state lives in one `Runtime` on the window thread. Windows messages and
//! control-pipe commands both end up in `Runtime` methods, so a command in a
//! test goes through the same code as a real mouse event, minus the OS.

use std::cell::RefCell;
use std::ffi::c_void;
use std::path::PathBuf;
use std::sync::atomic::{AtomicIsize, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use blink_core::settings::{normalize_settings, Settings};
use blink_core::store::{default_dir, Store};
use jiff::tz::TimeZone;
use serde_json::{json, Value};
use windows::core::{w, BOOL, PCWSTR};
use windows::Win32::Foundation::{COLORREF, HWND, LPARAM, LRESULT, POINT, RECT, SIZE, WPARAM};
use windows::Win32::Graphics::Dwm::DwmFlush;
use windows::Win32::Graphics::Gdi::{
    CreateCompatibleDC, CreateDIBSection, DeleteDC, DeleteObject, EnumDisplayMonitors, GetMonitorInfoW, SelectObject, AC_SRC_ALPHA,
    AC_SRC_OVER, BITMAPINFO, BITMAPINFOHEADER, BI_RGB, BLENDFUNCTION, DIB_RGB_COLORS, HBITMAP, HDC, HMONITOR, MONITORINFO,
};
use windows::Win32::Globalization::GetUserDefaultLocaleName;
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::System::Registry::{RegGetValueW, HKEY_CURRENT_USER, RRF_RT_REG_DWORD};
use windows::Win32::UI::HiDpi::{GetDpiForMonitor, SetProcessDpiAwarenessContext, DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2, MDT_EFFECTIVE_DPI};
use windows::Win32::UI::Accessibility::{SetWinEventHook, HWINEVENTHOOK};
use windows::Win32::UI::Input::KeyboardAndMouse::{ReleaseCapture, SetCapture, RegisterHotKey, TrackMouseEvent, UnregisterHotKey, HOT_KEY_MODIFIERS, TME_LEAVE, TRACKMOUSEEVENT};
use windows::Win32::UI::Shell::ShellExecuteW;
use windows::Win32::UI::WindowsAndMessaging::{
    SetWindowPos, HWND_TOPMOST, SWP_NOACTIVATE, SWP_NOMOVE, SWP_NOSIZE, RegisterWindowMessageW, CreateWindowExW, DefWindowProcW, DestroyWindow, DispatchMessageW, GetClassNameW, GetForegroundWindow, GetMessageW, GetQueueStatus,
    GetWindowLongPtrW, GetWindowRect, IsWindowVisible, KillTimer, LoadCursorW, PeekMessageW, PostMessageW, PostQuitMessage,
    RegisterClassExW, SetTimer, ShowWindow, SystemParametersInfoW, TranslateMessage, UpdateLayeredWindow, WindowFromPoint, CS_HREDRAW,
    CS_VREDRAW, GWL_EXSTYLE, IDC_ARROW, MSG, PM_REMOVE, QS_ALLINPUT, SPI_GETCLIENTAREAANIMATION, SW_SHOWNOACTIVATE, SW_SHOWNORMAL,
    ULW_ALPHA, EVENT_SYSTEM_FOREGROUND, SW_HIDE, WINEVENT_OUTOFCONTEXT, WM_ACTIVATE, WM_CHAR, WM_DESTROY, WM_DISPLAYCHANGE, WM_DPICHANGED, WM_ERASEBKGND, WM_HOTKEY, WM_KEYDOWN,
    WM_LBUTTONDOWN, WM_LBUTTONUP, WM_MBUTTONDOWN, WM_MOUSEACTIVATE, WM_MOUSEMOVE, WM_MOUSEWHEEL, WM_QUIT, WM_SETTINGCHANGE, WM_TIMER, WNDCLASSEXW,
    WS_EX_LAYERED, WS_EX_NOACTIVATE, WS_EX_TOOLWINDOW, WS_EX_TOPMOST, WS_POPUP,
};

use super::control::{self, Request, WM_CONTROL, WM_IPC};
use super::focus;
use super::system::{self, WM_SHOW_ME};
use super::mediakeys::{self, WM_MEDIAKEY};
use super::sysevents::{self, KeyHint, Sources, WM_INPUT, WM_SYSEVENT};
use super::uia;
use super::tray::{self, MenuLabels, Tray, WM_TRAY};
use crate::app::{App, AppEffect};
use crate::clock::Clock;
use crate::live::LiveSource;
use crate::modules::clock::ClockModule;
use crate::modules::controls::{ControlsModule, FakeControls};
use crate::modules::music::{FakeLyrics, FakeMedia, MediaBackend, MusicModule};
use crate::modules::projects::ProjectsModule;
use crate::modules::system::SystemModule;
use crate::modules::deadlines::{DeadlineSource, DeadlinesModule, DemoSource};
use crate::modules::{Key, Module, PointerKind};
use crate::shell::{WINDOW_H, WINDOW_W};

mod requests;
mod scripted;

/// Wall clock a manual-clock run starts at (2026-10-22 12:00 UTC), so tests are repeatable.
const MANUAL_WALL_BASE: i64 = 1_792_670_400_000;
const TIMER_WAKE: usize = 1;
const TIMER_QUIT: usize = 2;
const TIMER_POLL: usize = 3;
const TIMER_A11Y: usize = 4;
const TIMER_KEYS: usize = 5;
/// Keep screen-reader snapshots at most this often (mouse moves arrive far faster).
const A11Y_EVERY_MS: u128 = 100;
/// Slow safety net for the fullscreen check (the foreground hook is the fast path).
const POLL_MS: u32 = 3000;
/// Posted by the foreground hook.
const WM_FOREGROUND: u32 = 0x8000 + 4;
const HOTKEY_ID: i32 = 1;
/// Shows or hides the whole notch.
const HOTKEY_VISIBILITY_ID: i32 = 2;
/// Posted by a feed worker when it has new data.
const WM_DATA: u32 = 0x8000 + 5;
const WM_POWERBROADCAST: u32 = 0x0218;
const WM_TIMECHANGE: u32 = 0x001E;
const PBT_RESUME: usize = 0x0012; // PBT_APMRESUMEAUTOMATIC
const PBT_RESUME_SUSPEND: usize = 0x0007;
const NIN_BALLOONUSERCLICK: u32 = 0x0405;
static HOOK_TARGET: AtomicIsize = AtomicIsize::new(0);
/// Logical px scrolled per wheel notch.
const WHEEL_STEP: f32 = 48.0;
const WM_MOUSELEAVE: u32 = 0x02A3;

#[derive(Default)]
pub struct Options {
    pub pin_open: bool,
    /// Quit after this many seconds (for measurements).
    pub seconds: Option<u64>,
    /// Write a PNG of the last frame here on exit.
    pub dump: Option<String>,
    /// Name of the control pipe (`BLINK_CONTROL`).
    pub control: Option<String>,
    pub manual_clock: bool,
    pub data_dir: Option<PathBuf>,
    pub scale: Option<f32>,
    /// Show the sample deadlines instead of the user's feeds (screenshots, tests).
    pub demo: bool,
}

// -------------------------------------------------------------- the surface

/// A 32-bit top-down DIB the frame is copied into before `UpdateLayeredWindow`.
struct Surface {
    dc: HDC,
    bitmap: HBITMAP,
    bits: *mut u32,
    w: u32,
    h: u32,
}

impl Surface {
    fn new(w: u32, h: u32) -> Result<Surface, String> {
        unsafe {
            let dc = CreateCompatibleDC(None);
            let info = BITMAPINFO {
                bmiHeader: BITMAPINFOHEADER {
                    biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
                    biWidth: w as i32,
                    biHeight: -(h as i32),
                    biPlanes: 1,
                    biBitCount: 32,
                    biCompression: BI_RGB.0,
                    ..Default::default()
                },
                ..Default::default()
            };
            let mut bits: *mut c_void = std::ptr::null_mut();
            let bitmap = CreateDIBSection(Some(dc), &info, DIB_RGB_COLORS, &mut bits, None, 0).map_err(|e| format!("CreateDIBSection: {e}"))?;
            SelectObject(dc, bitmap.into());
            Ok(Surface { dc, bitmap, bits: bits as *mut u32, w, h })
        }
    }

    fn pixels(&mut self) -> &mut [u32] {
        unsafe { std::slice::from_raw_parts_mut(self.bits, (self.w * self.h) as usize) }
    }
}

impl Drop for Surface {
    fn drop(&mut self) {
        unsafe {
            let _ = DeleteDC(self.dc);
            let _ = DeleteObject(self.bitmap.into());
        }
    }
}

// ------------------------------------------------------------------ monitors

#[derive(Clone, Copy)]
struct MonitorInfoLite {
    rect: RECT,
    primary: bool,
    handle: HMONITOR,
}

fn monitors() -> Vec<MonitorInfoLite> {
    unsafe extern "system" fn each(handle: HMONITOR, _: HDC, rect: *mut RECT, data: LPARAM) -> BOOL {
        let list = &mut *(data.0 as *mut Vec<MonitorInfoLite>);
        let mut info = MONITORINFO { cbSize: std::mem::size_of::<MONITORINFO>() as u32, ..Default::default() };
        let primary = GetMonitorInfoW(handle, &mut info).as_bool() && info.dwFlags & 1 != 0;
        list.push(MonitorInfoLite { rect: *rect, primary, handle });
        BOOL(1)
    }
    let mut list: Vec<MonitorInfoLite> = Vec::new();
    unsafe {
        let _ = EnumDisplayMonitors(None, None, Some(each), LPARAM(&mut list as *mut _ as isize));
    }
    list
}

fn monitor_scale(m: &MonitorInfoLite) -> f32 {
    let (mut x, mut y) = (96u32, 96u32);
    unsafe {
        let _ = GetDpiForMonitor(m.handle, MDT_EFFECTIVE_DPI, &mut x, &mut y);
    }
    x as f32 / 96.0
}

// --------------------------------------------------------------- system info

fn system_language() -> String {
    let mut buf = [0u16; 85];
    let n = unsafe { GetUserDefaultLocaleName(&mut buf) };
    if n > 1 {
        String::from_utf16_lossy(&buf[..n as usize - 1])
    } else {
        "en-US".to_string()
    }
}

/// Windows "show animations" setting: false means the user wants less motion.
fn animations_enabled() -> bool {
    let mut on = BOOL(1);
    unsafe {
        let _ = SystemParametersInfoW(SPI_GETCLIENTAREAANIMATION, 0, Some(&mut on as *mut _ as *mut c_void), Default::default());
    }
    on.as_bool()
}

/// Whether apps are set to the light theme.
fn apps_use_light_theme() -> bool {
    let mut value = 0u32;
    let mut size = 4u32;
    let ok = unsafe {
        RegGetValueW(
            HKEY_CURRENT_USER,
            w!(r"Software\Microsoft\Windows\CurrentVersion\Themes\Personalize"),
            w!("AppsUseLightTheme"),
            RRF_RT_REG_DWORD,
            None,
            Some(&mut value as *mut _ as *mut c_void),
            Some(&mut size),
        )
    };
    ok.is_ok() && value != 0
}

/// Open a file or folder the way a double-click does.
fn open_path(path: &str) {
    let wide: Vec<u16> = path.encode_utf16().chain(Some(0)).collect();
    unsafe {
        ShellExecuteW(None, w!("open"), PCWSTR(wide.as_ptr()), None, None, SW_SHOWNORMAL);
    }
}

fn open_url(url: &str) {
    let lower = url.to_ascii_lowercase();
    if !(lower.starts_with("https://") || lower.starts_with("http://")) {
        return;
    }
    let wide: Vec<u16> = url.encode_utf16().chain(Some(0)).collect();
    unsafe {
        ShellExecuteW(None, w!("open"), PCWSTR(wide.as_ptr()), None, None, SW_SHOWNORMAL);
    }
}

// ------------------------------------------------------------------ runtime

struct Runtime {
    hwnd: HWND,
    app: App,
    store: Store,
    surface: Surface,
    /// Screen position of the window's top-left corner (physical px).
    origin: (i32, i32),
    tracking: bool,
    /// We hold real keyboard focus (so losing activation ends keyboard mode).
    kb_active: bool,
    dirty: bool,
    /// Rows of the surface that held pixels in the last frame (to clear what shrank away).
    prev_rows: usize,
    scale_override: Option<f32>,
    frames: u64,
    control: bool,
    /// In control mode, focus changes are logged instead of really taking focus.
    sim_focus: bool,
    effects_log: Vec<String>,
    dump_on_exit: Option<String>,
    paused: bool,
    fullscreen: bool,
    fullscreen_sim: Option<system::FullscreenKind>,
    /// Scripted runs pretend the desktop is (or is not) in front; None asks Windows.
    desktop_sim: Option<bool>,
    fullscreen_mode: crate::host::FullscreenMode,
    hidden: bool,
    tray: Option<Tray>,
    /// What clicking the last notification opens.
    toast_url: Option<String>,
    taskbar_created: u32,
    hotkey: String,
    /// The shortcut that shows or hides the notch, as registered ("" when off).
    vis_hotkey: String,
    /// The user showed the notch while a fullscreen app is in front: stays until that app is gone.
    fullscreen_override: bool,
    /// Time between consecutive animation frames (ms), for the pacing check.
    frame_times: Vec<f32>,
    /// Where the time of each presented frame went, in ms: (render, copy, UpdateLayeredWindow), summed since `frames` last asked.
    cost: (f32, f32, f32, u32),
    /// What woke the window since `frames` last asked: window messages by number (timers by id), for finding what keeps it busy.
    wakes: std::collections::BTreeMap<String, u32>,
    last_frame: Option<Instant>,
    last_a11y: Instant,
    sources: Sources,
    /// The user wants the volume keys taken over (the hook only acts while the notch is showing).
    take_volume_keys: bool,
    media_keys: bool,
    winctl: Option<std::sync::Arc<super::winctl::WinControls>>,
    fake_controls: Option<std::sync::Arc<FakeControls>>,
    fake_media: Option<std::sync::Arc<FakeMedia>>,
    caps_last: bool,
    /// Whether the key timer should look at Caps Lock when it fires.
    key_checks: u8,
}

thread_local! {
    static RT: RefCell<Option<Runtime>> = const { RefCell::new(None) };
}

/// Run `f` on the runtime. None while the runtime is already borrowed
/// (a message sent synchronously from inside one of its own calls).
fn with_rt<R>(f: impl FnOnce(&mut Runtime) -> R) -> Option<R> {
    RT.with(|cell| cell.try_borrow_mut().ok().and_then(|mut guard| guard.as_mut().map(f)))
}

fn lo(v: isize) -> i32 {
    (v & 0xffff) as i16 as i32
}

fn hi(v: isize) -> i32 {
    ((v >> 16) & 0xffff) as i16 as i32
}

impl Runtime {
    fn scale(&self) -> f32 {
        self.app.scale()
    }

    // ---------------------------------------------------------- placement

    fn place(&mut self) {
        let list = monitors();
        let settings = self.app.settings();
        let pick = list.get(settings.monitor as usize).or_else(|| list.iter().find(|m| m.primary)).or_else(|| list.first()).copied();
        let offset = settings.offset_x as f32 * self.scale();
        if let Some(m) = pick {
            let mw = m.rect.right - m.rect.left;
            let win_w = self.surface.w as i32;
            let mut x = m.rect.left + (mw - win_w) / 2 + offset.round() as i32;
            x = x.clamp(m.rect.left, (m.rect.right - win_w).max(m.rect.left));
            self.origin = (x, m.rect.top);
        }
        self.dirty = true;
    }

    fn set_scale(&mut self, scale: f32) {
        if (scale - self.scale()).abs() < 0.001 {
            return;
        }
        self.app.set_scale(scale);
        if let Ok(surface) = Surface::new(self.app.canvas().width(), self.app.canvas().height()) {
            self.surface = surface;
            self.prev_rows = self.surface.h as usize;
        }
        self.place();
    }

    // ------------------------------------------------------------ drawing

    /// Render the app and hand the rows that hold pixels to the window.
    fn present(&mut self) {
        if self.hidden {
            return;
        }
        let t0 = Instant::now();
        self.app.render();
        let t1 = Instant::now();
        // How far this frame really reaches, measured on the canvas. (Estimating it from the shape
        // at a slightly later moment under-counts while the notch moves fast, and the rows between
        // the guess and the truth kept their old pixels: trails of earlier frames.)
        let rows = self.app.canvas().content_rows();
        let copy_rows = rows.max(self.prev_rows).min(self.surface.h as usize);
        let width = self.surface.w as usize;
        {
            let src = self.app.canvas().pm.data();
            let dst = self.surface.pixels();
            for y in 0..copy_rows {
                let from = y * width * 4;
                for x in 0..width {
                    let i = from + x * 4;
                    // tiny-skia stores premultiplied RGBA; a layered window wants premultiplied BGRA.
                    dst[y * width + x] =
                        (u32::from(src[i + 3]) << 24) | (u32::from(src[i]) << 16) | (u32::from(src[i + 1]) << 8) | u32::from(src[i + 2]);
                }
            }
        }
        self.prev_rows = rows;
        let t2 = Instant::now();
        let origin = POINT { x: self.origin.0, y: self.origin.1 };
        let size = SIZE { cx: self.surface.w as i32, cy: self.surface.h as i32 };
        let source = POINT::default();
        let blend = BLENDFUNCTION { BlendOp: AC_SRC_OVER as u8, BlendFlags: 0, SourceConstantAlpha: 255, AlphaFormat: AC_SRC_ALPHA as u8 };
        unsafe {
            let _ = UpdateLayeredWindow(self.hwnd, None, Some(&origin), Some(&size), Some(self.surface.dc), Some(&source), COLORREF(0), Some(&blend), ULW_ALPHA);
        }
        let t3 = Instant::now();
        let ms = |a: Instant, b: Instant| b.duration_since(a).as_secs_f32() * 1000.0;
        self.cost = (self.cost.0 + ms(t0, t1), self.cost.1 + ms(t1, t2), self.cost.2 + ms(t2, t3), self.cost.3 + 1);
        self.frames += 1;
        self.dirty = false;
    }

    fn flush(&mut self) {
        if self.dirty {
            self.present();
        }
    }

    /// One animation frame.
    fn frame(&mut self) {
        let now = Instant::now();
        if let Some(last) = self.last_frame {
            let dt = now.duration_since(last).as_secs_f32() * 1000.0;
            // A long gap means the previous animation ended; this frame starts a new one.
            if dt < 100.0 && self.frame_times.len() < 4000 {
                self.frame_times.push(dt);
            }
        }
        self.last_frame = Some(now);
        self.app.tick();
        self.process_effects();
        self.present();
        if !self.app.wants_frames() {
            self.last_frame = None;
            self.reschedule();
            self.publish_a11y();
        }
    }

    /// Arm one timer for the next moment something must run on its own.
    fn reschedule(&mut self) {
        if self.app.clock.is_manual() {
            return;
        }
        let now = self.app.now();
        unsafe {
            match self.app.next_wake() {
                Some(at) => {
                    let delay = at.saturating_sub(now).clamp(1, 0x7fff_fff0) as u32;
                    SetTimer(Some(self.hwnd), TIMER_WAKE, delay, None);
                }
                None => {
                    let _ = KillTimer(Some(self.hwnd), TIMER_WAKE);
                }
            }
        }
    }

    /// After anything that can change the app: carry out its effects, redraw, re-arm.
    fn after(&mut self) {
        self.process_effects();
        self.dirty = true;
        self.reschedule();
        self.publish_a11y_soon();
    }

    /// Keep what a screen reader sees current. Costs nothing until one has asked.
    fn publish_a11y(&mut self) {
        if !uia::active() {
            return;
        }
        self.last_a11y = Instant::now();
        let tree = self.app.accessibility();
        uia::publish(self.hwnd, tree, self.origin, self.scale());
    }

    /// Publish now, or shortly if one just went out (a burst of mouse moves makes one).
    fn publish_a11y_soon(&mut self) {
        if !uia::active() {
            return;
        }
        if self.last_a11y.elapsed().as_millis() >= A11Y_EVERY_MS {
            self.publish_a11y();
        } else {
            unsafe {
                SetTimer(Some(self.hwnd), TIMER_A11Y, A11Y_EVERY_MS as u32 + 20, None);
            }
        }
    }

    fn process_effects(&mut self) {
        for effect in self.app.take_effects() {
            match effect {
                AppEffect::OpenUrl(url) => {
                    if self.control {
                        self.effects_log.push(format!("open_url:{url}"));
                    } else {
                        open_url(&url);
                    }
                }
                AppEffect::OpenPath(path) => {
                    if self.control && std::env::var_os("BLINK_REAL_SOURCES").is_none() {
                        self.effects_log.push(format!("open_path:{path}"));
                    } else {
                        open_path(&path);
                    }
                }
                AppEffect::RevealPath(path) => {
                    if self.control && std::env::var_os("BLINK_REAL_SOURCES").is_none() {
                        self.effects_log.push(format!("reveal_path:{path}"));
                    } else {
                        let _ = std::process::Command::new("explorer.exe").arg(format!("/select,{path}")).spawn();
                    }
                }
                AppEffect::OpenSettings => self.open_settings("feeds"),
                AppEffect::OpenAppSettings => self.open_settings("general"),
                AppEffect::OpenSettingsAt(section) => self.open_settings(&section),
                AppEffect::RefreshFeeds => self.effects_log.push("refresh_feeds".into()),
                AppEffect::Toast { title, body, url } => {
                    if self.control {
                        self.effects_log.push(format!("toast:{title}|{body}"));
                    } else {
                        let _ = self.toast(&title, &body, url);
                    }
                }
                AppEffect::TourDone => {
                    // Remembered, so the tour comes once (the settings can show it again).
                    let _ = self.store.write_atomic("tour.json", b"{\"done\":true}");
                    if self.control {
                        self.effects_log.push("tour_done".to_string());
                    }
                }
                AppEffect::Announce(text) => {
                    if self.control {
                        self.effects_log.push(format!("announce:{text}"));
                    }
                    uia::announce(self.hwnd, &text);
                }
                AppEffect::KeyboardFocus(on) => {
                    if self.control && self.sim_focus {
                        self.effects_log.push(format!("keyboard_focus:{on}"));
                    } else if on {
                        if focus::enter(self.hwnd) {
                            self.kb_active = true;
                        } else {
                            // Windows refused the foreground: leave keyboard mode rather than sit open and deaf.
                            focus::leave(self.hwnd, false);
                            self.app.focus_lost();
                        }
                    } else {
                        self.kb_active = false;
                        focus::leave(self.hwnd, true);
                    }
                }
            }
        }
    }

    // ------------------------------------------------- visibility, hotkey

    fn refresh_fullscreen(&mut self) {
        use crate::host::FullscreenMode;
        use system::FullscreenKind;
        let kind = self.fullscreen_sim.unwrap_or_else(system::fullscreen_kind);
        let busy = match self.fullscreen_mode {
            FullscreenMode::Never => false,
            FullscreenMode::Games => kind == FullscreenKind::Game,
            FullscreenMode::All => kind != FullscreenKind::None,
        };
        if busy != self.fullscreen {
            self.fullscreen = busy;
            if !busy {
                self.fullscreen_override = false;
            }
            self.update_hidden();
        }
        let desktop = self.desktop_sim.unwrap_or_else(system::desktop_in_front);
        if desktop != self.app.desktop() {
            self.app.set_desktop(desktop);
            self.dirty = true;
        }
        self.keep_on_top();
    }

    /// Other always-on-top windows (the taskbar, a game overlay, the Start menu) can end up above
    /// the notch; put it back on top whenever the foreground changes and on the slow check.
    fn keep_on_top(&self) {
        if !self.hidden {
            unsafe {
                let _ = SetWindowPos(self.hwnd, Some(HWND_TOPMOST), 0, 0, 0, 0, SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE);
            }
        }
    }

    /// Why the notch is not showing, if it is not: "paused" (the tray or the shortcut), or
    /// "fullscreen" (an app is fullscreen and the notch steps aside).
    fn hidden_by(&self) -> Option<&'static str> {
        if self.paused {
            Some("paused")
        } else if self.fullscreen && !self.fullscreen_override {
            Some("fullscreen")
        } else {
            None
        }
    }

    /// The show or hide shortcut: whatever the reason it is gone, bring it back; otherwise hide it.
    fn toggle_visibility(&mut self) {
        if self.hidden {
            self.paused = false;
            if self.fullscreen {
                self.fullscreen_override = true;
            }
        } else {
            self.paused = true;
        }
        self.update_hidden();
        self.after();
    }

    /// Hide while paused or while a fullscreen app is in front.
    fn update_hidden(&mut self) {
        let want = self.hidden_by().is_some();
        if want == self.hidden {
            return;
        }
        self.hidden = want;
        self.apply_media_keys();
        unsafe {
            let _ = ShowWindow(self.hwnd, if want { SW_HIDE } else { SW_SHOWNOACTIVATE });
        }
        if want {
            // Whatever hover or keyboard state there was is gone with the window.
            self.app.pointer(PointerKind::Leave, 0.0, 0.0);
            self.tracking = false;
            if self.kb_active {
                self.kb_active = false;
                focus::leave(self.hwnd, true);
                self.app.focus_lost();
            }
        } else {
            self.prev_rows = self.surface.h as usize;
            self.dirty = true;
        }
    }

    /// (Re)register the global hotkey: "" turns it off. Returns "ok", "HOTKEY_INVALID" or
    /// "HOTKEY_IN_USE"; on failure the previous hotkey stays registered.
    fn register_hotkey(&mut self, accelerator: &str) -> &'static str {
        let wanted = accelerator.trim();
        if wanted == self.hotkey {
            return "ok";
        }
        let parsed = if wanted.is_empty() {
            None
        } else {
            match blink_core::hotkey::parse_accelerator(wanted) {
                Some(p) => Some(p),
                None => return "HOTKEY_INVALID",
            }
        };
        unsafe {
            let _ = UnregisterHotKey(Some(self.hwnd), HOTKEY_ID);
            if let Some((mods, vk)) = parsed {
                // MOD_NOREPEAT (0x4000): holding the keys must not toggle repeatedly.
                if RegisterHotKey(Some(self.hwnd), HOTKEY_ID, HOT_KEY_MODIFIERS(mods | 0x4000), vk).is_err() {
                    if let Some((m, k)) = blink_core::hotkey::parse_accelerator(&self.hotkey) {
                        let _ = RegisterHotKey(Some(self.hwnd), HOTKEY_ID, HOT_KEY_MODIFIERS(m | 0x4000), k);
                    }
                    return "HOTKEY_IN_USE";
                }
            }
        }
        self.hotkey = wanted.to_string();
        "ok"
    }

    /// Same as `register_hotkey`, for the show/hide shortcut.
    fn register_visibility_hotkey(&mut self, accelerator: &str) -> &'static str {
        let wanted = accelerator.trim();
        if wanted == self.vis_hotkey {
            return "ok";
        }
        let parsed = if wanted.is_empty() {
            None
        } else {
            match blink_core::hotkey::parse_accelerator(wanted) {
                Some(p) => Some(p),
                None => return "HOTKEY_INVALID",
            }
        };
        unsafe {
            let _ = UnregisterHotKey(Some(self.hwnd), HOTKEY_VISIBILITY_ID);
            if let Some((mods, vk)) = parsed {
                if RegisterHotKey(Some(self.hwnd), HOTKEY_VISIBILITY_ID, HOT_KEY_MODIFIERS(mods | 0x4000), vk).is_err() {
                    if let Some((m, k)) = blink_core::hotkey::parse_accelerator(&self.vis_hotkey) {
                        let _ = RegisterHotKey(Some(self.hwnd), HOTKEY_VISIBILITY_ID, HOT_KEY_MODIFIERS(m | 0x4000), k);
                    }
                    return "HOTKEY_IN_USE";
                }
            }
        }
        self.vis_hotkey = wanted.to_string();
        "ok"
    }

    fn menu_labels(&self) -> MenuLabels {
        let t = self.app.translator();
        MenuLabels { pause: t.t("tray.pause", &[]), refresh: t.t("tray.refresh", &[]), settings: t.t("tray.settings", &[]), quit: t.t("tray.quit", &[]) }
    }

    fn tray_command(&mut self, cmd: u32) {
        match cmd {
            tray::CMD_PAUSE => {
                self.paused = !self.paused;
                self.update_hidden();
            }
            tray::CMD_REFRESH => self.app.refresh_sources(),
            tray::CMD_SETTINGS => self.open_settings("general"),
            tray::CMD_QUIT => unsafe {
                let _ = PostMessageW(Some(self.hwnd), windows::Win32::UI::WindowsAndMessaging::WM_CLOSE, WPARAM(0), LPARAM(0));
            },
            _ => {}
        }
        self.after();
    }

    /// Look at Caps Lock a moment from now (Windows updates it just after the key).
    fn check_keys_soon(&mut self, what: u8, ms: u32) {
        self.key_checks |= what;
        unsafe {
            SetTimer(Some(self.hwnd), TIMER_KEYS, ms, None);
        }
    }

    /// The hook takes the volume keys only while the user wants that and the notch is showing
    /// (hidden, it could not show where the volume went, so Windows keeps its own bar).
    fn apply_media_keys(&self) {
        mediakeys::set_enabled(self.media_keys && self.take_volume_keys && !self.hidden);
    }

    /// A volume key was pressed and swallowed: change the volume (in the same 2% steps Windows takes).
    fn media_key(&mut self, vk: u32) {
        use crate::modules::controls::{ControlCommand, ControlsBackend};
        let command = match vk {
            mediakeys::VK_VOLUME_UP => ControlCommand::VolumeStep(2),
            mediakeys::VK_VOLUME_DOWN => ControlCommand::VolumeStep(-2),
            _ => ControlCommand::ToggleMute,
        };
        match &self.winctl {
            Some(c) => c.send(command),
            None => self.effects_log.push(format!("media_key:{vk:#x}")),
        }
    }

    /// Keep the controls panel's picture of Windows current when something changes elsewhere.
    fn note_event(&self, event: &crate::events::SystemEvent) {
        if let Some(c) = &self.winctl {
            c.apply_event(event);
        }
    }

    fn check_keys(&mut self) {
        let checks = std::mem::take(&mut self.key_checks);
        if checks & 1 != 0 {
            let on = sysevents::caps_lock_on();
            if on != self.caps_last {
                self.caps_last = on;
                self.app.system_event(crate::events::SystemEvent::CapsLock(on));
            }
        }
        self.after();
    }

    /// Start listening to Windows for power, Wi-Fi, audio and the two kinds of key.
    fn start_sources(&mut self) {
        self.sources = sysevents::start(self.hwnd);
        self.media_keys = mediakeys::start(self.hwnd);
        self.apply_media_keys();
        // The state now is the baseline: nothing is announced for how things already were.
        if let Some(event) = sysevents::power_event() {
            self.app.system_event(event);
        }
        self.caps_last = sysevents::caps_lock_on();
    }

    /// A system notification, unless the notch is paused.
    fn toast(&mut self, title: &str, body: &str, url: Option<String>) -> bool {
        if self.paused {
            return false;
        }
        self.toast_url = url;
        self.tray.as_ref().is_some_and(|tray| tray.balloon(title, body))
    }

    fn open_toast_url(&mut self) {
        if let Some(url) = self.toast_url.take() {
            open_url(&url);
        }
    }

    /// Explorer restarted: its tray is empty again, so add ours back.
    fn restore_tray(&mut self) {
        self.tray = None;
        let tooltip = self.app.translator().t("tray.show", &[]);
        self.tray = Tray::add(self.hwnd, &tooltip);
    }

    // ----------------------------------------------------------- messages

    fn on_message(&mut self, msg: u32, wparam: WPARAM, lparam: LPARAM) -> Option<LRESULT> {
        let scale = self.scale();
        let key = if msg == WM_TIMER { format!("timer{}", wparam.0) } else { format!("{msg:#x}") };
        *self.wakes.entry(key).or_insert(0) += 1;
        match msg {
            WM_MOUSEACTIVATE => return Some(LRESULT(3)), // MA_NOACTIVATE
            WM_ERASEBKGND => return Some(LRESULT(1)),
            WM_MOUSEMOVE => {
                if !self.tracking {
                    let mut t = TRACKMOUSEEVENT { cbSize: std::mem::size_of::<TRACKMOUSEEVENT>() as u32, dwFlags: TME_LEAVE, hwndTrack: self.hwnd, dwHoverTime: 0 };
                    unsafe {
                        let _ = TrackMouseEvent(&mut t);
                    }
                    self.tracking = true;
                }
                self.app.pointer(PointerKind::Move, lo(lparam.0) as f32 / scale, hi(lparam.0) as f32 / scale);
                self.after();
            }
            WM_MOUSELEAVE => {
                self.tracking = false;
                self.app.pointer(PointerKind::Leave, 0.0, 0.0);
                self.after();
            }
            WM_LBUTTONDOWN => {
                self.app.pointer(PointerKind::Down, lo(lparam.0) as f32 / scale, hi(lparam.0) as f32 / scale);
                if self.app.pointer_captured() {
                    // A slider drag: keep getting the mouse until the button is released.
                    unsafe {
                        SetCapture(self.hwnd);
                    }
                }
                self.after();
            }
            WM_LBUTTONUP => {
                self.app.pointer(PointerKind::Up, lo(lparam.0) as f32 / scale, hi(lparam.0) as f32 / scale);
                unsafe {
                    let _ = ReleaseCapture();
                }
                self.after();
            }
            WM_MOUSEWHEEL => {
                let notches = hi(wparam.0 as isize) as f32 / 120.0;
                // The wheel message carries screen coordinates.
                let x = (lo(lparam.0) - self.origin.0) as f32 / scale;
                let y = (hi(lparam.0) - self.origin.1) as f32 / scale;
                // Over a notch that is not open the wheel is a gesture (volume; with Ctrl, brightness).
                let ctrl = lo(wparam.0 as isize) & 0x0008 != 0;
                if !self.app.quick_wheel(notches, ctrl) {
                    self.app.pointer(PointerKind::Wheel(notches * WHEEL_STEP), x, y);
                }
                self.after();
            }
            // Sideways scroll (a two-finger swipe): the next or the previous page.
            0x020E => {
                self.app.swipe(hi(wparam.0 as isize) as f32 / 120.0);
                self.after();
            }
            // A middle click on the pill: play or pause.
            WM_MBUTTONDOWN => {
                self.app.middle_click();
                self.after();
            }
            // A right click on the notch: the same menu as the tray icon.
            0x0205 => {
                self.context_menu();
            }
            WM_KEYDOWN => {
                let key = match wparam.0 as u32 {
                    0x25 => Some(Key::Left),
                    0x27 => Some(Key::Right),
                    0x09 => Some(Key::Tab),
                    0x26 => Some(Key::Up),
                    0x28 => Some(Key::Down),
                    0x24 => Some(Key::Home),
                    0x23 => Some(Key::End),
                    0x0D => Some(Key::Enter),
                    0x20 => Some(Key::Space),
                    0x1B => Some(Key::Escape),
                    0x08 => Some(Key::Backspace),
                    0x2E => Some(Key::Delete),
                    // Ctrl+V pastes (the clipboard's text goes to the page being typed into).
                    0x56 if unsafe { windows::Win32::UI::Input::KeyboardAndMouse::GetKeyState(0x11) } < 0 => {
                        if let Some(text) = super::clipboard::read_text() {
                            self.app.paste(&text);
                            self.after();
                        }
                        None
                    }
                    _ => None,
                };
                if let Some(k) = key {
                    self.app.key(k);
                    self.after();
                }
            }
            WM_CHAR => {
                if let Some(c) = char::from_u32(wparam.0 as u32).filter(|c| !c.is_control() && *c != ' ') {
                    self.app.key(Key::Char(c));
                    self.after();
                }
            }
            WM_ACTIVATE => {
                if (wparam.0 & 0xffff) == 0 && self.kb_active {
                    self.kb_active = false;
                    focus::leave(self.hwnd, false);
                    self.app.focus_lost();
                    self.after();
                }
            }
            WM_TIMER => match wparam.0 {
                TIMER_WAKE => {
                    unsafe {
                        let _ = KillTimer(Some(self.hwnd), TIMER_WAKE);
                    }
                    self.app.tick();
                    self.after();
                }
                TIMER_QUIT => unsafe { PostQuitMessage(0) },
                TIMER_POLL => self.refresh_fullscreen(),
                TIMER_KEYS => {
                    unsafe {
                        let _ = KillTimer(Some(self.hwnd), TIMER_KEYS);
                    }
                    self.check_keys();
                }
                TIMER_A11Y => {
                    unsafe {
                        let _ = KillTimer(Some(self.hwnd), TIMER_A11Y);
                    }
                    self.publish_a11y();
                }
                _ => {}
            },
            WM_HOTKEY if wparam.0 as i32 == HOTKEY_VISIBILITY_ID => self.toggle_visibility(),
            WM_HOTKEY => {
                if !self.hidden {
                    self.app.hotkey();
                    self.after();
                }
            }
            WM_MEDIAKEY => self.media_key(wparam.0 as u32),
            WM_FOREGROUND => {
                self.refresh_fullscreen();
            }
            uia::WM_A11Y_INVOKE => {
                self.app.accessible_invoke(wparam.0 as i32);
                self.after();
            }
            WM_DATA => {
                self.app.data_changed();
                self.after();
            }
            super::dropper::WM_DRAG => {
                self.app.drag_files(wparam.0 == 1);
                self.after();
            }
            super::dropper::WM_DROPPED => {
                let paths = unsafe { Box::from_raw(lparam.0 as *mut Vec<String>) };
                self.app.drop_files(&paths);
                self.after();
            }
            WM_POWERBROADCAST if wparam.0 == sysevents::PBT_POWERSETTINGCHANGE => {
                if let Some(event) = sysevents::power_setting_event(lparam) {
                    self.note_event(&event);
                    self.app.system_event(event);
                    self.after();
                }
            }
            WM_SYSEVENT => {
                let event = unsafe { Box::from_raw(lparam.0 as *mut crate::events::SystemEvent) };
                self.note_event(&event);
                self.app.system_event(*event);
                self.after();
            }
            WM_INPUT => {
                // Raw keyboard input: only Caps Lock matters; every other key is dropped here.
                match sysevents::key_hint(lparam) {
                    KeyHint::Caps => self.check_keys_soon(1, 40),
                    KeyHint::Other => {}
                }
                return None; // let Windows finish its own handling of the raw input
            }
            WM_POWERBROADCAST if wparam.0 == PBT_RESUME || wparam.0 == PBT_RESUME_SUSPEND => {
                // Back from sleep: timers did not run, and the clock jumped.
                self.app.tick();
                self.after();
            }
            WM_TIMECHANGE => {
                self.app.tick();
                self.after();
            }
            m if m == self.taskbar_created => self.restore_tray(),
            WM_SHOW_ME => {
                self.paused = false;
                self.update_hidden();
                self.app.peek();
                self.after();
            }
            WM_DPICHANGED => {
                if self.scale_override.is_none() {
                    self.set_scale((wparam.0 & 0xffff) as f32 / 96.0);
                    self.after();
                }
            }
            WM_DISPLAYCHANGE => {
                self.place();
                self.after();
            }
            WM_SETTINGCHANGE => {
                self.app.set_reduced_motion(!animations_enabled());
                self.app.set_windows_light_theme(apps_use_light_theme());
                self.after();
            }
            WM_CONTROL => {
                let request = unsafe { Box::from_raw(lparam.0 as *mut Request) };
                let answer = self.command(&request.line);
                let _ = request.reply.send(answer);
                self.after();
                return Some(LRESULT(0));
            }
            WM_IPC => {
                let request = unsafe { Box::from_raw(lparam.0 as *mut Request) };
                let answer = self.ipc_command(&request.line);
                let _ = request.reply.send(answer);
                self.after();
                return Some(LRESULT(0));
            }
            WM_DESTROY => {
                if let Some(path) = self.dump_on_exit.clone() {
                    let _ = self.app.dump_png(&path);
                }
                unsafe { PostQuitMessage(0) };
            }
            _ => return None,
        }
        Some(LRESULT(0))
    }
}

// ------------------------------------------------------------------ the loop

unsafe extern "system" fn wndproc(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    use windows::Win32::UI::WindowsAndMessaging::WM_CLOSE;
    if msg == WM_CLOSE {
        let _ = DestroyWindow(hwnd);
        return LRESULT(0);
    }
    if msg == uia::WM_GETOBJECT && lparam.0 as i32 == -25 {
        // First question from an assistive tool: have a tree ready, then answer.
        uia::set_active();
        with_rt(|rt| rt.publish_a11y());
        if let Some(answer) = uia::on_get_object(hwnd, wparam, lparam) {
            return answer;
        }
    }
    if msg == WM_TRAY {
        // The menu runs its own message loop, so it must not hold the runtime.
        match (lparam.0 & 0xffff) as u32 {
            0x0205 | 0x007B => {
                if let Some((labels, paused)) = with_rt(|rt| (rt.menu_labels(), rt.paused)) {
                    let cmd = tray::show_menu(hwnd, &labels, paused);
                    with_rt(|rt| rt.tray_command(cmd));
                }
            }
            NIN_BALLOONUSERCLICK => {
                with_rt(|rt| rt.open_toast_url());
            }
            0x0202 => {
                with_rt(|rt| {
                    rt.paused = false;
                    rt.update_hidden();
                    rt.app.peek();
                    rt.after();
                });
            }
            _ => {}
        }
        return LRESULT(0);
    }
    with_rt(|rt| rt.on_message(msg, wparam, lparam)).flatten().unwrap_or_else(|| DefWindowProcW(hwnd, msg, wparam, lparam))
}

unsafe extern "system" fn on_foreground(_hook: HWINEVENTHOOK, _event: u32, _hwnd: HWND, _object: i32, _child: i32, _thread: u32, _time: u32) {
    let target = HOOK_TARGET.load(Ordering::Relaxed);
    if target != 0 {
        let _ = PostMessageW(Some(HWND(target as *mut c_void)), WM_FOREGROUND, WPARAM(0), LPARAM(0));
    }
}

/// The user's feeds: Credential Manager for the links, worker threads for the fetching.
/// Tests can seed links with BLINK_TEST_FEEDS="id=url;id=url" instead (control mode only).
fn live_source(opts: &Options, store: &Store, settings: &Settings, tz: TimeZone, hwnd: Arc<AtomicIsize>) -> Result<LiveSource, String> {
    use blink_core::feed::FeedService;
    use blink_core::secret::{CredentialManager, MemorySecrets, Secrets};

    let dir = store.dir().to_path_buf();
    let fetcher: Arc<dyn crate::live::FeedFetcher> = match std::env::var("BLINK_TEST_FEEDS").ok().filter(|_| opts.control.is_some()) {
        Some(list) => {
            let secrets = MemorySecrets::default();
            for pair in list.split(';').filter(|p| !p.is_empty()) {
                if let Some((id, url)) = pair.split_once('=') {
                    let _ = secrets.set(id, url);
                }
            }
            Arc::new(FeedService::new(Store::new(&dir), secrets))
        }
        None => Arc::new(FeedService::new(Store::new(&dir), CredentialManager)),
    };
    let wake: Arc<dyn Fn() + Send + Sync> = Arc::new(move || {
        let h = hwnd.load(Ordering::Relaxed);
        if h != 0 {
            unsafe {
                let _ = PostMessageW(Some(HWND(h as *mut c_void)), WM_DATA, WPARAM(0), LPARAM(0));
            }
        }
    });
    Ok(LiveSource::new(Store::new(&dir), fetcher, settings, tz, wake, true))
}

fn create_window(width: i32, height: i32, x: i32, y: i32) -> Result<HWND, String> {
    unsafe {
        let instance = GetModuleHandleW(None).map_err(|e| e.to_string())?;
        let class = w!("BlinkNotch");
        let wc = WNDCLASSEXW {
            cbSize: std::mem::size_of::<WNDCLASSEXW>() as u32,
            style: CS_HREDRAW | CS_VREDRAW,
            lpfnWndProc: Some(wndproc),
            hInstance: instance.into(),
            hCursor: LoadCursorW(None, IDC_ARROW).unwrap_or_default(),
            lpszClassName: class,
            ..Default::default()
        };
        if RegisterClassExW(&wc) == 0 {
            return Err("RegisterClassExW failed".into());
        }
        CreateWindowExW(
            WS_EX_LAYERED | WS_EX_TOPMOST | WS_EX_TOOLWINDOW | WS_EX_NOACTIVATE,
            class,
            w!("Blink"),
            WS_POPUP,
            x,
            y,
            width,
            height,
            None,
            None,
            Some(instance.into()),
            None,
        )
        .map_err(|e| format!("CreateWindowExW: {e}"))
    }
}

pub fn run(opts: Options) -> Result<(), String> {
    // One notch per user (and per control-pipe name, so tests can run side by side).
    // Scripted runs each have their own name, so they never meet the notch someone is using; BLINK_INSTANCE names one by hand.
    let instance_tag = std::env::var("BLINK_INSTANCE").ok().filter(|t| !t.is_empty()).unwrap_or_else(|| opts.control.clone().unwrap_or_else(|| "main".to_string()));
    let Some(_instance) = system::single_instance(&instance_tag) else {
        return Ok(());
    };
    unsafe {
        let _ = SetProcessDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2);
    }
    let store = Store::new(opts.data_dir.clone().or_else(default_dir).ok_or("no data directory")?);
    let settings = normalize_settings(store.get_json("settings").ok().flatten().as_ref());

    let list = monitors();
    let first = list.get(settings.monitor as usize).or_else(|| list.iter().find(|m| m.primary)).or_else(|| list.first()).copied();
    let scale = opts.scale.or_else(|| first.as_ref().map(monitor_scale)).unwrap_or(1.0);

    let tz = std::env::var("BLINK_TZ").ok().and_then(|n| TimeZone::get(&n).ok()).unwrap_or_else(TimeZone::system);
    let clock = if opts.manual_clock { Clock::manual(MANUAL_WALL_BASE) } else { Clock::real() };
    let target = Arc::new(AtomicIsize::new(0));
    let source: Box<dyn DeadlineSource> = if opts.demo {
        Box::new(DemoSource::new(clock.wall_ms()))
    } else {
        Box::new(live_source(&opts, &store, &settings, tz.clone(), target.clone())?)
    };
    // The real controls talk to Windows; scripted runs get a stand-in that only remembers.
    let real_controls = !opts.control.is_some() || std::env::var_os("BLINK_REAL_SOURCES").is_some();
    let winctl = real_controls.then(super::winctl::WinControls::start);
    let fake_controls = winctl.is_none().then(FakeControls::new);
    let controls_backend: std::sync::Arc<dyn crate::modules::controls::ControlsBackend> = match (&winctl, &fake_controls) {
        (Some(real), _) => real.clone(),
        (None, Some(fake)) => fake.clone(),
        _ => unreachable!("one of the two exists"),
    };
    // What is playing comes from Windows, and lyrics from the internet; scripted runs get stand-ins.
    let media_wake: std::sync::Arc<dyn Fn() + Send + Sync> = {
        let target = target.clone();
        std::sync::Arc::new(move || {
            let h = target.load(Ordering::Relaxed);
            if h != 0 {
                unsafe {
                    let _ = PostMessageW(Some(HWND(h as *mut c_void)), WM_DATA, WPARAM(0), LPARAM(0));
                }
            }
        })
    };
    let real_media = !opts.control.is_some() || std::env::var_os("BLINK_REAL_SOURCES").is_some();
    // The stand-in starts with nothing playing (a playing song keeps the bars moving, which scripted runs do not want
    // unless they ask: the `media` command starts one).
    let fake_media = (!real_media).then(|| {
        let fake = FakeMedia::new(clock.wall_ms());
        fake.set(|s| s.active = false);
        fake
    });
    let (media_backend, lyrics_source): (std::sync::Arc<dyn MediaBackend>, std::sync::Arc<dyn blink_core::lyrics::LyricsSource>) = match &fake_media {
        Some(fake) => (fake.clone(), std::sync::Arc::new(FakeLyrics)),
        None => (super::media::WinMedia::start(media_wake.clone()), std::sync::Arc::new(blink_core::lyrics::LrcLib::new())),
    };
    // The weather comes from the internet; scripted runs get a stand-in that always says the same.
    let weather_source: std::sync::Arc<dyn blink_core::weather::WeatherSource> = if real_media {
        std::sync::Arc::new(blink_core::weather::OpenMeteo::new())
    } else {
        crate::modules::clock::FakeWeather::new()
    };
    let modules: Vec<Box<dyn Module>> = vec![
        // The pages in the order they turn: time and weather/calendar, then deadlines, then the controls, then music and projects.
        Box::new(ClockModule::new().with_weather(weather_source, media_wake.clone())),
        Box::new(DeadlinesModule::new(source)),
        Box::new(ControlsModule::new(controls_backend)),
        Box::new(MusicModule::new(media_backend, lyrics_source, media_wake)),
        Box::new(ProjectsModule::new(Store::new(store.dir()), clock.wall_ms())),
        Box::new(crate::modules::agents::AgentsModule::new()),
        Box::new(SystemModule::new()),
    ];
    let mut app = App::new(clock, settings, tz, scale, &system_language(), modules)?;
    app.configure_modules(&crate::host::parse_modules(store.get_json("host").ok().flatten().as_ref()));
    app.set_look(crate::look::parse(store.get_json("host").ok().flatten().as_ref()));
    app.set_reduced_motion(!animations_enabled());
    app.set_windows_light_theme(apps_use_light_theme());
    if opts.pin_open {
        app.pin_open();
    }

    let (pw, ph) = ((WINDOW_W * scale).round() as i32, (WINDOW_H * scale).round() as i32);
    let hwnd = create_window(pw, ph, 0, 0)?;
    super::dropper::register(hwnd);
    target.store(hwnd.0 as isize, Ordering::Relaxed);
    let surface = Surface::new(pw as u32, ph as u32)?;
    let control = opts.control.is_some();
    RT.with(|cell| {
        *cell.borrow_mut() = Some(Runtime {
            hwnd,
            app,
            store,
            surface,
            origin: (0, 0),
            tracking: false,
            kb_active: false,
            dirty: true,
            prev_rows: ph as usize,
            scale_override: opts.scale,
            frames: 0,
            control,
            sim_focus: true,
            effects_log: Vec::new(),
            dump_on_exit: opts.dump.clone(),
            paused: false,
            fullscreen: false,
            // Scripted runs must not depend on what the machine is doing; they ask for fullscreen explicitly.
            fullscreen_sim: control.then_some(system::FullscreenKind::None),
            desktop_sim: control.then_some(false),
            fullscreen_mode: crate::host::FullscreenMode::All,
            hidden: false,
            tray: None,
            toast_url: None,
            taskbar_created: unsafe { RegisterWindowMessageW(w!("TaskbarCreated")) },
            hotkey: String::new(),
            vis_hotkey: String::new(),
            fullscreen_override: false,
            frame_times: Vec::new(),
            cost: (0.0, 0.0, 0.0, 0),
            wakes: std::collections::BTreeMap::new(),
            last_frame: None,
            last_a11y: Instant::now(),
            sources: Sources::default(),
            take_volume_keys: true,
            media_keys: false,
            winctl,
            fake_controls,
            fake_media,
            caps_last: false,
            key_checks: 0,
        });
    });
    // Show first: the checks below may hide it again (a fullscreen app is already in front).
    unsafe {
        let _ = ShowWindow(hwnd, SW_SHOWNOACTIVATE);
    }
    HOOK_TARGET.store(hwnd.0 as isize, Ordering::Relaxed);
    let _hook = unsafe { SetWinEventHook(EVENT_SYSTEM_FOREGROUND, EVENT_SYSTEM_FOREGROUND, None, Some(on_foreground), 0, 0, WINEVENT_OUTOFCONTEXT) };
    with_rt(|rt| {
        rt.place();
        rt.present();
        rt.reschedule();
        let tooltip = rt.app.translator().t("tray.show", &[]);
        rt.tray = Tray::add(hwnd, &tooltip);
        let accelerator = rt.app.settings().hotkey.clone();
        let status = rt.register_hotkey(&accelerator);
        if status != "ok" {
            rt.effects_log.push(format!("hotkey:{status}"));
        }
        let file = rt.store.get_json("host").ok().flatten();
        rt.fullscreen_mode = crate::host::parse_fullscreen_mode(file.as_ref());
        rt.take_volume_keys = crate::host::parse_take_over_volume_keys(file.as_ref());
        let status = rt.register_visibility_hotkey(&crate::host::parse_visibility_hotkey(file.as_ref()));
        if status != "ok" {
            rt.effects_log.push(format!("hotkey:visibility:{status}"));
        }
        rt.refresh_fullscreen();
        // Scripted runs inject their own events; real sources would add real ones to the test.
        if !control || std::env::var_os("BLINK_REAL_SOURCES").is_some() {
            rt.start_sources();
        }
        let _ = &rt.store;
    });
    unsafe { SetTimer(Some(hwnd), TIMER_POLL, POLL_MS, None) };
    if let Some(name) = &opts.control {
        control::start(name, hwnd);
    }
    control::start_at(&blink_core::ipc::pipe_path(), hwnd, WM_IPC);
    if !control {
        blink_core::autostart::migrate_legacy();
        // First run: no feeds and never set up. Open the settings program, which shows its wizard.
        let first_run = with_rt(|rt| !rt.app.settings().onboarded && rt.app.settings().feeds.is_empty()).unwrap_or(false);
        if first_run {
            with_rt(|rt| rt.open_settings("feeds"));
        }
        // The very first start also gets the short tour in the notch itself (once; not in scripted runs).
        with_rt(|rt| {
            if !rt.control && !rt.app.settings().onboarded && rt.store.read("tour.json").is_none() {
                rt.app.start_tour();
            }
        });
    }
    if let Some(seconds) = opts.seconds {
        unsafe { SetTimer(Some(hwnd), TIMER_QUIT, (seconds * 1000).min(u64::from(u32::MAX)) as u32, None) };
    }

    let mut msg = MSG::default();
    loop {
        let animating = with_rt(|rt| rt.app.wants_frames()).unwrap_or(false);
        if animating {
            unsafe {
                while PeekMessageW(&mut msg, None, 0, 0, PM_REMOVE).as_bool() {
                    if msg.message == WM_QUIT {
                        return Ok(());
                    }
                    let _ = TranslateMessage(&msg);
                    DispatchMessageW(&msg);
                }
                // The notch opening and closing is drawn on every refresh. Anything else that moves (the lyrics gliding to
                // the next line, a slider being dragged) is drawn at about 70 frames a second at most: on a 144 Hz screen that
                // is half the work, and nobody sees the difference.
                let wait = with_rt(|rt| !rt.app.shell().is_animating(rt.app.now()) && rt.last_frame.is_some_and(|t| t.elapsed().as_secs_f32() < 0.012)).unwrap_or(false);
                if !wait {
                    with_rt(|rt| rt.frame());
                }
                let _ = DwmFlush();
            }
        } else {
            unsafe {
                let r = GetMessageW(&mut msg, None, 0, 0);
                if r.0 <= 0 {
                    return Ok(());
                }
                let _ = TranslateMessage(&msg);
                DispatchMessageW(&msg);
                // Draw once the queue is empty, so a burst of mouse moves costs one frame.
                if GetQueueStatus(QS_ALLINPUT) >> 16 == 0 {
                    with_rt(|rt| rt.flush());
                }
            }
        }
    }
}

#[allow(dead_code)]
fn pause(ms: u64) {
    std::thread::sleep(Duration::from_millis(ms));
}
