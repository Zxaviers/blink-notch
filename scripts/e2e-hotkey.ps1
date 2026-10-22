# Real-key check of the global hotkey and keyboard navigation. Seeds a throwaway
# profile with a fixture feed, puts a plain window in the foreground as "the
# app you were in", then sends Ctrl+Alt+D and navigation keys with keybd_event.
# Keep your hands off the keyboard and mouse while it runs (about 20 s).
param(
  [string]$Exe = "$PSScriptRoot\..\src-tauri\target\release\deadline-notch.exe",
  [int]$Port = 8769
)

$ErrorActionPreference = "Stop"
$out = Join-Path $PSScriptRoot "out"
New-Item -ItemType Directory -Force $out | Out-Null
$stale = Get-Process -Name "deadline-notch" -ErrorAction SilentlyContinue
if ($stale) { "ABORT  another deadline-notch is running (pid $($stale.Id -join ', '))"; exit 2 }

Add-Type -TypeDefinition @"
using System; using System.Runtime.InteropServices; using System.Text;
public static class K {
  [StructLayout(LayoutKind.Sequential)] public struct RECT { public int L, T, R, B; }
  public delegate bool EnumProc(IntPtr h, IntPtr l);
  [DllImport("user32.dll")] public static extern bool SetProcessDPIAware();
  [DllImport("user32.dll")] public static extern void keybd_event(byte vk, byte scan, uint flags, UIntPtr extra);
  [DllImport("user32.dll")] public static extern IntPtr GetForegroundWindow();
  [DllImport("user32.dll")] public static extern bool GetWindowRect(IntPtr h, out RECT r);
  [DllImport("user32.dll")] public static extern int GetWindowLong(IntPtr h, int i);
  [DllImport("user32.dll")] public static extern uint GetWindowThreadProcessId(IntPtr h, out uint p);
  [DllImport("user32.dll")] public static extern bool EnumWindows(EnumProc cb, IntPtr l);
  [DllImport("user32.dll", CharSet=CharSet.Unicode)] public static extern int GetWindowText(IntPtr h, StringBuilder sb, int n);
  [DllImport("user32.dll", CharSet=CharSet.Unicode)] public static extern int GetClassName(IntPtr h, StringBuilder sb, int n);
  public static IntPtr Find(uint pid, string title) {
    IntPtr found = IntPtr.Zero; int best = 0;
    EnumWindows((h, l) => { uint p; GetWindowThreadProcessId(h, out p);
      if (p == pid) { StringBuilder sb = new StringBuilder(64); GetWindowText(h, sb, 64);
        RECT r; GetWindowRect(h, out r); int w = r.R - r.L;
        if (sb.ToString() == title && w > best) { best = w; found = h; } }
      return true; }, IntPtr.Zero);
    return found;
  }
  public static string Describe(IntPtr h) {
    StringBuilder t = new StringBuilder(80), c = new StringBuilder(80);
    GetWindowText(h, t, 80); GetClassName(h, c, 80); uint pid; GetWindowThreadProcessId(h, out pid);
    return "class=" + c + " title='" + t + "' pid=" + pid;
  }
  public static void Chord(byte[] mods, byte key) {
    foreach (byte m in mods) keybd_event(m, 0, 0, UIntPtr.Zero);
    keybd_event(key, 0, 0, UIntPtr.Zero); keybd_event(key, 0, 2, UIntPtr.Zero);
    for (int i = mods.Length - 1; i >= 0; i--) keybd_event(mods[i], 0, 2, UIntPtr.Zero);
  }
  public static void Tap(byte key) { keybd_event(key, 0, 0, UIntPtr.Zero); keybd_event(key, 0, 2, UIntPtr.Zero); }
}
"@
[void][K]::SetProcessDPIAware()
Add-Type -AssemblyName System.Drawing
Add-Type -AssemblyName System.Windows.Forms

$VK_CONTROL = 0x11; $VK_MENU = 0x12; $VK_D = 0x44; $VK_DOWN = 0x28; $VK_ESCAPE = 0x1B
function Pass($name, $ok, $detail) { "{0}  {1}  {2}" -f $(if ($ok) { "PASS" } else { "FAIL" }), $name, $detail }
function Shot($name, $r) {
  $w = $r.R - $r.L; $h = $r.B - $r.T
  $bmp = New-Object System.Drawing.Bitmap $w, $h
  $g = [System.Drawing.Graphics]::FromImage($bmp); $g.CopyFromScreen($r.L, $r.T, 0, 0, $bmp.Size)
  $bmp.Save((Join-Path $out "$name.png")); $g.Dispose(); $bmp.Dispose()
}

$dataDir = Join-Path $env:APPDATA "dev.riski.deadlinenotch"
$backup = $null
if (Test-Path $dataDir) { $backup = "$dataDir.hotkey-backup"; if (Test-Path $backup) { Remove-Item $backup -Recurse -Force }; Move-Item $dataDir $backup }
New-Item -ItemType Directory -Force $dataDir | Out-Null
$feedId = "ics-hotkey"
$settings = @{ version = 1; onboarded = $true; toast = $false
  feeds = @(@{ id = $feedId; kind = "ics"; label = "Hotkey"; enabled = $true }) } | ConvertTo-Json -Depth 5
[System.IO.File]::WriteAllText((Join-Path $dataDir "settings.json"), $settings, (New-Object System.Text.UTF8Encoding($false)))
cmdkey /generic:"$feedId.deadline-notch" /user:$feedId /pass:"http://127.0.0.1:$Port/cal.ics" | Out-Null
$server = Start-Process node -ArgumentList "`"$PSScriptRoot\serve-fixture.mjs`" $Port" -PassThru -WindowStyle Hidden -RedirectStandardOutput (Join-Path $out "hotkey-server.log")
$log = Join-Path $out "hotkey.stderr.log"; Remove-Item $log -ErrorAction SilentlyContinue
$env:DN_DEBUG = "1"
$proc = $null; $form = $null
try {
  Start-Sleep -Seconds 1
  $proc = Start-Process -FilePath (Resolve-Path $Exe) -PassThru -RedirectStandardError $log
  Start-Sleep -Seconds 8
  $notch = [K]::Find([uint32]$proc.Id, "Deadline Notch")
  $r = New-Object K+RECT; [void][K]::GetWindowRect($notch, [ref]$r)

  # "The app you were in".
  [System.Windows.Forms.Application]::EnableVisualStyles()
  $form = New-Object System.Windows.Forms.Form
  $form.Text = "probe-active-app"; $form.StartPosition = "Manual"
  $form.Location = New-Object System.Drawing.Point(100, 500); $form.Size = New-Object System.Drawing.Size(300, 200)
  $form.Show(); $form.Activate(); [System.Windows.Forms.Application]::DoEvents(); Start-Sleep -Milliseconds 600
  [System.Windows.Forms.Application]::DoEvents()
  $before = [K]::GetForegroundWindow()
  Pass "precondition: probe window is foreground" ($before -eq $form.Handle) ([K]::Describe($before))

  # 1. Hotkey opens the notch and takes keyboard focus.
  [K]::Chord([byte[]]@($VK_CONTROL, $VK_MENU), $VK_D); Start-Sleep -Milliseconds 1200
  $fg = [K]::GetForegroundWindow()
  Pass "hotkey: notch holds keyboard focus" ($fg -eq $notch) ([K]::Describe($fg))
  Shot "hotkey-open" $r

  # 2. Navigation keys change the selection (visible in the screenshot).
  [K]::Tap($VK_DOWN); Start-Sleep -Milliseconds 300
  [K]::Tap($VK_DOWN); Start-Sleep -Milliseconds 400
  Shot "hotkey-selected" $r

  # 3. D marks the selected row done.
  [K]::Tap($VK_D); Start-Sleep -Milliseconds 800
  $doneFile = Join-Path $dataDir "done.json"
  $doneText = if (Test-Path $doneFile) { Get-Content $doneFile -Raw } else { "" }
  Pass "D marks the selected deadline done" ($doneText -match "::") ($doneText -replace "\s+", " ")
  Shot "hotkey-after-done" $r

  # 4. Esc closes and gives the foreground back.
  [K]::Tap($VK_ESCAPE); Start-Sleep -Milliseconds 1500
  $fg = [K]::GetForegroundWindow()
  Pass "Esc: focus returns to the previous app" ($fg -eq $form.Handle) ([K]::Describe($fg))
  Shot "hotkey-closed" $r

  # 5. The hotkey again toggles it closed.
  [K]::Chord([byte[]]@($VK_CONTROL, $VK_MENU), $VK_D); Start-Sleep -Milliseconds 1200
  $openAgain = ([K]::GetForegroundWindow() -eq $notch)
  [K]::Chord([byte[]]@($VK_CONTROL, $VK_MENU), $VK_D); Start-Sleep -Milliseconds 1500
  $fg = [K]::GetForegroundWindow()
  Pass "hotkey twice: opens, then closes and returns focus" ($openAgain -and $fg -eq $form.Handle) ([K]::Describe($fg))

  "--- app log ---"; Get-Content $log | Select-String -NotMatch "msg=0x6"
} finally {
  if ($form) { $form.Close() }
  if ($proc -and -not $proc.HasExited) { Stop-Process -Id $proc.Id -Force }
  if ($server -and -not $server.HasExited) { Stop-Process -Id $server.Id -Force }
  cmdkey /delete:"$feedId.deadline-notch" | Out-Null
  Start-Sleep -Milliseconds 800
  Remove-Item $dataDir -Recurse -Force -ErrorAction SilentlyContinue
  if ($backup) { Move-Item $backup $dataDir }
}
