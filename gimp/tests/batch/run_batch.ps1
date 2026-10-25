# SPDX-License-Identifier: GPL-3.0-or-later
# Runs gimp\tests\gimp_batch.py in headless gimp-console with a throwaway GIMP profile.
# Never touches %APPDATA%\GIMP or the GIMP install. Called by gimp\tests\run.ps1 -Gimp.
$ErrorActionPreference = 'Stop'
$repo = Split-Path (Split-Path (Split-Path $PSScriptRoot -Parent) -Parent) -Parent
$gimpDir = if ($env:SDC_GIMP_DIR) { $env:SDC_GIMP_DIR } else { 'C:\Program Files\GIMP 3' }
$console = Join-Path $gimpDir 'bin\gimp-console-3.0.exe'
$dll = if ($env:TESSALITH_PSDDS_DLL) { $env:TESSALITH_PSDDS_DLL } else { Join-Path $repo 'target\psdds\x86_64-pc-windows-msvc\release\psdds.dll' }
if (-not (Test-Path $console)) { [Console]::Error.WriteLine("gimp-console not found at ${console}: set SDC_GIMP_DIR"); exit 1 }
if (-not (Test-Path $dll)) { [Console]::Error.WriteLine("psdds.dll not found at ${dll}: run crates\psdds\build.ps1"); exit 1 }

$tmp = Join-Path ([IO.Path]::GetTempPath()) ("tdds-gimp-" + [Guid]::NewGuid().ToString('N'))
$plug = Join-Path $tmp 'plug-ins\tessalith-dds'
$out = Join-Path $tmp 'out'
New-Item -ItemType Directory -Force $plug, $out | Out-Null
$code = 1
$saved = @{}
foreach ($k in 'GIMP3_DIRECTORY', 'GIMP3_CACHEDIR', 'TDDS_OUT', 'TDDS_REPO', 'TESSALITH_PSDDS_DLL') { $saved[$k] = [Environment]::GetEnvironmentVariable($k) }
try {
    Copy-Item (Join-Path $repo 'gimp\tessalith-dds\*.py') $plug
    Copy-Item $dll (Join-Path $plug 'psdds.dll')
    $env:GIMP3_DIRECTORY = $tmp
    $env:GIMP3_CACHEDIR = Join-Path $tmp 'cache'  # pluginrc lives in the cache dir, not the profile
    $env:TDDS_OUT = $out
    $env:TDDS_REPO = $repo
    $env:TESSALITH_PSDDS_DLL = Join-Path $plug 'psdds.dll'
    $script = Join-Path $repo 'gimp\tests\gimp_batch.py'
    $stdout = Join-Path $tmp 'stdout.txt'
    $stderr = Join-Path $tmp 'stderr.txt'
    $argLine = "-i --batch-interpreter=python-fu-eval -b `"exec(open(r'$script').read())`" --quit"
    $p = Start-Process -FilePath $console -ArgumentList $argLine -NoNewWindow -PassThru `
        -RedirectStandardOutput $stdout -RedirectStandardError $stderr
    $null = $p.Handle  # keeps the exit code readable after exit
    if (-not $p.WaitForExit(240000)) {
        & taskkill.exe /T /F /PID $p.Id | Out-Null
        throw 'gimp-console did not finish in 240 s; killed it and its plug-ins'
    }
    $log = (Get-Content -Raw $stdout) + "`n" + (Get-Content -Raw $stderr)
    $log
    $code = 0
    if ($p.ExitCode -ne 0) { [Console]::Error.WriteLine("gimp-console exited $($p.ExitCode) (64 = the batch script raised)"); $code = 1 }
    if ((Get-Content -Raw $stderr) -match 'crashed') { [Console]::Error.WriteLine('GIMP stderr says a plug-in crashed (DC25)'); $code = 1 }
    $resFile = Join-Path $out 'results.json'
    if (-not (Test-Path $resFile)) { throw 'the batch script wrote no results.json' }
    $results = Get-Content -Raw $resFile | ConvertFrom-Json
    $expect = Get-Content -Raw (Join-Path $out 'expect_log.json') | ConvertFrom-Json
    foreach ($r in $results.PSObject.Properties) {
        $v = $r.Value
        if ($v -eq 'pass') {
            foreach ($text in @($expect.($r.Name))) {
                if ($text -and -not $log.Contains($text)) { $v = "GIMP log lacks: $text" }
            }
        }
        if ($v -eq 'pass') { "PASS $($r.Name)" }
        elseif ($v -like 'skip:*') { "SKIP $($r.Name): $v" }
        else { "FAIL $($r.Name): $v"; $code = 1 }
    }
} catch {
    [Console]::Error.WriteLine("$_")
    $code = 1
} finally {
    foreach ($k in $saved.Keys) { [Environment]::SetEnvironmentVariable($k, $saved[$k]) }
    foreach ($i in 1..5) {
        try { Remove-Item -Recurse -Force $tmp -ErrorAction Stop; break } catch { Start-Sleep -Milliseconds 500 }
    }
    if (Test-Path $tmp) { [Console]::Error.WriteLine("could not delete $tmp") }
}
exit $code
