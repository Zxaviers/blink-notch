# Privacy

Short version: Blink keeps everything on your PC. No account, no analytics, no tracking, no ads, nothing sent to anyone's server except the few things below, and only when you switch them on or press the button.

## What Blink contacts, and when

| What | Where | What is sent | When |
|---|---|---|---|
| Your calendar links | The address you added (for example your campus Moodle or Google Calendar) | A request for that file, with your link (which may hold a secret token) | When Blink refreshes the feed, about every 15 to 30 minutes, or when you press Refresh |
| Weather | Open-Meteo (open-meteo.com) | The name of a city you search for; then that city's coordinates | When the weather is shown or refreshed, and when you search for a city |
| Song lyrics | LRCLIB (lrclib.net) | The title, artist, album and length of the song | Only when lyrics are on for the Music page (Settings, Music) and a song is playing |
| Check for updates | The one link you typed into Settings, General | A request for that file | Only when you press "Check for updates" |
| SteelSeries Sonar | GG's own server on **this PC** (127.0.0.1) | The volume you set | Only when Sonar is your sound output; nothing leaves the PC |

Nothing else. Blink does not read your files, browse history, messages or keystrokes.

## What Blink looks at on your PC

- **What is playing** (title, artist, cover): from the Windows media controls, to show it. Not stored.
- **The sound** for the moving bars: Blink listens to the sound that goes to your speakers only while a song plays and the bars show, turns it into a few levels, and keeps nothing. You can turn it off (Settings, Display).
- **The clipboard:** only when you press the + on the Shelf or use its shortcut. If you switch on "put every picture I copy on the shelf", Blink is told when the clipboard changes and reads only pictures.
- **Battery, charger, Wi-Fi, Bluetooth, volume, brightness, Caps Lock:** to show their peeks.
- **Which window is in front:** to hide the notch for a fullscreen game or video.

## What Blink keeps

On your PC, in `%APPDATA%\dev.riski.deadlinenotch`:

- your settings and pages, which deadlines are done or snoozed, personal events you typed, your projects (tasks, and the paths of attached files), the shelf (the paths of files on it);
- a copy of each calendar feed it fetched (so it works offline) without the link;
- pictures and text you pasted onto the shelf, in `shelf-files` (deleted when you take them off the shelf, and a day after they leave it).

Your calendar links are kept in **Windows Credential Manager**, not in these files.

To remove everything: uninstall Blink and tick the box that deletes your settings (`/PURGE` for a silent uninstall), or delete that folder.

## Reports

"Copy diagnostics" in Settings, General builds a report with versions, counts and file sizes. It contains no feed links, names, titles or deadlines. It is only copied to your clipboard; you decide whether to send it.

## Contact

Questions about privacy: open an issue on the project's page.
