//! System peeks: a short expansion when the charger is plugged in or pulled, the battery
//! runs low, Wi-Fi connects or drops, headphones appear or go, volume changes, Caps Lock
//! turns on or the keyboard layout changes. No panel of its own: it only speaks up.
//!
//! Each kind can be switched off in the module's options (`host.json`):
//! `{ "battery": true, "wifi": true, "audio": true, "volume": true, "caps": true, "language": true }`.

use blink_core::i18n::Lang;
use serde_json::Value;

use super::{Effect, Env, Key, KeyResult, Kit, Module, PanelFrame, PeekView, PointerKind, RestView};
use crate::events::{AudioKind, SystemEvent};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Kinds {
    pub battery: bool,
    pub wifi: bool,
    pub audio: bool,
    pub volume: bool,
    pub caps: bool,
    pub language: bool,
}

impl Default for Kinds {
    fn default() -> Self {
        Kinds { battery: true, wifi: true, audio: true, volume: true, caps: true, language: true }
    }
}

impl Kinds {
    pub fn from_options(options: &Value) -> Kinds {
        let on = |k: &str| options.get(k).and_then(Value::as_bool).unwrap_or(true);
        Kinds { battery: on("battery"), wifi: on("wifi"), audio: on("audio"), volume: on("volume"), caps: on("caps"), language: on("language") }
    }
}

/// Where the battery is, for "low" and "critical" warnings that fire once per drop.
const LOW: u8 = 20;
const CRITICAL: u8 = 10;

#[derive(Default)]
pub struct SystemModule {
    kinds: Kinds,
    /// The first power report only sets the baseline; announcing the state at startup would be noise.
    power: Option<(bool, Option<u8>)>,
    announced_low: bool,
    announced_critical: bool,
    announced_full: bool,
}

impl SystemModule {
    pub fn new() -> Self {
        SystemModule::default()
    }
}

/// The words, English and Indonesian.
fn words(lang: Lang, key: &str) -> &'static str {
    let id = lang == Lang::Id;
    match key {
        "battery" => if id { "Baterai" } else { "Battery" },
        "charger_on" => if id { "Pengisi daya terhubung" } else { "Charger connected" },
        "charger_off" => if id { "Pengisi daya dicabut" } else { "Charger removed" },
        "full" => if id { "Baterai penuh" } else { "Fully charged" },
        "low" => if id { "Baterai lemah" } else { "Battery low" },
        "critical" => if id { "Baterai hampir habis" } else { "Battery almost empty" },
        "wifi" => "Wi-Fi",
        "wifi_on" => if id { "Terhubung ke" } else { "Connected to" },
        "wifi_on_plain" => if id { "Wi-Fi terhubung" } else { "Wi-Fi connected" },
        "wifi_off" => if id { "Wi-Fi terputus" } else { "Wi-Fi disconnected" },
        "audio" => if id { "Audio" } else { "Audio" },
        "headphones" => if id { "Headphone" } else { "Headphones" },
        "speakers" => if id { "Speaker" } else { "Speakers" },
        "connected" => if id { "terhubung" } else { "connected" },
        "disconnected" => if id { "terputus" } else { "disconnected" },
        "volume" => if id { "Volume" } else { "Volume" },
        "muted" => if id { "Senyap" } else { "Muted" },
        "caps_on" => if id { "Caps Lock menyala" } else { "Caps Lock on" },
        "caps_off" => if id { "Caps Lock mati" } else { "Caps Lock off" },
        "keyboard" => if id { "Papan ketik" } else { "Keyboard" },
        _ => "",
    }
}

fn view(code: &str, title: String, count: String, urgency: &'static str, priority: u8, key: &str, ttl_ms: Option<u64>, progress: Option<f32>) -> PeekView {
    PeekView { code: code.to_string(), title, count, urgency, priority, ttl_ms, key: Some(format!("system:{key}")), progress }
}

impl SystemModule {
    fn power(&mut self, lang: Lang, on_ac: bool, percent: Option<u8>, charging: bool) -> Option<PeekView> {
        let w = |k| words(lang, k);
        let pct = percent.map(|p| format!("{p}%")).unwrap_or_default();
        let progress = percent.map(|p| f32::from(p) / 100.0);
        let previous = self.power.replace((on_ac, percent));
        let Some((was_ac, was_pct)) = previous else { return None };

        if on_ac != was_ac {
            self.announced_low = false;
            self.announced_critical = false;
            self.announced_full = false;
            let title = if on_ac { w("charger_on") } else { w("charger_off") };
            return Some(view(w("battery"), title.to_string(), pct, "later", 1, "power", None, progress));
        }
        if on_ac {
            if charging {
                self.announced_full = false;
            }
            if percent == Some(100) && !self.announced_full && was_pct != Some(100) {
                self.announced_full = true;
                return Some(view(w("battery"), w("full").to_string(), pct, "later", 1, "power", None, progress));
            }
            return None;
        }
        // On battery: warn once as it crosses each line on the way down.
        let now = percent?;
        if now <= CRITICAL && !self.announced_critical {
            self.announced_critical = true;
            self.announced_low = true;
            return Some(view(w("battery"), w("critical").to_string(), pct, "late", 3, "power", Some(6000), progress));
        }
        if now <= LOW && now > CRITICAL && !self.announced_low {
            self.announced_low = true;
            return Some(view(w("battery"), w("low").to_string(), pct, "soon", 2, "power", Some(5000), progress));
        }
        None
    }
}

impl Module for SystemModule {
    fn id(&self) -> &'static str {
        "system"
    }

    fn title(&self, lang: Lang) -> String {
        if lang == Lang::Id { "Sistem" } else { "System" }.to_string()
    }

    fn as_any_mut(&mut self) -> &mut dyn std::any::Any {
        self
    }

    fn has_panel(&self) -> bool {
        false
    }

    fn set_options(&mut self, options: &Value) {
        self.kinds = Kinds::from_options(options);
    }

    fn on_event(&mut self, event: &SystemEvent, env: &Env) -> Vec<Effect> {
        let lang = env.t.lang();
        let w = |k| words(lang, k);
        let peek = match event {
            SystemEvent::Power { on_ac, percent, charging } => {
                // Keep the baseline up to date even when announcements are off.
                let p = self.power(lang, *on_ac, *percent, *charging);
                if self.kinds.battery { p } else { None }
            }
            SystemEvent::Wifi { connected, ssid } if self.kinds.wifi => {
                let (title, urgency) = match (connected, ssid) {
                    (true, Some(name)) => (format!("{} {name}", w("wifi_on")), "later"),
                    (true, None) => (w("wifi_on_plain").to_string(), "later"),
                    (false, _) => (w("wifi_off").to_string(), "soon"),
                };
                Some(view(w("wifi"), title, String::new(), urgency, 1, "wifi", None, None))
            }
            SystemEvent::AudioDevice { name, kind, connected } if self.kinds.audio => {
                let what = match kind {
                    AudioKind::Headphones => w("headphones"),
                    AudioKind::Speakers => w("speakers"),
                    AudioKind::Other => w("audio"),
                };
                let state = if *connected { w("connected") } else { w("disconnected") };
                Some(view(what, name.clone(), state.to_string(), if *connected { "later" } else { "soon" }, 1, "audio", None, None))
            }
            SystemEvent::Volume { percent, muted } if self.kinds.volume => {
                let (count, progress) = if *muted { (w("muted").to_string(), 0.0) } else { (format!("{percent}%"), f32::from(*percent) / 100.0) };
                Some(view(w("volume"), String::new(), count, "later", 1, "volume", Some(1400), Some(progress)))
            }
            SystemEvent::CapsLock(on) if self.kinds.caps => {
                let title = if *on { w("caps_on") } else { w("caps_off") };
                Some(view("Caps", title.to_string(), String::new(), if *on { "soon" } else { "later" }, 1, "caps", Some(1600), None))
            }
            SystemEvent::KeyboardLayout { code, name } if self.kinds.language => {
                Some(view(w("keyboard"), name.clone(), code.clone(), "later", 1, "layout", Some(1800), None))
            }
            _ => None,
        };
        peek.map(Effect::ShowPeek).into_iter().collect()
    }

    // ---- no panel: the rest of the module contract does nothing

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
        0.0
    }

    fn draw_panel(&mut self, _env: &Env, _kit: &mut Kit, _frame: &PanelFrame) {}

    fn pointer(&mut self, _kind: PointerKind, _x: f32, _y: f32, _env: &Env) -> Vec<Effect> {
        Vec::new()
    }

    fn key(&mut self, _key: Key, _env: &Env) -> KeyResult {
        KeyResult::default()
    }

    fn keyboard_mode(&mut self, _on: bool, _env: &Env) {}

    fn next_wake(&mut self, _env: &Env, _panel_open: bool) -> Option<u64> {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use blink_core::i18n::Translator;
    use jiff::tz::TimeZone;

    fn run(m: &mut SystemModule, lang: Lang, ev: SystemEvent) -> Option<PeekView> {
        let tz = TimeZone::UTC;
        let env = Env { now_ms: 0, wall_ms: 0, tz: &tz, t: Translator::new(lang) };
        m.on_event(&ev, &env).into_iter().find_map(|e| if let Effect::ShowPeek(p) = e { Some(p) } else { None })
    }

    fn power(on_ac: bool, percent: u8) -> SystemEvent {
        SystemEvent::Power { on_ac, percent: Some(percent), charging: on_ac && percent < 100 }
    }

    #[test]
    fn the_first_power_report_is_only_a_baseline() {
        let mut m = SystemModule::new();
        assert!(run(&mut m, Lang::En, power(true, 70)).is_none());
        let p = run(&mut m, Lang::En, power(false, 70)).expect("unplugging says so");
        assert_eq!((p.title.as_str(), p.count.as_str()), ("Charger removed", "70%"));
        assert_eq!(p.progress, Some(0.7));
    }

    #[test]
    fn plugging_in_and_filling_up() {
        let mut m = SystemModule::new();
        run(&mut m, Lang::En, power(false, 50));
        assert_eq!(run(&mut m, Lang::En, power(true, 50)).unwrap().title, "Charger connected");
        assert!(run(&mut m, Lang::En, power(true, 80)).is_none(), "charge ticking up is not news");
        assert_eq!(run(&mut m, Lang::En, power(true, 100)).unwrap().title, "Fully charged");
        assert!(run(&mut m, Lang::En, power(true, 100)).is_none(), "said once");
    }

    #[test]
    fn low_and_critical_each_fire_once_on_the_way_down_and_critical_is_urgent() {
        let mut m = SystemModule::new();
        run(&mut m, Lang::En, power(false, 40));
        assert!(run(&mut m, Lang::En, power(false, 25)).is_none());
        let low = run(&mut m, Lang::En, power(false, 20)).expect("20% warns");
        assert_eq!((low.title.as_str(), low.priority), ("Battery low", 2));
        assert!(run(&mut m, Lang::En, power(false, 19)).is_none(), "not again at 19");
        let crit = run(&mut m, Lang::En, power(false, 10)).expect("10% warns");
        assert_eq!((crit.title.as_str(), crit.priority, crit.urgency), ("Battery almost empty", 3, "late"));
        assert!(run(&mut m, Lang::En, power(false, 9)).is_none());
        // Plugging in and unplugging again re-arms the warnings.
        run(&mut m, Lang::En, power(true, 15));
        run(&mut m, Lang::En, power(false, 15));
        assert_eq!(run(&mut m, Lang::En, power(false, 12)).unwrap().title, "Battery low", "re-armed by the plug cycle");
    }

    #[test]
    fn volume_is_one_updating_peek_with_a_bar() {
        let mut m = SystemModule::new();
        let a = run(&mut m, Lang::En, SystemEvent::Volume { percent: 30, muted: false }).unwrap();
        let b = run(&mut m, Lang::En, SystemEvent::Volume { percent: 35, muted: false }).unwrap();
        assert_eq!(a.key, b.key, "same key: replaces itself");
        assert_eq!((b.count.as_str(), b.progress), ("35%", Some(0.35)));
        let muted = run(&mut m, Lang::En, SystemEvent::Volume { percent: 35, muted: true }).unwrap();
        assert_eq!((muted.count.as_str(), muted.progress), ("Muted", Some(0.0)));
    }

    #[test]
    fn devices_wifi_caps_and_layout_have_their_words_in_both_languages() {
        let mut m = SystemModule::new();
        let p = run(&mut m, Lang::En, SystemEvent::AudioDevice { name: "AirBuds Pro".into(), kind: AudioKind::Headphones, connected: true }).unwrap();
        assert_eq!((p.code.as_str(), p.title.as_str(), p.count.as_str()), ("Headphones", "AirBuds Pro", "connected"));
        let p = run(&mut m, Lang::Id, SystemEvent::AudioDevice { name: "AirBuds Pro".into(), kind: AudioKind::Headphones, connected: false }).unwrap();
        assert_eq!((p.code.as_str(), p.count.as_str()), ("Headphone", "terputus"));
        assert_eq!(run(&mut m, Lang::En, SystemEvent::Wifi { connected: true, ssid: Some("Campus".into()) }).unwrap().title, "Connected to Campus");
        assert_eq!(run(&mut m, Lang::Id, SystemEvent::Wifi { connected: false, ssid: None }).unwrap().title, "Wi-Fi terputus");
        assert_eq!(run(&mut m, Lang::En, SystemEvent::CapsLock(true)).unwrap().title, "Caps Lock on");
        let l = run(&mut m, Lang::En, SystemEvent::KeyboardLayout { code: "ID".into(), name: "Indonesian".into() }).unwrap();
        assert_eq!((l.title.as_str(), l.count.as_str()), ("Indonesian", "ID"));
    }

    #[test]
    fn each_kind_can_be_switched_off_and_the_battery_baseline_still_tracks() {
        let mut m = SystemModule::new();
        m.set_options(&serde_json::json!({ "battery": false, "caps": false }));
        assert!(run(&mut m, Lang::En, SystemEvent::CapsLock(true)).is_none());
        assert!(run(&mut m, Lang::En, power(true, 50)).is_none());
        assert!(run(&mut m, Lang::En, power(false, 50)).is_none(), "switched off");
        m.set_options(&serde_json::json!({}));
        assert!(run(&mut m, Lang::En, power(true, 50)).is_some(), "on again: it knew the state all along, so plugging in is news");
        assert!(run(&mut m, Lang::En, SystemEvent::Volume { percent: 1, muted: false }).is_some());
    }
}
