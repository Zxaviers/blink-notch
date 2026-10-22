# Plan: moving the notch (now called Blink) to native, settings on WebView2 on demand

Status: proposal, 2026-10-23. Nothing in this plan has been started. It follows the native spike (`spikes/native-notch`, commit 832b127) and the measurements in the README.

## 1. Why, and what "native" means here

The always-running part of the app is the notch. On WebView2 it costs about 63 MB at rest and 0.65 s to appear, almost all of it the web engine's fixed overhead (our own process is 5.6 MB). The spike showed a native notch at 3.9 MB and 47 ms with the same visuals, hover timings and animation pacing.

"Native" here means:

- The notch is a single Rust process that draws itself (tiny-skia on the CPU into a layered window, as in the spike). No web engine, no GPU driver.
- The settings window stays the existing HTML/JS UI on WebView2, but as a **separate small program** that is started only when you open settings and exits when you close it. Its ~60 MB exists only while it is open, and the always-running process never loads WebView2.

Not in scope: changing what the product does. The PRD scope, the data files and the Credential Manager entries stay as they are, so an existing install upgrades in place.

## 2. Target architecture

```
blink.exe                   always running, native, target <= 15 MB at rest
  blink-core  (library)     pure logic + feed fetch + stores + Credential Manager
  platform    (win32)       window, tray, toast, hotkey, fullscreen, autostart, single instance, IPC server
  ui kit      (tiny-skia)   text, layout helpers, lists, buttons, progress, animation, hit testing
  modules     (trait)       deadlines first; focus timer, media, system peeks later
        ^
        | named pipe (JSON lines), local user only
        v
deadline-blink-settings.exe on demand, Tauri + WebView2, exits when closed
  src/settings.*            the existing page, unchanged apart from the bridge
  blink-core  (library)     the same crate: feed test/save/remove, autostart, diagnostics
```

Repository layout (a Cargo workspace; the current Tauri app keeps working until cutover):

```
crates/blink-core        pure logic and stores (no UI, no windows)
crates/blink-native      the always-running exe
crates/blink-settings    the on-demand Tauri exe (today's src-tauri, minus the notch window)
src/                     settings page (JS) stays; notch JS is retired at cutover
spikes/native-notch      stays as the reference for the renderer
```

### Decisions and the alternatives I considered

| Decision | Why | Rejected |
|---|---|---|
| Separate settings **process** | Memory is returned in full when it closes; the main process never links WebView2 | Embedding WebView2 in the main exe (its memory is not reliably given back after the window closes, and the main process would still load the engine's libraries; I have not measured this, it is the reason to avoid the risk) |
| Named-pipe IPC (JSON lines, per-user) | Settings needs answers: "hotkey already in use", "feed saved", "reload now". Files alone cannot return errors | Watching files only |
| Same data files and credential names | Zero migration: `%APPDATA%\dev.riski.deadlinenotch`, `settings.json`, `done.json`, `fired.json`, `snoozed.json`, `local_events.json`, `feed-<id>.json`, service `deadline-notch` | A new format |
| CPU rendering with tiny-skia | 3.9 MB, 0% idle, 6.4% for a bar moving every frame, no driver memory | wgpu or Direct2D (loads GPU driver stack, tens of MB) |
| Glyphs from `ttf-parser`, rasterized lazily, cached | fontdue cost about 28 MB for two fonts because it parses every glyph up front | fontdue; DirectWrite (kept as a fallback, see risks) |
| Blocking HTTP on a worker thread (`ureq` with native-tls = SChannel) | No async runtime threads in a process that should sleep | `reqwest` + tokio (we use it today; heavier) |
| Rust owns all logic; the settings page only edits | One source of truth; settings JS keeps its own validation for instant feedback and the Rust side re-validates on load | Keeping JS logic and calling it from Rust |
| Idle = blocked in `GetMessage` | Zero CPU. Modules report their next wake-up time and the host sets one timer to the earliest | Fixed polling |

### Module contract (designed in from the start, so the notch is general)

```rust
trait Module {
    fn id(&self) -> &'static str;
    fn rest(&self, cx: &Cx) -> Option<RestView>;           // what the pill shows when this module is on top
    fn panel(&mut self, cx: &Cx, ui: &mut Ui);              // draw the expanded panel, record hit regions
    fn on_event(&mut self, ev: &Event, cx: &mut Cx) -> Vec<Effect>;
    fn next_wake(&self, now: Instant) -> Option<Instant>;   // when it next needs to run (timers, countdowns)
}
// A module can also ask the host for a peek: Peek { module, priority, content, duration }.
```

The host owns layout, animation, the module switcher (wheel or arrow keys), peek priority (a deadline alert outranks a track change) and which module owns the pill. Deadlines is the first implementation, so the trait is proven by real use before anything else is added.

## 3. Phases, gates and rough effort

Effort is in focused working days for one developer and is low-confidence (about plus or minus 40%). Total: roughly 17 to 24 days.

### Phase 0: set up the safety net (0.5 day)
- Tag the current app (`webview2-v0.1`) and work on a `native` branch. `main` keeps the working WebView2 build until cutover.
- Convert the repo to a workspace without changing behavior.

### Phase 1: `blink-core`, port the logic (3 to 4 days)
Port to Rust: ICS parser (`parseTasks`, 334 lines), feeds, merge, done store, local events, thresholds, planner and snooze, countdown and format, view model, settings normalization, hotkey accelerator parsing, i18n tables (360 lines, both languages), feed URL checks, plus the existing `feed.rs`, `secret.rs` and `store.rs` (about 330 lines of Rust already written). About 1,600 lines of JS and 913 lines of tests.
- **Differential tests:** a Node script runs the JS library on the same inputs and writes JSON; Rust tests assert identical output. Inputs: the Moodle fixture, folded lines, escaped text, all-day events, and a corpus of TZID and daylight-saving cases.
- **Time zones are the hard part.** JS uses `Intl` for IANA zones. Rust needs a tz database (`jiff` or `chrono-tz`). Measure binary size and memory before choosing.
- **Gate G1:** every ported test passes, and differential parity holds on the corpus.

### Phase 2: native shell and UI kit (4 to 6 days)
Grow the spike into the real shell:
- Window and states: rest, mini, hot strip, peek, open; hover intent and grace; focus-free by default; per-monitor DPI v2 with `WM_DPICHANGED`; monitor choice and offset; placement under a top taskbar; hot-plug.
- Windows integration: tray menu (Pause, Refresh, Settings, Quit), fullscreen and presentation detection, global hotkey with keyboard mode (simpler than today: no WebView2 focus bounce to work around), autostart, single instance, reduced-motion setting, light/dark/auto theme.
- UI kit: text with ellipsis and two-line clamp, rows, buttons, scroll list with a sticky footer, progress bar, easing, clip animation, hit regions, keyboard selection.
- **Testability without a cursor:** an internal event queue, and (debug builds or an env var) a control pipe that accepts `hover`, `key`, `tick` and `dump` commands. This lets me test hover, keys and screenshots without touching your mouse or keyboard. It also means no more test runs landing in a game or a document.
- **Gate G2:** scripted checks pass for click-through, no focus theft (including a real click), fullscreen hiding and DPI scaling; at-rest memory with fonts is at most 6 MB; you approve the text rendering on screenshots.

### Phase 3: deadlines as the first module (4 to 5 days)
Pill, hero block, rows, overdue and done sections, snooze chooser, mark done and restore, open page, footer sync status, peeks and toasts, keyboard navigation, feed worker with conditional requests and the offline retry ladder, notification planner running from `next_wake`.
- Toasts need an AppUserModelID for unpackaged apps. The installer creates the Start Menu shortcut that registers it; development runs use a fallback.
- **Gate G3:** a parity checklist against the WebView2 app passes: every feature on the README's "Built" line (deadlines from three feed types, countdowns and urgency, done and restore, snooze, personal events, peeks and toasts, hotkey and keyboard navigation, resting styles, themes, languages). Measured on the release exe with a feed loaded: at most 15 MB at rest, 0% idle CPU, window visible within 150 ms, sustained animation no worse than today.

### Phase 4: settings as a separate program and the pipe (2 to 3 days)
- `blink-settings`: today's Tauri project with the notch window removed. The settings JS keeps working; the commands it calls come from `blink-core`.
- Pipe messages: `reload_settings`, `set_hotkey` (answer: ok or `HOTKEY_IN_USE`), `refresh_now`, `data_changed`, `diagnostics`. Settings opens from the tray or the "Set up" button; a second launch focuses the open one; the first-run wizard opens it automatically as today.
- **Gate G4:** every settings flow works end to end through the real pipe (add and test feed, hotkey rebind including the in-use error, language change reaching the notch, export and import, diagnostics). With settings closed, the process tree contains no WebView2 processes.

### Phase 5: hardening and release (4 to 6 days)
- Accessibility: expose the panel through UI Automation (window name, a list with named rows, invoke actions), and announce changes with UIA notification events. This is real work for custom-drawn UI and is the largest unknown (see risks).
- Installer and uninstall (shortcut with AUMID, autostart entry, optional WebView2 check for the settings program), upgrade-in-place over the WebView2 build, code-signing plan.
- Long soak (30 minutes plus a feed refresh cycle), low-end laptop check, multi-monitor and DPI checks on real hardware.
- **Gate G5:** ready for a public beta.

### Phase 6: more modules on the host (after cutover)
System peeks first (charger, Wi-Fi, headphones, Caps Lock and language, volume and brightness), then the focus timer, then media with lyrics, then the to-do and file shelf. Each is a `Module` implementation.

## 4. Testing strategy

| What | How |
|---|---|
| Logic | Ported unit tests plus differential tests against the JS library until the JS is retired |
| Rendering | `--dump` PNG snapshots compared to goldens with a small tolerance; I review changes visually |
| Hover, keys, focus | The control pipe injects events (no real cursor); a small set of real-input probes (click-through, no focus theft) run only with your OK |
| Performance | The existing scripts, adapted: `mem-probe`, `startup-time`, `stress-cost`, `measure-full`, plus `native-spike.ps1` as the starting point |
| Settings flows | The existing browser harness keeps working for the page itself; Phase 4 adds real-pipe tests |

## 5. Risks and how I would handle them

1. **Text quality.** Grayscale antialiasing without ClearType, hand-built shaping. Latin and Indonesian are fine; complex scripts are not supported. Mitigation: you review screenshots at Gate G2; fallback is DirectWrite for glyph rasterization only (must be measured: it may add several MB).
2. **Accessibility for custom-drawn UI.** UI Automation providers are verbose, and WebView2 gave screen-reader support for free. The PRD lists screen-reader labels as important. Mitigation: do it in Phase 5, keep full keyboard operation, and be explicit if the first release has a limited screen-reader experience.
3. **Time-zone fidelity.** Different tz libraries can disagree with `Intl` at daylight-saving edges. Mitigation: differential corpus in Phase 1.
4. **Toast notifications for unpackaged apps** depend on a registered AppUserModelID. Mitigation: installer shortcut; test on a clean profile.
5. **Memory creep as features arrive.** The spike is a floor, not a forecast. Mitigation: memory budget enforced at every gate (spike 3.9 MB, deadlines at most 15 MB, each later module has a stated budget).
6. **The rewrite stalls.** Mitigation: the WebView2 app stays shippable on `main`; the abort rule below.

### Abort rule
Stop and stay on WebView2 if any of these hold: the text rendering is rejected at G2 and the DirectWrite fallback pushes the resting process past 25 MB; the deadlines module at G3 is over 25 MB; or the port is more than twice the estimate at the end of Phase 3.

## 6. What changes for the existing code

| Today | After |
|---|---|
| `src/lib/*.js`, `src/core.js`, `src/notch.js`, `src/modules/deadlines.js`, `src/notch.css`, `src/stress.js` | Replaced by `blink-core` and `blink-native`; retired at cutover |
| `src/settings.*`, `src/api.js`, `src/dom.js`, harness | Kept for the settings program; the bridge calls change to the pipe and `blink-core` |
| `src-tauri/src/{feed,secret,store}.rs` | Move into `blink-core` |
| `src-tauri/src/win32.rs`, `lib.rs` | Partly reused as reference for the native platform layer |
| Tauri bundler (NSIS) | A real installer script for two executables |

## 7. Decisions (answered 2026-10-23)

1. **Windows versions:** Windows 10 and later. The floor is Windows 10 version 1809 (build 17763), which covers every supported Windows 10 release and Windows 11.
2. **Screen readers:** the first release is keyboard-complete; UI Automation support for the open panel is planned for Phase 5 and can be pulled earlier if needed. Reminder toasts and the settings window are readable by screen readers from day one.
3. **Installer:** unsigned for now, with a clear SmartScreen note on the download page.
4. **Name:** the product is **Blink**. Crates are `blink-core`, `blink-native` and `blink-settings`; executables are `blink.exe` and `blink-settings.exe`. Internal identifiers stay as they are so installs upgrade in place: data folder `%APPDATA%\dev.riski.deadlinenotch`, Credential Manager service `deadline-notch`, autostart value migrated by the new build. Only user-visible text, window titles and the installer use "Blink". ("Blink" is also Chrome's rendering-engine name, which may make web searches noisy.)