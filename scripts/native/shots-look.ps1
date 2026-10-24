# Pictures of every look of the notch (pages, peeks, the pill, the eyes) on a manual clock, then one
# contact sheet, so the whole design can be judged at once. No cursor, no keyboard.
#   powershell -File scripts\native\shots-look.ps1 [-Exe path\to\blink.exe]
# Output: scripts\out\look-*.png and scripts\out\look-sheet.png
param([string]$Exe = '')
. "$PSScriptRoot\blink-ctl.ps1"
if ($Exe) { $env:BLINK_EXE = (Resolve-Path $Exe).Path }
$out = (Resolve-Path (Join-Path $PSScriptRoot '..\out')).Path
$data = Join-Path $env:TEMP 'blink-shots-look'; if (Test-Path $data) { Remove-Item $data -Recurse -Force }; New-Item -ItemType Directory -Force $data | Out-Null
$p = Start-Blink 'look' @{ BLINK_MANUAL_CLOCK = '1'; BLINK_DATA_DIR = $data; BLINK_TZ = 'Asia/Jakarta'; BLINK_DEMO = '1' }
try {
  $c = Connect-Blink 'look'
  function Ctl($o) { Send-Blink $c $o }
  function Shot($name) { Ctl @{cmd='dump'; path="$out\look-$name.png"} | Out-Null }
  function Peek($ev, $name) { $ev.cmd = 'system_event'; Ctl $ev | Out-Null; Ctl @{cmd='advance'; ms=700} | Out-Null; Shot $name; Ctl @{cmd='advance'; ms=6500} | Out-Null }

  # The pill and the peeks (nothing is open yet).
  Ctl @{cmd='advance'; ms=300} | Out-Null
  Shot 'rest-deadline'
  Peek @{ type='power'; onAc=$false; percent=70; charging=$false } 'peek-base'
  Peek @{ type='power'; onAc=$true; percent=70; charging=$true } 'peek-charger'
  Peek @{ type='wifi'; connected=$true; ssid='Campus' } 'peek-wifi'
  Peek @{ type='bluetooth'; name='AirBuds Pro'; connected=$true; battery=70; audio=$true } 'peek-bluetooth'
  Peek @{ type='volume'; percent=62; muted=$false } 'peek-volume'
  Peek @{ type='caps'; on=$true } 'peek-caps'
  Peek @{ type='power'; onAc=$false; percent=18; charging=$false } 'peek-battery-low'
  Ctl @{cmd='offer_peek'; code='CS101'; title='Problem Set 4'; count='1h'; priority=3} | Out-Null; Ctl @{cmd='advance'; ms=700} | Out-Null; Shot 'peek-deadline'; Ctl @{cmd='advance'; ms=6500} | Out-Null

  # The pages.
  Ctl @{cmd='mouse'; kind='move'; x=320; y=10} | Out-Null; Ctl @{cmd='advance'; ms=900} | Out-Null
  Shot 'page-deadlines'
  Ctl @{cmd='switch'; to='controls'} | Out-Null; Ctl @{cmd='advance'; ms=500} | Out-Null; Shot 'page-controls'
  Ctl @{cmd='switch'; to='music'} | Out-Null; Ctl @{cmd='advance'; ms=400} | Out-Null; Shot 'page-music-empty'
  Ctl @{cmd='media'; active=$true; title='Paper Sky'; artist='Lumen Coast'; playing=$true; position=75000; duration=216000} | Out-Null; Ctl @{cmd='advance'; ms=400} | Out-Null; Shot 'page-music'
  $h = (Ctl @{cmd='state'}).state.module.hits | Where-Object { $_.target -eq 'ToggleLyrics' } | Select-Object -First 1
  $x = $h.x + $h.w / 2; $y = $h.y + $h.h / 2
  Ctl @{cmd='mouse'; kind='move'; x=$x; y=$y} | Out-Null; Ctl @{cmd='mouse'; kind='down'; x=$x; y=$y} | Out-Null; Ctl @{cmd='mouse'; kind='up'; x=$x; y=$y} | Out-Null
  Start-Sleep -Milliseconds 600; Ctl @{cmd='advance'; ms=900} | Out-Null; Shot 'page-music-lyrics'
  Ctl @{cmd='media'; playing=$false} | Out-Null
  # Projects: empty, then a few tasks (typed with the keyboard commands), a date, statuses, files dropped.
  Ctl @{cmd='switch'; to='projects'} | Out-Null; Ctl @{cmd='advance'; ms=400} | Out-Null; Shot 'page-projects-empty'
  function Hit($pattern) { (Ctl @{cmd='state'}).state.module.hits | Where-Object { $_.target -like $pattern } | Select-Object -First 1 }
  function ClickAt($h) { $x = $h.x + $h.w / 2; $y = $h.y + $h.h / 2; Ctl @{cmd='mouse'; kind='move'; x=$x; y=$y} | Out-Null; Ctl @{cmd='mouse'; kind='down'; x=$x; y=$y} | Out-Null; Ctl @{cmd='mouse'; kind='up'; x=$x; y=$y} | Out-Null; Ctl @{cmd='advance'; ms=60} | Out-Null }
  function TypeLine($text) { foreach ($ch in $text.ToCharArray()) { if ($ch -eq ' ') { Ctl @{cmd='key'; key='space'} | Out-Null } else { Ctl @{cmd='key'; key='char'; ch=[string]$ch} | Out-Null } }; Ctl @{cmd='key'; key='enter'} | Out-Null; Ctl @{cmd='advance'; ms=60} | Out-Null }
  foreach ($t in 'Set up CI pipeline', 'Write API tests @fri', 'Agent prompt review @mon', 'Deploy staging @+10d') { ClickAt (Hit 'Add'); TypeLine $t }
  $ids = (Ctl @{cmd='state'}).state.module.tasks
  Ctl @{cmd='mouse'; kind='leave'} | Out-Null
  ClickAt (Hit 'Status(*') ; ClickAt (Hit 'Status(*')
  Ctl @{cmd='advance'; ms=300} | Out-Null; Shot 'page-projects'
  ClickAt (Hit 'Add'); foreach ($ch in 'Fix the flaky login te'.ToCharArray()) { if ($ch -eq ' ') { Ctl @{cmd='key'; key='space'} | Out-Null } else { Ctl @{cmd='key'; key='char'; ch=[string]$ch} | Out-Null } }
  Ctl @{cmd='advance'; ms=100} | Out-Null; Shot 'page-projects-typing'
  Ctl @{cmd='key'; key='escape'} | Out-Null
  $drop = Join-Path $env:TEMP 'blink-look-drop'; New-Item -ItemType Directory -Force (Join-Path $drop 'capstone-repo') | Out-Null; Set-Content (Join-Path $drop 'spec.pdf') 'x'; Set-Content (Join-Path $drop 'notes.md') 'x'
  Ctl @{cmd='drop'; paths=@((Join-Path $drop 'spec.pdf'), (Join-Path $drop 'capstone-repo'), (Join-Path $drop 'notes.md'))} | Out-Null; Ctl @{cmd='advance'; ms=400} | Out-Null; Shot 'page-projects-files'
  Ctl @{cmd='switch'; to='clock'} | Out-Null; Ctl @{cmd='advance'; ms=500} | Out-Null; Shot 'page-clock'
  Ctl @{cmd='mouse'; kind='leave'} | Out-Null; Ctl @{cmd='advance'; ms=1500} | Out-Null

  # The clock on the pill, then the eyes: open, mid-blink, dozing.
  Ctl @{cmd='settings'; patch=@{restingStyle='when-due'; dueHorizonHours=1}} | Out-Null; Ctl @{cmd='advance'; ms=800} | Out-Null
  Shot 'rest-clock'
  Ctl @{cmd='advance'; ms=46000} | Out-Null; Shot 'eyes-open'
  Ctl @{cmd='advance'; ms=3800} | Out-Null; Shot 'eyes-blink'
  Ctl @{cmd='advance'; ms=110000} | Out-Null; Shot 'eyes-doze'
  Ctl @{cmd='quit'} | Out-Null
} finally { Start-Sleep -Milliseconds 300; if (!$p.HasExited) { $p.Kill() } }

$py = @"
import glob, os
from PIL import Image
out = r'$out'
names = ['rest-deadline','rest-clock','eyes-open','eyes-blink','eyes-doze','peek-charger','peek-wifi','peek-bluetooth','peek-volume','peek-caps','peek-battery-low','peek-deadline','page-deadlines','page-controls','page-music','page-music-lyrics','page-music-empty','page-projects','page-projects-typing','page-projects-files','page-projects-empty','page-clock']
tiles = []
for n in names:
    f = os.path.join(out, 'look-' + n + '.png')
    if os.path.exists(f):
        im = Image.open(f).convert('RGBA')
        tall = n.startswith('page')
        tiles.append((n, im.crop((40, 0, 600, 200 if tall else 90))))
cols = 2
cw, ch = 560, 200
rows = (len(tiles) + cols - 1) // cols
sheet = Image.new('RGBA', (cols * (cw + 16) + 16, rows * (ch + 16) + 16), (40, 44, 60, 255))
for i, (n, im) in enumerate(tiles):
    x = 16 + (i % cols) * (cw + 16)
    y = 16 + (i // cols) * (ch + 16)
    bg = Image.new('RGBA', (cw, ch), (205, 212, 255, 255))
    bg.alpha_composite(im, (0, 0))
    sheet.paste(bg, (x, y))
sheet.convert('RGB').save(os.path.join(out, 'look-sheet.png'))
print('sheet', sheet.size)
"@
$tmp = Join-Path $env:TEMP 'blink-look-sheet.py'
Set-Content -Path $tmp -Value $py -Encoding UTF8
python $tmp
