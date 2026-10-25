# Builds the installer and exercises it silently: install, run the installed notch, upgrade over a
# running copy, uninstall (data kept), reinstall, uninstall with /PURGE, and the removal of a
# WebView2-build install. The interactive pages cannot be scripted and are not covered.
# Touches, for a few seconds each: a Start Menu folder "Blink", an Uninstall key "Blink" and,
# briefly, a fake "Deadline Notch" Uninstall key. All are removed again. Autostart goes to a scratch key.
#   powershell -File scripts\native\installer-test.ps1
param([string]$BinDir = '')
$ErrorActionPreference = 'Stop'
. "$PSScriptRoot\blink-ctl.ps1"
$root = Resolve-Path "$PSScriptRoot\..\.."
$bin = if ($BinDir) { (Resolve-Path $BinDir).Path } else { "$root\target\release" }
$nsis = Join-Path $env:LOCALAPPDATA 'tauri\NSIS\makensis.exe'
$setup = Join-Path $root 'scripts\out\Blink-0.2.0-setup.exe'
New-Item -ItemType Directory -Force (Split-Path $setup) | Out-Null

$script:pass = 0; $script:fail = 0
function Check([string]$what, [bool]$ok, [string]$detail = '') {
  if ($ok) { $script:pass++; "  ok    $what" } else { $script:fail++; "  FAIL  $what  $detail" }
}
function Wait-For([scriptblock]$cond, [int]$ms = 15000) { $sw = [System.Diagnostics.Stopwatch]::StartNew(); while ($sw.ElapsedMilliseconds -lt $ms) { if (& $cond) { return $true }; Start-Sleep -Milliseconds 100 }; $false }

'building the installer'
& $nsis -V1 /DVERSION=0.2.0 "/DBINDIR=$bin" "/DOUT=$setup" "$root\installer\blink.nsi" | Out-Null
Check 'makensis produced the installer' (Test-Path $setup)
"        ($([math]::Round((Get-Item $setup).Length / 1KB)) KB)"

$tmp = Join-Path $env:TEMP "blink-installer-test-$PID"
$app = Join-Path $tmp 'app'; $data = Join-Path $tmp 'data'
New-Item -ItemType Directory -Force $tmp, $data | Out-Null
$runKey = 'Software\BlinkTest\InstallRun'
$env:BLINK_RUN_KEY = $runKey
$env:BLINK_DATA_DIR = $data
$env:BLINK_IPC = 'inst'   # so the installer's --quit reaches this copy and no other
$uninstKey = 'HKCU:\Software\Microsoft\Windows\CurrentVersion\Uninstall\Blink'
$legacyKey = 'HKCU:\Software\Microsoft\Windows\CurrentVersion\Uninstall\Deadline Notch'
$menu = Join-Path $env:APPDATA 'Microsoft\Windows\Start Menu\Programs\Blink'
$marker = Join-Path $data 'settings.json'
$utf8 = New-Object System.Text.UTF8Encoding($false)

function Install([string[]]$extra = @()) { $a = @('/S') + $extra + @("/D=$app"); Start-Process -FilePath $setup -ArgumentList $a -Wait }
function Uninstall([string[]]$extra = @()) { Start-Process -FilePath (Join-Path $app 'uninstall.exe') -ArgumentList (@('/S') + $extra + @("_?=$app")) -Wait; Remove-Item (Join-Path $app 'uninstall.exe') -Force -ErrorAction SilentlyContinue; Remove-Item $app -Force -ErrorAction SilentlyContinue }
function RunValue() { try { (Get-ItemProperty "HKCU:\$runKey" -ErrorAction Stop).Blink } catch { $null } }

try {
  'a WebView2-build install is removed first, its autostart entry replaced'
  New-Item -Path $legacyKey -Force | Out-Null
  $legacyMarker = Join-Path $tmp 'legacy-ran.txt'
  New-ItemProperty -Path $legacyKey -Name UninstallString -Value "cmd.exe /c echo ran> `"$legacyMarker`" & rem" | Out-Null
  New-Item -Path "HKCU:\$runKey" -Force | Out-Null
  New-ItemProperty -Path "HKCU:\$runKey" -Name 'Deadline Notch' -Value 'C:\old\deadline-notch.exe' | Out-Null

  'install (silent, with autostart)'
  [System.IO.File]::WriteAllText($marker, '{"version":1,"onboarded":true,"feeds":[]}', $utf8)
  Install @('/AUTOSTART')
  Check 'the old install''s uninstaller was run' (Test-Path $legacyMarker)
  Check 'blink.exe and blink-settings.exe are in place' ((Test-Path "$app\blink.exe") -and (Test-Path "$app\blink-settings.exe") -and (Test-Path "$app\uninstall.exe"))
  Check 'Start Menu has Blink and Blink settings' ((Test-Path "$menu\Blink.lnk") -and (Test-Path "$menu\Blink settings.lnk"))
  $u = Get-ItemProperty $uninstKey -ErrorAction SilentlyContinue
  Check 'Windows lists it under Installed apps with its version and location' ($u.DisplayVersion -eq '0.2.0' -and $u.InstallLocation -eq $app -and $u.QuietUninstallString -like '*uninstall.exe*') ($u | Out-String)
  Check 'autostart points at the notch' ((RunValue) -eq "`"$app\blink.exe`"") (RunValue)
  Check 'the old autostart entry is gone' ($null -eq (Get-ItemProperty "HKCU:\$runKey" -ErrorAction SilentlyContinue).'Deadline Notch')
  Remove-Item $legacyKey -Force -ErrorAction SilentlyContinue

  'the installed notch runs'
  $env:BLINK_DEMO = '1'
  $psi = New-Object System.Diagnostics.ProcessStartInfo("$app\blink.exe"); $psi.UseShellExecute = $false
  $psi.EnvironmentVariables['BLINK_CONTROL'] = 'inst'; $psi.EnvironmentVariables['BLINK_MANUAL_CLOCK'] = '1'; $psi.EnvironmentVariables['BLINK_IPC'] = 'inst'
  $running = [System.Diagnostics.Process]::Start($psi)
  $c = Connect-Blink 'inst'
  $s = Send-Blink $c @{cmd='state'}
  Check 'it answers and shows the pill' ($s.window.visible -and $s.state.rest.count)
  Remove-Item Env:BLINK_DEMO

  'upgrade over a running copy'
  $before = (Get-Item "$app\blink.exe").LastWriteTime
  Start-Sleep -Milliseconds 1100
  Install
  Check 'the running notch was asked to leave' ($running.HasExited -or (Wait-For { $running.Refresh(); $running.HasExited } 5000))
  Check 'files were replaced and the install location kept' ((Test-Path "$app\blink.exe") -and (Get-Item "$app\blink.exe").LastWriteTime -ge $before)
  Check 'a silent upgrade without /AUTOSTART leaves autostart as it was' ((RunValue) -eq "`"$app\blink.exe`"") (RunValue)
  Check 'settings are untouched' (Test-Path $marker)
  $c.Pipe.Dispose()

  'uninstall (data kept)'
  Uninstall
  Check 'program files are gone' (-not (Test-Path "$app\blink.exe") -and -not (Test-Path "$app\blink-settings.exe"))
  Check 'Start Menu entries are gone' (-not (Test-Path "$menu\Blink.lnk"))
  Check 'the Installed apps entry is gone' (-not (Test-Path $uninstKey))
  Check 'autostart is gone' ($null -eq (RunValue))
  Check 'settings are kept' (Test-Path $marker)

  'reinstall, then uninstall with /PURGE'
  Install
  Check 'a plain silent install does not turn autostart on' ($null -eq (RunValue))
  Uninstall @('/PURGE')
  Check 'program files are gone' (-not (Test-Path "$app\blink.exe"))
  Check '/PURGE also removed settings and caches' (-not (Test-Path $data))
} finally {
  Get-Process blink, blink-settings -ErrorAction SilentlyContinue | Where-Object { $_.Path -like "$tmp*" } | Stop-Process -Force -ErrorAction SilentlyContinue
  Remove-Item $legacyKey -Force -ErrorAction SilentlyContinue
  Remove-Item $uninstKey -Force -ErrorAction SilentlyContinue
  Remove-Item $menu -Recurse -Force -ErrorAction SilentlyContinue
  Remove-Item 'HKCU:\Software\BlinkTest' -Recurse -Force -ErrorAction SilentlyContinue
  Remove-Item $tmp -Recurse -Force -ErrorAction SilentlyContinue
}
"`n$script:pass passed, $script:fail failed"
if ($script:fail -gt 0) { exit 1 }
