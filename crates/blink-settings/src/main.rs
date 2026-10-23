//! Blink settings: the page in `src/settings.html` in a Tauri window.
//!
//! A separate program on purpose: it runs only while the window is open, so
//! the always-running notch never loads a web engine and the memory comes back
//! when this closes. It edits the same files and Credential Manager entries as
//! the notch and tells it about changes over the notch's pipe
//! (`blink_core::ipc`). If the notch is not running, edits are simply picked
//! up the next time it starts.

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use blink_core::feed::{check_id, FeedResult, FeedService};
use blink_core::hotkey::parse_accelerator;
use blink_core::ipc::{self, IpcError};
use blink_core::secret::CredentialManager;
use blink_core::store::{default_dir, Store};
use serde::Serialize;
use serde_json::{json, Value};
use tauri::{Listener, Manager, WebviewUrl, WebviewWindow, WebviewWindowBuilder};
use tauri_plugin_opener::OpenerExt;

/// Both programs must start WebView2 with the same flags, or each gets its own process set.
/// Software rendering: a settings form does not need the GPU process (tens of MB).
const WEBVIEW_ARGS: &str = "--disable-gpu --disable-features=msWebOOUI,msPdfOOUI,msSmartScreenProtection";

fn store() -> Result<Store, String> {
    let dir = default_dir().ok_or("STORE")?;
    std::fs::create_dir_all(&dir).map_err(|_| "STORE".to_string())?;
    Ok(Store::new(dir))
}

fn feeds() -> Result<FeedService<CredentialManager>, String> {
    Ok(FeedService::new(store()?, CredentialManager))
}

// ------------------------------------------------------------------ commands

#[tauri::command]
fn store_get(name: String) -> Result<Option<Value>, String> {
    Ok(store()?.get_json(&name)?)
}

#[tauri::command]
fn store_set(name: String, value: Value) -> Result<(), String> {
    Ok(store()?.set_json(&name, &value)?)
}

/// Fetch a link the user typed, without saving it. Blocking network: off the UI thread.
#[tauri::command]
async fn feed_test(url: String) -> Result<FeedResult, String> {
    tauri::async_runtime::spawn_blocking(move || feeds().and_then(|f| f.test_url(&url).map_err(String::from)))
        .await
        .map_err(|_| "NETWORK".to_string())?
}

#[tauri::command]
fn feed_save(id: String, url: String) -> Result<(), String> {
    Ok(feeds()?.save(&id, &url)?)
}

#[tauri::command]
fn feed_remove(id: String) -> Result<(), String> {
    Ok(feeds()?.remove(&id)?)
}

#[tauri::command]
fn feed_has_secret(id: String) -> bool {
    feeds().is_ok_and(|f| f.has_secret(&id))
}

/// Rebind the global hotkey. The notch owns the registration, so it answers:
/// Ok, HOTKEY_IN_USE or HOTKEY_INVALID. With no notch running only the form is checked.
#[tauri::command]
async fn set_hotkey(accelerator: String) -> Result<(), String> {
    tauri::async_runtime::spawn_blocking(move || match ipc::request(&json!({ "cmd": "set_hotkey", "accelerator": accelerator })) {
        Ok(_) => Ok(()),
        Err(IpcError::NotRunning) => {
            if accelerator.trim().is_empty() || parse_accelerator(&accelerator).is_some() {
                Ok(())
            } else {
                Err("HOTKEY_INVALID".to_string())
            }
        }
        Err(IpcError::Failed(code)) => Err(code),
    })
    .await
    .map_err(|_| "HOTKEY_INVALID".to_string())?
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

/// Open a task page in the default browser. https only.
#[tauri::command]
fn open_url(app: tauri::AppHandle, url: String) -> Result<(), String> {
    if !url.to_ascii_lowercase().starts_with("https://") {
        return Err("URL_PROTOCOL".to_string());
    }
    app.opener().open_url(url, None::<&str>).map_err(|_| "OPEN".to_string())
}

#[tauri::command]
fn autostart_is_enabled() -> bool {
    blink_core::autostart::is_enabled()
}

#[tauri::command]
fn autostart_enable() -> Result<(), String> {
    blink_core::autostart::set_enabled(true)
}

#[tauri::command]
fn autostart_disable() -> Result<(), String> {
    blink_core::autostart::set_enabled(false)
}

/// Redacted report for bug reports: versions, counts, file sizes, error-free
/// facts only. No feed URLs, names, titles or deadlines.
#[tauri::command]
async fn diagnostics(window: WebviewWindow) -> String {
    let monitors: Vec<(u32, u32, f64)> =
        window.available_monitors().unwrap_or_default().iter().map(|m| (m.size().width, m.size().height, m.scale_factor())).collect();
    tauri::async_runtime::spawn_blocking(move || build_report(&monitors)).await.unwrap_or_default()
}

fn build_report(monitors: &[(u32, u32, f64)]) -> String {
    let mut out = format!("Blink {}\nOS: {} {}\n", env!("CARGO_PKG_VERSION"), std::env::consts::OS, std::env::consts::ARCH);
    out.push_str(&format!("Monitors: {}\n", monitors.len()));
    for (i, (w, h, scale)) in monitors.iter().enumerate() {
        out.push_str(&format!("  #{i}: {w}x{h} @{scale:.2}x\n"));
    }

    if let Ok(store) = store() {
        out.push_str("Data files:\n");
        if let Ok(entries) = std::fs::read_dir(store.dir()) {
            let mut files: Vec<_> = entries.flatten().collect();
            files.sort_by_key(|e| e.file_name());
            for e in files {
                // Feed cache names embed the feed id, which is generated, not personal.
                out.push_str(&format!("  {} ({} bytes)\n", e.file_name().to_string_lossy(), e.metadata().map(|m| m.len()).unwrap_or(0)));
            }
        }
        if let Ok(Some(v)) = store.get_json("settings") {
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
            if let Some(list) = v.get("feeds").and_then(|f| f.as_array()) {
                out.push_str(&format!("Feeds: {}\n", list.len()));
                for f in list {
                    let id = f.get("id").and_then(|x| x.as_str()).unwrap_or("");
                    let kind = f.get("kind").and_then(|x| x.as_str()).unwrap_or("?");
                    let enabled = f.get("enabled").and_then(|x| x.as_bool()).unwrap_or(true);
                    let saved = check_id(id).is_ok() && feeds().is_ok_and(|s| s.has_secret(id));
                    out.push_str(&format!("  kind={kind} enabled={enabled} link_saved={saved}\n"));
                }
            }
        }
    }
    out.push_str(&format!("Autostart: {}\n", blink_core::autostart::is_enabled()));

    match ipc::request(&json!({ "cmd": "diagnostics" })) {
        Ok(v) => out.push_str(v["text"].as_str().unwrap_or("")),
        Err(IpcError::NotRunning) => out.push_str("Notch: not running\n"),
        Err(IpcError::Failed(code)) => out.push_str(&format!("Notch: no answer ({code})\n")),
    }
    out
}

// ------------------------------------------------------------------- window

/// The section asked for on the command line (`--section feeds`), if it is a plain word.
fn section_from(args: &[String]) -> Option<String> {
    let at = args.iter().position(|a| a == "--section")?;
    args.get(at + 1).filter(|s| s.chars().all(|c| c.is_ascii_alphanumeric())).cloned()
}

fn show_settings(app: &tauri::AppHandle, section: Option<String>) {
    if let Some(w) = app.get_webview_window("settings") {
        let _ = w.unminimize();
        let _ = w.show();
        let _ = w.set_focus();
        if let Some(section) = section {
            let _ = w.eval(format!("window.__openSection && window.__openSection('{section}')"));
        }
        return;
    }
    // The page asks this flag which bridge it has (autostart goes through app commands here).
    // Scripted runs keep the window hidden so they never take focus from what the user is doing.
    let hidden = std::env::var_os("BLINK_SETTINGS_TEST").is_some();
    let _ = WebviewWindowBuilder::new(app, "settings", WebviewUrl::App("settings.html".into()))
        .title("Blink")
        .visible(!hidden)
        .focused(!hidden)
        .inner_size(900.0, 660.0)
        .min_inner_size(700.0, 520.0)
        .center()
        .initialization_script(format!("window.__BLINK_SETTINGS__ = true; window.__SECTION__ = {:?};", section.unwrap_or_default()))
        .additional_browser_args(WEBVIEW_ARGS)
        .build();
}

/// Tell the notch something changed, off the UI thread. Failure is fine: it picks files up at start.
fn tell_notch(cmd: &'static str) {
    std::thread::spawn(move || {
        let _ = ipc::request(&json!({ "cmd": cmd }));
    });
}

fn main() {
    tauri::Builder::default()
        // Must be first: a second launch brings the open window to the front.
        .plugin(tauri_plugin_single_instance::init(|app, args, _cwd| show_settings(app, section_from(&args))))
        .plugin(tauri_plugin_opener::init())
        .invoke_handler(tauri::generate_handler![
            store_get,
            store_set,
            feed_test,
            feed_save,
            feed_remove,
            feed_has_secret,
            set_hotkey,
            list_monitors,
            open_url,
            autostart_is_enabled,
            autostart_enable,
            autostart_disable,
            diagnostics,
        ])
        // Closing the window ends the program, so no web engine stays behind.
        .on_window_event(|window, event| {
            if matches!(event, tauri::WindowEvent::CloseRequested { .. }) {
                window.app_handle().exit(0);
            }
        })
        .setup(|app| {
            // The page announces its changes as events; they become messages to the notch.
            app.listen("settings-changed", |_| tell_notch("reload_settings"));
            app.listen("data-changed", |_| tell_notch("reload_local"));
            show_settings(app.handle(), section_from(&std::env::args().collect::<Vec<_>>()));
            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("error while running Blink settings");
}
