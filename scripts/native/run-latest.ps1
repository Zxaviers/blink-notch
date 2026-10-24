# Build the newest Blink into target-run and start it, replacing the copy that is running.
#
#   .\scripts\native\run-latest.ps1              build, then start it (quits a copy started from target-run first)
#   .\scripts\native\run-latest.ps1 -Replace     also quit a copy that runs from somewhere else (asks it to leave
#                                                cleanly with `blink --quit`, never kills it)
#   .\scripts\native\run-latest.ps1 -NoBuild     start what is already built
#
# A running program cannot be overwritten on Windows, which is why a new build used to need a new folder each time.
param([switch]$Replace, [switch]$NoBuild, [switch]$Demo)
$ErrorActionPreference = 'Stop'
$repo = (Resolve-Path "$PSScriptRoot\..\..").Path
$target = Join-Path $repo 'target-run'
$exe = Join-Path $target 'release\blink.exe'

function Quit-Running([string]$why, [scriptblock]$filter) {
  $procs = @(Get-Process -Name 'blink' -ErrorAction SilentlyContinue | Where-Object $filter)
  foreach ($p in $procs) {
    "asking $($p.Path) to quit ($why)"
    & $p.Path --quit
  }
  foreach ($p in $procs) {
    for ($i = 0; $i -lt 40 -and -not $p.HasExited; $i++) { Start-Sleep -Milliseconds 250 }
    if (-not $p.HasExited) { throw "$($p.Path) did not quit: close it from the tray icon (right-click, Quit) and run this again" }
  }
}

# The copy built here must not be running while it is rebuilt.
Quit-Running 'it is about to be rebuilt' { $_.Path -like "$target\*" }
if ($Replace) { Quit-Running 'replaced by the new build' { $true } }

if (-not $NoBuild) {
  Push-Location $repo
  try {
    cargo build --release -p blink-native -p blink-settings --target-dir $target -j 1
    if ($LASTEXITCODE -ne 0) { throw 'the build failed' }
  } finally { Pop-Location }
}
if (-not (Test-Path $exe)) { throw "no build at $exe" }

$other = @(Get-Process -Name 'blink' -ErrorAction SilentlyContinue)
if ($other.Count -gt 0) {
  "Another Blink is running ($($other[0].Path)); a second one would only ask it to show itself. Run again with -Replace to swap it for this build."
  return
}
if ($Demo) { $env:BLINK_DEMO = '1' }
Start-Process -FilePath $exe
"started $exe"
