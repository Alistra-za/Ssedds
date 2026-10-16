# Builds (unless -NoBuild) and packages the release zip: dist/Tessalith-<version>-win-x64.zip + SHA256SUMS.txt.
# Run from anywhere. The zip holds the two exes and the named files only, nothing else.
#   tools\package-release.ps1                      build, stage, zip
#   tools\package-release.ps1 -Suffix alpha.1      names it Tessalith-0.8.0-alpha.1-win-x64.zip
#   tools\package-release.ps1 -NoBuild -TargetDir <dir holding tessalith.exe and sdc.exe>
param(
    [switch]$NoBuild,
    [switch]$AllowDirty,
    [string]$TargetDir,
    [string]$Suffix = ''
)
$ErrorActionPreference = 'Stop'
$root = Split-Path -Parent $PSScriptRoot
Set-Location $root

if (-not $AllowDirty -and (git status --porcelain)) {
    throw 'git tree is dirty; commit or pass -AllowDirty'
}
$m = [regex]::Match((Get-Content Cargo.toml -Raw), '(?ms)\[workspace\.package\].*?^version\s*=\s*"([^"]+)"')
if (-not $m.Success) { throw 'no workspace version in Cargo.toml' }
$version = $m.Groups[1].Value
if ($Suffix) { $version = "$version-$Suffix" }

if (-not $TargetDir) { $TargetDir = Join-Path $root 'target\x86_64-pc-windows-msvc\release' }
if (-not $NoBuild) {
    cargo build --release -p sdc-gui -p sdc
    if ($LASTEXITCODE -ne 0) { throw 'cargo build failed' }
    if ($env:CARGO_TARGET_DIR) { $TargetDir = Join-Path $env:CARGO_TARGET_DIR 'x86_64-pc-windows-msvc\release' }
}

$name = "Tessalith-$version"
$dist = Join-Path $root 'dist'
$stage = Join-Path $dist $name
$zip = Join-Path $dist "$name-win-x64.zip"
Remove-Item -Recurse -Force $stage, $zip -ErrorAction SilentlyContinue
New-Item -ItemType Directory $stage | Out-Null

foreach ($exe in 'tessalith.exe', 'sdc.exe') {
    $src = Join-Path $TargetDir $exe
    if (-not (Test-Path $src)) { throw "missing $src" }
    Copy-Item $src $stage
}
foreach ($f in 'LICENSE', 'THIRD_PARTY_NOTICES.md', 'THIRD_PARTY_LICENSES.md', 'CHANGELOG.md') {
    Copy-Item (Join-Path $root $f) $stage
}
Copy-Item (Join-Path $root 'LICENSES') (Join-Path $stage 'LICENSES') -Recurse
Copy-Item (Join-Path $dist 'README.txt') (Join-Path $stage 'README.txt')

Compress-Archive -Path $stage -DestinationPath $zip
$sums = foreach ($f in $zip, (Join-Path $stage 'tessalith.exe'), (Join-Path $stage 'sdc.exe')) {
    $h = (Get-FileHash $f -Algorithm SHA256).Hash.ToLower()
    "$h  $(Split-Path -Leaf $f)"
}
$sums | Set-Content (Join-Path $dist 'SHA256SUMS.txt') -Encoding ascii
$sums
