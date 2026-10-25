# Makes everything a release needs, in scripts\out\release\:
#   Blink-<version>-setup.exe   the installer
#   SHA256SUMS.txt              its SHA-256 (what the release page shows)
#   latest.json                 the update file ("Check for updates" in Blink settings reads it): version, link, hash, notes
#
#   powershell -File scripts\native\make-release.ps1 [-BinDir target-b\release] [-Repo Zxaviers/blink-notch] [-Notes "text"]
#
# With -BinDir the programs already built there are packed; otherwise both are built (cargo build --release). The version is the
# workspace's (Cargo.toml). Signing is not done here; see docs/RELEASE.md.
param([string]$BinDir = '', [string]$Repo = 'Zxaviers/blink-notch', [string]$Notes = '')
$ErrorActionPreference = 'Stop'
$root = (Resolve-Path "$PSScriptRoot\..\..").Path
Push-Location $root
try {
  $version = (Select-String -Path 'Cargo.toml' -Pattern '^version = "(.+)"').Matches[0].Groups[1].Value
  if ($BinDir) {
    $bin = (Resolve-Path $BinDir).Path
  } else {
    cargo build --release -p blink-native -p blink-settings
    if ($LASTEXITCODE -ne 0) { throw 'cargo build failed' }
    $bin = "$root\target\release"
  }
  foreach ($f in 'blink.exe', 'blink-settings.exe') { if (-not (Test-Path "$bin\$f")) { throw "$bin\$f is missing" } }
  $installed = (Get-Item "$bin\blink.exe").VersionInfo.ProductVersion
  if ($installed -ne $version) { throw "blink.exe is version $installed but Cargo.toml says ${version}; build again" }

  $nsis = if ($env:MAKENSIS) { $env:MAKENSIS } else { Join-Path $env:LOCALAPPDATA 'tauri\NSIS\makensis.exe' }
  if (-not (Test-Path $nsis)) {
    $found = Get-Command makensis -ErrorAction SilentlyContinue
    if ($found) { $nsis = $found.Source } else { throw "makensis not found (set MAKENSIS to it; any NSIS 3 works)" }
  }
  $out = "$root\scripts\out\release"
  Remove-Item $out -Recurse -Force -ErrorAction SilentlyContinue
  New-Item -ItemType Directory -Force $out | Out-Null
  $setup = "$out\Blink-$version-setup.exe"
  $url = "https://github.com/$Repo"
  & $nsis -V1 "/DVERSION=$version" "/DBINDIR=$bin" "/DOUT=$setup" "/DURL=$url" "$root\installer\blink.nsi"
  if ($LASTEXITCODE -ne 0) { throw 'makensis failed' }

  $hash = (Get-FileHash $setup -Algorithm SHA256).Hash.ToLower()
  $utf8 = New-Object System.Text.UTF8Encoding($false)
  [System.IO.File]::WriteAllText("$out\SHA256SUMS.txt", "$hash  Blink-$version-setup.exe`n", $utf8)
  if (-not $Notes) { $Notes = "Blink $version. What changed is in the changelog on the project page." }
  $update = [ordered]@{ version = $version; url = "$url/releases/download/v$version/Blink-$version-setup.exe"; sha256 = $hash; notes = $Notes }
  [System.IO.File]::WriteAllText("$out\latest.json", ($update | ConvertTo-Json), $utf8)

  Get-ChildItem $out | ForEach-Object { '{0,-34} {1,8} KB' -f $_.Name, [math]::Round($_.Length / 1KB) }
  "SHA-256 $hash"
  "Upload all three to the release tagged v$version; the link people paste into Blink is"
  "  $url/releases/latest/download/latest.json"
} finally { Pop-Location }
