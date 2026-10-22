# Startup time of the real app: from launching the exe to (a) the notch window
# being visible, (b) the page running, (c) settings loaded and the first feed
# synced. Milestones inside the app come from its debug log (DN_DEBUG=1); the
# window-visible time is observed from outside. Several runs, medians at the end.
#   .\scripts\startup-time.ps1 -Runs 5
param(
  [string]$Exe = "$PSScriptRoot\..\src-tauri\target\release\deadline-notch.exe",
  [int]$Runs = 5,
  [int]$Port = 8771
)

$ErrorActionPreference = "Stop"
. "$PSScriptRoot\profile.ps1"
$out = Join-Path $PSScriptRoot "out"
New-Item -ItemType Directory -Force $out | Out-Null
if (Get-Process -Name "deadline-notch" -ErrorAction SilentlyContinue) { "ABORT  another deadline-notch is running"; exit 2 }

Add-Type -TypeDefinition @"
using System; using System.Runtime.InteropServices; using System.Text;
public static class S {
  public delegate bool EnumProc(IntPtr h, IntPtr l);
  [DllImport("user32.dll")] public static extern bool IsWindowVisible(IntPtr h);
  [DllImport("user32.dll")] public static extern uint GetWindowThreadProcessId(IntPtr h, out uint p);
  [DllImport("user32.dll")] public static extern bool EnumWindows(EnumProc cb, IntPtr l);
  [DllImport("user32.dll", CharSet=CharSet.Unicode)] public static extern int GetWindowText(IntPtr h, StringBuilder sb, int n);
  public static bool NotchVisible(uint pid) {
    bool found = false;
    EnumWindows((h, l) => { uint p; GetWindowThreadProcessId(h, out p);
      if (p == pid && IsWindowVisible(h)) { StringBuilder sb = new StringBuilder(64); GetWindowText(h, sb, 64); if (sb.ToString() == "Deadline Notch") { found = true; return false; } }
      return true; }, IntPtr.Zero);
    return found;
  }
}
"@

function Milestone($lines, $text) {
  $m = $lines | Select-String -Pattern ("\+(\d+)ms\] " + [regex]::Escape($text)) | Select-Object -First 1
  if ($m) { [int]$m.Matches[0].Groups[1].Value } else { $null }
}

$profile = Start-TestProfile -Id "startup" -Port $Port -Out $out
$env:DN_DEBUG = "1"
$results = @()
try {
  for ($i = 1; $i -le $Runs; $i++) {
    $log = Join-Path $out "startup-$i.log"; Remove-Item $log -ErrorAction SilentlyContinue
    $sw = [System.Diagnostics.Stopwatch]::StartNew()
    $proc = Start-Process -FilePath (Resolve-Path $Exe) -PassThru -RedirectStandardError $log
    $visible = $null
    while ($sw.ElapsedMilliseconds -lt 15000) {
      if (-not $visible -and [S]::NotchVisible([uint32]$proc.Id)) { $visible = $sw.ElapsedMilliseconds }
      if ($visible -and (Test-Path $log) -and (Select-String -Path $log -Pattern "core ready" -Quiet)) { break }
      Start-Sleep -Milliseconds 5
    }
    $ready = $sw.ElapsedMilliseconds
    Start-Sleep -Milliseconds 300
    $lines = Get-Content $log
    $row = [pscustomobject]@{
      Run = $i
      WindowVisible_ms = $visible
      RustSetup_ms = Milestone $lines "setup start"
      RustWindowShown_ms = Milestone $lines "window shown"
      PageScript_ms = Milestone $lines "page script running"
      CoreReady_ms = Milestone $lines "core ready"
      ObservedReady_ms = $ready
    }
    $results += $row
    Stop-Process -Id $proc.Id -Force
    Start-Sleep -Seconds 2
  }
  "Times are ms. Milestones are from the app's own clock (starts when its main() runs); WindowVisible and ObservedReady are measured from launching the exe."
  $results | Format-Table -AutoSize | Out-String
  function Median($v) { $s = @($v | Where-Object { $null -ne $_ } | Sort-Object); if ($s.Count) { $s[[int][math]::Floor(($s.Count - 1) / 2)] } }
  foreach ($col in 'WindowVisible_ms', 'RustWindowShown_ms', 'PageScript_ms', 'CoreReady_ms', 'ObservedReady_ms') {
    "median {0,-20} {1}" -f $col, (Median ($results | ForEach-Object { $_.$col }))
  }
} finally {
  Remove-Item Env:DN_DEBUG -ErrorAction SilentlyContinue
  Get-Process -Name "deadline-notch" -ErrorAction SilentlyContinue | Stop-Process -Force
  Restore-TestProfile $profile
}
