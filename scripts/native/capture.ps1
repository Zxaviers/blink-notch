# Screenshots of the real window as Windows composites it (not just our own pixel buffer),
# plus the renderer's own dumps. No cursor or keyboard involved: the app is driven through its pipe.
#   powershell -File scripts\native\capture.ps1 [-Exe ...]
param([string]$Exe = "$PSScriptRoot\..\..\target\release\blink.exe")
$ErrorActionPreference = 'Stop'
. "$PSScriptRoot\blink-ctl.ps1"
$env:BLINK_EXE = (Resolve-Path $Exe).Path
$out = Join-Path $PSScriptRoot '..\out'; New-Item -ItemType Directory -Force $out | Out-Null; $out = (Resolve-Path $out).Path
$data = Join-Path $env:TEMP 'blink-capture-data'; New-Item -ItemType Directory -Force $data | Out-Null

Add-Type -TypeDefinition 'using System.Runtime.InteropServices; public static class Dpi { [DllImport("user32.dll")] public static extern bool SetProcessDPIAware(); }'
[void][Dpi]::SetProcessDPIAware()
Add-Type -AssemblyName System.Drawing

function Shot($state, [string]$name) {
  $r = $state.window.rect
  $bmp = New-Object System.Drawing.Bitmap($r.w, $r.h)
  $g = [System.Drawing.Graphics]::FromImage($bmp)
  $g.CopyFromScreen($r.x, $r.y, 0, 0, (New-Object System.Drawing.Size($r.w, $r.h)))
  $g.Dispose(); $bmp.Save((Join-Path $out $name), [System.Drawing.Imaging.ImageFormat]::Png); $bmp.Dispose()
}

$p = Start-Blink 'cap' @{ BLINK_MANUAL_CLOCK = '1'; BLINK_DATA_DIR = $data; BLINK_TZ = 'Asia/Jakarta'; BLINK_DEMO = '1' }
try {
  $c = Connect-Blink 'cap'
  Start-Sleep -Milliseconds 400
  $s = Send-Blink $c @{cmd='state'}
  "monitor scale: $($s.state.scale)  window: $($s.window.rect.x),$($s.window.rect.y) $($s.window.rect.w)x$($s.window.rect.h)"
  Shot $s 'screen-rest.png'

  Send-Blink $c @{cmd='peek'} | Out-Null
  Send-Blink $c @{cmd='advance'; ms=500} | Out-Null
  Start-Sleep -Milliseconds 200
  Shot $s 'screen-peek.png'
  Send-Blink $c @{cmd='advance'; ms=6000} | Out-Null

  Send-Blink $c @{cmd='mouse'; kind='move'; x=320; y=10} | Out-Null
  Send-Blink $c @{cmd='advance'; ms=800} | Out-Null
  Start-Sleep -Milliseconds 200
  Shot $s 'screen-open-dark.png'

  Send-Blink $c @{cmd='settings'; patch=@{theme='light'}} | Out-Null
  Send-Blink $c @{cmd='advance'; ms=100} | Out-Null
  Start-Sleep -Milliseconds 200
  Shot $s 'screen-open-light.png'

  # The renderer's own frames at other display scales, for comparing glyph quality.
  Send-Blink $c @{cmd='settings'; patch=@{theme='dark'}} | Out-Null
  foreach ($scale in 1.0, 1.5, 2.0) {
    Send-Blink $c @{cmd='scale'; value=$scale} | Out-Null
    Send-Blink $c @{cmd='mouse'; kind='move'; x=320; y=10} | Out-Null
    Send-Blink $c @{cmd='advance'; ms=800} | Out-Null
    Send-Blink $c @{cmd='dump'; path=(Join-Path $out "dump-open-x$scale.png")} | Out-Null
  }
  Send-Blink $c @{cmd='quit'} | Out-Null
} finally { Start-Sleep -Milliseconds 300; if (-not $p.HasExited) { $p.Kill() } }
Get-ChildItem "$out\*" -Include 'screen-*.png','dump-open-x*.png' | Select-Object Name, Length
