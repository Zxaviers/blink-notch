# Deadline Notch (working title)

A Windows overlay that turns the top-center edge of the screen into a small notch. At rest it shows the nearest deadline and its countdown. Hover it and it expands into a deadline list. Deadlines come from calendar links you add (Moodle/BRONE, Google Calendar secret iCal address, or any `.ics` URL) and from events you type in yourself.

Requirements and plan live in the PRD ("PRD Deadline Notch (working title)") and `HANDOFF.md`. This repo is a clean start; the Reminder Widget v1.2 repo is only a reference for the feed parser and tests.

## Status: v0.1 in progress

Built: notch shell (click-through, hover/peek/open, fullscreen hiding, multi-monitor placement), deadlines module, feed fetching with conditional requests, notifications at configurable thresholds, mark done, personal events, snooze, global hotkey with keyboard navigation, tray, settings window, first-run wizard, English and Indonesian.

Not built yet: group by course, focus timer, to-do, media module, system peeks.

## Keyboard

Ctrl+Alt+D (rebindable in Settings, Display; can be turned off) opens the notch and moves keyboard focus into it. Inside: Up/Down/Home/End select a deadline, Enter opens its page, D (or Space) marks it done, S then 1/2/3 snoozes it for 30 minutes, 2 hours or until tomorrow 08:00, Esc closes (the first Esc only closes the snooze chooser). Pressing the hotkey again, clicking another app or Alt+Tab also closes it, and focus goes back to the app you were in. Without the hotkey the notch never takes focus.

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
- `scripts/e2e-hotkey.ps1`: sends the real hotkey and navigation keys, checks focus goes in and comes back, and that D marks a deadline done (real keystrokes, so keep your hands off the keyboard too).
- `scripts/mem-probe.ps1` (memory by WebView2 process, flag trials) and `scripts/anim-cost.ps1` (CPU per expand and collapse).
- `scripts/startup-time.ps1` (launch to window visible and to content ready) and `scripts/stress-cost.ps1` (CPU of sustained animation variants; `DN_STRESS` loads `src/stress.js`, which is never loaded otherwise). Both use the shared `scripts/profile.ps1` test profile and need no cursor movement.
- `scripts/measure-idle.ps1 -Minutes 30`: samples CPU and memory of the app plus its WebView2 processes.

All three back up and restore `%APPDATA%\dev.riski.deadlinenotch`.

## Measured so far

Release exe, resting, one fixture feed loaded, 5 minutes (28 samples, 16 logical cores):

- CPU 0.05% average, 0.17% peak (PRD target: under 0.5%).
- Memory as Task Manager's Memory column shows it (private working set summed over the app and its six WebView2 processes): 62.7 MB average, 66.2 MB peak (PRD target: under 80 MB).

How memory got there: the first full build sat at about 97 MB. A breakdown by WebView2 process (`scripts/mem-probe.ps1`) showed the GPU process at 38 MB. Starting WebView2 with `--disable-gpu` (software rendering) took the total to 66 MB. On their own, `--in-process-gpu` reached 86 MB and `--enable-low-end-device-mode` changed nothing; on top of `--disable-gpu`, `--in-process-gpu` and extra `--disable-features` saved only 2 to 3 MB, so they were not adopted. The flags are in `src-tauri/tauri.conf.json` and `WEBVIEW_ARGS` in `lib.rs`; both windows must use the same string or WebView2 starts a second set of processes.

What software rendering costs: an expand plus collapse uses about 0.3 to 0.4 CPU-seconds across the app and WebView2 (about 0.01 with the GPU), mostly in the GPU process doing software compositing. Frame pacing of the expand animation, measured in the app (DN_DEBUG=1 logs `frames open: ...`): median 6.9 ms between frames on a 144 Hz display, none over 20 ms. That is one machine; check a low-end laptop before calling it done.

Startup (`scripts/startup-time.ps1`, five launches, release exe): the notch window is visible after a median 0.65 s and the page has its settings loaded and first feed synced after 0.9 s (PRD target: under 1.5 s). The first launch of a batch is slowest (1.2 s).

Sustained animation (`scripts/stress-cost.ps1`, panel open and something moving every frame, 10 s windows, repeated; 100% = one full core of 16): with the shipped software rendering it costs about 9 to 13% of a core whatever is animating (transform, width, text, scrolling list). With the GPU enabled (a comparison build, not shipped) a transform or text animation costs 1 to 2%, a width animation still 11%, and the scrolling list 32 to 34%, so the GPU is not uniformly cheaper and memory goes back to about 97 MB. Only the open panel animates, so a resting notch with music playing would still draw nothing. Takeaway: a native build would mainly save memory; sustained animation under WebView2 is affordable if modules update coarsely (about once a second) except while the panel is open.

Native checks on the release exe (`scripts/probe.ps1`, four runs in a row): click-through outside the notch, input on the pill and expanded area, no focus stolen by hover or a real click. Not yet checked on real hardware: fullscreen hiding, multi-monitor and DPI changes, Windows toasts, and a long soak (the full app has only been sampled for 5 minutes).