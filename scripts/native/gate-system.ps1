# System peeks: injected events through the real window (manual clock), then a check that the
# real Windows sources register. The injected part moves nothing on your PC; the real part only
# registers listeners and does not wait for anything to happen.
#   powershell -File scripts\native\gate-system.ps1 [-Exe path\to\blink.exe]
param([string]$Exe = '')
$ErrorActionPreference = 'Stop'
. "$PSScriptRoot\blink-ctl.ps1"
if ($Exe) { $env:BLINK_EXE = (Resolve-Path $Exe).Path }
$script:pass = 0; $script:fail = 0
function Check([string]$what, [bool]$ok, [string]$detail = '') { if ($ok) { $script:pass++; "  ok    $what" } else { $script:fail++; "  FAIL  $what  $detail" } }
$out = (Resolve-Path (Join-Path $PSScriptRoot '..\out')).Path
$data = Join-Path $env:TEMP 'blink-system-data'; if (Test-Path $data) { Remove-Item $data -Recurse -Force }; New-Item -ItemType Directory -Force $data | Out-Null
$p = Start-Blink 'sys' @{ BLINK_MANUAL_CLOCK = '1'; BLINK_DATA_DIR = $data; BLINK_TZ = 'Asia/Jakarta'; BLINK_DEMO = '1' }
try {
  $c = Connect-Blink 'sys'
  function Ctl($o) { Send-Blink $c $o }
  function St() { (Ctl @{cmd='state'}).state }
  function Ev($o) { $o.cmd = 'system_event'; Ctl $o | Out-Null; Ctl @{cmd='advance'; ms=300} | Out-Null }
  function Calm() { Ctl @{cmd='advance'; ms=7000} | Out-Null; Ctl @{cmd='advance'; ms=600} | Out-Null }

  'the system module is on, without a tab'
  $m = Ctl @{cmd='modules'}
  Check 'it is in the module list' (@($m.modules | Where-Object { $_.id -eq 'system' }).Count -eq 1)
  Check 'and never owns the pill or a panel' ($m.restOwner -ne 'system' -and $m.active -ne 'system')

  'each event, as a peek'
  Ev @{type='volume'; percent=40; muted=$false}
  $s = St
  Check 'volume: a peek with the level and a bar' ($s.mode -eq 'peek' -and $s.peek.count -eq '40%' -and [math]::Abs($s.peek.progress - 0.4) -lt 0.001) ($s.peek | ConvertTo-Json -Compress)
  Ctl @{cmd='dump'; path="$out\peek-volume.png"} | Out-Null
  Ev @{type='volume'; percent=45; muted=$false}; Ev @{type='volume'; percent=50; muted=$false}
  Check 'more volume steps update the same peek' ((St).peek.count -eq '50%' -and (St).queuedPeeks -eq 0)
  Ev @{type='volume'; percent=50; muted=$true}
  Check 'mute says Muted' ((St).peek.count -eq 'Muted')
  Calm
  Check 'it goes away by itself, quickly' ((St).mode -eq 'rest')

  Ev @{type='power'; onAc=$true; percent=70; charging=$true}
  Check 'the first power report is only a baseline' ((St).mode -eq 'rest')
  Ev @{type='power'; onAc=$false; percent=70; charging=$false}
  $s = St
  Check 'unplugging the charger says so, with the charge' ($s.peek.title -eq 'Charger removed' -and $s.peek.count -eq '70%') ($s.peek | ConvertTo-Json -Compress)
  Ctl @{cmd='dump'; path="$out\peek-battery.png"} | Out-Null
  Calm
  Ev @{type='power'; onAc=$false; percent=20; charging=$false}
  Check 'battery low at 20%' ((St).peek.title -eq 'Battery low')
  Calm
  Ev @{type='power'; onAc=$false; percent=10; charging=$false}
  Check 'battery almost empty at 10%, as an urgent peek' ((St).peek.title -eq 'Battery almost empty' -and (St).peek.priority -eq 3)
  Calm

  Ev @{type='audio'; name='AirBuds Pro'; kind='headphones'; connected=$true}
  $s = St
  Check 'headphones connected' ($s.peek.code -eq 'Headphones' -and $s.peek.title -eq 'AirBuds Pro' -and $s.peek.count -eq 'connected') ($s.peek | ConvertTo-Json -Compress)
  Ctl @{cmd='dump'; path="$out\peek-headphones.png"} | Out-Null
  Calm
  Ev @{type='bluetooth'; name='AirBuds Pro'; connected=$false; audio=$true}
  Ev @{type='bluetooth'; name='AirBuds Pro'; connected=$true; audio=$true}
  $s = St
  Check 'Bluetooth earbuds connected: a headphones peek' ($s.peek.code -eq 'Headphones' -and $s.peek.title -eq 'AirBuds Pro' -and $s.peek.count -eq 'connected') ($s.peek | ConvertTo-Json -Compress)
  Ev @{type='bluetooth'; name='AirBuds Pro'; connected=$true; battery=70; audio=$true}
  $s = St
  Check 'their battery level fills in the same peek, with a bar' ($s.peek.count -eq '70%' -and [math]::Abs($s.peek.progress - 0.7) -lt 0.001 -and $s.queuedPeeks -eq 0) ($s.peek | ConvertTo-Json -Compress)
  Ctl @{cmd='dump'; path="$out\peek-bluetooth.png"} | Out-Null
  Ev @{type='audio'; name='Headphones (AirBuds Pro Stereo)'; kind='headphones'; connected=$true}
  Check 'their audio output does not announce the same thing again' ((St).queuedPeeks -eq 0 -and (St).peek.count -eq '70%')
  Calm
  Ev @{type='bluetooth'; name='Pad'; connected=$false}
  Ev @{type='bluetooth'; name='Pad'; connected=$true}
  Check 'another Bluetooth device (a controller, a mouse) connected' ((St).peek.code -eq 'Bluetooth' -and (St).peek.title -eq 'Pad')
  Calm
  Ev @{type='bluetooth'; name='Pad'; connected=$false}
  Check 'and disconnected' ((St).peek.count -eq 'disconnected')
  Calm
  Ev @{type='wifi'; connected=$true; ssid='Campus'}
  Check 'Wi-Fi connected, with the network name' ((St).peek.title -eq 'Connected to Campus')
  Calm
  Ev @{type='wifi'; connected=$false}
  Check 'Wi-Fi disconnected' ((St).peek.title -eq 'Wi-Fi disconnected')
  Calm
  Ev @{type='caps'; on=$true}
  Check 'Caps Lock on' ((St).peek.title -eq 'Caps Lock on')
  Calm
  Ev @{type='layout'; code='ID'; name='Indonesian'}
  Check 'keyboard layout change' ((St).peek.title -eq 'Indonesian' -and (St).peek.count -eq 'ID')
  Calm

  'a deadline alert beats a volume change'
  Ev @{type='volume'; percent=20; muted=$false}
  Ctl @{cmd='offer_peek'; title='Quiz in 1h'; code='MA201'; count='1h'; priority=3} | Out-Null
  Check 'the alert replaces it' ((St).peek.title -eq 'Quiz in 1h')
  Ev @{type='volume'; percent=25; muted=$false}
  Check 'and the next volume step waits instead of interrupting' ((St).peek.title -eq 'Quiz in 1h' -and (St).queuedPeeks -eq 1)
  Calm; Calm

  'switching kinds off'
  Ctl @{cmd='host'; config=@{ modules = @(@{id='deadlines'; enabled=$true}, @{id='clock'; enabled=$true}, @{id='system'; enabled=$true; options=@{ volume=$false; caps=$false }}) }} | Out-Null
  Ev @{type='volume'; percent=60; muted=$false}; Ev @{type='caps'; on=$false}
  Check 'volume and Caps Lock peeks are off' ((St).mode -eq 'rest')
  Ev @{type='audio'; name='Speakers'; kind='speakers'; connected=$true}
  Check 'the others still work' ((St).peek.code -eq 'Speakers')
  Calm
  Ctl @{cmd='settings'; patch=@{language='id'}} | Out-Null
  Ev @{type='wifi'; connected=$false}
  Check 'Indonesian words follow the language setting' ((St).peek.title -eq 'Wi-Fi terputus') ((St).peek.title)
  Ctl @{cmd='quit'} | Out-Null
} finally { Start-Sleep -Milliseconds 300; if (-not $p.HasExited) { $p.Kill() } }

'the real Windows sources'
$p2 = Start-Blink 'sys2' @{ BLINK_REAL_SOURCES = '1'; BLINK_DATA_DIR = $data; BLINK_TZ = 'Asia/Jakarta'; BLINK_DEMO = '1' }
try {
  $c = Connect-Blink 'sys2'
  Start-Sleep -Milliseconds 800
  $w = (Send-Blink $c @{cmd='state'}).window
  Check 'audio devices and volume are being watched' ($w.sources.audio -eq $true)
  Check 'the keyboard source is registered' ($w.sources.keys -eq $true)
  Check 'the Bluetooth watcher is running' ($w.sources.bluetooth -eq $true) ($w.sources | ConvertTo-Json -Compress)
  "        power: $($w.sources.power)   wi-fi: $($w.sources.wifi)   (false is normal on a desktop PC without a battery or Wi-Fi card)"
  Send-Blink $c @{cmd='quit'} | Out-Null
} finally { Start-Sleep -Milliseconds 300; if (-not $p2.HasExited) { $p2.Kill() } }
"`n$script:pass passed, $script:fail failed"
if ($script:fail -gt 0) { exit 1 }
