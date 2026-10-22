# blink-core

The logic of Blink with no UI and no windows: calendar parsing, task merging, done state, personal events, the notification planner, countdown and label text, translations, settings validation, JSON stores, Windows Credential Manager access and blocking feed fetching. Shared by the native notch and the settings program.

It is a Rust port of `src/lib/*.js` (the WebView2 app). While the JavaScript still exists it is the oracle:

```bash
node crates/blink-core/tests/diff/gen-ics.mjs     # ICS corpus -> tests/fixtures/ics-*.json
node crates/blink-core/tests/diff/gen-ops.mjs     # small-module calls -> tests/fixtures/ops-*.json
cargo test -p blink-core                          # Rust must reproduce every expected output exactly
```

Both generators run the JS once per machine time zone (Asia/Jakarta, America/New_York, Europe/London, Australia/Lord_Howe) with `TZ` set, because floating times, all-day events, "tomorrow 08:00" and day labels depend on the zone. The Rust tests pass each zone explicitly, so they do not depend on the machine they run on. Fixtures are committed; regenerate them only when adding a case.

`tests/diff/ops-cases.mjs` is where cases are added. A value like `{"$localWall": "2026-10-19T12:00:00", "plusMs": 0}` means "that wall-clock time in the zone under test"; use it for anything that depends on the zone, because the case list is written once.

## Known differences from the JavaScript

- Titles with the same due time are ordered by a simple case-insensitive comparison, not ICU collation.
- A description cut inside a surrogate pair drops the half pair (a Rust `String` cannot hold a lone surrogate).
- Personal events and feed tasks need a string id; a malformed event without one is dropped.
- Translations were imported once from `src/lib/i18n.js` into `src/i18n_data.rs`; after cutover that file is edited by hand and `gen-i18n.mjs` is retired.
- The hotkey page helpers that read browser key events (`acceleratorFromEvent`, `nextSelection`, `keyAction`) stay in JavaScript for the settings page; the native notch reads Windows key codes instead.

## What the tests cover

- `ics_diff`: 102 calendars (UTC, TZID including daylight-saving gaps and overlaps, aliases, floating, all-day, folding, escapes, HTML, courses, URLs, windows) in 4 zones.
- `ops_diff`: about 1,800 calls (thresholds, feeds, URL checks, hotkey validation, settings, merge, done store, personal events, planner, snooze, view, translations in both languages, countdown and day labels, formatting, footer status) in 4 zones.
- `feed_service`: conditional requests, the 15 minute floor, offline fallback, error codes, size and time limits, redirects, id and URL checks, cache compatibility with the WebView2 app, against a local scripted server.
- `store_secret`: atomic JSON writes, the name allowlist, corrupt files, and a real Credential Manager round trip.

The differential tests were each confirmed to fail when the Rust side is deliberately broken.