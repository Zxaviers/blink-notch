# End-to-end check against the built debug exe with a local fixture feed.
# Seeds a throwaway profile (settings + credential), launches the app, hovers
# the notch, captures screenshots and prints the app log. Moves the real cursor
# for a few seconds, so keep your hands off the mouse while it runs.
param(
  [string]$Exe = "$PSScriptRoot\..\src-tauri\target\debug\deadline-notch.exe",
  [int]$Port = 8765
)

$ErrorActionPreference = "Stop"
$out = Join-Path $PSScriptRoot "out"
New-Item -ItemType Directory -Force $out | Out-Null

$stale = Get-Process -Name "deadline-notch" -ErrorAction SilentlyContinue
if ($stale) { "ABORT  another deadline-notch is running (pid $($stale.Id -join ', '))"; exit 2 }

Add-Type -TypeDefinition @"
using System; using System.Runtime.InteropServices; using System.Text;
public static class W {
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
[void][W]::SetProcessDPIAware()
Add-Type -AssemblyName System.Drawing

function Shot($name, $r) {
  $w = $r.R - $r.L; $h = $r.B - $r.T
  $bmp = New-Object System.Drawing.Bitmap $w, $h
  $g = [System.Drawing.Graphics]::FromImage($bmp)
  $g.CopyFromScreen($r.L, $r.T, 0, 0, $bmp.Size)
  $bmp.Save((Join-Path $out "$name.png")); $g.Dispose(); $bmp.Dispose()
}

$dataDir = Join-Path $env:APPDATA "dev.riski.deadlinenotch"
$backup = $null
if (Test-Path $dataDir) { $backup = "$dataDir.e2e-backup"; if (Test-Path $backup) { Remove-Item $backup -Recurse -Force }; Move-Item $dataDir $backup }
New-Item -ItemType Directory -Force $dataDir | Out-Null

$feedId = "ics-e2e"
$url = "http://127.0.0.1:$Port/cal.ics"
$settings = @{ version = 1; onboarded = $true; toast = $false; thresholdsHours = @(24, 6, 1)
  feeds = @(@{ id = $feedId; kind = "ics"; label = "E2E"; enabled = $true }) } | ConvertTo-Json -Depth 5
[System.IO.File]::WriteAllText((Join-Path $dataDir "settings.json"), $settings, (New-Object System.Text.UTF8Encoding($false)))
cmdkey /generic:"$feedId.deadline-notch" /user:$feedId /pass:$url | Out-Null

$server = Start-Process node -ArgumentList "`"$PSScriptRoot\serve-fixture.mjs`" $Port" -PassThru -WindowStyle Hidden -RedirectStandardOutput (Join-Path $out "server.log")
$log = Join-Path $out "app.stderr.log"; Remove-Item $log -ErrorAction SilentlyContinue
$env:DN_DEBUG = "1"
$proc = $null
try {
  Start-Sleep -Seconds 1
  $proc = Start-Process -FilePath (Resolve-Path $Exe) -PassThru -RedirectStandardError $log
  Start-Sleep -Seconds 6
  $hwnd = [W]::Find([uint32]$proc.Id, "Deadline Notch")
  if ($hwnd -eq [IntPtr]::Zero) { "FAIL  notch window not found"; return }
  $r = New-Object W+RECT; [void][W]::GetWindowRect($hwnd, [ref]$r)
  $cx = [int](($r.L + $r.R) / 2)

  [void][W]::SetCursorPos($r.L - 200, $r.T + 400); Start-Sleep -Milliseconds 500
  Shot "e2e-rest" $r

  [void][W]::SetCursorPos($cx, $r.T + 10); Start-Sleep -Milliseconds 1200
  Shot "e2e-open" $r

  [void][W]::SetCursorPos($r.L - 200, $r.T + 400); Start-Sleep -Milliseconds 1200
  Shot "e2e-collapsed" $r

  "files in app data:"; Get-ChildItem $dataDir | ForEach-Object { "  $($_.Name)  $($_.Length) bytes" }
  "server log:"; Get-Content (Join-Path $out "server.log")
  "app log:"; Get-Content $log
} finally {
  if ($proc -and -not $proc.HasExited) { Stop-Process -Id $proc.Id -Force }
  if ($server -and -not $server.HasExited) { Stop-Process -Id $server.Id -Force }
  cmdkey /delete:"$feedId.deadline-notch" | Out-Null
  Remove-Item $dataDir -Recurse -Force -ErrorAction SilentlyContinue
  if ($backup) { Move-Item $backup $dataDir }
}
