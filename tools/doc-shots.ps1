# Refreshes the documentation screenshots (docs/DOCUMENTATION.md §5) before a push:
#   1. Render: runs crates/gui/tests/doc_shots.rs render_doc_shots into a new temp folder (-Out), one PNG per
#      [[shot]] of docs/img/gui/shots.toml (or only the ids in -Only).
#   2. Compare: each render against the committed PNG; a pixel changes when a channel moves by more than 8, a shot
#      when more than 0.1 % of its pixels change.
#   3. Overlay: a changed shot gets <id>.diff.png (old image dimmed, changes magenta); summary.md lists every shot.
#   4. Look: open each overlay. Intended: accept it (below) and re-read the pages in its "used by" column.
#      Not intended: it is a GUI bug; report it.
#   5. Accept: -Accept <id,id> -From <that Out> copies only the named, changed shots over the committed PNGs.
#      Unchanged shots keep their committed bytes.
#
#   .\tools\doc-shots.ps1 [-Out <dir>] [-Only <id,id>]     exit 0 all unchanged, 1 any changed/new/size, 2 render failed
#   .\tools\doc-shots.ps1 -Accept <id,id> -From <dir>
param(
    [string]$Out,
    [string]$Only,
    [string]$Accept,
    [string]$From
)
$ErrorActionPreference = 'Stop'
$root = Split-Path $PSScriptRoot -Parent

function Read-Summary([string]$dir) {
    $tsv = Join-Path $dir 'summary.tsv'
    if (-not (Test-Path $tsv)) { throw "no summary.tsv in $dir" }
    @(Import-Csv $tsv -Delimiter "`t")
}

if ($Accept) {
    if (-not $From) { throw '-Accept needs -From <the render folder>' }
    $rows = Read-Summary $From
    foreach ($id in $Accept -split ',' | ForEach-Object { $_.Trim() } | Where-Object { $_ }) {
        $row = $rows | Where-Object { $_.id -eq $id } | Select-Object -First 1
        if (-not $row) { "${id}: not in $From\summary.tsv"; continue }
        if ($row.status -eq 'unchanged') { "${id}: unchanged: committed bytes kept"; continue }
        if ($row.status -notin 'changed', 'size', 'new') { "${id}: $($row.status), not accepted"; continue }
        $dst = Join-Path $root "docs\img\gui\$($row.page)\$id.png"
        New-Item -ItemType Directory -Force (Split-Path $dst -Parent) | Out-Null
        Copy-Item (Join-Path $From "$($row.page)\$id.png") $dst -Force
        "${id}: accepted ($($row.status)) -> docs/img/gui/$($row.page)/$id.png"
    }
    return
}

if (-not $Out) { $Out = Join-Path $env:TEMP ("doc-shots-" + (Get-Date -Format 'yyyyMMdd-HHmmss')) }
New-Item -ItemType Directory -Force $Out | Out-Null
$env:CARGO_BUILD_JOBS = '4'
$env:DOC_SHOTS_OUT = $Out
if ($Only) { $env:DOC_SHOTS_ONLY = $Only } else { Remove-Item Env:DOC_SHOTS_ONLY -ErrorAction SilentlyContinue }
$log = Join-Path $Out 'test.log'
Push-Location $root
try {
    cargo test --offline -p sdc-gui --test doc_shots render_doc_shots -- --ignored --exact --test-threads=1 *> $log
} finally { Pop-Location }
"Out: $Out"
if (Test-Path (Join-Path $Out 'summary.md')) { Get-Content (Join-Path $Out 'summary.md') }
$ok = Select-String -Path $log -Pattern 'test result: ok' -Quiet
if (-not $ok -or -not (Test-Path (Join-Path $Out 'summary.tsv'))) { "render failed, see $log"; exit 2 }
$rows = Read-Summary $Out
if ($rows | Where-Object status -eq 'failed') { "render failed, see $log"; exit 2 }
$changed = @($rows | Where-Object status -ne 'unchanged')
foreach ($r in $changed) {
    $d = Join-Path $Out "$($r.page)\$($r.id).diff.png"
    if (Test-Path $d) { "overlay: $d" }
}
if ($changed.Count) { exit 1 }
exit 0
