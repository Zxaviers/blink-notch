# Watches the top-centre of the screen and saves every frame that changed (to find drawing glitches).
param([int]$Seconds = 60, [int]$X = 640, [int]$W = 640, [int]$H = 360, [int]$Max = 120)
Add-Type -TypeDefinition 'using System.Runtime.InteropServices; public static class D3 { [DllImport("user32.dll")] public static extern bool SetProcessDPIAware(); }'
[void][D3]::SetProcessDPIAware(); Add-Type -AssemblyName System.Drawing
$out = Join-Path $PSScriptRoot '..\out\anim'; New-Item -ItemType Directory -Force $out | Out-Null
$bmp = New-Object System.Drawing.Bitmap($W, $H); $g = [System.Drawing.Graphics]::FromImage($bmp)
$prev = $null; $n = 0; $sw = [System.Diagnostics.Stopwatch]::StartNew()
while ($sw.Elapsed.TotalSeconds -lt $Seconds -and $n -lt $Max) {
  $g.CopyFromScreen($X, 0, 0, 0, (New-Object System.Drawing.Size($W, $H)))
  # a cheap signature of the notch area: pixels down the middle column and two side columns
  $sig = New-Object System.Text.StringBuilder
  foreach ($x in 120, 320, 520) { for ($y = 0; $y -lt $H; $y += 6) { [void]$sig.Append($bmp.GetPixel($x, $y).ToArgb().ToString('x')) } }
  $s = $sig.ToString()
  if ($prev -ne $null -and $s -ne $prev) { $bmp.Save((Join-Path $out ('f{0:D3}-{1}.png' -f $n, [int]$sw.ElapsedMilliseconds))); $n++ }
  $prev = $s
}
"saved $n changed frames"
