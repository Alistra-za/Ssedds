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

Raw rows are beside this page: `bc7e-tuning/headroom.csv` and `headroom-attribution.csv` (step 2),
`normal-mode-transitions.csv`, `sweep-parameters.csv` (step 3, parameters only) and `sweep-patch0002.csv` (step 3,
with the patch). Δ is the per-image difference against `base` (Tessalith's `veryslow`), mean with a 95% t
interval; for angles the sign is flipped so + is better. Time is the summed encode time against `base`.

### 1. Baseline

bc7e `veryslow`, linear weights, over the corpus (n = 35, 35, 15, 15):

| Class | RGB PSNR (dB) | Luma PSNR | Alpha PSNR | Premultiplied | Angle (°) | s/Mpix (4 cores, Linux) |
|---|---|---|---|---|---|---|
| diffuse | 48.32 | 51.49 | — | — | — | 0.83 |
| normal | 40.56 | 42.90 | — | — | 1.587 | 0.79 |
| cutout | 45.94 | 49.03 | 55.66 | 62.57 | — | 0.55 |
| smooth | 46.31 | 49.48 | 51.46 | 46.84 | — | 0.65 |

### 2. Headroom against the reference (10 images)

| Class (n) | Metric | bc7e `veryslow` | bc7e `slowest` | Reference | Reference Δ vs base [95% CI] | Reference time |
|---|---|---|---|---|---|---|
| diffuse (3) | RGB dB | 49.307 | 49.292 | 49.289 | −0.018 [−0.088, +0.051] | 49× |
| normal (3) | angle ° | 2.296 | 2.289 | 2.226 | +0.070 [+0.021, +0.120] | 50× |
| normal (3) | RGB dB | 36.418 | 36.484 | 36.669 | +0.252 [+0.131, +0.373] | |
| cutout (2) | RGB dB | 45.811 | 45.817 | 45.913 | +0.103 [−0.146, +0.352] | 59× |
| smooth (2) | premul dB | 48.785 | 48.712 | 48.861 | +0.076 [−1.213, +1.366] | 57× |

- On colour, `veryslow` is already at the reference: within 0.02 dB, and the reference loses on two of the
  three diffuse images (bc7e's single-colour tables and selector-perturbation passes find solutions the
  reference's local search does not). By the study's rule there was nothing to gain on colour from search
  alone.
- On normal maps the reference is better on every image, by 0.07° and 0.25 dB RGB. The attribution puts
  roughly a third of the squared-error gain on blocks where the reference keeps bc7e's mode and partition
  (endpoint quality) and two thirds on blocks where it picks a different structure; of the structural gain,
  48% is mode 4 to mode 4 with a different rotation or index selector, 17% is mode 4 to a three-subset mode
  (0 or 2), 7% mode 5 to mode 4. Mode 4 at 5-bit endpoints is where rounding each channel on its own costs
  most, which pointed at the endpoint fit rather than at the mode decision.
- The alpha classes show about 0.1 dB with intervals that include 0 at n = 2.

### 3. Targeted changes

**Parameters only** (the sweep over all 100 images; no source change):

| Change | diffuse Δ RGB | normal Δ angle | normal Δ RGB | cutout Δ RGB | smooth Δ premul | Time |
|---|---|---|---|---|---|---|
| `slowest` (4 partitions, uber 4) | −0.023 [±0.009] | +0.009 [±0.005] | +0.078 | +0.012 [±0.007] | −0.028 [±0.033] | 1.3 to 1.7× |
| partitions tried 4 | −0.044 [±0.008] | +0.005 [±0.004] | +0.054 | −0.008 [±0.004] | −0.051 [±0.030] | 0.9 to 1.2× |
| partitions tried 16 | −0.018 [±0.012] | +0.014 [±0.010] | +0.142 | −0.006 [±0.004] | −0.020 [±0.025] | 1.4 to 2.8× |
| partitions tried 64 | −0.006 [±0.020] | +0.015 [±0.010] | +0.165 | −0.006 [±0.004] | −0.008 [±0.031] | 1.7 to 4.0× |
| uber 4 | +0.025 [±0.005] | +0.004 [±0.001] | +0.022 | +0.020 [±0.006] | +0.026 [±0.010] | 1.4× |
| 2 least-squares passes | +0.004 [±0.001] | +0.001 [±0.001] | +0.009 | +0.010 [±0.005] | +0.006 [±0.002] | 1.0 to 1.1× |
| p-bit search off | −0.165 [±0.037] | −0.002 [±0.001] | −0.018 | −0.099 [±0.050] | −0.105 [±0.054] | 0.85× |
| weights (2, 2, 1) for normals | −0.129 [±0.018] | +0.033 [±0.008] | −0.096 | (colour only) | | 1.0× |
| weights (4, 4, 1) for normals | −0.421 [±0.065] | +0.038 [±0.010] | −0.337 | | | 1.0× |
| partitions 64 + uber 4 | +0.014 [±0.020] | +0.019 [±0.010] | +0.189 | +0.014 [±0.007] | +0.014 [±0.031] | 2.4 to 5.8× |

- **Partition breadth as bc7e implements it does not pay on colour.** Once more than two partitions are
  tried, bc7e ranks them without refinement and refines only the winner (`disable_faster_part_selection`),
  so the ranking itself gets worse; 4 partitions lose 0.044 dB on diffuse. 64 partitions recover to the
  baseline at 3.4× the time. On normals breadth helps (+0.165 dB at 64), but at a high price.
- **Uber 4 is the one parameter worth having**: +0.02 to +0.03 dB on every colour class at 1.4×, 35 of 35
  diffuse images.
- **The p-bit search is already pulling its weight**: turning it off costs 0.10 to 0.17 dB.
- **Weighting X and Y over Z lowers the angular error of normals** (+0.033° at ×2, +0.038° at ×4) but lowers
  their RGB PSNR by 0.10 to 0.34 dB, that is, it moves error into Z. Skyrim reads the stored Z directly and
  does not rebuild it, so this trades the channel the shader uses for the metric; not adopted.
- **Endpoint refinement passes** add almost nothing: the least-squares fit converges in one pass.

**Patch 0002** (source changes behind new flags, measured over all 100 images):

| Change | diffuse Δ RGB | normal Δ angle | normal Δ RGB | cutout Δ RGB | smooth Δ premul | Time |
|---|---|---|---|---|---|---|
| endpoint search (`ep`) | +0.051 [±0.015] | +0.065 [±0.016] | +0.337 [±0.143] | +0.140 [±0.070] | +0.055 [±0.015] | 1.4 to 1.5× |
| refine partitions, 2 tried (`rp`) | 0 | 0 | 0 | 0 | 0 | 1.0× |
| refine partitions, 4 tried | +0.040 [±0.007] | +0.005 [±0.004] | +0.061 | +0.004 [±0.003] | +0.043 [±0.020] | 1.3 to 1.5× |
| refine partitions, 8 tried | +0.067 [±0.013] | +0.010 [±0.007] | +0.111 | +0.006 [±0.005] | +0.070 [±0.034] | 1.8 to 2.5× |
| `ep` + uber 4 | +0.073 [±0.014] | +0.064 [±0.015] | +0.339 | +0.155 [±0.068] | +0.077 [±0.011] | 1.8 to 1.9× |
| **`max`** = `ep` + refined 4 partitions + uber 4 + 2 passes | **+0.115 [±0.018]** | **+0.069 [±0.015]** | **+0.398 [±0.130]** | **+0.162 [±0.068]** | **+0.121 [±0.014]** | **2.4 to 2.9×** |
| `ep` + refined 8 partitions + uber 4 + 2 passes | +0.140 [±0.023] | +0.073 [±0.015] | +0.446 | +0.164 [±0.068] | +0.148 [±0.026] | 3.4 to 4.9× |

Every row above `rp` wins on 35 of 35 (normal, diffuse) and 15 of 15 (cutout, smooth) images; every interval
excludes 0.

- **The quantised endpoint search is the finding.** bc7e fits endpoints by least squares and rounds each
  channel on its own; a greedy ±1 walk over the quantised codes with the selectors re-assigned at each step
  (what the reference encoder does) gives nearly the whole reference gap on normals (+0.065° of the 0.070°
  measured on the subset, +0.34 dB RGB) and +0.05 to +0.14 dB on colour, for 40 to 50% more time. It also
  lifts the alpha PSNR of opaque normal maps from 81 to 87 dB, because more blocks land on p-bit choices that
  decode alpha as exactly 255.
- **Refining every ranked partition fixes the breadth lever.** With refinement, 4 partitions tried gains
  +0.040 dB on diffuse instead of losing 0.044; 8 tried gains +0.067 at 2.4×.
- **`max`** is the composition that stays under 3× `veryslow`: +0.12 dB RGB on diffuse (luma +0.09),
  +0.069° and +0.40 dB on normals, +0.16 dB on cutouts (alpha +0.23), +0.12 dB premultiplied on smooth alpha
  (alpha +0.27). The 8-partition composition adds a further 0.02 to 0.05 dB for 1.6× more time; not taken.
- For comparison, the encoder benchmark put NVTT Production at −0.27 dB mean / −0.03 dB median against bc7e
  on colour at about 40 to 60× the time, and ahead by 0.3 to 0.4 dB on linear data. `max` reaches a similar
  gain on data-like content (normals) at 2.7×.

### 4. Recommendation

| Option | Verdict |
|---|---|
| Adopt `max` as a new, off-by-default level | **Yes.** Done in this branch: `encoder.effort = "max"`, bc7e patch 0002, `Bc7Level::Max`. Default output is byte-identical (the level-identity test pins every shipped level; the goldens do not move). |
| Adopt the endpoint search in the default (`veryslow`) | **Owner ruling needed.** It is the cheapest of the gains: +0.05 dB colour, +0.065° / +0.34 dB normals, +0.14 dB cutouts at 1.4× the time, 100 of 100 images. It changes every BC7 output, so the goldens, the version and an `if-changed` re-encode of every recorded output follow. Recorded here, not done. |
| Partition breadth without refinement (the `slowest` level's approach) | Drop as a lever; keep `slowest` as it is for compatibility. |
| Per-channel weights for normals | Drop: lower angular error but worse Z, which Skyrim reads. |
| More least-squares passes | Drop: converges in one. |

## What Tessalith does because of this

- **A new `max` effort level** (`encoder.effort = "max"`, DECISIONS Note BZ) maps to bc7e's
  `bc7e_compress_block_params_init_sdc_max`: `veryslow`'s search with 4 partitions tried and fully refined,
  uber 4, two least-squares passes and the quantised endpoint search. It is off by default and about 2.7× the
  time of `veryslow`. BC6H under `max` uses ISPC `veryslow`, as `slowest` does.
- **The default is unchanged.** `crates/encoders-sys/tests/level_identity.rs` pins the output of every shipped
  level for both perceptual and both exact-opaque settings; the patch leaves all of them byte-identical, and
  `cargo test -p engine --test golden` passes.
- **The patch is `crates/encoders-sys/vendor/bc7e/patches/0002-sdc-max-level.patch`**, described in
  [`README-VENDOR.md`](../../crates/encoders-sys/vendor/README-VENDOR.md). The vendored Windows objects still
  carry only patch 0001; until the owner rebuilds them with `tools/regen-ispc.ps1 -Update -Bc7eOnly` (then
  `tools/hash-vendor.ps1`), `encoders_sys::BC7E_MAX_NATIVE` is false and the engine refuses `effort = "max"`
  with `CFG-INVALID`.
- **The measurement path stays**: `sdc-bench bc7-tune` and the reference encoder, for the next question.
