. "$PSScriptRoot\blink-ctl.ps1"
$out = Join-Path $PSScriptRoot '..\out'; New-Item -ItemType Directory -Force $out | Out-Null
$data = Join-Path $env:TEMP 'blink-smoke-data'; New-Item -ItemType Directory -Force $data | Out-Null
$p = Start-Blink 'smoke' @{ BLINK_MANUAL_CLOCK = '1'; BLINK_DATA_DIR = $data; BLINK_TZ = 'Asia/Jakarta'; BLINK_DEMO = '1' }
try {
  $c = Connect-Blink 'smoke'
  $s = Send-Blink $c @{cmd='state'}
  "initial: mode=$($s.state.mode) rest=$($s.state.rest.label) $($s.state.rest.count) window=$($s.window.rect.x),$($s.window.rect.y) $($s.window.rect.w)x$($s.window.rect.h) layered=$($s.window.layered) noActivate=$($s.window.noActivate) fg=$($s.window.foreground)"
  $h = Send-Blink $c @{cmd='hit'; x=320; y=10}; "hit centre: alpha=$($h.alpha) ours=$($h.ours)"
  $h = Send-Blink $c @{cmd='hit'; x=50; y=10}; "hit far left: alpha=$($h.alpha) ours=$($h.ours) class=$($h.windowClass)"
  Send-Blink $c @{cmd='mouse'; kind='move'; x=320; y=10} | Out-Null
  Send-Blink $c @{cmd='advance'; ms=700} | Out-Null
  $s = Send-Blink $c @{cmd='state'}; "after hover: mode=$($s.state.mode) shape=$($s.state.shape.w)x$($s.state.shape.h)"
  $d = (Resolve-Path $out).Path + '\smoke-open.png'
  Send-Blink $c @{cmd='dump'; path=$d} | Out-Null; "dumped $d"
  Send-Blink $c @{cmd='quit'} | Out-Null
} finally { Start-Sleep -Milliseconds 300; if (!$p.HasExited) { $p.Kill() } }
