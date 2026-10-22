//! Calendar feed networking. This is the only code that talks to the network:
//! the page's CSP forbids it, so requests can only go to feed URLs the user
//! saved (or typed into the settings "test link" box).
//!
//! - Conditional GET (ETag / Last-Modified) with a 15 minute re-fetch floor.
//! - Offline or failing feeds fall back to the cached copy plus an error code.
//! - URLs are never logged and never appear in error values: reqwest's own
//!   error text includes the URL, so only fixed codes are returned.

use std::sync::OnceLock;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use reqwest::header::{ACCEPT, IF_MODIFIED_SINCE, IF_NONE_MATCH};
use serde::{Deserialize, Serialize};
use tauri::AppHandle;

use crate::{secret, store};

const MAX_BYTES: usize = 8 * 1024 * 1024;
const MIN_REFETCH_MS: i64 = 15 * 60 * 1000;
/// Windows Credential Manager stores at most 2560 bytes (UTF-16).
const MAX_URL_CHARS: usize = 1200;
const USER_AGENT: &str =
    "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/131.0.0.0 Safari/537.36";

#[derive(Serialize, Clone)]
pub struct FeedError {
    code: &'static str,
    /// True when the server was never reached (offline, DNS, timeout).
    transport: bool,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FeedResult {
    ics: Option<String>,
    from_cache: bool,
    fetched_at_ms: Option<i64>,
    error: Option<FeedError>,
}

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
    Fresh {
        ics: String,
        etag: Option<String>,
        last_modified: Option<String>,
    },
}

fn client() -> &'static reqwest::Client {
    static CLIENT: OnceLock<reqwest::Client> = OnceLock::new();
    CLIENT.get_or_init(|| {
        reqwest::Client::builder()
            .timeout(Duration::from_secs(20))
            .redirect(reqwest::redirect::Policy::limited(5))
            .user_agent(USER_AGENT)
            .build()
            .expect("http client")
    })
}

fn now_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

fn fail(code: &'static str, transport: bool) -> FeedError {
    FeedError { code, transport }
}

fn transport_error(e: &reqwest::Error) -> FeedError {
    fail(if e.is_timeout() { "TIMEOUT" } else { "NETWORK" }, true)
}

fn check_id(id: &str) -> Result<(), String> {
    let ok = !id.is_empty()
        && id.len() <= 64
        && id.chars().next().is_some_and(|c| c.is_ascii_alphanumeric())
        && id
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-'));
    if ok {
        Ok(())
    } else {
        Err("ID_INVALID".to_string())
    }
}

fn check_url(url: &str) -> Result<(), String> {
    let lower = url.trim().to_ascii_lowercase();
    if url.chars().count() > MAX_URL_CHARS {
        return Err("URL_INVALID".to_string());
    }
    if lower.starts_with("https://") || lower.starts_with("http://") {
        Ok(())
    } else {
        Err("URL_PROTOCOL".to_string())
    }
}

fn cache_file(id: &str) -> String {
    format!("feed-{id}.json")
}

fn load_cache(app: &AppHandle, id: &str) -> Option<Cache> {
    let bytes = store::read_file(app, &cache_file(id))?;
    let cache: Cache = serde_json::from_slice(&bytes).ok()?;
    cache.ics.contains("BEGIN:VCALENDAR").then_some(cache)
}

fn save_cache(app: &AppHandle, id: &str, cache: &Cache) {
    if let Ok(bytes) = serde_json::to_vec(cache) {
        let _ = store::write_atomic(app, &cache_file(id), &bytes);
    }
}

async fn request(url: &str, cache: Option<&Cache>) -> Result<Fetched, FeedError> {
    let mut req = client()
        .get(url)
        .header(ACCEPT, "text/calendar, text/plain;q=0.9, */*;q=0.5");
    if let Some(c) = cache {
        if let Some(etag) = &c.etag {
            req = req.header(IF_NONE_MATCH, etag);
        }
        if let Some(modified) = &c.last_modified {
            req = req.header(IF_MODIFIED_SINCE, modified);
        }
    }

    let resp = req.send().await.map_err(|e| transport_error(&e))?;
    let status = resp.status();

    if status.as_u16() == 304 {
        return if cache.is_some() {
            Ok(Fetched::NotModified)
        } else {
            Err(fail("HTTP_ERROR", false))
        };
    }
    if status.as_u16() == 401 || status.as_u16() == 403 {
        return Err(fail("UNAUTHORIZED", false));
    }
    if !status.is_success() {
        return Err(fail("HTTP_ERROR", false));
    }
    if resp.content_length().is_some_and(|n| n as usize > MAX_BYTES) {
        return Err(fail("TOO_LARGE", false));
    }

    let header = |name: &str| {
        resp.headers()
            .get(name)
            .and_then(|v| v.to_str().ok())
            .map(str::to_string)
    };
    let etag = header("etag");
    let last_modified = header("last-modified");

    let body = resp.bytes().await.map_err(|e| transport_error(&e))?;
    if body.len() > MAX_BYTES {
        return Err(fail("TOO_LARGE", false));
    }
    let ics = String::from_utf8_lossy(&body).into_owned();
    if !ics.contains("BEGIN:VCALENDAR") {
        return Err(fail("NOT_CALENDAR", false));
    }
    Ok(Fetched::Fresh {
        ics,
        etag,
        last_modified,
    })
}

fn result_from(cache: &Cache, from_cache: bool, error: Option<FeedError>) -> FeedResult {
    FeedResult {
        ics: Some(cache.ics.clone()),
        from_cache,
        fetched_at_ms: Some(cache.fetched_at_ms),
        error,
    }
}

fn error_only(error: FeedError) -> FeedResult {
    FeedResult {
        ics: None,
        from_cache: false,
        fetched_at_ms: None,
        error: Some(error),
    }
}

/// Fetch (or serve from cache) one saved feed. Never returns the URL.
#[tauri::command]
pub async fn feed_fetch(app: AppHandle, id: String, force: bool) -> Result<FeedResult, String> {
    check_id(&id)?;
    let Some(url) = secret::get(&id) else {
        return Ok(error_only(fail("NO_SECRET", false)));
    };
    let cache = load_cache(&app, &id);
    let now = now_ms();

    if !force {
        if let Some(c) = &cache {
            if now - c.fetched_at_ms < MIN_REFETCH_MS {
                return Ok(result_from(c, true, None));
            }
        }
    }

    match request(&url, cache.as_ref()).await {
        Ok(Fetched::NotModified) => {
            let mut c = cache.expect("304 is only accepted with a cache");
            c.fetched_at_ms = now;
            save_cache(&app, &id, &c);
            Ok(result_from(&c, false, None))
        }
        Ok(Fetched::Fresh {
            ics,
            etag,
            last_modified,
        }) => {
            let c = Cache {
                etag,
                last_modified,
                fetched_at_ms: now,
                ics,
            };
            save_cache(&app, &id, &c);
            Ok(result_from(&c, false, None))
        }
        Err(error) => Ok(match cache {
            Some(c) => result_from(&c, true, Some(error)),
            None => error_only(error),
        }),
    }
}

/// Fetch a URL the user typed, without saving anything (settings "Test link").
#[tauri::command]
pub async fn feed_test(url: String) -> Result<FeedResult, String> {
    check_url(&url)?;
    Ok(match request(url.trim(), None).await {
        Ok(Fetched::Fresh { ics, .. }) => FeedResult {
            ics: Some(ics),
            from_cache: false,
            fetched_at_ms: Some(now_ms()),
            error: None,
        },
        Ok(Fetched::NotModified) => error_only(fail("HTTP_ERROR", false)),
        Err(error) => error_only(error),
    })
}

/// Store the URL in Credential Manager and drop any cache from an older URL.
#[tauri::command]
pub fn feed_save(app: AppHandle, id: String, url: String) -> Result<(), String> {
    check_id(&id)?;
    check_url(&url)?;
    secret::set(&id, url.trim())?;
    store::remove_file(&app, &cache_file(&id));
    Ok(())
}

#[tauri::command]
pub fn feed_remove(app: AppHandle, id: String) -> Result<(), String> {
    check_id(&id)?;
    secret::delete(&id);
    store::remove_file(&app, &cache_file(&id));
    Ok(())
}

#[tauri::command]
pub fn feed_has_secret(id: String) -> bool {
    check_id(&id).is_ok() && secret::get(&id).is_some()
}
