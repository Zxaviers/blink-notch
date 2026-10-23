param([string]$Exe = "$PSScriptRoot\..\..\target-alt\release\blink.exe")
. "$PSScriptRoot\blink-ctl.ps1"
$env:BLINK_EXE = (Resolve-Path $Exe).Path
$out = (Resolve-Path (Join-Path $PSScriptRoot '..\out')).Path
$data = Join-Path $env:TEMP 'blink-shots-ctl'; New-Item -ItemType Directory -Force $data | Out-Null
$p = Start-Blink 'shotsc' @{ BLINK_MANUAL_CLOCK='1'; BLINK_DATA_DIR=$data; BLINK_TZ='Asia/Jakarta'; BLINK_DEMO='1' }
try {
  $c = Connect-Blink 'shotsc'
  function Ctl($o) { Send-Blink $c $o }
  Ctl @{cmd='mouse'; kind='move'; x=320; y=10} | Out-Null; Ctl @{cmd='advance'; ms=900} | Out-Null
  Ctl @{cmd='switch'; to='controls'} | Out-Null; Ctl @{cmd='advance'; ms=300} | Out-Null
  Ctl @{cmd='dump'; path="$out\controls.png"} | Out-Null
  Ctl @{cmd='quit'} | Out-Null
} finally { Start-Sleep -Milliseconds 300; if (!$p.HasExited) { $p.Kill() } }
