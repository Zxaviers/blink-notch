# Builds both programs in release mode and the installer, and prints their sizes.
#   powershell -File scripts\native\build-release.ps1
# Signing is not done here; see docs/RELEASE.md for the plan.
$ErrorActionPreference = 'Stop'
$root = Resolve-Path "$PSScriptRoot\..\.."
Push-Location $root
try {
  cargo build --release -p blink-native -p blink-settings -p blink-cli
  if ($LASTEXITCODE -ne 0) { throw 'cargo build failed' }
  $version = (Select-String -Path 'Cargo.toml' -Pattern '^version = "(.+)"').Matches[0].Groups[1].Value
  $nsis = Join-Path $env:LOCALAPPDATA 'tauri\NSIS\makensis.exe'
  if (-not (Test-Path $nsis)) { throw "makensis not found at $nsis (any NSIS 3 works; point `$nsis at it)" }
  $out = Join-Path $root "scripts\out\Blink-$version-setup.exe"
  New-Item -ItemType Directory -Force (Split-Path $out) | Out-Null
  & $nsis -V1 "/DVERSION=$version" "/DBINDIR=$root\target\release" "/DOUT=$out" "$root\installer\blink.nsi"
  if ($LASTEXITCODE -ne 0) { throw 'makensis failed' }
  foreach ($f in 'target\release\blink.exe', 'target\release\blink-settings.exe', 'target\release\blinkctl.exe', "scripts\out\Blink-$version-setup.exe") {
    '{0,-40} {1,8} KB' -f $f, [math]::Round((Get-Item (Join-Path $root $f)).Length / 1KB)
  }
  Get-FileHash $out -Algorithm SHA256 | ForEach-Object { "SHA-256 $($_.Hash)  $(Split-Path $out -Leaf)" }
} finally { Pop-Location }
