# The controls panel through the real window with a stand-in for Windows (it only remembers the
# commands), plus the volume-key takeover plumbing. No cursor, no keyboard, nothing on your PC changes.
#   powershell -File scripts\native\gate-controls.ps1 [-Exe path\to\blink.exe]
param([string]$Exe = '')
$ErrorActionPreference = 'Stop'
. "$PSScriptRoot\blink-ctl.ps1"
if ($Exe) { $env:BLINK_EXE = (Resolve-Path $Exe).Path }
$script:pass = 0; $script:fail = 0
function Check([string]$what, [bool]$ok, [string]$detail = '') { if ($ok) { $script:pass++; "  ok    $what" } else { $script:fail++; "  FAIL  $what  $detail" } }
$data = Join-Path $env:TEMP 'blink-ctl-data'; if (Test-Path $data) { Remove-Item $data -Recurse -Force }; New-Item -ItemType Directory -Force $data | Out-Null
$p = Start-Blink 'ctl' @{ BLINK_MANUAL_CLOCK = '1'; BLINK_DATA_DIR = $data; BLINK_TZ = 'Asia/Jakarta'; BLINK_DEMO = '1' }
try {
  $c = Connect-Blink 'ctl'
  function Ctl($o) { Send-Blink $c $o }
  function Hits() { (Ctl @{cmd='state'}).state.module.hits }
  function Log() { (Ctl @{cmd='controls_log'}).commands }
  function ClickHit($pattern) { $h = Hits | Where-Object { $_.target -like $pattern } | Select-Object -First 1; if (-not $h) { throw "no control $pattern" }; $x = $h.x + $h.w / 2; $y = $h.y + $h.h / 2; Ctl @{cmd='mouse'; kind='move'; x=$x; y=$y} | Out-Null; Ctl @{cmd='mouse'; kind='down'; x=$x; y=$y} | Out-Null; Ctl @{cmd='mouse'; kind='up'; x=$x; y=$y} | Out-Null }

  Ctl @{cmd='mouse'; kind='move'; x=320; y=10} | Out-Null; Ctl @{cmd='advance'; ms=900} | Out-Null
  Ctl @{cmd='switch'; to='controls'} | Out-Null; Ctl @{cmd='advance'; ms=400} | Out-Null
  $opening = Log
  $m = Ctl @{cmd='modules'}
  Check 'Controls is one of the tabs' (@($m.modules | Where-Object { $_.id -eq 'controls' -and $_.enabled }).Count -eq 1)
  Check 'the panel is open on it' ($m.active -eq 'controls' -and (Ctl @{cmd='state'}).state.mode -eq 'open')
  Check 'opening it asked Windows to be read again' ($opening -contains 'Refresh') ($opening -join ',')

  'sliders'
  $vol = Hits | Where-Object { $_.target -eq 'Volume' } | Select-Object -First 1
  $y = $vol.y + $vol.h / 2
  $a = $vol.x + 8; $b = $vol.x + $vol.w - 8
  Ctl @{cmd='mouse'; kind='move'; x=($a + ($b - $a) * 0.25); y=$y} | Out-Null
  Ctl @{cmd='mouse'; kind='down'; x=($a + ($b - $a) * 0.25); y=$y} | Out-Null
  Ctl @{cmd='mouse'; kind='move'; x=($a + ($b - $a) * 0.8); y=$y} | Out-Null
  Ctl @{cmd='mouse'; kind='up'; x=($a + ($b - $a) * 0.8); y=$y} | Out-Null
  $log = Log
  Check 'dragging the volume slider sends the levels it passes' ($log -contains 'SetVolume(25)' -and $log -contains 'SetVolume(80)') ($log -join ', ')
  $br = Hits | Where-Object { $_.target -eq 'Brightness' } | Select-Object -First 1
  Ctl @{cmd='mouse'; kind='move'; x=($br.x + 8); y=($br.y + 10)} | Out-Null
  Ctl @{cmd='mouse'; kind='down'; x=($br.x + 8); y=($br.y + 10)} | Out-Null
  Ctl @{cmd='mouse'; kind='up'; x=($br.x + 8); y=($br.y + 10)} | Out-Null
  Check 'the brightness slider sets brightness' ((Log) -contains 'SetBrightness(0)')

  'buttons'
  ClickHit '*ToggleMute*'
  Check 'the speaker button mutes' ((Log) -contains 'ToggleMute')
  ClickHit '*SetOutput("bt")*'
  Check 'choosing another output sends it' ((Log) -contains 'SetOutput("bt")')
  ClickHit '*SetBluetooth(false)*'
  Check 'the Bluetooth chip toggles it' ((Log) -contains 'SetBluetooth(false)')
  ClickHit '*ConnectWifi("Home")*'
  Check 'a saved network connects' ((Log) -contains 'ConnectWifi("Home")')
  ClickHit '*Project(Duplicate)*'
  Check 'a projection mode is chosen' ((Log) -contains 'Project(Duplicate)')
  ClickHit '*Cast*'
  Check 'Cast is a button' ((Log) -contains 'Cast')

  'the volume keys'
  $fx = @((Ctl @{cmd='media_key'; vk=175}).effects) + @((Ctl @{cmd='media_key'; vk=173}).effects)
  Check 'a swallowed volume key reaches the notch (logged here, since this run has no real controls)' (@($fx | Where-Object { $_ -like 'media_key:*' }).Count -eq 2) ($fx -join ',')

  'looks'
  $dib = Ctl @{cmd='dibcheck'}
  Check 'the window shows exactly what was drawn' ($dib.differing -eq 0)
  Ctl @{cmd='dump'; path=(Join-Path (Resolve-Path (Join-Path $PSScriptRoot '..\out')).Path 'controls-gate.png')} | Out-Null
  Ctl @{cmd='quit'} | Out-Null
} finally { Start-Sleep -Milliseconds 300; if (-not $p.HasExited) { $p.Kill() } }
"`n$script:pass passed, $script:fail failed"
if ($script:fail -gt 0) { exit 1 }
