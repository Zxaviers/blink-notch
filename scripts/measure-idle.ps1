# Spike D: samples CPU and memory of the notch process tree (exe plus its
# WebView2 children) while it rests. Leave the cursor away from the top edge.
#   .\scripts\measure-idle.ps1 -Minutes 30
param(
  [string]$ProcessName = "deadline-notch",
  [int]$Minutes = 30,
  [int]$IntervalSec = 10
)

$root = Get-Process -Name $ProcessName -ErrorAction SilentlyContinue | Select-Object -First 1
if (-not $root) { "Process '$ProcessName' is not running. Start it first."; exit 1 }

function Get-Tree([int]$rootPid) {
  $all = Get-CimInstance Win32_Process | Select-Object ProcessId, ParentProcessId
  $ids = [System.Collections.Generic.HashSet[int]]::new()
  [void]$ids.Add($rootPid)
  do {
    $added = 0
    foreach ($p in $all) {
      if ($ids.Contains([int]$p.ParentProcessId) -and $ids.Add([int]$p.ProcessId)) { $added++ }
    }
  } while ($added -gt 0)
  $ids
}

$cores = [Environment]::ProcessorCount
$out = Join-Path $PSScriptRoot "out"
New-Item -ItemType Directory -Force $out | Out-Null
$csv = Join-Path $out ("idle-{0:yyyyMMdd-HHmmss}.csv" -f (Get-Date))
"t_sec,cpu_pct,working_set_mb,private_mb,private_ws_mb,procs" | Set-Content $csv

$prevCpu = $null; $prevT = $null
$samples = @()
$end = (Get-Date).AddMinutes($Minutes)
$start = Get-Date
while ((Get-Date) -lt $end) {
  $ids = Get-Tree $root.Id
  $procs = Get-Process -Id @($ids) -ErrorAction SilentlyContinue
  $cpuSec = ($procs | Measure-Object -Property CPU -Sum).Sum
  $ws = ($procs | Measure-Object -Property WorkingSet64 -Sum).Sum / 1MB
  $pm = ($procs | Measure-Object -Property PrivateMemorySize64 -Sum).Sum / 1MB
  # "Working set - private" is what Task Manager's Memory column shows.
  $pws = (Get-CimInstance Win32_PerfFormattedData_PerfProc_Process | Where-Object { $ids.Contains([int]$_.IDProcess) } | Measure-Object -Property WorkingSetPrivate -Sum).Sum / 1MB
  $now = Get-Date
  if ($null -ne $prevCpu) {
    $cpuPct = 100 * ($cpuSec - $prevCpu) / (($now - $prevT).TotalSeconds * $cores)
    $row = "{0:N0},{1:N2},{2:N1},{3:N1},{4:N1},{5}" -f ($now - $start).TotalSeconds, $cpuPct, $ws, $pm, $pws, $procs.Count
    Add-Content $csv $row
    $samples += [pscustomobject]@{ Cpu = $cpuPct; Ws = $ws; Pm = $pm; Pws = $pws }
    $row
  }
  $prevCpu = $cpuSec; $prevT = $now
  Start-Sleep -Seconds $IntervalSec
}

if ($samples.Count -gt 0) {
  "--- summary ({0} samples, {1} logical cores) ---" -f $samples.Count, $cores
  "CPU avg {0:N2}%  max {1:N2}%   (PRD target: avg < 0.5%)" -f ($samples | Measure-Object Cpu -Average).Average, ($samples | Measure-Object Cpu -Maximum).Maximum
  "Private working set (Task Manager Memory) avg {0:N1} MB  max {1:N1} MB   (PRD target: < 80 MB)" -f ($samples | Measure-Object Pws -Average).Average, ($samples | Measure-Object Pws -Maximum).Maximum
  "Working set (includes shared pages) avg {0:N1} MB" -f ($samples | Measure-Object Ws -Average).Average
  "Private avg {0:N1} MB" -f ($samples | Measure-Object Pm -Average).Average
  "CSV: $csv"
}
