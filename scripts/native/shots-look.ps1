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
names = ['rest-deadline','rest-clock','eyes-open','eyes-blink','eyes-doze','peek-charger','peek-wifi','peek-bluetooth','peek-volume','peek-caps','peek-battery-low','peek-deadline','page-deadlines','page-controls','page-clock']
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
