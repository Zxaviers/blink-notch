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
use windows::Win32::UI::Input::KeyboardAndMouse::{RegisterHotKey, TrackMouseEvent, UnregisterHotKey, HOT_KEY_MODIFIERS, TME_LEAVE, TRACKMOUSEEVENT};
use windows::Win32::UI::Shell::ShellExecuteW;
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, DestroyWindow, DispatchMessageW, GetClassNameW, GetForegroundWindow, GetMessageW, GetQueueStatus,
    GetWindowLongPtrW, GetWindowRect, IsWindowVisible, KillTimer, LoadCursorW, PeekMessageW, PostMessageW, PostQuitMessage,
    RegisterClassExW, SetTimer, ShowWindow, SystemParametersInfoW, TranslateMessage, UpdateLayeredWindow, WindowFromPoint, CS_HREDRAW,
    CS_VREDRAW, GWL_EXSTYLE, IDC_ARROW, MSG, PM_REMOVE, QS_ALLINPUT, SPI_GETCLIENTAREAANIMATION, SW_SHOWNOACTIVATE, SW_SHOWNORMAL,
    ULW_ALPHA, EVENT_SYSTEM_FOREGROUND, SW_HIDE, WINEVENT_OUTOFCONTEXT, WM_ACTIVATE, WM_CHAR, WM_DESTROY, WM_DISPLAYCHANGE, WM_DPICHANGED, WM_ERASEBKGND, WM_HOTKEY, WM_KEYDOWN,
    WM_LBUTTONDOWN, WM_MOUSEACTIVATE, WM_MOUSEMOVE, WM_MOUSEWHEEL, WM_QUIT, WM_SETTINGCHANGE, WM_TIMER, WNDCLASSEXW,
    WS_EX_LAYERED, WS_EX_NOACTIVATE, WS_EX_TOOLWINDOW, WS_EX_TOPMOST, WS_POPUP,
};

use super::control::{self, Request, WM_CONTROL};
use super::focus;
use super::system::{self, WM_SHOW_ME};
use super::tray::{self, MenuLabels, Tray, WM_TRAY};
use crate::app::{App, AppEffect};
use crate::clock::Clock;
use crate::modules::deadlines::{DeadlinesModule, DemoSource};
use crate::modules::{Key, Module, PointerKind};
use crate::shell::{WINDOW_H, WINDOW_W};

/// Wall clock a manual-clock run starts at (2026-10-22 12:00 UTC), so tests are repeatable.
const MANUAL_WALL_BASE: i64 = 1_792_670_400_000;
const TIMER_WAKE: usize = 1;
const TIMER_QUIT: usize = 2;
const TIMER_POLL: usize = 3;
/// Slow safety net for the fullscreen check (the foreground hook is the fast path).
const POLL_MS: u32 = 3000;
/// Posted by the foreground hook.
const WM_FOREGROUND: u32 = 0x8000 + 4;
const HOTKEY_ID: i32 = 1;
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
    fullscreen_sim: Option<bool>,
    hidden: bool,
    tray: Option<Tray>,
    hotkey: String,
    /// Time between consecutive animation frames (ms), for the pacing check.
    frame_times: Vec<f32>,
    last_frame: Option<Instant>,
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
        self.app.render();
        let rows = self.app.visible_rows();
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
        let origin = POINT { x: self.origin.0, y: self.origin.1 };
        let size = SIZE { cx: self.surface.w as i32, cy: self.surface.h as i32 };
        let source = POINT::default();
        let blend = BLENDFUNCTION { BlendOp: AC_SRC_OVER as u8, BlendFlags: 0, SourceConstantAlpha: 255, AlphaFormat: AC_SRC_ALPHA as u8 };
        unsafe {
            let _ = UpdateLayeredWindow(self.hwnd, None, Some(&origin), Some(&size), Some(self.surface.dc), Some(&source), COLORREF(0), Some(&blend), ULW_ALPHA);
        }
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
                AppEffect::OpenSettings => self.open_settings(),
                AppEffect::RefreshFeeds => self.effects_log.push("refresh_feeds".into()),
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
        let busy = self.fullscreen_sim.unwrap_or_else(system::fullscreen_app_active);
        if busy != self.fullscreen {
            self.fullscreen = busy;
            self.update_hidden();
        }
    }

    /// Hide while paused or while a fullscreen app is in front.
    fn update_hidden(&mut self) {
        let want = self.paused || self.fullscreen;
        if want == self.hidden {
            return;
        }
        self.hidden = want;
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
            tray::CMD_REFRESH => self.effects_log.push("refresh_feeds".into()),
            tray::CMD_SETTINGS => self.open_settings(),
            tray::CMD_QUIT => unsafe {
                let _ = PostMessageW(Some(self.hwnd), windows::Win32::UI::WindowsAndMessaging::WM_CLOSE, WPARAM(0), LPARAM(0));
            },
            _ => {}
        }
        self.after();
    }

    /// The settings program is a separate executable next to this one (Phase 4).
    fn open_settings(&mut self) {
        if self.control {
            self.effects_log.push("open_settings".into());
            return;
        }
        if let Some(path) = std::env::current_exe().ok().map(|p| p.with_file_name("blink-settings.exe")).filter(|p| p.exists()) {
            let _ = std::process::Command::new(path).spawn();
        }
    }

    // ----------------------------------------------------------- messages

    fn on_message(&mut self, msg: u32, wparam: WPARAM, lparam: LPARAM) -> Option<LRESULT> {
        let scale = self.scale();
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
                self.after();
            }
            WM_MOUSEWHEEL => {
                let notches = hi(wparam.0 as isize) as f32 / 120.0;
                // The wheel message carries screen coordinates.
                let x = (lo(lparam.0) - self.origin.0) as f32 / scale;
                let y = (hi(lparam.0) - self.origin.1) as f32 / scale;
                self.app.pointer(PointerKind::Wheel(notches * WHEEL_STEP), x, y);
                self.after();
            }
            WM_KEYDOWN => {
                let key = match wparam.0 as u32 {
                    0x26 => Some(Key::Up),
                    0x28 => Some(Key::Down),
                    0x24 => Some(Key::Home),
                    0x23 => Some(Key::End),
                    0x0D => Some(Key::Enter),
                    0x20 => Some(Key::Space),
                    0x1B => Some(Key::Escape),
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
                _ => {}
            },
            WM_HOTKEY => {
                if !self.hidden {
                    self.app.hotkey();
                    self.after();
                }
            }
            WM_FOREGROUND => self.refresh_fullscreen(),
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

    // ------------------------------------------------------------ control

    fn window_info(&self) -> Value {
        let mut rect = RECT::default();
        unsafe {
            let _ = GetWindowRect(self.hwnd, &mut rect);
        }
        let ex = unsafe { GetWindowLongPtrW(self.hwnd, GWL_EXSTYLE) } as u32;
        let foreground = unsafe { GetForegroundWindow() };
        json!({
            "rect": { "x": rect.left, "y": rect.top, "w": rect.right - rect.left, "h": rect.bottom - rect.top },
            "visible": unsafe { IsWindowVisible(self.hwnd) }.as_bool(),
            "layered": ex & WS_EX_LAYERED.0 != 0,
            "topmost": ex & WS_EX_TOPMOST.0 != 0,
            "toolWindow": ex & WS_EX_TOOLWINDOW.0 != 0,
            "noActivate": ex & WS_EX_NOACTIVATE.0 != 0,
            "foreground": foreground == self.hwnd,
            "keyboardActive": self.kb_active,
            "frames": self.frames,
            "hidden": self.hidden,
            "paused": self.paused,
            "fullscreen": self.fullscreen,
            "hotkey": self.hotkey,
            "tray": self.tray.is_some(),
            "surface": { "w": self.surface.w, "h": self.surface.h },
        })
    }

    /// The pixel Windows would hit-test: alpha of the last presented frame, and
    /// which window `WindowFromPoint` reports there (no cursor movement needed).
    fn hit(&mut self, x: f32, y: f32) -> Value {
        let s = self.scale();
        let (px, py) = ((x * s).round() as i32, (y * s).round() as i32);
        let inside = px >= 0 && py >= 0 && (px as u32) < self.surface.w && (py as u32) < self.surface.h;
        let width = self.surface.w as usize;
        let alpha = if inside { self.surface.pixels()[py as usize * width + px as usize] >> 24 } else { 0 };
        let at = POINT { x: self.origin.0 + px, y: self.origin.1 + py };
        let found = unsafe { WindowFromPoint(at) };
        let mut class = [0u16; 64];
        let n = if found.0.is_null() { 0 } else { unsafe { GetClassNameW(found, &mut class) } };
        json!({
            "alpha": alpha,
            "ours": found == self.hwnd,
            "windowClass": String::from_utf16_lossy(&class[..n.max(0) as usize]),
        })
    }

    fn advance(&mut self, ms: u64) {
        let target = self.app.now() + ms;
        while self.app.now() < target {
            let now = self.app.now();
            let step = if self.app.wants_frames() {
                8
            } else {
                self.app.next_wake().map_or(target - now, |w| w.saturating_sub(now).max(1))
            };
            let t = (now + step).min(target);
            self.app.clock.set_manual_ms(t);
            self.app.tick();
            self.process_effects();
        }
    }

    fn command(&mut self, line: &str) -> String {
        let reply = match serde_json::from_str::<Value>(line) {
            Ok(v) => self.run_command(&v),
            Err(e) => Err(format!("bad json: {e}")),
        };
        match reply {
            Ok(mut v) => {
                if let Some(o) = v.as_object_mut() {
                    o.insert("ok".into(), Value::Bool(true));
                }
                v.to_string()
            }
            Err(e) => json!({ "ok": false, "error": e }).to_string(),
        }
    }

    fn run_command(&mut self, v: &Value) -> Result<Value, String> {
        let cmd = v["cmd"].as_str().ok_or("missing cmd")?;
        let f = |name: &str| v[name].as_f64().unwrap_or(0.0) as f32;
        match cmd {
            "state" => {
                let state = self.app.state_json();
                return Ok(json!({ "state": state, "window": self.window_info() }));
            }
            "mouse" => {
                let kind = match v["kind"].as_str().unwrap_or("move") {
                    "move" => PointerKind::Move,
                    "leave" => PointerKind::Leave,
                    "down" => PointerKind::Down,
                    "wheel" => PointerKind::Wheel(f("delta")),
                    other => return Err(format!("unknown mouse kind {other}")),
                };
                self.app.pointer(kind, f("x"), f("y"));
            }
            "click" => {
                self.app.pointer(PointerKind::Move, f("x"), f("y"));
                self.app.pointer(PointerKind::Down, f("x"), f("y"));
            }
            "key" => {
                let key = match v["key"].as_str().unwrap_or("") {
                    "up" => Key::Up,
                    "down" => Key::Down,
                    "home" => Key::Home,
                    "end" => Key::End,
                    "enter" => Key::Enter,
                    "space" => Key::Space,
                    "escape" => Key::Escape,
                    "char" => Key::Char(v["ch"].as_str().and_then(|s| s.chars().next()).ok_or("char needs ch")?),
                    other => return Err(format!("unknown key {other}")),
                };
                self.app.key(key);
            }
            "hotkey" => self.app.hotkey(),
            "focus_lost" => self.app.focus_lost(),
            "focus_mode" => self.sim_focus = !v["real"].as_bool().unwrap_or(false),
            "peek" => self.app.peek(),
            "advance" => {
                if !self.app.clock.is_manual() {
                    return Err("NOT_MANUAL".into());
                }
                self.advance(v["ms"].as_u64().unwrap_or(0));
            }
            "settings" => {
                let settings: Settings = normalize_settings(Some(&v["patch"]));
                let lang = system_language();
                let accelerator = settings.hotkey.clone();
                self.app.apply_settings(settings, &lang);
                self.place();
                let status = self.register_hotkey(&accelerator);
                self.process_effects();
                self.present();
                return Ok(json!({ "hotkey": status }));
            }
            "register_hotkey" => {
                let status = self.register_hotkey(v["accelerator"].as_str().unwrap_or(""));
                return Ok(json!({ "hotkey": status }));
            }
            "pause" => {
                self.paused = v["on"].as_bool().unwrap_or(true);
                self.update_hidden();
            }
            "fullscreen" => {
                // Pretend a fullscreen app is (or is not) in front; null goes back to asking Windows.
                self.fullscreen_sim = v["on"].as_bool();
                self.refresh_fullscreen();
            }
            "frames" => {
                let times = std::mem::take(&mut self.frame_times);
                let n = times.len();
                let (min, max) = times.iter().fold((f32::MAX, 0.0f32), |(a, b), t| (a.min(*t), b.max(*t)));
                let avg = if n > 0 { times.iter().sum::<f32>() / n as f32 } else { 0.0 };
                let late = times.iter().filter(|t| **t > 20.0).count();
                let mut sorted = times.clone();
                sorted.sort_by(|a, b| a.total_cmp(b));
                let pct = |q: f32| sorted.get(((n as f32 - 1.0) * q).round() as usize).copied().unwrap_or(0.0);
                return Ok(json!({ "frames": n, "minMs": if n > 0 { min } else { 0.0 }, "maxMs": max, "avgMs": avg, "p50Ms": pct(0.5), "p95Ms": pct(0.95), "over20ms": late }));
            }
            "scale" => {
                self.scale_override = Some(f("value"));
                self.set_scale(f("value").clamp(1.0, 4.0));
            }
            "reduced_motion" => self.app.set_reduced_motion(v["on"].as_bool().unwrap_or(true)),
            "light" => self.app.set_windows_light_theme(v["on"].as_bool().unwrap_or(true)),
            "dump" => {
                let path = v["path"].as_str().ok_or("dump needs path")?;
                self.app.dump_png(path)?;
            }
            "hit" => {
                self.present();
                return Ok(self.hit(f("x"), f("y")));
            }
            "effects" => {
                let log = std::mem::take(&mut self.effects_log);
                return Ok(json!({ "effects": log }));
            }
            "quit" => {
                unsafe {
                    let _ = PostMessageW(Some(self.hwnd), windows::Win32::UI::WindowsAndMessaging::WM_CLOSE, WPARAM(0), LPARAM(0));
                }
            }
            other => return Err(format!("unknown command {other}")),
        }
        self.process_effects();
        self.present();
        Ok(json!({ "effects": std::mem::take(&mut self.effects_log) }))
    }
}

// ------------------------------------------------------------------ the loop

unsafe extern "system" fn wndproc(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    use windows::Win32::UI::WindowsAndMessaging::WM_CLOSE;
    if msg == WM_CLOSE {
        let _ = DestroyWindow(hwnd);
        return LRESULT(0);
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
    let Some(_instance) = system::single_instance(opts.control.as_deref().unwrap_or("main")) else {
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
    let source = DemoSource::new(clock.wall_ms());
    let modules: Vec<Box<dyn Module>> = vec![Box::new(DeadlinesModule::new(Box::new(source)))];
    let mut app = App::new(clock, settings, tz, scale, &system_language(), modules)?;
    app.set_reduced_motion(!animations_enabled());
    app.set_windows_light_theme(apps_use_light_theme());
    if opts.pin_open {
        app.pin_open();
    }

    let (pw, ph) = ((WINDOW_W * scale).round() as i32, (WINDOW_H * scale).round() as i32);
    let hwnd = create_window(pw, ph, 0, 0)?;
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
            fullscreen_sim: None,
            hidden: false,
            tray: None,
            hotkey: String::new(),
            frame_times: Vec::new(),
            last_frame: None,
        });
    });
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
        rt.refresh_fullscreen();
        let _ = &rt.store;
    });
    unsafe { SetTimer(Some(hwnd), TIMER_POLL, POLL_MS, None) };
    unsafe {
        let _ = ShowWindow(hwnd, SW_SHOWNOACTIVATE);
    }
    if let Some(name) = &opts.control {
        control::start(name, hwnd);
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
                with_rt(|rt| rt.frame());
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
