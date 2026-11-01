# Local encoder benchmark

Written 2026-10-16 (commit 1bae037b).

The [encoder survey](encoder-survey.md) found that nobody had published a fair comparison of the main BC7 encoders at their top settings, or any good data on alpha, BC4, BC5 or BC6H. So the project ran its own benchmark in three rounds (8 to 9 October 2026), plus a later test of the BC4/BC5 search radius (12 October 2026). This page merges them. Where round 3 re-measured a claim on 100 images, its number is the one shown.

## The questions

1. Which BC7 encoder is best on real textures, gradients, cutouts, smooth alpha and data in alpha, and at what speed?
2. Which encoder is best for BC1, BC3, BC4, BC5 and BC6H?
3. Is BC5 or BC7 better for normal maps at the same size?
4. Do the round 1 and 2 results hold on a much larger set of images?

## How it was measured

### Machine and tools

| Item | Value |
|---|---|
| CPU / GPU | AMD Ryzen 7 9800X3D (8 cores, 16 threads), NVIDIA RTX 4090 |
| OS | Windows 11 Pro 26200 |
| bc7enc_rdo | v1.08, commit b943862; bc7e built with ISPC 1.31.0 |
| Intel ISPC TC | commit 79ddbc9, through a small command-line wrapper (16 row strips on 16 threads) |
| NVIDIA Texture Tools | 3.2.5 (`nvcompress`) |
| DirectXTex texconv | 2026.5.8.1 |
| AMD Compressonator CLI | 4.5.52 |

### Images

- Round 1: 39 images in 10 sets, 2 to 7 real textures each (CC0 ambientCG, 1K) plus synthetic gradients. Round 2 added mask, normal, data and HDR sets of 2 to 7 images each (4 real and 4 synthetic HDR images).
- Round 3: 100 real images per set (18 HDR images), from 121 ambientCG materials, 191 ambientCG materials, decals and atlases with opacity, and 14 Poly Haven HDRIs. At most 4 assets per material family. All CC0.

| Set | What it holds | Main metric |
|---|---|---|
| `rgb_real` | colour maps | RGB PSNR |
| `rgb_grad` | synthetic gradients (round 1 only, 7 images) | RGB PSNR |
| `cutout_real` | colour with anti-aliased opacity | RGB PSNR over visible pixels, coverage error |
| `alpha_real` | smooth opacity (decals) | premultiplied RGB PSNR |
| `packed_real` | colour plus an unrelated map in alpha | RGB PSNR and alpha PSNR |
| `stray_real` | opaque images with 2% of texels at alpha 250 | RGB PSNR against the clean twin |
| `mask_real` | single-channel masks (roughness, AO, height, opacity, metalness) | PSNR |
| `normal_real` | DirectX normal maps | angular error in degrees |
| `normalh_real` | normal plus height in alpha | angle and height PSNR |
| `data_real` / `dataa_real` | three data maps in RGB, with and without a fourth map in alpha | RGB and alpha PSNR |
| `hdr_real` | HDR images | multi-exposure PSNR (mPSNR), RMSLE, peak ratio |

### Protocol

- **One decoder for every output**, mip 0 only, compared with the source in 8-bit code values. No colour-space conversion on either side. BC1 colours were decoded as `round((2a+b)/3)` ("IdealRound4"). BC4 interpolants (BC4, BC5, BC3 alpha) were decoded rounded down, as upstream rgbcx does, where GPUs round to nearest; [BC3, BC4 and BC5](#bc3-bc4-and-bc5-tessaliths-encoder-against-the-others-2026-10-31) rescores them both ways.
- **Normal maps** were scored the way a BC5 shader reads them: X and Y decoded, Z rebuilt. BC7 was also scored with its stored Z.
- **BC6H** used identical fp16 input for every tool. mPSNR uses 13 exposures from 2⁻⁶ to 2⁶. Peak ratio is decoded peak luminance divided by source peak; 1.0 means the brightest highlight survives.
- **Alpha flags were set the way a real pipeline would**: NVTT `-noalpha` on opaque sets and `-alpha` on alpha sets; ISPC alpha profiles on alpha sets.
- **Statistics (round 3).** For each claim "A against B", the per-image difference was averaged, with a 95% Student-t interval, a bootstrap interval and a bootstrap that resamples material families. A side is called better only when both the t-interval and the family interval exclude 0. Medians and win counts are given too, because a few near-lossless images can pull a mean.

In the tables, Δ is A minus B, so a positive number means A is better. For angles and RMSLE the sign is flipped so that + still means A is better.

## BC7 on opaque colour

Round 3, `rgb_real`, n = 99, against bc7e level 6 (`-u6`):

| Encoder | Mean Δ RGB (dB) [95% CI] | Median | Images won | Round 1 (n = 7) |
|---|---|---|---|---|
| bc7e u5 | +0.03 [+0.01, +0.05] | +0.02 | 70/99 | +0.01 |
| bc7e u4 | −0.40 [−0.44, −0.36] | −0.38 | 0/99 | −0.39 |
| NVTT Production | −0.27 [−0.51, −0.02] | −0.03 | 32/99 | −0.32 |
| ISPC slow | −0.44 [−0.50, −0.38] | −0.36 | 0/99 | −0.59 |
| ISPC basic | −0.59 [−0.70, −0.49] | −0.43 | 0/99 | −0.75 |
| NVTT Normal (GPU) | −1.36 [−1.69, −1.02] | −0.92 | 0/99 | −1.42 |
| texconv `-bc x` | −1.47 [−1.77, −1.17] | −0.96 | 0/99 | −1.58 |
| texconv GPU | −1.76 [−1.99, −1.54] | −1.55 | 0/99 | −1.76 |
| bc7e u6 perceptual, RGB PSNR | −1.05 [−1.18, −0.91] | −1.07 | 5/99 | −1.03 |
| bc7e u6 perceptual, luma PSNR | +3.69 [+3.31, +4.06] | +3.41 | 99/99 | +4.77 |
| Compressonator `-Quality 1.0` | not re-measured | | 4/7 | −0.16 |

- **bc7e level 5 equals level 6.** It is at least as good on average and 1.5× faster.
- **NVTT Production is a practical tie on typical textures** (median −0.03 dB). Its mean is lower because of a few near-flat images where both encoders are far below one code value.
- **Perceptual mode trades RGB for luma.** It loses about 1 dB of RGB PSNR and gains about 3.7 dB of luma PSNR.
- **texconv is the weakest BC7 encoder at every setting.** `-bc x` (modes 0 and 2 on) adds only +0.04 dB.

Speed, round 1, file-to-file time for a 1-Mpix texture (start-up cost subtracted):

| Encoder | s per Mpix | Δ RGB on real images (round 1) |
|---|---|---|
| NVTT Normal (GPU), texconv GPU | 0.03 to 0.09 | −1.4 / −1.8 |
| ISPC basic | 0.11 | −0.75 |
| bc7e u4 | 0.12 | −0.39 |
| bc7e u5 | 0.22 | +0.01 |
| bc7e u6 | 0.28 | 0 |
| ISPC slow | 0.30 | −0.59 |
| NVTT Production (= Highest) | about 14 | −0.32 |
| texconv CPU `-bc x` | about 16 to 20 | −1.6 |
| Compressonator `-Quality 1.0` | about 33 | −0.16 |

bc7e kernel speed by level (Mpix/s): u0 146, u1 44, u2 38, u3 19, u4 24, u5 6.7, u6 4.5. Levels 3 and 4 are out of order: level 3 is slower than level 4, and on gradients level 3 beats level 4.

### Smooth gradients (round 1, 7 synthetic images)

| Encoder | Mean RGB PSNR (dB) | Δ against bc7e u6 |
|---|---|---|
| NVTT Production | 66.14 | +3.86 |
| Compressonator `-Quality 1.0` | 65.49 | +3.21 |
| ISPC slow | 63.01 | +0.73 |
| bc7e u6 / u5 | 62.28 / 62.27 | 0 / −0.01 |
| bc7e u4 | 59.89 | −2.39 |
| bc7e u6 perceptual | 58.42 | −3.86 |
| NVTT Normal | 56.08 | −6.20 |
| texconv (any setting) | 55.4 to 55.9 | −6.4 to −6.9 |

On gradients NVTT Production and Compressonator spend their much longer time fitting the ramp exactly. The gap is not visible at 8 bits: every BC7 encoder at a high setting kept the largest error at 1 to 2 code values on every gradient except the noise image. It matters only if the texture is later contrast-stretched (for example a lookup ramp or a brightness multiplier).

## BC7 with alpha

Round 3, n = 100, against bc7e u6:

| Set, metric | Encoder | Mean Δ [95% CI] | Median | Wins | Verdict |
|---|---|---|---|---|---|
| Cutout, visible RGB PSNR (dB) | NVTT Production | −0.14 [−0.19, −0.08] | −0.06 | 29/100 | bc7e better, by a small margin |
| Cutout, coverage error (points) | NVTT Production | +0.005 [+0.001, +0.009] | +0.001 | 66/100 | negligible either way |
| Smooth alpha, premultiplied PSNR (dB) | NVTT Production | −0.06 [−0.15, +0.02] | −0.07 | 42/100 | tie |
| Smooth alpha, premultiplied PSNR (dB) | ISPC slow | −0.58 [−0.65, −0.50] | −0.50 | 1/100 | bc7e better |
| Smooth alpha, premultiplied PSNR (dB) | texconv `-bc x` | −2.04 [−2.31, −1.78] | −1.74 | 1/100 | bc7e better |
| Packed, RGB PSNR (dB) | NVTT Production | +0.04 [−0.00, +0.09] | +0.04 | 69/100 | tie |
| Packed, alpha PSNR (dB) | NVTT Production | +0.27 [−0.05, +0.59] | +0.06 | 73/100 | tie, leaning NVTT |
| Packed, RGB PSNR (dB) | bc7e u6 perceptual | −2.89 [−3.07, −2.70] | −2.81 | 0/100 | perceptual is wrong here |

- On cutouts every BC7 encoder kept the coverage error (texels flipping across alpha 128) at 0.05 to 0.13% in round 1.
- Compressonator was only measured in round 1 (n = 4 to 5): −1.50 dB on smooth alpha and −0.47 dB on cutouts. Its mode counts explain why: it used modes 4 and 5 in **0.0%** of blocks at every setting, where bc7e used them in 47% of alpha blocks and NVTT Production in 48%.
- Perceptual mode weights alpha at 256 against luma at 128, so with data in alpha it spends bits on alpha at the cost of colour.

### Stray alpha (round 1)

Opaque images where 2% of texels have alpha 250, compared with the same encoder on the clean image:

| Encoder | Mean Δ RGB (dB) | Worst image |
|---|---|---|
| bc7e u6 | −1.71 | −4.33 (linear gradient) |
| ISPC slow | −1.32 | −4.27 |
| NVTT Production | −2.47 | −6.98 |
| NVTT Production `-noalpha` | −2.03 | −6.98 |
| texconv CPU `-bc x` | −0.57 | −1.44 |
| Compressonator `-Quality 1.0` | −5.19 | −17.21 |

- Stray alpha costs about 1 dB on real textures and up to 17 dB on smooth ones, because blocks with alpha below 255 lose modes 0 to 3.
- NVTT's `-noalpha` does not strip the alpha. In round 3 it helped on 98 of 100 images (+0.58 dB [+0.50, +0.65]) but only partly.
- The only complete fix is to set alpha to 255, or drop the channel, before encoding.

### "Opaque" BC7 is not always alpha 255

Even on RGB-only input, some encoders decode alpha as 254 in some mode-6 blocks:

| Encoder | Pixels with alpha ≠ 255 on opaque input |
|---|---|
| bc7e u6 | 28.0% (all 254, mode 6) |
| ISPC slow | 14.8% (254, mode 6) |
| texconv CPU | 2.4% |
| Compressonator `-Quality 1.0` | 1.1% |
| NVTT Production | 0.0% |

Mode 6 stores alpha in 7 bits plus a p-bit shared with RGB, so alpha 255 needs that p-bit set to 1. bc7e's opaque path picks the p-bit for RGB only. This matters only where something reads the alpha of an "opaque" texture, which in Skyrim is the `_m` environment mask (see [Design overview](design-overview.md#skyrim-texture-rules)).

## BC1 and BC3

Round 3, n = 100:

| Comparison | Mean Δ (dB) [95% CI] | Median | Wins |
|---|---|---|---|
| BC1: NVTT `-highest` against rgbcx L18 `-r` (encoded for the rounding decoder) | −0.11 [−0.19, −0.03] | −0.02 | 4/100 |
| BC1: NVTT `-highest` against rgbcx L18 (default rounding) | +0.19 [−0.01, +0.40] | +0.06 | 97/100 |
| BC1: rgbcx L18 `-r` against default rgbcx L18 | +0.30 [+0.09, +0.52] | | 100/100 |
| BC1: Compressonator against rgbcx L18 | −0.49 [−0.56, −0.42] | −0.47 | 4/100 |
| BC1: texconv against rgbcx L18 | −2.13 [−2.49, −1.77] | −1.60 | 0/100 |
| BC3 premultiplied: NVTT `-highest` against rgbcx L18 `-r` | +0.09 [−0.04, +0.21] | −0.14 | 38/100 |
| BC3 premultiplied: ISPC against rgbcx L18 `-r` | −1.18 [−1.30, −1.07] | −1.02 | 0/100 |

- rgbcx at level 18 is the best BC1 and BC3 encoder when it is told which decoder rounding to expect. The decoder model matters more than the choice between the top encoders.
- On cutouts, BC1's 1-bit alpha keeps coverage exact but the visible RGB was **6.5 dB (NVTT) to 11.9 dB (Compressonator) worse than BC7** in round 1, and the anti-aliased edges are lost.

### Format gap at best settings (round 1)

| | BC7 best | BC1 best | BC3 best |
|---|---|---|---|
| Opaque real (RGB PSNR, dB) | 50.96 | 41.24 (−9.7) | – |
| Opaque gradients | 62.28 (66.14 NVTT Production) | 48.48 (−14) | – |
| Smooth alpha real (premultiplied) | 55.42 | – | 50.43 (−5.0) |

The encoder moves BC7 quality by about 0.3 to 1.6 dB at top settings. The format moves it by 5 to 15 dB.

## BC4 (single-channel masks)

Round 3, n = 99, against bc7enc_rdo's rgbcx BC4 (`-4`):

| Encoder | Mean Δ (dB) [95% CI] | Median | Wins |
|---|---|---|---|
| rgbcx with search radius 10 (`-hr10`) | +0.13 [+0.07, +0.20] | +0.05 | 98/99 |
| NVTT `-highest` | −1.29 [−1.76, −0.83] | −0.89 | 10/99 |
| Compressonator | −1.16 [−1.56, −0.76] | −0.81 | 13/99 |
| ISPC | −2.98 [−3.69, −2.26] | −2.10 | 0/99 |
| texconv | −2.93 [−3.69, −2.17] | −1.99 | 1/99 |

The encoder matters more for BC4 than its reputation suggests. In round 2, ISPC and texconv could not reproduce a binary metalness map exactly and lost 6 to 8 dB on smooth gradients.

### BC4/BC5 search radius

rgbcx searches endpoint pairs within a radius around its first guess. The radius was tested on the round-2 mask and normal sets (7 images each), decoded with bcdec.

The first run found that a wider search could make results worse (BC5 at radius 10 scored 47.92 dB against 57.99 dB at radius 5). The cause: upstream rgbcx modelled BC4 decoding with floored interpolants, while bcdec and the GPU round to nearest. A wider search found more endpoint pairs that were exact only because of the floor. rgbcx was patched to round like the decoder. After the patch:

| Format | Radius | Mean PSNR (dB) | Δ against radius 5 | Time against radius 5 |
|---|---|---|---|---|
| BC4 | 3 | 57.0994 | −0.1577 | 0.49× |
| BC4 | 4 | 57.1980 | −0.0591 | 0.73× |
| BC4 | 5 | 57.2571 | 0 | 1.00× |
| BC4 | 10 | 57.3802 | +0.1231 | 2.80× |
| BC5 | 3 | 60.4957 | −0.1066 | 0.47× |
| BC5 | 4 | 60.5661 | −0.0362 | 0.71× |
| BC5 | 5 | 60.6023 | 0 | 1.00× |
| BC5 | 10 | 60.6622 | +0.0599 | 2.96× |

- The patch raised the radius-5 result by 3.07 dB (BC4) and 2.61 dB (BC5).
- PSNR now rises with the radius on every one of the 14 images.
- Radius 10 gains little for about 3× the time. Radius 4 loses under 0.06 dB on the means for about 0.7× the time.

## Normal maps: BC5 against BC7

Round 3, n = 100, angular error (+ means A has less error):

| Comparison | Mean Δ [95% CI] | Median | Wins |
|---|---|---|---|
| BC7 (NVTT Production) against BC5 (rgbcx), both with Z rebuilt | −0.56° [−0.64, −0.49] | −0.52° | 3/100 |
| BC5: Compressonator against rgbcx | −0.039° [−0.049, −0.029] | −0.041° | 12/100 |
| BC5: NVTT `-highest` against rgbcx | −0.063° [−0.070, −0.056] | −0.070° | 3/100 |
| BC5: ISPC against rgbcx | −0.141° [−0.163, −0.120] | −0.143° | 9/100 |
| BC5: texconv against rgbcx | −0.144° [−0.160, −0.127] | −0.151° | 5/100 |
| BC7: bc7e u6 against NVTT Production (stored Z) | −0.051° [−0.058, −0.044] | −0.044° | 3/100 |
| BC7: perceptual against linear bc7e u6 | −0.363° [−0.441, −0.284] | −0.257° | 10/100 |
| Normal + height: bc7e u6 against NVTT, angle | −0.061° [−0.071, −0.051] | −0.052° | 7/100 |
| Normal + height: bc7e u6 against NVTT, height PSNR | −0.26 dB [−0.33, −0.18] | −0.17 dB | 15/100 |

- **When the shader rebuilds Z, BC5 is the better normal format** at the same 8 bits per texel. BC5 gives X and Y two independent channels; BC7 shares endpoints across X, Y and Z.
- Round 2 had compared BC7's stored Z with BC5's rebuilt Z. That method is biased, because many ambientCG normal maps are not unit length (rebuilding Z from the source's own XY already differs by 0.43° on average for the round-2 maps and 1.47° for the new ones). The fair test rebuilds Z for both.
- **For BC7 normals, NVTT Production is slightly better than bc7e**, and perceptual mode is always wrong for normals.
- **Skyrim's shaders do not rebuild Z.** They read the blue channel directly, so the BC5 result does not apply to a Skyrim `_n` (see [Design overview](design-overview.md#skyrim-texture-rules)).

## BC7 for linear data

Round 3, n = 100, against NVTT Production:

| Set | Encoder | Mean Δ (dB) [95% CI] | Median | Wins |
|---|---|---|---|---|
| Data, opaque | bc7e u6 | −0.56 [−0.94, −0.18] | −0.30 | 4/100 |
| Data, opaque | ISPC slow | −0.74 [−1.19, −0.30] | −0.42 | 0/100 |
| Data, opaque | texconv GPU | −4.25 [−4.88, −3.62] | −3.44 | 0/100 |
| Data, opaque | bc7e u6 perceptual against linear | −2.37 [−2.72, −2.01] | −2.09 | 0/100 |
| Data + alpha, RGB | bc7e u6 | −0.27 [−0.38, −0.17] | −0.20 | 2/100 |
| Data + alpha, alpha | bc7e u6 | −0.32 [−0.43, −0.22] | −0.19 | 18/100 |
| Data + alpha, RGB | texconv GPU | −3.26 [−3.54, −2.98] | −2.93 | 0/100 |

- NVTT Production is the best BC7 encoder for linear data, on both RGB and alpha, but only by a few tenths of a dB over bc7e, at roughly 40 to 60× the time (round 2).
- Perceptual mode is wrong for data: it favours green at the cost of red and blue.
- In round 2, separate BC4 masks (about 49.8 dB) beat three maps sharing one BC7 (about 42.8 dB), at 1.5× the memory.

## BC7 at the `max` level (2026-10-31)

`bc7e_max` is Tessalith's `Bc7Level::Max` (bc7e patch 0002, [bc7e tuning](bc7e-tuning.md)), run on every BC7 image of all three rounds through `sdc-bench encode-one --format bc7 --level max`. It writes the bytes the engine writes: it calls `engine::encode::encode_level` with the default preset's profile at effort `max` (perceptual off, no exact-opaque flag, no custom weights, `presets/sdc-base.toml`), so the lossless rotated-block alpha fix is included. The source PNG goes in unchanged, as for every other encoder, and the output goes through the same decoder and scorer. On these 925 images the fix changed one block in one file (a mode-4 block in `data_real`, whose alpha is not scored); at `max` bc7e already writes opaque alpha in rotated blocks, so every score is the same as raw bc7e Max.

Round 3, mean Δ of `bc7e_max` against the other encoder [95% t-interval], wins out of n (+ = `max` better; angles in degrees, sign flipped):

| Set (n) | Metric | vs bc7e u6 | vs NVTT Production | vs ISPC slow |
|---|---|---|---|---|
| `rgb_real` (92) | RGB PSNR | +0.19 dB [±0.03], 92/92 | +0.45 [±0.26], 85/92 | +0.62 [±0.07], 92/92 |
| `cutout_real` (95) | visible RGB PSNR | +0.19 [±0.02], 95/95 | +0.34 [±0.05], 94/95 | +0.46 [±0.04], 94/95 |
| `alpha_real` (96) | premultiplied PSNR | +0.12 [±0.02], 89/96 | +0.17 [±0.08], 77/96 | +0.69 [±0.08], 95/96 |
| `packed_real` (96) | RGB / A PSNR | +0.12 / +0.16 | +0.07 / −0.11 [±0.33] | +0.56 / +0.58 |
| `stray_real` (98) | RGB PSNR | +0.30 [±0.22], 97/98 | +0.45 [±0.14] | +0.66 [±0.53] |
| `normal_real` (93) | mean angle | +0.028° [±0.005], 90/93 | −0.012° [±0.002], 8/93 | +0.036° [±0.007] |
| `normalh_real` (96) | mean angle / height PSNR | +0.034° / +0.23 dB | −0.017° / −0.02 dB [±0.05] | +0.047° / +0.42 dB |
| `data_real` (96) | RGB PSNR | +0.25 [±0.13], 96/96 | −0.13 [±0.03], 18/96 | +0.40 [±0.13] |
| `dataa_real` (96) | lowest-channel PSNR | +0.19 [±0.04], 92/96 | −0.08 [±0.04], 25/96 | +0.24 [±0.03] |

- **`max` beats bc7e u6 on every set in every round**, by 0.1 to 0.3 dB on real textures (cutout coverage error moves by 0.002 points, negligible) (round 1: +0.15 dB on `rgb_real`, n = 7; up to +3.2 dB on the `rgb_grad` gradients).
- **It now beats NVTT Production on colour and alpha** (opaque, cutout, smooth alpha, stray). On linear data and normal maps NVTT Production stays ahead by about 0.1 dB, or 0.01 to 0.02°, at roughly 100× the time.
- **On gradients (rounds 1 and 2) it roughly ties NVTT Production** (`rgb_grad` −0.13 dB [±0.73]), where u6 was 3.9 dB behind.
- Against bc7e u6 perceptual it loses luma, as linear mode always does (`rgb_real` luma PSNR −3.5 dB).
- **Speed.** Round 1: 0.9 to 1.5× the time of bc7e u6 per set, 8.7 s against 8.2 s over all 39 images (process start included; 2.5 s against 2.2 s for the 7 `rgb_real` images). In round 3 `max` measured 0.64 to 0.83× the time of u6, but the round-3 r1 and r2 passes that wrote the u6 rows ran at the same time (their logs overlap on 9 October), while `max` ran alone. Those ratios favour `max` and are not a speed claim.

The full per-set tables for all three rounds (including bc7e u6 perceptual, and the seconds) are in the bench folder, `results/summary_maxpeak.md` (`scripts/analyze_maxpeak.py`) and `results/summary_engine_bytes.md` (`scripts/analyze_engine_bytes.py`).

## BC3, BC4 and BC5: Tessalith's encoder against the others (2026-10-31)

`sdc rgbcx (rounded)` (CSV ids `sdc_bc3_rounded`, `sdc_bc4_rounded`, `sdc_bc5_rounded`) is the engine's own BC3, BC4 and BC5 encode, through `sdc-bench encode-one --format bc3|bc4|bc5 --level default` (`engine::encode::encode_level`). That is the vendored rgbcx with the round-to-nearest BC4 model (DECISIONS T13), level 18, BC4/BC5 search radius 5, the default preset's settings. BC4 uses a `height` profile, BC5 a `flow` profile. BC3 has no built-in sub-type, so it is a `diffuse` profile with the format set to BC3, as a user override would set it. Its BC3 colour is encoded for the engine's default BC1 model (`ideal`). The rows ran on every set that has BC3, BC4 or BC5 rows, in all three rounds.

**The bench decoder floors BC4 interpolants.** `bcdecode` is built on upstream rgbcx, which decodes the six interpolated BC4 values as `(6a + b) / 7` and so on, rounded down. bcdec and GPUs round to nearest. This affects BC4, both BC5 channels and BC3 alpha, and every row in the [BC1 and BC3](#bc1-and-bc3), [BC4](#bc4-single-channel-masks) and [BC5](#normal-maps-bc5-against-bc7) tables above was scored this way. Upstream rgbcx (the `bc3_rgbcx_*`, `bc4_rgbcx*` and `bc5_rgbcx*` rows) models the same floor, so those rows score well under this decoder. Those rows also use level 18 and radius 5 (bc7enc's default; `-hr10` is radius 10), so against Tessalith's rows the decoder model is the only difference. Every BC3, BC4 and BC5 output was therefore scored twice: with the bench decoder (**floor**), and with the same files decoded with bcdec's round-to-nearest weights (**round**, `scripts/rescore_rounded.py`). BC3 colour is decoded as IdealRound4 in both.

Round 3, mean Δ of `sdc rgbcx (rounded)` against the other encoder [95% t-interval], wins/losses (+ = Tessalith better):

| Set (n), metric | Against | Floor decoder | Round decoder |
|---|---|---|---|
| `mask_real` (93), BC4 PSNR (dB) | rgbcx `-4` | −1.79 [±0.72], 0/93 | +1.65 [±0.45], 93/0 |
| | rgbcx `-hr10` | −1.93 [±0.72], 0/93 | +1.71 [±0.56], 84/9 |
| | NVTT `-highest` | −0.46 [±0.28], 9/84 | −0.07 [±0.09], 28/65 |
| | NVTT default | −0.07 [±0.29], 74/19 | +0.73 [±0.13], 88/5 |
| | Compressonator | −0.60 [±0.37], 17/76 | +0.30 [±0.16], 74/19 |
| | ISPC | +1.19 [±0.10], 93/0 | +1.82 [±0.11], 93/0 |
| | texconv | +1.14 [±0.11], 92/1 | +2.11 [±0.59], 92/1 |
| `normal_real` (93), BC5 mean angle, Z rebuilt (°) | rgbcx `-5` | −0.068 [±0.009], 4/87 | +0.080 [±0.008], 90/1 |
| | rgbcx `-hr10` | −0.076 [±0.010], 7/84 | +0.094 [±0.015], 90/3 |
| | NVTT `-highest` | −0.004 [±0.007], 46/45 | +0.005 [±0.004], 73/18 |
| | Compressonator | −0.028 [±0.010], 16/75 | +0.010 [±0.007], 74/17 |
| | ISPC | +0.073 [±0.020], 82/10 | +0.102 [±0.022], 81/10 |
| | texconv | +0.075 [±0.010], 85/7 | +0.093 [±0.011], 87/5 |
| `alpha_real` (96), BC3 premultiplied PSNR (dB) | rgbcx L18 | −0.06 [±0.02], 22/74 | +0.15 [±0.03], 94/2 |
| | rgbcx L18 `-r` | −0.12 [±0.02], 7/89 | +0.06 [±0.03], 74/22 |
| | NVTT `-highest` | −0.20 [±0.14], 47/49 | −0.18 [±0.13], 46/50 |
| | Compressonator | +0.33 [±0.18], 66/30 | +0.37 [±0.17], 67/29 |
| | ISPC | +1.07 [±0.10], 96/0 | +1.08 [±0.11], 96/0 |
| `cutout_real` (95), BC3 alpha PSNR (dB) | rgbcx L18 | −0.15 [±0.01], 0/95 | +0.14 [±0.01], 95/0 |
| | NVTT `-highest` | −2.12 [±0.46], 1/94 | −2.01 [±0.45], 3/92 |
| | Compressonator | −2.33 [±0.45], 0/95 | −2.19 [±0.45], 0/95 |
| `packed_real` (95), BC3 alpha PSNR (dB) | rgbcx L18 | −2.40 [±0.75], 0/95 | +2.05 [±0.45], 95/0 |
| | NVTT `-highest` | −0.23 [±0.34], 74/21 | +0.87 [±0.16], 94/1 |
| | Compressonator | −0.89 [±0.41], 12/83 | +0.43 [±0.17], 87/8 |

- **Under the rounding decoder Tessalith's rgbcx beats upstream rgbcx** on BC4 (+1.7 dB), BC5 (+0.08°) and BC3 alpha (+0.14 to +2.0 dB), on almost every image. Under the floor decoder the order reverses, by about the same amount. Each scores about the same under the model it targets (BC4 mean 48.95 dB rounded for Tessalith, 48.90 dB floored for upstream), so the gap is the decoder model, not the search.
- **NVTT `-highest` is the only close rival.** On BC4 it is ahead by 0.07 dB [±0.09] and wins 65 of 93 images; on BC5 Tessalith is ahead by 0.005°. Every other encoder is behind under both decoders.
- **The wider search radius gains nothing once decoding rounds.** Upstream rgbcx `-hr10` against `-4` is +0.13 dB [±0.07] under the floor decoder and −0.06 dB [±0.16] under the rounding one; BC5 `-hr10` is −0.014° [±0.016].
- **BC3 alpha on cutouts is rgbcx's weak spot.** NVTT and Compressonator are about 2 dB ahead there under both decoders, while visible RGB is within 0.1 dB.
- **BC3 colour is exactly upstream rgbcx L18's** (Δ 0 on every image). Encoded for `ideal`, it is 0.10 to 0.32 dB behind `-r` under the bench's IdealRound4 colour decoder; that gap is the BC1 model setting, not the encoder.
- Rounds 1 and 2 (2 to 7 images per set) point the same way. BC4 `mask_real` (n = 6) against rgbcx: −1.59 dB floor, +1.14 dB round; against NVTT `-highest`: −1.02 / −0.39 dB. BC5 `normal_real` (n = 7) against rgbcx: −0.078° / +0.056°.
- Speed: on `mask_real` the BC4 encode took 0.78× the time of bc7enc's `-4` (13.1 s against 16.8 s over 93 images, process start included), with another build running on the machine.

Per-set tables for all three rounds, with the mean of each variant under both decoders and the seconds, are in `results/summary_engine_bytes.md`.

## BC6H (HDR)

Round 2 (4 real HDR images):

| Encoder | mPSNR (dB) | RMSLE ×1000 | Worst peak ratio | s per Mpix |
|---|---|---|---|---|
| ISPC `bc6h_veryslow` | 51.50 | 3.53 | 0.22 (a sunset sun) | 0.19 |
| texconv CPU | 50.65 | 4.25 | 0.29 | 10.7 |
| Compressonator (any quality) | 50.32 | 4.32 | 0.29 | 1.2 to 16.7 |
| NVTT `-bc6 -production` (= `-highest`) | 50.01 | 3.30 | 0.95 | 2.9 |
| texconv GPU | 49.31 | 4.41 | 0.92 | 0.41 |

Round 3, n = 18, against ISPC `bc6h_veryslow`:

| Encoder | Metric | Mean Δ [95% CI] | Wins |
|---|---|---|---|
| texconv CPU | mPSNR | −0.76 dB [−1.05, −0.47] | 0/18 |
| NVTT Production | mPSNR | −2.38 dB [−3.36, −1.40] | 1/18 |
| NVTT Production | RMSLE | −0.00005 [−0.00048, +0.00037] | 11/18 |
| NVTT Production | peak ratio | +0.07 [−0.03, +0.17] | 9/18 |

- **ISPC `bc6h_veryslow` is the best general BC6H encoder**, and the fastest.
- ISPC (and texconv CPU) can clip a small, extremely bright highlight such as a visible sun. Round 3 showed this is specific to that kind of content, not general. Round 2's "NVTT has the best RMSLE" did not hold at n = 18.
- NVTT keeps highlights but is clearly weaker in darks and wide-range content.

### Peak-preserving ISPC mode (2026-10-31, Note FE)

**Why ISPC loses the sun.** ISPC TC measures error on the half-float bit patterns, which is a log-like scale (`kernel.ispc` `bc6h_setup`). The sun's texels come from an RGBE source, where blue is stored as exactly 0.0, and on that scale 0.0 is far from every other value. So blue dominates the PCA axis, the index choice and the least-squares endpoints. venice_sunset's 6464 peak decodes as 1464, at every effort level.

**The new mode.** `Bc6hLevel::VeryslowPeak` (ISPC TC patch 0001) changes only highlight blocks: blocks whose brightest texel is at least 32× the block's geometric-mean luminance. In those blocks it keeps that texel at 0.9 or more of its source luminance whenever some candidate can. Every other block encodes exactly as veryslow.

Measured with `sdc-bench bc6-peak` against the `.f16` source as float (not the `.ref.npy` the Python bench used), n = 18 real images:

| Image | Peak ratio (veryslow → peak) | mPSNR Δ (dB) | RMSLE ×1000 (veryslow → peak) |
|---|---|---|---|
| venice_sunset | 0.219 → 0.923 | −0.674 | 3.96 → 3.34 |
| moonless_golf | 0.732 → 0.995 | −0.349 | 8.50 → 9.03 |
| dikhololo_night | 0.817 → 1.090 | −0.002 | 1.99 → 2.02 |
| dreifaltigkeitsberg | 0.849 → 1.008 | 0.000 | 5.57 → 5.59 |
| satara_night | 1.026 → 1.026 | −0.048 | 14.43 → 14.99 |
| the other 13 | unchanged | −0.007 to 0.000 | within ±0.01 |

- Mean Δ over the 18 real images:
  - peak ratio: +0.078 [−0.012, +0.168];
  - mPSNR: −0.061 dB [−0.147, +0.026];
  - RMSLE: +0.000028 [−0.00009, +0.00015], which is +0.6 % relative.
- No image's peak goes down.
- The four `hdr_grad` images encode identically.
- Encode time is unchanged within noise: about 0.1 s per 0.5 Mpix image, multi-threaded.
- The RMSLE bar ("not worse") is missed by that small, non-significant margin. The cost sits in moonless_golf and satara_night, where keeping the peak forces a two-region block to give up its dim texels.
- **Through the Python bench (2026-10-31),** as `bc6_ispc_veryslow_peak` beside the other BC6H rows (same texconv decode, scored against `.ref.npy`), n = 18 real images against `bc6h_veryslow`: peak ratio +0.078 [±0.090] (4 up, 0 down), mPSNR −0.061 dB [±0.086], RMSLE 0.00003 worse [±0.00012] (3 images better, 6 worse). That reproduces the small, non-significant RMSLE miss of the `.f16` study above. Against NVTT Production it keeps +2.32 dB [±0.96] mPSNR (17/18) and ties on RMSLE and peak ratio. The 4 `hdr_grad` images score identically to veryslow.
- Without the 32× contrast gate (first attempt), the guard re-encodes many ordinary blocks: mean mPSNR −0.168 dB and RMSLE +0.00023. Other effort levels do not use this mode.

## Settings that change nothing

- NVTT 3 BC7: `-fast` and the default give byte-identical output, and so do `-production` and `-highest` (39 of 39 files). The same holds for BC6H `-production` and `-highest`.
- NVTT's GPU and CPU paths at Normal give different bytes but the same quality (±0.01 dB).
- NVTT's `-normal` flag does not change the compressed data. It only sets the non-standard `DDPF_NORMAL` header flag.
- Compressonator BC6H `-Quality 0.05` and `1.0` give the same metrics, at 14× the time.

## Not re-measured

Round 3 was paused to free the CPU. These claims rest on the small round-1 and round-2 sets only:

- Compressonator `-Quality 1.0` on BC7 (opaque, cutout, smooth alpha, data, normals, stray alpha) and on BC6H.
- Smooth gradients, speed and the mode-usage counts.

## Limitations

- The new images all come from ambientCG and Poly Haven: mostly procedural PBR textures, not hand-painted or UI art.
- The smooth-alpha set covers only 15 material families (31 of its 100 images are road-line decals), so its family intervals are wide.
- Metrics are PSNR in 8-bit code values, box-window SSIM, angular error, mPSNR and RMSLE. No perceptual metric such as SSIMULACRA2 was used.
- Only mip 0 was scored. Normal-map scores are pure angle, without shading.
- Timings come from single runs on one machine. Round 3 timings are not comparable with rounds 1 and 2.

## What Tessalith does because of this

- **bc7e at `veryslow` (level 5) is the default BC7 encoder.** Level 5 matches level 6 at 1.5× the speed. Level 4 is not offered, because level 3 beats it on gradients. The levels are in [`crates/engine/src/encode.rs`](../../crates/engine/src/encode.rs) and the default in [`presets/sdc-base.toml`](../../presets/sdc-base.toml). The `fast-preview` preset uses `fast`.
- **The vendored encoders reproduce the bench.** Spike S3 matched the bench's bc7e level 5 result on `rgb_real` (mean 50.9771 dB, 39 of 39 block payloads byte-identical) and rgbcx BC4 bit for bit ([DECISIONS](../../.claude/DECISIONS.md), Spike S3 and Phase 0 exit).
- **rgbcx level 18 for BC1 and BC3, rgbcx for BC4 and BC5, ISPC `veryslow` for BC6H.** BC3 is never picked to save space, because it is the same size as BC7.
- **The BC1 decoder model is one batch-wide setting, default Ideal**, with NVIDIA, AMD and IdealRound4 selectable ([DECISIONS](../../.claude/DECISIONS.md) Note F). The bench's `-r` gain applies when the decoder rounds.
- **BC4/BC5 search radius 5 is the default; radius 4 is the `fast` setting** used by the compact and preview presets; radius 10 is `max` ([`crates/encoders-sys/src/rgbcx.rs`](../../crates/encoders-sys/src/rgbcx.rs)). rgbcx is patched to round its BC4 palette like the decoder; the patch and its test are described in [`README-VENDOR.md`](../../crates/encoders-sys/vendor/README-VENDOR.md).
- **Normals are BC7, not BC5**, because Skyrim does not rebuild Z. BC4 or BC5 on a normal map is the blocking warning `FMT-BC5-NORMAL`. BC5 is the default only for `_flow` and the user-picked `generic_two_channel`.
- **Perceptual mode is off by default** and refused for anything but colour without data in alpha (`FMT-PERCEPTUAL-DATA`).
- **The `remove` alpha policy drops the alpha channel before mips and encoding.** Source alpha whose lowest value is 251 or more is treated as opaque (`ALPHA-NEAR-OPAQUE`, [DECISIONS](../../.claude/DECISIONS.md) Note L).
- **Exact alpha 255 where alpha is read.** The encode stage always applies a lossless fix to BC7 mode-4/5 blocks whose input alpha is 255. With `alpha.exact_opaque` on, a patched bc7e restricts mode-6 p-bits to 1 so alpha decodes to exactly 255. Spike S5 measured the RGB cost of that constraint at −0.515 dB on `rgb_real` and −1.020 dB on `rgb_grad`, too much to turn on everywhere, so it is on by default only for `_m` and environment masks. Spike S6 found 7 of 28 real `_m` files with 40% or more of their alpha off 255 without it, so turning it off on a BC7 `_m` is the blocking code `ALPHA-EXACT-OFF-M` ([DECISIONS](../../.claude/DECISIONS.md) Notes A and D, Spikes S5 and S6; [`crates/encoders-sys/src/bc7_fixup.rs`](../../crates/encoders-sys/src/bc7_fixup.rs)).
- **NVTT is an opt-in quality option for BC7 and BC6H, at Production only**, run as the user's own `nvcompress.exe`. Spike S7 showed that feeding each engine-built mip level to its own `-nomips` process matched a direct encode exactly at mip 0, and that `-nocuda` gives byte-identical output while saving about 0.13 s per process. Spike S7b showed nvcompress reads uncompressed DX10 DDS input as garbage, so levels are fed with a legacy header ([DECISIONS](../../.claude/DECISIONS.md) Spikes S7 and S7b, Note NV; [`crates/engine/src/nvtt.rs`](../../crates/engine/src/nvtt.rs); [NVTT backend](../wiki/NVTT-Backend.md)).
- **BC6H is used only under the `cs` target** (vanilla has no BC6H path), for `hdr_sky` and for a `pbr_emissive` whose source is a float image that peaks above 1.0. Validation warns when the decoded peak ratio falls below 0.8 (`VAL-BC6H-PEAK`).
- **The bench lives on as gates.** `cargo run --release -p sdc-bench -- gate …` reruns the parity and exact-opaque checks; `radius45` reruns the search-radius test.
