# Blink as a general notch: plan

Decided with you on 2026-10-23. Reference for the idea: Nusa (features and ideas only; own name, visuals and code). Deadlines are now one module among several.

## What you chose

| Topic | Decision |
|---|---|
| Modules | **System peeks** (general), **Project board** (tasks, to-do, and files or folders attached to a project: the file shelf lives inside it), **Music and lyrics**, and the existing **Deadlines**. No focus timer. |
| AI agents, build tools and "others" | Through the **plugin interface** rather than one built-in module each; an AI-agent status plugin is the first example. |
| Customization | Modules on or off, order, per-module settings; **themes and layouts**; and a **plugin interface** so anyone can write a module. |
| Audience | You and friends first. Polish for strangers comes later; nothing here needs it yet. |
| Look | As beautiful as Nusa: smooth morphing, crossfades, album-art accent colors, a resting clock, an optional companion on the resting pill. |

## Honest limits to design around

- The notch is a per-pixel-alpha layered window. It can draw any shape, but **cannot blur what is behind it** (no acrylic), and **no drop shadow**: any pixel that is not fully transparent catches mouse clicks, so a shadow would block the title bars of whatever is under the top edge. The look is solid dark or solid light surfaces with careful color and motion.
- It is drawn on the CPU (tiny-skia), which is why it idles at 0% and 3.5 MB. Large animated images (album art crossfades, blurred glow) cost CPU while they move; each module gets a budget and is measured like the deadlines module was.
- Music needs Windows' media-session API (WinRT). It loads extra system libraries only while the music module is enabled; the memory is measured before it ships.
- A drawn text field (for typing a task into the notch) needs a caret, selection and input-method support. Until then, typing happens in the settings program's web window, which opens in about 140 ms; the notch is for glancing, ticking things off, dragging files and quick actions.

## Architecture changes (Phase 6a)

1. **Host with several modules.** The app owns a list of modules, which are on or off and in a chosen order. One owns the resting pill; the panel shows one at a time with a small switcher (wheel, arrow keys, or tab dots) and remembers the last one.
2. **Resting pill by rule.** Priority: a module with something urgent (a deadline inside the horizon, music playing) owns it; otherwise a calm clock (and the optional companion). Configurable.
3. **Peek queue.** Any module (or plugin) can request a peek with a priority, a duration and a coalescing key. Higher priority replaces lower, equal waits, repeats collapse (volume changes update one peek rather than queue ten). A deadline alert outranks a track change; "charger connected" is low.
4. **Settings schema.** `settings.json` gains a `modules` section (enabled, order, options per module) and a `theme` section. Old files keep working (missing means defaults).
5. **Theme system.** Colors, corner radius, font choice, sizes, animation speed and easing in one JSON file; built-in themes (dark, light, and a few accents); a user theme is a file in the data folder. The settings program edits and previews it.

## Phases

| Phase | What | Gate |
|---|---|---|
| **6a** | Host: several modules, switcher, peek queue, resting clock, module and theme settings, visual polish of the host (spring motion, icons) | Existing gates still pass; a demo second module proves switching and peek priority; at most 6 MB at rest |
| **6b** | **System peeks**: charger and battery, Wi-Fi, headphones and audio device, Caps Lock and keyboard language, volume and brightness. Event driven (no polling). Each one switchable | Each event shows the right peek; idle CPU still 0%; the keys are read only for those few keys, never logged (stated in the settings page) |
| **6c** | **Plugin interface**: plugins are programs that talk JSON lines to the notch pipe; they register, push peeks, and publish a small declarative panel (text, progress, list, buttons) that the host draws in the same style and sends button events back. A `blink` command line (`blink notify`, `blink event`) for shell scripts and hooks. Manifest plus an explicit "allow this plugin" step. First plugin: AI-agent status (running, waiting for you, done) | A plugin written in a few lines of PowerShell or Node can show a peek and a panel; a plugin that crashes or hangs cannot freeze the notch |
| **6d** | **Project board**: projects, tasks with status, priority, due date and notes, to-do items, and attached files or folders (stored as references, opened or dragged out, never copied). Tasks with due dates feed the existing reminders. Glance and tick-off in the notch, drop files onto a project, full editing in the web window | Data in a plain JSON file, survives restarts and upgrades; dropping a file attaches it; due dates produce reminders; at most about 8 MB with a board loaded |
| **6e** | **Music and lyrics**: now playing from any player (Windows media sessions), controls and seek, album art with an accent color, synced lyrics from LRCLIB (the one network call, stated in the privacy text), the notch splitting in two for lyrics | Works with Spotify and a browser player; memory measured with the module on; nothing drawn while paused and still |
| **6f** | **Themes and layout polish**, a settings "Modules" and "Appearance" pages, a first-run tour | Every module respects the theme; a shared theme file round-trips |

Order: 6a, 6b (quick, visible wins and the proof the host is general), 6c (before the heavy modules, because AI agents and "others" depend on it), 6d, 6e, 6f. The look is worked on all the way through, not left to the end.

## Budgets

| | At rest |
|---|---|
| Whole notch, any modules enabled but idle | 15 MB private, 0% CPU |
| Settings program | on demand only, nothing left running when closed |
| Each module | stated at its gate and measured with the release build |

## Questions still open (I will ask when the phase starts)

- 6b: which of the system peeks matter most to you, and whether you want brightness (the hardest to detect reliably across laptops).
- 6c: what you want the first plugins to be besides AI-agent status (GitHub checks, a build watcher, a local-server list).
- 6d: how the board should look in the notch: a list grouped by status, or a compact kanban.
- 6e: your music player, so I test with the right one.

## Progress

### 6a: host with several modules, built (2026-10-23)
The app now owns a list of modules (`Slot`: module plus on/off). Which are on and their order come from `host.json` (`crates/blink-native/src/host.rs`; its own file so the settings page and the old app never see it).

- **Resting pill by rule:** each module reports how much it deserves the pill (`rest_priority`); the highest wins, ties go to the earlier module. A deadline inside the horizon scores 3, any deadline with resting style "always" scores 2, the clock always scores 1. With the default setting behavior is unchanged; with "when due" and nothing near, the new **clock module** shows the time and date.
- **Tabs:** with two or more modules on, a strip of tabs runs along the bottom of the panel (click, Left/Right or Tab). Opening from the pill or from a peek shows the module that pill or peek was about. Keyboard mode survives a switch. Screen readers get the tabs as buttons.
- **Peek queue:** `PeekView` now has a priority, a time and a key. A more important peek replaces the showing one, a lesser one waits, one with the same key updates in place (ten volume steps are one peek), and anything offered while the panel is open waits until it closes. `App::offer_peek` is what system peeks and plugins will call.
- **Control pipe:** `modules`, `switch`, `host` (write and apply `host.json`), `offer_peek`.
- **Checks:** 10 new app-level tests, 3 for `host.json`, 1 for the clock, `scripts/native/gate-host.ps1` (17 checks through the real window). Earlier gates unchanged: 47, 26, 27 and 8 still pass.

Not done in 6a: the settings page for modules and themes (6f), the theme system and visual polish of the host. The deadlines list now scrolls a little sooner because the tab strip takes 28 px.

### 6b: system peeks, built (2026-10-23)
`modules/system.rs` is a module with no panel (no tab, never owns the pill) that turns events into peeks. Event sources are in `platform/sysevents.rs` and are all event driven:

| Peek | Source | Notes |
|---|---|---|
| Charger connected or removed, fully charged, battery low (20%) and almost empty (10%) | power-setting notifications | First report is only a baseline; each warning fires once per drop; the 10% one is urgent and outranks other peeks |
| Wi-Fi connected (with the network name) or disconnected | WLAN service notifications | Desktops without Wi-Fi simply have no source |
| Headphones, earbuds or speakers appearing, going, or becoming the default output | MMDevice notifications | Switching back to speakers is not announced |
| Volume and mute, with a bar | default output's endpoint volume | Steps update one peek |
| Bluetooth devices (earbuds, controller, mouse, keyboard) connecting or disconnecting, with the battery level the device reports | a watcher over paired Bluetooth devices (classic and low energy) | Earbuds' battery fills in the same peek; their audio output is not announced a second time; a low battery on a connected device warns once. Battery shows only for devices that report it to Windows |
| Caps Lock on or off, keyboard layout changes | raw keyboard input | Looks at a key's code only for Caps Lock and the layout-switch keys (Shift, Alt, Windows, Space, Control), drops everything else, stores and sends nothing; layouts are also rechecked when the foreground window changes |

Each kind can be switched off in `host.json` (module options `battery`, `wifi`, `audio`, `bluetooth`, `volume`, `caps`, `language`). Words in English and Indonesian.

Verified: 13 unit tests of the event-to-peek rules, 4 app-level tests, `scripts/native/gate-system.ps1` (28 checks through the real window with injected events, plus a check that the real sources register: all five did on this laptop). Cost with every source running: 4.9 MB private at rest, 0% idle CPU (the Bluetooth watcher adds about 1.2 MB).

Not verified: any real event firing (needs real hardware actions; `scripts/native/real-system.ps1` does the Caps Lock, volume and layout ones with supervision), reading a real earbuds battery, Bluetooth on or off, brightness (skipped by choice), and the key source while an elevated (administrator) app has focus.
