//! Calendar feed networking. The only code that talks to the network, and only
//! to feed URLs the user saved (or the one being tested in settings).
//!
//! - Conditional GET (ETag / Last-Modified) with a 15 minute re-fetch floor.
//! - A failing or offline feed falls back to the cached copy plus an error code.
//! - URLs are never logged and never appear in errors: library error text can
//!   include the URL, so only fixed codes are returned.
//!
//! Ported from src-tauri/src/feed.rs (async reqwest) to blocking ureq.

use std::time::{Duration, SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};

use crate::secret::Secrets;
use crate::store::Store;

const MAX_BYTES: u64 = 8 * 1024 * 1024;
const MIN_REFETCH_MS: i64 = 15 * 60 * 1000;
/// Windows Credential Manager stores at most 2560 bytes (UTF-16).
const MAX_URL_CHARS: usize = 1200;
const USER_AGENT: &str =
    "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/131.0.0.0 Safari/537.36";

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct FeedError {
    pub code: &'static str,
    /// True when the server was never reached (offline, DNS, timeout).
    pub transport: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FeedResult {
    pub ics: Option<String>,
    pub from_cache: bool,
    pub fetched_at_ms: Option<i64>,
    pub error: Option<FeedError>,
}

/// On disk as `feed-<id>.json`, same shape the WebView2 app wrote.
#[derive(Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
struct Cache {
    etag: Option<String>,
    last_modified: Option<String>,
    fetched_at_ms: i64,
    ics: String,
}

enum Fetched {
    NotModified,
    Fresh { ics: String, etag: Option<String>, last_modified: Option<String> },
}

fn fail(code: &'static str, transport: bool) -> FeedError {
    FeedError { code, transport }
}

pub fn check_id(id: &str) -> Result<(), &'static str> {
    let ok = !id.is_empty()
        && id.len() <= 64
        && id.chars().next().is_some_and(|c| c.is_ascii_alphanumeric())
        && id.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-'));
    if ok { Ok(()) } else { Err("ID_INVALID") }
}

pub fn check_url(url: &str) -> Result<(), &'static str> {
    if url.chars().count() > MAX_URL_CHARS {
        return Err("URL_INVALID");
    }
    let lower = url.trim().to_ascii_lowercase();
    if lower.starts_with("https://") || lower.starts_with("http://") { Ok(()) } else { Err("URL_PROTOCOL") }
}

fn cache_file(id: &str) -> String {
    format!("feed-{id}.json")
}

fn real_now_ms() -> i64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_millis() as i64).unwrap_or(0)
}

fn error_only(error: FeedError) -> FeedResult {
    FeedResult { ics: None, from_cache: false, fetched_at_ms: None, error: Some(error) }
}

fn result_from(cache: &Cache, from_cache: bool, error: Option<FeedError>) -> FeedResult {
    FeedResult { ics: Some(cache.ics.clone()), from_cache, fetched_at_ms: Some(cache.fetched_at_ms), error }
}

pub struct FeedService<S: Secrets> {
    store: Store,
    secrets: S,
    agent: ureq::Agent,
    now_ms: Box<dyn Fn() -> i64 + Send + Sync>,
}

impl<S: Secrets> FeedService<S> {
    pub fn new(store: Store, secrets: S) -> Self {
        Self::with_timeout(store, secrets, Duration::from_secs(20))
    }

    pub fn with_timeout(store: Store, secrets: S, timeout: Duration) -> Self {
        let config = ureq::Agent::config_builder()
            .timeout_global(Some(timeout))
            .max_redirects(5)
            .http_status_as_error(false)
            .user_agent(USER_AGENT)
            .tls_config(ureq::tls::TlsConfig::builder().provider(ureq::tls::TlsProvider::NativeTls).build())
            .build();
        FeedService { store, secrets, agent: config.into(), now_ms: Box::new(real_now_ms) }
    }

    /// Replace the clock (tests).
    pub fn with_clock(mut self, now_ms: impl Fn() -> i64 + Send + Sync + 'static) -> Self {
        self.now_ms = Box::new(now_ms);
        self
    }

    fn load_cache(&self, id: &str) -> Option<Cache> {
        let cache: Cache = serde_json::from_slice(&self.store.read(&cache_file(id))?).ok()?;
        cache.ics.contains("BEGIN:VCALENDAR").then_some(cache)
    }

    fn save_cache(&self, id: &str, cache: &Cache) {
        if let Ok(bytes) = serde_json::to_vec(cache) {
            let _ = self.store.write_atomic(&cache_file(id), &bytes);
        }
    }

    fn request(&self, url: &str, cache: Option<&Cache>) -> Result<Fetched, FeedError> {
        let mut req = self.agent.get(url).header("Accept", "text/calendar, text/plain;q=0.9, */*;q=0.5");
        if let Some(c) = cache {
            if let Some(etag) = &c.etag {
                req = req.header("If-None-Match", etag.as_str());
            }
            if let Some(modified) = &c.last_modified {
                req = req.header("If-Modified-Since", modified.as_str());
            }
        }
        let resp = req.call().map_err(|e| match e {
            ureq::Error::Timeout(_) => fail("TIMEOUT", true),
            _ => fail("NETWORK", true),
        })?;

        let status = resp.status().as_u16();
        if status == 304 {
            return if cache.is_some() { Ok(Fetched::NotModified) } else { Err(fail("HTTP_ERROR", false)) };
        }
        if status == 401 || status == 403 {
            return Err(fail("UNAUTHORIZED", false));
        }
        if !(200..300).contains(&status) {
            return Err(fail("HTTP_ERROR", false));
        }
        let header = |name: &str| resp.headers().get(name).and_then(|v| v.to_str().ok()).map(str::to_string);
        if header("content-length").and_then(|v| v.parse::<u64>().ok()).is_some_and(|n| n > MAX_BYTES) {
            return Err(fail("TOO_LARGE", false));
        }
        let (etag, last_modified) = (header("etag"), header("last-modified"));

        let mut body = resp.into_body();
        let bytes = body.with_config().limit(MAX_BYTES).read_to_vec().map_err(|e| match e {
            ureq::Error::BodyExceedsLimit(_) => fail("TOO_LARGE", false),
            ureq::Error::Timeout(_) => fail("TIMEOUT", true),
            _ => fail("NETWORK", true),
        })?;
        let ics = String::from_utf8_lossy(&bytes).into_owned();
        if !ics.contains("BEGIN:VCALENDAR") {
            return Err(fail("NOT_CALENDAR", false));
        }
        Ok(Fetched::Fresh { ics, etag, last_modified })
    }

    /// Fetch (or serve from cache) one saved feed. Never returns the URL.
    pub fn fetch(&self, id: &str, force: bool) -> Result<FeedResult, &'static str> {
        check_id(id)?;
        let Some(url) = self.secrets.get(id) else { return Ok(error_only(fail("NO_SECRET", false))) };
        let cache = self.load_cache(id);
        let now = (self.now_ms)();

        if !force {
            if let Some(c) = &cache {
                if now - c.fetched_at_ms < MIN_REFETCH_MS {
                    return Ok(result_from(c, true, None));
                }
            }
        }
        Ok(match self.request(&url, cache.as_ref()) {
            Ok(Fetched::NotModified) => {
                let mut c = cache.expect("304 is only accepted with a cache");
                c.fetched_at_ms = now;
                self.save_cache(id, &c);
                result_from(&c, false, None)
            }
            Ok(Fetched::Fresh { ics, etag, last_modified }) => {
                let c = Cache { etag, last_modified, fetched_at_ms: now, ics };
                self.save_cache(id, &c);
                result_from(&c, false, None)
            }
            Err(error) => match cache {
                Some(c) => result_from(&c, true, Some(error)),
                None => error_only(error),
            },
        })
    }

    /// Fetch a URL the user typed, without saving anything (settings "Test link").
    pub fn test_url(&self, url: &str) -> Result<FeedResult, &'static str> {
        check_url(url)?;
        Ok(match self.request(url.trim(), None) {
            Ok(Fetched::Fresh { ics, .. }) => FeedResult {
                ics: Some(ics),
                from_cache: false,
                fetched_at_ms: Some((self.now_ms)()),
                error: None,
            },
            Ok(Fetched::NotModified) => error_only(fail("HTTP_ERROR", false)),
            Err(error) => error_only(error),
        })
    }

    /// Store the URL and drop any cache from an older URL.
    pub fn save(&self, id: &str, url: &str) -> Result<(), &'static str> {
        check_id(id)?;
        check_url(url)?;
        self.secrets.set(id, url.trim())?;
        self.store.remove(&cache_file(id));
        Ok(())
    }

    pub fn remove(&self, id: &str) -> Result<(), &'static str> {
        check_id(id)?;
        self.secrets.delete(id);
        self.store.remove(&cache_file(id));
        Ok(())
    }

    pub fn has_secret(&self, id: &str) -> bool {
        check_id(id).is_ok() && self.secrets.get(id).is_some()
    }
}