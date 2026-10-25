# Installs (or removes) the Tessalith DDS nodes inside chaiNNer's own app folder (nodes: GPL-3.0-or-later).
#
#   .\chainner\install.ps1                        newest %LOCALAPPDATA%\chaiNNer\app-*
#   .\chainner\install.ps1 -Target <src folder>   a given resources\src folder (testing, portable copies)
#   .\chainner\install.ps1 -Uninstall             removes only the tessalith_dds folder
#
# Re-run after every chaiNNer update: an update installs a new app-<version> folder and deletes the old one.
param(
    [string]$ChaiNNerRoot = (Join-Path $env:LOCALAPPDATA 'chaiNNer'),
    [string]$Target,   # a resources\src folder; skips the app-* search and the running check
    [string]$Dll,
    [switch]$Uninstall
)
$ErrorActionPreference = 'Stop'
$here = $PSScriptRoot
$repo = Split-Path $here -Parent
if (-not $Dll) { $Dll = Join-Path $repo 'target\psdds\x86_64-pc-windows-msvc\release\psdds.dll' }

if ($Target) {
    $src = $ExecutionContext.SessionState.Path.GetUnresolvedProviderPathFromPSPath($Target)
} else {
    if (Get-Process chaiNNer -ErrorAction SilentlyContinue) {
        throw 'chaiNNer is running (its worker holds files open). Close chaiNNer, then run this again.'
    }
    $app = Get-ChildItem $ChaiNNerRoot -Directory -Filter 'app-*' -ErrorAction SilentlyContinue |
        Sort-Object { try { [version]($_.Name -replace '^app-', '') } catch { [version]'0.0' } } | Select-Object -Last 1
    if (-not $app) { throw "no app-* folder under $ChaiNNerRoot; is chaiNNer installed there?" }
    $src = Join-Path $app.FullName 'resources\src'
}
$pkg = Join-Path $src 'packages\chaiNNer_standard'
if (-not (Test-Path $pkg)) { throw "not a chaiNNer src folder (missing $pkg)" }
$dest = Join-Path $pkg 'tessalith_dds'

if ($Uninstall) {
    if (Test-Path $dest) { Remove-Item $dest -Recurse -Force; "Removed $dest" } else { "Nothing to remove at $dest" }
    return
}

if (-not (Test-Path $Dll)) {
    throw "psdds.dll not found at $Dll. Build it with crates\psdds\build.ps1 (or pass -Dll)."
}

$tmp = Join-Path ([IO.Path]::GetTempPath()) ("tessalith-install-" + [guid]::NewGuid().ToString('N'))
New-Item -ItemType Directory $tmp | Out-Null
try {
    # Generate first, so a failed licence check leaves any existing install untouched.
    $notices = Join-Path $tmp 'THIRD_PARTY_NOTICES.md'
    & (Join-Path $repo 'crates\psdds\notices.ps1') -Binary psdds.dll -Product 'Tessalith psdds.dll' -Out $notices
    if (-not (Test-Path $notices)) { throw 'notices.ps1 wrote no THIRD_PARTY_NOTICES.md' }

    if (Test-Path $dest) { Remove-Item $dest -Recurse -Force }
    New-Item -ItemType Directory $dest | Out-Null
    Get-ChildItem (Join-Path $here 'tessalith_dds') -File -Filter '*.py' | Copy-Item -Destination $dest
    Copy-Item $Dll (Join-Path $dest 'psdds.dll')
    Copy-Item $notices (Join-Path $dest 'THIRD_PARTY_NOTICES.md')
    Copy-Item (Join-Path $here 'LICENSE') (Join-Path $dest 'LICENSE')
    Copy-Item (Join-Path $here 'LICENSE-psdds.txt') (Join-Path $dest 'LICENSE-psdds.txt')

    $py = Join-Path $env:APPDATA 'chaiNNer\python\python\python.exe'
    "Installed to $dest"
    if (Test-Path $py) {
        $out = & $py -I (Join-Path $dest '_psdds.py') 2>&1   # prints "<psdds version> <ABI>"
        if ($LASTEXITCODE -ne 0) { throw "psdds.dll failed to load: $out" }
        "psdds version and ABI: $out"
    } else {
        "chaiNNer's bundled Python not found at $py; version not checked."
    }
    'Re-run after every chaiNNer update; updates replace the app folder.'
} finally {
    Remove-Item $tmp -Recurse -Force -ErrorAction SilentlyContinue
}
