# Checks the hidden ruling markers in the user-facing docs (docs/DOCUMENTATION.md §9).
# A marker is `<!-- D:<ID>[,<ID>…] -->` after the sentence or table row a ruling decided. It fails, printing
# `file:line ID reason`, for:
#   unknown        an ID in neither DECISIONS file
#   archived       an ID only in DECISIONS-ARCHIVE.md (the sentence is stale)
#   superseded     an ID whose item line in DECISIONS.md says "(superseded by X)"
#   malformed      a marker inside a fenced code block, or with bad syntax
#   placeholder    an `F?` ID on branch main
# An item is `- **<ID>` or `**<ID>` at a line start; a note is `## Note <L>`.
#
#   .\tools\doc-markers.ps1 [-Root <repo>] [-ListUnmarked]   exit 0 clean, 1 any failure
#   .\tools\doc-markers.ps1 -SelfTest                        runs the fixture cases, no repo files
param(
    [string]$Root,
    [switch]$ListUnmarked,
    [switch]$SelfTest
)
$ErrorActionPreference = 'Stop'
if (-not $Root) { $Root = Split-Path $PSScriptRoot -Parent }
$DecisionsPath = '.claude\DECISIONS.md'
$ArchivePath = '.claude\DECISIONS-ARCHIVE.md'
$IdChars = '[A-Z][A-Za-z0-9?-]*'

# The IDs a DECISIONS file defines: items and note letters, each with its "(superseded by X)" when the line has one.
function Get-Ids([string[]]$lines) {
    $ids = @{}
    foreach ($l in $lines) {
        if ($l -cmatch "^## Note ($IdChars)") { $ids[$Matches[1]] = '' ; continue }
        if ($l -cmatch "^(?:- )?\*\*($IdChars)") {
            $id = $Matches[1].TrimEnd('-')
            $sup = if ($l -match '\(superseded by ([^)]+)\)') { $Matches[1] } else { '' }
            $ids[$id] = $sup
        }
    }
    $ids
}

# Every failure in one doc: "<name>:<line> <ID> <reason>".
function Test-Doc([string]$name, [string[]]$lines, [hashtable]$live, [hashtable]$archived, [bool]$onMain, [hashtable]$seen) {
    $out = @()
    $fence = $false
    for ($i = 0; $i -lt $lines.Count; $i++) {
        $l = $lines[$i]
        if ($l -match '^\s*(```|~~~)') { $fence = -not $fence; continue }
        $at = "${name}:$($i + 1)"
        foreach ($m in [regex]::Matches($l, '<!--\s*D:(.*?)-->')) {
            $body = $m.Groups[1].Value
            if ($fence) { $out += "$at $($body.Trim()) malformed: inside a code block"; continue }
            if ($body -cnotmatch "^\s*$IdChars(\s*,\s*$IdChars)*\s*$") {
                $out += "$at $($body.Trim()) malformed"; continue
            }
            foreach ($id in $body -split ',' | ForEach-Object { $_.Trim() } | Where-Object { $_ }) {
                $seen[$id] = $true
                if ($id.Contains('?')) {
                    if ($onMain) { $out += "$at $id placeholder: F? on main" }
                    if (-not $live.ContainsKey($id)) { $out += "$at $id unknown" }
                    continue
                }
                if ($live.ContainsKey($id)) {
                    if ($live[$id]) { $out += "$at $id superseded by $($live[$id])" }
                } elseif ($archived.ContainsKey($id)) {
                    $out += "$at $id archived: the sentence is stale"
                } else {
                    $out += "$at $id unknown"
                }
            }
        }
    }
    $out
}

if ($SelfTest) {
    $dec = @('## Note FC — x', '- **FC5 Enter** never saves.', '- **FC6 Old** rule. (superseded by FC9)',
        '## Note F? — policy', '- **F?P1 Counts.**')
    $arc = @('## Note AG — y', '- **AG11 Dialog keys** Enter = Save.')
    $live = Get-Ids $dec; $arch = Get-Ids $arc
    $cases = @(
        @{ name = 'clean'; doc = @('Enter cancels. <!-- D:FC5 -->', 'Whole note. <!-- D:FC -->'); main = $false; want = @() },
        @{ name = 'unknown'; doc = @('x <!-- D:ZZ9 -->'); main = $false; want = @('t:1 ZZ9 unknown') },
        @{ name = 'archived'; doc = @('x <!-- D:AG11 -->'); main = $false; want = @('t:1 AG11 archived: the sentence is stale') },
        @{ name = 'superseded'; doc = @('x <!-- D:FC5,FC6 -->'); main = $false; want = @('t:1 FC6 superseded by FC9') },
        @{ name = 'fenced'; doc = @('```', 'x <!-- D:FC5 -->', '```'); main = $false; want = @('t:2 FC5 malformed: inside a code block') },
        @{ name = 'bad syntax'; doc = @('x <!-- D:fc5 -->'); main = $false; want = @('t:1 fc5 malformed') },
        @{ name = 'placeholder on branch'; doc = @('x <!-- D:F?P1 -->'); main = $false; want = @() },
        @{ name = 'placeholder on main'; doc = @('x <!-- D:F?P1 -->'); main = $true; want = @('t:1 F?P1 placeholder: F? on main') }
    )
    $fail = 0
    foreach ($c in $cases) {
        $got = @(Test-Doc 't' $c.doc $live $arch $c.main @{})
        if (($got -join '|') -ne ($c.want -join '|')) { "FAIL $($c.name): got [$($got -join '; ')] want [$($c.want -join '; ')]"; $fail++ }
        else { "ok   $($c.name)" }
    }
    "$($cases.Count - $fail) passed, $fail failed"
    exit [int]($fail -gt 0)
}

$live = Get-Ids (Get-Content (Join-Path $Root $DecisionsPath) -Encoding UTF8)
$archived = Get-Ids (Get-Content (Join-Path $Root $ArchivePath) -Encoding UTF8)
$branch = (git -C $Root rev-parse --abbrev-ref HEAD 2>$null)
$onMain = $branch -eq 'main'
$docs = @(Get-ChildItem (Join-Path $Root 'docs\wiki') -Filter *.md) + @(Get-Item (Join-Path $Root 'README.md')) +
    @(Get-ChildItem (Join-Path $Root 'docs') -Filter *.md | Where-Object { $_.Name -notmatch '^(DECISIONS|DECISIONS-ARCHIVE|CHANGELOG|DOCUMENTATION)\.md$' })
$seen = @{}
$fails = @()
foreach ($f in $docs) {
    $rel = $f.FullName.Substring($Root.Length).TrimStart('\') -replace '\\', '/'
    $fails += Test-Doc $rel (Get-Content $f.FullName -Encoding UTF8) $live $archived $onMain $seen
}
$fails
if ($ListUnmarked) {
    'Live rulings with no marker (information only):'
    $live.Keys | Where-Object { -not $live[$_] -and -not $seen.ContainsKey($_) } | Sort-Object | ForEach-Object { "  $_" }
}
"$($seen.Count) marked id(s), $($fails.Count) failure(s)"
exit [int]($fails.Count -gt 0)
