# Tessalith DDS: builds psdds (static CRT) as psdds.dll + psdds.lib for every host. MIT licence.
#
#   .\crates\psdds\build.ps1          release build into target\psdds, then the dependents and exports checks
#   .\crates\psdds\build.ps1 -Test    also runs the DLL host check (examples\dll_host.rs: FreeLibrary must leave psdds.dll mapped)
param([switch]$Test)
$ErrorActionPreference = 'Stop'
$repo = Split-Path (Split-Path $PSScriptRoot -Parent) -Parent

$saved = @{ CARGO_TARGET_DIR = $env:CARGO_TARGET_DIR; RUSTFLAGS = $env:RUSTFLAGS; CARGO_BUILD_JOBS = $env:CARGO_BUILD_JOBS }
try {
    $env:CARGO_TARGET_DIR = Join-Path $repo 'target\psdds'
    $env:RUSTFLAGS = '-C target-feature=+crt-static'
    if (-not $env:CARGO_BUILD_JOBS) { $env:CARGO_BUILD_JOBS = '4' }
    cargo build -p psdds --release --offline --manifest-path (Join-Path $repo 'Cargo.toml')
    if ($LASTEXITCODE -ne 0) { throw "cargo build -p psdds failed ($LASTEXITCODE)" }

} finally {
    foreach ($k in $saved.Keys) {
        if ($null -eq $saved[$k]) { Remove-Item "env:$k" -ErrorAction SilentlyContinue } else { Set-Item "env:$k" $saved[$k] }
    }
}

$dir = Join-Path $repo 'target\psdds\x86_64-pc-windows-msvc\release'
$dll = Join-Path $dir 'psdds.dll'
foreach ($f in $dll, (Join-Path $dir 'psdds.lib')) { if (-not (Test-Path $f)) { throw "Build finished but $f is missing." } }

$vswhere = Join-Path ${env:ProgramFiles(x86)} 'Microsoft Visual Studio\Installer\vswhere.exe'
$vs = & $vswhere -latest -products * -requires Microsoft.VisualStudio.Component.VC.Tools.x86.x64 -property installationPath
if (-not $vs) { throw 'No Visual Studio with the C++ x64 tools was found (vswhere).' }
$dumpbin = Get-ChildItem (Join-Path $vs 'VC\Tools\MSVC') -Recurse -Filter dumpbin.exe |
    Where-Object { $_.FullName -match 'Hostx64\\x64' } | Select-Object -Last 1
if (-not $dumpbin) { throw 'dumpbin.exe was not found.' }

$deps = & $dumpbin.FullName /nologo /dependents $dll | Out-String
if ($deps -match 'VCRUNTIME|MSVCP|api-ms-win-crt') { throw "psdds.dll depends on the dynamic CRT:`n$deps" }

# Export names: the "name" column of the exports table, after its header row.
$exp = [regex]::Matches((& $dumpbin.FullName /nologo /exports $dll | Out-String), '(?m)^\s+\d+\s+[0-9A-F]+\s+[0-9A-F]+\s+(\w+)') |
    ForEach-Object { $_.Groups[1].Value } | Sort-Object -Unique
$hdr = [regex]::Matches((Get-Content (Join-Path $repo 'photoshop\psdds.h') -Raw), 'psdds_\w+(?=\()') |
    ForEach-Object { $_.Value } | Sort-Object -Unique
$diff = Compare-Object $hdr $exp
if ($diff) { throw "psdds.dll exports differ from photoshop\psdds.h:`n$($diff | Out-String)" }

Write-Host "Built $dll ($($exp.Count) exports)"

if ($Test) {
    $saved = @{ CARGO_TARGET_DIR = $env:CARGO_TARGET_DIR; RUSTFLAGS = $env:RUSTFLAGS }
    try {
        $env:CARGO_TARGET_DIR = Join-Path $repo 'target\psdds'
        $env:RUSTFLAGS = '-C target-feature=+crt-static'
        cargo run -p psdds --example dll_host --release --offline --manifest-path (Join-Path $repo 'Cargo.toml') -- $dll
        if ($LASTEXITCODE -ne 0) { throw "dll_host failed ($LASTEXITCODE)" }
    } finally {
        foreach ($k in $saved.Keys) { if ($null -eq $saved[$k]) { Remove-Item "env:$k" -ErrorAction SilentlyContinue } else { Set-Item "env:$k" $saved[$k] } }
    }
}
