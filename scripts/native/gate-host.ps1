# The host with several modules, through the real window and the control pipe (manual clock):
# which module owns the pill, tabs, switching, the module list written to host.json, and the
# peek priority rules. No cursor, no keyboard.
#   powershell -File scripts\native\gate-host.ps1 [-Exe path\to\blink.exe]
param([string]$Exe = '')
$ErrorActionPreference = 'Stop'
. "$PSScriptRoot\blink-ctl.ps1"
if ($Exe) { $env:BLINK_EXE = (Resolve-Path $Exe).Path }
$script:pass = 0; $script:fail = 0
function Check([string]$what, [bool]$ok, [string]$detail = '') { if ($ok) { $script:pass++; "  ok    $what" } else { $script:fail++; "  FAIL  $what  $detail" } }
$data = Join-Path $env:TEMP 'blink-host-data'; if (Test-Path $data) { Remove-Item $data -Recurse -Force }; New-Item -ItemType Directory -Force $data | Out-Null
$p = Start-Blink 'host' @{ BLINK_MANUAL_CLOCK = '1'; BLINK_DATA_DIR = $data; BLINK_TZ = 'Asia/Jakarta'; BLINK_DEMO = '1' }
try {
  $c = Connect-Blink 'host'
  function Ctl($o) { Send-Blink $c $o }
  function St() { (Ctl @{cmd='state'}).state }

  'modules and the pill'
  $m = Ctl @{cmd='modules'}
  Check 'the clock, deadlines, controls, music and the system peeks are on, in that order' (($m.modules | ForEach-Object { "$($_.id):$($_.enabled)" }) -join ',' -eq 'clock:True,deadlines:True,controls:True,music:True,projects:True,system:True') (($m.modules | ConvertTo-Json -Compress))
  Check 'the nearest deadline owns the pill' ($m.restOwner -eq 'deadlines')
  Ctl @{cmd='settings'; patch=@{restingStyle='when-due'; dueHorizonHours=1}} | Out-Null
  Ctl @{cmd='advance'; ms=600} | Out-Null
  $s = St
  Check 'with nothing near, the clock takes the pill' ((Ctl @{cmd='modules'}).restOwner -eq 'clock' -and $s.rest.count -eq '19:00') "$($s.rest.count)"
  Check 'and the pill keeps its normal size' ($s.shape.w -eq 232)
  Ctl @{cmd='mouse'; kind='move'; x=320; y=10} | Out-Null
  Ctl @{cmd='advance'; ms=800} | Out-Null
  $s = St
  Check 'hovering the clock pill opens the clock' ((Ctl @{cmd='modules'}).active -eq 'clock' -and $s.mode -eq 'open')
  Check 'with two modules on, the panel has tabs' ($s.tabs -eq $true)
  Ctl @{cmd='key'; key='left'} | Out-Null
  Check 'the left arrow goes to the previous tab' ((Ctl @{cmd='modules'}).active -eq 'projects')
  Ctl @{cmd='switch'; to='clock'} | Out-Null
  Check 'switching by name works, and a wrong name is refused' (((Ctl @{cmd='modules'}).active -eq 'clock') -and (Send-Blink $c @{cmd='switch'; to='nope'}).ok -eq $false)

  'turning modules off from the host file'
  Ctl @{cmd='mouse'; kind='leave'} | Out-Null; Ctl @{cmd='advance'; ms=1500} | Out-Null
  Ctl @{cmd='host'; config=@{ modules = @(@{id='clock'; enabled=$true}, @{id='deadlines'; enabled=$false}, @{id='controls'; enabled=$false}, @{id='music'; enabled=$false}, @{id='projects'; enabled=$false}) }} | Out-Null
  $m = Ctl @{cmd='modules'}
  Check 'the order and flags are applied' (($m.modules | ForEach-Object { "$($_.id):$($_.enabled)" }) -join ',' -eq 'clock:True,deadlines:False,controls:False,music:False,projects:False,system:True')
  Check 'host.json was written' (Test-Path (Join-Path $data 'host.json'))
  Check 'with one module on there are no tabs and the clock owns the pill' ((St).tabs -eq $false -and $m.restOwner -eq 'clock')
  Ctl @{cmd='host'; config=@{ modules = @(@{id='clock'; enabled=$false}, @{id='deadlines'; enabled=$false}, @{id='controls'; enabled=$false}) }} | Out-Null
  Check 'turning everything off still leaves one module' (@((Ctl @{cmd='modules'}).modules | Where-Object { $_.enabled -and $_.id -ne 'system' }).Count -eq 1)
  Ctl @{cmd='host'; config=@{ modules = @(@{id='deadlines'; enabled=$true}, @{id='controls'; enabled=$true}, @{id='clock'; enabled=$true}) }} | Out-Null

  'peeks'
  Ctl @{cmd='offer_peek'; title='Charger connected'; code='Battery'; count='70%'; priority=1; key='power'} | Out-Null
  $s = St
  Check 'a peek shows' ($s.mode -eq 'peek')
  Ctl @{cmd='offer_peek'; title='Problem Set 4'; code='CS101'; count='3h'; priority=3} | Out-Null
  Ctl @{cmd='offer_peek'; title='Wi-Fi lost'; code='Network'; count=''; priority=1; key='net'} | Out-Null
  Check 'a lesser peek waits behind a more important one' ((St).queuedPeeks -eq 1)
  Ctl @{cmd='advance'; ms=4300} | Out-Null; Ctl @{cmd='advance'; ms=500} | Out-Null
  Check 'and shows when the notch is free' ((St).mode -eq 'peek' -and (St).queuedPeeks -eq 0)
  Ctl @{cmd='advance'; ms=4500} | Out-Null; Ctl @{cmd='advance'; ms=600} | Out-Null
  Check 'then it rests again' ((St).mode -eq 'rest')

  'show and hide the notch from the keyboard'
  $w = (St); $win = (Ctl @{cmd='state'}).window
  Check 'there is a show/hide shortcut, Ctrl+Alt+H by default' ($win.visibilityHotkey -eq 'Ctrl+Alt+H' -or $win.visibilityHotkey -eq '') ($win.visibilityHotkey)
  Ctl @{cmd='toggle_visibility'} | Out-Null
  $win = (Ctl @{cmd='state'}).window
  Check 'the shortcut hides it' ($win.hidden -eq $true -and $win.hiddenBy -eq 'paused' -and $win.visible -eq $false) ($win | ConvertTo-Json -Compress)
  Ctl @{cmd='toggle_visibility'} | Out-Null
  $win = (Ctl @{cmd='state'}).window
  Check 'and shows it again' ($win.hidden -eq $false -and $win.visible -eq $true)
  Ctl @{cmd='fullscreen'; on=$true} | Out-Null
  $win = (Ctl @{cmd='state'}).window
  Check 'a fullscreen app hides it, and the state says why' ($win.hidden -eq $true -and $win.hiddenBy -eq 'fullscreen')
  Ctl @{cmd='toggle_visibility'} | Out-Null
  $win = (Ctl @{cmd='state'}).window
  Check 'the shortcut brings it back even over a fullscreen app' ($win.hidden -eq $false -and $win.visible -eq $true)
  Ctl @{cmd='fullscreen'; on=$false} | Out-Null
  Ctl @{cmd='fullscreen'; on=$true} | Out-Null
  Check 'after that app is gone, the next fullscreen app hides it again as usual' ((Ctl @{cmd='state'}).window.hiddenBy -eq 'fullscreen')
  Ctl @{cmd='fullscreen'; on=$false} | Out-Null
  Check 'and leaving fullscreen shows it' ((Ctl @{cmd='state'}).window.hidden -eq $false)

  'fullscreen: only games hide it by default'
  Ctl @{cmd='fullscreen'; on=$true; kind='app'} | Out-Null
  Check 'an ordinary app that covers the screen (a browser, a chat app) does not hide it' ((Ctl @{cmd='state'}).window.hidden -eq $false)
  $mods = @(@{id='deadlines'; enabled=$true}, @{id='controls'; enabled=$true}, @{id='clock'; enabled=$true}, @{id='system'; enabled=$true})
  Ctl @{cmd='host'; config=@{ hideInFullscreen='all'; modules=$mods }} | Out-Null
  Check 'unless the user asks for it to hide for any fullscreen app' ((Ctl @{cmd='state'}).window.hiddenBy -eq 'fullscreen')
  Ctl @{cmd='host'; config=@{ hideInFullscreen='never'; modules=$mods }} | Out-Null
  Ctl @{cmd='fullscreen'; on=$true} | Out-Null
  Check 'or never to hide, even for a game' ((Ctl @{cmd='state'}).window.hidden -eq $false)
  Ctl @{cmd='host'; config=@{ modules=$mods }} | Out-Null
  Check 'back to the default, a game hides it' ((Ctl @{cmd='state'}).window.hiddenBy -eq 'fullscreen')
  Ctl @{cmd='fullscreen'; on=$false} | Out-Null

  'drawing'
  Ctl @{cmd='mouse'; kind='move'; x=320; y=10} | Out-Null; Ctl @{cmd='advance'; ms=800} | Out-Null
  $dib = Ctl @{cmd='dibcheck'}
  Check 'the window shows exactly what the app drew, tabs included' ($dib.differing -eq 0) "differing=$($dib.differing)"
  Ctl @{cmd='quit'} | Out-Null
} finally { Start-Sleep -Milliseconds 300; if (-not $p.HasExited) { $p.Kill() } }
"`n$script:pass passed, $script:fail failed"
if ($script:fail -gt 0) { exit 1 }
