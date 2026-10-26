# Publishes docs/wiki/*.md to the GitHub wiki (a separate repo, <repo>.wiki.git), rewriting links for the wiki:
#   [x](Page.md#a)          -> [x](Page#a)            (wiki pages have no .md)
#   [x](../research/y.md)   -> https://github.com/<owner>/<repo>/blob/<ref>/docs/research/y.md
#   ![x](../img/y.svg)      -> https://github.com/<owner>/<repo>/blob/<ref>/docs/img/y.svg?raw=true
# docs/wiki keeps .md links so the pages also read correctly when browsing the repo.
#
#   .\tools\publish-wiki.ps1 -Out <folder>   write the converted pages to a folder only (check them first)
#   .\tools\publish-wiki.ps1                 clone the wiki repo, replace the pages that docs/wiki has, commit, push
#
# The wiki repo exists only after its first page was saved once on github.com (Wiki > Create the first page).
# Pages in the wiki that docs/wiki does not have are left alone.
param(
    [string]$Repo = 'Alistra-za/Tessalith',
    [string]$Ref = 'main',
    [string]$Out
)
$ErrorActionPreference = 'Stop'
$root = Split-Path $PSScriptRoot -Parent
$src = Join-Path $root 'docs\wiki'
$pages = Get-ChildItem $src -Filter *.md -File
$names = @($pages | ForEach-Object { $_.BaseName })
$base = "https://github.com/$Repo/blob/$Ref"

function Convert-Page([string]$text) {
    [regex]::Replace($text, '(!?)\[([^\]]*)\]\(([^)\s]+)\)', {
        param($m)
        $bang, $label, $target = $m.Groups[1].Value, $m.Groups[2].Value, $m.Groups[3].Value
        if ($target -match '^(https?:|mailto:|#)') { return $m.Value }
        $path, $anchor = $target -split '#', 2
        $anchor = if ($anchor) { "#$anchor" } else { '' }
        $page = [IO.Path]::GetFileNameWithoutExtension($path)
        if ($path -notmatch '[\\/]' -and $path -match '\.md$' -and $names -contains $page) { return "$bang[$label]($page$anchor)" }
        # anything else is a repo file relative to docs/wiki
        $full = [IO.Path]::GetFullPath((Join-Path $src $path))
        if (-not (Test-Path $full)) { throw "broken link in docs/wiki: $target" }
        $rel = $full.Substring($root.Length + 1) -replace '\\', '/'
        $url = "$base/$rel" + $(if ($bang) { '?raw=true' } else { '' })
        "$bang[$label]($url$anchor)"
    })
}

function Write-Pages([string]$dir) {
    foreach ($p in $pages) {
        $text = Convert-Page (Get-Content $p.FullName -Raw)
        [IO.File]::WriteAllText((Join-Path $dir $p.Name), $text, [Text.UTF8Encoding]::new($false))
    }
    "Converted $($pages.Count) pages into $dir"
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
