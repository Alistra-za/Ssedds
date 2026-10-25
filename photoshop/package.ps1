# Tessalith DDS: builds the release zip dist\TessalithDDS-<workspace version>.zip. MIT licence (see photoshop/LICENSE).
#
#   .\photoshop\package.ps1
#
# Runs build.ps1 (Release), the licence check (cargo about over the psdds crate tree; fails on any licence outside
# photoshop\about.toml), checks the plugin links no VC++ runtime DLL, then zips TessalithDDS.8bi, LICENSE,
# THIRD_PARTY_NOTICES.md and README.md. Writes dist\TessalithDDS-<version>.zip.sha256.txt next to it.
#
# Needs cargo-about on PATH (cargo install cargo-about --locked), besides what build.ps1 needs.
$ErrorActionPreference = 'Stop'
$here = $PSScriptRoot
$repo = Split-Path $here -Parent
$version = (Select-String -Path (Join-Path $repo 'Cargo.toml') -Pattern '^version\s*=\s*"([^"]+)"' | Select-Object -First 1).Matches[0].Groups[1].Value
if (-not $version) { throw 'Workspace version not found in Cargo.toml.' }

& (Join-Path $here 'build.ps1') -Configuration Release
if ($LASTEXITCODE -ne 0) { throw "build.ps1 failed ($LASTEXITCODE)" }
$plugin = Join-Path $here 'build\Release\TessalithDDS.8bi'
if (-not (Test-Path $plugin)) { throw "$plugin is missing." }

# Static runtime: dumpbin must list no VCRUNTIME / MSVCP / UCRT DLL.
$vswhere = Join-Path ${env:ProgramFiles(x86)} 'Microsoft Visual Studio\Installer\vswhere.exe'
$vs = & $vswhere -latest -products * -requires Microsoft.VisualStudio.Component.VC.Tools.x86.x64 -property installationPath
$dumpbin = Get-ChildItem (Join-Path $vs 'VC\Tools\MSVC') -Recurse -Filter dumpbin.exe | Where-Object { $_.FullName -match 'Hostx64\\x64' } | Select-Object -First 1
if (-not $dumpbin) { throw 'dumpbin.exe was not found.' }
$deps = & $dumpbin.FullName /dependents $plugin
$deps | Write-Host
if ($deps -match '(?i)VCRUNTIME|MSVCP|api-ms-win-crt') { throw 'The plugin depends on a VC++ runtime DLL; it must use the static CRT.' }

# Licence check + notices.
$stage = Join-Path $here 'build\zip'
Remove-Item $stage -Recurse -Force -ErrorAction SilentlyContinue
New-Item -ItemType Directory -Force $stage | Out-Null
$about = Join-Path $stage 'about.md'
if (-not (Get-Command cargo-about -ErrorAction SilentlyContinue)) {
    throw 'cargo-about is not installed (cargo install cargo-about --locked); the licence check needs it.'
}
$aboutLog = Join-Path $stage 'about.log'
cargo about generate --offline --frozen --fail -c (Join-Path $here 'about.toml') -m (Join-Path $repo 'crates\psdds\Cargo.toml') (Join-Path $here 'about.hbs') -o $about 2> $aboutLog
$aboutExit = $LASTEXITCODE
$aboutErr = Get-Content $aboutLog -Raw -ErrorAction SilentlyContinue
Remove-Item $aboutLog -ErrorAction SilentlyContinue
if ($aboutExit -ne 0) {
    if ($aboutErr) { Write-Host $aboutErr }
    # cargo-about reports a rejected licence as "failed to satisfy license requirements"; any other failure is the tool.
    if ($aboutErr -match 'failed to satisfy license requirements') {
        throw "Licence check failed ($aboutExit): a dependency has a licence outside photoshop\about.toml (see above)."
    }
    throw "cargo about failed to run ($aboutExit); see its output above. This is not a licence verdict."
}

$lic = Join-Path $repo 'LICENSES'
$notices = Join-Path $stage 'THIRD_PARTY_NOTICES.md'
$commit = git -C $repo rev-parse HEAD
if ($LASTEXITCODE -ne 0 -or -not $commit) { throw 'git rev-parse HEAD failed; the notices must name the source commit (MPL-2.0 section 3.2).' }
$head = @"
# Third-party notices (Tessalith DDS)

Tessalith DDS (``TessalithDDS.8bi``) is MIT-licensed (see ``LICENSE``), except the Tessalith engine inside it (the ``engine`` and ``encoders-sys`` crates), which is under the Mozilla Public License 2.0 (its text is in the MPL-2.0 section below). Their source code is at https://github.com/Alistra-za/Tessalith, commit ``$commit`` (version $version).

It contains the code below. Full licence texts follow.

## Vendored encoders

- **bc7e.ispc** (bc7enc_rdo commit b943862), Copyright (C) 2018-2021 Binomial LLC, Apache License 2.0. Modified copy: patch ``0001-exact-opaque-alpha.patch`` (change notice at the top of ``bc7e.ispc``).
- **rgbcx** (bc7enc_rdo commit b943862), Richard Geldreich, Jr., MIT or Public Domain (Unlicense); used here under MIT. Modified ``rgbcx.h`` (BC4 round-to-nearest).
- **Intel ISPC Texture Compressor** (commit 79ddbc9), Copyright (c) 2016-2019 Intel Corporation, MIT.
- The ISPC objects were compiled with Intel ISPC 1.31.0 (BSD-3-Clause compiler; the objects carry the source files' licences).

### bc7e: Apache License 2.0

``````text

"@
$parts = @($head, (Get-Content (Join-Path $lic 'Apache-2.0-bc7e.txt') -Raw), "``````n`n### rgbcx and bc7enc_rdo: licence file`n`n``````text`n", (Get-Content (Join-Path $lic 'bc7enc_rdo-LICENSE.txt') -Raw), "``````n`n### Intel ISPC Texture Compressor: MIT`n`n``````text`n", (Get-Content (Join-Path $lic 'ISPCTextureCompressor-license.txt') -Raw), "``````n`n", (Get-Content $about -Raw))
[IO.File]::WriteAllText($notices, ($parts -join ''), (New-Object Text.UTF8Encoding $false))
Remove-Item $about

Copy-Item $plugin $stage
Copy-Item (Join-Path $here 'LICENSE') $stage
Copy-Item (Join-Path $here 'README.md') $stage
$dist = Join-Path $repo 'dist'
New-Item -ItemType Directory -Force $dist | Out-Null
$zip = Join-Path $dist "TessalithDDS-$version.zip"
Remove-Item $zip -ErrorAction SilentlyContinue
Compress-Archive -Path (Join-Path $stage 'TessalithDDS.8bi'), (Join-Path $stage 'LICENSE'), $notices, (Join-Path $stage 'README.md') -DestinationPath $zip
$hash = (Get-FileHash $zip -Algorithm SHA256).Hash.ToLower()
"$hash  $(Split-Path $zip -Leaf)" | Set-Content "$zip.sha256.txt" -Encoding ascii
Write-Host "Packaged $zip"
Write-Host "SHA256 $hash"
