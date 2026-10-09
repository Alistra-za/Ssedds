# Rewrites crates/encoders-sys/vendor/SHA256SUMS ("<sha256>  <relative/path>", sorted, ASCII).
$ErrorActionPreference = "Stop"
$v = (Resolve-Path "$PSScriptRoot\..\crates\encoders-sys\vendor").Path
Get-ChildItem $v -Recurse -File | Where-Object Name -ne "SHA256SUMS" | Sort-Object FullName | ForEach-Object {
  "{0}  {1}" -f (Get-FileHash $_.FullName -Algorithm SHA256).Hash.ToLower(), ($_.FullName.Substring($v.Length + 1) -replace '\\', '/')
} | Set-Content -Encoding ascii "$v\SHA256SUMS"
