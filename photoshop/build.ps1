# Tessalith DDS: builds the Photoshop format plugin TessalithDDS.8bi. MIT licence (see photoshop/LICENSE).
#
#   .\photoshop\build.ps1                  Release build (static CRT); prints the .8bi path
#   .\photoshop\build.ps1 -Configuration Debug
#   .\photoshop\build.ps1 -Test            build the plugin, then build and run the console tests (host_test loads the .8bi)
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

# 1. The Rust bridge as a static library with the static CRT.
& (Join-Path $repo 'crates\psdds\build.ps1')
$libDir = Join-Path $repo 'target\psdds\x86_64-pc-windows-msvc\release'

$sdk = $env:PS_SDK
if (-not $sdk) {
    $common = (git -C $repo rev-parse --path-format=absolute --git-common-dir).Trim()
    $sdk = Join-Path (Split-Path $common -Parent) 'tools\adobe photoshop sdk 2026'
}
$sdk = $sdk.TrimEnd('\', '/')
if (-not (Test-Path (Join-Path $sdk 'pluginsdk\photoshopapi\photoshop\PIFormat.h'))) {
    throw "Photoshop SDK not found at '$sdk'. Set PS_SDK to the folder that holds pluginsdk\."
}

# 2. The plugin. PiPL.json is embedded as the JSON_PIPL resource by TessalithDDS.rc (the SDK's JSON PiPL form).
$msbuild = & $vswhere -latest -products * -requires Microsoft.Component.MSBuild -find 'MSBuild\**\Bin\MSBuild.exe' | Select-Object -First 1
if (-not $msbuild) { throw 'MSBuild was not found (vswhere).' }
& $msbuild (Join-Path $here 'TessalithDDS.vcxproj') /nologo /v:minimal /p:Configuration=$Configuration /p:Platform=x64 "/p:PsSdk=$sdk" "/p:PsddsLibDir=$libDir"
if ($LASTEXITCODE -ne 0) { throw "MSBuild failed ($LASTEXITCODE)" }

$plugin = Join-Path $out "$Configuration\TessalithDDS.8bi"
if (-not (Test-Path $plugin)) { throw "Build finished but $plugin is missing." }
Write-Host "Built $plugin"

if ($Test) {
    $devCmd = Join-Path $vs 'Common7\Tools\VsDevCmd.bat'
    $libs = "psdds.lib ws2_32.lib userenv.lib ntdll.lib dbghelp.lib user32.lib gdi32.lib"
    $api = Join-Path $sdk 'pluginsdk\photoshopapi'
    $sdkInc = "/DWIN32 /D_WINDOWS /I`"$api\photoshop`" /I`"$api\pica_sp`""
    $tests = @(
        @{ exe = 'pixels_test.exe'; src = "`"$here\tests\pixels_test.cpp`" `"$here\src\pixels.cpp`""; link = "/link /LIBPATH:`"$libDir`" $libs" },
        @{ exe = 'dialog_test.exe'; src = "`"$here\tests\dialog_test.cpp`" `"$here\src\dialog.cpp`" `"$here\src\presets.cpp`""; link = "/link /LIBPATH:`"$libDir`" $libs" },
        @{ exe = 'host_test.exe'; src = "$sdkInc `"$here\tests\host_test.cpp`""; link = '/link user32.lib'; args = @($plugin) }
    )
    $failed = 0
    foreach ($t in $tests) {
        $exe = Join-Path $out $t.exe
        $cl = "cl /nologo /EHsc /MT /W4 /WX /DNOMINMAX /Fe`"$exe`" $($t.src) $($t.link)"
        & $env:ComSpec /c "`"$devCmd`" -arch=x64 -no_logo && cd /d `"$out`" && $cl"
        if ($LASTEXITCODE -ne 0) { Write-Host "$($t.exe): FAILED to build"; $failed++; continue }
        & $exe @($t.args)
        if ($LASTEXITCODE -ne 0) { $failed++ }
    }
    exit [int]($failed -ne 0)
}
