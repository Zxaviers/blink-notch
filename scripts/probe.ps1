# Spike A probe: launches the built exe, drives the real cursor and checks
# click-through, focus and hover behaviour. Writes screenshots to scripts/out.
param(
  [string]$Exe = "$PSScriptRoot\..\src-tauri\target\debug\deadline-notch.exe",
  [switch]$KeepRunning
)

Add-Type -TypeDefinition @"
using System;
using System.Runtime.InteropServices;
using System.Text;
public static class Win {
  [StructLayout(LayoutKind.Sequential)] public struct RECT { public int Left, Top, Right, Bottom; }
  [StructLayout(LayoutKind.Sequential)] public struct POINT { public int X, Y; }
  public delegate bool EnumProc(IntPtr h, IntPtr l);
  [DllImport("user32.dll")] public static extern bool SetProcessDPIAware();
  [DllImport("user32.dll")] public static extern bool SetCursorPos(int x, int y);
  [DllImport("user32.dll")] public static extern void mouse_event(uint flags, uint dx, uint dy, uint data, UIntPtr extra);
  [DllImport("user32.dll")] public static extern IntPtr WindowFromPoint(POINT p);
  [DllImport("user32.dll")] public static extern IntPtr GetAncestor(IntPtr h, uint flags);
  [DllImport("user32.dll")] public static extern IntPtr GetForegroundWindow();
  [DllImport("user32.dll")] public static extern bool GetWindowRect(IntPtr h, out RECT r);
  [DllImport("user32.dll")] public static extern bool IsWindowVisible(IntPtr h);
  [DllImport("user32.dll")] public static extern int GetWindowLong(IntPtr h, int idx);
  [DllImport("user32.dll")] public static extern uint GetWindowThreadProcessId(IntPtr h, out uint pid);
  [DllImport("user32.dll")] public static extern bool EnumWindows(EnumProc cb, IntPtr l);
  [DllImport("user32.dll", CharSet=CharSet.Unicode)] public static extern int GetWindowText(IntPtr h, StringBuilder sb, int n);
  [DllImport("user32.dll", CharSet=CharSet.Unicode)] public static extern int GetClassName(IntPtr h, StringBuilder sb, int n);
  public static IntPtr FindByPid(uint pid) {
    IntPtr found = IntPtr.Zero;
    EnumWindows((h, l) => {
      uint p; GetWindowThreadProcessId(h, out p);
      if (p == pid && GetWindowLong(h, -20) != 0) {
        StringBuilder sb = new StringBuilder(64); GetWindowText(h, sb, 64);
        if (sb.ToString() == "Deadline Notch") { found = h; return false; }
      }
      return true;
    }, IntPtr.Zero);
    return found;
  }
  public static string Describe(IntPtr h) {
    if (h == IntPtr.Zero) return "(none)";
    StringBuilder t = new StringBuilder(80), c = new StringBuilder(80);
    GetWindowText(h, t, 80); GetClassName(h, c, 80);
    uint pid; GetWindowThreadProcessId(h, out pid);
    return "hwnd=0x" + h.ToInt64().ToString("X") + " class=" + c + " title='" + t + "' pid=" + pid;
  }
}
"@
[void][Win]::SetProcessDPIAware()
Add-Type -AssemblyName System.Drawing

$out = Join-Path $PSScriptRoot "out"
New-Item -ItemType Directory -Force $out | Out-Null
$log = Join-Path $out "notch.stderr.log"
Remove-Item $log -ErrorAction SilentlyContinue

function Shot($name, $r) {
  $w = $r.Right - $r.Left; $h = $r.Bottom - $r.Top
  $bmp = New-Object System.Drawing.Bitmap $w, $h
  $g = [System.Drawing.Graphics]::FromImage($bmp)
  $g.CopyFromScreen($r.Left, $r.Top, 0, 0, $bmp.Size)
  $bmp.Save((Join-Path $out "$name.png")); $g.Dispose(); $bmp.Dispose()
}
function Result($name, $ok, $detail) {
  $tag = if ($ok) { "PASS" } else { "FAIL" }
  "{0}  {1}  {2}" -f $tag, $name, $detail
}

$stale = Get-Process -Name "deadline-notch" -ErrorAction SilentlyContinue
if ($stale) { "ABORT  another deadline-notch is already running (pid $($stale.Id -join ', ')); it would overlap the test window. Quit it first."; exit 2 }

# Throwaway profile: onboarded, no feeds, so no settings window pops up.
$dataDir = Join-Path $env:APPDATA "dev.riski.deadlinenotch"
$backup = $null
if (Test-Path $dataDir) { $backup = "$dataDir.probe-backup"; if (Test-Path $backup) { Remove-Item $backup -Recurse -Force }; Move-Item $dataDir $backup }
New-Item -ItemType Directory -Force $dataDir | Out-Null
[System.IO.File]::WriteAllText((Join-Path $dataDir "settings.json"), '{"version":1,"onboarded":true}', (New-Object System.Text.UTF8Encoding($false)))

$env:DN_DEBUG = "1"
$proc = Start-Process -FilePath (Resolve-Path $Exe) -PassThru -RedirectStandardError $log
Start-Sleep -Seconds 4
$hwnd = [Win]::FindByPid([uint32]$proc.Id)
if ($hwnd -eq [IntPtr]::Zero) { "FAIL  window not found (process alive: $(-not $proc.HasExited))"; exit 1 }

$r = New-Object Win+RECT
[void][Win]::GetWindowRect($hwnd, [ref]$r)
$ex = [Win]::GetWindowLong($hwnd, -20)
"window rect: $($r.Left),$($r.Top) - $($r.Right),$($r.Bottom)  exstyle=0x$($ex.ToString('X'))"
Result "visible" ([Win]::IsWindowVisible($hwnd)) ""
Result "toolwindow (no Alt+Tab)" (($ex -band 0x80) -ne 0) ""
Result "noactivate" (($ex -band 0x08000000) -ne 0) ""
Result "topmost" (($ex -band 0x8) -ne 0) ""
$cx = [int](($r.Left + $r.Right) / 2)

# Put a plain WinForms window in the foreground as the "active app" and park
# the cursor away from the notch.
Add-Type -AssemblyName System.Windows.Forms
[System.Windows.Forms.Application]::EnableVisualStyles()
$form = New-Object System.Windows.Forms.Form
$form.Text = "probe-active-app"; $form.StartPosition = "Manual"
$form.Location = New-Object System.Drawing.Point(100, 500); $form.Size = New-Object System.Drawing.Size(300, 200)
$form.Show(); $form.Activate(); [System.Windows.Forms.Application]::DoEvents()
[void][Win]::SetCursorPos($r.Left - 200, $r.Top + 400); Start-Sleep -Milliseconds 600
[System.Windows.Forms.Application]::DoEvents()
$fgBefore = [Win]::GetForegroundWindow()
Result "probe window is foreground (precondition)" ($fgBefore -eq $form.Handle) ([Win]::Describe($fgBefore))

# 1. Click-through: inside the window rect, away from the pill.
$pt = New-Object Win+POINT; $pt.X = $r.Left + 60; $pt.Y = $r.Top + 250
[void][Win]::SetCursorPos($pt.X, $pt.Y); Start-Sleep -Milliseconds 400
$under = [Win]::GetAncestor([Win]::WindowFromPoint($pt), 2)
Result "click-through outside pill" ($under -ne $hwnd) ([Win]::Describe($under))

# 2. Hover the pill: window should receive input there; no focus steal.
$pt.X = $cx; $pt.Y = $r.Top + 10
[void][Win]::SetCursorPos($pt.X, $pt.Y); Start-Sleep -Milliseconds 900
$under = [Win]::GetAncestor([Win]::WindowFromPoint($pt), 2)
Result "pill receives input" ($under -eq $hwnd) ([Win]::Describe($under))
Result "focus not stolen" ([Win]::GetForegroundWindow() -eq $fgBefore) ("fg before/after: " + [Win]::Describe($fgBefore) + " / " + [Win]::Describe([Win]::GetForegroundWindow()))
Shot "expanded" $r

# 2b. A real click on the pill must reach the page and still not steal focus.
[Win]::mouse_event(0x2, 0, 0, 0, [UIntPtr]::Zero); [Win]::mouse_event(0x4, 0, 0, 0, [UIntPtr]::Zero)
Start-Sleep -Milliseconds 400
Result "focus not stolen by click" ([Win]::GetForegroundWindow() -eq $fgBefore) ([Win]::Describe([Win]::GetForegroundWindow()))
Shot "after-click" $r

# 3. Inside the expanded area (lower than the rest pill) still receives input.
$pt.X = $cx; $pt.Y = $r.Top + 120
[void][Win]::SetCursorPos($pt.X, $pt.Y); Start-Sleep -Milliseconds 500
$under = [Win]::GetAncestor([Win]::WindowFromPoint($pt), 2)
Result "expanded area receives input" ($under -eq $hwnd) ([Win]::Describe($under))

# 4. Leave: collapses and goes click-through again at the old expanded spot.
[void][Win]::SetCursorPos($r.Left + 60, $r.Top + 250); Start-Sleep -Milliseconds 1200
Shot "collapsed" $r
$pt.X = $cx; $pt.Y = $r.Top + 120
$under = [Win]::GetAncestor([Win]::WindowFromPoint($pt), 2)
Result "click-through after collapse" ($under -ne $hwnd) ([Win]::Describe($under))

"--- stderr log ---"
Get-Content $log
$form.Close()
if (-not $KeepRunning) {
  Stop-Process -Id $proc.Id -Force; "stopped pid $($proc.Id)"
  Start-Sleep -Milliseconds 500
  Remove-Item $dataDir -Recurse -Force -ErrorAction SilentlyContinue
  if ($backup) { Move-Item $backup $dataDir }
} else { "left running: pid $($proc.Id) (profile left in $dataDir)" }
