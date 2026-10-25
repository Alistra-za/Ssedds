# Tessalith DDS: builds the release zip dist\TessalithDDS-<workspace version>.zip. MIT licence (see photoshop/LICENSE).
#
#   .\photoshop\package.ps1
#
# Runs build.ps1 (Release), the licence check (cargo about over the psdds crate tree; fails on any licence outside
# crates\psdds\about.toml), checks the plugin links no VC++ runtime DLL, then zips TessalithDDS.8bi, LICENSE,
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
$notices = Join-Path $stage 'THIRD_PARTY_NOTICES.md'
& (Join-Path $repo 'crates\psdds\notices.ps1') -Binary TessalithDDS.8bi -Product 'Tessalith DDS' -Out $notices

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
