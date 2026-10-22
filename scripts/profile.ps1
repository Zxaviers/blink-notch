# Shared helpers for the measurement scripts (dot-source this file).
# Seeds a throwaway profile (onboarded, one fixture feed whose link goes in
# Credential Manager) and starts the fixture calendar server; Restore-TestProfile
# undoes all of it and puts your own app data back.

function Start-TestProfile {
  param([string]$Id, [int]$Port, [string]$Out)
  $dataDir = Join-Path $env:APPDATA "dev.riski.deadlinenotch"
  $backup = $null
  if (Test-Path $dataDir) {
    $backup = "$dataDir.$Id-backup"
    if (Test-Path $backup) { Remove-Item $backup -Recurse -Force }
    Move-Item $dataDir $backup
  }
  New-Item -ItemType Directory -Force $dataDir | Out-Null
  $settings = @{ version = 1; onboarded = $true; toast = $false
    feeds = @(@{ id = $Id; kind = "ics"; label = $Id; enabled = $true }) } | ConvertTo-Json -Depth 5
  [System.IO.File]::WriteAllText((Join-Path $dataDir "settings.json"), $settings, (New-Object System.Text.UTF8Encoding($false)))
  cmdkey /generic:"$Id.deadline-notch" /user:$Id /pass:"http://127.0.0.1:$Port/cal.ics" | Out-Null
  $server = Start-Process node -ArgumentList "`"$PSScriptRoot\serve-fixture.mjs`" $Port" -PassThru -WindowStyle Hidden `
    -RedirectStandardOutput (Join-Path $Out "$Id-server.log")
  Start-Sleep -Seconds 1
  [pscustomobject]@{ Id = $Id; DataDir = $dataDir; Backup = $backup; Server = $server }
}

function Restore-TestProfile {
  param($Profile)
  if ($Profile.Server -and -not $Profile.Server.HasExited) { Stop-Process -Id $Profile.Server.Id -Force }
  cmdkey /delete:"$($Profile.Id).deadline-notch" | Out-Null
  Start-Sleep -Milliseconds 800
  Remove-Item $Profile.DataDir -Recurse -Force -ErrorAction SilentlyContinue
  if ($Profile.Backup) { Move-Item $Profile.Backup $Profile.DataDir }
}

# Sum of per-process CPU seconds for the app and everything under it, keyed by
# process type (app, wv2-browser, renderer, gpu-process, utility, ...).
function Get-TreeCpu {
  param([int]$RootPid)
  $all = Get-CimInstance Win32_Process | Select-Object ProcessId, ParentProcessId, Name, CommandLine
  $ids = [System.Collections.Generic.HashSet[int]]::new(); [void]$ids.Add($RootPid)
  do { $n = 0; foreach ($p in $all) { if ($ids.Contains([int]$p.ParentProcessId) -and $ids.Add([int]$p.ProcessId)) { $n++ } } } while ($n -gt 0)
  $map = @{}
  foreach ($p in $all | Where-Object { $ids.Contains([int]$_.ProcessId) }) {
    $type = if ($p.Name -eq "deadline-notch.exe") { "app" } elseif ($p.CommandLine -match '--type=([\w-]+)') { $Matches[1] } else { "wv2-browser" }
    $cpu = (Get-Process -Id $p.ProcessId -ErrorAction SilentlyContinue).CPU
    $map[$type] = [double]$map[$type] + [double]$cpu
  }
  $map
}
