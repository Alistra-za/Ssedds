# Tessalith DDS: builds the Photoshop format plugin TessalithDDS.8bi. MIT licence (see photoshop/LICENSE).
#
#   .\photoshop\build.ps1                  Release build (static CRT); prints the .8bi path
#   .\photoshop\build.ps1 -Configuration Debug
#   .\photoshop\build.ps1 -Test            build and run the pixel tests only
#
# The Photoshop SDK is read from $env:PS_SDK, or from "tools\adobe photoshop sdk 2026" in the repo's main checkout.
# Build outputs go to photoshop\build\ (ignored) and the Rust library to target\psdds\.
param(
    [ValidateSet('Release', 'Debug')][string]$Configuration = 'Release',
    [switch]$Test
)
$ErrorActionPreference = 'Stop'
$here = $PSScriptRoot
$repo = Split-Path $here -Parent
$out = Join-Path $here 'build'
New-Item -ItemType Directory -Force $out | Out-Null

$vswhere = Join-Path ${env:ProgramFiles(x86)} 'Microsoft Visual Studio\Installer\vswhere.exe'
$vs = & $vswhere -latest -products * -requires Microsoft.VisualStudio.Component.VC.Tools.x86.x64 -property installationPath
if (-not $vs) { throw 'No Visual Studio with the C++ x64 tools was found (vswhere).' }
# VsDevCmd.bat calls vswhere and System32 tools by name.
$env:PATH = "$env:SystemRoot\System32;$(Split-Path $vswhere);$env:PATH"

if ($Test) {
    $devCmd = Join-Path $vs 'Common7\Tools\VsDevCmd.bat'
    $exe = Join-Path $out 'pixels_test.exe'
    $cl = "cl /nologo /EHsc /MT /W4 /WX /Fe`"$exe`" `"$here\tests\pixels_test.cpp`" `"$here\src\pixels.cpp`""
    & $env:ComSpec /c "`"$devCmd`" -arch=x64 -no_logo && cd /d `"$out`" && $cl"
    if ($LASTEXITCODE -ne 0) { Write-Host 'pixels_test: FAILED to build'; exit 1 }
    & $exe
    exit $LASTEXITCODE
}

$sdk = $env:PS_SDK
if (-not $sdk) {
    $common = (git -C $repo rev-parse --path-format=absolute --git-common-dir).Trim()
    $sdk = Join-Path (Split-Path $common -Parent) 'tools\adobe photoshop sdk 2026'
}
$sdk = $sdk.TrimEnd('\', '/')
if (-not (Test-Path (Join-Path $sdk 'pluginsdk\photoshopapi\photoshop\PIFormat.h'))) {
    throw "Photoshop SDK not found at '$sdk'. Set PS_SDK to the folder that holds pluginsdk\."
}

# 1. The Rust bridge as a static library with the static CRT.
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
$libDir = Join-Path $repo 'target\psdds\x86_64-pc-windows-msvc\release'

# 2. The plugin. PiPL.json is embedded as the JSON_PIPL resource by TessalithDDS.rc (the SDK's JSON PiPL form).
$msbuild = & $vswhere -latest -products * -requires Microsoft.Component.MSBuild -find 'MSBuild\**\Bin\MSBuild.exe' | Select-Object -First 1
if (-not $msbuild) { throw 'MSBuild was not found (vswhere).' }
& $msbuild (Join-Path $here 'TessalithDDS.vcxproj') /nologo /v:minimal /p:Configuration=$Configuration /p:Platform=x64 "/p:PsSdk=$sdk" "/p:PsddsLibDir=$libDir"
if ($LASTEXITCODE -ne 0) { throw "MSBuild failed ($LASTEXITCODE)" }

$plugin = Join-Path $out "$Configuration\TessalithDDS.8bi"
if (-not (Test-Path $plugin)) { throw "Build finished but $plugin is missing." }
Write-Host "Built $plugin"
