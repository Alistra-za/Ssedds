# SPDX-License-Identifier: GPL-3.0-or-later
# Tessalith DDS nodes for chaiNNer: builds dist\TessalithDDS-chaiNNer-<workspace version>.zip and its .sha256.txt.
#
#   .\chainner\package.ps1      build psdds.dll (crates\psdds\build.ps1), stage, zip, then run the checks
#
# The zip installs without the repo: unzip, close chaiNNer, run install.ps1 (it finds psdds.dll and the notices beside it).
# Needs cargo-about (cargo install cargo-about --locked) for the notices, and chaiNNer's bundled Python for the ABI check.
$ErrorActionPreference = 'Stop'
$here = $PSScriptRoot
$repo = Split-Path $here -Parent
$version = (Select-String -Path (Join-Path $repo 'Cargo.toml') -Pattern '^version\s*=\s*"([^"]+)"' | Select-Object -First 1).Matches[0].Groups[1].Value
if (-not $version) { throw 'Workspace version not found in Cargo.toml.' }

& (Join-Path $repo 'crates\psdds\build.ps1')
if ($LASTEXITCODE -ne 0) { throw "crates\psdds\build.ps1 failed ($LASTEXITCODE)" }
$dll = Join-Path $repo 'target\psdds\x86_64-pc-windows-msvc\release\psdds.dll'
if (-not (Test-Path $dll)) { throw "$dll is missing." }

$dist = Join-Path $repo 'dist'
$stage = Join-Path $dist 'TessalithDDS-chaiNNer-stage'
Remove-Item $stage -Recurse -Force -ErrorAction SilentlyContinue
$nodes = Join-Path $stage 'tessalith_dds'
New-Item -ItemType Directory -Force $nodes | Out-Null

$pyFiles = Get-ChildItem (Join-Path $here 'tessalith_dds') -Filter *.py -File | Sort-Object Name
foreach ($f in $pyFiles) { Copy-Item $f.FullName $nodes }
Copy-Item $dll $nodes
foreach ($f in 'install.ps1', 'README.md', 'COPYING', 'LICENSE') { Copy-Item (Join-Path $here $f) $stage }
$notices = Join-Path $stage 'THIRD_PARTY_NOTICES.md'
& (Join-Path $repo 'crates\psdds\notices.ps1') -Binary psdds.dll -Product 'Tessalith psdds.dll' -Out $notices
if (-not (Test-Path $notices)) { throw 'notices.ps1 wrote no THIRD_PARTY_NOTICES.md' }

$entries = @('tessalith_dds/psdds.dll') + ($pyFiles | ForEach-Object { "tessalith_dds/$($_.Name)" }) +
    @('install.ps1', 'README.md', 'COPYING', 'LICENSE', 'THIRD_PARTY_NOTICES.md')

New-Item -ItemType Directory -Force $dist | Out-Null
$zip = Join-Path $dist "TessalithDDS-chaiNNer-$version.zip"
Remove-Item $zip, "$zip.sha256.txt" -ErrorAction SilentlyContinue
Add-Type -AssemblyName System.IO.Compression, System.IO.Compression.FileSystem
$za = [IO.Compression.ZipFile]::Open($zip, 'Create')
try {
    foreach ($e in $entries) {
        [void][IO.Compression.ZipFileExtensions]::CreateEntryFromFile($za, (Join-Path $stage ($e -replace '/', '\')), $e, 'Optimal')
    }
} finally { $za.Dispose() }

# ---- Checks ----
$fail = @()
$za = [IO.Compression.ZipFile]::OpenRead($zip)
try { $names = @($za.Entries | ForEach-Object { $_.FullName } | Sort-Object) } finally { $za.Dispose() }
if (($names -join '|') -ne (@($entries | Sort-Object) -join '|')) { $fail += "zip holds [$($names -join ', ')], expected [$($entries -join ', ')]" }

$probe = Join-Path ([IO.Path]::GetTempPath()) ("tdds-chainner-pkg-" + [guid]::NewGuid())
try {
    [IO.Compression.ZipFile]::ExtractToDirectory($zip, $probe)
    if ((Get-FileHash (Join-Path $probe 'tessalith_dds\psdds.dll')).Hash -ne (Get-FileHash $dll).Hash) { $fail += 'packaged psdds.dll differs from the built one' }
    $gpl = Get-Content (Join-Path $probe 'COPYING') -Raw
    if ($gpl -notmatch 'GNU GENERAL PUBLIC LICENSE\s+Version 3, 29 June 2007' -or $gpl -notmatch 'END OF TERMS AND CONDITIONS') { $fail += 'COPYING is not the full GPL-3.0 text' }
    if ((Get-Content (Join-Path $probe 'LICENSE') -Raw) -notmatch '^MIT License') { $fail += 'LICENSE is not the MIT text' }
    $n = Get-Content (Join-Path $probe 'THIRD_PARTY_NOTICES.md') -Raw
    if ($n -notmatch 'MPL-2\.0') { $fail += 'THIRD_PARTY_NOTICES.md has no MPL-2.0 text' }
    if ($n -match '(?i)(engine|encoders-sys)[^.\r\n]{0,80}\bMIT\b') { $fail += 'notices describe the engine as MIT' }

    # Install the unzipped release into a fake chaiNNer src folder: proves install.ps1 works without the repo
    # (it also loads the staged psdds.dll with chaiNNer's Python and prints its ABI).
    $fake = Join-Path $probe 'fake-src'
    New-Item -ItemType Directory -Force (Join-Path $fake 'packages\chaiNNer_standard') | Out-Null
    $saved = $env:TESSALITH_PSDDS_DLL; Remove-Item env:TESSALITH_PSDDS_DLL -ErrorAction SilentlyContinue
    try { $out = & pwsh -NoProfile -File (Join-Path $probe 'install.ps1') -Target $fake 2>&1 | ForEach-Object { "$_" }; $rc = $LASTEXITCODE }
    finally { if ($saved) { $env:TESSALITH_PSDDS_DLL = $saved } }
    $installed = Join-Path $fake 'packages\chaiNNer_standard\tessalith_dds'
    if ($rc -ne 0 -or -not (Test-Path (Join-Path $installed 'psdds.dll'))) { $fail += "release install failed: $($out -join ' ')" }
    elseif (-not ($out -match '^psdds version and ABI: \S+ 4$')) { $fail += "release install did not report ABI 4: $($out -join ' ')" }
    else {
        $got = @(Get-ChildItem $installed -File -Name | Sort-Object) -join '|'
        $want = @(@($pyFiles.Name) + 'psdds.dll', 'THIRD_PARTY_NOTICES.md', 'COPYING', 'LICENSE' | Sort-Object) -join '|'
        if ($got -ne $want) { $fail += "installed [$got], expected [$want]" } else { Write-Host ($out -match '^psdds version') }
    }
} finally { Remove-Item $probe -Recurse -Force -ErrorAction SilentlyContinue }

if ($fail) { $fail | ForEach-Object { Write-Host "FAIL $_" }; throw 'package check failed' }
Remove-Item $stage -Recurse -Force
$hash = (Get-FileHash $zip -Algorithm SHA256).Hash.ToLower()
"$hash  $(Split-Path $zip -Leaf)" | Set-Content "$zip.sha256.txt" -Encoding ascii
Write-Host 'PASS package check'
Write-Host "Packaged $zip"
Write-Host "SHA256 $hash"
