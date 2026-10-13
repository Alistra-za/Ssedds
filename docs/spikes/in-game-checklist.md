# In-game spike checklist (owner-run)

Spikes S1, S2, S6 and S8 need your Skyrim SE/AE install with Community Shaders (CS), so they cannot run in CI. None of them blocks Phases 1-2: the defaults they could change are already decided (DECISIONS #2-5, #7). They feed open items section 14 #6-9 and the in-game checklist of milestone M7.

Record for every spike: Skyrim version, CS version, ENB/CPM state, date. Put screenshots in `docs/spikes/<spike>/` (for example `docs/spikes/s1/`).

## Setup

Commands are PowerShell 7 (Git Bash also works for everything except the S1 staging snippet and the S6 one-liner). Run them from `F:\ai\dds\converter`.

- Release binaries (built from branch `feat/phase0-2`):
  - `target/x86_64-pc-windows-msvc/release/sdc.exe`
  - `target/x86_64-pc-windows-msvc/release/sdc-bench.exe`
  - Rebuild if in doubt: `cargo build --release`
- Alias used below: `Set-Alias sdc F:\ai\dds\converter\target\x86_64-pc-windows-msvc\release\sdc.exe`
- Generated asset pack (S2 and S8 sources): `target/spike-src` (200 files in `s2/`, 6 in `s8/`). Regenerate with
  `target/x86_64-pc-windows-msvc/release/sdc-bench.exe gen-spike-assets --out target/spike-src`
- Ready-made S1 source: your test folder `F:/ai/dds/mvp-test` (sources in `src/`, converted baseline in `out2/`). It holds TruePBR sets with `_g` and `_f` maps: `enchantingworkbench01`, `spriggan`, `taproot`, `horncandles01` (`_g`) and `coffin01`, `wrchbanner01` (`_f`).
- Outputs go under `out/` here (`out/s1a`, `out/s2`, ...). `--json` writes JSON Lines to stdout only, so redirect it to a file; human output stays on stderr. Use PowerShell 7 or Git Bash for `>` (Windows PowerShell 5.1 writes UTF-16).
- Add `--dry-run` to any `sdc convert` line below to see what it would do without writing files.

## S1 - BC6H `_g` and `_SRGB` `_g`/`_f` under CS

Question: does CS accept BC6H_UF16 and BC7_SRGB `_g` (emissive), and is `_SRGB` `_f` (fuzz colour) right with linear lighting on and off?

1. Stage only the `_g` and `_f` sources of your test folder, keeping the `textures/...` layout so the outputs line up with `out2/`:
   ```powershell
   $root = 'F:\ai\dds\mvp-test\src'; $dst = 'out\s1-src'
   Get-ChildItem $root -Recurse -File | Where-Object { $_.Name -match '_(g|f)\.png$' } | ForEach-Object { $t = Join-Path $dst $_.FullName.Substring($root.Length + 1); New-Item -ItemType Directory -Force (Split-Path $t) | Out-Null; Copy-Item $_.FullName $t }
   ```
2. Convert `_g` as BC6H_UF16 (the default) and `_f` as `_SRGB` (the default):
   ```powershell
   sdc convert out/s1-src -o out/s1a --recursive --target cs --accept-suggestions high
   ```
3. Convert `_g` as `BC7_SRGB`, and `_f` as UNORM:
   ```powershell
   sdc convert out/s1-src -o out/s1b --recursive --target cs --accept-suggestions high --set "file:**/*_g.png.format=bc7" --set "file:**/*_g.png.color.tag=srgb"
   sdc convert out/s1-src -o out/s1c --recursive --target cs --accept-suggestions high --set "file:**/*_f.png.color.tag=unorm" --allow TAG-UNORM-PBR
   ```
   (`s1b` also contains the default `_f` files; `s1c` also contains the BC6H `_g` files. `TAG-UNORM-PBR` is blocking since 2026-10-13, so the `s1c` command needs `--allow TAG-UNORM-PBR`: UNORM on PBR colour is the variant under test.)
4. Install the baseline `F:\ai\dds\mvp-test\out2` as a mod, then overlay one variant at a time (`out/s1a`, `out/s1b`, `out/s1c`) on top; each folder has the same `textures/pbr/...` layout.
5. Screenshot each variant with linear lighting on and off.

Pass: BC6H and `_SRGB` `_g` look alike in both linear-lighting modes with no load errors; `_SRGB` `_f` matches the authoring reference and the UNORM twin looks washed out or brighter with linear lighting off.

## S2 - small BC7 textures

Question: do 16-128 px BC7 textures with a full mip chain (no small-texture fallback, DECISIONS #7) load without crashing?

```powershell
sdc convert target/spike-src/s2 -o out/s2 --set subtype=diffuse --target cs
```
Assign the 200 outputs (`out/s2/small_<n>_<size>.dds`) to common meshes (for example replace clutter textures) and play for 10 minutes.

Pass: no crash.

## S6 - plain `_m` with alpha, flag off (PGPatcher 50% rule)

Question: with the exact-opaque flag off, how many alpha texels of a plain `_m` end up different from 255 (the BC7 mode-6 254s), and does that cross the PGPatcher 50% non-white rule?

Collect at least 20 real plain `_m` sources from your mods (not committed to the repo) into one folder, then run:
```powershell
sdc convert <folder> -o out/s6 --set subtype=envmask --set format=bc7 --set alpha.policy=remove --set alpha.exact_opaque=false --allow LOSS-ALPHA-DATA --allow ALPHA-EXACT-OFF-M --json > s6.jsonl
```
`alpha.policy=remove` drops the source alpha, so the measured value is the decoded output's share of alpha not equal to 255. `--allow LOSS-ALPHA-DATA` acknowledges that dropped alpha; `--allow ALPHA-EXACT-OFF-M` is harmless today (the code is a warning) and keeps the command valid if S6 makes it blocking.

Share of alpha not equal to 255 per file (mip 0, from the `file_done` events: `1 - eq255 / pixels`):
```powershell
Get-Content s6.jsonl | ForEach-Object { $e = $_ | ConvertFrom-Json; if ($e.kind -eq 'file_done') { $a = $e.validation.alpha[0]; '{0}  {1:P2}' -f $e.path, (1 - $a.eq255 / $a.pixels) } }
```
Pass/decision: if any file is at or above 40%, `ALPHA-EXACT-OFF-M` becomes blocking (open item section 14 #9). Optionally load the worst file in game to confirm the PGPatcher 50% rule behaviour.

## S8 - CS complex-material detection boundary

Question: at which constant `_m` alpha does CS switch complex shading and parallax off, and is the 4-code margin around 251/255 right?

```powershell
sdc convert target/spike-src/s8 -o out/s8 --target cs --set subtype=complex_material --set alpha.policy=data --json > s8.jsonl
```
Why the two `--set` options: a source whose alpha is at least 251 everywhere is treated as opaque (DECISIONS Note L), so `m_252_m` and `m_255_m` would resolve to `envmask` with the alpha removed. This command forces `complex_material` and keeps the alpha, so all five constant-alpha variants (245, 248, 250, 252, 255) reach the game with their alpha intact. `m_255_m` also logs `SUG-CONTRADICTION` and `CHAN-CONSTANT-ALPHA-DATA`; that is expected. `heightmask_m` (black RGB, alpha gradient from 0 to 252) is a control with varying alpha; note how it behaves.

Check the `VAL-M-COMPLEX-MARGIN` warnings in `s8.jsonl` (events with `"kind":"warning"`), and the smallest-mip alpha in `validation.alpha` (last entry, field `min`) for each file. Then load each output as the `_m` of a test mesh (`out/s8/m_<alpha>_m.dds`).

Pass: complex shading/parallax on for 245-250, off for 252-255 (record what `heightmask_m` does); the margin (open item section 14 #7) is set from the observed boundary.

## Report back

Send the results of S1, S2, S6 and S8 (pass/fail, screenshots, `s6.jsonl` numbers, the observed S8 boundary). They are recorded in `docs/DECISIONS.md` as `## Spike S1 ...` entries in the same shape as the S3 entry.
