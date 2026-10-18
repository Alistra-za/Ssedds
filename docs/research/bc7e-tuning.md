# bc7e tuning: is there quality left in Tessalith's BC7 encoder?

Tessalith encodes BC7 with bc7e (bc7enc_rdo b943862, patch 0001) at its `veryslow` level, which the
[encoder benchmark](encoder-benchmark.md) found ties or beats NVTT Production on real textures. This study asks
whether bc7e, as vendored, can be made measurably better for Skyrim textures, and if so what it costs.
Everything here was measured on 18 October 2026 in a Linux container; see "How to read the timings".

## Method

1. **Baseline.** bc7e at Tessalith's settings (`veryslow`, linear weights, exact-opaque off) over the corpus.
2. **Headroom.** A near-exhaustive reference encoder on a 10-image subset. If bc7e is within 0.1 dB of it,
   there is nothing to gain in quality and the study stops.
3. **Targeted changes**, one at a time, each measured for quality against time: partition and mode search
   breadth, endpoint refinement, p-bit choice, per-channel weighting for normal maps.
4. **Recommendation**: adopt as a new off-by-default level, adopt as default (needs an owner ruling, since
   the goldens change), or drop.

### Corpus

Public, redistributable images only: Poly Haven textures, all CC0 (https://polyhaven.com/license), 1k PNG,
one asset per material family, fetched through the Poly Haven API on 18 October 2026. The download list with
Poly Haven's MD5, the SHA-256 of each download and the SHA-256 of each PNG written is
`bc7e-tuning/MANIFEST.tsv` next to this page.

| Class | n | Source | Metric |
|---|---|---|---|
| `diffuse` | 35 | the asset's `Diffuse` map, alpha 255 | RGB PSNR (luma PSNR also reported) |
| `normal` | 35 | the asset's `nor_dx` map (DirectX green, as Skyrim stores it), alpha 255 | mean angular error of the stored (x, y, z), both normalised |
| `cutout` | 15 | `Diffuse` RGB with alpha from the asset's `Displacement` (or `AO`) map thresholded at its median and blurred one texel, so the edge is anti-aliased | RGB PSNR, alpha PSNR |
| `smooth` | 15 | `Diffuse` RGB with alpha = the asset's `Displacement` (or `AO`) map, a smooth ramp | premultiplied RGB PSNR, alpha PSNR |

Poly Haven textures carry no alpha or opacity maps, so the two alpha classes are compositions of each asset's
own maps rather than real decals. They exercise the alpha paths (modes 4 to 7, rotations, the alpha index set)
with real colour and real alpha statistics, but their alpha is correlated with the colour in a way a hand-painted
decal's is not; treat the alpha-class numbers as a guide, not as decal results.

### Metrics

All scoring decodes with `bcdec_rs` and compares 8-bit code values with the source, mip 0 only, as the
encoder benchmark did. RGB PSNR over the three colour channels; luma PSNR over Rec. 709 luma of the decoded
8-bit values; alpha PSNR over the alpha channel; premultiplied PSNR over RGB × alpha / 255 on both sides; angular
error as the mean angle in degrees between the source and decoded vectors (x, y, z) = c / 255 × 2 − 1, each
normalised. Per class, the per-image difference against `base` is averaged with a 95% Student-t interval; a
change is called real only when the interval excludes 0.

### Tools

- `sdc-bench bc7-tune` (`crates/bench/src/bc7tune.rs`): encodes every image with named bc7e variants through
  `encoders_sys::bc7e_compress_blocks_tuned`, a measurement-only overlay on a level's parameters, and writes one
  CSV row per image and variant. `--reference N` adds the reference encoder.
- The reference encoder (`crates/bench/src/bc7ref.rs`) tries every mode, every partition (16 or 64), all four
  rotations of modes 4 and 5 and both index selectors of mode 4; fits each subset with a PCA start, iterated
  least squares under every p-bit combination, and a greedy ±1 local search over the quantised endpoints and
  p-bits with the indices re-assigned at every step; and gives the `N` best partitions per mode the full fit.
  It is exhaustive over structure and locally optimal over endpoints, not a global optimum. A test checks that its
  packer and error model agree with the decoder on 400 random blocks across all eight modes.
- `crates/encoders-sys/tests/level_identity.rs` pins the SHA-256 of every shipped level's output over four
  deterministic images, for both perceptual settings and both exact-opaque settings, so a patch or a rebuilt
  object cannot change a shipped level unnoticed.

### How to read the timings

The study ran on Linux objects built from the vendored `bc7e.ispc` with the same ISPC 1.31.0 and the same
flags as `tools/regen-ispc.ps1`, except `--target-os=linux --pic`; they reproduce the vendored objects' output
byte for byte (the level-identity hashes match). Times are wall-clock seconds per megapixel on 4 cores with
rayon; only the ratios between variants carry over to a Windows machine. The Linux ISPC 1.31.0 binary cannot
emit Windows COFF objects (`--target-os` offers only linux, custom_linux, freebsd and android), so the vendored
objects were not rebuilt here.

## Results

_Pending: filled in from `bc7e-tuning/results.csv`._

## What Tessalith does because of this

_Pending._
