# Vendored encoder sources and objects

| Path | Upstream | Commit | Licence |
|---|---|---|---|
| bc7enc_rdo/ (rgbcx.*, bc7e.ispc pristine) | https://github.com/richgel999/bc7enc_rdo | b9438627eef73a1157e84201b6fa6eb2ffd6d9f0 | rgbcx: MIT or Unlicense; bc7e.ispc: Apache-2.0 |
| ispctc/ | https://github.com/GameTechDev/ISPCTextureCompressor | 79ddbc9 | MIT |
| bc7e/ (bc7e.ispc, patches/0001-exact-opaque-alpha.patch) | bc7enc_rdo/bc7e.ispc + sdc patch 0001 | b9438627eef73a1157e84201b6fa6eb2ffd6d9f0 + patch 0001 | Apache-2.0 (modified; change notice at the top of the file) |
| ispc-objs/bc7e/ | built from bc7e/bc7e.ispc (pristine + patch 0001) with ISPC 1.31.0: `-O2 --target=sse2,sse4,avx,avx2 --opt=fast-math --opt=disable-assertions` | — | Apache-2.0 |
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

Patch 0001 "exact opaque alpha" (`bc7e/patches/0001-exact-opaque-alpha.patch`) is the only change to
any vendored encoder. `bc7e/bc7e.ispc` is the pristine `bc7enc_rdo/bc7e.ispc` with that patch applied:
it renames the unused `bc7e_compress_block_params::m_unused0` to `m_exact_opaque_alpha` (same struct
layout) and, when that flag is set, restricts the mode-6 encodes of opaque blocks to p-bits = 1 so bc7e
optimises RGB under the constraint and alpha decodes to exactly 255. With the flag false the output is
bit-identical to upstream (gate `sdc-bench gate exact-opaque-native`, reference
`crates/bench/tests/data/pre-patch-bc7e.sha256`). The patch file is produced with
`git diff --no-index --text crates/encoders-sys/vendor/bc7enc_rdo/bc7e.ispc crates/encoders-sys/vendor/bc7e/bc7e.ispc`.
Since patch 0001 the bc7e objects are built from the patched source and no longer match the bench's.
