# SPDX-License-Identifier: GPL-3.0-or-later
# Runs the GIMP plug-in tests with GIMP's own Python. -Gimp also runs the batch suite (Task 3).
param([switch]$Gimp)
$ErrorActionPreference = 'Stop'
$repo = Split-Path (Split-Path $PSScriptRoot -Parent) -Parent
$gimpDir = if ($env:SDC_GIMP_DIR) { $env:SDC_GIMP_DIR } else { 'C:\Program Files\GIMP 3' }
$py = Join-Path $gimpDir 'bin\python.exe'
if (-not $env:TESSALITH_PSDDS_DLL) { $env:TESSALITH_PSDDS_DLL = Join-Path $repo 'target\psdds\x86_64-pc-windows-msvc\release\psdds.dll' }
$missing = @()
if (-not (Test-Path $env:TESSALITH_PSDDS_DLL)) { $missing += "psdds.dll not found at $($env:TESSALITH_PSDDS_DLL): run crates\psdds\build.ps1" }
if (-not (Test-Path $py)) { $missing += "GIMP's Python not found at ${py}: set SDC_GIMP_DIR" }
if ($missing) { [Console]::Error.WriteLine(($missing -join '; ')); exit 1 }
Push-Location $repo
try {
    $lines = & $py -I -m unittest discover -s gimp\tests -p 'test_*.py' 2>&1 | ForEach-Object { "$_" }
    $code = $LASTEXITCODE
    $lines
    if ($code -eq 0 -and ($lines -match '^Ran 0 tests')) { [Console]::Error.WriteLine('no tests were discovered'); $code = 1 }
    if ($Gimp -and $code -eq 0) {
        $batch = 'gimp\tests\batch\run_batch.ps1'
        if (-not (Test-Path $batch)) { [Console]::Error.WriteLine("$batch does not exist yet (Task 3)"); $code = 1 } else { & $batch; $code = $LASTEXITCODE }
    }
} finally { Pop-Location }
exit $code
