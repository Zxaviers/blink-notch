# The bars against the real speakers: plays a very quiet tone (a generated WAV, about 2.5 seconds, a few percent of full scale) and
# checks that the loopback listener hears it in the right band. Needs no mouse or keyboard. Loopback hears the digital stream before
# the volume control, so the tone is almost inaudible even though it is clearly there.
#   powershell -File scripts\native\check-spectrum-real.ps1 [-Exe path\to\blink.exe]
param([string]$Exe = '', [double]$Hz = 1000, [double]$Amplitude = 0.03)
. "$PSScriptRoot\blink-ctl.ps1"
if ($Exe) { $env:BLINK_EXE = (Resolve-Path $Exe).Path }
$rate = 44100; $seconds = 2.6; $n = [int]($rate * $seconds)
$wav = Join-Path $env:TEMP 'blink-quiet-tone.wav'
$ms = New-Object System.IO.MemoryStream; $w = New-Object System.IO.BinaryWriter($ms)
$w.Write([Text.Encoding]::ASCII.GetBytes('RIFF')); $w.Write([int](36 + $n * 2)); $w.Write([Text.Encoding]::ASCII.GetBytes('WAVEfmt '))
$w.Write([int]16); $w.Write([int16]1); $w.Write([int16]1); $w.Write([int]$rate); $w.Write([int]($rate * 2)); $w.Write([int16]2); $w.Write([int16]16)
$w.Write([Text.Encoding]::ASCII.GetBytes('data')); $w.Write([int]($n * 2))
for ($i = 0; $i -lt $n; $i++) { $w.Write([int16](32767 * $Amplitude * [Math]::Sin(2 * [Math]::PI * $Hz * $i / $rate))) }
[IO.File]::WriteAllBytes($wav, $ms.ToArray())
$data = Join-Path $env:TEMP 'blink-spectrum-data'; if (Test-Path $data) { Remove-Item $data -Recurse -Force }; New-Item -ItemType Directory -Force $data | Out-Null
$fake = Start-Process powershell -ArgumentList '-STA', '-NoProfile', '-File', "$PSScriptRoot\fake-player.ps1", '-Seconds', '60' -PassThru -WindowStyle Hidden
Start-Sleep -Seconds 4
$p = Start-Blink 'spectrumreal' @{ BLINK_REAL_SOURCES = '1'; BLINK_DATA_DIR = $data; BLINK_TZ = 'Asia/Jakarta'; BLINK_DEMO = '1' }
try {
  $c = Connect-Blink 'spectrumreal'
  function State() { (Send-Blink $c @{cmd='state'}).state }
  Start-Sleep -Milliseconds 1500
  $s = State
  # What the listener costs while the bars show.
  $pr0 = (Get-Process -Id $p.Id).TotalProcessorTime.TotalMilliseconds; $tm0 = Get-Date
  Start-Sleep 10
  $pr1 = (Get-Process -Id $p.Id).TotalProcessorTime.TotalMilliseconds; $tm1 = Get-Date
  "10 s with the loopback listener running (bars showing): {0:N0} ms CPU = {1:N2}% of one core" -f ($pr1 - $pr0), (($pr1 - $pr0) / (($tm1 - $tm0).TotalMilliseconds) * 100)
  $s = State
  "before the tone: wanted=$($s.spectrum.wanted) working=$($s.spectrum.working) why=[$($s.spectrum.why)] levels=$(($s.spectrum.levels | ForEach-Object { [math]::Round($_, 2) }) -join ' ')"
  $player = New-Object System.Media.SoundPlayer $wav
  $player.Play()
  $best = $null
  for ($i = 0; $i -lt 20; $i++) {
    Start-Sleep -Milliseconds 100
    $s = State
    if ($s.spectrum.working) { $l = @($s.spectrum.levels); $peak = ($l | Measure-Object -Maximum).Maximum; if ($null -eq $best -or $peak -gt $best.peak) { $best = @{ peak = $peak; levels = $l } } }
  }
  if ($null -eq $best) { 'the capture never started' } else {
    $at = -1; for ($k = 0; $k -lt $best.levels.Count; $k++) { if ([double]$best.levels[$k] -eq [double]$best.peak) { $at = $k } }
    "during the tone: peak level $([math]::Round($best.peak, 2)) in band $at (a $Hz Hz tone belongs in band 4): levels $(($best.levels | ForEach-Object { [math]::Round($_, 2) }) -join ' ')"
  }
  "heard (packets, silent, loudest sample x1000): $(@($s.spectrum.heard) -join ', ')"
  $player.Stop()
  Start-Sleep -Milliseconds 1500
  $s = State
  "after: levels=$(($s.spectrum.levels | ForEach-Object { [math]::Round($_, 2) }) -join ' ')"
  Send-Blink $c @{cmd='quit'} | Out-Null
} finally { Start-Sleep -Milliseconds 300; if (!$p.HasExited) { $p.Kill() }; if (!$fake.HasExited) { $fake.Kill() } }
