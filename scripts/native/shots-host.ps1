. "$PSScriptRoot\blink-ctl.ps1"
$out = (Resolve-Path (Join-Path $PSScriptRoot '..\out')).Path
$data = Join-Path $env:TEMP 'blink-shots-host'; New-Item -ItemType Directory -Force $data | Out-Null
$p = Start-Blink 'shots' @{ BLINK_MANUAL_CLOCK='1'; BLINK_DATA_DIR=$data; BLINK_TZ='Asia/Jakarta'; BLINK_DEMO='1' }
try {
  $c = Connect-Blink 'shots'
  function Ctl($o) { Send-Blink $c $o }
  Ctl @{cmd='mouse'; kind='move'; x=320; y=10} | Out-Null; Ctl @{cmd='advance'; ms=900} | Out-Null
  Ctl @{cmd='dump'; path="$out\host-deadlines.png"} | Out-Null
  Ctl @{cmd='key'; key='right'} | Out-Null; Ctl @{cmd='advance'; ms=500} | Out-Null
  Ctl @{cmd='dump'; path="$out\host-clock.png"} | Out-Null
  Ctl @{cmd='mouse'; kind='leave'} | Out-Null; Ctl @{cmd='advance'; ms=1500} | Out-Null
  Ctl @{cmd='settings'; patch=@{restingStyle='when-due'; dueHorizonHours=1}} | Out-Null; Ctl @{cmd='advance'; ms=800} | Out-Null
  Ctl @{cmd='dump'; path="$out\host-rest-clock.png"} | Out-Null
  Ctl @{cmd='quit'} | Out-Null
} finally { Start-Sleep -Milliseconds 300; if (!$p.HasExited) { $p.Kill() } }
