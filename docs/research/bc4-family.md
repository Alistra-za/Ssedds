# BC4-family encoding: BC3 alpha, BC4 and BC5 (2026-10-31)

What was asked: why Tessalith's BC3 alpha trails NVTT and Compressonator by about 2 dB on cutouts, whether a better BC4 block search fixes that and also helps BC4 and BC5, and whether a luma-weighted error is worth adding to rgbcx BC1.

Short answer:
- The cause is in rgbcx's BC4 block search, not in the engine. rgbcx anchors both BC4 modes on the block's overall minimum and maximum. In the 6-value mode that pins the endpoints next to 0 and 255, so the mode's free 0 and 255 entries are wasted.
- A search that also anchors the 6-value mode on the block's inner range, plus least-squares refits and a small local search, gains **+3.40 dB** on cutout alpha (NVTT −highest: +2.01 dB). It also gains **+0.99 dB** on smooth alpha, **+0.15 dB** on BC4 masks and **+0.10 dB** on BC5 normals. No block gets worse under the round-to-nearest decoder, and the BC4 search takes about **2×** as long.
- A coverage guard on BC3 alpha costs 0.22 dB of that gain and **halves** the cutouts' alpha-test errors at 128.
- A luma-weighted BC1 error trades colour accuracy for luma accuracy: luma PSNR +0.66 dB, RGB PSNR −0.28 dB, CIELAB ΔE worse. That is an option at most, off by default.

## Method

- **Corpus and decoder.** These are the bench sets the BC3/BC4/BC5 rows use (F:\ai\dds\bench). Round 3 (n = 91 to 98 per set): `cutout_real`, `alpha_real`, `packed_real` and `stray_real` as BC3 alpha, `mask_real` as BC4 (R), and `normal_real` as BC5 (R, G). Rounds 1 and 2 (n = 2 to 7) are a cross-check. Every block is decoded with bcdec's round-to-nearest BC4 weights, which the vendored rgbcx searches with since DECISIONS T13. The metrics are the runners' own: `a_psnr`, `coverage_err_pct` (alpha ≥ 128 mismatch, in percent of texels), `c0_psnr`, `rg_psnr` and `ang_mean` (Z rebuilt from XY). For the shipped bytes and for NVTT and Compressonator, the probe reproduces `results_rounded_dec.csv`: for example cutout `a_psnr` 47.120 / 49.128 dB, and mask 48.947 / 49.021 dB.
- **Probe.** `crates/bench/examples/bc4_family.rs` reads each source PNG and the DDS files the bench already wrote (`sdc_*_rounded`, `*_nvtt_highest`, `bc3_cmp_q1` / `bc*_cmp`). It encodes every block with the shipped FFI encoder (`encoders_sys::bc4_encode_block`, radius 5) and with these variants. Each one starts from the shipped result and replaces it only on a strictly lower SSE, so ties keep the shipped bytes:
  - `v0`: a Rust port of `rgbcx::encode_bc4_hq`. It is **byte-identical to the FFI encoder on all 43,206,656 block-channels** of the corpus.
  - `v1`: v0 plus a 6-value-mode search anchored on the inner range, for blocks that contain 0 or 255. The inner range is the values other than 0 and 255, ±5, and e0 = e1 is allowed.
  - `v2`: v1 plus least-squares endpoint refits. They run on the best block of each mode, 3 rounds each, over a ±1 rounding neighbourhood.
  - `v4`: v2 plus a 6-value search on trimmed inner ranges, where texels within 2, 6, 12, 24 or 48 of 0 or 255 are left to the palette's fixed entries. That search is anchored ±2 and refitted. Last comes an exhaustive ±3 search around the best pair in its mode.
  - `v5`: v4, except that a block keeps v0's bytes when v4 would flip more of its texels across the alpha-test value 128.
  - `v3` (`--oracle`): v2 plus an exhaustive search over every pair in [min − 16, max + 16] in both modes. It is a near-ceiling for this bitstream, at 7 to 40× the time.
- **Per block.** For each block the probe records the class (`const`; `bin` = only 0 and 255; `both+inner` = 0, 255 and other values; `one+inner` = exactly one of 0 or 255 plus others; `noext` = neither). It also records the mode (8-value when e0 > e1), whether a 6-value block's endpoints sit more than 5 away from the block's min and max (which rgbcx cannot produce), and the SSE. The attribution table replaces only one class of the shipped blocks with another encoder's blocks and reports the PSNR change.
- **Statistics.** Paired Δ per image, + = better (coverage and angle sign-flipped). The interval is a 95 % Student-t CI, and images at the 99 dB clamp are dropped. `F:\ai\dds\bench\scripts\analyze_bc4_family.py` does this; the CSVs and the generated summary are in `F:\ai\dds\bench\results\bc4_family\`.
- **Timing.** One thread on an otherwise idle machine (`RAYON_NUM_THREADS=1`), rounds 1 and 2, BC4 block search only. For BC3 that excludes the BC1 colour half, which dominates the BC3 encode time.

Reproduce: `$env:SDC_BENCH_ROOT='F:\ai\dds\bench'; cargo run --release --offline -p sdc-bench --example bc4_family -- --out <dir> [--oracle] [--rounds 1,2,3] [--sets cutout_real]` (round 3 with `--oracle`: 10 min on 16 threads), then `py -3.13 -X utf8 -I analyze_bc4_family.py <dir>...`.

## Diagnosis: the cutout gap

### Hypotheses

| Hypothesis | Result |
|---|---|
| The engine changes the alpha before encoding (premultiply, dilation, `exact_opaque`) | **No, 0 dB.** The engine's DDS files (`sdc_bc3_rounded`, `sdc_bc4_rounded`, `sdc_bc5_rounded`, from `encode_level`) equal the FFI encoder's bytes on the raw PNG channel on every block-channel: 0 of 43.2 M differ. |
| rgbcx never uses the 6-value mode with exact 0 and 255 | **Half true.** It does use the 6-value mode (17.8 % of `both+inner` cutout blocks), but always with its endpoints within ±5 of the block's min and max, which are 0 and 255 in those blocks. Its 0/255 palette entries then duplicate the endpoints. NVTT puts 86 % of those blocks in the 6-value mode, 85.5 % with inner endpoints. |
| The search radius is too narrow | **Not the radius, the anchor.** Radius 10 does not help under rounding (rgbcx `-hr10` − rgbcx: −0.06 [±0.16] dB on mask_real). A wider radius around the same anchors cannot reach endpoints around the inner range. The anchor fix alone (v1) closes 78 % of NVTT's gap. |
| No endpoint refinement | **True, and worth about 0.8 dB** on cutouts (v1 → v2), plus about 1.05 dB more from the trimmed-range and local searches (v2 → v4). |

### Where the gap is (round 3, `cutout_real`, 95 images, alpha channel)

| Class | Blocks | Share of shipped SSE | Shipped: 6-value | Shipped: 6-value with inner endpoints | NVTT: 6-value | NVTT: inner |
|---|---|---|---|---|---|---|
| const | 4,897,593 | 0 % | – | – | – | – |
| both+inner | 233,241 | 54.2 % | 17.8 % | 0 | 86.0 % | 199,525 |
| one+inner | 572,878 | 42.3 % | 25.5 % | 0 | 46.9 % | 196,397 |
| noext | 49,120 | 3.5 % | 2.3 % | 0 | 9.0 % | 2,706 |

Attribution: the mean PSNR change from replacing one class of the shipped blocks (dB):

| From | All | both+inner | one+inner | noext |
|---|---|---|---|---|
| NVTT −highest | +2.008 | +1.426 | +0.364 | +0.007 |
| Compressonator | +2.192 | +1.481 | +0.432 | +0.030 |
| v1 (anchor fix) | +1.567 | +1.152 | +0.264 | 0 |
| v4 | +3.403 | +2.213 | +0.672 | +0.025 |
| v3 (near-ceiling) | +3.656 | +2.348 | +0.717 | +0.039 |

So about 71 % of NVTT's 2.0 dB lead comes from blocks that contain both 0 and 255 plus anti-aliased values, and about 18 % from blocks with one extreme. That is the 6-value mode with inner endpoints. The rest is endpoint refinement. The `bin` class (only 0 and 255) is empty on this set, and constant blocks are exact for every encoder.

## Results (round 3, round-to-nearest decoder; Δ against the shipped encoder)

| Set, metric | n | Shipped | v1 | v2 | **v4** | **v5** | v3 near-ceiling | NVTT −highest | Compressonator |
|---|---|---|---|---|---|---|---|---|---|
| cutout_real `a_psnr` | 95 | 47.120 | +1.567 [±0.455] 95/0 | +2.349 [±0.451] 95/0 | **+3.403 [±0.476] 95/0** | **+3.183 [±0.474] 95/0** | +3.656 [±0.478] | +2.008 [±0.459] 92/3 | +2.192 [±0.455] 95/0 |
| cutout_real `coverage_err_pct` | 95 | 0.037 | −0.006 21/67 | −0.005 21/68 | −0.000 [±0.005] 32/58 | **+0.018 [±0.006] 89/0** (0.019) | +0.001 | −0.009 14/75 | −0.007 19/71 |
| alpha_real `a_psnr` | 96 | 47.242 | +0.277 [±0.105] | +0.759 [±0.232] | **+0.987 [±0.297] 96/0** | +0.925 [±0.283] 96/0 | +1.082 | +0.138 [±0.250] 34/62 | +0.490 [±0.210] |
| alpha_real `coverage_err_pct` | 96 | 0.185 | −0.001 | +0.001 | +0.003 49/33 | **+0.019 [±0.009] 80/0** | +0.006 | −0.021 10/81 | +0.004 |
| packed_real `a_psnr` | 95 | 50.198 | +0.055 | +0.114 | **+0.120 [±0.138] 92/0** | +0.104 [±0.112] 92/0 | +0.198 | −0.866 [±0.161] 1/94 | −0.426 |
| mask_real BC4 `c0_psnr` | 93 | 48.947 | +0.058 [±0.067] 35/0 | +0.118 [±0.073] 90/0 | **+0.154 [±0.093] 90/0** | +0.150 | +0.230 [±0.102] | +0.074 [±0.089] 65/28 | −0.299 |
| normal_real BC5 `rg_psnr` | 91 | 45.915 | +0.020 | +0.089 [±0.031] 88/0 | **+0.105 [±0.038] 89/0** | +0.100 | +0.180 [±0.046] | +0.103 [±0.047] 84/7 | −0.196 |
| normal_real BC5 `ang_mean` (°) | 93 | 1.891 | +0.000 | +0.001 | −0.001 [±0.003] 60/29 | −0.000 | +0.005 | −0.005 [±0.004] 18/73 | −0.010 |

`stray_real` alpha is exact for every variant (99 dB; coverage 0). Rounds 1 and 2 point the same way: cutout v4 +3.96 dB (n 4), alpha v4 +0.98 dB (n 4), mask v4 +0.38 dB (n 6, NVTT +0.39), and BC5 rg v4 +0.029 dB (n 6, NVTT −0.011).

The same data in terms of NVTT: with v4, Tessalith's BC3 cutout alpha is about **1.4 dB ahead** of NVTT −highest instead of 2.0 dB behind. BC4 goes from 0.07 dB behind to about 0.08 dB ahead, and BC5 ties on `rg_psnr` and is slightly ahead on angle.

### Never-worse

Across every variant, round and set (43.2 M block-channels, rounds 1 to 3), **0 blocks** have a higher bcdec SSE than the shipped encoder's block. That holds by construction (each variant searches a superset of the shipped candidates and replaces the result only on a strictly lower SSE), and the probe checks it with `bcdec_rs::bc4`. v5 is also never worse per block on alpha-test flips at 128, by construction. So the T13 bench gates (never-worse per block under bcdec) keep holding.

### Encode time (BC4 block search, one thread, s per megapixel)

| Set | Shipped (FFI) | v0 (port) | v1 | v2 | v4 | v5 as measured |
|---|---|---|---|---|---|---|
| cutout_real (BC3 alpha) | 0.074 | 0.045 | 0.058 | 0.099 | 0.153 (2.06×) | 0.180 |
| alpha_real (BC3 alpha) | 0.558 | 0.457 | 0.503 | 0.860 | 1.044 (1.87×) | 1.280 |
| mask_real (BC4) | 0.568 | 0.484 | 0.486 | 0.876 | 1.022 (1.80×) | 1.278 |
| normal_real (BC5) | 1.253 | 1.039 | 1.036 | 1.874 | 2.153 (1.72×) | 2.713 |

v4 roughly doubles the BC4 search time: 1.7 to 2.1× the shipped FFI and 2.1 to 3.4× the Rust port. The measured v5 re-runs v0 for its guard. A real guard reuses the shipped candidate, so it costs about the same as v4 plus a 16-texel comparison. For BC3 the alpha search is a small part of the encode, since BC1 at level 18 dominates, so the whole BC3 encode slows by much less than 2×. That was not measured end to end. The near-ceiling v3 costs 7 to 40×.

## BC1: luma-weighted error

rgbcx's BC1 error is equal-weight RGB SSE. It is summed at 14 places in 5 functions of `rgbcx.cpp` (`bc1_find_sels4_fasterr`, `_check2_err`, `_fullerr`, the forced-selector path and `bc1_find_sels3_fullerr`). The experiment (`F:\ai\dds\bench\scripts\bc1w\`) weights only those sums, in a scratch copy of the vendored rgbcx built as a separate tool. The PCA axis, the least-squares fits and the cluster orderings stay unweighted. It encodes with the engine's call (level 18, Ideal model, 3-colour blocks allowed, no transparent black). Scoring uses the bench's decoder (`bcdecode.exe`) and `run_bench.metrics`. Besides the bench's luma metrics (`y_psnr`, and `ssim_y` = SSIM on Rec.709 luma, the bench's perceptual metric), it adds mean CIELAB ΔE76 (sRGB, D65) as an independent colour-perception metric.

Round 3 `rgb_real` (93 images), Δ against unweighted. For ΔE, + = lower (better):

| Weights | `rgb_psnr` | `y_psnr` | `ssim_y` | ΔE76 | time |
|---|---|---|---|---|---|
| Rec.709 (0.2126, 0.7152, 0.0722; ints 54/183/19) | −0.284 [±0.041] 1/92 | **+0.656 [±0.121] 93/0** | **+0.00354 [±0.00075] 93/0** | −0.062 [±0.013] 8/85 | same |
| Rec.601 (0.299, 0.587, 0.114; 77/150/29) | −0.163 [±0.023] 1/92 | +0.488 [±0.095] 93/0 | +0.00275 [±0.00063] 93/0 | −0.038 [±0.010] 9/84 | same |

Round 1 is the same picture: `rgb_real` (7) Rec.709 y +0.95 dB, ssim_y +0.0045 and ΔE −0.088; `rgb_grad` (7) y +0.98 dB.

Reading: the weighting does what it says. It moves error out of luma into chroma, and both luma metrics are computed with the same Rec.709 weights it optimises. On those metrics weighted rgbcx BC1 beats every BC1 row in the bench, NVTT −highest included (y 41.65 vs 41.33 dB; ssim_y 0.9824 vs 0.9793). A metric that also sees chroma (ΔE76) gets worse on 85 of 93 images, and RGB PSNR drops on 92 of 93. So this is a preference, not a free win, like bc7e's `encoder.perceptual`. Is it worth it? Only as an opt-in option for colour textures where luma detail matters more than hue (diffuse at BC1 in the compact presets). It should not be a default. It costs no time.

## What this means for Tessalith

- The data supports shipping v4's search for BC4, for BC5's two channels and for BC3 alpha, and v5's coverage guard for BC3 alpha. The plan is `docs/superpowers/plans/2026-10-31-bc4-family.md` (not committed).
- Owner calls: whether BC3 alpha takes the coverage guard (−0.22 dB, half the alpha-test errors at 128); whether BC1 gets a perceptual option, and its default; whether the near-ceiling search becomes the `max` search radius.
- Until then nothing changes: no shipped encoder output changed on this branch.

## Limitations

- The coverage metric and v5's guard use a single alpha-test value, 128. Skyrim's per-mesh alpha threshold varies, so a guard at 128 protects the common value only.
- `premul_psnr` for alpha_real was not recomputed: only alpha changes, and the colour half is identical, but premultiplied scores combine both.
- BC5 classes and attribution use channel 0's class for both channels.
- The time table measures the BC4 search in Rust. A C++ port inside rgbcx will differ by a constant factor (the Rust v0 port is already 0.6 to 0.96× the FFI). Rounds 1 and 2 hold 2 to 7 images per set.
- BC1 is scored under the bench decoder (`bcdecode.exe`), and the base run uses the Ideal model the engine encodes for. The weighted-vs-unweighted comparison shares that decoder, so the Δs stand. Absolute values are 0.01 dB from the bench's own `bc1_rgbcx_L18` row (39.413 vs 39.424 dB), since bc7enc's CLI flags differ slightly.

## Search radius (Task 1 of the plan, rounds 1-2)

The probe measured radius 5 only. `crates/bench/src/bc4ref.rs` now holds the reference (`shipped`, `search`); the example takes `--radius`. Mean over rounds 1-2 images, v4 against the shipped search at the same radius (images at 99 dB excluded, as in the summary; radius 5 reproduces the summary's v4/v5 means). No block is worse than shipped at any radius.

| radius | set | shipped dB | v4 dB | v4 Δ | v5 Δ |
|---|---|---|---|---|---|
| 4 | alpha_real (BC3 alpha) | 47.808 | 48.862 | +1.054 | +0.967 |
| 4 | cutout_real | 45.027 | 49.087 | +4.060 | +3.889 |
| 4 | packed_real | 57.746 | 57.767 | +0.022 | +0.021 |
| 4 | mask_real (BC4) | 50.231 | 50.662 | +0.431 | +0.409 |
| 4 | normal_real (BC5 R/G) | 54.160 | 54.215 | +0.055 | +0.053 |
| 5 | alpha_real | 47.893 | 48.874 | +0.981 | +0.901 |
| 5 | cutout_real | 45.138 | 49.100 | +3.962 | +3.789 |
| 5 | packed_real | 57.770 | 57.782 | +0.013 | +0.013 |
| 5 | mask_real | 50.300 | 50.680 | +0.380 | +0.360 |
| 5 | normal_real | 54.203 | 54.232 | +0.029 | +0.028 |
| 10 | alpha_real | 48.137 | 48.903 | +0.766 | +0.706 |
| 10 | cutout_real | 45.602 | 49.129 | +3.526 | +3.399 |
| 10 | packed_real | 57.812 | 57.815 | +0.002 | +0.002 |
| 10 | mask_real | 50.444 | 50.731 | +0.287 | +0.271 |
| 10 | normal_real | 54.273 | 54.280 | +0.008 | +0.008 |

Time per image (Rust v4 against the FFI shipped encoder, 29 images, single thread, release build, ms; median / worst):

| radius | shipped (FFI) | v4 (Rust) |
|---|---|---|
| 4 | 157 / 2113 | 345 / 3354 |
| 5 | 284 / 2689 | 457 / 4573 |
| 10 | 1026 / 7887 | 1318 / 10685 |

The fast presets (radius 4) gain as much as the default (+1.05 dB BC3 alpha, +0.43 dB BC4) at about 2.2× the shipped time (v4 at radius 4 costs about what shipped costs at radius 5 plus 20 percent). Radius 10 gains less, since the shipped search already covers more. The v4 times are the Rust reference, not the C++ port.