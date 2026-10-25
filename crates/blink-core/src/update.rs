//! Is there a newer Blink? Asked only when the person presses "Check for updates" (nothing is checked in the background), at a link
//! they were given: a small JSON file, `{"version": "0.3.0", "url": "https://.../Blink-0.3.0-setup.exe", "notes": "..."}`.
//!
//! The answer says what the newest version is and where to get it; the person opens the link, or, when the file also gives the
//! installer's SHA-256 (`"sha256": "<64 hex digits>"`), Blink can download it and check it is exactly that file before anything is
//! run (`download_verified`). An installer with no hash is never run by Blink. Only `https` links are asked (a link to this computer,
//! `http://127.0.0.1`, is allowed for tests).

use std::time::Duration;

use serde_json::Value;

/// What the update file said, for a version newer than this one.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Release {
    pub version: String,
    pub url: String,
    pub notes: String,
    /// The installer's SHA-256 as the update file gave it (lowercase hex), when it gave one.
    pub sha256: Option<String>,
}

/// The result of a check.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Checked {
    /// This is the newest (or newer than what the file says).
    UpToDate { latest: String },
    Newer(Release),
}

/// `1.2.10` against `1.2.9`: numbers, not text. A part that is not a number counts as zero; missing parts are zero.
fn parts(version: &str) -> Vec<u64> {
    version.trim().trim_start_matches(['v', 'V']).split(['.', '-', '+']).map(|p| p.parse::<u64>().unwrap_or(0)).collect()
}

pub fn is_newer(candidate: &str, current: &str) -> bool {
    let (a, b) = (parts(candidate), parts(current));
    for i in 0..a.len().max(b.len()) {
        let (x, y) = (a.get(i).copied().unwrap_or(0), b.get(i).copied().unwrap_or(0));
        if x != y {
            return x > y;
        }
    }
    false
}

/// Whether a link may be asked: https, or a loopback address.
fn allowed(link: &str) -> bool {
    let lower = link.to_ascii_lowercase();
    lower.starts_with("https://") || lower.starts_with("http://127.0.0.1") || lower.starts_with("http://localhost")
}

/// Read the update file's text against the running version.
pub fn judge(body: &str, current: &str) -> Result<Checked, String> {
    let v: Value = serde_json::from_str(body).map_err(|_| "BAD_UPDATE_FILE".to_string())?;
    let version = v.get("version").and_then(Value::as_str).filter(|s| !s.trim().is_empty()).ok_or("BAD_UPDATE_FILE")?.trim().to_string();
    if !is_newer(&version, current) {
        return Ok(Checked::UpToDate { latest: version });
    }
    let url = v.get("url").and_then(Value::as_str).unwrap_or("").trim().to_string();
    // The link is opened by the person: only a web page, never something that runs.
    if !url.to_ascii_lowercase().starts_with("https://") {
        return Err("BAD_UPDATE_LINK".to_string());
    }
    let notes: String = v.get("notes").and_then(Value::as_str).unwrap_or("").chars().take(600).collect();
    let sha256 = match v.get("sha256").and_then(Value::as_str).map(|s| s.trim().to_ascii_lowercase()) {
        None => None,
        // A hash that is not a hash is a mistake in the file, not something to ignore.
        Some(h) if h.len() == 64 && h.bytes().all(|b| b.is_ascii_hexdigit()) => Some(h),
        Some(_) => return Err("BAD_UPDATE_FILE".to_string()),
    };
    Ok(Checked::Newer(Release { version, url, notes, sha256 }))
}

/// Ask the update file at `link` (blocks: call it off the interface thread).
pub fn check(link: &str, current: &str) -> Result<Checked, String> {
    if !allowed(link.trim()) {
        return Err("UPDATE_URL_PROTOCOL".to_string());
    }
    let agent: ureq::Agent = ureq::Agent::config_builder()
        .timeout_global(Some(Duration::from_secs(8)))
        .max_redirects(3)
        .http_status_as_error(false)
        .user_agent("Blink (a personal notch for Windows)")
        .tls_config(ureq::tls::TlsConfig::builder().provider(ureq::tls::TlsProvider::NativeTls).root_certs(ureq::tls::RootCerts::PlatformVerifier).build())
        .build()
        .into();
    let response = agent.get(link.trim()).call().map_err(|_| "UPDATE_OFFLINE".to_string())?;
    if response.status().as_u16() != 200 {
        return Err("UPDATE_NOT_FOUND".to_string());
    }
    let body = response.into_body().with_config().limit(100_000).read_to_string().map_err(|_| "UPDATE_OFFLINE".to_string())?;
    judge(&body, current)
}

/// An installer is never bigger than this.
const MAX_INSTALLER_BYTES: u64 = 120 * 1024 * 1024;

fn agent(timeout_secs: u64) -> ureq::Agent {
    ureq::Agent::config_builder()
        .timeout_global(Some(Duration::from_secs(timeout_secs)))
        .max_redirects(5)
        .http_status_as_error(false)
        .user_agent("Blink (a personal notch for Windows)")
        .tls_config(ureq::tls::TlsConfig::builder().provider(ureq::tls::TlsProvider::NativeTls).root_certs(ureq::tls::RootCerts::PlatformVerifier).build())
        .build()
        .into()
}

/// Download `link` to `dest` and check its SHA-256 is `expected` (64 hex digits, any case). The file is kept only if it matches: a
/// different file is deleted and `UPDATE_HASH_MISMATCH` comes back. Blocks (a few seconds to minutes): call it off the interface thread.
pub fn download_verified(link: &str, expected: &str, dest: &std::path::Path) -> Result<(), String> {
    use std::io::{Read, Write};
    let expected = expected.trim().to_ascii_lowercase();
    if expected.len() != 64 || !expected.bytes().all(|b| b.is_ascii_hexdigit()) {
        return Err("BAD_UPDATE_FILE".to_string());
    }
    if !allowed(link.trim()) {
        return Err("UPDATE_URL_PROTOCOL".to_string());
    }
    let response = agent(600).get(link.trim()).call().map_err(|_| "UPDATE_OFFLINE".to_string())?;
    if response.status().as_u16() != 200 {
        return Err("UPDATE_NOT_FOUND".to_string());
    }
    if let Some(parent) = dest.parent() {
        std::fs::create_dir_all(parent).map_err(|_| "UPDATE_DISK".to_string())?;
    }
    let part = dest.with_extension("part");
    let result = (|| {
        let mut file = std::fs::File::create(&part).map_err(|_| "UPDATE_DISK".to_string())?;
        let mut reader = response.into_body().into_with_config().limit(MAX_INSTALLER_BYTES).reader();
        let mut hasher = crate::sha256::Sha256::new();
        let mut buffer = vec![0u8; 64 * 1024];
        loop {
            let n = reader.read(&mut buffer).map_err(|_| "UPDATE_OFFLINE".to_string())?;
            if n == 0 {
                break;
            }
            hasher.update(&buffer[..n]);
            file.write_all(&buffer[..n]).map_err(|_| "UPDATE_DISK".to_string())?;
        }
        file.flush().map_err(|_| "UPDATE_DISK".to_string())?;
        if hasher.finish_hex() != expected {
            return Err("UPDATE_HASH_MISMATCH".to_string());
        }
        Ok(())
    })();
    match result {
        Ok(()) => {
            let _ = std::fs::remove_file(dest);
            std::fs::rename(&part, dest).map_err(|_| "UPDATE_DISK".to_string())
        }
        Err(code) => {
            let _ = std::fs::remove_file(&part);
            Err(code)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{Read, Write};
    use std::net::TcpListener;

    #[test]
    fn versions_compare_as_numbers() {
        assert!(is_newer("0.3.0", "0.2.0"));
        assert!(is_newer("0.2.10", "0.2.9"), "numbers, not text");
        assert!(is_newer("v1.0", "0.9.9"));
        assert!(!is_newer("0.2.0", "0.2.0"));
        assert!(!is_newer("0.2", "0.2.0"), "missing parts are zero");
        assert!(!is_newer("0.1.9", "0.2.0"));
        assert!(is_newer("0.2.1-beta", "0.2.0"));
    }

    #[test]
    fn the_update_file_is_judged_and_only_a_web_link_is_accepted() {
        let file = r#"{"version":"0.3.0","url":"https://example.org/Blink-0.3.0-setup.exe","notes":"Faster."}"#;
        assert_eq!(judge(file, "0.2.0"), Ok(Checked::Newer(Release { version: "0.3.0".into(), url: "https://example.org/Blink-0.3.0-setup.exe".into(), notes: "Faster.".into(), sha256: None })));
        let hash = "A".repeat(64);
        let with_hash = file.replace("\"notes\"", &format!("\"sha256\":\"{hash}\",\"notes\""));
        assert!(matches!(judge(&with_hash, "0.2.0"), Ok(Checked::Newer(r)) if r.sha256.as_deref() == Some("a".repeat(64).as_str())), "lowercased");
        assert_eq!(judge(&file.replace("\"notes\"", "\"sha256\":\"abc\",\"notes\""), "0.2.0"), Err("BAD_UPDATE_FILE".to_string()), "a hash that is not one"); 
        assert_eq!(judge(file, "0.3.0"), Ok(Checked::UpToDate { latest: "0.3.0".into() }));
        assert_eq!(judge(file, "0.4.0"), Ok(Checked::UpToDate { latest: "0.3.0".into() }), "newer than the file says is up to date");
        assert_eq!(judge(&file.replace("https://example.org/Blink-0.3.0-setup.exe", "file:///C:/Windows/System32/calc.exe"), "0.2.0"), Err("BAD_UPDATE_LINK".to_string()));
        assert_eq!(judge(&file.replace("https://example.org/Blink-0.3.0-setup.exe", "http://example.org/x.exe"), "0.2.0"), Err("BAD_UPDATE_LINK".to_string()), "http is not enough");
        assert_eq!(judge("not json", "0.2.0"), Err("BAD_UPDATE_FILE".to_string()));
        assert_eq!(judge(r#"{"url":"https://x"}"#, "0.2.0"), Err("BAD_UPDATE_FILE".to_string()));
    }

    #[test]
    fn only_https_or_this_computer_is_asked() {
        assert_eq!(check("http://example.org/latest.json", "0.2.0"), Err("UPDATE_URL_PROTOCOL".to_string()));
        assert_eq!(check("ftp://example.org/latest.json", "0.2.0"), Err("UPDATE_URL_PROTOCOL".to_string()));
        assert_eq!(check("", "0.2.0"), Err("UPDATE_URL_PROTOCOL".to_string()));
    }

    fn serve(status: &'static str, body: &'static str) -> String {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let link = format!("http://{}/latest.json", listener.local_addr().unwrap());
        std::thread::spawn(move || {
            if let Ok((mut stream, _)) = listener.accept() {
                let mut buf = [0u8; 1024];
                let _ = stream.read(&mut buf);
                let _ = write!(stream, "HTTP/1.1 {status}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len());
            }
        });
        link
    }

    #[test]
    fn a_file_on_this_computer_is_read_and_a_missing_one_is_said() {
        let link = serve("200 OK", r#"{"version":"9.9.9","url":"https://example.org/setup.exe"}"#);
        assert!(matches!(check(&link, "0.2.0"), Ok(Checked::Newer(r)) if r.version == "9.9.9"));
        let missing = serve("404 Not Found", "nope");
        assert_eq!(check(&missing, "0.2.0"), Err("UPDATE_NOT_FOUND".to_string()));
        assert_eq!(check("http://127.0.0.1:1/latest.json", "0.2.0"), Err("UPDATE_OFFLINE".to_string()));
    }

    fn serve_bytes(status: &'static str, body: &'static [u8]) -> String {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let link = format!("http://{}/Blink-setup.exe", listener.local_addr().unwrap());
        std::thread::spawn(move || {
            if let Ok((mut stream, _)) = listener.accept() {
                let mut buf = [0u8; 1024];
                let _ = stream.read(&mut buf);
                let _ = write!(stream, "HTTP/1.1 {status}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n", body.len());
                let _ = stream.write_all(body);
            }
        });
        link
    }

    #[test]
    fn a_download_is_kept_only_when_it_is_exactly_the_file_the_hash_names() {
        let dir = std::env::temp_dir().join(format!("blink-update-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let dest = dir.join("setup.exe");
        let body: &'static [u8] = b"pretend this is an installer";
        let good = crate::sha256::hex(body);
        // The right hash (in capitals, as people paste it): kept.
        let link = serve_bytes("200 OK", body);
        assert_eq!(download_verified(&link, &good.to_ascii_uppercase(), &dest), Ok(()));
        assert_eq!(std::fs::read(&dest).unwrap(), body);
        assert!(!dir.join("setup.part").exists(), "no leftovers");
        // Another file with the same name asked for: the old one is replaced only by a good one.
        let link = serve_bytes("200 OK", b"something else");
        assert_eq!(download_verified(&link, &good, &dest), Err("UPDATE_HASH_MISMATCH".to_string()));
        assert_eq!(std::fs::read(&dest).unwrap(), body, "the good file is still there");
        assert!(!dir.join("setup.part").exists(), "and the bad one is gone");
        // A missing file, a link that is not allowed, a hash that is not one.
        assert_eq!(download_verified(&serve_bytes("404 Not Found", b"no"), &good, &dir.join("x.exe")), Err("UPDATE_NOT_FOUND".to_string()));
        assert_eq!(download_verified("http://example.org/x.exe", &good, &dir.join("y.exe")), Err("UPDATE_URL_PROTOCOL".to_string()));
        assert_eq!(download_verified("https://example.org/x.exe", "nothex", &dir.join("z.exe")), Err("BAD_UPDATE_FILE".to_string()));
        let _ = std::fs::remove_dir_all(&dir);
    }
}
