# Idle-cost run against the release exe with a realistic profile: one fixture
# feed (so parsing, timers and the notification planner are live). Seeds and
# restores %APPDATA%\dev.riski.deadlinenotch, then samples CPU and memory.
#   .\scripts\measure-full.ps1 -Minutes 5
param(
  [string]$Exe = "$PSScriptRoot\..\src-tauri\target\release\deadline-notch.exe",
  [int]$Minutes = 5,
  [int]$Port = 8766
)

$ErrorActionPreference = "Stop"
$out = Join-Path $PSScriptRoot "out"
New-Item -ItemType Directory -Force $out | Out-Null

$stale = Get-Process -Name "deadline-notch" -ErrorAction SilentlyContinue
if ($stale) { "ABORT  another deadline-notch is running (pid $($stale.Id -join ', '))"; exit 2 }

$dataDir = Join-Path $env:APPDATA "dev.riski.deadlinenotch"
$backup = $null
if (Test-Path $dataDir) { $backup = "$dataDir.measure-backup"; if (Test-Path $backup) { Remove-Item $backup -Recurse -Force }; Move-Item $dataDir $backup }
New-Item -ItemType Directory -Force $dataDir | Out-Null

$feedId = "ics-measure"
$settings = @{ version = 1; onboarded = $true; toast = $false
  feeds = @(@{ id = $feedId; kind = "ics"; label = "Measure"; enabled = $true }) } | ConvertTo-Json -Depth 5
[System.IO.File]::WriteAllText((Join-Path $dataDir "settings.json"), $settings, (New-Object System.Text.UTF8Encoding($false)))
cmdkey /generic:"$feedId.deadline-notch" /user:$feedId /pass:"http://127.0.0.1:$Port/cal.ics" | Out-Null

$server = Start-Process node -ArgumentList "`"$PSScriptRoot\serve-fixture.mjs`" $Port" -PassThru -WindowStyle Hidden -RedirectStandardOutput (Join-Path $out "measure-server.log")
$proc = $null
try {
  Start-Sleep -Seconds 1
  $proc = Start-Process -FilePath (Resolve-Path $Exe) -PassThru
  Start-Sleep -Seconds 25   # let startup settle before sampling
  & "$PSScriptRoot\measure-idle.ps1" -Minutes $Minutes -IntervalSec 10
} finally {
  if ($proc -and -not $proc.HasExited) { Stop-Process -Id $proc.Id -Force }
  if ($server -and -not $server.HasExited) { Stop-Process -Id $server.Id -Force }
  cmdkey /delete:"$feedId.deadline-notch" | Out-Null
  Start-Sleep -Milliseconds 500
  Remove-Item $dataDir -Recurse -Force -ErrorAction SilentlyContinue
  if ($backup) { Move-Item $backup $dataDir }
}
