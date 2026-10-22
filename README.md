# Deadline Notch (working title)

A Windows overlay that turns the top-center edge of the screen into a small notch. At rest it shows the nearest deadline and its countdown. Hover it and it expands into a deadline list. Deadlines come from calendar links you add (Moodle/BRONE, Google Calendar secret iCal address, or any `.ics` URL) and from events you type in yourself.

Requirements and plan live in the PRD ("PRD Deadline Notch (working title)") and `HANDOFF.md`. This repo is a clean start; the Reminder Widget v1.2 repo is only a reference for the feed parser and tests.

## Status: v0.1 in progress

Built: notch shell (click-through, hover/peek/open, fullscreen hiding, multi-monitor placement), deadlines module, feed fetching with conditional requests, notifications at configurable thresholds, mark done, personal events, tray, settings window, first-run wizard, English and Indonesian.

Not built yet: snooze, global hotkey, settings export/import, diagnostics, group by course, focus timer, to-do, media module, system peeks.

## Privacy

- No account, no analytics.
- Feed links carry access tokens, so they live in Windows Credential Manager. They are never written to settings, caches or logs, and the page never sees them again after saving.
- The page has no network access (CSP `default-src 'self'`). Only the Rust side fetches, and only the feed links you saved (or the one you are testing).

## Develop

Prerequisites: Rust (MSVC toolchain), Visual Studio Build Tools with the C++ workload, WebView2 runtime, Node 20+.

```bash
npm install
npm test                  # unit tests for the pure modules in src/lib
npm run build:debug       # debug exe at src-tauri/target/debug/deadline-notch.exe
npx tauri build --no-bundle   # optimized exe at src-tauri/target/release/
```

The frontend is plain ES modules with no bundler; the frontend assets are embedded at build time, so rebuild after editing `src/`.

## Layout

- `src/lib/` pure logic with tests: ICS parser, feed merging, done store, personal events, notification planner, countdown text, settings validation, i18n, view model.
- `src/core.js` loads data, syncs feeds, plans notifications. `src/notch.js` is the shell. `src/modules/deadlines.js` is the first module (the internal module contract is `rest`, `peek`, `panel`).
- `src/settings.*` is the settings window and the first-run wizard.
- `src-tauri/src/` Rust: `feed.rs` (network), `secret.rs` (Credential Manager), `store.rs` (atomic JSON files), `win32.rs` (overlay window and fullscreen detection), `lib.rs` (window, tray, placement, commands).

## Checks that drive the real app (Windows)

These move the real cursor for a few seconds, so keep your hands off the mouse.

- `scripts/probe.ps1`: click-through, hover, focus-stealing checks.
- `scripts/e2e.ps1`: seeds a throwaway profile and a fixture feed (`scripts/serve-fixture.mjs`), hovers the notch and saves screenshots to `scripts/out/`.
- `scripts/e2e-settings.ps1`: first-run wizard opens.
- `scripts/measure-idle.ps1 -Minutes 30`: samples CPU and memory of the app plus its WebView2 processes.

All three back up and restore `%APPDATA%\dev.riski.deadlinenotch`.

## Measured so far

Release exe, resting, one fixture feed loaded, 3 minutes (16 samples, 16 logical cores): CPU 0.03% average (0.15% peak). Memory as Task Manager's Memory column shows it (private working set summed over the app and its six WebView2 processes): 97 MB average, 103 MB peak. The PRD targets are under 0.5% CPU and under 80 MB, so CPU passes and memory is over. The exe alone is about 4 MB; the rest is WebView2.

Earlier, on the spike shell with mock data (25 minutes): 0.07% CPU, about 90 MB, flat. A 30-minute soak of the full app has not been run yet.

Native checks on the release exe (`scripts/probe.ps1`): click-through outside the notch, input on the pill and expanded area, no focus stolen by hover or a real click. Not yet checked on real hardware: fullscreen hiding, multi-monitor and DPI changes, Windows toasts.
