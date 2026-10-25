# Tessalith DDS for Paint.NET: builds dist\TessalithDDS-PaintDotNet-<workspace version>.zip. GPL-3.0-or-later (see paintdotnet/COPYING).
#
#   .\paintdotnet\package.ps1
#
# Runs build.ps1 (Release; it builds psdds.dll with the dependents and exports checks), writes THIRD_PARTY_NOTICES.md with
# crates\psdds\notices.ps1 (needs cargo-about on PATH), and zips seven files under TessalithDDS\ so that unzipping into a
# FileTypes folder gives the right layout: TessalithDds.dll, TessalithDds.deps.json, psdds.dll, README.md, LICENSE (psdds.dll's
# MIT text), COPYING (GPL-3.0 plus the Paint.NET permission) and THIRD_PARTY_NOTICES.md. Fails if any PaintDotNet*.dll is staged.
$ErrorActionPreference = 'Stop'
$here = $PSScriptRoot
$repo = Split-Path $here -Parent
$m = [regex]::Match((Get-Content (Join-Path $repo 'Cargo.toml') -Raw), '(?ms)^\[workspace\.package\].*?^version\s*=\s*"([^"]+)"')
if (-not $m.Success) { throw 'No workspace version in Cargo.toml.' }
$version = $m.Groups[1].Value

& (Join-Path $here 'build.ps1') -Configuration Release
$out = Join-Path $here 'bin\Release\net9.0-windows'

$root = Join-Path $here 'bin\zip'
$stage = Join-Path $root 'TessalithDDS'
Remove-Item $root -Recurse -Force -ErrorAction SilentlyContinue
New-Item -ItemType Directory -Force $stage | Out-Null
foreach ($f in 'TessalithDds.dll', 'TessalithDds.deps.json', 'psdds.dll') {
    $src = Join-Path $out $f
    if (-not (Test-Path $src)) { throw "$src is missing." }
    Copy-Item $src $stage
}
foreach ($f in 'README.md', 'LICENSE', 'COPYING') { Copy-Item (Join-Path $here $f) $stage }
& (Join-Path $repo 'crates\psdds\notices.ps1') -Binary psdds.dll -Product 'Tessalith psdds.dll' -Out (Join-Path $stage 'THIRD_PARTY_NOTICES.md')

if (Get-ChildItem $stage -Recurse -Filter 'PaintDotNet*.dll') { throw 'A PaintDotNet*.dll is staged; the package must hold none of Paint.NET.' }
$count = (Get-ChildItem $stage -File).Count
if ($count -ne 7) { throw "Expected 7 staged files, found $count." }

$dist = Join-Path $repo 'dist'
New-Item -ItemType Directory -Force $dist | Out-Null
$zip = Join-Path $dist "TessalithDDS-PaintDotNet-$version.zip"
Remove-Item $zip -ErrorAction SilentlyContinue
Compress-Archive -Path $stage -DestinationPath $zip
$hash = (Get-FileHash $zip -Algorithm SHA256).Hash.ToLower()
"$hash  $(Split-Path $zip -Leaf)" | Set-Content "$zip.sha256.txt" -Encoding ascii
Write-Host "Packaged $zip"
Write-Host "SHA256 $hash"
