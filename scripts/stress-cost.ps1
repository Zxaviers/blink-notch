# CPU cost of sustained animation: runs the app with DN_STRESS=<variant> (see
# src/stress.js) and reports CPU per second over a 10 s window, by process
# type. Needs no cursor movement. Extra WebView2 flags can be added with -Flags.
#   .\scripts\stress-cost.ps1 -Variants idle,transform,layout,text,scroll
param(
  [string]$Exe = "$PSScriptRoot\..\src-tauri\target\release\deadline-notch.exe",
  [string[]]$Variants = @("idle", "transform", "layout", "text", "scroll"),
  [string]$Flags = "",
  [int]$WindowSeconds = 10,
  [int]$Port = 8772
)

$ErrorActionPreference = "Stop"
. "$PSScriptRoot\profile.ps1"
$out = Join-Path $PSScriptRoot "out"
New-Item -ItemType Directory -Force $out | Out-Null
if (Get-Process -Name "deadline-notch" -ErrorAction SilentlyContinue) { "ABORT  another deadline-notch is running"; exit 2 }

$profile = Start-TestProfile -Id "stress" -Port $Port -Out $out
$rows = @()
try {
  foreach ($variant in $Variants) {
    $env:DN_STRESS = $variant
    if ($Flags) { $env:WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS = $Flags } else { Remove-Item Env:WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS -ErrorAction SilentlyContinue }
    $proc = Start-Process -FilePath (Resolve-Path $Exe) -PassThru
    Start-Sleep -Seconds 20
    $a = Get-TreeCpu $proc.Id
    Start-Sleep -Seconds $WindowSeconds
    $b = Get-TreeCpu $proc.Id
    $total = 0.0; $detail = @()
    foreach ($k in ($b.Keys | Sort-Object)) {
      $rate = ([double]$b[$k] - [double]$a[$k]) / $WindowSeconds
      $total += $rate
      if ($rate -ge 0.005) { $detail += ("{0}={1:P0}" -f $k, $rate) }
    }
    $rows += [pscustomobject]@{ Variant = $variant; "CPU (% of one core)" = [math]::Round($total * 100, 1); Where = ($detail -join "  ") }
    Stop-Process -Id $proc.Id -Force
    Start-Sleep -Seconds 2
  }
  "flags: '$Flags'   window: ${WindowSeconds}s   (100% = one full core; machine has $([Environment]::ProcessorCount) logical cores)"
  $rows | Format-Table -AutoSize | Out-String -Width 160
} finally {
  Remove-Item Env:DN_STRESS -ErrorAction SilentlyContinue
  Remove-Item Env:WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS -ErrorAction SilentlyContinue
  Get-Process -Name "deadline-notch" -ErrorAction SilentlyContinue | Stop-Process -Force
  Restore-TestProfile $profile
}
