# Run Blink for real so you can try it.
#
#   .\scripts\native\try-it.ps1            start it with your real feeds (from the same data folder as the old app)
#   .\scripts\native\try-it.ps1 -Demo      start it with sample deadlines instead of your feeds
#   .\scripts\native\try-it.ps1 -Preview   start it, then show each kind of system peek once (simulated),
#                                          so you can see how they look without doing anything; the real
#                                          sources stay on and keep working after the preview
#   .\scripts\native\try-it.ps1 -Stop      ask the running notch to exit
#
# The notch hides itself while a fullscreen app is in front (a game, a video, or a browser or app in
# fullscreen). If you see nothing, leave fullscreen.
param([switch]$Demo, [switch]$Preview, [switch]$Stop, [string]$Exe = '')

# Use the newest build there is (one is made in target-alt while another copy is running).
if (-not $Exe) {
  $Exe = @("$PSScriptRoot\..\..\target\release\blink.exe", "$PSScriptRoot\..\..\target-alt\release\blink.exe") |
    Where-Object { Test-Path $_ } | Sort-Object { (Get-Item $_).LastWriteTime } -Descending | Select-Object -First 1
}
$Exe = (Resolve-Path $Exe).Path
if ($Stop) {
  & $Exe --quit
  Start-Sleep -Milliseconds 800
  if (Get-Process blink -ErrorAction SilentlyContinue | Where-Object { $_.Path -eq $Exe }) { 'still running: use the tray icon (right-click, Quit)' } else { 'stopped' }
  return
}

$old = Get-Process -Name 'deadline-notch' -ErrorAction SilentlyContinue
if ($old) {
  "The old WebView2 notch (deadline-notch.exe) is running. Two notches would stack at the top of the screen and fight over the hotkey."
  "Quit it from its tray icon (right-click, Quit) and run this again."
  return
}
if (Get-Process blink -ErrorAction SilentlyContinue | Where-Object { $_.Path -eq $Exe }) {
  'Blink is already running. Use -Stop to quit it first.'
  return
}

if ($Preview) {
  . "$PSScriptRoot\blink-ctl.ps1"
  $env:BLINK_EXE = $Exe
  $env:BLINK_REAL_SOURCES = '1'
  if ($Demo) { $env:BLINK_DEMO = '1' }
  $p = Start-Blink 'try' @{}
  $c = Connect-Blink 'try'
  Start-Sleep -Milliseconds 1200
  $events = @(
    @{ note = 'volume';             e = @{ type = 'volume'; percent = 35; muted = $false } },
    @{ note = 'volume up';          e = @{ type = 'volume'; percent = 60; muted = $false } },
    @{ note = 'charger removed';    e = @{ type = 'power'; onAc = $true; percent = 82; charging = $true }; prep = $true },
    @{ note = 'charger removed';    e = @{ type = 'power'; onAc = $false; percent = 82; charging = $false } },
    @{ note = 'battery low';        e = @{ type = 'power'; onAc = $false; percent = 20; charging = $false } },
    @{ note = 'battery empty';      e = @{ type = 'power'; onAc = $false; percent = 10; charging = $false } },
    @{ note = 'earbuds connected';  e = @{ type = 'bluetooth'; name = 'AirBuds Pro'; connected = $false; audio = $true }; prep = $true },
    @{ note = 'earbuds connected';  e = @{ type = 'bluetooth'; name = 'AirBuds Pro'; connected = $true; battery = 70; audio = $true } },
    @{ note = 'Wi-Fi';              e = @{ type = 'wifi'; connected = $true; ssid = 'Campus' } },
    @{ note = 'Wi-Fi lost';         e = @{ type = 'wifi'; connected = $false } },
    @{ note = 'Caps Lock';          e = @{ type = 'caps'; on = $true } },
    @{ note = 'deadline alert';     offer = @{ cmd = 'offer_peek'; title = 'Problem Set 4'; code = 'CS101'; count = '1h'; priority = 3 } }
  )
  foreach ($item in $events) {
    if ($item.offer) { [void](Send-Blink $c $item.offer) } else { $item.e.cmd = 'system_event'; [void](Send-Blink $c $item.e) }
    if (-not $item.prep) { "preview: $($item.note)"; Start-Sleep -Milliseconds 3200 }
  }
  $c.Pipe.Dispose()
  'preview done. Blink keeps running with the real sources on. Use -Stop to quit.'
  return
}

if ($Demo) { $env:BLINK_DEMO = '1' }
Start-Process -FilePath $Exe
'started. Look at the top centre of the screen (hover it to open). Use -Stop, or the tray icon, to quit.'
