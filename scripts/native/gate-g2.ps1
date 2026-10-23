# Gate G2 checks that need no mouse or keyboard: they drive blink.exe through its
# control pipe, and read hit-testing from Windows itself (WindowFromPoint).
#   powershell -File scripts\native\gate-g2.ps1 [-Exe path\to\blink.exe]
# Real-input checks (a real click, the real hotkey) are separate: scripts\native\real-input.ps1.
param([string]$Exe = '')
$ErrorActionPreference = 'Stop'
. "$PSScriptRoot\blink-ctl.ps1"
if ($Exe) { $env:BLINK_EXE = (Resolve-Path $Exe).Path }

$script:pass = 0; $script:fail = 0
function Check([string]$what, [bool]$ok, [string]$detail = '') {
  if ($ok) { $script:pass++; "  ok    $what" } else { $script:fail++; "  FAIL  $what  $detail" }
}
function Cmd($c, $o) { Send-Blink $c $o }

$data = Join-Path $env:TEMP 'blink-g2-data'; New-Item -ItemType Directory -Force $data | Out-Null
$envs = @{ BLINK_MANUAL_CLOCK = '1'; BLINK_DATA_DIR = $data; BLINK_TZ = 'Asia/Jakarta'; BLINK_DEMO = '1' }

# ------------------------------------------------------------ window and click-through
'window and click-through'
$p1 = Start-Blink 'g2a' $envs
try {
  $c = Connect-Blink 'g2a'
  $s = Cmd $c @{cmd='state'}
  $w = $s.window
  Check 'layered + topmost + tool window + no-activate' ($w.layered -and $w.topmost -and $w.toolWindow -and $w.noActivate)
  Check 'not the foreground window after launch' (-not $w.foreground)
  Check 'visible' $w.visible
  Check 'window is 640x360 at scale 1' ($w.rect.w -eq 640 -and $w.rect.h -eq 360)

  # Resting pill is 232x32 centred in 640 wide: x 204..436, y 0..32.
  $inside = Cmd $c @{cmd='hit'; x=320; y=10}
  Check 'pill centre: alpha 255 and Windows delivers to us' ($inside.alpha -eq 255 -and $inside.ours)
  foreach ($pt in @(@(100,10), @(540,10), @(320,60), @(320,300), @(210,40), @(639,359))) {
    $h = Cmd $c @{cmd='hit'; x=$pt[0]; y=$pt[1]}
    Check "transparent at ($($pt[0]),$($pt[1])): alpha 0 and click passes through" ($h.alpha -eq 0 -and -not $h.ours) "alpha=$($h.alpha) ours=$($h.ours) class=$($h.windowClass)"
  }
  $corner = Cmd $c @{cmd='hit'; x=205; y=31}
  Check 'rounded bottom corner is see-through' ($corner.alpha -lt 255 -and -not $corner.ours -or $corner.alpha -eq 0) "alpha=$($corner.alpha)"

  Cmd $c @{cmd='mouse'; kind='move'; x=320; y=10} | Out-Null
  Cmd $c @{cmd='advance'; ms=800} | Out-Null
  $s = Cmd $c @{cmd='state'}
  Check 'hover opens the panel' ($s.state.mode -eq 'open') $s.state.mode
  $h = Cmd $c @{cmd='hit'; x=320; y=150}
  Check 'open panel takes the mouse' ($h.alpha -eq 255 -and $h.ours)
  $h = Cmd $c @{cmd='hit'; x=100; y=200}
  Check 'beside the open panel still passes through' ($h.alpha -eq 0 -and -not $h.ours)
  Cmd $c @{cmd='mouse'; kind='leave'} | Out-Null
  Cmd $c @{cmd='advance'; ms=1000} | Out-Null
  $s = Cmd $c @{cmd='state'}
  Check 'leaving closes it' ($s.state.mode -eq 'rest') $s.state.mode

  # Hot strip: invisible but hit-testable.
  Cmd $c @{cmd='settings'; patch=@{restingStyle='hover'}} | Out-Null
  Cmd $c @{cmd='advance'; ms=600} | Out-Null
  $s = Cmd $c @{cmd='state'}
  Check 'hover-only style rests as a hot strip' ($s.state.restStyle -eq 'hot') $s.state.restStyle
  $h = Cmd $c @{cmd='hit'; x=320; y=2}
  Check 'hot strip is hit-testable (alpha 1) though invisible' ($h.alpha -eq 1 -and $h.ours) "alpha=$($h.alpha) ours=$($h.ours)"
  $h = Cmd $c @{cmd='hit'; x=320; y=12}
  $s = Cmd $c @{cmd='state'}
  Check 'below the hot strip passes through' ($h.alpha -eq 0 -and -not $h.ours) "alpha=$($h.alpha) shape=$($s.state.shape.w)x$($s.state.shape.h) mode=$($s.state.mode) animating=$($s.state.animating)" 
  Cmd $c @{cmd='settings'; patch=@{}} | Out-Null

  # ---------------------------------------------------------------- keyboard mode (simulated focus)
  'keyboard mode (focus simulated; real focus is in real-input.ps1)'
  Cmd $c @{cmd='hotkey'} | Out-Null
  Cmd $c @{cmd='advance'; ms=600} | Out-Null
  $s = Cmd $c @{cmd='state'}
  Check 'hotkey opens the panel in keyboard mode' ($s.state.mode -eq 'open' -and $s.state.keyboard)
  Cmd $c @{cmd='key'; key='down'} | Out-Null
  $s = Cmd $c @{cmd='state'}
  Check 'arrow selects a row' ($null -ne $s.state.module.selected) ($s.state.module | ConvertTo-Json -Compress -Depth 4)
  Cmd $c @{cmd='mouse'; kind='leave'} | Out-Null
  Cmd $c @{cmd='advance'; ms=2000} | Out-Null
  $s = Cmd $c @{cmd='state'}
  Check 'keyboard mode stays open when the mouse is elsewhere' ($s.state.mode -eq 'open')
  Cmd $c @{cmd='key'; key='escape'} | Out-Null
  Cmd $c @{cmd='advance'; ms=800} | Out-Null
  $s = Cmd $c @{cmd='state'}
  Check 'Escape closes and releases focus' ($s.state.mode -eq 'rest' -and -not $s.state.keyboard)

  # ------------------------------------------------------------------------------- fullscreen / pause
  'fullscreen and pause'
  Cmd $c @{cmd='fullscreen'; on=$true} | Out-Null
  $s = Cmd $c @{cmd='state'}
  Check 'fullscreen app hides the window' ($s.window.hidden -and -not $s.window.visible)
  Cmd $c @{cmd='fullscreen'; on=$false} | Out-Null
  $s = Cmd $c @{cmd='state'}
  Check 'and it comes back' ((-not $s.window.hidden) -and $s.window.visible)
  Cmd $c @{cmd='pause'; on=$true} | Out-Null
  $s = Cmd $c @{cmd='state'}
  Check 'pause hides' ($s.window.hidden -and -not $s.window.visible)
  Cmd $c @{cmd='pause'; on=$false} | Out-Null
  $s = Cmd $c @{cmd='state'}
  Check 'resume shows' ($s.window.visible)
  Check 'tray icon was added' $s.window.tray

  # ------------------------------------------------------------------------------------------------ DPI
  'DPI scaling'
  foreach ($scale in 1.25, 1.5, 2.0) {
    Cmd $c @{cmd='scale'; value=$scale} | Out-Null
    $s = Cmd $c @{cmd='state'}
    $wantW = [math]::Round(640 * $scale); $wantH = [math]::Round(360 * $scale)
    Check "scale $scale -> window ${wantW}x${wantH}" ($s.window.rect.w -eq $wantW -and $s.window.rect.h -eq $wantH) "$($s.window.rect.w)x$($s.window.rect.h)"
    $h = Cmd $c @{cmd='hit'; x=320; y=10}
    Check "scale $scale -> pill still hit at logical centre" ($h.alpha -eq 255 -and $h.ours)
    $h = Cmd $c @{cmd='hit'; x=100; y=10}
    Check "scale $scale -> outside still passes through" ($h.alpha -eq 0 -and -not $h.ours)
    $left = [math]::Round(([math]::Round(640 * $scale) - 232 * $scale) / 2)
    Check "scale $scale -> window stays centred on its monitor" ($s.window.rect.x -gt 0)
  }
  Cmd $c @{cmd='scale'; value=1.0} | Out-Null

  # ------------------------------------------------------------------ reduced motion
  'reduced motion'
  Cmd $c @{cmd='reduced_motion'; on=$true} | Out-Null
  Cmd $c @{cmd='mouse'; kind='move'; x=320; y=10} | Out-Null
  Cmd $c @{cmd='advance'; ms=160} | Out-Null
  Cmd $c @{cmd='advance'; ms=60} | Out-Null
  $s = Cmd $c @{cmd='state'}
  Check 'panel snaps open (no 280 ms tween) with reduced motion' ($s.state.mode -eq 'open' -and -not $s.state.animating) "animating=$($s.state.animating) h=$($s.state.shape.h)"
  Cmd $c @{cmd='reduced_motion'; on=$false} | Out-Null

  # ---------------------------------------------------------------- hotkey registration codes
  'hotkey registration'
  $r = Cmd $c @{cmd='register_hotkey'; accelerator='Ctrl+Alt+Shift+F9'}
  Check 'a free hotkey registers' ($r.hotkey -eq 'ok') $r.hotkey
  $r = Cmd $c @{cmd='register_hotkey'; accelerator='Banana+Z'}
  Check 'garbage is HOTKEY_INVALID' ($r.hotkey -eq 'HOTKEY_INVALID') $r.hotkey
  $p2 = Start-Blink 'g2b' $envs
  try {
    $c2 = Connect-Blink 'g2b'
    $r2 = Cmd $c2 @{cmd='register_hotkey'; accelerator='Ctrl+Alt+Shift+F9'}
    Check 'the same hotkey in another process is HOTKEY_IN_USE' ($r2.hotkey -eq 'HOTKEY_IN_USE') $r2.hotkey
    $s2 = Cmd $c2 @{cmd='state'}
    Check 'a failed bind leaves the old (none) in place' ($s2.window.hotkey -ne 'Ctrl+Alt+Shift+F9')
    Cmd $c2 @{cmd='quit'} | Out-Null
  } finally { Start-Sleep -Milliseconds 300; if (-not $p2.HasExited) { $p2.Kill() } }
  Cmd $c @{cmd='quit'} | Out-Null
} finally { Start-Sleep -Milliseconds 300; if (-not $p1.HasExited) { $p1.Kill() } }

# ------------------------------------------------------------------------ single instance
'single instance'
$exe = if ($env:BLINK_EXE) { $env:BLINK_EXE } else { Join-Path $PSScriptRoot '..\..\target\debug\blink.exe' }
$env:BLINK_DATA_DIR = $data
$env:BLINK_SETTINGS_EXE = 'none'   # a notch with no feeds would otherwise open the settings program
$a = Start-Process -FilePath $exe -ArgumentList '--seconds', '6' -PassThru
Start-Sleep -Milliseconds 800
$b = Start-Process -FilePath $exe -ArgumentList '--seconds', '6' -PassThru
Start-Sleep -Milliseconds 1200
Check 'a second launch exits at once' $b.HasExited
Check 'the first keeps running' (-not $a.HasExited)
if (-not $a.HasExited) { $a.Kill() }
if (-not $b.HasExited) { $b.Kill() }

"`n$script:pass passed, $script:fail failed"
if ($script:fail -gt 0) { exit 1 }

