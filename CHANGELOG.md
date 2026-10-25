# Changelog

Versions follow `major.minor.patch`. The newest is first.

## 0.3.0: the first public version

A native Windows notch, rebuilt from the earlier WebView2 prototype.

**New**
- A new app icon.
- **Shelf:** drop files on the notch and drag them out onto another app; paste copied files, screenshots and text with the + (or `Ctrl+Alt+V` with the notch closed); an option takes every picture you copy.
- **Deadlines:** click a course to show only that course; make a project from a deadline; snooze a deadline's reminders, and resume them early.
- **Music:** bars that move with the real sound, a colour taken from the cover, and the real cover (not the player's logo) in the new-song peek.
- **Controls:** with SteelSeries Sonar as the sound output, the volume, the wheel and mute move Sonar's master; a change made with Sonar's own shortcuts shows a peek.
- **Gestures:** the wheel on the closed notch (volume; with Ctrl, brightness), middle click (play/pause), Shift+wheel or a swipe (turn pages); each can be switched off. The notch opens on the page you left.
- **Look:** palettes, accents and pill shapes.
- **Updates:** "Check for updates" in Settings, General (nothing is checked unless you press it).
- A short tour on first run; English and Indonesian.

**Fixed**
- The notch could stay open after files were dragged out of it, or after Windows lost a mouse-up or mouse-leave: it now looks at the real pointer and lets go.
- Dragging a file out of the shelf into Explorer was refused; it works, and offers a copy.
- The × on a shelf row opened the file instead of taking it off the shelf.
- A copy made while Blink watched the clipboard could fail in the program that copied.

**Removed**
- The AI-agents page and `blinkctl.exe`.

## 0.2.0

The native rebuild: the notch, the deadlines, the clock and weather, the controls, the music page with lyrics, the projects page, and the installer.

## 0.1

The WebView2 prototype (deadlines only).
