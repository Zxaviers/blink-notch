//! SteelSeries Sonar's master volume.
//!
//! Sonar's "Master" is a stage inside its own mixer, not a Windows sound device, so Windows' volume calls only ever reach one of its
//! channels (the virtual output that happens to be the default). GG, the program Sonar lives in, keeps a small web server on this PC
//! for its own windows; this talks to it the way those windows do. Nothing leaves the PC: GG's address comes from a file GG wrote
//! and is only used when it is a loopback address.
//!
//! Not a public interface of SteelSeries: it may change with a GG update. Every call can fail, and the caller then falls back to
//! Windows' own volume.

use std::time::Duration;

use serde_json::Value;

/// Where GG writes its addresses (`%ProgramData%`).
fn core_props_path() -> Option<std::path::PathBuf> {
    std::env::var_os("PROGRAMDATA").map(|p| std::path::PathBuf::from(p).join("SteelSeries").join("SteelSeries Engine 3").join("coreProps.json"))
}

/// GG's own (encrypted, self-signed) address out of `coreProps.json`.
fn gg_address(core_props: &str) -> Option<String> {
    let v: Value = serde_json::from_str(core_props).ok()?;
    let a = v.get("ggEncryptedAddress")?.as_str()?;
    a.starts_with("127.0.0.1:").then(|| a.to_string())
}

/// Where Sonar's own server listens, out of GG's `/subApps` answer (only when Sonar is running).
fn sonar_address(sub_apps: &str) -> Option<String> {
    let v: Value = serde_json::from_str(sub_apps).ok()?;
    let sonar = v.get("subApps")?.get("sonar")?;
    if !sonar.get("isRunning")?.as_bool()? {
        return None;
    }
    let a = sonar.get("metadata")?.get("webServerAddress")?.as_str()?;
    a.starts_with("http://127.0.0.1:").then(|| a.to_string())
}

/// The master as Sonar reports it: percent and muted.
fn parse_master(volume_settings: &str) -> Option<(u8, bool)> {
    let v: Value = serde_json::from_str(volume_settings).ok()?;
    let master = v.get("masters")?.get("classic")?;
    let volume = master.get("volume")?.as_f64()?;
    Some(((volume * 100.0).round().clamp(0.0, 100.0) as u8, master.get("muted")?.as_bool()?))
}

pub struct Sonar {
    agent: ureq::Agent,
    base: String,
}

impl Sonar {
    fn agent() -> ureq::Agent {
        // GG's address is encrypted with a certificate it made itself (for 127.0.0.1): it cannot be checked against the
        // system's roots, and it is only ever asked at a loopback address.
        ureq::Agent::config_builder()
            .timeout_global(Some(Duration::from_millis(1500)))
            .max_redirects(0)
            .http_status_as_error(false)
            .tls_config(ureq::tls::TlsConfig::builder().provider(ureq::tls::TlsProvider::NativeTls).disable_verification(true).build())
            .build()
            .into()
    }

    fn get(&self, url: &str) -> Option<String> {
        let resp = self.agent.get(url).call().ok()?;
        if resp.status().as_u16() != 200 {
            return None;
        }
        resp.into_body().with_config().limit(200_000).read_to_string().ok()
    }

    /// Find a running Sonar through GG. `None` when GG or Sonar is not there (the usual case on most PCs).
    pub fn discover() -> Option<Sonar> {
        let props = std::fs::read_to_string(core_props_path()?).ok()?;
        let gg = gg_address(&props)?;
        let sonar = Sonar { agent: Sonar::agent(), base: String::new() };
        let base = sonar_address(&sonar.get(&format!("https://{gg}/subApps"))?)?;
        Some(Sonar { base, ..sonar })
    }

    /// A Sonar at a known address (tests).
    pub fn at(base: &str) -> Sonar {
        Sonar { agent: Sonar::agent(), base: base.trim_end_matches('/').to_string() }
    }

    /// Whether Sonar is in its classic mode (the one with a single master); streamer mode has two masters and is left to Windows.
    pub fn is_classic(&self) -> bool {
        self.get(&format!("{}/mode", self.base)).is_some_and(|m| m.trim().trim_matches('"') == "classic")
    }

    /// The master volume (percent) and whether it is muted.
    pub fn master(&self) -> Option<(u8, bool)> {
        parse_master(&self.get(&format!("{}/volumeSettings/classic", self.base))?)
    }

    fn put(&self, path: &str) -> bool {
        self.agent.put(&format!("{}/volumeSettings/classic/master/{path}", self.base)).send_empty().is_ok_and(|r| r.status().as_u16() == 200)
    }

    pub fn set_volume(&self, percent: u8) -> bool {
        self.put(&format!("Volume/{}", format_args!("{:.2}", f32::from(percent.min(100)) / 100.0)))
    }

    pub fn set_muted(&self, muted: bool) -> bool {
        self.put(&format!("Mute/{muted}"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{Read, Write};
    use std::net::TcpListener;
    use std::sync::{Arc, Mutex};

    #[test]
    fn addresses_are_read_from_what_gg_says() {
        assert_eq!(gg_address(r#"{"ggEncryptedAddress":"127.0.0.1:6327","address":"127.0.0.1:53864"}"#).as_deref(), Some("127.0.0.1:6327"));
        assert_eq!(gg_address(r#"{"ggEncryptedAddress":"10.0.0.5:6327"}"#), None, "only loopback is ever asked");
        assert_eq!(gg_address("not json"), None);
        let apps = r#"{"subApps":{"sonar":{"isRunning":true,"metadata":{"webServerAddress":"http://127.0.0.1:53975"}}}}"#;
        assert_eq!(sonar_address(apps).as_deref(), Some("http://127.0.0.1:53975"));
        assert_eq!(sonar_address(&apps.replace("true", "false")), None, "not running");
        assert_eq!(sonar_address(&apps.replace("127.0.0.1", "example.com")), None);
        assert_eq!(sonar_address(r#"{"subApps":{}}"#), None);
    }

    #[test]
    fn the_master_is_read_from_the_classic_settings() {
        let json = r#"{"masters":{"stream":{},"classic":{"volume":0.1,"muted":false}},"devices":{"game":{"classic":{"volume":0.5,"muted":true}}}}"#;
        assert_eq!(parse_master(json), Some((10, false)));
        assert_eq!(parse_master(r#"{"masters":{"classic":{"volume":1.0,"muted":true}}}"#), Some((100, true)));
        assert_eq!(parse_master(r#"{"masters":{"stream":{}}}"#), None);
    }

    /// A one-thread server that answers every request with `body` and remembers the request lines.
    fn server(body: &'static str) -> (String, Arc<Mutex<Vec<String>>>) {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let base = format!("http://{}", listener.local_addr().unwrap());
        let seen = Arc::new(Mutex::new(Vec::new()));
        let log = seen.clone();
        std::thread::spawn(move || {
            for stream in listener.incoming() {
                let Ok(mut stream) = stream else { break };
                let mut buf = [0u8; 2048];
                let n = stream.read(&mut buf).unwrap_or(0);
                let text = String::from_utf8_lossy(&buf[..n]).to_string();
                log.lock().unwrap().push(text.lines().next().unwrap_or("").to_string());
                let _ = write!(stream, "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len());
            }
        });
        (base, seen)
    }

    #[test]
    fn it_reads_and_writes_the_master_with_the_calls_sonar_expects() {
        let (base, seen) = server(r#"{"masters":{"classic":{"volume":0.35,"muted":true}}}"#);
        let sonar = Sonar::at(&base);
        assert_eq!(sonar.master(), Some((35, true)));
        assert!(sonar.set_volume(40));
        assert!(sonar.set_muted(false));
        let lines = seen.lock().unwrap().clone();
        assert_eq!(lines[0], "GET /volumeSettings/classic HTTP/1.1");
        assert_eq!(lines[1], "PUT /volumeSettings/classic/master/Volume/0.40 HTTP/1.1");
        assert_eq!(lines[2], "PUT /volumeSettings/classic/master/Mute/false HTTP/1.1");
    }

    #[test]
    fn nothing_listening_is_a_quiet_failure() {
        let sonar = Sonar::at("http://127.0.0.1:1");
        assert_eq!(sonar.master(), None);
        assert!(!sonar.set_volume(10));
        assert!(!sonar.is_classic());
    }

    /// Needs SteelSeries GG with Sonar running on this PC: `cargo test -p blink-core sonar_on_this_pc -- --ignored --nocapture`.
    /// Writes the master volume it just read back to itself (no audible change).
    #[test]
    #[ignore]
    fn sonar_on_this_pc() {
        let sonar = Sonar::discover().expect("GG with a running Sonar");
        assert!(sonar.is_classic(), "classic mode");
        let (volume, muted) = sonar.master().expect("the master");
        println!("master: {volume}% muted={muted}");
        assert!(sonar.set_volume(volume));
        assert_eq!(sonar.master(), Some((volume, muted)));
    }
}
