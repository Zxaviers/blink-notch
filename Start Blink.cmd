@echo off
rem Starts Blink with sample deadlines and the real system sources (volume, Caps Lock, charger, Wi-Fi, Bluetooth).
cd /d "%~dp0"
powershell -NoProfile -ExecutionPolicy Bypass -File "scripts\native\try-it.ps1" -Demo
pause
