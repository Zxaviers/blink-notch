# Drawing checks in real time (the manual-clock gates never draw mid-animation): after real
# open and close animations, and while they run, the window's bitmap must equal the app's frame
# exactly. A trail of stale rows from earlier frames shows up as differing pixels.
#   powershell -File scripts\native\gate-draw.ps1 [-Exe path\to\blink.exe]
param([string]$Exe = '')
$ErrorActionPreference = 'Stop'
. "$PSScriptRoot\blink-ctl.ps1"
if ($Exe) { $env:BLINK_EXE = (Resolve-Path $Exe).Path }
$script:pass = 0; $script:fail = 0
function Check([string]$what, [bool]$ok, [string]$detail = '') { if ($ok) { $script:pass++; "  ok    $what" } else { $script:fail++; "  FAIL  $what  $detail" } }
$data = Join-Path $env:TEMP 'blink-draw-data'; New-Item -ItemType Directory -Force $data | Out-Null
$p = Start-Blink 'draw' @{ BLINK_DATA_DIR = $data; BLINK_TZ = 'Asia/Jakarta'; BLINK_DEMO = '1' }
try {
  $c = Connect-Blink 'draw'
  function Ctl($o) { Send-Blink $c $o }
  Start-Sleep -Milliseconds 500
  $bad = 0; $checks = 0
  for ($i = 0; $i -lt 6; $i++) {
    Ctl @{cmd='mouse'; kind='move'; x=320; y=10} | Out-Null
    # while it opens (about 150 ms of hover intent, then 280 ms of motion), look every ~25 ms
    for ($t = 0; $t -lt 24; $t++) { $r = Ctl @{cmd='dibcheck'}; $checks++; if ($r.differing -gt 0) { $bad++ }; Start-Sleep -Milliseconds 25 }
    Start-Sleep -Milliseconds 400
    Ctl @{cmd='mouse'; kind='leave'} | Out-Null
    for ($t = 0; $t -lt 60; $t++) { $r = Ctl @{cmd='dibcheck'}; $checks++; if ($r.differing -gt 0) { $bad++ }; Start-Sleep -Milliseconds 25 }
    $r = Ctl @{cmd='dibcheck'}
    Check "cycle $($i + 1): after closing, the bitmap equals the frame" ($r.differing -eq 0) "differing=$($r.differing) firstRow=$($r.firstRow)"
  }
  Check "during $checks mid-animation checks the bitmap always equalled the frame" ($bad -eq 0) "$bad checks differed"
  $s = Ctl @{cmd='state'}
  Check 'it ends at rest' ($s.state.mode -eq 'rest')
  Ctl @{cmd='quit'} | Out-Null
} finally { Start-Sleep -Milliseconds 300; if (-not $p.HasExited) { $p.Kill() } }
"`n$script:pass passed, $script:fail failed"
if ($script:fail -gt 0) { exit 1 }
