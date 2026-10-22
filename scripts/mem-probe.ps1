# Memory breakdown of the resting app by WebView2 process type, for comparing
# WebView2 flags without rebuilding: extra flags go in through the
# WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS environment variable.
#   .\scripts\mem-probe.ps1 -Flags "--in-process-gpu" -Label gpu-in-proc
# Seeds a throwaway profile with one fixture feed and restores your data after.
param(
  [string]$Exe = "$PSScriptRoot\..\src-tauri\target\release\deadline-notch.exe",
  [string]$Flags = "",
  [string]$Label = "baseline",
  [int]$SettleSeconds = 25,
  [int]$Samples = 4,
  [int]$Port = 8767
)

$ErrorActionPreference = "Stop"
$out = Join-Path $PSScriptRoot "out"
New-Item -ItemType Directory -Force $out | Out-Null

$stale = Get-Process -Name "deadline-notch" -ErrorAction SilentlyContinue
if ($stale) { "ABORT  another deadline-notch is running (pid $($stale.Id -join ', '))"; exit 2 }

$dataDir = Join-Path $env:APPDATA "dev.riski.deadlinenotch"
$backup = $null
if (Test-Path $dataDir) { $backup = "$dataDir.mem-backup"; if (Test-Path $backup) { Remove-Item $backup -Recurse -Force }; Move-Item $dataDir $backup }
New-Item -ItemType Directory -Force $dataDir | Out-Null

$feedId = "ics-mem"
$settings = @{ version = 1; onboarded = $true; toast = $false
  feeds = @(@{ id = $feedId; kind = "ics"; label = "Mem"; enabled = $true }) } | ConvertTo-Json -Depth 5
[System.IO.File]::WriteAllText((Join-Path $dataDir "settings.json"), $settings, (New-Object System.Text.UTF8Encoding($false)))
cmdkey /generic:"$feedId.deadline-notch" /user:$feedId /pass:"http://127.0.0.1:$Port/cal.ics" | Out-Null

$server = Start-Process node -ArgumentList "`"$PSScriptRoot\serve-fixture.mjs`" $Port" -PassThru -WindowStyle Hidden -RedirectStandardOutput (Join-Path $out "mem-server.log")
$proc = $null
try {
  Start-Sleep -Seconds 1
  if ($Flags) { $env:WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS = $Flags } else { Remove-Item Env:WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS -ErrorAction SilentlyContinue }
  $proc = Start-Process -FilePath (Resolve-Path $Exe) -PassThru
  Start-Sleep -Seconds $SettleSeconds

  $totals = @()
  $last = $null
  for ($i = 0; $i -lt $Samples; $i++) {
    $all = Get-CimInstance Win32_Process | Select-Object ProcessId, ParentProcessId, Name, CommandLine
    $ids = [System.Collections.Generic.HashSet[int]]::new(); [void]$ids.Add($proc.Id)
    do { $n = 0; foreach ($p in $all) { if ($ids.Contains([int]$p.ParentProcessId) -and $ids.Add([int]$p.ProcessId)) { $n++ } } } while ($n -gt 0)
    $perf = Get-CimInstance Win32_PerfFormattedData_PerfProc_Process | Where-Object { $ids.Contains([int]$_.IDProcess) }
    $rows = foreach ($p in $all | Where-Object { $ids.Contains([int]$_.ProcessId) }) {
      $type = if ($p.Name -eq "deadline-notch.exe") { "app" } elseif ($p.CommandLine -match '--type=([\w-]+)') { $Matches[1] } else { "webview2-browser" }
      if ($type -eq "utility" -and $p.CommandLine -match '--utility-sub-type=([\w\.]+)') { $type = "utility:" + ($Matches[1] -replace 'network.mojom.|storage.mojom.|\.mojom','') }
      $w = ($perf | Where-Object { $_.IDProcess -eq $p.ProcessId }).WorkingSetPrivate
      [pscustomobject]@{ Type = $type; PID = $p.ProcessId; PrivateWS_MB = [math]::Round($w / 1MB, 1) }
    }
    $totals += ($rows | Measure-Object PrivateWS_MB -Sum).Sum
    $last = $rows
    Start-Sleep -Seconds 5
  }
  "== $Label  (flags: '$Flags') =="
  $last | Sort-Object PrivateWS_MB -Descending | Format-Table -AutoSize | Out-String -Width 120
  "processes: $($last.Count)   total private WS (last sample): $([math]::Round(($last | Measure-Object PrivateWS_MB -Sum).Sum, 1)) MB   avg over $Samples samples: $([math]::Round(($totals | Measure-Object -Average).Average, 1)) MB"
} finally {
  Remove-Item Env:WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS -ErrorAction SilentlyContinue
  if ($proc -and -not $proc.HasExited) { Stop-Process -Id $proc.Id -Force }
  if ($server -and -not $server.HasExited) { Stop-Process -Id $server.Id -Force }
  cmdkey /delete:"$feedId.deadline-notch" | Out-Null
  Start-Sleep -Milliseconds 800
  Remove-Item $dataDir -Recurse -Force -ErrorAction SilentlyContinue
  if ($backup) { Move-Item $backup $dataDir }
}
