//! What other programs ask of the running notch: the settings program (open it, reload what it changed), the
//! installer (quit) and the tour. They arrive on the notch's private pipe as one JSON line each.

use super::*;

impl Runtime {
    /// The settings program is a separate executable next to this one (Phase 4).
    /// Open the settings program, on one of its sections (`feeds` for the deadlines, `general` for Blink).
    /// The menu of the tray icon, at the pointer. In a scripted run it is only noted, since an OS menu would wait for a click.
    pub(super) fn context_menu(&mut self) {
        if self.control {
            self.effects_log.push("context_menu".to_string());
            return;
        }
        // The menu needs the foreground to close when clicking elsewhere; the app that had it gets it back afterwards.
        let before = unsafe { windows::Win32::UI::WindowsAndMessaging::GetForegroundWindow() };
        let cmd = tray::show_menu(self.hwnd, &self.menu_labels(), self.paused);
        if cmd != 0 {
            self.tray_command(cmd);
        }
        if !before.0.is_null() && before != self.hwnd {
            unsafe {
                let _ = windows::Win32::UI::WindowsAndMessaging::SetForegroundWindow(before);
            }
        }
    }

    pub(super) fn open_settings(&mut self, section: &str) {
        let path = std::env::var_os("BLINK_SETTINGS_EXE")
            .map(std::path::PathBuf::from)
            .or_else(|| std::env::current_exe().ok().map(|p| p.with_file_name("blink-settings.exe")))
            .filter(|p| p.exists());
        // Scripted runs only log it, unless they name a settings program to start.
        if self.control && std::env::var_os("BLINK_SETTINGS_EXE").is_none() {
            self.effects_log.push(format!("open_settings:{section}"));
            return;
        }
        // A second launch of the settings program just brings the open one to the front.
        if let Some(path) = path {
            let _ = std::process::Command::new(path).args(["--section", section]).spawn();
        }
    }

    /// The settings program changed `settings.json`: take it in, as if it had just been loaded.
    /// Returns the hotkey status ("ok", "HOTKEY_IN_USE", "HOTKEY_INVALID").
    pub(super) fn reload_settings(&mut self) -> &'static str {
        let settings = normalize_settings(self.store.get_json("settings").ok().flatten().as_ref());
        let accelerator = settings.hotkey.clone();
        self.app.apply_settings(settings, &system_language());
        self.place();
        let status = self.register_hotkey(&accelerator);
        self.reload_host();
        self.dirty = true;
        status
    }

    /// Re-read which modules are on and in what order (`host.json`).
    pub(super) fn reload_host(&mut self) {
        let file = self.store.get_json("host").ok().flatten();
        self.fullscreen_mode = crate::host::parse_fullscreen_mode(file.as_ref());
        self.take_volume_keys = crate::host::parse_take_over_volume_keys(file.as_ref());
        self.apply_media_keys();
        self.refresh_fullscreen();
        self.app.configure_modules(&crate::host::parse_modules(file.as_ref()));
        self.app.set_look(crate::look::parse(file.as_ref()));
        self.app.set_gestures(crate::host::parse_gestures(file.as_ref()));
        let status = self.register_visibility_hotkey(&crate::host::parse_visibility_hotkey(file.as_ref()));
        if status != "ok" {
            self.effects_log.push(format!("hotkey:visibility:{status}"));
        }
        let status = self.register_shelf_hotkey(&crate::host::parse_shelf_hotkey(file.as_ref()));
        if status != "ok" {
            self.effects_log.push(format!("hotkey:shelf:{status}"));
        }
    }

    /// Redacted facts about the running notch, appended to the settings program's report.
    pub(super) fn diagnostics(&mut self) -> String {
        let state = self.app.state_json();
        let module = &state["module"];
        format!(
            "Notch: running, version {}, scale {:.2}\n  mode={} resting={} hidden={} paused={} fullscreen={}\n  shell: {}\n  hotkey={} tray={}\n  deadlines: {} upcoming, {} overdue, sync={}\n  frames drawn: {}\n",
            env!("CARGO_PKG_VERSION"),
            self.scale(),
            state["mode"].as_str().unwrap_or("?"),
            state["restStyle"].as_str().unwrap_or("?"),
            self.hidden,
            self.paused,
            self.fullscreen,
            self.app.why_open(),
            if self.hotkey.is_empty() { "off" } else { self.hotkey.as_str() },
            self.tray.is_some(),
            module["upcoming"].as_array().map_or(0, Vec::len),
            module["overdue"].as_array().map_or(0, Vec::len),
            module["sync"]["kind"].as_str().unwrap_or("?"),
            self.frames,
        )
    }

    /// Requests from the settings program (the notch pipe). A small, fixed set.
    pub(super) fn ipc_command(&mut self, line: &str) -> String {
        let reply = match serde_json::from_str::<Value>(line) {
            Ok(v) => match v["cmd"].as_str().unwrap_or("") {
                "ping" => Ok(json!({ "version": env!("CARGO_PKG_VERSION") })),
                "reload_settings" => match self.reload_settings() {
                    "ok" => Ok(json!({ "hotkey": "ok" })),
                    code => Ok(json!({ "hotkey": code })),
                },
                "reload_local" => {
                    self.app.reload_local_events();
                    Ok(json!({}))
                }
                "reload_host" => {
                    self.reload_host();
                    self.dirty = true;
                    Ok(json!({}))
                }
                "set_hotkey" => match self.register_hotkey(v["accelerator"].as_str().unwrap_or("")) {
                    "ok" => Ok(json!({})),
                    code => Err(code.to_string()),
                },
                "set_shelf_hotkey" => match self.register_shelf_hotkey(v["accelerator"].as_str().unwrap_or("")) {
                    "ok" => Ok(json!({})),
                    code => Err(code.to_string()),
                },
                "refresh_now" => {
                    self.app.refresh_sources();
                    Ok(json!({}))
                }
                "diagnostics" => Ok(json!({ "text": self.diagnostics() })),
                "show_tour" => {
                    self.app.start_tour();
                    self.dirty = true;
                    Ok(json!({}))
                }
                // The installer and `blink --quit` ask the running notch to leave cleanly.
                "quit" => {
                    unsafe {
                        let _ = PostMessageW(Some(self.hwnd), windows::Win32::UI::WindowsAndMessaging::WM_CLOSE, WPARAM(0), LPARAM(0));
                    }
                    Ok(json!({}))
                }
                other => Err(format!("UNKNOWN_COMMAND {other}")),
            },
            Err(_) => Err("BAD_JSON".to_string()),
        };
        match reply {
            Ok(mut v) => {
                v["ok"] = Value::Bool(true);
                v.to_string()
            }
            Err(e) => json!({ "ok": false, "error": e }).to_string(),
        }
    }
}
