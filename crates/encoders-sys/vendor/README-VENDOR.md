# Vendored encoder sources and objects

| Path | Upstream | Commit | Licence |
|---|---|---|---|
| bc7enc_rdo/ (rgbcx.*, bc7e.ispc pristine) | https://github.com/richgel999/bc7enc_rdo | b9438627eef73a1157e84201b6fa6eb2ffd6d9f0 | rgbcx: MIT or Unlicense; bc7e.ispc: Apache-2.0 |
| ispctc/ | https://github.com/GameTechDev/ISPCTextureCompressor | 79ddbc9 | MIT |
| ispc-objs/bc7e/ | built from bc7enc_rdo/bc7e.ispc (Task 14: from bc7e/bc7e.ispc, patched) with ISPC 1.31.0: `-O2 --target=sse2,sse4,avx,avx2 --opt=fast-math --opt=disable-assertions` | — | Apache-2.0 |
| ispc-objs/ispctc/ | built from ispctc/kernel.ispc with ISPC 1.31.0: `-O2 --target=sse2,sse4,avx,avx2 --opt=fast-math` | — | MIT |

All objects are produced by `tools/regen-ispc.ps1` (ISPC 1.31.0, which also passes
`--arch=x86-64 --target-os=windows`, runs with the repo root as working directory and a
repo-relative source path, and normalises line 2 of the generated headers), so a rebuild on any
machine is byte-identical. Run `tools/regen-ispc.ps1` to check (exit 0 = all MATCH); after `-Update`,
run `tools/hash-vendor.ps1`.

Provenance of the first objects: the bc7e objects are byte-identical to the ones the bench measured
(`F:/ai/dds/bench/build/bc7enc_rdo`). The ISPC TC objects were rebuilt from `ispctc/kernel.ispc`
because the bench's copies embed the bench's absolute source path; same source and flags,
but the files differ from the bench's.
