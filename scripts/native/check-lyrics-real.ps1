# Real Windows media session + the real lyrics service: shows whatever is playing (a pretend player if nothing
# else is) on the Music page, asks for lyrics and prints what came back. No cursor, no keyboard.
#   powershell -File scripts\native\check-lyrics-real.ps1 [-Exe path\to\blink.exe]
param([string]$Exe = '', [string]$Title = 'Magnolia', [string]$Artist = 'magnolia celebration', [string]$Album = 'Fantasia')
. "$PSScriptRoot\blink-ctl.ps1"
if ($Exe) { $env:BLINK_EXE = (Resolve-Path $Exe).Path }
$data = Join-Path $env:TEMP 'blink-lyrics-real'; if (Test-Path $data) { Remove-Item $data -Recurse -Force }; New-Item -ItemType Directory -Force $data | Out-Null
$fake = Start-Process powershell -ArgumentList '-STA', '-NoProfile', '-File', "$PSScriptRoot\fake-player.ps1", '-Title', $Title, '-Artist', $Artist, '-Album', $Album, '-Seconds', '45', '-LengthSeconds', '256' -PassThru -WindowStyle Hidden
Start-Sleep -Seconds 4
$p = Start-Blink 'lyricsreal' @{ BLINK_REAL_SOURCES = '1'; BLINK_DATA_DIR = $data; BLINK_TZ = 'Asia/Jakarta'; BLINK_DEMO = '1'; BLINK_MEDIA_DEBUG = '1' }
try {
  $c = Connect-Blink 'lyricsreal'
  function Ctl($o) { Send-Blink $c $o }
  Ctl @{cmd='mouse'; kind='move'; x=320; y=10} | Out-Null; Ctl @{cmd='advance'; ms=900} | Out-Null
  Ctl @{cmd='switch'; to='music'} | Out-Null; Start-Sleep -Milliseconds 800; Ctl @{cmd='advance'; ms=300} | Out-Null
  $st = (Ctl @{cmd='state'}).state.module
  "playing: $($st.title) / $($st.artist)  lyrics-state: $($st.lyrics)"
  $h = $st.hits | Where-Object { $_.target -eq 'ToggleLyrics' } | Select-Object -First 1
  if ($h) {
    $x = $h.x + $h.w / 2; $y = $h.y + $h.h / 2
    Ctl @{cmd='mouse'; kind='move'; x=$x; y=$y} | Out-Null; Ctl @{cmd='mouse'; kind='down'; x=$x; y=$y} | Out-Null; Ctl @{cmd='mouse'; kind='up'; x=$x; y=$y} | Out-Null
    for ($i = 0; $i -lt 40; $i++) { Start-Sleep -Milliseconds 500; Ctl @{cmd='advance'; ms=100} | Out-Null; $l = (Ctl @{cmd='state'}).state.module.lyrics; if ($l -notin @('loading', 'none-asked')) { break } }
    "lyrics-state after asking: $l"
  } else { 'no lyrics button on the page' }
  Ctl @{cmd='quit'} | Out-Null
} finally { Start-Sleep -Milliseconds 300; if (!$p.HasExited) { $p.Kill() }; if (!$fake.HasExited) { $fake.Kill() } }
