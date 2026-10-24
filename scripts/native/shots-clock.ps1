# The Clock page: weather beside the time, then the calendar. No cursor, no keyboard (scripted pipe only).
#   powershell -File scripts\native\shots-clock.ps1 [-Exe path\to\blink.exe]
# Output: scripts\out\clock-weather.png, clock-calendar.png
param([string]$Exe = '')
. "$PSScriptRoot\blink-ctl.ps1"
if ($Exe) { $env:BLINK_EXE = (Resolve-Path $Exe).Path }
$out = (Resolve-Path (Join-Path $PSScriptRoot '..\out')).Path
$data = Join-Path $env:TEMP 'blink-shots-clock'; if (Test-Path $data) { Remove-Item $data -Recurse -Force }; New-Item -ItemType Directory -Force $data | Out-Null
$p = Start-Blink 'clockshots' @{ BLINK_MANUAL_CLOCK = '1'; BLINK_DATA_DIR = $data; BLINK_TZ = 'Asia/Jakarta'; BLINK_DEMO = '1' }
try {
  $c = Connect-Blink 'clockshots'
  function Ctl($o) { Send-Blink $c $o }
  Ctl @{cmd='mouse'; kind='move'; x=320; y=10} | Out-Null; Ctl @{cmd='advance'; ms=900} | Out-Null
  Ctl @{cmd='switch'; to='clock'} | Out-Null; Ctl @{cmd='advance'; ms=500} | Out-Null
  Start-Sleep -Milliseconds 400; Ctl @{cmd='advance'; ms=100} | Out-Null
  Ctl @{cmd='dump'; path="$out\clock-weather.png"} | Out-Null
  $h = (Ctl @{cmd='state'}).state.module.hits | Where-Object { $_.target -eq 'ShowCalendar' } | Select-Object -First 1
  $x = $h.x + $h.w / 2; $y = $h.y + $h.h / 2
  Ctl @{cmd='mouse'; kind='move'; x=$x; y=$y} | Out-Null; Ctl @{cmd='mouse'; kind='down'; x=$x; y=$y} | Out-Null; Ctl @{cmd='mouse'; kind='up'; x=$x; y=$y} | Out-Null
  Ctl @{cmd='advance'; ms=200} | Out-Null
  Ctl @{cmd='dump'; path="$out\clock-calendar.png"} | Out-Null
  Ctl @{cmd='quit'} | Out-Null
} finally { Start-Sleep -Milliseconds 300; if (!$p.HasExited) { $p.Kill() } }
