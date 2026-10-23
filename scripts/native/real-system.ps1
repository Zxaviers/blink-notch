# Real system events: this PRESSES KEYS for about 15 seconds (Caps Lock twice, the volume keys
# up and down). Keep your hands off the
# keyboard, close games, and expect your volume to move by a few percent and come back.
#   powershell -File scripts\native\real-system.ps1 [-Exe ...]
param([string]$Exe = "$PSScriptRoot\..\..\target\release\blink.exe")
$ErrorActionPreference = 'Stop'
. "$PSScriptRoot\blink-ctl.ps1"
$env:BLINK_EXE = (Resolve-Path $Exe).Path
Add-Type -TypeDefinition 'using System; using System.Runtime.InteropServices; public static class K { [DllImport("user32.dll")] public static extern void keybd_event(byte vk, byte scan, uint flags, UIntPtr extra); [DllImport("user32.dll")] public static extern short GetKeyState(int vk); }'
function Press($vk) { [K]::keybd_event([byte]$vk, 0, 1, [UIntPtr]::Zero); [K]::keybd_event([byte]$vk, 0, 3, [UIntPtr]::Zero) }
$script:pass = 0; $script:fail = 0
function Check([string]$what, [bool]$ok, [string]$detail = '') { if ($ok) { $script:pass++; "  ok    $what" } else { $script:fail++; "  FAIL  $what  $detail" } }
$data = Join-Path $env:TEMP 'blink-realsys-data'; New-Item -ItemType Directory -Force $data | Out-Null
$p = Start-Blink 'realsys' @{ BLINK_REAL_SOURCES = '1'; BLINK_DATA_DIR = $data; BLINK_TZ = 'Asia/Jakarta'; BLINK_DEMO = '1' }
try {
  $c = Connect-Blink 'realsys'
  Start-Sleep -Milliseconds 1000
  function Peek() { (Send-Blink $c @{cmd='state'}).state.peek }
  function WaitPeek([scriptblock]$ok, [int]$ms = 3000) { $sw = [System.Diagnostics.Stopwatch]::StartNew(); while ($sw.ElapsedMilliseconds -lt $ms) { $x = Peek; if ($x -and (& $ok $x)) { return $x }; Start-Sleep -Milliseconds 80 }; $null }
  $capsBefore = [K]::GetKeyState(0x14) -band 1

  'Caps Lock'
  Press 0x14
  $x = WaitPeek { param($p) $p.title -like 'Caps Lock*' }
  Check 'pressing Caps Lock shows a peek' ($null -ne $x) ($x | ConvertTo-Json -Compress)
  Start-Sleep -Milliseconds 2200
  Press 0x14
  $y = WaitPeek { param($p) $p.title -like 'Caps Lock*' -and $p.title -ne $x.title }
  Check 'and pressing it again shows the other state' ($null -ne $y) ($y | ConvertTo-Json -Compress)
  if (($capsBefore -ne 0) -ne (([K]::GetKeyState(0x14) -band 1) -ne 0)) { Press 0x14 }   # leave Caps Lock as it was

  'Volume'
  Start-Sleep -Milliseconds 2000
  Press 0xAF   # volume up
  $v = WaitPeek { param($p) $p.code -eq 'Volume' }
  Check 'a volume key shows the level with a bar' ($null -ne $v -and $null -ne $v.progress) ($v | ConvertTo-Json -Compress)
  Start-Sleep -Milliseconds 400
  Press 0xAE   # volume down: back where it was
  Start-Sleep -Milliseconds 400

  Send-Blink $c @{cmd='quit'} | Out-Null
} finally { Start-Sleep -Milliseconds 300; if (-not $p.HasExited) { $p.Kill() } }
"`n$script:pass passed, $script:fail failed"
if ($script:fail -gt 0) { exit 1 }
