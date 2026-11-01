# Vendored encoder sources and objects

| Path | Upstream | Commit | Licence |
|---|---|---|---|
| bc7enc_rdo/ (rgbcx.* with the rgbcx BC4 round-to-nearest and BC4 search patches, bc7e.ispc pristine) | https://github.com/richgel999/bc7enc_rdo | b9438627eef73a1157e84201b6fa6eb2ffd6d9f0 | rgbcx: MIT or Unlicense; bc7e.ispc: Apache-2.0 |
| ispctc/ (kernel.ispc with patches/0001-bc6h-peak-mode.patch) | https://github.com/GameTechDev/ISPCTextureCompressor | 79ddbc9 + patch 0001 | MIT (kernel.ispc modified; change notice at the top) |
| bc7e/ (bc7e.ispc, patches/0001-exact-opaque-alpha.patch, patches/0002-sdc-max-level.patch) | bc7enc_rdo/bc7e.ispc + sdc patches 0001 and 0002 | b9438627eef73a1157e84201b6fa6eb2ffd6d9f0 + patches 0001, 0002 | Apache-2.0 (modified; change notices at the top of the file) |
| ispc-objs/bc7e/ | built from bc7e/bc7e.ispc (pristine + patches 0001 and 0002, rebuilt on Windows 2026-10-18) with ISPC 1.31.0: `-O2 --target=sse2,sse4,avx,avx2 --opt=fast-math --opt=disable-assertions` | — | Apache-2.0 |
| ispc-objs/ispctc/ | built from ispctc/kernel.ispc (with patch 0001, rebuilt on Windows 2026-10-31) with ISPC 1.31.0: `-O2 --target=sse2,sse4,avx,avx2 --opt=fast-math` | — | MIT |

All objects are produced by `tools/regen-ispc.ps1` (ISPC 1.31.0, which also passes
`--arch=x86-64 --target-os=windows`, runs with the repo root as working directory and a
repo-relative source path, and normalises line 2 of the generated headers), so a rebuild on any
machine is byte-identical. Run `tools/regen-ispc.ps1` to check (exit 0 = all MATCH); after `-Update`,
run `tools/hash-vendor.ps1`.

Provenance of the first objects: the bc7e objects are byte-identical to the ones the bench measured
(`<bench>/build/bc7enc_rdo`). The ISPC TC objects were rebuilt from `ispctc/kernel.ispc`
because the bench's copies embed the bench's absolute source path; same source and flags,
but the files differ from the bench's.

Three vendored encoders carry sdc patches: bc7e (patches 0001 and 0002 below), ISPC TC (BC6H patch 0001 below) and rgbcx (the BC4 round-to-nearest and BC4 search patches at
the end of this file).

Patch 0001 "exact opaque alpha" (`bc7e/patches/0001-exact-opaque-alpha.patch`) changes bc7e. `bc7e/bc7e.ispc` is the pristine `bc7enc_rdo/bc7e.ispc` with that patch applied:
it renames the unused `bc7e_compress_block_params::m_unused0` to `m_exact_opaque_alpha` (same struct
layout) and, when that flag is set, restricts the mode-6 encodes of opaque blocks to p-bits = 1 so bc7e
optimises RGB under the constraint and alpha decodes to exactly 255. With the flag false the output is
bit-identical to upstream (gate `sdc-bench gate exact-opaque-native`, reference
`crates/bench/tests/data/pre-patch-bc7e.sha256`). The patch file is produced with
`git diff --no-index --text crates/encoders-sys/vendor/bc7enc_rdo/bc7e.ispc crates/encoders-sys/vendor/bc7e/bc7e.ispc`.
Since patch 0001 the bc7e objects are built from the patched source and no longer match the bench's.

Patch 0002 "sdc_max level" (`bc7e/patches/0002-sdc-max-level.patch`, 2026-10-18) applies on top of patch 0001. It adds
four trailing `bool` fields to `bc7e_compress_block_params` (`m_endpoint_search`, `m_refine_partitions`, two unused), so
the struct grows and the generated header changes; `bc7e_compress_block_params_init` clears them. `m_endpoint_search` runs
a greedy +-1 search over the quantised endpoints (one channel of one endpoint at a time, selectors re-assigned) at the end
of `color_cell_compression`; `m_refine_partitions` makes the mode 1/3/7 partition ranking refine every candidate instead of
only the winner once more than two are tried. The new export `bc7e_compress_block_params_init_sdc_max` is veryslow plus
4 partitions tried, uber 4, 2 least-squares passes and both flags; sdc maps `encoder.effort = "max"` to it. With both flags
false every existing level is bit-identical to the patch-0001 source: `encoders-sys` `tests/level_identity.rs` pins the
output of every shipped level for both perceptual and both exact-opaque settings, and passed against Linux objects built
from the patched source. The patch file is `git diff --no-index --text <patch-0001 bc7e.ispc> crates/encoders-sys/vendor/bc7e/bc7e.ispc`.
Measurements: docs/research/bc7e-tuning.md.

**Objects.** The vendored `ispc-objs/bc7e/*.obj` and `bc7e_ispc.h` were rebuilt from the patch-0002 source on Windows
(2026-10-18) with `tools/regen-ispc.ps1 -Update -Bc7eOnly` and `tools/hash-vendor.ps1`; a second `regen-ispc.ps1` run
matches 12/12. `build.rs` finds `m_endpoint_search` in the header, so `encoders_sys::BC7E_MAX_NATIVE` is true and
`effort = "max"` works. Levels 0 to 6 encode byte-identically to the patch-0001 objects on each of the four ISPC
targets (`tests/level_identity.rs`, which pins every level per target because ISPC's sse2/sse4/avx paths and its avx2
path encode differently; engine goldens 95/95).

ISPC TC patch 0001 "BC6H peak mode" (`ispctc/patches/0001-bc6h-peak-mode.patch`, 2026-10-31) changes `ispctc/kernel.ispc`
(`git diff main -- crates/encoders-sys/vendor/ispctc/kernel.ispc` at the time). It adds a peak-preserving BC6H mode that is
on when a `bc6h_enc_settings` has both `slow_mode` and `fast_mode` set, a combination no upstream profile uses (`fast_mode`
is read only when `slow_mode` is off), so the settings struct, `ispc_texcomp.h/.cpp` and the generated header do not change.
In that mode the encoder finds each block's brightest texel and guards only highlight blocks, where that texel is at
least 32 times the block's geometric-mean luminance; other blocks encode exactly as veryslow. In a guarded block every
candidate is ranked by its squared error against the block, and one that decodes the brightest texel below 0.9 of its
source luminance ranks after every candidate that keeps it. A second search then runs on a copy with every channel
lifted to at least 2^-12 of its texel's largest channel; it is ranked against the unlifted block too, and the better of
the two results is kept. The shim builds the
profile as veryslow plus `fast_mode` (`g_bc6[5]`, `Bc6hLevel::VeryslowPeak`). The five upstream profiles encode
byte-identically to the pre-patch objects: `encoders-sys` `tests/bc6h.rs` pins their AVX2-path output, recorded from the
pre-patch objects. Rebuilt with `tools/regen-ispc.ps1 -Update` and `tools/hash-vendor.ps1`; the bc7e objects did not
change and a second `regen-ispc.ps1` run matches 12/12. Why and measurements: .claude/DECISIONS.md (Note FE, "BC6H
peak-preserving mode") and docs/research/encoder-benchmark.md ("BC6H (HDR)").

Patch "rgbcx BC4 round-to-nearest" (M6 Track D, 2026-10-12) edits `bc7enc_rdo/rgbcx.h` in place, marked
`sdc patch (M6 Track D): round to nearest to match hardware/bcdec`. `bc4_block::get_block_values6/8`, the BC4 palette
that `encode_bc4_hq` (BC4, both BC5 halves, BC3 alpha) and `unpack_bc4` use, floored the interpolants (`(6l+h)/7`);
they now use bcdec_rs 0.2.0's `bc4_block` fixed-point weights verbatim (`(w0*l + w1*h + 32768) >> 16`), so the
endpoint search minimises the error a rounding decoder sees (docs/research/encoder-benchmark.md, "BC4/BC5 search radius"). Nothing else in
rgbcx changes; BC1 is untouched. Test: `encoders-sys` `bc4_search_model_matches_bcdec_for_every_endpoint_pair`.
Upstream `rgbcx.h` at b943862 has SHA-256 `bd51b3d07a2a6e158cf9761ac3bd76b9f761c0f770c317e15ed7609e74e092fa`.

Patch "rgbcx BC4 search" (2026-11-01) applies on top of round-to-nearest, marked `sdc patch (bc4 search, see
README-VENDOR.md)`. `rgbcx.h` gains two `mode_flag` bits, `BC4_SDC_SEARCH = 4` and `BC4_SDC_GUARD128 = 8`; `rgbcx.cpp`
gains `namespace sdc_bc4` (helpers) and a block at the end of `encode_bc4_hq`. With `BC4_SDC_SEARCH` (and no forced
selectors), after the unchanged mode loop the block it chose is the start, and further endpoint pairs replace it only on
a strictly lower error: a 6-value search around the inner range (values other than 0 and 255), least-squares endpoint
refits of the best and of a fresh other-mode search, 6-value searches on trimmed inner ranges, and an exhaustive +-3
around the best pair. `BC4_SDC_GUARD128` keeps the mode-loop block when the result would flip more texels across 128.
The search equals `sdc_bench::bc4ref::search` byte for byte (`crates/bench/tests/bc4ref.rs`, including the bench corpus
when `SDC_BENCH_ROOT` is set); its least-squares step is f64, so `build.rs` compiles with `/fp:precise`; do not add `/arch:AVX2` (or enable FP contraction) without re-running
`ffi_equals_reference_on_corpus`. `BC4_SDC_SEARCH` requires `BC4_USE_ALL_MODES`. Without the new
bits `encode_bc4_hq` is unchanged. The shim sets `BC4_SDC_SEARCH` for BC4 and both BC5 halves, plus `BC4_SDC_GUARD128`
for BC3 alpha (`encode_bc3_hq` passes its flags through). Measurements: docs/research/bc4-family.md.
Before this patch (round-to-nearest only) `rgbcx.cpp` had SHA-256 `6f4d4664daa957a9d49f9fe881eab4a04c3d7c58a381280da0ecea1cf354c51c`
(the upstream b943862 file; round-to-nearest did not touch it) and `rgbcx.h` `38f1bd4551dab3700552d9a9d726946a527bb9072dc3337dc58794f3d9fef87e`.
