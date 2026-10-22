# CPU cost of expand/collapse: runs N hover-in/hover-out cycles against the
# real window and reports CPU seconds used by the whole process tree (app plus
# WebView2) per cycle. Moves the real cursor for about 3 s per cycle, so keep
# your hands off the mouse. Extra WebView2 flags go in through -Flags.
param(
  [string]$Exe = "$PSScriptRoot\..\src-tauri\target\release\deadline-notch.exe",
  [string]$Flags = "",
  [int]$Cycles = 8,
  [int]$Port = 8768
)

$ErrorActionPreference = "Stop"
$out = Join-Path $PSScriptRoot "out"
New-Item -ItemType Directory -Force $out | Out-Null
$stale = Get-Process -Name "deadline-notch" -ErrorAction SilentlyContinue
if ($stale) { "ABORT  another deadline-notch is running"; exit 2 }

Add-Type -TypeDefinition @"
using System; using System.Runtime.InteropServices; using System.Text;
public static class A {
  [StructLayout(LayoutKind.Sequential)] public struct RECT { public int L, T, R, B; }
  public delegate bool EnumProc(IntPtr h, IntPtr l);
  [DllImport("user32.dll")] public static extern bool SetProcessDPIAware();
  [DllImport("user32.dll")] public static extern bool SetCursorPos(int x, int y);
  [DllImport("user32.dll")] public static extern bool GetWindowRect(IntPtr h, out RECT r);
  [DllImport("user32.dll")] public static extern uint GetWindowThreadProcessId(IntPtr h, out uint p);
  [DllImport("user32.dll")] public static extern bool EnumWindows(EnumProc cb, IntPtr l);
  [DllImport("user32.dll", CharSet=CharSet.Unicode)] public static extern int GetWindowText(IntPtr h, StringBuilder sb, int n);
  public static IntPtr Find(uint pid, string title) {
    IntPtr found = IntPtr.Zero;
    EnumWindows((h, l) => { uint p; GetWindowThreadProcessId(h, out p);
      if (p == pid) { StringBuilder sb = new StringBuilder(64); GetWindowText(h, sb, 64); if (sb.ToString() == title) { found = h; return false; } }
      return true; }, IntPtr.Zero);
    return found;
  }
}
"@
[void][A]::SetProcessDPIAware()

function Tree-Cpu([int]$rootPid) {
  # Per-process CPU seconds keyed by "type:pid".
  $all = Get-CimInstance Win32_Process | Select-Object ProcessId, ParentProcessId, Name, CommandLine
  $ids = [System.Collections.Generic.HashSet[int]]::new(); [void]$ids.Add($rootPid)
  do { $n = 0; foreach ($p in $all) { if ($ids.Contains([int]$p.ParentProcessId) -and $ids.Add([int]$p.ProcessId)) { $n++ } } } while ($n -gt 0)
  $map = @{}
  foreach ($p in $all | Where-Object { $ids.Contains([int]$_.ProcessId) }) {
    $type = if ($p.Name -eq "deadline-notch.exe") { "app" } elseif ($p.CommandLine -match '--type=([\w-]+)') { $Matches[1] } else { "wv2-browser" }
    $cpu = (Get-Process -Id $p.ProcessId -ErrorAction SilentlyContinue).CPU
    $map["$type"] = [double]$map["$type"] + [double]$cpu
  }
  $map
}

function Tree-CpuSeconds([int]$rootPid) {
  $all = Get-CimInstance Win32_Process | Select-Object ProcessId, ParentProcessId
  $ids = [System.Collections.Generic.HashSet[int]]::new(); [void]$ids.Add($rootPid)
  do { $n = 0; foreach ($p in $all) { if ($ids.Contains([int]$p.ParentProcessId) -and $ids.Add([int]$p.ProcessId)) { $n++ } } } while ($n -gt 0)
  (Get-Process -Id @($ids) -ErrorAction SilentlyContinue | Measure-Object -Property CPU -Sum).Sum
}

$dataDir = Join-Path $env:APPDATA "dev.riski.deadlinenotch"
$backup = $null
if (Test-Path $dataDir) { $backup = "$dataDir.anim-backup"; if (Test-Path $backup) { Remove-Item $backup -Recurse -Force }; Move-Item $dataDir $backup }
New-Item -ItemType Directory -Force $dataDir | Out-Null
$feedId = "ics-anim"
$settings = @{ version = 1; onboarded = $true; toast = $false
  feeds = @(@{ id = $feedId; kind = "ics"; label = "Anim"; enabled = $true }) } | ConvertTo-Json -Depth 5
[System.IO.File]::WriteAllText((Join-Path $dataDir "settings.json"), $settings, (New-Object System.Text.UTF8Encoding($false)))
cmdkey /generic:"$feedId.deadline-notch" /user:$feedId /pass:"http://127.0.0.1:$Port/cal.ics" | Out-Null
$server = Start-Process node -ArgumentList "`"$PSScriptRoot\serve-fixture.mjs`" $Port" -PassThru -WindowStyle Hidden -RedirectStandardOutput (Join-Path $out "anim-server.log")
$proc = $null
try {
  Start-Sleep -Seconds 1
  if ($Flags) { $env:WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS = $Flags } else { Remove-Item Env:WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS -ErrorAction SilentlyContinue }
  $proc = Start-Process -FilePath (Resolve-Path $Exe) -PassThru
  Start-Sleep -Seconds 25
  $hwnd = [A]::Find([uint32]$proc.Id, "Deadline Notch")
  $r = New-Object A+RECT; [void][A]::GetWindowRect($hwnd, [ref]$r)
  $cx = [int](($r.L + $r.R) / 2)
  [void][A]::SetCursorPos($r.L - 200, $r.T + 400); Start-Sleep -Seconds 3

  $idle0 = Tree-CpuSeconds $proc.Id; Start-Sleep -Seconds 10; $idle1 = Tree-CpuSeconds $proc.Id
  $before = Tree-CpuSeconds $proc.Id
  $beforeMap = Tree-Cpu $proc.Id
  for ($i = 0; $i -lt $Cycles; $i++) {
    [void][A]::SetCursorPos($cx, $r.T + 10); Start-Sleep -Milliseconds 1500
    [void][A]::SetCursorPos($r.L - 200, $r.T + 400); Start-Sleep -Milliseconds 1500
  }
  $after = Tree-CpuSeconds $proc.Id
  $afterMap = Tree-Cpu $proc.Id
  "per process type, CPU-s per cycle (includes idle share):"
  foreach ($k in $afterMap.Keys) { "  {0,-14} {1:N3}" -f $k, (($afterMap[$k] - [double]$beforeMap[$k]) / $Cycles) }
  $idleRate = ($idle1 - $idle0) / 10
  $perCycle = (($after - $before) - $idleRate * 3 * $Cycles) / $Cycles
  "flags: '$Flags'"
  "idle: {0:N3} CPU-s per second   expand+collapse: {1:N3} CPU-s per cycle (idle share removed), {2} cycles" -f $idleRate, $perCycle, $Cycles
} finally {
  Remove-Item Env:WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS -ErrorAction SilentlyContinue
  if ($proc -and -not $proc.HasExited) { Stop-Process -Id $proc.Id -Force }
  if ($server -and -not $server.HasExited) { Stop-Process -Id $server.Id -Force }
  cmdkey /delete:"$feedId.deadline-notch" | Out-Null
  Start-Sleep -Milliseconds 800
  Remove-Item $dataDir -Recurse -Force -ErrorAction SilentlyContinue
  if ($backup) { Move-Item $backup $dataDir }
}
