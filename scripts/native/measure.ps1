# Resting cost of the native notch: memory, idle CPU, startup, frame pacing.
# Uses the control pipe only (no cursor, no keyboard).
#   powershell -File scripts\native\measure.ps1 [-Exe target\release\blink.exe]
param([string]$Exe = "$PSScriptRoot\..\..\target\release\blink.exe", [int]$IdleSeconds = 20)
$ErrorActionPreference = 'Stop'
. "$PSScriptRoot\blink-ctl.ps1"
$env:BLINK_EXE = (Resolve-Path $Exe).Path
$data = Join-Path $env:TEMP 'blink-measure-data'; New-Item -ItemType Directory -Force $data | Out-Null

function Mem($id) {
  $p = Get-CimInstance Win32_PerfFormattedData_PerfProc_Process | Where-Object { $_.IDProcess -eq $id }
  [pscustomobject]@{ PrivateMB = [math]::Round($p.WorkingSetPrivate / 1MB, 2); WorkingSetMB = [math]::Round($p.WorkingSet / 1MB, 2) }
}

$sw = [System.Diagnostics.Stopwatch]::StartNew()
$proc = Start-Blink 'measure' @{ BLINK_DATA_DIR = $data; BLINK_TZ = 'Asia/Jakarta' }
try {
  $c = Connect-Blink 'measure'
  $s = Send-Blink $c @{cmd='state'}
  "startup: process start -> first answer from the running app: $($sw.ElapsedMilliseconds) ms (window visible: $($s.window.visible))"
  Start-Sleep -Seconds 3
  $m0 = Mem $proc.Id
  "resting memory after 3 s:  private $($m0.PrivateMB) MB   working set $($m0.WorkingSetMB) MB"

  $cpu0 = $proc.TotalProcessorTime.TotalMilliseconds; $t0 = Get-Date
  Start-Sleep -Seconds $IdleSeconds
  $proc.Refresh()
  $cpu = $proc.TotalProcessorTime.TotalMilliseconds - $cpu0
  $secs = ((Get-Date) - $t0).TotalSeconds
  "idle CPU over $([math]::Round($secs)) s: $([math]::Round($cpu, 1)) ms of CPU = $([math]::Round($cpu / 10 / $secs / [Environment]::ProcessorCount, 4)) % of the machine"

  # Open and close the panel ten times (real time), then look at memory again.
  $frames = @()
  for ($i = 0; $i -lt 10; $i++) {
    Send-Blink $c @{cmd='mouse'; kind='move'; x=320; y=10} | Out-Null
    Start-Sleep -Milliseconds 900
    Send-Blink $c @{cmd='mouse'; kind='leave'} | Out-Null
    Start-Sleep -Milliseconds 1100
  }
  $f = Send-Blink $c @{cmd='frames'}
  "animation frames over 10 open/close cycles: n=$($f.frames) avg $([math]::Round($f.avgMs, 2)) ms  p50 $([math]::Round($f.p50Ms, 2))  p95 $([math]::Round($f.p95Ms, 2))  min $([math]::Round($f.minMs, 2))  max $([math]::Round($f.maxMs, 2))  >20ms: $($f.over20ms)"
  $s = Send-Blink $c @{cmd='state'}
  "mode after the cycles: $($s.state.mode); glyphs cached: $($s.state.cachedGlyphs)"
  $m1 = Mem $proc.Id
  "memory after 10 open/close cycles: private $($m1.PrivateMB) MB   working set $($m1.WorkingSetMB) MB"
  Start-Sleep -Seconds 5
  $m2 = Mem $proc.Id
  "memory 5 s later (settled): private $($m2.PrivateMB) MB   working set $($m2.WorkingSetMB) MB"
  Send-Blink $c @{cmd='quit'} | Out-Null
} finally { Start-Sleep -Milliseconds 300; if (-not $proc.HasExited) { $proc.Kill() } }
