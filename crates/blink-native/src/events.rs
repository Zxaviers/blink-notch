//! Things that happen on the computer that a module may want to tell you about.
//! The window layer turns Windows notifications into these; modules turn them into peeks.
//! No Windows here, so every mapping is testable.

use serde_json::Value;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AudioKind {
    Headphones,
    Speakers,
    Other,
}

#[derive(Clone, Debug, PartialEq)]
pub enum SystemEvent {
    /// The state of power after any change: plugged in or not, charge, charging.
    Power { on_ac: bool, percent: Option<u8>, charging: bool },
    Wifi { connected: bool, ssid: Option<String> },
    /// An audio output appeared, went away, or became the one in use.
    AudioDevice { name: String, kind: AudioKind, connected: bool },
    /// System volume (0 to 100) or mute changed.
    Volume { percent: u8, muted: bool },
    CapsLock(bool),
    /// The built-in screen's brightness changed (0 to 100).
    Brightness { percent: u8 },
    /// A paired Bluetooth device connected or disconnected, or its battery level changed.
    /// `audio` is true for headphones, earbuds and speakers.
    Bluetooth { name: String, connected: bool, battery: Option<u8>, audio: bool },
}

/// Whether an audio output is software (a mixer, a virtual cable, a game or streaming helper) rather
/// than something you plug in or pair. Those come and go with their programs and are not news.
pub fn is_software_audio(name: &str) -> bool {
    const SOFTWARE: [&str; 18] = [
        "virtual", "sonar", "steelseries", "voicemeeter", "vb-audio", "cable input", "cable output", "nvidia broadcast", "steam streaming", "oculus", "meta quest",
        "wave link", "elgato", "nahimic", "loopback", "stereo mix", "remote audio", "obs",
    ];
    let n = name.to_lowercase();
    SOFTWARE.iter().any(|w| n.contains(w))
}

fn kind_from(s: &str) -> AudioKind {
    match s {
        "headphones" => AudioKind::Headphones,
        "speakers" => AudioKind::Speakers,
        _ => AudioKind::Other,
    }
}

impl SystemEvent {
    /// For the control pipe: `{ "type": "volume", "percent": 40, "muted": false }` and so on.
    pub fn from_json(v: &Value) -> Option<SystemEvent> {
        let text = |k: &str| v.get(k).and_then(Value::as_str).map(String::from);
        let flag = |k: &str| v.get(k).and_then(Value::as_bool);
        let num = |k: &str| v.get(k).and_then(Value::as_u64);
        Some(match v.get("type")?.as_str()? {
            "power" => SystemEvent::Power { on_ac: flag("onAc")?, percent: num("percent").map(|p| p.min(100) as u8), charging: flag("charging").unwrap_or(false) },
            "wifi" => SystemEvent::Wifi { connected: flag("connected")?, ssid: text("ssid") },
            "audio" => SystemEvent::AudioDevice { name: text("name")?, kind: kind_from(&text("kind").unwrap_or_default()), connected: flag("connected")? },
            "volume" => SystemEvent::Volume { percent: num("percent")?.min(100) as u8, muted: flag("muted").unwrap_or(false) },
            "caps" => SystemEvent::CapsLock(flag("on")?),
            "brightness" => SystemEvent::Brightness { percent: num("percent")?.min(100) as u8 },
            "bluetooth" => SystemEvent::Bluetooth { name: text("name")?, connected: flag("connected")?, battery: num("battery").map(|b| b.min(100) as u8), audio: flag("audio").unwrap_or(false) },
            _ => return None,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn events_parse_from_the_control_pipe_shape() {
        assert_eq!(SystemEvent::from_json(&json!({ "type": "volume", "percent": 140, "muted": true })), Some(SystemEvent::Volume { percent: 100, muted: true }));
        assert_eq!(
            SystemEvent::from_json(&json!({ "type": "audio", "name": "AirBuds", "kind": "headphones", "connected": true })),
            Some(SystemEvent::AudioDevice { name: "AirBuds".into(), kind: AudioKind::Headphones, connected: true })
        );
        assert_eq!(SystemEvent::from_json(&json!({ "type": "caps", "on": true })), Some(SystemEvent::CapsLock(true)));
        assert_eq!(SystemEvent::from_json(&json!({ "type": "nope" })), None);
        assert_eq!(SystemEvent::from_json(&json!({ "type": "wifi" })), None, "missing fields are refused, not guessed");
    }
}
