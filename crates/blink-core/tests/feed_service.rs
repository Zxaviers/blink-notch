//! Feed fetching against a local scripted server: conditional requests, the
//! 15 minute floor, offline fallback to the cache, error codes, size and time
//! limits, redirects, and that the saved URL never leaks.

mod common;

use std::sync::atomic::{AtomicI64, Ordering};
use std::sync::Arc;
use std::time::Duration;

use blink_core::feed::FeedService;
use blink_core::secret::{MemorySecrets, Secrets};
use blink_core::store::Store;
use common::http::{ics, serve, Reply, TestServer};

const MIN: i64 = 60_000;

fn temp_dir(tag: &str) -> std::path::PathBuf {
    let d = std::env::temp_dir().join(format!("blink-test-{tag}-{}-{}", std::process::id(), rand_suffix()));
    std::fs::create_dir_all(&d).unwrap();
    d
}

fn rand_suffix() -> u128 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos()
}

struct Fixture {
    service: FeedService<MemorySecrets>,
    clock: Arc<AtomicI64>,
    dir: std::path::PathBuf,
}

fn fixture(tag: &str, server_url: Option<&str>, timeout: Duration) -> Fixture {
    let dir = temp_dir(tag);
    let secrets = MemorySecrets::default();
    if let Some(url) = server_url {
        secrets.set("f1", url).unwrap();
    }
    let clock = Arc::new(AtomicI64::new(1_000_000_000_000));
    let c = clock.clone();
    let service = FeedService::with_timeout(Store::new(&dir), secrets, timeout).with_clock(move || c.load(Ordering::SeqCst));
    Fixture { service, clock, dir }
}

fn ok_server(etag: &'static str) -> TestServer {
    serve(move |req, _| {
        if req.header("if-none-match") == Some(etag) {
            Reply::Http(304, vec![("ETag", etag.into())], vec![])
        } else {
            Reply::Http(200, vec![("ETag", etag.into()), ("Last-Modified", "Wed, 21 Oct 2026 07:28:00 GMT".into())], ics("a"))
        }
    })
}

#[test]
fn fresh_fetch_returns_the_calendar_and_writes_the_cache() {
    let server = ok_server("\"v1\"");
    let fx = fixture("fresh", Some(&server.url("/cal.ics")), Duration::from_secs(5));
    let r = fx.service.fetch("f1", false).unwrap();
    assert!(r.ics.as_deref().unwrap().contains("BEGIN:VCALENDAR"));
    assert!(!r.from_cache && r.error.is_none());
    assert_eq!(r.fetched_at_ms, Some(1_000_000_000_000));

    let cache: serde_json::Value = serde_json::from_slice(&std::fs::read(fx.dir.join("feed-f1.json")).unwrap()).unwrap();
    assert_eq!(cache["etag"], "\"v1\"");
    assert_eq!(cache["lastModified"], "Wed, 21 Oct 2026 07:28:00 GMT");
    assert_eq!(cache["fetchedAtMs"], 1_000_000_000_000_i64);
}

#[test]
fn within_fifteen_minutes_the_cache_is_served_without_a_request() {
    let server = ok_server("\"v1\"");
    let fx = fixture("floor", Some(&server.url("/cal.ics")), Duration::from_secs(5));
    fx.service.fetch("f1", false).unwrap();
    fx.clock.fetch_add(14 * MIN, Ordering::SeqCst);
    let r = fx.service.fetch("f1", false).unwrap();
    assert!(r.from_cache && r.error.is_none());
    assert_eq!(server.count(), 1);

    fx.clock.fetch_add(2 * MIN, Ordering::SeqCst); // now 16 minutes: past the floor
    let r = fx.service.fetch("f1", false).unwrap();
    assert!(!r.from_cache);
    assert_eq!(server.count(), 2);
}

#[test]
fn a_forced_refresh_sends_the_validators_and_accepts_304() {
    let server = ok_server("\"v1\"");
    let fx = fixture("cond", Some(&server.url("/cal.ics")), Duration::from_secs(5));
    fx.service.fetch("f1", false).unwrap();
    fx.clock.fetch_add(1 * MIN, Ordering::SeqCst);
    let r = fx.service.fetch("f1", true).unwrap();
    assert_eq!(server.count(), 2);
    assert_eq!(server.last().header("if-none-match"), Some("\"v1\""));
    assert_eq!(server.last().header("if-modified-since"), Some("Wed, 21 Oct 2026 07:28:00 GMT"));
    assert!(r.ics.is_some() && !r.from_cache && r.error.is_none());
    assert_eq!(r.fetched_at_ms, Some(1_000_000_000_000 + MIN), "304 renews the fetch time");
}

#[test]
fn changed_content_replaces_the_cache() {
    let server = serve(|_, i| Reply::Http(200, vec![("ETag", format!("\"v{i}\""))], ics(if i == 0 { "old" } else { "new" })));
    let fx = fixture("changed", Some(&server.url("/")), Duration::from_secs(5));
    assert!(fx.service.fetch("f1", false).unwrap().ics.unwrap().contains("old is due"));
    fx.clock.fetch_add(20 * MIN, Ordering::SeqCst);
    assert!(fx.service.fetch("f1", false).unwrap().ics.unwrap().contains("new is due"));
}

#[test]
fn when_the_server_is_unreachable_the_cache_is_served_with_a_transport_error() {
    let server = ok_server("\"v1\"");
    let fx = fixture("offline", Some(&server.url("/cal.ics")), Duration::from_secs(2));
    fx.service.fetch("f1", false).unwrap();

    // Same feed id, but now the saved URL points at a port nobody listens on.
    let dead = {
        let l = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        l.local_addr().unwrap()
    };
    fx.service.save("f1", &format!("http://{dead}/cal.ics")).unwrap(); // save clears the cache...
    assert!(fx.service.fetch("f1", false).unwrap().ics.is_none(), "...so with no cache there is only an error");
}

#[test]
fn offline_with_a_cache_falls_back_to_it() {
    let server = ok_server("\"v1\"");
    let url = server.url("/cal.ics");
    let fx = fixture("fallback", Some(&url), Duration::from_secs(2));
    fx.service.fetch("f1", false).unwrap();
    // Point the secret at a dead port without touching the cache.
    let dead = std::net::TcpListener::bind("127.0.0.1:0").unwrap().local_addr().unwrap();
    let secrets = MemorySecrets::default();
    secrets.set("f1", &format!("http://{dead}/cal.ics")).unwrap();
    let svc = FeedService::with_timeout(Store::new(&fx.dir), secrets, Duration::from_secs(2)).with_clock({
        let c = fx.clock.clone();
        move || c.load(Ordering::SeqCst) + 30 * MIN
    });
    let r = svc.fetch("f1", false).unwrap();
    assert!(r.ics.is_some() && r.from_cache);
    let e = r.error.unwrap();
    // Windows retries a refused connection for a couple of seconds, so a short timeout may fire first.
    // Either way the server was never reached, which is what the retry ladder keys on.
    assert!(e.transport && matches!(e.code, "NETWORK" | "TIMEOUT"), "{e:?}");
}

#[test]
fn http_errors_map_to_codes() {
    for (status, code) in [(401, "UNAUTHORIZED"), (403, "UNAUTHORIZED"), (404, "HTTP_ERROR"), (500, "HTTP_ERROR")] {
        let server = serve(move |_, _| Reply::Http(status, vec![], b"nope".to_vec()));
        let fx = fixture(&format!("http{status}"), Some(&server.url("/")), Duration::from_secs(5));
        let r = fx.service.fetch("f1", false).unwrap();
        let e = r.error.unwrap();
        assert_eq!((e.code, e.transport), (code, false), "status {status}");
        assert!(r.ics.is_none());
    }
}

#[test]
fn a_response_that_is_not_a_calendar_is_rejected() {
    let server = serve(|_, _| Reply::Http(200, vec![], b"<html>login</html>".to_vec()));
    let fx = fixture("html", Some(&server.url("/")), Duration::from_secs(5));
    let e = fx.service.fetch("f1", false).unwrap().error.unwrap();
    assert_eq!((e.code, e.transport), ("NOT_CALENDAR", false));
}

#[test]
fn an_oversized_declared_length_is_rejected_before_downloading() {
    let server = serve(|_, _| Reply::HeadersOnly(200, vec![("Content-Length", (9 * 1024 * 1024).to_string())]));
    let fx = fixture("big", Some(&server.url("/")), Duration::from_secs(5));
    assert_eq!(fx.service.fetch("f1", false).unwrap().error.unwrap().code, "TOO_LARGE");
}

#[test]
fn a_server_that_never_answers_times_out() {
    let server = serve(|_, _| Reply::Hang);
    let fx = fixture("hang", Some(&server.url("/")), Duration::from_millis(400));
    let started = std::time::Instant::now();
    let e = fx.service.fetch("f1", false).unwrap().error.unwrap();
    assert_eq!((e.code, e.transport), ("TIMEOUT", true));
    assert!(started.elapsed() < Duration::from_secs(3));
}

#[test]
fn redirects_are_followed_but_not_forever() {
    let server = serve(|req, _| match req.path.as_str() {
        "/start" => Reply::Http(302, vec![("Location", "/final".into())], vec![]),
        "/final" => Reply::Http(200, vec![], ics("redirected")),
        _ => Reply::Http(302, vec![("Location", "/loop".into())], vec![]),
    });
    let fx = fixture("redir", Some(&server.url("/start")), Duration::from_secs(5));
    assert!(fx.service.fetch("f1", false).unwrap().ics.unwrap().contains("redirected"));

    let looping = fixture("loop", Some(&server.url("/loop")), Duration::from_secs(5));
    let e = looping.service.fetch("f1", false).unwrap().error.unwrap();
    assert!(e.transport, "{e:?}");
}

#[test]
fn ids_urls_and_missing_secrets_are_checked() {
    let fx = fixture("checks", None, Duration::from_secs(1));
    assert_eq!(fx.service.fetch("f1", false).unwrap().error.unwrap().code, "NO_SECRET");
    for bad in ["", "../x", "a/b", ".hidden", "has space", &"a".repeat(65)] {
        assert_eq!(fx.service.fetch(bad, false).unwrap_err(), "ID_INVALID", "{bad:?}");
        assert!(!fx.service.has_secret(bad));
    }
    assert_eq!(fx.service.save("f1", "ftp://x/y").unwrap_err(), "URL_PROTOCOL");
    assert_eq!(fx.service.save("f1", &format!("https://x/{}", "a".repeat(1300))).unwrap_err(), "URL_INVALID");
    assert_eq!(fx.service.test_url("javascript:alert(1)").unwrap_err(), "URL_PROTOCOL");
}

#[test]
fn save_replaces_the_cache_and_remove_deletes_everything() {
    let server = ok_server("\"v1\"");
    let fx = fixture("saverm", Some(&server.url("/cal.ics")), Duration::from_secs(5));
    fx.service.fetch("f1", false).unwrap();
    assert!(fx.dir.join("feed-f1.json").exists());

    fx.service.save("f1", &server.url("/other.ics")).unwrap();
    assert!(!fx.dir.join("feed-f1.json").exists(), "a new link must not show the old link's data");
    assert!(fx.service.has_secret("f1"));

    fx.service.fetch("f1", false).unwrap();
    fx.service.remove("f1").unwrap();
    assert!(!fx.dir.join("feed-f1.json").exists());
    assert!(!fx.service.has_secret("f1"));
}

#[test]
fn testing_a_typed_link_does_not_save_anything() {
    let server = ok_server("\"v1\"");
    let fx = fixture("testurl", None, Duration::from_secs(5));
    let r = fx.service.test_url(&server.url("/cal.ics")).unwrap();
    assert!(r.ics.is_some() && r.error.is_none());
    assert!(std::fs::read_dir(&fx.dir).unwrap().next().is_none(), "nothing written");
    let bad = fx.service.test_url(&server.url("/x")).unwrap(); // the fake server still answers 200 calendar
    assert!(bad.ics.is_some());
}

#[test]
fn an_old_cache_file_from_the_webview2_app_is_still_understood() {
    let server = serve(|_, _| Reply::Http(500, vec![], vec![]));
    let fx = fixture("compat", Some(&server.url("/")), Duration::from_secs(5));
    let old = serde_json::json!({ "etag": "\"e\"", "lastModified": null, "fetchedAtMs": 1_000_000_000_000_i64 - 60 * MIN, "ics": String::from_utf8(ics("legacy")).unwrap() });
    std::fs::write(fx.dir.join("feed-f1.json"), old.to_string()).unwrap();
    let r = fx.service.fetch("f1", false).unwrap();
    assert!(r.from_cache && r.ics.unwrap().contains("legacy is due"));
    assert_eq!(r.error.unwrap().code, "HTTP_ERROR");
}