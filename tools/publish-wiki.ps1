# Publishes docs/wiki/*.md to the GitHub wiki (a separate repo, <repo>.wiki.git), rewriting links for the wiki:
#   [x](Page.md#a)          -> [x](Page#a)            (wiki pages have no .md)
#   [x](../research/y.md)   -> https://github.com/<owner>/<repo>/blob/<ref>/docs/research/y.md
#   ![x](../img/y.svg)      -> https://github.com/<owner>/<repo>/blob/<ref>/docs/img/y.svg?raw=true
# docs/wiki keeps .md links so the pages also read correctly when browsing the repo.
# Every link must resolve: a missing file, or a #anchor with no matching heading on a wiki page, stops the run and
# lists every broken link. Anchors follow GitHub's heading slugs (see Get-Slugs).
#
#   .\tools\publish-wiki.ps1 -Out <folder>   write the converted pages to a folder only (check them first)
#   .\tools\publish-wiki.ps1 -Check <file>   check the links of one repo file (README.md, docs\DOCUMENTATION.md); writes nothing
#   .\tools\publish-wiki.ps1 -SelfTest       check the slug rules
#   .\tools\publish-wiki.ps1                 clone the wiki repo, replace the pages that docs/wiki has, commit, push
#   .\tools\publish-wiki.ps1 -Prune          list the wiki pages docs/wiki does not have (never Home or _Sidebar), then stop
#   .\tools\publish-wiki.ps1 -Prune -Yes     delete those pages, then publish
#
# The wiki repo exists only after its first page was saved once on github.com (Wiki > Create the first page).
# Without -Prune, pages in the wiki that docs/wiki does not have are left alone.
param(
    [string]$Repo = 'Alistra-za/Tessalith',
    [string]$Ref = 'main',
    [string]$Out,
    [string]$Check,
    [switch]$SelfTest,
    [switch]$Prune,
    [switch]$Yes
)
$ErrorActionPreference = 'Stop'
$root = Split-Path $PSScriptRoot -Parent
$src = Join-Path $root 'docs\wiki'
$pages = Get-ChildItem $src -Filter *.md -File
$names = @($pages | ForEach-Object { $_.BaseName })
$base = "https://github.com/$Repo/blob/$Ref"
$slugCache = @{}

# GitHub's heading slugs: markdown removed (link targets, backticks, *), lower-cased, every character but Unicode
# letters, marks, digits, _, - and space deleted, each space a hyphen (runs not collapsed); repeats get -1, -2.
function Get-Slugs([string]$text) {
    $seen = @{}
    $slugs = [System.Collections.Generic.HashSet[string]]::new()
    $fence = $false
    foreach ($line in $text -split "`r?`n") {
        if ($line -match '^\s*(```|~~~)') { $fence = -not $fence; continue }
        if ($fence) { continue }
        foreach ($m in [regex]::Matches($line, '<a\s+(?:name|id)="([^"]+)"')) { [void]$slugs.Add($m.Groups[1].Value) }
        if ($line -notmatch '^#{1,6} (.*)$') { continue }
        $h = $Matches[1].Trim() -replace '\s+#+\s*$', ''
        $h = [regex]::Replace($h, '!?\[([^\]]*)\]\([^)]*\)', '$1')
        $h = $h -replace '[`*]', ''
        $s = [regex]::Replace($h.ToLowerInvariant(), '[^\p{L}\p{M}\p{N}_\- ]', '') -replace ' ', '-'
        if ($seen.ContainsKey($s)) { $seen[$s]++; $s = "$s-$($seen[$s])" } else { $seen[$s] = 0 }
        [void]$slugs.Add($s)
    }
    , $slugs
}

function Get-PageSlugs([string]$file) {
    $full = [IO.Path]::GetFullPath($file)
    if (-not $slugCache.ContainsKey($full)) { $slugCache[$full] = Get-Slugs (Get-Content $full -Raw) }
    $slugCache[$full]
}

# Rewrites one page's links; appends every broken link to $errors. $self is the page's own file.
function Convert-Page([string]$text, [string]$self, [System.Collections.Generic.List[string]]$errors) {
    $where = $self.Substring($root.Length + 1) -replace '\\', '/'
    [regex]::Replace($text, '(!?)\[([^\]]*)\]\(([^)\s]+)\)', {
        param($m)
        $bang, $label, $target = $m.Groups[1].Value, $m.Groups[2].Value, $m.Groups[3].Value
        if ($target -match '^(https?:|mailto:)') { return $m.Value }
        $path, $anchor = $target -split '#', 2
        if (-not $path) {
            if ($anchor -and -not (Get-PageSlugs $self).Contains($anchor)) { $errors.Add("broken anchor in ${where}: #$anchor") }
            return $m.Value
        }
        $hash = if ($anchor) { "#$anchor" } else { '' }
        $page = [IO.Path]::GetFileNameWithoutExtension($path)
        $full = [IO.Path]::GetFullPath((Join-Path (Split-Path $self -Parent) $path))
        if (-not (Test-Path $full)) { $errors.Add("broken link in ${where}: $target"); return $m.Value }
        $inWiki = (Split-Path $full -Parent) -eq $src -and $full -match '\.md$'
        if ($anchor -and $inWiki -and -not (Get-PageSlugs $full).Contains($anchor)) {
            $errors.Add("broken anchor in ${where}: $page.md#$anchor")
        }
        if ((Split-Path $self -Parent) -eq $src -and $path -notmatch '[\\/]' -and $inWiki -and $names -contains $page) {
            return "$bang[$label]($page$hash)"
        }
        # anything else is a repo file relative to the page
        $rel = $full.Substring($root.Length + 1) -replace '\\', '/'
        $url = "$base/$rel" + $(if ($bang) { '?raw=true' } else { '' })
        "$bang[$label]($url$hash)"
    })
}

function Write-Pages([string]$dir) {
    $errors = [System.Collections.Generic.List[string]]::new()
    $out = @{}
    foreach ($p in $pages) { $out[$p.Name] = Convert-Page (Get-Content $p.FullName -Raw) $p.FullName $errors }
    if ($errors.Count) { throw ($errors -join "`n") }
    foreach ($n in $out.Keys) { [IO.File]::WriteAllText((Join-Path $dir $n), $out[$n], [Text.UTF8Encoding]::new($false)) }
    "Converted $($pages.Count) pages into $dir"
}

if ($SelfTest) {
    $f = '```'
    $cases = @(
        @('## Export & review', 'export--review'),
        @('## `on_existing` key', 'on_existing-key'),
        @('## Café', 'café'),
        @("## A`n## A", 'a-1'),
        @("$f`n## Not a heading`n$f`n## [Link](X.md) *x*", 'link-x')
    )
    $fail = 0
    foreach ($c in $cases) {
        $s = Get-Slugs $c[0]
        if (-not $s.Contains($c[1])) { $fail++; "FAIL: $($c[0] -replace "`n", ' / ') -> $($s -join ', ') (want $($c[1]))" }
    }
    if ((Get-Slugs "$f`n## Not a heading`n$f").Count) { $fail++; 'FAIL: a heading inside a fence counted' }
    if ($fail) { throw "self-test: $fail failed" }
    "self-test: $($cases.Count + 1) passed"
    return
}

if ($Check) {
    $file = [IO.Path]::GetFullPath((Join-Path (Get-Location) $Check))
    $errors = [System.Collections.Generic.List[string]]::new()
    [void](Convert-Page (Get-Content $file -Raw) $file $errors)
    if ($errors.Count) { throw ($errors -join "`n") }
    "Links in $Check resolve"
    return
}

if ($Out) {
    New-Item -ItemType Directory -Force $Out | Out-Null
    Write-Pages $Out
    return
}

$work = Join-Path ([IO.Path]::GetTempPath()) ("tessalith-wiki-" + [guid]::NewGuid().ToString('N'))
try {
    git clone --quiet "https://github.com/$Repo.wiki.git" $work
    if ($LASTEXITCODE -ne 0) { throw "could not clone https://github.com/$Repo.wiki.git (create the wiki's first page on github.com once)" }
    if ($Prune) {
        $extra = @(Get-ChildItem $work -Filter *.md -File | Where-Object { $names -notcontains $_.BaseName -and $_.Name -notin 'Home.md', '_Sidebar.md' })
        if (-not $Yes) {
            if ($extra.Count) { "Wiki pages docs/wiki does not have (re-run with -Prune -Yes to delete them):"; $extra | ForEach-Object { "  $($_.Name)" } }
            else { 'No wiki page is missing from docs/wiki.' }
            return
        }
        foreach ($e in $extra) { git -C $work rm --quiet -- $e.Name; "Deleting $($e.Name)" }
    }
    Write-Pages $work
    $head = (git -C $root rev-parse --short HEAD).Trim()
    git -C $work add -A
    if (-not (git -C $work status --porcelain)) { 'Wiki already up to date.'; return }
    git -C $work commit --quiet -m "Sync from $Repo docs/wiki at $head"
    git -C $work push --quiet origin HEAD
    if ($LASTEXITCODE -ne 0) { throw 'wiki push failed' }
    "Published to https://github.com/$Repo/wiki"
} finally {
    Remove-Item $work -Recurse -Force -ErrorAction SilentlyContinue
}
