# Tessalith DDS: builds the Paint.NET file type plugin TessalithDds.dll.
#
#   .\paintdotnet\build.ps1                 Release build: psdds.dll (shared script), then the plugin; prints the output folder
#   .\paintdotnet\build.ps1 -Configuration Debug
#   .\paintdotnet\build.ps1 -Test           also runs the xunit tests (output in paintdotnet\tests\last-test.log)
#
# Paint.NET's DLLs are read from $env:PDN_DIR or C:\Program Files\paint.net at build time only; none is copied.
param(
    [ValidateSet('Release', 'Debug')][string]$Configuration = 'Release',
    [switch]$Test
)
$ErrorActionPreference = 'Stop'
$here = $PSScriptRoot
$repo = Split-Path $here -Parent

& (Join-Path $repo 'crates\psdds\build.ps1')
$psdds = Join-Path $repo 'target\psdds\x86_64-pc-windows-msvc\release\psdds.dll'
if (-not (Test-Path $psdds)) { throw "psdds.dll is missing at $psdds" }

$m = [regex]::Match((Get-Content (Join-Path $repo 'Cargo.toml') -Raw), '(?ms)^\[workspace\.package\].*?^version\s*=\s*"([^"]+)"')
if (-not $m.Success) { throw 'No workspace version in Cargo.toml.' }
$version = $m.Groups[1].Value

dotnet build (Join-Path $here 'TessalithDds.csproj') -c $Configuration "-p:Version=$version" --nologo -v minimal
if ($LASTEXITCODE -ne 0) { throw "dotnet build failed ($LASTEXITCODE)" }

$out = Join-Path $here "bin\$Configuration\net9.0-windows"
Copy-Item $psdds $out -Force
Write-Host "Built $out\TessalithDds.dll"

if ($Test) {
    $log = Join-Path $here 'tests\last-test.log'
    dotnet test (Join-Path $here 'tests\TessalithDds.Tests.csproj') -c $Configuration "-p:PsddsDll=$psdds" --logger 'console;verbosity=normal' *> $log
    $code = $LASTEXITCODE
    Select-String -Path $log -Pattern '^\s*(Passed!|Failed!|Passed|Failed)\b.*(Total|:)' | Select-Object -Last 3 | ForEach-Object { Write-Host $_.Line }
    if ($code -ne 0) { throw "dotnet test failed ($code); see $log" }
}