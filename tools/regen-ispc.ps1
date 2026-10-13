# Rebuilds the ISPC objects with ISPC 1.31.0 into a temp dir and compares SHA-256 with the vendored objects.
# -Update copies the new objects and main headers into vendor/ispc-objs (then run tools/hash-vendor.ps1).
#
# Reproducibility: ISPC embeds the source path it was given, so it is always run with the working
# directory at the repo root and a repo-relative source path, with the target pinned explicitly
# (--arch=x86-64 --target-os=windows). The generated header's line 2 (the output path, which is a
# temp dir) is normalised to the bare file name before hashing and vendoring.
param(
  [string]$Ispc = "ispc.exe",
  [string]$Bc7eSource = "$PSScriptRoot\..\crates\encoders-sys\vendor\bc7e\bc7e.ispc",
  [switch]$Update,
  # with -Update: replace only the bc7e objects (leave the ISPC TC objects untouched)
  [switch]$Bc7eOnly
)
$ErrorActionPreference = "Stop"
$repo = (Resolve-Path "$PSScriptRoot\..").Path
$vendor = Join-Path $repo "crates\encoders-sys\vendor"
if (-not (Get-Command $Ispc -ErrorAction SilentlyContinue)) { throw "ISPC 1.31.0 not found as '$Ispc'; pass -Ispc <path to ispc.exe>" }
$ver = & $Ispc --version | Out-String
if ($ver -notmatch "1\.31\.0") { throw "ISPC 1.31.0 required; got: $ver" }

# Repo-relative, forward-slash source path (absolute only if the source lives outside the repo).
function Get-SourceArg([string]$path) {
  $full = (Resolve-Path $path).Path
  if ($full.StartsWith($repo + "\", [StringComparison]::OrdinalIgnoreCase)) { $full = $full.Substring($repo.Length + 1) }
  return $full.Replace([string][char]92, "/")
}

# Rewrites header line 2 ("// <output path>") to "// <file name>" so the header does not depend on the temp dir.
function Normalize-Header([string]$path) {
  $lines = [IO.File]::ReadAllLines($path)
  if ($lines.Length -gt 1 -and $lines[1] -match '^// .*[/\\]') { $lines[1] = "// " + [IO.Path]::GetFileName($path) }
  [IO.File]::WriteAllLines($path, $lines, (New-Object Text.UTF8Encoding($false)))
}

$tmp = Join-Path ([IO.Path]::GetTempPath()) ("sdc-ispc-" + [guid]::NewGuid())
New-Item -ItemType Directory "$tmp\bc7e", "$tmp\ispctc" | Out-Null
Push-Location $repo
try {
  $common = @("-O2", "--arch=x86-64", "--target-os=windows", "--target=sse2,sse4,avx,avx2", "--opt=fast-math")
  # ISPC prints hundreds of compiler warnings for these two vendored sources; show them only on failure.
  $log = & $Ispc @common "--opt=disable-assertions" (Get-SourceArg $Bc7eSource) -o "$tmp\bc7e\bc7e.obj" -h "$tmp\bc7e\bc7e_ispc.h" 2>&1
  if ($LASTEXITCODE) { $log; throw "ispc failed on $Bc7eSource" }
  $log = & $Ispc @common (Get-SourceArg "$vendor\ispctc\kernel.ispc") -o "$tmp\ispctc\kernel.obj" -h "$tmp\ispctc\kernel_ispc.h" 2>&1
  if ($LASTEXITCODE) { $log; throw "ispc failed on kernel.ispc" }
}
finally { Pop-Location }

Normalize-Header "$tmp\bc7e\bc7e_ispc.h"
Normalize-Header "$tmp\ispctc\kernel_ispc.h"

$diff = 0
foreach ($dir in "bc7e", "ispctc") {
  # the four per-target headers ISPC also emits are not vendored; compare objects and the main header
  $outputs = Get-ChildItem "$tmp\$dir" | Where-Object { $_.Extension -eq ".obj" -or $_.Name -in "bc7e_ispc.h", "kernel_ispc.h" }
  foreach ($o in $outputs) {
    $old = "$vendor\ispc-objs\$dir\$($o.Name)"
    $same = (Test-Path $old) -and ((Get-FileHash $old).Hash -eq (Get-FileHash $o.FullName).Hash)
    if (-not $same) { $diff++ }
    "{0,-5} {1}/{2}" -f ($(if ($same) { "MATCH" } else { "DIFF" })), $dir, $o.Name
  }
}
if ($Update) {
  Copy-Item "$tmp\bc7e\*.obj", "$tmp\bc7e\bc7e_ispc.h" "$vendor\ispc-objs\bc7e\" -Force
  if (-not $Bc7eOnly) { Copy-Item "$tmp\ispctc\*.obj", "$tmp\ispctc\kernel_ispc.h" "$vendor\ispc-objs\ispctc\" -Force }
  "Updated vendor/ispc-objs. Now run tools/hash-vendor.ps1."
}
Remove-Item -Recurse -Force $tmp
exit $(if ($Update) { 0 } else { $diff })
