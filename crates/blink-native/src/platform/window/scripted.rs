//! What scripted runs can ask of the window over the control pipe: where things are, move the clock, and every
//! command the gates use. A command goes through the same code as a real mouse event, minus the OS.

use super::*;

impl Runtime {
    // ------------------------------------------------------------ control

    pub(super) fn window_info(&self) -> Value {
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
            "hwnd": self.hwnd.0 as isize,
            "frames": self.frames,
            "hidden": self.hidden,
            "hiddenBy": self.hidden_by(),
            "visibilityHotkey": self.vis_hotkey,
            "paused": self.paused,
            "fullscreen": self.fullscreen,
            "hotkey": self.hotkey,
            "tray": self.tray.is_some(),
            "sources": { "power": self.sources.power, "wifi": self.sources.wifi, "audio": self.sources.audio, "bluetooth": self.sources.bluetooth, "keys": self.sources.keys, "volumeKeys": self.media_keys && self.take_volume_keys && !self.hidden, "dropTarget": crate::platform::dropper::registered() },
            "surface": { "w": self.surface.w, "h": self.surface.h },
        })
    }

    /// The pixel Windows would hit-test: alpha of the last presented frame, and
    /// which window `WindowFromPoint` reports there (no cursor movement needed).
    pub(super) fn hit(&mut self, x: f32, y: f32) -> Value {
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

    pub(super) fn advance(&mut self, ms: u64) {
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

    pub(super) fn command(&mut self, line: &str) -> String {
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

    pub(super) fn run_command(&mut self, v: &Value) -> Result<Value, String> {
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
                    "up" => PointerKind::Up,
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
                    "left" => Key::Left,
                    "right" => Key::Right,
                    "tab" => Key::Tab,
                    "up" => Key::Up,
                    "down" => Key::Down,
                    "home" => Key::Home,
                    "end" => Key::End,
                    "enter" => Key::Enter,
                    "space" => Key::Space,
                    "escape" => Key::Escape,
                    "backspace" => Key::Backspace,
                    "delete" => Key::Delete,
                    "char" => Key::Char(v["ch"].as_str().and_then(|s| s.chars().next()).ok_or("char needs ch")?),
                    other => return Err(format!("unknown key {other}")),
                };
                self.app.key(key);
            }
            "paste" => self.app.paste(v["text"].as_str().unwrap_or("")),
            "drop" => {
                let paths: Vec<String> = v["paths"].as_array().map(|a| a.iter().filter_map(|p| p.as_str().map(String::from)).collect()).unwrap_or_default();
                self.app.drop_files(&paths);
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
            "toast" => {
                // Really show a system notification (control mode normally only logs them).
                let title = v["title"].as_str().unwrap_or("Blink");
                let body = v["body"].as_str().unwrap_or("");
                let shown = self.toast(title, body, v["url"].as_str().map(String::from));
                return Ok(json!({ "handedToWindows": shown }));
            }
            "open_settings" => self.open_settings(v["section"].as_str().unwrap_or("general")),
            "modules" => {
                let list: Vec<Value> = self.app.module_list().iter().map(|(id, on)| json!({ "id": id, "enabled": on })).collect();
                return Ok(json!({ "modules": list, "active": self.app.active_module(), "restOwner": self.app.rest_owner_module() }));
            }
            "switch" => {
                let target = v["to"].as_str().unwrap_or("");
                match self.app.module_list().iter().position(|(id, _)| *id == target) {
                    Some(i) => self.app.switch_to(i),
                    None => return Err(format!("no module {target}")),
                }
            }
            "offer_peek" => {
                // What a module (or later a plugin) does: offer a peek with a priority, a time and a key.
                let view = crate::modules::PeekView {
                    code: v["code"].as_str().unwrap_or("").to_string(),
                    title: v["title"].as_str().unwrap_or("").to_string(),
                    count: v["count"].as_str().unwrap_or("").to_string(),
                    urgency: "later",
                    priority: v["priority"].as_u64().unwrap_or(1) as u8,
                    ttl_ms: v["ttl"].as_u64(),
                    key: v["key"].as_str().map(String::from),
                    progress: v["progress"].as_f64().map(|p| p as f32),
                    icon: None,
                    accent: None,
                    cover: None,
                };
                self.app.offer_peek(view, None);
            }
            "system_event" => match crate::events::SystemEvent::from_json(v) {
                Some(event) => self.app.system_event(event),
                None => return Err("unknown or incomplete system event".into()),
            },
            "controls" => {
                // Send a command to the real (or stand-in) controls: for scripted checks.
                use crate::modules::controls::{ControlCommand, ControlsBackend};
                let value = v["value"].as_u64().unwrap_or(0).min(100) as u8;
                let command = match v["action"].as_str().unwrap_or("") {
                    "set_volume" => ControlCommand::SetVolume(value),
                    "set_brightness" => ControlCommand::SetBrightness(value),
                    "mute" => ControlCommand::ToggleMute,
                    "refresh" => ControlCommand::Refresh,
                    other => return Err(format!("unknown controls action {other}")),
                };
                match (&self.winctl, &self.fake_controls) {
                    (Some(real), _) => real.send(command),
                    (None, Some(fake)) => fake.send(command),
                    _ => {}
                }
            }
            "media" => {
                // Scripted runs: change what the stand-in player says (title, artist, playing, position in ms, duration in ms, active).
                let Some(fake) = &self.fake_media else { return Err("the real player is in use".into()) };
                let wall = self.app.clock.wall_ms();
                fake.set(|s| {
                    if let Some(t) = v["title"].as_str() {
                        s.title = t.into();
                        s.track += 1;
                    }
                    if let Some(a) = v["artist"].as_str() {
                        s.artist = a.into();
                    }
                    if let Some(a) = v["active"].as_bool() {
                        s.active = a;
                    }
                    if let Some(p) = v["playing"].as_bool() {
                        s.position_ms = s.position_at(wall);
                        s.sampled_wall_ms = wall;
                        s.playing = p;
                    }
                    if let Some(p) = v["position"].as_u64() {
                        s.position_ms = p;
                        s.sampled_wall_ms = wall;
                    }
                    if let Some(d) = v["duration"].as_u64() {
                        s.duration_ms = d;
                    }
                });
                self.app.data_changed();
            }
            "media_log" => {
                let log = self.fake_media.as_ref().map(|f| f.take_log().iter().map(|c| format!("{c:?}")).collect::<Vec<_>>()).unwrap_or_default();
                return Ok(json!({ "commands": log }));
            }
            "controls_log" => {
                let log = self.fake_controls.as_ref().map(|f| f.take_log().iter().map(|c| format!("{c:?}")).collect::<Vec<_>>()).unwrap_or_default();
                return Ok(json!({ "commands": log }));
            }
            "controls_state" => {
                let state = self.winctl.as_ref().map(|c| crate::modules::controls::ControlsBackend::state(c.as_ref()));
                return Ok(json!({ "real": self.winctl.is_some(), "debug": format!("{state:?}") }));
            }
            "media_key" => self.media_key(v["vk"].as_u64().unwrap_or(0xAF) as u32),
            "reload_host" => self.reload_host(),
            "host" => {
                // Write host.json from the command, as the settings program will, then apply it.
                self.store.set_json("host", &v["config"]).map_err(|e| e.to_string())?;
                self.reload_host();
            }
            "dibcheck" => {
                // Does the window's bitmap equal the app's last frame, pixel for pixel? (Trails of old
                // frames would show up here as differing pixels.)
                self.present();
                let width = self.surface.w as usize;
                let height = self.surface.h as usize;
                let src = self.app.canvas().pm.data();
                let dst = self.surface.pixels();
                let mut differing = 0usize;
                let mut first_row = None;
                for y in 0..height {
                    for x in 0..width {
                        let i = (y * width + x) * 4;
                        let want = (u32::from(src[i + 3]) << 24) | (u32::from(src[i]) << 16) | (u32::from(src[i + 1]) << 8) | u32::from(src[i + 2]);
                        if dst[y * width + x] != want {
                            differing += 1;
                            first_row.get_or_insert(y);
                        }
                    }
                }
                return Ok(json!({ "differing": differing, "firstRow": first_row }));
            }
            "refresh" => self.app.refresh_sources(),
            "reload_local" => self.app.reload_local_events(),
            "pause" => {
                self.paused = v["on"].as_bool().unwrap_or(true);
                self.update_hidden();
            }
            "toggle_visibility" => self.toggle_visibility(),
            "fullscreen" => {
                // Pretend a fullscreen app is (or is not) in front; null goes back to asking Windows.
                // on=true: a game is fullscreen; kind="app": an ordinary app is; on=false: none; null: ask Windows.
                self.fullscreen_sim = match (v["on"].as_bool(), v["kind"].as_str()) {
                    (Some(true), Some("app")) => Some(system::FullscreenKind::App),
                    (Some(true), _) => Some(system::FullscreenKind::Game),
                    (Some(false), _) => Some(system::FullscreenKind::None),
                    (None, _) => None,
                };
                self.refresh_fullscreen();
            }
            "agent_update" | "agent_remove" | "agent_list" => return self.agent_command(&v),
            // The gestures, as the window messages would make them.
            "wheel_quick" => {
                let taken = self.app.quick_wheel(v["notches"].as_f64().unwrap_or(1.0) as f32, v["ctrl"].as_bool().unwrap_or(false));
                self.effects_log.push(format!("wheel_quick:{taken}"));
            }
            "middle" => {
                let taken = self.app.middle_click();
                self.effects_log.push(format!("middle:{taken}"));
            }
            "swipe" => self.app.swipe(v["delta"].as_f64().unwrap_or(1.0) as f32),
            "context_menu" => self.context_menu(),
            "tour" => {
                // The first-run tour, on demand (it starts by itself only the first time, and not in scripted runs).
                self.app.start_tour();
            }
            "desktop" => {
                // Pretend the empty desktop is (or is not) in front; null goes back to asking Windows.
                self.desktop_sim = v["on"].as_bool();
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
                let (cr, cc, cu, cn) = std::mem::take(&mut self.cost);
                let wakes = std::mem::take(&mut self.wakes);
                let per = |v: f32| if cn > 0 { v / cn as f32 } else { 0.0 };
                return Ok(json!({ "wakes": wakes, "presented": cn, "renderMs": per(cr), "copyMs": per(cc), "presentMs": per(cu), "frames": n, "minMs": if n > 0 { min } else { 0.0 }, "maxMs": max, "avgMs": avg, "p50Ms": pct(0.5), "p95Ms": pct(0.95), "over20ms": late }));
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
