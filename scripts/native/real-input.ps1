# Real-input gate checks: these MOVE THE CURSOR and SEND KEYS for about 10 seconds.
# Keep your hands off the mouse and keyboard while it runs (and close games).
#   powershell -File scripts\native\real-input.ps1 [-Exe ...]
#
# 1. A real click on the pill opens the panel and does not take focus from the app underneath.
# 2. A real click on a transparent part of the window reaches the app underneath.
# 3. The real global hotkey takes keyboard focus; Escape hands it back to the previous app.
param([string]$Exe = "$PSScriptRoot\..\..\target\release\blink.exe")
$ErrorActionPreference = 'Stop'
. "$PSScriptRoot\blink-ctl.ps1"
$env:BLINK_EXE = (Resolve-Path $Exe).Path
$data = Join-Path $env:TEMP 'blink-real-data'; New-Item -ItemType Directory -Force $data | Out-Null

Add-Type -TypeDefinition @'
using System; using System.Runtime.InteropServices; using System.Text;
public static class Win {
  [DllImport("user32.dll")] public static extern bool SetCursorPos(int x, int y);
  [DllImport("user32.dll")] public static extern bool GetCursorPos(out POINT p);
  [DllImport("user32.dll")] public static extern void mouse_event(uint f, int dx, int dy, uint d, UIntPtr e);
  [DllImport("user32.dll")] public static extern void keybd_event(byte vk, byte scan, uint flags, UIntPtr e);
  [DllImport("user32.dll")] public static extern IntPtr GetForegroundWindow();
  [DllImport("user32.dll")] public static extern bool SetForegroundWindow(IntPtr h);
  [DllImport("user32.dll")] public static extern bool SetWindowPos(IntPtr h, IntPtr after, int x, int y, int w, int h2, uint flags);
  [DllImport("user32.dll")] public static extern bool SetProcessDPIAware();
  [DllImport("user32.dll", CharSet=CharSet.Unicode)] public static extern IntPtr FindWindow(string c, string t);
  [DllImport("user32.dll")] public static extern int GetWindowText(IntPtr h, StringBuilder s, int n);
  [StructLayout(LayoutKind.Sequential)] public struct POINT { public int X, Y; }
}
'@
[void][Win]::SetProcessDPIAware()
function Title($h) { $sb = New-Object System.Text.StringBuilder 256; [void][Win]::GetWindowText($h, $sb, 256); $sb.ToString() }
function Click($x, $y) {
  [void][Win]::SetCursorPos($x, $y); Start-Sleep -Milliseconds 60
  [Win]::mouse_event(0x0001, 1, 0, 0, [UIntPtr]::Zero); [Win]::mouse_event(0x0001, -1, 0, 0, [UIntPtr]::Zero)   # a real move event
  Start-Sleep -Milliseconds 60
  [Win]::mouse_event(0x0002, 0, 0, 0, [UIntPtr]::Zero); Start-Sleep -Milliseconds 40; [Win]::mouse_event(0x0004, 0, 0, 0, [UIntPtr]::Zero)
}
function Key($vk, $down) { [Win]::keybd_event([byte]$vk, 0, $(if ($down) { 0 } else { 2 }), [UIntPtr]::Zero) }

$pass = 0; $fail = 0
function Check([string]$what, [bool]$ok, [string]$detail = '') { if ($ok) { $script:pass++; "  ok    $what" } else { $script:fail++; "  FAIL  $what  $detail" } }

$saved = New-Object Win+POINT; [void][Win]::GetCursorPos([ref]$saved)
# A plain window to stand in for "the app underneath" (Windows 11 Notepad hands off to another process).
$formScript = Join-Path $env:TEMP 'blink-test-window.ps1'
[System.IO.File]::WriteAllText($formScript, 'Add-Type -AssemblyName System.Windows.Forms; $f = New-Object System.Windows.Forms.Form; $f.Text = "Blink test window"; $t = New-Object System.Windows.Forms.TextBox; $t.Multiline = $true; $t.Dock = "Fill"; $f.Controls.Add($t); [System.Windows.Forms.Application]::Run($f)')
$notepad = Start-Process powershell -ArgumentList '-NoProfile','-ExecutionPolicy','Bypass','-File',$formScript -PassThru
$blink = $null
try {
  Start-Sleep -Milliseconds 1200
  $np = [IntPtr]::Zero
  for ($i = 0; $i -lt 30 -and $np -eq [IntPtr]::Zero; $i++) { Start-Sleep -Milliseconds 200; $np = [Win]::FindWindow([NullString]::Value, 'Blink test window') }
  if ($np -eq [IntPtr]::Zero) { throw 'test window did not appear' }
  # Put Notepad under the notch's window so a click-through has something to land on.
  [void][Win]::SetWindowPos($np, [IntPtr]::Zero, 100, 0, 1400, 500, 0x0040)
  [void][Win]::SetForegroundWindow($np); Start-Sleep -Milliseconds 400

  $blink = Start-Blink 'real' @{ BLINK_DATA_DIR = $data; BLINK_TZ = 'Asia/Jakarta' }
  $c = Connect-Blink 'real'
  Send-Blink $c @{cmd='focus_mode'; real=$true} | Out-Null   # control mode simulates focus unless told otherwise
  Send-Blink $c @{cmd='settings'; patch=@{hotkey='Ctrl+Alt+Shift+F8'}} | Out-Null
  Start-Sleep -Milliseconds 300
  [void][Win]::SetForegroundWindow($np); Start-Sleep -Milliseconds 300
  $s = Send-Blink $c @{cmd='state'}
  $r = $s.window.rect
  $scale = $s.state.scale
  "test window foreground: '$(Title ([Win]::GetForegroundWindow()))'"
  Check 'test window is in front before the test' ([Win]::GetForegroundWindow() -eq $np)

  # 1. real click on the pill
  $px = [int]($r.x + 320 * $scale); $py = [int]($r.y + 10 * $scale)
  Click $px $py
  Start-Sleep -Milliseconds 900
  $s = Send-Blink $c @{cmd='state'}
  Check 'a real click opens the panel' ($s.state.mode -eq 'open') $s.state.mode
  Check 'the click did not take focus: test window is still the foreground window' ([Win]::GetForegroundWindow() -eq $np) "foreground='$(Title ([Win]::GetForegroundWindow()))'"
  Check 'blink reports it is not foreground' (-not $s.window.foreground)

  # move away and let it close
  [void][Win]::SetCursorPos($r.x + 50, $r.y + 300); Start-Sleep -Milliseconds 60
  [Win]::mouse_event(0x0001, 1, 0, 0, [UIntPtr]::Zero); Start-Sleep -Milliseconds 1500
  $s = Send-Blink $c @{cmd='state'}
  Check 'moving away closes the panel' ($s.state.mode -eq 'rest') $s.state.mode

  # 2. real click through a transparent corner of the window onto test window
  [void][Win]::SetForegroundWindow($np); Start-Sleep -Milliseconds 200
  Click ([int]($r.x + 60 * $scale)) ([int]($r.y + 200 * $scale))
  Start-Sleep -Milliseconds 300
  Check 'a click on the transparent part reaches test window and keeps it focused' ([Win]::GetForegroundWindow() -eq $np) "foreground='$(Title ([Win]::GetForegroundWindow()))'"
  $s = Send-Blink $c @{cmd='state'}
  Check 'and does not open the notch' ($s.state.mode -eq 'rest') $s.state.mode

  # 3. the real hotkey (Ctrl+Alt+Shift+F8 for this test)
  Key 0x11 $true; Key 0x12 $true; Key 0x10 $true; Key 0x77 $true; Start-Sleep -Milliseconds 50
  Key 0x77 $false; Key 0x10 $false; Key 0x12 $false; Key 0x11 $false
  Start-Sleep -Milliseconds 900
  $s = Send-Blink $c @{cmd='state'}
  Check 'the real hotkey opens the panel in keyboard mode' ($s.state.mode -eq 'open' -and $s.state.keyboard) "mode=$($s.state.mode) keyboard=$($s.state.keyboard)"
  Check 'blink really is the foreground window now' $s.window.foreground "foreground='$(Title ([Win]::GetForegroundWindow()))'"
  Key 0x28 $true; Key 0x28 $false; Start-Sleep -Milliseconds 200
  $s = Send-Blink $c @{cmd='state'}
  Check 'a real Down key selects a row' ($null -ne $s.state.module.selected)
  Key 0x1B $true; Key 0x1B $false
  Start-Sleep -Milliseconds 1200
  $s = Send-Blink $c @{cmd='state'}
  Check 'Escape closes the panel' ($s.state.mode -eq 'rest') $s.state.mode
  Check 'and focus goes back to test window' ([Win]::GetForegroundWindow() -eq $np) "foreground='$(Title ([Win]::GetForegroundWindow()))'"
  Check 'the no-activate flag is back' $s.window.noActivate
  Send-Blink $c @{cmd='quit'} | Out-Null
} finally {
  [void][Win]::SetCursorPos($saved.X, $saved.Y)
  Start-Sleep -Milliseconds 300
  if ($blink -and -not $blink.HasExited) { $blink.Kill() }
  if (-not $notepad.HasExited) { $notepad.Kill() }
}
"`n$pass passed, $fail failed"
if ($fail -gt 0) { exit 1 }
