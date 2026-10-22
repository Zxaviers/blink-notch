//! Deadline Notch.
//!
//! One fixed-size transparent window at top center. The page draws and
//! animates the notch shape in CSS and reports its bounds via `set_shape`.
//! A cursor loop keeps the window click-through except over that shape and
//! tells the page when the cursor enters or leaves it.
//!
//! Everything with side effects lives here: network (feed.rs), secrets
//! (secret.rs), files (store.rs), toasts, tray, placement and the settings
//! window. The page itself has no network access.

mod feed;
mod secret;
mod store;
mod win32;

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use serde::Serialize;
use tauri::menu::{CheckMenuItem, Menu, MenuItem, PredefinedMenuItem};
use tauri::tray::TrayIconBuilder;
use tauri::{
    AppHandle, Emitter, LogicalSize, Manager, PhysicalPosition, WebviewUrl, WebviewWindow,
    WebviewWindowBuilder, Wry,
};
use tauri_plugin_notification::NotificationExt;
use tauri_plugin_opener::OpenerExt;

/// WebView2 flags. `--disable-gpu` (software rendering) removes most of the
/// GPU process's memory: about 98 MB down to 66 MB at rest, measured with
/// scripts/mem-probe.ps1. The page animates a clip-path so it stays cheap
/// without the GPU. Must match "additionalBrowserArgs" in tauri.conf.json.
#[cfg(windows)]
const WEBVIEW_ARGS: &str =
    "--disable-gpu --disable-features=msWebOOUI,msPdfOOUI,msSmartScreenProtection";

/// Margin (logical px) around the shape inside which the cursor loop polls fast.
const NEAR_MARGIN: f64 = 24.0;
/// The notch window is fixed in logical px; the shape animates inside it.
const WIN_W: f64 = 640.0;
const WIN_H: f64 = 360.0;

/// Notch shape in logical (CSS) px, relative to the window's top-left.
#[derive(Clone, Copy, Default)]
struct Shape {
    x: f64,
    y: f64,
    w: f64,
    h: f64,
}

impl Shape {
    fn contains(&self, px: f64, py: f64, margin: f64) -> bool {
        px >= self.x - margin
            && px < self.x + self.w + margin
            && py >= self.y - margin
            && py < self.y + self.h + margin
    }
}

#[derive(Clone, Copy, Default)]
struct Placement {
    monitor: usize,
    offset_x: i32,
}

struct TrayItems {
    pause: CheckMenuItem<Wry>,
    refresh: MenuItem<Wry>,
    settings: MenuItem<Wry>,
    quit: MenuItem<Wry>,
}

#[derive(Default)]
struct NotchState {
    shape: Mutex<Shape>,
    placement: Mutex<Placement>,
    tray: Mutex<Option<TrayItems>>,
    /// A fullscreen app or presentation is active.
    fullscreen: AtomicBool,
    /// The user paused the notch from the tray.
    paused: AtomicBool,
    /// Effective: the window is currently hidden (fullscreen or paused).
    hidden: AtomicBool,
}

type SharedState = Arc<NotchState>;

fn dlog(msg: &str) {
    if std::env::var_os("DN_DEBUG").is_some() {
        eprintln!("[notch] {msg}");
    }
}

// ----------------------------------------------------------------- commands

#[tauri::command]
fn set_shape(state: tauri::State<'_, SharedState>, x: f64, y: f64, w: f64, h: f64) {
    *state.shape.lock().unwrap() = Shape { x, y, w, h };
}

#[derive(Serialize)]
struct MonitorInfo {
    index: usize,
    width: u32,
    height: u32,
    primary: bool,
}

#[tauri::command]
fn list_monitors(window: WebviewWindow) -> Vec<MonitorInfo> {
    let primary = window.primary_monitor().ok().flatten();
    window
        .available_monitors()
        .unwrap_or_default()
        .iter()
        .enumerate()
        .map(|(index, m)| MonitorInfo {
            index,
            width: m.size().width,
            height: m.size().height,
            primary: primary.as_ref().is_some_and(|p| p.position() == m.position()),
        })
        .collect()
}

#[tauri::command]
fn set_placement(
    app: AppHandle,
    state: tauri::State<'_, SharedState>,
    monitor: usize,
    offset_x: i32,
) {
    let placement = Placement { monitor, offset_x };
    *state.placement.lock().unwrap() = placement;
    if let Some(win) = app.get_webview_window("notch") {
        place(&win, placement);
    }
}

#[tauri::command]
fn set_tray_labels(
    state: tauri::State<'_, SharedState>,
    pause: String,
    refresh: String,
    settings: String,
    quit: String,
) {
    if let Some(items) = state.tray.lock().unwrap().as_ref() {
        let _ = items.pause.set_text(pause);
        let _ = items.refresh.set_text(refresh);
        let _ = items.settings.set_text(settings);
        let _ = items.quit.set_text(quit);
    }
}

/// Lets the page add a line to the debug log (DN_DEBUG=1); otherwise a no-op.
#[tauri::command]
fn debug_log(msg: String) {
    dlog(&msg);
}

#[tauri::command]
fn notify(app: AppHandle, title: String, body: String) {
    let _ = app.notification().builder().title(title).body(body).show();
}

/// Open a task page in the default browser. https only.
#[tauri::command]
fn open_url(app: AppHandle, url: String) -> Result<(), String> {
    if !url.to_ascii_lowercase().starts_with("https://") {
        return Err("URL_PROTOCOL".to_string());
    }
    app.opener()
        .open_url(url, None::<&str>)
        .map_err(|_| "OPEN".to_string())
}

/// Redacted report for bug reports: versions, counts, file sizes, error-free
/// facts only. No feed URLs, names, titles or deadlines.
#[tauri::command]
fn diagnostics(app: AppHandle, window: WebviewWindow) -> String {
    let mut out = String::new();
    out.push_str(&format!(
        "Deadline Notch {}\nOS: {} {}\n",
        env!("CARGO_PKG_VERSION"),
        std::env::consts::OS,
        std::env::consts::ARCH
    ));

    let monitors = window.available_monitors().unwrap_or_default();
    out.push_str(&format!("Monitors: {}\n", monitors.len()));
    for (i, m) in monitors.iter().enumerate() {
        out.push_str(&format!(
            "  #{i}: {}x{} @{:.2}x\n",
            m.size().width,
            m.size().height,
            m.scale_factor()
        ));
    }

    if let Ok(dir) = store::data_dir(&app) {
        out.push_str("Data files:\n");
        if let Ok(entries) = std::fs::read_dir(dir) {
            let mut files: Vec<_> = entries.flatten().collect();
            files.sort_by_key(|e| e.file_name());
            for e in files {
                let len = e.metadata().map(|m| m.len()).unwrap_or(0);
                // Feed cache names embed the feed id, which is generated, not personal.
                out.push_str(&format!("  {} ({} bytes)\n", e.file_name().to_string_lossy(), len));
            }
        }
    }

    if let Some(bytes) = store::read_file(&app, "settings.json") {
        if let Ok(v) = serde_json::from_slice::<serde_json::Value>(&bytes) {
            let pick = |k: &str| v.get(k).map(|x| x.to_string()).unwrap_or_else(|| "-".into());
            out.push_str(&format!(
                "Settings: language={} theme={} resting={} refreshMinutes={} thresholds={} toast={}\n",
                pick("language"),
                pick("theme"),
                pick("restingStyle"),
                pick("refreshMinutes"),
                pick("thresholdsHours"),
                pick("toast")
            ));
            if let Some(feeds) = v.get("feeds").and_then(|f| f.as_array()) {
                out.push_str(&format!("Feeds: {}\n", feeds.len()));
                for f in feeds {
                    let id = f.get("id").and_then(|x| x.as_str()).unwrap_or("");
                    let kind = f.get("kind").and_then(|x| x.as_str()).unwrap_or("?");
                    let enabled = f.get("enabled").and_then(|x| x.as_bool()).unwrap_or(true);
                    out.push_str(&format!(
                        "  kind={kind} enabled={enabled} link_saved={}\n",
                        feed::feed_has_secret(id.to_string())
                    ));
                }
            }
        }
    }
    out
}

fn show_settings(app: &AppHandle) {
    if let Some(w) = app.get_webview_window("settings") {
        let _ = w.unminimize();
        let _ = w.show();
        let _ = w.set_focus();
        return;
    }
    let builder = WebviewWindowBuilder::new(app, "settings", WebviewUrl::App("settings.html".into()))
        .title("Deadline Notch")
        .inner_size(900.0, 660.0)
        .min_inner_size(700.0, 520.0)
        .center();
    // Same browser args as the notch window (tauri.conf.json): WebView2 starts
    // a separate set of processes for each distinct set of args.
    #[cfg(windows)]
    let builder = builder.additional_browser_args(WEBVIEW_ARGS);
    let _ = builder.build();
}

#[tauri::command]
async fn open_settings(app: AppHandle) {
    show_settings(&app);
}

// ---------------------------------------------------------------- placement

/// Put the notch window top-center on the chosen monitor (below a top taskbar,
/// thanks to the work area), shifted by the user's horizontal offset.
fn place(win: &WebviewWindow, placement: Placement) {
    let monitors = win.available_monitors().unwrap_or_default();
    let primary = win.primary_monitor().ok().flatten();
    let Some(monitor) = monitors
        .get(placement.monitor)
        .cloned()
        .or(primary)
        .or_else(|| monitors.first().cloned())
    else {
        return;
    };

    // Move onto the monitor first so the window adopts its DPI, then size it.
    let _ = win.set_position(*monitor.position());
    let _ = win.set_size(LogicalSize::new(WIN_W, WIN_H));
    let Ok(size) = win.outer_size() else {
        return;
    };

    let work = monitor.work_area();
    let scale = monitor.scale_factor();
    let x = work.position.x
        + (work.size.width as i32 - size.width as i32) / 2
        + (placement.offset_x as f64 * scale).round() as i32;
    let y = work.position.y;
    let _ = win.set_position(PhysicalPosition::new(x, y));
}

/// Changes whenever a monitor is plugged, unplugged, resized or rescaled.
fn monitors_signature(win: &WebviewWindow) -> String {
    win.available_monitors()
        .unwrap_or_default()
        .iter()
        .map(|m| {
            let (p, s, w) = (m.position(), m.size(), m.work_area());
            format!(
                "{},{},{}x{},{:.2},{},{}",
                p.x, p.y, s.width, s.height, m.scale_factor(), w.position.y, w.size.height
            )
        })
        .collect::<Vec<_>>()
        .join(";")
}

// -------------------------------------------------------------- visibility

fn apply_visibility(win: &WebviewWindow, state: &NotchState) {
    let want_hidden = state.fullscreen.load(Ordering::Relaxed) || state.paused.load(Ordering::Relaxed);
    if state.hidden.swap(want_hidden, Ordering::Relaxed) != want_hidden {
        if want_hidden {
            let _ = win.hide();
        } else {
            let _ = win.show();
            win32::make_overlay(win);
        }
        dlog(&format!("hidden={want_hidden}"));
    }
}

fn spawn_cursor_loop(win: WebviewWindow, state: SharedState) {
    std::thread::spawn(move || {
        let mut inside = false;
        loop {
            let mut delay = Duration::from_millis(150);
            if state.hidden.load(Ordering::Relaxed) {
                if inside {
                    inside = false;
                    let _ = win.set_ignore_cursor_events(true);
                }
                delay = Duration::from_millis(500);
            } else if let (Ok(cur), Ok(pos), Ok(size), Ok(scale)) = (
                win.cursor_position(),
                win.outer_position(),
                win.outer_size(),
                win.scale_factor(),
            ) {
                let rx = cur.x - pos.x as f64;
                let ry = cur.y - pos.y as f64;
                let in_window =
                    rx >= 0.0 && ry >= 0.0 && rx < size.width as f64 && ry < size.height as f64;
                let shape = *state.shape.lock().unwrap();
                let (lx, ly) = (rx / scale, ry / scale);
                let now_inside = in_window && shape.contains(lx, ly, 0.0);
                if now_inside != inside {
                    inside = now_inside;
                    if inside {
                        win32::remember_foreground(&win);
                    }
                    let _ = win.set_ignore_cursor_events(!inside);
                    let _ = win.emit("notch-hover", inside);
                    dlog(&format!("hover inside={inside}"));
                }
                delay = if in_window && shape.contains(lx, ly, NEAR_MARGIN) {
                    Duration::from_millis(16)
                } else if in_window {
                    Duration::from_millis(60)
                } else {
                    Duration::from_millis(150)
                };
            }
            std::thread::sleep(delay);
        }
    });
}

/// Every 1.5 s: fullscreen/presentation check, and re-place the window when
/// the monitor layout, resolution or DPI changed.
fn spawn_watchdog(win: WebviewWindow, state: SharedState) {
    std::thread::spawn(move || {
        let mut signature = monitors_signature(&win);
        loop {
            std::thread::sleep(Duration::from_millis(1500));

            let busy = win32::fullscreen_app_active();
            if state.fullscreen.swap(busy, Ordering::Relaxed) != busy {
                dlog(&format!("fullscreen={busy}"));
            }
            apply_visibility(&win, &state);

            let now = monitors_signature(&win);
            if now != signature {
                signature = now;
                let placement = *state.placement.lock().unwrap();
                place(&win, placement);
                dlog("monitor layout changed, re-placed");
            }
        }
    });
}

// --------------------------------------------------------------------- tray

fn build_tray(app: &tauri::App, state: &SharedState) -> tauri::Result<()> {
    let pause = CheckMenuItem::with_id(app, "pause", "Pause notch", true, false, None::<&str>)?;
    let refresh = MenuItem::with_id(app, "refresh", "Refresh now", true, None::<&str>)?;
    let settings = MenuItem::with_id(app, "settings", "Settings", true, None::<&str>)?;
    let quit = MenuItem::with_id(app, "quit", "Quit", true, None::<&str>)?;
    let separator = PredefinedMenuItem::separator(app)?;
    let menu = Menu::with_items(app, &[&pause, &refresh, &settings, &separator, &quit])?;

    *state.tray.lock().unwrap() = Some(TrayItems {
        pause,
        refresh,
        settings,
        quit,
    });

    let mut tray = TrayIconBuilder::new()
        .menu(&menu)
        .tooltip("Deadline Notch")
        .on_menu_event(|app, event| match event.id.as_ref() {
            "pause" => {
                let state = app.state::<SharedState>();
                let checked = state
                    .tray
                    .lock()
                    .unwrap()
                    .as_ref()
                    .and_then(|t| t.pause.is_checked().ok())
                    .unwrap_or(false);
                state.paused.store(checked, Ordering::Relaxed);
                if let Some(win) = app.get_webview_window("notch") {
                    apply_visibility(&win, &state);
                }
            }
            "refresh" => {
                let _ = app.emit("refresh-now", ());
            }
            "settings" => show_settings(app),
            "quit" => app.exit(0),
            _ => {}
        });
    if let Some(icon) = app.default_window_icon() {
        tray = tray.icon(icon.clone());
    }
    tray.build(app)?;
    Ok(())
}

// ---------------------------------------------------------------------- run

pub fn run() {
    tauri::Builder::default()
        // Must be first: a second launch opens the running app's settings.
        .plugin(tauri_plugin_single_instance::init(|app, _args, _cwd| {
            show_settings(app);
        }))
        .plugin(tauri_plugin_notification::init())
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_autostart::init(
            tauri_plugin_autostart::MacosLauncher::LaunchAgent,
            None,
        ))
        .invoke_handler(tauri::generate_handler![
            set_shape,
            list_monitors,
            set_placement,
            set_tray_labels,
            debug_log,
            notify,
            open_url,
            open_settings,
            diagnostics,
            store::store_get,
            store::store_set,
            feed::feed_fetch,
            feed::feed_test,
            feed::feed_save,
            feed::feed_remove,
            feed::feed_has_secret,
        ])
        .setup(|app| {
            let win = app
                .get_webview_window("notch")
                .expect("notch window is declared in tauri.conf.json");
            let state: SharedState = Arc::new(NotchState::default());
            app.manage(state.clone());

            win32::install_no_activate(&win);
            win32::make_overlay(&win);
            place(&win, Placement::default());
            let _ = win.set_ignore_cursor_events(true);
            let _ = win.show();
            win32::make_overlay(&win);

            build_tray(app, &state)?;

            spawn_cursor_loop(win.clone(), state.clone());
            spawn_watchdog(win, state);
            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("error while running Deadline Notch");
}
