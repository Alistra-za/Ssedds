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
| Raise bc7e's alpha error weight for maps with data in alpha (`_n` specular, complex `_m` height) | **Measured 2026-10-21, see "Alpha weighting" below; owner decides.** Alpha PSNR rises about 3 dB per dB of RGB lost; a `_n` loses 0.17° of normal accuracy at weight 2; a height map's 0 and 255 regions are not reliably pinned. Recommended: drop for `_n`; for complex `_m` at most weight 2 on top of exact-opaque, as an opt-in, not a default. |
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
  [`README-VENDOR.md`](../../crates/encoders-sys/vendor/README-VENDOR.md). The vendored Windows objects were
  rebuilt with it on 2026-10-18 (`tools/regen-ispc.ps1 -Update -Bc7eOnly`, then `tools/hash-vendor.ps1`), so
  `encoders_sys::BC7E_MAX_NATIVE` is true and `effort = "max"` works. `level_identity.rs` now pins every level
  per ISPC target (see the Windows re-check below).
- **The measurement path stays**: `sdc-bench bc7-tune` and the reference encoder, for the next question.

## Windows re-check on Skyrim textures (2026-10-18)

Question: does the endpoint search (`ep`) hold up as a default-`veryslow` change on real Skyrim textures,
with the Windows objects? Answer under the owner's acceptance rule: **no, it does not hold**. It wins on
nearly every image, but two of the four criteria fail. The default stays as it is.

### Setup

- **Objects.** The vendored Windows objects with patch 0002 (commit `79dcbb2`), ISPC dispatch on this
  machine's CPU (the avx2 path). `sdc-bench bc7-tune --variants base,ep --repeats 5`: the fastest of 5 runs
  is kept, rayon on all cores, on a machine with other builds running. `base` and `ep` run back to back per
  image.
- **Sample.** Drawn from the owner's set of 10,000 PNGs, taken from installed mods and not redistributable.
  The set is not in the repo, and paths are relative to its root. Every PNG is listed in sorted order, and
  the list is split into two pools:
  - **normal:** the stem ends in `_n` or `_normal` (2,515 files);
  - **colour:** no non-colour suffix (`_n`, `_normal`, `_nrm`, `_norm`, `_msn`, `_rmaos`, `_p`, `_s`, `_f`,
    `_g`, `_sk`, `_mask`, `_m`, `_e`, `_em`, `_h`, `_coat`, `_b`, `_i`, `_cnr`), 2,823 files.

  Each pool is shuffled with Python's `random.Random(20261018)`, and files are taken in that order. Sizes that
  are not multiples of 4 are skipped.
  - **Normal:** the first 60.
  - **Colour, opaque:** fewer than 1% of texels have alpha below 255. The first 60 go to `diffuse`.
  - **Colour, alpha-bearing:** the first 30. These go to `cutout` when at least 90% of texels have alpha
    exactly 0 or 255, and to `smooth` otherwise. That gives 16 cutout and 14 smooth.

  The list is `bc7e-tuning/skyrim-sample.tsv`: class, corpus name, relative path, SHA-256 and alpha
  statistics. The raw rows are in `.superpowers/bc7e-ep-verify/skyrim-base-ep.csv`, which is gitignored.
- **Metrics:** as in the study (Metrics above). Δ = `ep` − `base` per image, averaged per class, with a 95%
  Student-t interval. For angles the sign is flipped, so + is better. Time is the summed `ep` time over the
  summed `base` time.
- **Differences from the study's corpus.**
  - Skyrim `_n` maps keep specular in alpha, while the study's normals had alpha 255. Part of each block's
    effort goes to alpha.
  - Many Skyrim diffuse alphas are masks, not decals.
  - A few textures are near-flat, with a base PSNR above 60 dB. On those a tiny absolute error change shows
    up as a very large PSNR change. Two diffuse images gain +10.9 and +21.1 dB, and one normal map gains
    +32.0 dB RGB.

### Results

| Class | n | Primary Δ [95% CI] | Wins | Second Δ [95% CI] | Time | Study (Linux, Poly Haven) |
|---|---|---|---|---|---|---|
| diffuse | 60 | RGB +0.645 [±0.784] (−0.139 to +1.429) | 60/60 | luma +0.892 [±1.128] | 1.37× | RGB +0.051 [±0.015] |
| normal | 60 | angle +0.0285° [±0.0048] (+0.0236 to +0.0333) | 59/60, 1 tie | RGB +1.081 [±1.075] | 1.45× | +0.065° [±0.016], RGB +0.337 [±0.143] |
| cutout | 16 | RGB +0.133 [±0.142] (−0.009 to +0.275) | 16/16 | alpha +0.102 [±0.064] | 1.39× | RGB +0.140 [±0.070] |
| smooth | 14 | premul +0.204 [±0.088] | 12/14, 2 ties | alpha +0.216 [±0.144] | 1.50× | premul +0.055 [±0.015] |

No image got worse on its class's primary metric. All four classes together take 1.41× the time.

### Verdict under the owner's rule

| Criterion | Result | |
|---|---|---|
| Diffuse Δ RGB > 0, CI excludes 0, at least +0.025 dB | +0.645, CI −0.139 to +1.429 | **fails** (CI includes 0) |
| Normal Δ angle > 0, CI excludes 0, at least +0.03° | +0.0285°, CI +0.0236 to +0.0333 | **fails** (below +0.03°) |
| No class with mean Δ < 0 and CI excluding 0 | all means positive | passes |
| Time ratio at most 1.7× | at most 1.50× | passes |

**The claim does not hold on this sample, so `veryslow` is unchanged.**

Context for a re-ruling (not part of the rule):
- **The diffuse failure comes from the two near-flat images.** The diffuse median Δ is +0.056 dB and the 10%
  trimmed mean is +0.083 dB. With images over 60 dB base excluded (n = 54), the mean is +0.079 [±0.019] dB.
- **The normal shortfall is 0.0015°.** The median is +0.026° and the trimmed mean +0.027°. With images over
  60 dB base excluded (n = 53), the mean is +0.0313° [±0.0049]. It is about half the study's +0.065°, which
  fits the alpha (specular) channel taking part of the effort.
- **Cutout and smooth.** Without the over-60 dB images, cutout gives +0.048 [±0.011] and smooth +0.238
  [±0.079].

### Correction: the Linux objects do not reproduce the Windows output

"How to read the timings" above says the study's Linux objects reproduce the vendored objects' output byte
for byte. On Windows they do not.
- None of the 32 level hashes recorded on Linux matches any of the four Windows ISPC code paths. The level
  test therefore failed on `main` with the shipped objects, while the goldens passed.
- On this machine the sse2, sse4 and avx paths encode alike, and the avx2 path (FMA) encodes differently. So
  bc7e output depends on the CPU the encode runs on.
- `crates/encoders-sys/tests/level_identity.rs` now pins every level per target (commit `6ef3bbf`).
  Patch 0002 leaves levels 0 to 6 byte-identical on all four targets (commit `79dcbb2`).
- The study's quality deltas are unaffected, since each one compares two variants built from the same
  objects.

### `max` against `veryslow` and `slowest` on the same sample

Same objects, sample and metrics as above, avx2 path, rayon on all 16 threads. Two timing methods:
- **Alternating:** `--variants base,max,slowest` repeated three times with `--repeats 1`, so each image runs
  base, max, slowest, base, max, slowest, and so on. The fastest of the three is kept.
- **Separate:** one run per variant over the whole sample, `--repeats 3`, fastest kept, run back to back.

Per-image rows: `bc7e-tuning/skyrim-max.csv` (scores, fastest alternating time, fastest separate time).
Δ is `max` − `base` on the class's primary metric (angle sign flipped), 95% t interval.

| Class | n | Δ [95% CI] | Wins | `max` s/Mpix (alt / sep) | Ratio to `veryslow` (alt / sep) | `slowest` ratio (alt / sep) |
|---|---|---|---|---|---|---|
| diffuse | 60 | RGB +0.762 [±0.783]; +0.178 [±0.026] without images over 60 dB | 60/60 | 0.421 / 0.379 | 2.68× / 2.70× | 1.61× / 1.61× |
| normal | 60 | angle +0.0338° [±0.0050] | 59/60, 1 tie | 0.517 / 0.476 | 2.84× / 3.18× | 1.64× / 1.62× |
| cutout | 16 | RGB +0.287 [±0.268] | 16/16 | 0.399 / 0.464 | 2.54× / 3.03× | 1.50× / 1.51× |
| smooth | 14 | premul +0.227 [±0.093] | 12/14, 2 ties | 0.212 / 0.243 | 2.43× / 2.81× | 1.33× / 1.38× |
| all | 150 | | | 0.438 / 0.414 | **2.73× / 2.95×** | 1.59× / 1.59× |

- `veryslow` ran at 0.161 s/Mpix alternating and 0.141 s/Mpix separate. These are kernel times on all
  cores, not the file-to-file times of the [encoder benchmark](encoder-benchmark.md).
- **Load.** The machine was in use. A cargo build ran during about half of the alternating run (up to 6
  rustc processes; CPU 14% busy just before the run). The separate runs had no cargo or rustc running
  (CPU 2 to 10% busy before each). Under that build both `base` and `max` slowed, and the ratio came out
  lower. Both ratios are within 15% of the expected ~3×, and the alternating one is inside the study's 2.4 to
  2.9×. The separate ratio is just above it. Expect roughly 3× the `veryslow` time; yours may differ under load.
- On this sample `slowest` is worse than `veryslow` on diffuse (RGB −0.175 [±0.098], 7/60 wins) and slightly
  better on normals (+0.0072°). `max` beats `slowest` on every class: diffuse RGB +0.323 [±0.122] without
  images over 60 dB, normals +0.0266° [±0.0043], alpha PSNR +0.320 [±0.125] over the 30 alpha textures.

## Alpha weighting (2026-10-21)

Question: does raising bc7e's alpha error weight give cleaner alpha on BC7 maps that store data in alpha, and
what does it cost in RGB? Measurement only: no engine, preset or encoder change, and no golden moves.
Branch `research/bc7-alpha-weight`; rows in `bc7e-tuning/alpha-weighting.csv`.

bc7e's error is `w_r·ΔR² + w_g·ΔG² + w_b·ΔB² + w_a·ΔA²` per texel with linear per-channel weights
(`Bc7Tune.weights`, `perceptual = false`, Tessalith's setting); the weight travels with the channel through the
mode 4/5 rotations and drives the partition and mode choices as well as the endpoint fit. Opaque blocks never
see `w_a`. The baseline is `veryslow` with weights 1,1,1,1 and exact-opaque off.

### Corpus

Built by `bc7e-tuning/make_alpha_classes.py` from the same Poly Haven CC0 assets as the study corpus (every
source PNG re-checked against `MANIFEST.tsv`; the new classes and their maps, URLs and hashes are in
`MANIFEST-alpha.tsv`). This is a synthetic stand-in for Skyrim `_n` and `_m` files, not real game textures:

| Class | n | Construction | Alpha statistics |
|---|---|---|---|
| `n_spec` | 35 | RGB = `nor_dx`, alpha = 255 − `Rough` (specular as 1 − roughness), 1k | smooth to mid-frequency greyscale, mean 99, 2% at 0 |
| `n_spec_mip2` | 35 | the same at mip 2: two 2×2 box passes, RGB renormalised, 256 px | mean 100 |
| `m_height` | 34 | R = `AO` (env-mask stand-in), G = 255 − `Rough` (gloss), B = `Metal` or 0, alpha = `Displacement` stretched so its 1st and 99th percentiles land on 0 and 255 and clipped | real 0 regions (1.0%) and 255 regions (1.1%) |
| `cutout`, `smooth` | 15, 15 | the study's controls (Diffuse with a thresholded or raw Displacement/AO alpha) | |

Poly Haven's displacement PNGs are 16-bit; the generator scales them to 8 bits itself (PIL's grey conversion
clips them). One diffuse asset has a flat displacement map and was skipped.

### Metrics

Per image at mip 0, and at mip 2 for `n_spec`: RGB PSNR (for the normal classes the mean angular error of the
stored vector instead), alpha PSNR, the largest absolute alpha error, the share of source alpha-0 and alpha-255
texels that decode exactly, alpha SSIM (Gaussian window σ 1.5, radius 5, K1 0.01, K2 0.03, L 255, averaged over
the region where the window fits), the share of texels whose alpha test at 128 flips (the cutout edge), and
encode time (fastest of 2 runs, 4 cores, Linux objects built from the vendored source). Δ is the per-image
difference against the baseline, mean with a 95% Student-t interval and the worst image; a change is called
real when the interval excludes 0. Angles are shown as degrees of error, so a negative Δ is worse.

### Results

Baselines: `n_spec` angle 1.962°, RGB 38.28 dB, alpha 39.20 dB, alpha SSIM 0.916; `n_spec_mip2` 1.758°,
40.10 dB, 40.59 dB, 0.927; `m_height` RGB 43.54 dB, alpha 41.75 dB, exact-0 share 86.4%, exact-255 share 70.9%,
SSIM 0.984; `cutout` RGB 45.94 dB, alpha 55.66 dB, exact-0 91.9%, exact-255 75.9%, flips 0.009%; `smooth`
premultiplied 46.84 dB, RGB 46.31 dB, alpha 51.46 dB.

**`n_spec` (n = 35), Δ against the baseline, mean [±95% CI], worst image:**

| Variant | angle (°) | RGB (dB) | alpha PSNR (dB) | alpha max err (codes) | alpha SSIM (×1000) | time |
|---|---|---|---|---|---|---|
| weight 2 | −0.171 [±0.047], worst −0.691 | −0.600 [±0.143], worst −1.93 | +1.82 [±0.25], worst +0.57 | −5.8 [±2.6] | +22.4 [±6.2] | 1.00× |
| weight 4 | −0.394 [±0.106], worst −1.459 | −1.280 [±0.286], worst −3.89 | +3.69 [±0.44], worst +1.20 | −12.1 [±3.9] | +41.1 [±11.7] | 1.02× |
| weight 8 | −0.658 [±0.175], worst −2.084 | −1.986 [±0.383], worst −4.86 | +5.49 [±0.56], worst +2.26 | −17.0 [±5.0] | +55.6 [±16.0] | 1.01× |
| exact-opaque | 0 | 0 | 0 | 0 | 0 | 1.00× |

At mip 2 the picture is the same: weight 2 costs 0.148° [±0.045] and 0.64 dB RGB for +1.81 dB alpha; weight 8
costs 0.56° and 2.10 dB for +5.41 dB. Exact-opaque changes nothing here because no block has alpha 255
throughout.

**`m_height` (n = 34):**

| Variant | RGB (dB) | alpha PSNR (dB) | alpha max err | exact-0 share (pt) | exact-255 share (pt) | alpha SSIM (×1000) | time |
|---|---|---|---|---|---|---|---|
| weight 2 | −0.906 [±0.081], worst −1.38 | +1.73 [±0.14], worst +0.75 | −4.6 [±1.3] | +3.2 [±1.1], worst −2.9 | +4.3 [±1.5], worst −5.8 | +5.3 [±1.6] | 1.00× |
| weight 4 | −1.824 [±0.168], worst −2.77 | +3.37 [±0.31], worst +1.04 | −7.5 [±1.5] | +4.9 [±2.1], worst −6.5 | +7.8 [±2.5], worst −8.8 | +8.9 [±2.8] | 1.01× |
| weight 8 | −2.684 [±0.248], worst −4.15 | +4.75 [±0.45], worst +1.14 | −9.2 [±1.5] | +5.4 [±2.9], worst −9.6 | +9.8 [±3.0], worst −11.9 | +11.1 [±3.5] | 1.01× |
| exact-opaque | 0.000 | +0.002 | 0 | 0 | +9.0 [±5.0], worst 0 | 0 | 1.00× |
| weight 2 + exact-opaque | −0.906 [±0.081] | +1.73 [±0.14] | −4.6 | +3.2 [±1.1], worst −2.9 | +13.4 [±5.5], worst −5.8 | +5.3 | 1.02× |
| weight 4 + exact-opaque | −1.824 [±0.168] | +3.38 [±0.31] | −7.5 | +4.9 [±2.1], worst −6.5 | +16.8 [±5.9], worst −8.8 | +8.9 | 1.02× |

**Controls:**

| Class, variant | primary | RGB (dB) | alpha PSNR (dB) | exact-0 / exact-255 (pt) | alpha SSIM (×1000) | alpha-test flips at 128 (pt) |
|---|---|---|---|---|---|---|
| cutout, weight 2 | RGB −0.199 [±0.119], worst −0.71 | | +5.5 [±1.8] | +5.2 [±1.8] / +1.2 [±1.3] | +2.7 [±1.4] | −0.001 [±0.001] (no change) |
| cutout, weight 4 | RGB −0.402 [±0.280], worst −1.44 | | +12.2 [±3.8] | +7.1 [±2.8] / +2.2 [±2.3] | +3.6 [±1.9] | −0.002 [±0.003] (no change) |
| cutout, weight 8 | RGB −0.659 [±0.521], worst −2.50 | | +19.6 [±5.9] | +7.6 [±3.1] / +2.8 [±3.1] | +3.8 [±2.0] | −0.004 [±0.004] (no change) |
| cutout, exact-opaque | RGB −0.004 [±0.005] | | +0.02 | 0 / +19.5 [±27.7] | +0.2 | 0 |
| smooth, weight 2 | premul +0.015 [±0.030] | −0.178 [±0.148], worst −0.66 | +0.45 [±0.37] | | +1.9 [±1.7] | |
| smooth, weight 4 | premul −0.064 [±0.069] | −0.431 [±0.361], worst −1.72 | +0.98 [±0.81] | | +3.4 [±3.0] | |
| smooth, weight 8 | premul −0.249 [±0.211] | −0.745 [±0.626], worst −2.92 | +1.58 [±1.34] | | +4.7 [±4.1] | |
| smooth, exact-opaque | premul +0.082 [±0.186] | −0.124 [±0.119], worst −0.83 | +29.2 [±11.9] | | +1.3 [±0.7] | |

### What the numbers say

- **The trade is about 3 dB of alpha for 1 dB of RGB, at every weight.** On `n_spec` weight 2 buys +1.8 dB
  alpha for −0.60 dB RGB, weight 8 +5.5 dB for −2.0 dB; on `m_height` +1.7 dB for −0.9 dB and +4.7 dB for
  −2.7 dB. Encode time does not change. Every variant trips the study's 0.1 dB RGB flag on every class.
- **A `_n` pays in normal accuracy.** Weight 2 adds 0.17° of angular error (9% of the baseline, 2.6× the 0.065°
  the endpoint search recovered in this study), weight 8 adds 0.66°; the worst image loses 0.7° to 2.1°. The
  specular channel's SSIM rises from 0.916 to 0.938 (weight 2) and 0.971 (weight 8). The same holds at mip 2.
- **A height map's 0 and 255 regions are not reliably pinned by weighting.** On average the exact-0 and
  exact-255 shares rise by 3 to 10 points, but the worst images lose 3 to 12 points: the weight moves the fit,
  it does not constrain it. The largest alpha error drops by 5 to 9 codes out of 24. Exact-opaque pins the
  all-255 blocks (+9 points of exact-255 share) at no cost, which is what Tessalith already does for `_m`;
  the weight adds to that (+13 points together at weight 2) and still leaves worst images below the baseline.
- **The cutout edge does not get worse.** Alpha-test flips at 128 stay at 0.009% ± 0.001 for every weight.
  Cutout alpha PSNR jumps (+5.5 to +19.6 dB) because its alpha is mostly 0 and 255 and the weight pushes those
  texels onto exact codes; RGB falls 0.2 to 0.66 dB. Smooth alpha is near neutral on its premultiplied metric
  at weight 2 and worse at weight 8.
- **Exact-opaque on the controls** shows the known mode-6 behaviour: on `smooth`, whose Poly Haven alpha maps
  saturate at 255 over large regions, pinning the all-255 blocks lifts alpha PSNR by 29 dB [±12] for −0.12 dB
  RGB; on `cutout` it raises the exact-255 share by 19.5 points on average (the interval is wide: a few images
  have no all-255 blocks).

### Recommendation

- **`_n` with specular in alpha: drop (keep weight 1).** The angular cost is large against anything else this
  study has moved, and the specular gain is 1.8 dB on a channel that is already at 39 dB.
- **Complex `_m` with height in alpha: not as a default.** If parallax quality is judged to matter more than
  gloss, metal and the env mask together, weight 2 on top of exact-opaque is the only setting worth offering,
  as an opt-in: −0.9 dB RGB for +1.7 dB alpha, +13 points of exactly decoded 255 texels, alpha SSIM 0.984 →
  0.990, with the caveat that it still loses exact-extreme texels on some images. Weights 4 and 8 cost 1.8 and
  2.7 dB of RGB and are not worth it.
- The corpus is synthetic Poly Haven data: `n_spec`'s alpha is 1 − roughness, not a painted specular map, and
  `m_height`'s RGB is AO / gloss / metal from a scan, not an artist's `_m`. The trade-off ratio is a property of
  the encoder's error sum and should carry over; the absolute SSIM and exact-share numbers depend on the
  content and should be re-measured on real Skyrim `_n` / `_m` files before a weight is adopted anywhere.

### Not checked

- No in-game or rendered comparison: PSNR, SSIM and exact shares only.
- Mip 2 only for `n_spec`; `m_height` was measured at mip 0 only.
- Weights between 1 and 2, and asymmetric RGB weights (for example lowering only the env-mask channel), were not
  tried.
- Only `veryslow`; the `max` level and NVTT were not measured with alpha weights.
- Linux objects and timings; the Windows objects encode the same source with per-target differences (see the
  Windows re-check), so the Δs should hold but were not re-run there.
