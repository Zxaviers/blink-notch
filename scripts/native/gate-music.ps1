# The music page through the real window with a stand-in player and stand-in lyrics (nothing
# plays, nothing leaves the computer, no cursor, no keyboard).
#   powershell -File scripts\native\gate-music.ps1 [-Exe path\to\blink.exe]
param([string]$Exe = '')
$ErrorActionPreference = 'Stop'
. "$PSScriptRoot\blink-ctl.ps1"
if ($Exe) { $env:BLINK_EXE = (Resolve-Path $Exe).Path }
Remove-Item Env:BLINK_REAL_SOURCES -ErrorAction SilentlyContinue
$script:pass = 0; $script:fail = 0
function Check([string]$what, [bool]$ok, [string]$detail = '') { if ($ok) { $script:pass++; "  ok    $what" } else { $script:fail++; "  FAIL  $what  $detail" } }
$data = Join-Path $env:TEMP 'blink-music-data'; if (Test-Path $data) { Remove-Item $data -Recurse -Force }; New-Item -ItemType Directory -Force $data | Out-Null
$p = Start-Blink 'music' @{ BLINK_MANUAL_CLOCK = '1'; BLINK_DATA_DIR = $data; BLINK_TZ = 'Asia/Jakarta'; BLINK_DEMO = '1' }
try {
  $c = Connect-Blink 'music'
  function Ctl($o) { Send-Blink $c $o }
  function Mod() { (Ctl @{cmd='state'}).state.module }
  function Click($target) { $h = (Mod).hits | Where-Object { $_.target -eq $target } | Select-Object -First 1; if (-not $h) { throw "no control $target" }; $x = $h.x + $h.w / 2; $y = $h.y + $h.h / 2; Ctl @{cmd='mouse'; kind='move'; x=$x; y=$y} | Out-Null; Ctl @{cmd='mouse'; kind='down'; x=$x; y=$y} | Out-Null; Ctl @{cmd='mouse'; kind='up'; x=$x; y=$y} | Out-Null; Ctl @{cmd='advance'; ms=60} | Out-Null }

  $m = Ctl @{cmd='modules'}
  Check 'music is one of the pages, after the controls' (($m.modules | ForEach-Object { $_.id }) -join ',' -eq 'clock,deadlines,controls,music,system') (($m.modules | ForEach-Object { $_.id }) -join ',')
  Ctl @{cmd='mouse'; kind='move'; x=320; y=10} | Out-Null; Ctl @{cmd='advance'; ms=900} | Out-Null
  Ctl @{cmd='switch'; to='music'} | Out-Null; Ctl @{cmd='advance'; ms=400} | Out-Null
  Check 'with nothing playing the page says so' ((Mod).active -eq $false)

  'a song starts'
  Ctl @{cmd='media'; active=$true; title='Paper Sky'; artist='Lumen Coast'; playing=$true; position=60000; duration=216000} | Out-Null
  Ctl @{cmd='advance'; ms=300} | Out-Null
  $d = Mod
  Check 'the page shows it' ($d.active -and $d.title -eq 'Paper Sky' -and $d.artist -eq 'Lumen Coast') ($d | ConvertTo-Json -Compress)
  Check 'it is playing and moves with the clock' ($d.playing -and $d.position -ge 60000)
  Ctl @{cmd='advance'; ms=5000} | Out-Null
  Check 'five seconds later it is five seconds further' ((Mod).position -ge $d.position + 4900) "was $($d.position), now $((Mod).position)"

  'buttons'
  Click 'PlayPause'; Check 'the play button pauses' ((Ctl @{cmd='media_log'}).commands -contains 'PlayPause')
  Click 'Next'; Check 'next skips' ((Ctl @{cmd='media_log'}).commands -contains 'Next')
  Click 'Previous'; Check 'previous goes back' ((Ctl @{cmd='media_log'}).commands -contains 'Previous')
  $seek = (Mod).hits | Where-Object { $_.target -eq 'Seek' } | Select-Object -First 1
  $y = $seek.y + $seek.h / 2; $a = $seek.x + 4; $b = $seek.x + $seek.w - 4
  Ctl @{cmd='mouse'; kind='move'; x=($a + ($b - $a) * 0.5); y=$y} | Out-Null; Ctl @{cmd='mouse'; kind='down'; x=($a + ($b - $a) * 0.5); y=$y} | Out-Null; Ctl @{cmd='mouse'; kind='up'; x=($a + ($b - $a) * 0.5); y=$y} | Out-Null
  Check 'a click on the bar seeks to the middle' ((Ctl @{cmd='media_log'}).commands -contains 'Seek(108000)')

  'lyrics'
  Check 'nothing has been looked up yet' ((Mod).lyrics -eq 'none-asked')
  Click 'ToggleLyrics'
  $got = $false
  for ($i = 0; $i -lt 40 -and -not $got; $i++) { Start-Sleep -Milliseconds 50; Ctl @{cmd='advance'; ms=50} | Out-Null; $got = ((Mod).lyrics -eq 'synced') }
  Check 'turning lyrics on fetches them (the stand-in source)' $got ((Mod).lyrics)
  Ctl @{cmd='media'; position=21000} | Out-Null; Ctl @{cmd='advance'; ms=500} | Out-Null
  Check 'the line at 0:21 is the fifth (a line every four seconds)' ((Mod).scrollTo -eq 4) "$((Mod).scrollTo)"
  $line = (Mod).hits | Where-Object { $_.target -like 'Line(*' } | Select-Object -First 1
  Check 'lines can be clicked to jump there' ($null -ne $line)

  'looks'
  $dib = Ctl @{cmd='dibcheck'}
  Check 'the window shows exactly what was drawn' ($dib.differing -eq 0)
  Ctl @{cmd='dump'; path=(Join-Path (Resolve-Path (Join-Path $PSScriptRoot '..\out')).Path 'music-gate.png')} | Out-Null
  Ctl @{cmd='quit'} | Out-Null
} finally { Start-Sleep -Milliseconds 300; if (-not $p.HasExited) { $p.Kill() } }
"`n$script:pass passed, $script:fail failed"
if ($script:fail -gt 0) { exit 1 }
