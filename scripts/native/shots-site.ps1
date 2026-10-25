# The pictures for the website: the pill, the peeks, every page and a few palettes, drawn by the real program (demo data, a manual
# clock, at twice the size) and trimmed to the notch. No cursor, no keyboard.
#   powershell -File scripts\native\shots-site.ps1 [-Exe path\to\blink.exe]
# Output: site\img\*.png (transparent, so the page puts them on its own background)
param([string]$Exe = '')
. "$PSScriptRoot\blink-ctl.ps1"
if ($Exe) { $env:BLINK_EXE = (Resolve-Path $Exe).Path }
$root = (Resolve-Path (Join-Path $PSScriptRoot '..\..')).Path
$img = Join-Path $root 'site\img'; New-Item -ItemType Directory -Force $img | Out-Null
$raw = Join-Path $env:TEMP 'blink-site-raw'; if (Test-Path $raw) { Remove-Item $raw -Recurse -Force }; New-Item -ItemType Directory -Force $raw | Out-Null
$data = Join-Path $env:TEMP 'blink-site-data'; if (Test-Path $data) { Remove-Item $data -Recurse -Force }; New-Item -ItemType Directory -Force $data | Out-Null
$files = Join-Path $env:TEMP 'blink-site-files\Kuliah'; if (Test-Path $files) { Remove-Item $files -Recurse -Force }; New-Item -ItemType Directory -Force $files | Out-Null
$names = 'Laporan Praktikum 7.pdf', 'Slides Sistem Operasi.pptx', 'Screenshot 2026-10-22.png', 'Catatan Kuliah.md'
foreach ($n in $names) { Set-Content (Join-Path $files $n) 'x' }
$p = Start-Blink 'site' @{ BLINK_MANUAL_CLOCK = '1'; BLINK_DATA_DIR = $data; BLINK_TZ = 'Asia/Jakarta'; BLINK_DEMO = '1'; BLINK_SCALE = '2' }
try {
  $c = Connect-Blink 'site'
  function Ctl($o) { Send-Blink $c $o }
  function Shot($name) { Ctl @{cmd='dump'; path="$raw\$name.png"} | Out-Null }
  function Peek($ev, $name) { $ev.cmd = 'system_event'; Ctl $ev | Out-Null; Ctl @{cmd='advance'; ms=700} | Out-Null; Shot $name; Ctl @{cmd='advance'; ms=6500} | Out-Null }
  function Hit($pattern) { (Ctl @{cmd='state'}).state.module.hits | Where-Object { $_.target -like $pattern } | Select-Object -First 1 }
  function ClickAt($h) { $x = $h.x + $h.w / 2; $y = $h.y + $h.h / 2; Ctl @{cmd='mouse'; kind='move'; x=$x; y=$y} | Out-Null; Ctl @{cmd='mouse'; kind='down'; x=$x; y=$y} | Out-Null; Ctl @{cmd='mouse'; kind='up'; x=$x; y=$y} | Out-Null; Ctl @{cmd='advance'; ms=60} | Out-Null }
  function TypeLine($text) { foreach ($ch in $text.ToCharArray()) { if ($ch -eq ' ') { Ctl @{cmd='key'; key='space'} | Out-Null } else { Ctl @{cmd='key'; key='char'; ch=[string]$ch} | Out-Null } }; Ctl @{cmd='key'; key='enter'} | Out-Null; Ctl @{cmd='advance'; ms=60} | Out-Null }
  function Open() { Ctl @{cmd='mouse'; kind='move'; x=320; y=10} | Out-Null; Ctl @{cmd='advance'; ms=900} | Out-Null }

  # The pill and the peeks.
  Ctl @{cmd='advance'; ms=300} | Out-Null
  Shot 'rest-clock'
  Peek @{ type='power'; onAc=$true; percent=70; charging=$true } 'peek-charger'
  Peek @{ type='bluetooth'; name='AirBuds Pro'; connected=$true; battery=70; audio=$true } 'peek-bluetooth'
  Peek @{ type='volume'; percent=62; muted=$false } 'peek-volume'
  Ctl @{cmd='offer_peek'; code='CS101'; title='Problem Set 4'; count='1h'; priority=3} | Out-Null; Ctl @{cmd='advance'; ms=700} | Out-Null; Shot 'peek-deadline'; Ctl @{cmd='advance'; ms=6500} | Out-Null

  # The pages.
  Open
  Ctl @{cmd='switch'; to='deadlines'} | Out-Null; Ctl @{cmd='advance'; ms=400} | Out-Null; Shot 'page-deadlines'
  Ctl @{cmd='switch'; to='controls'} | Out-Null; Ctl @{cmd='advance'; ms=500} | Out-Null; Shot 'page-controls'
  Ctl @{cmd='switch'; to='music'} | Out-Null; Ctl @{cmd='advance'; ms=400} | Out-Null
  Ctl @{cmd='media'; active=$true; title='Paper Sky'; artist='Lumen Coast'; playing=$true; position=75000; duration=216000} | Out-Null; Ctl @{cmd='advance'; ms=400} | Out-Null
  $h = Hit 'ToggleLyrics'
  ClickAt $h
  Start-Sleep -Milliseconds 600; Ctl @{cmd='advance'; ms=900} | Out-Null; Shot 'page-music'
  Ctl @{cmd='media'; playing=$false} | Out-Null
  Ctl @{cmd='switch'; to='projects'} | Out-Null; Ctl @{cmd='advance'; ms=400} | Out-Null
  foreach ($t in 'Set up CI pipeline', 'Write API tests @fri', 'Revise for the OS quiz @mon', 'Submit the lab report @+3d') { ClickAt (Hit 'Add'); TypeLine $t }
  Ctl @{cmd='mouse'; kind='leave'} | Out-Null
  ClickAt (Hit 'Status(*'); ClickAt (Hit 'Status(*')
  Ctl @{cmd='advance'; ms=300} | Out-Null; Shot 'page-projects'
  # The shelf: files dropped on its half of the notch.
  Ctl @{cmd='mouse'; kind='leave'} | Out-Null; Ctl @{cmd='advance'; ms=1500} | Out-Null
  $paths = $names | ForEach-Object { Join-Path $files $_ }
  Ctl @{cmd='drag'; on=$true; x=250} | Out-Null; Ctl @{cmd='advance'; ms=600} | Out-Null; Shot 'page-dropzones'
  Ctl @{cmd='drop_at'; x=250; paths=@($paths)} | Out-Null; Ctl @{cmd='advance'; ms=700} | Out-Null; Shot 'peek-shelf'
  Ctl @{cmd='mouse'; kind='leave'} | Out-Null; Ctl @{cmd='advance'; ms=4500} | Out-Null
  Open
  Ctl @{cmd='switch'; to='shelf'} | Out-Null; Ctl @{cmd='advance'; ms=500} | Out-Null; Shot 'page-shelf'
  Ctl @{cmd='switch'; to='clock'} | Out-Null; Ctl @{cmd='advance'; ms=500} | Out-Null; Shot 'page-clock'
  Ctl @{cmd='mouse'; kind='leave'} | Out-Null; Ctl @{cmd='advance'; ms=1500} | Out-Null

  # A few palettes, on the deadlines page.
  foreach ($pal in 'aurora', 'sunset', 'forest', 'ocean') {
    $modules = 'clock', 'deadlines', 'controls', 'music', 'projects', 'shelf', 'system' | ForEach-Object { @{ id = $_; enabled = $true } }
    Ctl @{cmd='host'; config=@{ look=@{ palette=$pal }; modules=@($modules) }} | Out-Null
    Open
    Ctl @{cmd='switch'; to='deadlines'} | Out-Null; Ctl @{cmd='advance'; ms=500} | Out-Null; Shot "palette-$pal"
    Ctl @{cmd='mouse'; kind='leave'} | Out-Null; Ctl @{cmd='advance'; ms=1500} | Out-Null
  }
  Ctl @{cmd='quit'} | Out-Null
} finally { Start-Sleep -Milliseconds 300; if (!$p.HasExited) { $p.Kill() } }

# Trim each picture to what is drawn (the window around it is empty), with a little room, and put it in site\img.
$py = @"
import glob, os
from PIL import Image
raw, out = r'$raw', r'$img'
for f in sorted(glob.glob(os.path.join(raw, '*.png'))):
    im = Image.open(f).convert('RGBA')
    box = im.getchannel('A').point(lambda a: 255 if a > 8 else 0).getbbox()
    if not box:
        print('empty', os.path.basename(f)); continue
    pad = 6
    box = (max(box[0] - pad, 0), max(box[1] - pad, 0), min(box[2] + pad, im.width), min(box[3] + pad, im.height))
    im.crop(box).save(os.path.join(out, os.path.basename(f)), optimize=True)
    print(os.path.basename(f), im.crop(box).size)
"@
$tmp = Join-Path $env:TEMP 'blink-site-trim.py'
Set-Content -Path $tmp -Value $py -Encoding UTF8
python $tmp
