# Cost of the settings program while it is open: time to a usable window, memory of its whole
# process tree, and that everything is gone after it closes. Shows a window for ~10 seconds.
param([string]$Exe = "$PSScriptRoot\..\..\target\release\blink-settings.exe")
$ErrorActionPreference = 'Stop'
$env:BLINK_DATA_DIR = Join-Path $env:TEMP 'blink-measure-settings'; New-Item -ItemType Directory -Force $env:BLINK_DATA_DIR | Out-Null
$sw = [System.Diagnostics.Stopwatch]::StartNew()
$p = Start-Process (Resolve-Path $Exe) -PassThru
while (-not $p.HasExited -and $sw.ElapsedMilliseconds -lt 20000) {
  $p.Refresh(); if ($p.MainWindowHandle -ne [IntPtr]::Zero) { break }; Start-Sleep -Milliseconds 20
}
"window up after $($sw.ElapsedMilliseconds) ms"
Start-Sleep -Seconds 6
$all = Get-CimInstance Win32_Process | Select-Object ProcessId, ParentProcessId, Name
$ids = [System.Collections.Generic.HashSet[int]]::new(); [void]$ids.Add($p.Id)
do { $n = 0; foreach ($q in $all) { if ($ids.Contains([int]$q.ParentProcessId) -and $ids.Add([int]$q.ProcessId)) { $n++ } } } while ($n -gt 0)
$perf = Get-CimInstance Win32_PerfFormattedData_PerfProc_Process | Where-Object { $ids.Contains([int]$_.IDProcess) }
$total = ($perf | Measure-Object WorkingSetPrivate -Sum).Sum / 1MB
"open: $($ids.Count) processes, private memory $([math]::Round($total, 1)) MB"
$p.Refresh(); [void]$p.CloseMainWindow()
$sw.Restart(); while ($sw.ElapsedMilliseconds -lt 15000 -and @(Get-Process -Id ($ids | ForEach-Object { $_ }) -ErrorAction SilentlyContinue | Where-Object { -not $_.HasExited }).Count -gt 0) { Start-Sleep -Milliseconds 100 }
"all gone after $($sw.ElapsedMilliseconds) ms"
$left = @(Get-Process -Id ($ids | ForEach-Object { $_ }) -ErrorAction SilentlyContinue | Where-Object { -not $_.HasExited }).Count
"after closing: $left of those processes still running"
Get-Process blink-settings -ErrorAction SilentlyContinue | Stop-Process -Force
