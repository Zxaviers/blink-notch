# Measures the native notch spike (spikes/native-notch) the same way the
# WebView2 app was measured, so the numbers can sit side by side:
#   startup  launch to window visible
#   idle     CPU and memory while resting
#   hover    CPU per expand + collapse (real mouse moves, ~3 s per cycle)
#   open     CPU and memory with the panel open and still
#   stress   CPU and memory with the progress bar animating every frame
# The hover part moves the real cursor, so it only runs with -Hover; keep your hands off the mouse then.
param(
  [string]$Exe = "$PSScriptRoot\..\spikes\native-notch\target\release\native-notch-spike.exe",
  [int]$StartupRuns = 5,
  [int]$IdleSeconds = 30,
  [int]$HoverCycles = 8,
  [int]$WindowSeconds = 10,
  # Moves the real cursor (about 30 s). Off by default so it never lands in a game or a document.
  [switch]$Hover
)

$ErrorActionPreference = "Stop"
$Exe = (Resolve-Path $Exe).Path
$out = Join-Path $PSScriptRoot "out"
New-Item -ItemType Directory -Force $out | Out-Null
if (Get-Process -Name "native-notch-spike" -ErrorAction SilentlyContinue) { "ABORT  a spike is already running"; exit 2 }

Add-Type -TypeDefinition @"
using System; using System.Runtime.InteropServices; using System.Text;
public static class NS {
  [StructLayout(LayoutKind.Sequential)] public struct RECT { public int L, T, R, B; }
  public delegate bool EnumProc(IntPtr h, IntPtr l);
  [DllImport("user32.dll")] public static extern bool SetProcessDPIAware();
  [DllImport("user32.dll")] public static extern bool SetCursorPos(int x, int y);
  [DllImport("user32.dll")] public static extern void mouse_event(uint f, int dx, int dy, uint d, UIntPtr e);
  [DllImport("user32.dll")] public static extern bool GetWindowRect(IntPtr h, out RECT r);
  [DllImport("user32.dll")] public static extern bool IsWindowVisible(IntPtr h);
  [DllImport("user32.dll")] public static extern uint GetWindowThreadProcessId(IntPtr h, out uint p);
  [DllImport("user32.dll")] public static extern bool EnumWindows(EnumProc cb, IntPtr l);
  [DllImport("user32.dll", CharSet=CharSet.Unicode)] public static extern int GetWindowText(IntPtr h, StringBuilder sb, int n);
  public static IntPtr Find(uint pid, bool visibleOnly) { IntPtr f = IntPtr.Zero;
    EnumWindows((h, l) => { uint p; GetWindowThreadProcessId(h, out p);
      if (p == pid && (!visibleOnly || IsWindowVisible(h))) { StringBuilder sb = new StringBuilder(64); GetWindowText(h, sb, 64); if (sb.ToString() == "Native Notch Spike") { f = h; return false; } }
      return true; }, IntPtr.Zero); return f; }
}
"@
[void][NS]::SetProcessDPIAware()

function Start-Spike([string]$args2 = "") {
  $log = Join-Path $out "native-run.log"; Remove-Item $log -ErrorAction SilentlyContinue
  $env:DN_DEBUG = "1"
  if ($args2) { Start-Process -FilePath $Exe -ArgumentList $args2 -PassThru -RedirectStandardError $log }
  else { Start-Process -FilePath $Exe -PassThru -RedirectStandardError $log }
}
function Cpu([int]$procId) { (Get-Process -Id $procId).CPU }
function PrivWS([int]$procId) { [math]::Round(((Get-CimInstance Win32_PerfFormattedData_PerfProc_Process | Where-Object { $_.IDProcess -eq $procId }).WorkingSetPrivate) / 1MB, 1) }
function Wait-Window($p, $sw = [System.Diagnostics.Stopwatch]::StartNew()) { while ($sw.ElapsedMilliseconds -lt 10000) { $h = [NS]::Find([uint32]$p.Id, $true); if ($h -ne [IntPtr]::Zero) { return $sw.ElapsedMilliseconds }; Start-Sleep -Milliseconds 2 }; $null }
function Jiggle { [NS]::mouse_event(1, 2, 0, 0, [UIntPtr]::Zero); Start-Sleep -Milliseconds 40; [NS]::mouse_event(1, -2, 0, 0, [UIntPtr]::Zero) }
function Shot($r, $name) {
  Add-Type -AssemblyName System.Drawing
  $w = $r.R - $r.L; $h = $r.B - $r.T
  $b = New-Object System.Drawing.Bitmap $w, $h; $g = [System.Drawing.Graphics]::FromImage($b)
  $g.CopyFromScreen($r.L, $r.T, 0, 0, $b.Size); $b.Save((Join-Path $out "$name.png")); $g.Dispose(); $b.Dispose()
}

$result = [ordered]@{}
try {
  # --- startup
  $times = @(); $shown = @()
  for ($i = 0; $i -lt $StartupRuns; $i++) {
    $sw = [System.Diagnostics.Stopwatch]::StartNew()   # includes process creation, like startup-time.ps1
    $p = Start-Spike
    $t = Wait-Window $p $sw; $times += $t
    Start-Sleep -Milliseconds 300
    $m = Select-String -Path (Join-Path $out "native-run.log") -Pattern "\+(\d+)ms\] window shown" | Select-Object -First 1
    if ($m) { $shown += [int]$m.Matches[0].Groups[1].Value }
    Stop-Process -Id $p.Id -Force; Start-Sleep -Seconds 1
  }
  $med = { param($v) $s = @($v | Sort-Object); $s[[int][math]::Floor(($s.Count - 1) / 2)] }
  $result["startup: window visible, median ms (runs: $($times -join ', '))"] = & $med $times
  $result["startup: app-internal 'window shown', median ms"] = & $med $shown

  # --- idle
  $p = Start-Spike; [void](Wait-Window $p); Start-Sleep -Seconds 5
  $mem0 = PrivWS $p.Id; $c0 = Cpu $p.Id; Start-Sleep -Seconds $IdleSeconds; $c1 = Cpu $p.Id; $mem1 = PrivWS $p.Id
  $result["idle: CPU % of one core (over ${IdleSeconds}s)"] = [math]::Round(100 * ($c1 - $c0) / $IdleSeconds, 2)
  $result["idle: private working set MB (after 5s / after $($IdleSeconds + 5)s)"] = "$mem0 / $mem1"

  # --- hover cycles (real cursor)
  if ($Hover) {
  $h = [NS]::Find([uint32]$p.Id, $true); $r = New-Object NS+RECT; [void][NS]::GetWindowRect($h, [ref]$r)
  $cx = [int](($r.L + $r.R) / 2)
  [void][NS]::SetCursorPos($r.L - 200, $r.T + 400); Start-Sleep -Seconds 2
  $c0 = Cpu $p.Id; $t0 = Get-Date
  for ($i = 0; $i -lt $HoverCycles; $i++) {
    [void][NS]::SetCursorPos($cx, $r.T + 10); Start-Sleep -Milliseconds 80; Jiggle; Start-Sleep -Milliseconds 1500
    if ($i -eq 0) { Shot $r "native-spike-open" }
    [void][NS]::SetCursorPos($r.L - 200, $r.T + 400); Start-Sleep -Milliseconds 80; Jiggle; Start-Sleep -Milliseconds 1400
    if ($i -eq 0) { Shot $r "native-spike-closed" }
  }
  $c1 = Cpu $p.Id; $elapsed = ((Get-Date) - $t0).TotalSeconds
  $result["hover: CPU seconds per expand+collapse"] = [math]::Round(($c1 - $c0) / $HoverCycles, 3)
  $frames = Select-String -Path (Join-Path $out "native-run.log") -Pattern "frames open" | Select-Object -Last 1
  if ($frames) { $result["hover: frame pacing (last expand)"] = ($frames.Line -replace '^.*frames open: ', '') }
  }
  Stop-Process -Id $p.Id -Force; Start-Sleep -Seconds 1

  # --- open still, open + stress
  foreach ($variant in @(@("open, still", "--open"), @("open + progress bar every frame", "--open --stress"))) {
    $p = Start-Spike $variant[1]; [void](Wait-Window $p); Start-Sleep -Seconds 5
    $c0 = Cpu $p.Id; Start-Sleep -Seconds $WindowSeconds; $c1 = Cpu $p.Id
    $result["$($variant[0]): CPU % of one core"] = [math]::Round(100 * ($c1 - $c0) / $WindowSeconds, 1)
    $result["$($variant[0]): private working set MB"] = PrivWS $p.Id
    Stop-Process -Id $p.Id -Force; Start-Sleep -Seconds 1
  }
} finally {
  Remove-Item Env:DN_DEBUG -ErrorAction SilentlyContinue
  Get-Process -Name "native-notch-spike" -ErrorAction SilentlyContinue | Stop-Process -Force
}

"exe size: {0:N0} KB   machine: {1} logical cores" -f ((Get-Item $Exe).Length / 1KB), [Environment]::ProcessorCount
$result.GetEnumerator() | ForEach-Object { "{0,-70} {1}" -f $_.Key, $_.Value }
