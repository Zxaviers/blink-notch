# Soak test: the notch running for a long time with a real feed, being used.
# Memory, handles and threads are sampled every 10 s; the panel is opened and closed, a screen
# reader client walks the tree, the notch is paused and resumed, and the feed refreshes on its
# schedule (15 minute interval, so a 35 minute run sees the first fetch and two refreshes,
# the later ones as conditional requests answered 304).
# Uses the control pipe only: no cursor, no keyboard.
#   powershell -File scripts\native\soak.ps1 [-Minutes 35] [-Exe path\to\blink.exe]
param([int]$Minutes = 35, [string]$Exe = "$PSScriptRoot\..\..\target\release\blink.exe", [int]$Port = 8794)
$ErrorActionPreference = 'Stop'
. "$PSScriptRoot\blink-ctl.ps1"
$env:BLINK_EXE = (Resolve-Path $Exe).Path
Add-Type -AssemblyName UIAutomationClient, UIAutomationTypes
Add-Type -TypeDefinition 'using System; using System.Runtime.InteropServices; public static class SoakWin { [DllImport("user32.dll", CharSet=CharSet.Unicode)] public static extern IntPtr FindWindow(string c, string t); }'

$data = Join-Path $env:TEMP 'blink-soak-data'
if (Test-Path $data) { Remove-Item $data -Recurse -Force }
New-Item -ItemType Directory -Force $data | Out-Null
$utf8 = New-Object System.Text.UTF8Encoding($false)
[System.IO.File]::WriteAllText((Join-Path $data 'settings.json'), '{"version":1,"onboarded":true,"toast":false,"refreshMinutes":15,"feeds":[{"id":"ics-soak","kind":"ics","label":"Soak","enabled":true}]}', $utf8)
$serverLog = Join-Path $env:TEMP 'blink-soak-server.log'
$server = Start-Process node -ArgumentList "`"$PSScriptRoot\..\serve-fixture.mjs`" $Port" -PassThru -WindowStyle Hidden -RedirectStandardOutput $serverLog
Start-Sleep -Milliseconds 800

function Sample($proc) {
  $proc.Refresh()
  $perf = Get-CimInstance Win32_PerfFormattedData_PerfProc_Process | Where-Object { $_.IDProcess -eq $proc.Id }
  [pscustomobject]@{
    At = Get-Date
    PrivateMB = [math]::Round($perf.WorkingSetPrivate / 1MB, 2)
    Handles = $proc.HandleCount
    Threads = $proc.Threads.Count
    CpuMs = $proc.TotalProcessorTime.TotalMilliseconds
  }
}

$proc = Start-Blink 'soak' @{ BLINK_DATA_DIR = $data; BLINK_TZ = 'Asia/Jakarta'; BLINK_TEST_FEEDS = "ics-soak=http://127.0.0.1:$Port/cal.ics" }
$samples = New-Object System.Collections.Generic.List[object]
$problems = New-Object System.Collections.Generic.List[string]
try {
  $c = Connect-Blink 'soak'
  Start-Sleep -Seconds 5
  $start = Get-Date
  $end = $start.AddMinutes($Minutes)
  $nextSample = $start; $nextCycle = $start.AddSeconds(20); $nextUia = $start.AddSeconds(45); $nextPause = $start.AddMinutes(3)
  $cycles = 0; $uiaWalks = 0; $pauses = 0
  $hwnd = [IntPtr][int64](Send-Blink $c @{cmd='state'}).window.hwnd
  "soak: $Minutes minutes, started $start"
  while ((Get-Date) -lt $end) {
    if ($proc.HasExited) { $problems.Add("the notch exited after $([math]::Round(((Get-Date) - $start).TotalMinutes, 1)) minutes"); break }
    $now = Get-Date
    if ($now -ge $nextSample) {
      $s = Sample $proc; $samples.Add($s); $nextSample = $now.AddSeconds(10)
      if ($samples.Count % 18 -eq 0) { "  +$([math]::Round(($now - $start).TotalMinutes)) min  private $($s.PrivateMB) MB  handles $($s.Handles)  threads $($s.Threads)" }
    }
    if ($now -ge $nextCycle) {
      # Use the panel: open, look, close.
      Send-Blink $c @{cmd='mouse'; kind='move'; x=320; y=10} | Out-Null
      Start-Sleep -Milliseconds 900
      Send-Blink $c @{cmd='mouse'; kind='move'; x=330; y=120} | Out-Null
      Send-Blink $c @{cmd='mouse'; kind='wheel'; x=330; y=120; delta=-48} | Out-Null
      Start-Sleep -Milliseconds 300
      Send-Blink $c @{cmd='mouse'; kind='leave'} | Out-Null
      Start-Sleep -Milliseconds 800
      $cycles++; $nextCycle = $now.AddSeconds(30)
    }
    if ($now -ge $nextUia) {
      # A screen reader looking around: walk the whole tree.
      try {
        $root = [System.Windows.Automation.AutomationElement]::FromHandle($hwnd)
        $walker = [System.Windows.Automation.TreeWalker]::RawViewWalker
        $n = 0; $child = $walker.GetFirstChild($root); while ($child) { $n++; [void]$child.Current.Name; $child = $walker.GetNextSibling($child) }
        if ($n -lt 1) { $problems.Add('the screen reader tree was empty') }
        $uiaWalks++
      } catch { $problems.Add("UI Automation walk failed: $($_.Exception.Message)") }
      $nextUia = $now.AddMinutes(1)
    }
    if ($now -ge $nextPause) {
      Send-Blink $c @{cmd='pause'; on=$true} | Out-Null; Start-Sleep -Seconds 2
      Send-Blink $c @{cmd='pause'; on=$false} | Out-Null
      Send-Blink $c @{cmd='fullscreen'; on=$true} | Out-Null; Start-Sleep -Seconds 2
      Send-Blink $c @{cmd='fullscreen'; on=$false} | Out-Null
      $pauses++; $nextPause = $now.AddMinutes(5)
    }
    Start-Sleep -Milliseconds 250
  }
  $state = Send-Blink $c @{cmd='state'}
  $frames = Send-Blink $c @{cmd='frames'}
  Send-Blink $c @{cmd='quit'} | Out-Null
} catch {
  $problems.Add("script error: $($_.Exception.Message)")
} finally {
  Start-Sleep -Milliseconds 500
  if (-not $proc.HasExited) { $proc.Kill() }
  if (-not $server.HasExited) { $server.Kill() }
}

"`n== soak result =="
if ($samples.Count -lt 10) { $problems.Add("only $($samples.Count) samples") }
else {
  # Baseline: minute 5 onward (start-up and the first feed parse are over); compare with the last 5 minutes.
  $base = $samples | Where-Object { ($_.At - $start).TotalMinutes -ge 5 -and ($_.At - $start).TotalMinutes -lt 7 }
  $tail = $samples | Where-Object { ($_.At - $start).TotalMinutes -ge ($Minutes - 5) }
  $avg = { param($set, $p) [math]::Round(($set | Measure-Object $p -Average).Average, 2) }
  $mb0 = & $avg $base 'PrivateMB'; $mb1 = & $avg $tail 'PrivateMB'
  $h0 = & $avg $base 'Handles'; $h1 = & $avg $tail 'Handles'
  $t0 = & $avg $base 'Threads'; $t1 = & $avg $tail 'Threads'
  $peak = ($samples | Measure-Object PrivateMB -Maximum).Maximum
  $span = ($samples[-1].At - $samples[0].At).TotalSeconds
  $cpu = [math]::Round(($samples[-1].CpuMs - $samples[0].CpuMs) / 1000 / $span / [Environment]::ProcessorCount * 100, 4)
  "samples: $($samples.Count) over $([math]::Round($span / 60, 1)) minutes; panel cycles: $cycles; screen reader walks: $uiaWalks; pause/fullscreen rounds: $pauses"
  "private memory: minutes 5-7 $mb0 MB -> last 5 minutes $mb1 MB (peak $peak MB)"
  "handles: $h0 -> $h1    threads: $t0 -> $t1"
  "average CPU including the scripted use: $cpu % of the machine"
  if ($Minutes -lt 12) { 'run is too short to judge growth (needs 12+ minutes)' }
  elseif ($true) {
  if ($mb1 - $mb0 -gt 1.0) { $problems.Add("private memory grew by $([math]::Round($mb1 - $mb0, 2)) MB") }
  if ($h1 - $h0 -gt 10) { $problems.Add("handles grew by $($h1 - $h0)") }
  if ([math]::Abs($t1 - $t0) -gt 2) { $problems.Add("thread count moved from $t0 to $t1") }
  # The average includes the scripted use (a 1.2 s animation every 30 s, a screen reader walk every minute); idle alone is far lower (see measure.ps1).
  if ($cpu -gt 0.25) { $problems.Add("CPU average $cpu %") }
  }
}
if ($state) {
  "final: mode=$($state.state.mode) hidden=$($state.window.hidden) sync=$($state.state.module.sync.kind) upcoming=$($state.state.module.upcoming.Count)"
  if ($state.state.mode -ne 'rest') { $problems.Add("ended in mode $($state.state.mode)") }
  if ($state.state.module.sync.kind -ne 'ok') { $problems.Add("feed not in sync at the end: $($state.state.module.sync.kind)") }
}
if ($frames) { "animation frames since the last check: n=$($frames.frames) p95 $([math]::Round($frames.p95Ms, 2)) ms max $([math]::Round($frames.maxMs, 2)) ms; over 20 ms: $($frames.over20ms)" }
$log = Get-Content $serverLog -ErrorAction SilentlyContinue
$ok = @($log | Where-Object { $_ -match '-> 200' }).Count; $nm = @($log | Where-Object { $_ -match '-> 304' }).Count
"feed requests: $ok full (200), $nm not modified (304)"
if ($Minutes -ge 17) {
  $want = if ($Minutes -ge 31) { 3 } else { 2 }
  if ($ok + $nm -lt $want) { $problems.Add("only $($ok + $nm) feed requests in $Minutes minutes (wanted ${want}: the first fetch and a refresh every 15)") }
  if ($nm -lt 1) { $problems.Add('no conditional request was answered 304') }
}
if ($problems.Count -eq 0) { 'SOAK PASSED' } else { $problems | ForEach-Object { "PROBLEM: $_" }; 'SOAK FAILED'; exit 1 }
