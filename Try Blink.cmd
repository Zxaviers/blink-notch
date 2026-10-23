@echo off
rem Starts Blink with sample deadlines, then shows each kind of system peek once.
rem Look at the TOP CENTRE of the screen. Leave fullscreen first (the notch hides in fullscreen).
cd /d "%~dp0"
powershell -NoProfile -ExecutionPolicy Bypass -File "scripts\native\try-it.ps1" -Preview -Demo
echo.
echo Blink is still running. Double-click "Stop Blink.cmd" to quit it.
pause
