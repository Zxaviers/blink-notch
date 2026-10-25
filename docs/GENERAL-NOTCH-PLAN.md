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

### 6b follow-up after first hands-on test (2026-10-23)
Feedback from trying it, and what changed:

1. **Keyboard layout peek removed** (not needed). Caps Lock stays.
2. **Show/hide shortcut:** Ctrl+Alt+H (`visibilityHotkey` in `host.json`) shows or hides the notch, also over fullscreen. The notch now hides itself only for games and presentations by default (`hideInFullscreen`: `games` | `all` | `never`); the old "any fullscreen-like app" rule was the likely cause of it vanishing until the Windows key was pressed. `hiddenBy` in the state says why it is hidden. The z-order is reasserted so it stays on top. Cause not proven on the real laptop.
3. **Controls module (tab):** volume with mute and output chips, brightness, Wi-Fi and Bluetooth on/off, saved networks, Cast, and projection modes (shown only with a second screen). Real backend in `platform/winctl.rs`; a fake backend is used in scripted runs (`gate-controls.ps1`, 13 checks).
4. **Volume keys taken over** (`takeOverVolumeKeys`, on by default): a low-level hook swallows only Volume Up/Down/Mute while the notch is showing, so Windows' own volume bar does not appear. Verified with injected keys. Not verified: Fn keys the laptop firmware handles itself, and Windows' brightness bar (not suppressed).
5. Direction stays "Nusa-like, but more customizable": modules on/off/order/options in `host.json`; a settings page for it comes in 6f.

### Controls v2: tiles (2026-10-23)
Feedback: the Controls tab looked plain and lacked features. Nusa itself has no control center (only volume and brightness overlays), so the look follows Nusa's language (pure black, white "on" pills, big rounded cards, small grey captions) and the features follow what a quick-settings panel needs.

- **Sliders:** volume and brightness are wide tiles that fill like a progress bar (dark text over the fill, light elsewhere, a grip at the edge). The speaker icon is mute. The wheel nudges; `[` `]` change brightness from the keyboard. A slider shows where you put it at once, not when Windows reports back.
- **Output row** under the volume shows the device in use; it opens a page listing every output (the one in use first, a check on it). Choosing one goes back.
- **Tiles (4 across):** Wi-Fi (the network name shows; a small arrow opens the saved networks page), Bluetooth, **Microphone** (mute, new), Cast, Project (one press moves to the next mode; only with a second screen), **Lock**, **Screenshot** (the Win+Shift+S tool), **Settings**. A pressed toggle looks done at once and is checked against Windows a moment later.
- **Customizable:** `tiles` (ids `wifi`, `bluetooth`, `mic`, `cast`, `project`, `lock`, `snip`, `settings`; order and subset) and `output` (true/false) in the controls module options of `host.json`.
- **Live:** while the panel is open it re-reads Windows every 2 s, so a change made elsewhere shows up.
- **Checks:** 11 panel tests (new: pages, microphone, tiles option, optimistic toggle, periodic refresh), `gate-controls.ps1` 22 checks, other gates unchanged. Checked against the real PC read-only (outputs, Wi-Fi, brightness); not clicked on real hardware: mic mute, lock, screenshot tool, settings, cast, projection, output switching, Wi-Fi connect.
- **Not done:** Night light, Do not disturb, battery saver and airplane mode (Windows has no clean public way to switch these; they would be a link to Windows settings at best), a Bluetooth device list, per-app volume, DDC brightness for external monitors.

### Look v2: the Nusa-style notch (2026-10-23)
Feedback: "not flat". References: Nusa's pages (screenshots from the user), mock in `docs/design/blink-look-mock.html`, result in `docs/design/blink-look-actual.png` (made by `scripts/native/shots-look.ps1`).

- **Shape:** the open notch is wide and short (540 wide, about 192 high: header 42, page 124, pager 26) and every page is the same size. Pages turn with round arrow buttons and a row of dots (the old tab strip is gone; Left/Right/Tab still turn pages from the keyboard).
- **Header (host):** the page title with its second half in the page's own colour, the date and time with a small ring (battery when there is one, otherwise how much of the day has gone), and activity bars. Each module gives `accent()` and `title_parts()`.
- **Pages:** Deadlines (a big light countdown for the next one, rows tinted blue/amber/red by how close, actions on hover, snooze chips inside the row), Controls (gradient-filled sliders with a glow at the edge, round glowing buttons with names under them, Output and Wi-Fi as pages with their own back button), Clock (big light time).
- **Peeks:** a ring with the picture (it fills with the level), a bold line and a quiet one, and the big light number at the right; colour by kind (charger green, Wi-Fi and Bluetooth blue, headphones violet, volume cyan, caps and brightness amber, deadlines by urgency).
- **Resting pill:** ring, time (or the next deadline), quiet bars. After 45 s of quiet, and when nothing is due soon, a pair of eyes (own design): they blink every few seconds, then doze after 2.5 min and the notch stops waking for them. `eyes: false` in the clock's options turns them off.
- **Canvas:** gradients, glow, rings (arcs), orbs; light and bold Segoe UI weights for numbers and titles (regular and semibold stand in if a computer lacks them).
- **Checks:** all unit and panel tests, and gates g2 (47), g3 (26), a11y (27), draw (8), host (28), system (27), controls (22). The idle-wake test now allows the eyes' wakes and requires silence once they doze.
- **Next:** music + synced lyrics (page with album colours), projects (tasks, files, folders; the shelf), plugin interface.

### Look v2, round two: feedback (2026-10-24)
1. **Text no longer blurry:** glyphs are rasterized by Windows' DirectWrite with GDI-classic hinting (stems snapped to whole pixels), averaged to one alpha per pixel. Compared side by side with the old outline rasterizer and with natural/symmetric hinting; GDI-classic was the crispest. `BLINK_TEXT=off|natural|sym|gdi` picks another for comparing. Memory unchanged (3.9 MB private), idle CPU 0%.
2. **Software audio devices (SteelSeries Sonar, virtual cables, mixers) give no "connected" peek** (`events::is_software_audio`); in the Output page they are listed after the real devices.
3. **Page order:** Clock, Deadlines, Controls. The Clock page is the time, the date and a month calendar (Monday first): today is a filled circle, days with deadlines are coloured by how pressing (blue, amber, red) with a dot; arrows, the wheel, Up/Down browse months, Home or a click on the name returns.
4. **Top-right dots:** now a button for Blink's own settings (opens the settings program on General). The gear on the Deadlines page opens it on the feeds (deadline settings). `blink-settings --section <name>` and a running window switch section. A "Modules" section (turn pages on/off and reorder) is the next settings work.
5. **Eyes:** a timeline (`eyes.rs`) drawn frame by frame while it moves: the clock cross-fades into the eyes, they look aside now and then, blink with a fast close and slower open at uneven intervals, grow drowsy over two seconds and then the notch stops waking for them.

### Music and lyrics, built (2026-10-24)
- **Page:** the cover (rounded, with a glow in its colour), title, artist, a progress bar with elapsed and remaining time (click or drag to seek), previous, play/pause, next, and a lyrics button. The page washes up from the bottom in the cover's most vivid colour (by hue, not the muddy average); the header's second half takes it too. With nothing playing it says so.
- **Lyrics view:** the cover shrinks to the left and the lyrics fill the right: the line being sung in bold in the middle, the others fading away, gliding to the next line (340 ms) as the song moves; click a line to jump to it. Words without times are shown as a plain list. Lyrics come from LRCLIB (`blink-core::lyrics`: LRC parser with offsets and repeated stamps, title cleaner, answer reader), asked for only when lyrics are turned on, once per song.
- **Source:** `platform::media` reads Windows' media sessions (event driven, one worker thread): title, artist, album, state, position, length, which buttons the player supports, and the cover decoded to 144 px with Windows' imaging. Commands go back the same way.
- **Around it:** bars in the header and on the pill move while a song plays (a few redraws a second, only then); a peek on a new song (not for the one already playing at start); keys on the page: Space, N, P, L, Up/Down jump 10 s. Options: `lyrics`, `peek`.
- **Verified:** 7 page tests with a stand-in player and stand-in lyrics, 14 checks in `gate-music.ps1`, 5 lyrics and 3 colour unit tests. Against Windows for real with `scripts/native/fake-player.ps1` (a pretend player that shows a song and cover without any sound): title, artist, state, position and cover all arrived and drew. **Not verified:** a real Spotify or browser session, and a real lookup at lrclib.net (this development machine's HTTPS is intercepted, so the lookup could only be checked up to the network call; `cargo test -p blink-core -- --ignored lrclib` checks it on another machine).
- Memory with everything on: 4.4 MB private, 0% idle CPU.

### Projects, built (2026-10-24)
- **Data** (`blink-core::projects`): projects with tasks (to do, in progress, done; an optional due date, the end of that day) and attachments (a path, a folder flag). Saved to `projects.json` after every change (atomic write). A date can be typed after `@` in a title: `today`, `tomorrow`/`besok`, a weekday (the next one, never today; English or Indonesian), `+3d`, `+2w`, `25/10`, `25/10/2026`, `2026-10-25`; a date already gone means next year; an `@word` that is not a date stays in the title.
- **Page:** left, the project name (click for the next project, `+` for a new one), the done count big and light with a progress bar, `+ Task` and a Files button. Right, the tasks (doing first, then to do by date, then done at the bottom): a circle that moves a task on (to do, doing, done, back), the title (click to edit), a coloured due chip (overdue red, today and tomorrow amber, later blue), and a delete button on hover that asks with a second click. The Files button swaps the list for the shelf: files and folders with their folder, a click opens one, hover shows "show in folder" and "remove from project" (two clicks; the file itself is never touched).
- **Typing:** there is a small text field drawn on the page (caret, Left/Right/Home/End/Backspace/Delete, Enter saves, Escape cancels, Ctrl+V pastes the first line). Clicking `+ Task` takes the keyboard and gives it back when the typing is done; if the notch was opened with the hotkey it keeps it. While typing, Left, Right and Tab stay in the field instead of turning the page.
- **Dropping files:** the notch is a drop target (OLE). Dragging files or folders onto it opens it on the Projects shelf with the drop zone lit ("Release to add"); letting go attaches them and a peek confirms. Dragging files *out* of the shelf to another app is not built (a click opens, hover offers "show in folder").
- **Calendar:** open tasks with dates mark the calendar like deadlines do.
- **Keys on the page** (keyboard mode): A or + adds a task, Up/Down choose, Space moves a task on, Enter edits it (or opens a file), Delete removes, F files, P next project, B new project, R rename.
- **Verified:** 8 unit tests for the data and the typed dates, 11 page tests, `gate-projects.ps1` (15 checks, including that the window really registered as a drop target), all other gates. **Not verified on this machine's real hardware:** typing with a real keyboard into the field (focus after a click), and a real drag from Explorer (the drop command stands in for it).

### Round three: feedback after the Projects page (2026-10-24)
1. **Clock page:** the right half beside the time is now the weather (picture, temperature, how it feels, humidity and wind, and the next five days); a small two-way switch at its top right swaps it for the month calendar. Default pane and Celsius/Fahrenheit are options (`pane`, `weather.units`). With no city chosen, the city named by the Windows time zone is used ("Jakarta (time zone)"); clicking the place under the weather opens Blink's settings on the weather section, where a city is searched and chosen. Source: Open-Meteo (`blink-core::weather`, no key, no account), read in a worker thread, refreshed after 30 minutes when the page is looked at, retried on click after a failure.
2. **Pill:** shows the clock unless a deadline is due within `urgentHours` (default 5; the Deadlines settings "The pill"). Opening the notch from a clock pill lands on Clock.
3. **Two settings scopes:** the gear on the Deadlines page opens *deadlines settings* (calendar feeds, notifications, my events, the pill). The three dots open *Blink settings* (display, pages on/off and order, weather, music, general). Same program, `--section` picks the scope, each has its own sidebar. Pages and their options are written to `host.json`; the settings program tells the running notch with `reload_host`.
4. **Lyrics on Brave / YouTube Music:** lyrics are not tied to an app. Any player that reports to Windows' media session works, and Brave does for YouTube Music. A missing line is either a title/artist that LRCLIB does not know under that spelling, or the lookup being unreachable; the lookup now cleans titles ("(Official Video)", "- Topic", "feat."), tries several spellings and a search, and says "could not be reached" with a retry instead of silently showing nothing.
5. **Drag onto the notch** goes straight to Projects, also when the notch was resting (the page used to be reset by the height refresh).
6. **Lyrics "could not reach":** the HTTPS client trusted only its own bundled roots, so behind a TLS-inspecting antivirus or proxy (which installs its own root in Windows) every request failed with "unable to find any user-specified roots". It now trusts the Windows certificate store (lyrics, weather and feeds). Checked with the real service for the song from the report ("Magnolia" by Magnolia Celebration: plain lyrics found).
7. **Calendar button** on the Deadlines page (next to the gear), like the Reminder Widget's link to the BRONE calendar: opens `https://brone.ub.ac.id/calendar/view.php?view=month` in the default browser, to add or change a deadline there. The address is `calendarUrl` in the deadlines options (Deadlines settings, "The pill"); https only.
8. **BRONE shortcut moved to the settings** (the page button was removed): in the add-feed form, "Open the BRONE page to get the link" opens `https://brone.ub.ac.id/calendar/export.php`, and "Paste and add" reads the clipboard (Tauri command `clipboard_text`, only on that click), tests the link and, if the calendar reads, saves the feed at once.
9. **Music on the empty desktop:** with no window in front (the desktop or taskbar has the foreground; `desktop_in_front` by window class) and a song playing, the notch opens by itself on the Music page with the lyrics and stays until a window comes up (not on a paused song; Escape closes it until the desktop has been left and visited again). Music option `desktopLive` (on by default). The shell has a `hold` (set by the host in `App::update_live`) that stops the pointer leaving from closing it.
10. **Pin:** a small pin on the Music page keeps the notch open while the song plays, over any window and without the pointer on it (option `keepOpen` sets the start state; the pin itself lasts until Blink restarts).
11. **Fullscreen:** the default is now to hide the notch for anything really fullscreen (`QUNS_BUSY` or a game), and it returns by itself when that ends (`hideInFullscreen`: `all` default, `games`, `never`; Display settings). A maximized window with the taskbar showing is not fullscreen.

### Round four: cross-check, performance, agents, tour (2026-10-24)
1. **Cost of a frame:** the music page was drawn in 18 ms, 8 to 12 times a second, while a song played (19% of a core with the lyrics held open on the desktop). Measured in `tests/perf_render.rs` and fixed at the root: the colour wash and glow are drawn pixel by pixel instead of through tiny-skia's gradient pipeline (12.7 ms and 3.2 ms down to 1.2 and 0.5), a rectangular clip narrows the soft mask by hand (0.57 ms down to 0.02), a fill takes no mask where the mask lets everything through, text blends in whole numbers, and animations other than the notch opening and closing (the lyrics gliding, a drag) are drawn at about 70 frames a second at most. Result: 4.6 ms a frame and about 5% of a core in the same scene. `frames` on the control pipe now also tells where a frame's time goes and what woke the window.
2. **Gates no longer meet a notch in use:** `BLINK_INSTANCE` names the single-instance mutex (scripted runs each have their own); the settings e2e points the "no notch" checks at a pipe name nobody listens on. All gates and the e2e pass while a notch runs on the same PC.
3. **`run-latest.ps1`:** builds into one folder (`target-run`), quits the previous copy from there with `blink --quit`, and starts the new one (`-Replace` also quits one running from elsewhere, never kills).
4. **AI agents (plugin interface):** `docs/AGENTS.md`. `blinkctl.exe` (new crate `blink-cli`) sends `agent_update`, `agent_remove` and `agent_list` over the notch's pipe; `blink-core::agents` validates and keeps up to 12 agents in memory (ids, short plain text, https links only, forgotten after 6 h); the Agents page lists them (waiting for you first), a peek says when one needs you, finishes or fails, and an agent waiting for you takes the resting pill. Settings, Agents: accept reports on/off, peek on/off, the commands to copy. Not built: a manifest or per-program permission step, and answers from the notch back to an agent.
5. **First-run tour:** four cards in the notch itself (pages and arrows, the two settings buttons, music, files and peeks, AI agents), started by itself on the very first start (settings not yet onboarded, no `tour.json`), Skip/Back/Next/Done and the keys, remembered in `tour.json`, replayable from Blink settings, General.
6. **`window.rs` split:** the scripted-run commands (`window/scripted.rs`) and the requests from other programs (`window/requests.rs`) moved out; 1,784 lines became 1,363.

### Round five: connectors for every agent, and a look of your own (2026-10-25)
1. **Connectors** (`blink-core::connectors`, manifests in `connectors/`, extra ones from the data folder): one data-driven way to connect any agent, CLI or GUI, with the settings page listing what is on the PC and Connect/Disconnect per agent (backup of a file the first time, only Blink's own entries removed). Kinds: `json-hooks` (Claude Code, Gemini CLI), `file` (OpenCode plugin), `json-mcp` (Cursor, Windsurf, Claude Desktop, Antigravity; only writes to a config file that exists), `snippet` (copy). `blinkctl hook <dialect>` turns an agent's own hook events into states (one entry per session, named for the project folder), `blinkctl run -- cmd` shows any program while it lives, `blinkctl mcp` is an MCP server with one tool for agents that speak MCP. Manifests marked unverified say so on their card. See `docs/AGENTS.md`.
2. **Look** (`look.rs`, Blink settings, Display): six palettes (the body colour of the pill and the panel, and a soft light in the panel's corner), an accent for the chrome (header title, ring, page dots, Clock page, tour; "each page its own" keeps the page colours), and four pill shapes (classic, compact, wide, square corners). Kept in `host.json` under `look`; the notch changes at once and the pill eases to its new shape. The accent does not repaint a page's own meanings (urgency colours, a song's cover colour).
3. **Music cover in the peek**, **Try it now** for agents, and the cost of a frame (4.6 ms instead of 18) are in the earlier rounds.

### Round six: gestures, Shelf, a living visual (2026-10-25)
The Agents page looked useless to its first user, so it is now **off until an app is connected** (connecting one switches it on). Three things were added instead:
1. **Gestures** (`app.rs`): wheel over the resting pill = volume, Ctrl+wheel = brightness, middle click = play/pause, right click = menu, a two-finger swipe over the open notch = next/previous page (locked for 600 ms so a swipe is one page).
2. **Shelf** (`modules/shelf.rs`, `platform/dragout.rs`, `dropzones.rs`): dragging files over the notch opens two drop zones (Shelf on the left half, Projects on the right). Files are kept by path in `shelf.json`; pressing on one and moving carries the real file out through OLE drag and drop into Explorer, a chat, a browser; a click opens it; "Drag all" and "Clear". A file taken by another app leaves the shelf. Nothing is copied.
3. **Live visuals** (`platform/spectrum.rs`, `look.rs`): the bars follow the sound itself. It listens (WASAPI loopback, every active output, because a router such as Sonar sends music to a virtual output) only while a song plays and the bars show; eight log bands from 50 Hz to 12 kHz by FFT, smoothed. No sound is recorded or kept. When nothing can be heard, the bars move on the old beat. `look.accent = "cover"` makes the chrome take the colour of the song cover. Both are in Blink settings, Display.
