# SPDX-License-Identifier: GPL-3.0-or-later
# Tessalith DDS for GIMP: builds dist\TessalithDDS-GIMP-<workspace version>.zip and its .sha256.txt.
#
#   .\gimp\package.ps1          build psdds.dll (crates\psdds\build.ps1), stage, zip, then run the checks
#   .\gimp\package.ps1 -Check   the same; the checks always run, so this only says so out loud
#
# Needs cargo-about (cargo install cargo-about --locked) for the notices, and GIMP 3 (SDC_GIMP_DIR) for the ABI check.
param([switch]$Check)
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
$stage = Join-Path $dist 'TessalithDDS-GIMP-stage'
Remove-Item $stage -Recurse -Force -ErrorAction SilentlyContinue
$plug = Join-Path $stage 'tessalith-dds'
New-Item -ItemType Directory -Force $plug | Out-Null

# Every .py the plug-in folder holds is needed at runtime (the plug-in file imports the other two).
$src = Join-Path $here 'tessalith-dds'
$pyFiles = Get-ChildItem $src -Filter *.py -File | Sort-Object Name
foreach ($f in $pyFiles) { Copy-Item $f.FullName $plug }
Copy-Item $dll $plug

# COPYING = the plug-in's GPL-3.0-or-later text; LICENSE = psdds.dll's MIT text (the notices say "see LICENSE").
Copy-Item (Join-Path $repo 'chainner\COPYING') (Join-Path $stage 'COPYING')
Copy-Item (Join-Path $repo 'crates\psdds\LICENSE') (Join-Path $stage 'LICENSE')
Copy-Item (Join-Path $here 'README.md') (Join-Path $stage 'README.md')
$notices = Join-Path $stage 'THIRD_PARTY_NOTICES.md'
& (Join-Path $repo 'crates\psdds\notices.ps1') -Binary psdds.dll -Product 'Tessalith psdds.dll' -Out $notices
if ($LASTEXITCODE -ne $null -and $LASTEXITCODE -ne 0) { throw "notices.ps1 failed ($LASTEXITCODE)" }
# notices.ps1 leaves its work files next to -Out; the zip takes only the notices file.
Add-Content $notices -Encoding utf8NoBOM -Value "`n## Licences of this package`n`nThe plug-in's Python files are GPL-3.0-or-later (see COPYING). The psdds crate is MIT (see LICENSE). The engine and encoders-sys code inside psdds.dll is MPL-2.0 (source: the repository and commit named above, DC41)."

$entries = @(
    'tessalith-dds/psdds.dll'
) + ($pyFiles | ForEach-Object { "tessalith-dds/$($_.Name)" }) + @('COPYING', 'LICENSE', 'THIRD_PARTY_NOTICES.md', 'README.md')

New-Item -ItemType Directory -Force $dist | Out-Null
$zip = Join-Path $dist "TessalithDDS-GIMP-$version.zip"
Remove-Item $zip -ErrorAction SilentlyContinue
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
$want = @($entries | Sort-Object)
if (($names -join '|') -ne ($want -join '|')) { $fail += "zip holds [$($names -join ', ')], expected [$($want -join ', ')]" }

# The plug-in folder must hold every .py the source folder holds and nothing else, and the plug-in must import no other module.
$imports = Select-String -Path (Join-Path $src '*.py') -Pattern '^\s*(?:from|import)\s+(psdds_\w+|tdds_\w+|tessalith_\w+)' | ForEach-Object { $_.Matches[0].Groups[1].Value } | Sort-Object -Unique
foreach ($m in $imports) { if ($names -notcontains "tessalith-dds/$m.py") { $fail += "imported module $m.py is not in the zip" } }

$probe = Join-Path ([IO.Path]::GetTempPath()) ("tdds-gimp-pkg-" + [guid]::NewGuid())
try {
    [IO.Compression.ZipFile]::ExtractToDirectory($zip, $probe)
    if ((Get-FileHash (Join-Path $probe 'tessalith-dds\psdds.dll')).Hash -ne (Get-FileHash $dll).Hash) { $fail += 'packaged psdds.dll differs from the built one' }
    $gpl = Get-Content (Join-Path $probe 'COPYING') -Raw
    if ($gpl -notmatch 'GNU GENERAL PUBLIC LICENSE\s+Version 3, 29 June 2007' -or $gpl -notmatch 'END OF TERMS AND CONDITIONS' -or $gpl -notmatch 'How to Apply These Terms') { $fail += 'COPYING is not the full GPL-3.0 text' }
    if ((Get-Content (Join-Path $probe 'LICENSE') -Raw) -notmatch '^MIT License') { $fail += 'LICENSE is not the MIT text' }
    $n = Get-Content (Join-Path $probe 'THIRD_PARTY_NOTICES.md') -Raw
    if ($n -notmatch 'MPL-2\.0') { $fail += 'THIRD_PARTY_NOTICES.md has no MPL-2.0 text' }
    if ($n -match '(?i)(engine|encoders-sys)[^.\r\n]{0,80}\bMIT\b' -and $n -notmatch 'MPL') { $fail += 'notices describe the engine as MIT' }

    # ABI: the packaged wrapper loads the packaged DLL and refuses unless psdds_abi_version() == EXPECTED_ABI.
    $gimpDir = if ($env:SDC_GIMP_DIR) { $env:SDC_GIMP_DIR } else { 'C:\Program Files\GIMP 3' }
    $py = Join-Path $gimpDir 'bin\python.exe'
    if (-not (Test-Path $py)) { $fail += "GIMP's Python not found at $py (set SDC_GIMP_DIR); cannot check the ABI" }
    else {
        $saved = $env:TESSALITH_PSDDS_DLL; Remove-Item env:TESSALITH_PSDDS_DLL -ErrorAction SilentlyContinue
        try {
            $out = & $py -I -c 'import sys; sys.path.insert(0, sys.argv[1]); import psdds_ctypes as p; p.Psdds(); print("abi", p.EXPECTED_ABI)' (Join-Path $probe 'tessalith-dds') 2>&1 | ForEach-Object { "$_" }
            if ($LASTEXITCODE -ne 0 -or -not ($out -match '^abi \d+$')) { $fail += "ABI check failed: $($out -join ' ')" } else { Write-Host "ABI ok: $($out -join ' ')" }
        } finally { if ($saved) { $env:TESSALITH_PSDDS_DLL = $saved } }
    }
} finally { Remove-Item $probe -Recurse -Force -ErrorAction SilentlyContinue }

if ($fail) { $fail | ForEach-Object { Write-Host "FAIL $_" }; throw 'package check failed' }
Remove-Item $stage -Recurse -Force
$hash = (Get-FileHash $zip -Algorithm SHA256).Hash.ToLower()
"$hash  $(Split-Path $zip -Leaf)" | Set-Content "$zip.sha256.txt" -Encoding ascii
Write-Host "PASS package check"
Write-Host "Packaged $zip"
Write-Host "SHA256 $hash"
