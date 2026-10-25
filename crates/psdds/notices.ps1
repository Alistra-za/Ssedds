# Third-party notices for one psdds binary: runs the licence check (cargo about over the psdds crate tree) and writes
# THIRD_PARTY_NOTICES.md text to -Out. MIT licence.
#
#   .\crates\psdds\notices.ps1 -Binary TessalithDDS.8bi -Product 'Tessalith DDS' -Out build\zip\THIRD_PARTY_NOTICES.md
#
# Fails on any licence outside crates\psdds\about.toml, and when cargo-about is missing or cannot run
# (cargo install cargo-about --locked).
param(
    [Parameter(Mandatory)][string]$Binary,   # file name shown in the notices, e.g. TessalithDDS.8bi or psdds.dll
    [Parameter(Mandatory)][string]$Product,  # heading name, e.g. 'Tessalith DDS'
    [Parameter(Mandatory)][string]$Out       # notices file to write
)
$ErrorActionPreference = 'Stop'
$here = $PSScriptRoot
$repo = Split-Path (Split-Path $here -Parent) -Parent
if (-not (Get-Command cargo-about -ErrorAction SilentlyContinue)) {
    throw 'cargo-about is not installed (cargo install cargo-about --locked); the licence check needs it.'
}
$Out = $ExecutionContext.SessionState.Path.GetUnresolvedProviderPathFromPSPath($Out)  # relative or absolute
$outDir = Split-Path $Out -Parent
New-Item -ItemType Directory -Force $outDir | Out-Null
$about = Join-Path $outDir 'about.md'
$tpl = Join-Path $outDir 'about.hbs'
# cargo-about's template engine has no custom variables, so fill {{binary}} here.
[IO.File]::WriteAllText($tpl, (Get-Content (Join-Path $here 'about.hbs') -Raw).Replace('{{binary}}', $Binary), (New-Object Text.UTF8Encoding $false))
$aboutLog = Join-Path $outDir 'about.log'
cargo about generate --offline --frozen --fail -c (Join-Path $here 'about.toml') -m (Join-Path $here 'Cargo.toml') $tpl -o $about 2> $aboutLog
$aboutExit = $LASTEXITCODE
$aboutErr = Get-Content $aboutLog -Raw -ErrorAction SilentlyContinue
Remove-Item $aboutLog, $tpl -ErrorAction SilentlyContinue
if ($aboutExit -ne 0) {
    if ($aboutErr) { Write-Host $aboutErr }
    # cargo-about reports a rejected licence as "failed to satisfy license requirements"; any other failure is the tool.
    if ($aboutErr -match 'failed to satisfy license requirements') {
        throw "Licence check failed ($aboutExit): a dependency has a licence outside crates\psdds\about.toml (see above)."
    }
    throw "cargo about failed to run ($aboutExit); see its output above. This is not a licence verdict."
}

$lic = Join-Path $repo 'LICENSES'
$version = (Select-String -Path (Join-Path $repo 'Cargo.toml') -Pattern '^version\s*=\s*"([^"]+)"' | Select-Object -First 1).Matches[0].Groups[1].Value
if (-not $version) { throw 'Workspace version not found in Cargo.toml.' }
$commit = git -C $repo rev-parse HEAD
if ($LASTEXITCODE -ne 0 -or -not $commit) { throw 'git rev-parse HEAD failed; the notices must name the source commit (MPL-2.0 section 3.2).' }
$head = @"
# Third-party notices ($Product)

$Product (``$Binary``) is MIT-licensed (see ``LICENSE``), except the Tessalith engine inside it (the ``engine`` and ``encoders-sys`` crates), which is under the Mozilla Public License 2.0 (its text is in the MPL-2.0 section below). Their source code is at https://github.com/Alistra-za/Tessalith, commit ``$commit`` (version $version).

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
[IO.File]::WriteAllText($Out, ($parts -join ''), (New-Object Text.UTF8Encoding $false))
Remove-Item $about
