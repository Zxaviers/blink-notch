# Needs SteelSeries GG with Sonar running and a Sonar output as the default one. Raises the Sonar master by one wheel step (4%)
# and puts it back; no mouse or keyboard. It does not move anything else.
#   powershell -File scripts\native\check-sonar-real.ps1 [-Exe path\to\blink.exe]
param([string]$Exe = "$PSScriptRoot\..\..\target\release\blink.exe")
$ErrorActionPreference = 'Stop'
. "$PSScriptRoot\blink-ctl.ps1"
$env:BLINK_EXE = (Resolve-Path $Exe).Path
$data = Join-Path $env:TEMP 'blink-sonarchk-data'; New-Item -ItemType Directory -Force $data | Out-Null
$props = Get-Content "$env:ProgramData\SteelSeries\SteelSeries Engine 3\coreProps.json" | ConvertFrom-Json
$apps = curl.exe -sk "https://$($props.ggEncryptedAddress)/subApps" | ConvertFrom-Json
$base = $apps.subApps.sonar.metadata.webServerAddress
function Master() { (Invoke-RestMethod "$base/volumeSettings/classic").masters.classic }
$game = (Invoke-RestMethod "$base/volumeSettings/classic").devices.game.classic.volume
$before = Master
"sonar at $base; master before: $([math]::Round($before.volume * 100))% muted=$($before.muted); game channel $([math]::Round($game * 100))%"
$p = Start-Blink 'sonarchk' @{ BLINK_REAL_SOURCES = '1'; BLINK_DATA_DIR = $data; BLINK_TZ = 'Asia/Jakarta'; BLINK_DEMO = '1' }
try {
  $c = Connect-Blink 'sonarchk'
  Start-Sleep -Milliseconds 1500
  $up = Send-Blink $c @{cmd='wheel_quick'; notches=1}
  Start-Sleep -Milliseconds 800
  $after = Master
  "after one wheel step up: master $([math]::Round($after.volume * 100))% (taken: $(@($up.effects) -join ','))"
  [void](Send-Blink $c @{cmd='wheel_quick'; notches=-1})
  Start-Sleep -Milliseconds 800
  $back = Master
  $gameAfter = (Invoke-RestMethod "$base/volumeSettings/classic").devices.game.classic.volume
  "after one step down: master $([math]::Round($back.volume * 100))%; game channel $([math]::Round($gameAfter * 100))%"
  $ok = ([math]::Round(($after.volume - $before.volume) * 100) -eq 4) -and ([math]::Round($back.volume * 100) -eq [math]::Round($before.volume * 100)) -and ([math]::Abs($gameAfter - $game) -lt 0.001)
  if ($ok) { 'OK: the wheel moved the Sonar master, not the channel, and it is back where it was' } else { 'FAIL' }
} finally { if ($p -and -not $p.HasExited) { $p.Kill() } }
