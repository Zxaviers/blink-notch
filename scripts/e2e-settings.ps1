# Launches the exe with an empty profile so the first-run wizard opens, and
# screenshots the settings window. Does not move the cursor.
param(
  [string]$Exe = "$PSScriptRoot\..\src-tauri\target\debug\deadline-notch.exe"
)

$ErrorActionPreference = "Stop"
$out = Join-Path $PSScriptRoot "out"
New-Item -ItemType Directory -Force $out | Out-Null

$stale = Get-Process -Name "deadline-notch" -ErrorAction SilentlyContinue
if ($stale) { "ABORT  another deadline-notch is running (pid $($stale.Id -join ', '))"; exit 2 }

Add-Type -TypeDefinition @"
using System; using System.Runtime.InteropServices; using System.Text;
public static class W2 {
  [StructLayout(LayoutKind.Sequential)] public struct RECT { public int L, T, R, B; }
  public delegate bool EnumProc(IntPtr h, IntPtr l);
  [DllImport("user32.dll")] public static extern bool SetProcessDPIAware();
  [DllImport("user32.dll")] public static extern bool GetWindowRect(IntPtr h, out RECT r);
  [DllImport("user32.dll")] public static extern bool IsWindowVisible(IntPtr h);
  [DllImport("user32.dll")] public static extern uint GetWindowThreadProcessId(IntPtr h, out uint p);
  [DllImport("user32.dll")] public static extern bool EnumWindows(EnumProc cb, IntPtr l);
  [DllImport("user32.dll", CharSet=CharSet.Unicode)] public static extern int GetWindowText(IntPtr h, StringBuilder sb, int n);
  public static string Titles(uint pid) {
    StringBuilder all = new StringBuilder();
    EnumWindows((h, l) => { uint p; GetWindowThreadProcessId(h, out p);
      if (p == pid && IsWindowVisible(h)) { StringBuilder sb = new StringBuilder(80); GetWindowText(h, sb, 80); all.Append("[" + sb + "]"); }
      return true; }, IntPtr.Zero);
    return all.ToString();
  }
  public static IntPtr Find(uint pid, string title) {
    IntPtr found = IntPtr.Zero; int best = 0;
    EnumWindows((h, l) => { uint p; GetWindowThreadProcessId(h, out p);
      if (p == pid && IsWindowVisible(h)) { StringBuilder sb = new StringBuilder(80); GetWindowText(h, sb, 80);
        RECT r; GetWindowRect(h, out r); int w = r.R - r.L;
        if (sb.ToString() == title && w > best) { best = w; found = h; } }
      return true; }, IntPtr.Zero);
    return found;
  }
}
"@
[void][W2]::SetProcessDPIAware()
Add-Type -AssemblyName System.Drawing

$dataDir = Join-Path $env:APPDATA "dev.riski.deadlinenotch"
$backup = $null
if (Test-Path $dataDir) { $backup = "$dataDir.e2e-backup"; if (Test-Path $backup) { Remove-Item $backup -Recurse -Force }; Move-Item $dataDir $backup }

$proc = $null
try {
  $proc = Start-Process -FilePath (Resolve-Path $Exe) -PassThru
  Start-Sleep -Seconds 7
  "visible windows: " + [W2]::Titles([uint32]$proc.Id)
  $hwnd = [W2]::Find([uint32]$proc.Id, "Deadline Notch")
  if ($hwnd -eq [IntPtr]::Zero) { "FAIL  settings window did not open on first run"; return }
  $r = New-Object W2+RECT; [void][W2]::GetWindowRect($hwnd, [ref]$r)
  $w = $r.R - $r.L; $h = $r.B - $r.T
  $bmp = New-Object System.Drawing.Bitmap $w, $h
  $g = [System.Drawing.Graphics]::FromImage($bmp)
  $g.CopyFromScreen($r.L, $r.T, 0, 0, $bmp.Size)
  $bmp.Save((Join-Path $out "e2e-settings.png")); $g.Dispose(); $bmp.Dispose()
  "PASS  settings window opened on first run ($w x $h)"
} finally {
  if ($proc -and -not $proc.HasExited) { Stop-Process -Id $proc.Id -Force }
  Remove-Item $dataDir -Recurse -Force -ErrorAction SilentlyContinue
  if ($backup) { Move-Item $backup $dataDir }
}
