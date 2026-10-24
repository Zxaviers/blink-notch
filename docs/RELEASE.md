# Blink: release notes for the beta

What ships, how it is built, what has and has not been checked, and what testers should try.

## What is in the box

| File | What it is | Size (release) |
|---|---|---|
| `blink.exe` | The notch. Runs all the time. Native, no web engine. | 1.9 MB |
| `blink-settings.exe` | The settings window and first-run wizard. Starts when you open settings, exits when you close it. Needs the Microsoft Edge WebView2 Runtime (already on Windows 11 and most up-to-date Windows 10). | 3.3 MB |
| `Blink-<version>-setup.exe` | Per-user installer (no administrator rights), about 1.7 MB. | |

Resting cost of the notch, measured on the release build with a feed loaded: about 3.5 MB private memory (15 MB working set including shared system libraries), 0% idle CPU, window visible about 150 ms after launch. The settings program, while open, is about 64 MB across 7 processes and leaves nothing running after it closes.

## Build

```powershell
powershell -File scripts\native\build-release.ps1
```

Builds both programs and `scripts\out\Blink-<version>-setup.exe` (NSIS 3; the copy that Tauri downloads works) and prints sizes and the SHA-256.

## Install, upgrade, remove

Interactive: run the setup. Pages: welcome, folder (default `%LOCALAPPDATA%\Programs\Blink`), options (start with Windows; on for a first install, otherwise it keeps what you had), install, finish (offers to start Blink).

Silent:

```
Blink-0.2.0-setup.exe /S [/AUTOSTART] [/D=C:\where]      install; autostart only with /AUTOSTART
"%LOCALAPPDATA%\Programs\Blink\uninstall.exe" /S [/PURGE]   remove; /PURGE also deletes settings and saved links
```

- **Upgrade in place** is the same installer again: it asks the running notch to leave (`blink.exe --quit`), ends only copies running from the install folder, replaces the files, and keeps settings, caches, done and snooze state, saved links and the autostart choice.
- **From the WebView2 build ("Deadline Notch")**: if its uninstaller is registered, it is run silently first (its data is kept) and its autostart entry is replaced. The new build reads the same data folder (`%APPDATA%\dev.riski.deadlinenotch`) and the same Credential Manager entries, so nothing needs to be re-entered.
- **Uninstall** removes the programs, Start Menu entries, the autostart entry and the Installed apps entry. Your data stays unless you tick the box (or pass `/PURGE`): that deletes the data folder, the settings window's web profile and the saved calendar links in Credential Manager.
- The notch also removes an old "Deadline Notch" autostart entry itself the first time it runs.

Nothing is written outside the user's profile. There is no service, scheduled task or driver.

## Notifications

Reminders use the tray icon's balloon, which Windows 10 and 11 show as a toast; clicking one opens the task's page. No AppUserModelID or Start Menu registration is needed. Windows holds notifications back while a fullscreen app or presentation is in front. WinRT toasts (action buttons, a richer look) can be added later behind the same call.

## Accessibility

The notch is custom drawn, so it exposes itself to UI Automation:

- Closed: one button, "Next deadline: CS101 3h 14m", that opens the panel (and moves keyboard focus into it, like the hotkey).
- Open: lists "Upcoming deadlines", "Overdue" and "Done"; each deadline reads title, course, when and how long, explains the keys in its help text, and contains buttons Mark done, Snooze 30 minutes, Snooze 2 hours, Snooze until tomorrow (Restore in the done list). Footer buttons: Refresh now, Settings.
- Focus-changed events when the keyboard selection moves; spoken confirmations ("Marked done: ...", "Snoozed: ...") and reminders through UI Automation notification events.
- English and Indonesian, following the language setting. The whole thing is keyboard operable (Ctrl+Alt+D by default).

Nothing is built until an assistive tool first asks, so it costs nothing otherwise. It has been tested with the .NET UI Automation client (27 scripted checks), not yet with Narrator, NVDA or JAWS: that is the first thing for a beta tester who uses one to try. The settings window is a web page and uses normal web accessibility.

## Signing plan

The beta installer is **unsigned**; Windows SmartScreen will say "Windows protected your PC" until the file builds reputation. Say so on the download page: More info, then Run anyway.

When ready to sign (not needed for the beta): an OV or EV code-signing certificate (or Azure Trusted Signing), `signtool sign /fd SHA256 /tr <timestamp-url> /td SHA256` over `blink.exe`, `blink-settings.exe` and then the installer, in that order (the installer embeds the signed programs). EV or a signing service with reputation removes the SmartScreen warning fastest. Keep the build deterministic enough that the published SHA-256 matches what testers download.

## What has been verified

Scripted, with no mouse or keyboard (all in `scripts/native/`):

| Area | How | Result |
|---|---|---|
| Logic | Differential tests against the JavaScript it replaced (about 1,800 calls in 4 time zones, 102 calendars) and unit tests | passing |
| Window behavior | `gate-g2.ps1`: click-through by Windows' own hit testing, no focus theft, fullscreen and pause hiding, DPI 1.25/1.5/2.0, reduced motion, hotkey errors, single instance | 47 checks |
| Real data | `gate-g3.ps1`: feed fetch, offline and recovery, done, snooze, reminders once, personal events, Credential Manager | 26 checks |
| Settings | `settings-e2e.mjs`: the real page and the real pipe: wizard, test link, save, hotkey in-use error, language, events, export/import, diagnostics, autostart, remove, no web engine left | 37 checks |
| Accessibility | `gate-a11y.ps1`: a real UI Automation client walks the tree, invokes buttons, receives focus events | 27 checks |
| Installer | `installer-test.ps1`: silent install, installed notch runs, upgrade over a running copy, uninstall keeps data, `/PURGE`, WebView2-build removal | 20 checks |
| Real input (with the cursor and keys, supervised) | `real-input.ps1`: real click opens and keeps focus elsewhere, click-through reaches the app below, real hotkey takes and returns focus | 13 checks |
| Long run | `soak.ps1`: 35 minutes with a feed, panel use every 30 s, screen reader walks, pause and fullscreen rounds, two feed refreshes | see the progress notes in `NATIVE-PLAN.md` |

## What has not been checked (please try)

1. **A notification actually appearing**, and clicking it opening the page (the capture was taken while a fullscreen app was in front).
2. **A real fullscreen game or video** hiding the notch and showing it again.
3. **A second monitor**, especially a different DPI, plugging and unplugging.
4. **A low-end laptop**: startup, and animation smoothness (the machine used so far has a 144 Hz display and 16 cores).
5. **Narrator or NVDA** reading the open panel.
6. **The interactive installer pages** (only the silent path is scripted), including the WebView2 prompt on a machine without the runtime.
7. **Sleep and resume**, and a time-zone or clock change while running (handled, not yet exercised on real hardware).
8. **The tray menu** by hand: Pause, Refresh now, Settings, Quit.

## Known limits

- Text is drawn with grayscale antialiasing (no ClearType) and simple shaping: Latin and Indonesian are fine; complex scripts are not supported.
- Windows 10 version 1809 and later.
- Feed links are kept in Credential Manager and are limited to about 1,200 characters.
- Interface language: English and Indonesian.

## Going back

The WebView2 build is tagged `webview2-v0.1` and stays on `main` until the native build replaces it. To roll back, uninstall Blink (keep data) and install the old build: both read the same data.

## Music and lyrics

The Music page shows what Windows says is playing (the same list the volume flyout and the lock screen use), so it works with Spotify, a browser tab or any player that reports to Windows. It reads the title, artist, album, position and cover from Windows; none of that is stored or sent anywhere.

**Lyrics are the one thing that goes online**, and only after you turn lyrics on for a song (the speech-bubble button): the song's title, artist, album and length are sent to [lrclib.net](https://lrclib.net), a free open lyrics service, to find matching lyrics. Nothing else is sent, and nothing is saved on disk (the answer is remembered while Blink runs). `"lyrics": false` in the music module's options in `host.json` forbids the lookup altogether.

## Weather

The Clock page shows the weather. Blink asks [open-meteo.com](https://open-meteo.com) (a free service, no account or key) and sends only the **coordinates of the city you chose** (or, until you choose one, the name of the city in your Windows time zone, to find its coordinates). Nothing else is sent, and nothing is saved on disk. Swap the pane to the calendar with the switch at the top right of the page and Blink stops asking while you have the calendar as the default (`"pane": "calendar"` in the clock module's options).

## Projects

The Projects page keeps tasks and the paths of files and folders in one small file, `projects.json`, in Blink's data folder. Nothing is sent anywhere. A file or folder you drop on the notch is only remembered by its path (nothing is copied or read), and removing it from a project never touches the file. The clipboard is read only when you press Ctrl+V while typing into one of the page's fields, and only its text.
