# 06 — NIF layout check on the owner's third-party mod NIFs

Date: 2026-10-11. Read-only run of the 01 prototype scanner (`scratchpad\nifscan_scripts\scan.py`, driver `scratchpad\check3p\run.py`, raw output `check3p\result.json`) over **every loose `.nif` in `C:\_mount\moddingvol\Acherus\mods\*` and `overwrite`**. FaceGen folders were skipped. Disabled mods were included, so the 87k count is higher than 02's 69k enabled-only count. Python, single thread, below-normal priority, warm cache.

Closes 01 open question 1 (block-size reliability on third-party NIFs) and most of open question 7 (LE evidence).

## Results [Measured]

| Check | Result |
|---|---|
| Files | **87,266** loose NIFs in 63.7 s (0.73 ms/file including the whole-file read; 29.2 GB read, of which the scanner decoded 195 MB = 0.67 %) |
| Header + block-size array → footer identity (Σ sizes + root list = file length) | **87,265 / 87,265 parsed files OK, 0 bad** |
| Decoded block consumed exactly its declared size | BSLightingShaderProperty **285,489 / 285,489**; BSEffectShaderProperty **24,290 / 24,290**; BSShaderTextureSet **256,319 / 256,319**; NiAlphaProperty **88,665 / 88,665** |
| Exceptions | **1**: a string that is not valid in the Windows code page (`SkyrimMeshesExtract\...\dwelexiconstandrunes01.nif`). Scanner bug, not a file fault: NIF strings must be kept as bytes and decoded lossily |
| Stream version | BSVER 100: 85,203 · **BSVER 83 (LE): 2,062 (2.4 %) in 80 mods** |
| Texture-set slot count | 9: 255,784 · 6: 341 · 2: 68 · 3: 60 · 4: 57 · 1: 9 (535 sets with fewer than 9 — read the count, never assume 9) |
| Shader → owning shape | 309,638 referenced; 141 unreferenced (orphan blocks; harmless) |
| Shader types seen | 0: 221,567 · 1: 25,377 · 6: 13,701 · 11: 12,178 · 2: 5,959 · 5: 5,728 · 3: 750 · 16: 123 · 4: 83 · 14: 23 |
| Slots used | 0: 256,245 · 1: 254,820 · 5: 103,695 · 3: 74,861 · 4: 27,354 · 2: 22,109 · 7: 20,134 · 6: 10,718 · 8: never |

## What this changes in the plan

1. **The seek-only scanner design holds for third-party files.** No file needed a fallback. Keep the size check (it costs one addition per block) and flag failures, but no repair path is needed.
2. **Strings are bytes.** Decode lossily (Windows-1252 → UTF-8 with replacement); never fail a file on a string.
3. **LE meshes are common in real load orders, including mandatory fixes.** 2,062 LE NIFs sit in 80 mods. Mods with the most:
   - `ParallaxGen_Output` (710 of its 25,529);
   - JS Vanilla Circlets (312);
   - Northern Roads (124);
   - Meshes Optimization Project (96);
   - Mesh Improvement Compilation (68);
   - Assorted mesh fixes (57).

   The owner's ruling stands: warn and skip rendering. The warning will therefore appear on mesh-fix mods too, and the scan still lists their texture sets.
4. **A PGPatcher (ParallaxGen) output mod exists: `ParallaxGen_Output`, 25,529 NIFs.** See 05 for what this means for the priority list.
5. **Shader types 3 (Parallax, 750) and 14 (Sparkle Snow, 23) do occur.** They were dropped from the §8 slot table as "unused". The scanner must still record them; the UI can treat them as rare.
