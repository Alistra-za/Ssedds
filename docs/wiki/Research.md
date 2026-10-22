# Research behind the defaults

The defaults come from a literature review of BCn encoders, mip filtering and Skyrim's shaders, a local benchmark of about 12,500 encodes over three rounds, and a set of measured spikes. The per-decision record is [DECISIONS.md](../DECISIONS.md); the design is summarised in [How Tessalith came about](../research/design-overview.md). The full research pages, with their tables, are in [docs/research](../research/README.md).

## What matters most

- Picking the right format and colour space for a texture moves quality by 5 to 15 dB, and a Skyrim semantic mistake (wrong tag, lost alpha data) leaves the texture broken. The top BC7 encoders sit within 0.16 to 0.32 dB of each other. So the priorities are: correct format and tag per map type and target, then correct mips, then the best encoder at a practical speed.

## Encoders

- BC7: on the only independent multi-encoder benchmark (Aras Pranckevičius, 2020), bc7e "slow" averaged 47.40 dB luma PSNR against 46.28 dB for ISPC TC "slow", at about 10× the speed. Local runs put bc7e level 5 within ±0.03 dB of level 6 at 0.22 vs 0.28 s/Mpix, and level 4 below level 3 on gradients. Drove: bc7e at `veryslow` (level 5) as the default; level 4 is not offered.
- BC7 `max` (bc7e with patch 0002) beat `veryslow` on a 150-texture Skyrim sample: +0.18 dB RGB on diffuse, +0.037° on normal maps and +0.24 dB on alpha, at 2.7 to 2.9× the time (about 3×). This confirms the study behind DECISIONS Note BZ: `max` is an opt-in effort level, not the default.
- DirectXTex/texconv was the weakest BC7 encoder at every setting. Compressonator is unmaintained, never uses BC7 modes 4/5 and writes BC7 headers with required flags missing. Drove: neither is used to encode; texconv is a test oracle only.
- BC1: all tested BC1 encoders fell within 0.66 dB of each other, with rgbcx level 18 on top (38.47 dB). BC3 colour is a BC1 block, so BC3 never improves RGB over BC1. Drove: rgbcx 18 for BC1/BC3; BC3 is never picked for size savings, because it is the size of BC7.
- Perceptual (luma-weighted) error raised normal-map angular error by 37%, cost 1.6 dB on packed data and 2.7 to 3.5 dB when alpha carries data. Drove: `encoder.perceptual` is off by default and locked off except for colour maps whose alpha is not data.
- RDO only trades quality for smaller archives (42.25 dB fell to 39.51 dB at λ = 1 in one published run) and never reduces GPU memory. Drove: no RDO.
- A scalar C++ port of bc7e (spike S4) was 3.8 to 4.8× slower and missed the per-image tolerance on 1 of 11 images. Drove: bc7e ships as prebuilt ISPC objects, vendored with SHA-256 hashes, so building needs no ISPC.

## Formats and Skyrim conventions

- Community Shaders reads the `_n` blue channel directly and never rebuilds Z, so a BC5 `_n` decodes with Z = -1 and loses the specular alpha. Benchmarks that favour BC5 for normals rebuild Z, which Skyrim does not. Drove: normals are BC7; BC4/BC5 on a normal map is a blocking warning; BC5 is the default only for `_flow` and the user-picked `generic_two_channel`.
- Vanilla diffuse stores gamma-encoded colour in UNORM files, and an `_SRGB` tag darkens it in vanilla shaders, CS non-PBR paths and LOD. TruePBR expects albedo tagged `_SRGB`. Drove: `_SRGB` only for TruePBR colour (albedo, `_g`, `_s`, `_f`) and only under `cs`; `_SRGB` under a `lod` path is flagged.
- BC1 alpha on cutouts lost anti-aliased edges and measured 6.5 to 11.9 dB worse in visible RGB than BC7. Drove: BC1 is opaque only; the compact presets use it only for colour without alpha.
- Spike S2: 66 BC7 textures of 16 to 128 px with full chains ran in game with no crash. Drove: no small-texture fallback to another format.
- Skyrim SE loads both legacy `BC4U`/`BC5U` and DX10 BC4/BC5 headers, unsigned only. Drove: unsigned DX10 by default, legacy as an option, `ATI1`/`ATI2` never written.

## Alpha

- Stray alpha in a source costs about 1 dB on real images and up to 17 dB on smooth ones. Drove: the `remove` policy drops the alpha channel before any mip or encode step.
- Even with alpha removed, bc7e decoded 254 instead of 255 on 28% of pixels of RGB-only input (ISPC 15%, texconv 2.4%, NVTT 0%), because its mode 6 picks the shared p-bit for RGB alone. Forcing 255 costs 0.5 to 1.0 dB RGB on average (spike S5). On 28 real `_m` files encoded without the fix, 7 had 40% or more of their alpha off 255 (spike S6). Drove: exact-opaque alpha is on by default only for `_m` and env masks, where CS, ENB and PGPatcher read the alpha, and turning it off on a BC7 `_m` needs an explicit acknowledgement.
- A RenderDoc read of the Community Shaders lighting shader (spike S8) showed the complex-material gate exactly: the smallest mip's alpha must be below 251/255, with a separate rule for grey env masks. Drove: the `_m` margin check warns at 247 and applies the shader's own grey rule; near-opaque source alpha uses the same 251 cut-off.
- Box-filtered alpha thins out alpha-tested foliage at distance. Coverage-preserving mips (Castaño's method, also the idea behind AlphaBloom's Auto Coverage) keep the share of texels that pass the alpha test the same at every level. Drove: Tessalith's own solver passes exactly mip 0's share at every level, splits ties in a Bayer order and keeps remapped texels two codes clear of the threshold so BC7 cannot flip them. Default threshold 127.

## Mips

- Averaging gamma-encoded values darkens thin bright detail; box is area-exact and never overshoots; sharper kernels help only LDR colour and ring on HDR and data. No published PSNR benchmark of mip filters for game textures exists. Drove: box stays the default for everything, colour is filtered in linear light, and Kaiser is fixed at width 3, α 4 (the NVTT wiki found larger radii ring).
- Building every level from the previous one cost tent 1.1 to 2.7 dB and Mitchell 0.8 to 1.8 dB against a direct Lanczos-8 reference in local tests. Drove: tent, Mitchell and Kaiser build each level directly from mip 0; box stays recursive, where it is exact.

## Normal maps

- Averaging unit normals shortens them, and the lost length is exactly the variance the mip throws away. Unity HDRP, The Order: 1886 and Call of Duty: WWII convert it to extra GGX roughness and renormalise the stored normal. Drove: every normal level is renormalised, and in a TruePBR set the variance goes into roughness with a vMF fit, κ = (3L − L³)/(1 − L²), clamped at t = 0.25 (HDRP's `NORMALMAP_HIGHEST_VARIANCE`).
- Plain 8-bit quantisation noise on a flat normal map gave 1 − L of 1e-5 to 1e-4 and lifted zero roughness to 0.04 to 0.10 at lower mips. Drove: a noise floor at L = 0.999 that adds no variance (about a 2° tilt).

## BC4/BC5 search radius

- Upstream rgbcx modelled BC4 decoding with floored interpolants while the GPU and bcdec round to nearest, so a wider search could make results worse (BC5 at radius 10 scored 47.9 dB against 58.0 dB at radius 5). Patching rgbcx to round like the decoder fixed this and raised the default-radius result by 3.07 dB (BC4) and 2.61 dB (BC5). After the patch, radius 10 gained only 0.12 dB (BC4) and 0.06 dB (BC5) at about 3× the time, and radius 4 lost under 0.06 dB at 0.7× the time. Drove: radius 5 stays the default, radius 4 is the `fast` setting used by the compact and preview presets. Full tables: [BC4/BC5 search radius](../research/encoder-benchmark.md#bc4bc5-search-radius).

## NVTT 3

- NVTT Production led the built-in encoders on gradients (+3.9 dB) and linear data (+0.3 to +0.4 dB) and kept BC6H highlight peaks at 95% or more, where ISPC BC6H cut one sun's peak to 22%. It took 40 to 60× the time. `-production` and `-highest` gave byte-identical output. Drove: NVTT is an opt-in quality option for BC7 and BC6H at Production only.
- NVTT's licence forbids use that would put it under an open-source licence. Drove: Tessalith never links or ships it and runs your installed `nvcompress.exe` as a separate process.
- Spike S7: feeding each pre-built level to its own `nvcompress -nomips` process matched a direct encode exactly at mip 0. BC7 Production runs on the CPU even with CUDA available, and `-nocuda` gave byte-identical output while saving about 0.13 s of CUDA start-up per process. Drove: Tessalith builds the mip chain itself and always passes `-nocuda`.
- Spike S7b: nvcompress read an uncompressed DDS with a DX10 header as garbage (5.05 dB) but read the same pixels with a legacy header exactly, and BC6H encoded every level down to 1×1. Drove: levels are fed as legacy-header DDS (BGRA8 for BC7, RGBA16F for BC6H) and no fallback encoder is needed. NVTT 3.2.5 is the tested version; others need confirmation.

## Encoder measurements

The full data behind the charts in the README. "—" means not measured. The charts are drawn by `tools/readme-charts.py`.

BC7E for BC7, rgbcx for BC1–BC5, Intel ISPC for BC6H.

### BC7: GPU and CPU encoders, speed and quality

Δ is against bc7e u6, in dB. "Real" means real textures.

| Encoder | Speed (s/Mpix) | Δ RGB (real) | Δ (real alpha) |
|---|--:|--:|--:|
| NVTT Normal (GPU) / texconv GPU | 0.03–0.09 | −1.4 / −1.8 | −0.75 / −2.8 |
| ISPC basic | 0.11 | −0.75 | −0.69 |
| bc7e u4 | 0.12 | −0.39 | −0.36 |
| bc7e u5 | 0.22 | +0.01 | −0.03 |
| bc7e u6 | 0.28 | 0 | 0 |
| bc7e max‡ | ~0.6 | +0.32 | +0.32 |
| ISPC slow | 0.30 | −0.59 | −0.67 |
| NVTT Production/Highest | ~14 | −0.32 | −0.37 |
| texconv CPU (-bc x) | ~16–20 | −1.6 | −2.4 |
| Compressonator q1.0 | ~33 | −0.16 | −1.50 |

‡ `max` is opt-in (`encoder.effort = "max"`). It was measured later, on a 150-texture Skyrim sample, not on the set above. It took 2.7× the time of u5 (`veryslow`) with the two run alternately per image, and 2.9× in separate runs, so about 0.6 s/Mpix on this table's scale. That matches the expected ~3×. Δ is against u6 on that sample: RGB on diffuse (images over 60 dB left out; +0.94 with all 60) and alpha PSNR on the 30 textures with alpha. Against u5 it gains +0.18 dB on diffuse, +0.037° on normal maps and +0.24 dB on alpha. A build ran during part of the alternating run (CPU about 14% busy before it, up to 6 rustc processes); the separate runs had the machine nearly idle. Details: [bc7e tuning](../research/bc7e-tuning.md).

### BC7: bc7e u5 against the other CPU encoders at maximum settings

| Measure | bc7e -u5 | NVTT 3 Production | Compressonator q1.0 | Intel ISPC slow | texconv -bc x |
|---|--:|--:|--:|--:|--:|
| Real colour textures (Δ dB) | +0.03 | −0.27 | −0.16† | −0.44 | −1.47 |
| Smooth gradients, synthetic (Δ dB) | 0.00 | +3.86 | +3.21 | +0.73 | −6.89 |
| Real decals, smooth alpha (Δ dB) | −0.02 | −0.06 | −1.50† | −0.58 | −2.04 |
| Packed data maps (RGB PSNR, dB) | 41.22 | 41.58 | —† | 41.20 | 40.38 |
| Normal maps (mean angle error) | 1.77° | 1.71° | —† | 1.79° | 2.19° |
| Opaque input decoded with alpha 254 | 28%* | 0% | 1.1% | 15% | 2.4% |
| Time per megapixel | 0.22 s | ~14 s | ~33 s | 0.30 s | ~20 s |

\* An optional patch (mode 6 p-bit override, exact opaque) is for when alpha 254 is not acceptable.

† Compressonator q1.0 was not re-measured on the wider set; its values come from the original 4–7 images. On those images it was 0.15 dB behind NVTT on packed data and 0.27° behind on normals. The alpha-254 and timing rows are also from the original run.

### Other formats: best encoders per format

| Format and measure | rgbcx | NVTT 3 | Compressonator | Intel ISPC | texconv (CPU) |
|---|--:|--:|--:|--:|--:|
| BC1 colour (RGB PSNR, dB) | 39.85 | 39.74 | 39.05 | 38.11 | 37.41 |
| BC3 colour + alpha (premultiplied PSNR, dB) | 49.20 | 49.29 | 48.75 | 48.02 | 48.07 |
| BC4 masks (PSNR, dB) | 45.59 | 45.05 | 45.19 | 43.70 | 43.94 |
| BC5 normals (mean angle error) | 0.41° | 0.47° | 0.45° | 0.55° | 0.56° |
| BC6H HDR (multi-exposure PSNR, dB) | — | 48.00 | —† | 50.38 | 49.62 |
| BC6H brightest highlight kept (worst image) | — | 94% | 29%† | 22% | 29% |

rgbcx beats NVTT on BC1 on 96 of 100 images, and BC3 is a tie. On BC6H the texconv GPU path is slower than ISPC (0.41 against 0.19 s/Mpix) and 2.2 dB lower in mPSNR (4 HDR images).

† Original 4 HDRIs only; Compressonator's mPSNR there was 1.2 dB below ISPC.

## SSIM and VIF on the encoder benchmark

Source: local study, SSIM and VIF on the BCn benchmark encodes (scored 2026-10-21). It rescores the existing benchmark encodes with Gaussian SSIM and multi-scale VIFp, and compares the same pairs as the round 3 confidence-interval study. Nothing was re-encoded.

**Method.** SSIM uses an 11×11 Gaussian window (σ 1.5); VIFp uses 4 scales (windows 17, 9, 5, 3). Colour sets are scored on Rec.709 luma (cutout on visible luma, smooth alpha on premultiplied luma). Masks use R, normals use the stored R and G, data uses R, G and B, and alpha is scored on its own. Both metrics are on the 0 to 255 scale. They are not defined for BC6H HDR, so BC6H is not scored. These are the study's mean differences with a verdict from its 95% intervals (t and family-cluster). The tables drop most confidence intervals and show one only where the study printed it in the table. ΔSSIM is ×10³ and ΔVIF is ×10². W/L counts images won and lost by A. **Bold** marks a verdict that differs from the PSNR or angle verdict.

### Bottom line

SSIM and VIF mostly confirm the PSNR rankings: of 55 paired comparisons, SSIM gives the same verdict as PSNR on 46 and VIF on 47. Most disagreements come from channel weighting. On colour, SSIM and VIF see luma only, so they side with luma PSNR. Where there is no luma step (masks, normals, data) the three metrics agree almost perfectly.

| Round 3 said (PSNR) | SSIM / VIF say | Why |
|---|---|---|
| bc7e u5 ≥ u6, +0.03 dB RGB | **u6 better** on 85 of 99 images (both metrics) | Luma PSNR also favours u6 (−0.06 dB). The u5 edge is in chroma. |
| NVTT Production vs bc7e u6: −0.27 dB, but a tie on typical images (median −0.03) | **bc7e better on 81/99 (SSIM) and 89/99 (VIF)**, tight CIs | The luma gap is −0.52 dB, twice the RGB gap. On the structure that SSIM and VIF see, bc7e's lead is real and consistent. |
| bc7e perceptual costs −1.05 dB RGB | **Perceptual wins on 99/99** (SSIM and VIF), the best BC7 encoder on opaque colour by both | The luma-weighted encoder wins luma-weighted metrics. Its RGB cost is chroma error that SSIM_Y and VIF_Y cannot see. |
| BC1: texconv −2.13 dB vs rgbcx L18 | SSIM tie, **VIF: texconv better** (+0.029, 55/45); texconv is the top BC1 encoder in the SSIM/VIF means | Default texconv BC1 is perceptually weighted (the `-bc u` uniform variant ranks last). Luma PSNR is already a tie (−0.17 dB). |
| BC1: NVTT `-highest` vs rgbcx `-r`: rgbcx better by −0.11 dB | Tie on both (VIF leans NVTT, 68/32) | A small RGB difference. On luma it is also a tie. |

![Two bar charts of the SSIM and VIF difference against bc7e u6 on 99 real colour textures. bc7e perceptual is the only encoder above zero, at plus 1.63 SSIM and plus 3.10 VIF. The other encoders are at or below u6, and texconv GPU is furthest below at minus 2.40 VIF.](../img/encoder-ssim-vif.svg)

### BC7 opaque colour (99 images; B = bc7e u6 unless noted)

| A vs B | PSNR Δ dB (verdict) | ΔSSIM×10³ (W/L) | ΔVIF×10² (W/L) | SSIM / VIF verdict | Luma PSNR Δ (verdict) |
|---|---|---|---|---|---|
| bc7e u5 | +0.03 (A) | −0.058 [−0.070, −0.047] (14/85) | −0.10 [−0.13, −0.08] (14/85) | **B / B** | −0.06 (B) |
| bc7e u4 | −0.40 (B) | −0.27 (0/99) | −0.54 (0/99) | B / B | −0.45 (B) |
| NVTT Production | −0.27 (B) | −0.180 [−0.215, −0.144] (18/81) | −0.46 [−0.55, −0.37] (10/89) | B / B | −0.52 (B) |
| ISPC slow | −0.44 (B) | −0.28 (2/97) | −0.49 (7/92) | B / B | −0.58 (B) |
| ISPC basic | −0.59 (B) | −0.32 (2/97) | −0.62 (6/93) | B / B | −0.77 (B) |
| NVTT Normal GPU | −1.36 (B) | −0.80 (0/99) | −1.74 (0/99) | B / B | −1.83 (B) |
| texconv `-bc x` | −1.47 (B) | −0.98 (0/99) | −2.05 (0/99) | B / B | −1.95 (B) |
| texconv GPU | −1.76 (B) | −1.23 (0/99) | −2.40 (0/99) | B / B | −2.10 (B) |
| texconv `-bc x` vs default | +0.04 (A) | +0.006 (49/42) | −0.00 (49/42) | **tie / tie** | +0.01 (tie) |
| bc7e u6 perceptual (round-3 metric: luma PSNR) | +3.69 (A) | +1.63 [+1.32, +1.93] (99/0) | +3.10 [+2.63, +3.56] (99/0) | A / A | +3.69 (A) |
| bc7e u6 perceptual (round-3 metric: RGB PSNR) | −1.05 (B) | same as above | same as above | **A / A** | +3.69 (A) |
| Compressonator q1.0 (n = 7 only) | −0.16 (tie) | −0.12 (2/5) | −0.16 (2/5) | tie / tie | −0.24 (tie) |

### BC7 with alpha

| Set, metric | A vs B | PSNR Δ (verdict) | ΔSSIM×10³ (W/L) | ΔVIF×10² (W/L) | SSIM / VIF |
|---|---|---|---|---|---|
| Cutout, visible colour | NVTT Prod vs bc7e u6 | −0.14 (B) | −0.088 (16/84) | −0.32 (10/90) | B / B |
| Cutout, alpha (coverage error in round 3) | NVTT Prod vs bc7e u6 | +0.005 pp (A) | +1.35 (99/1) | +0.33 (82/18) | A / A |
| Smooth, premultiplied | NVTT Prod vs bc7e u6 | −0.06 (tie) | +0.13 [+0.04, +0.22], cluster [−0.17, +0.39] (49/51) | −0.06 (43/57) | tie / tie |
| Smooth, premultiplied | ISPC slow vs bc7e u6 | −0.58 (B) | −0.49 (2/98) | −0.45 (6/94) | B / B |
| Smooth, premultiplied | texconv `-bc x` vs bc7e u6 | −2.04 (B) | −0.91 (4/96) | −2.42 (2/98) | B / B |
| Packed, RGB | NVTT Prod vs bc7e u6 | +0.04 (tie) | −0.006 (41/59) | −0.12 (26/74) | tie / tie |
| Packed, A | NVTT Prod vs bc7e u6 | +0.27 (tie) | +0.28 [+0.15, +0.42] (70/30) | +0.04 (62/38) | **A** / tie |
| Packed, RGB | bc7e perceptual vs u6 | −2.89 (B) | −3.84 (16/84) | −4.58 (13/87) | B / B |
| Stray, RGB | NVTT `-noalpha` vs default | +0.58 (A) | +0.33 (98/0) | +0.65 (98/0) | A / A |

### BC1, BC3, BC4

| Format | A vs B | PSNR Δ dB (verdict) | ΔSSIM×10³ (W/L) | ΔVIF×10² (W/L) | SSIM / VIF | Luma PSNR |
|---|---|---|---|---|---|---|
| BC1 | NVTT `-highest` vs rgbcx L18 | +0.19 (tie) | +0.49 (93/7) | +0.72 (91/9) | **A / A** | +0.31 (A) |
| BC1 | NVTT `-highest` vs rgbcx L18 `-r` | −0.11 (B) | −0.035 (58/42) | +0.09 (68/32) | **tie / tie** | −0.10 (tie) |
| BC1 | rgbcx `-r` vs default | +0.30 (A) | +0.53 (95/5) | +0.63 (93/7) | A / A | +0.41 (A) |
| BC1 | Compressonator q1 vs rgbcx L18 | −0.49 (B) | −2.43 (8/92) | −1.42 (9/91) | B / B | −0.49 (B) |
| BC1 | texconv vs rgbcx L18 | −2.13 (B) | +0.68 [−0.71, +2.06] (55/45) | +2.88 [+0.41, +5.34], cluster [+1.00, +5.63] (55/45) | **tie / A** | −0.17 (tie) |
| BC3 premul | NVTT `-highest` vs rgbcx `-r` | +0.09 (tie) | −0.07 (36/64) | +0.08 (38/62) | tie / tie | — |
| BC3 premul | Compressonator q1 vs rgbcx `-r` | −0.45 (tie: cluster CI spans 0) | −0.55, cluster [−1.04, −0.21] (20/80) | −1.04, cluster [−2.12, +0.02] (24/76) | **B** / tie | — |
| BC3 premul | ISPC vs rgbcx `-r` | −1.18 (B) | −0.83 (0/100) | −1.84 (0/100) | B / B | — |
| BC4 | `-hr10` vs rgbcx | +0.13 (A) | +0.037 (93/5) | +0.14 (98/0) | A / A | — |
| BC4 | NVTT `-highest` / Compressonator / ISPC / texconv vs rgbcx | −1.29 / −1.16 / −2.98 / −2.93 (all B) | −0.80 / −0.66 / −1.88 / −1.45 | −0.94 / −0.83 / −2.75 / −2.13 | all B / B | — |

### Normals (stored R and G only)

| A vs B | Angle Δ° (verdict) | ΔSSIM×10³ (W/L) | ΔVIF×10² (W/L) | SSIM / VIF |
|---|---|---|---|---|
| BC5 Compressonator vs bc7enc_rdo | −0.039 (B) | −0.63 (7/90) | −0.77 (9/88) | B / B |
| BC5 NVTT `-highest` vs bc7enc_rdo | −0.063 (B) | −0.78 (2/95) | −1.02 (4/93) | B / B |
| BC5 `-hr10` vs default | +0.007 (A) | +0.064 (92/5) | +0.17 (97/0) | A / A |
| BC5 ISPC / texconv vs bc7enc_rdo | −0.141 / −0.144 (B) | −2.12 / −1.48 | −3.13 / −2.33 | B / B |
| **BC7 NVTT Prod vs BC5 bc7enc_rdo** (round 3: both with Z rebuilt) | −0.56 (B) | −17.8 (1/96) | −16.5 (2/95) | B / B |
| Same pair, round-2 biased method (BC7 stored Z) | +0.10 (tie) | −17.8 | −16.5 | **B / B** |
| BC7 bc7e u6 vs NVTT Prod | −0.051 (B) | −1.29 (2/95) | −0.71 (9/88) | B / B |
| BC7 perceptual vs linear bc7e u6 | −0.363 (B) | −12.1 (13/87) | −4.40 (30/70) | B / B |
| Normal + height: bc7e u6 vs NVTT, R,G | −0.061 (B) | −1.77 (11/89) | −0.78 (12/88) | B / B |
| Normal + height: bc7e u6 vs NVTT, height A | −0.26 dB (B) | −4.16 (9/91) | −0.62 (23/77) | B / B |

### BC7 linear data (B = NVTT Production)

| Set | A | PSNR Δ dB (verdict) | ΔSSIM×10³ (W/L) | ΔVIF×10² (W/L) | SSIM / VIF |
|---|---|---|---|---|---|
| Data, opaque | bc7e u6 | −0.56 (B) | −0.97 (3/97) | −0.70, cluster [−1.47, +0.11] (8/92) | B / **tie** |
| Data, opaque | ISPC slow | −0.74 (B) | −1.28 (0/100) | −0.95 (2/98) | B / B |
| Data, opaque | texconv GPU | −4.25 (B) | −16.8 (0/100) | −9.96 (2/98) | B / B |
| Data, opaque | bc7e perceptual vs linear | −2.37 (B) | −8.45 (5/95) | −4.59 (3/97) | B / B |
| Data + alpha, RGB | bc7e u6 | −0.27 (B) | −1.27 (4/96) | −0.62 (8/92) | B / B |
| Data + alpha, A | bc7e u6 | −0.32 (B) | −2.17 (21/79) | −0.89 (18/82) | B / B |
| Data + alpha, RGB | texconv GPU | −3.26 (B) | −32.2 (0/100) | −10.7 (1/99) | B / B |

### Not scored

- BC7 Compressonator q1.0 on round 3 colour, cutout and alpha: the encodes were never made, so only 4 to 7 old images exist.
- BC7 Compressonator and texconv CPU on packed and stray sets, and Compressonator on round 2 sets: skipped in round 3.
- texconv CPU on normal + height: 40 of 96 images missing, so those ranks use the 60 images where all variants exist.
- BC6H HDR (all variants): SSIM and VIF are not defined for HDR without a tonemapping choice.
- Synthetic gradient sets and the round 1 variants that round 3 dropped: same scope as round 3.
- texconv CPU outside real colour: only 2 to 5 old images.

## BC3 against BC7 alpha weights: study results

Source: local study, 2026-10-21, and DECISIONS Note CP. It asks whether BC7 with a higher alpha error weight can replace BC3 for textures with delicate alpha, and keep BC7's better RGB. BC7 is the built-in bc7e at its default level (veryslow), with linear weights 1,1,1,w for w = 1, 2, 4, 8 and 16. BC3 is the bundled rgbcx (level 18, `-r`). w1 is today's Linear. The dropdown offers Alpha ×2, ×8 and ×16, and Perceptual. The chart is in the [GUI guide](GUI-Guide.md#bc7-weighting-against-bc3).

These tables copy the study's values. They leave out the 95% confidence intervals and the per-image worst cases that the study also reports. PSNR is in dB, alpha SSIM is 0 to 1, alpha max error is in codes, and exact-0 and exact-255 are the share of source texels at 0 or 255 that decode exactly. Each *Means* table is the mean over the class. Each *Where BC7 beats BC3* table counts the images where the BC7 setting beats BC3.

### Answer in one table

| Class | n | BC3 alternative to offer | What it gives against BC3 | Ranked options |
|---|---|---|---|---|
| `alpha_real` (colour + smooth alpha) | 96 | **BC7 w8** | Alpha PSNR better on 85/96 (mean +4.2 dB), alpha SSIM better on 77/96, max alpha error 3.5 codes lower, RGB +5.6 dB (better on 88/96), premultiplied +3.7 dB | 1. BC7 w8. 2. BC7 w4 (milder: SSIM 62/96, RGB +6.1 dB on 94/96). 3. BC3 kept as a manual choice for faint, low-contrast alpha such as fingerprint decals: no weight up to 16 closes them. bc7e perceptual is beaten by w8 on every mean. Where alpha must decode to exactly 0 and 255, NVTT-style BC3 still wins on every image. |
| `n_spec` (`_n`, specular in alpha), mip 0 and mip 2 | 30 + 30 | **BC7 w16**, a compromise | Normal error still 0.28° lower than BC3 (24/30). Specular is 1.1 dB below BC3 (BC7 wins 10/30) and alpha SSIM 0.993 against 0.997. Mip 2 gives the same picture. | 1. BC7 w16. 2. BC3, when specular fidelity matters more than the normal: +8.8 dB alpha over today's BC7, for 1.1° more normal error (+67%). 3. BC7 as today: the best normals, but specular at 37 dB with a mean worst-texel error of 102 codes. |
| `rmaos_spec` (`_rmaos`, specular in alpha) | 24 | **BC7 w8**, not BC3 | Against today's BC7: +4.8 dB specular and a mean worst-texel error of 35 → 18 codes, for −1.0 dB RGB. It is still +5.6 dB RGB over BC3. | 1. BC7 w8. 2. BC7 as today. 3. BC3 only if exact specular 255 matters: BC3 keeps 91% of 255 texels exact, BC7 65 to 73%. BC3 costs 6.5 dB on roughness and 6.2 dB on AO. |
| `m_height` (complex `_m`, height in alpha) | 30 (proxy) | **BC3 still** (see Follow-up) | Measured on a proxy: real `_rmaos` RGB plus real `_p` height. BC7 w16 is still −2.25 dB height (9/30 wins) and loses exact 0/255 on nearly every image, for +3.4 dB RGB. | 1. BC3. 2. BC7 w16. 3. BC7 today (−8.5 dB height). |

Follow-up on `m_height`:

| Item | Headline | Options, ranked |
|---|---|---|
| `m_height` (real `_rmaos` RGB + real `_p` height in alpha, n = 30) | **BC3 still.** No BC7 weight up to 16 reaches BC3 on height. w16 is −2.25 dB alpha PSNR (9/30 wins), loses alpha SSIM on 28/30 and loses exact-0/255 on all but one image. BC3 keeps 98 to 100% of the 0 and 255 texels exact; BC7 keeps 75 to 97%. BC3 costs 6.1 dB RGB. | 1. BC3 (rgbcx or NVTT: rgbcx has +0.7 dB alpha PSNR, NVTT pins 100% of extremes). 2. BC7 w16 as a compromise: −2.3 dB height, +3.4 dB RGB over BC3. 3. BC7 today: −8.5 dB height, +6.1 dB RGB. Exact-opaque adds almost nothing here (+4.6 pt exact-255, mean). |

### Colour with smooth alpha (`alpha_real`, 96 textures)

Means:

| Setting | RGB PSNR | premul PSNR | alpha PSNR | alpha SSIM | alpha max err | exact-0 share (%) | exact-255 share (%) |
|---|---|---|---|---|---|---|---|
| BC3 rgbcx L18 -r | 41.00 | 49.17 | 47.24 | 0.9978 | 17.35 | 89.35 | 55.87 |
| BC3 round-3 rgbcx -r | 41.00 | 49.04 | 46.43 | 0.9967 | 17.59 | 89.72 | 56.84 |
| BC3 round-3 NVTT -highest | 40.96 | 49.29 | 47.38 | 0.9976 | 17.90 | 100.00 | 100.00 |
| BC7 w1 (today) | 48.40 | 53.04 | 47.93 | 0.9951 | 22.27 | 88.35 | 54.71 |
| BC7 w2 | 47.74 | 53.29 | 49.39 | 0.9970 | 18.26 | 90.95 | 61.87 |
| BC7 w4 | 47.13 | 53.17 | 50.56 | 0.9979 | 15.96 | 92.13 | 63.69 |
| BC7 w8 | 46.60 | 52.87 | 51.40 | 0.9984 | 13.91 | 92.69 | 67.00 |
| BC7 w16 | 46.17 | 52.53 | 51.93 | 0.9987 | 13.02 | 93.06 | 66.01 |
| BC7 perceptual | 46.54 | 52.81 | 51.17 | 0.9983 | 14.39 | 92.70 | 65.27 |

Where BC7 beats BC3:

| Setting | alpha PSNR wins | alpha SSIM wins | both | RGB PSNR Δ vs BC3 | RGB PSNR wins | alpha PSNR Δ | alpha SSIM Δ (×1000) |
|---|---|---|---|---|---|---|---|
| BC7 w1 (today) | 61/96 | 30/96 | 30/96 | +7.40 dB | 96/96 | +0.69 | -2.65 |
| BC7 w2 | 73/96 | 46/96 | 46/96 | +6.74 dB | 95/96 | +2.14 | -0.77 |
| BC7 w4 | 77/96 | 62/96 | 62/96 | +6.13 dB | 94/96 | +3.32 | +0.17 |
| BC7 w8 | 85/96 | 77/96 | 77/96 | +5.61 dB | 88/96 | +4.16 | +0.67 |
| BC7 w16 | 86/96 | 80/96 | 80/96 | +5.18 dB | 84/96 | +4.69 | +0.97 |
| BC7 perceptual | 82/96 | 71/96 | 71/96 | +5.54 dB | 91/96 | +3.93 | +0.54 |

### Normal map with specular in alpha (`n_spec`, 30 textures, mip 0)

Means:

| Setting | angle | RGB PSNR | alpha PSNR | alpha SSIM | alpha max err | exact-0 share (%) | exact-255 share (%) |
|---|---|---|---|---|---|---|---|
| BC3 rgbcx L18 -r | 2.806 | 33.47 | 46.06 | 0.9966 | 16.73 | 47.08 | 47.97 |
| BC7 w1 (today) | 1.682 | 37.54 | 37.24 | 0.9537 | 102.47 | 83.07 | 37.14 |
| BC7 w2 | 1.868 | 36.79 | 39.28 | 0.9695 | 75.47 | 83.09 | 37.91 |
| BC7 w4 | 2.081 | 36.00 | 41.39 | 0.9807 | 55.90 | 81.60 | 43.31 |
| BC7 w8 | 2.308 | 35.22 | 43.36 | 0.9883 | 41.10 | 80.19 | 47.44 |
| BC7 w16 | 2.523 | 34.53 | 44.96 | 0.9929 | 32.17 | 76.90 | 51.26 |

Where BC7 beats BC3:

| Setting | alpha PSNR wins | alpha SSIM wins | both | angle Δ vs BC3 | angle wins | alpha PSNR Δ | alpha SSIM Δ (×1000) |
|---|---|---|---|---|---|---|---|
| BC7 w1 (today) | 0/30 | 0/30 | 0/30 | +1.124 ° | 30/30 | -8.83 | -42.94 |
| BC7 w2 | 0/30 | 0/30 | 0/30 | +0.938 ° | 29/30 | -6.78 | -27.09 |
| BC7 w4 | 1/30 | 0/30 | 0/30 | +0.725 ° | 29/30 | -4.67 | -15.95 |
| BC7 w8 | 3/30 | 1/30 | 1/30 | +0.498 ° | 27/30 | -2.70 | -8.30 |
| BC7 w16 | 10/30 | 3/30 | 3/30 | +0.283 ° | 24/30 | -1.11 | -3.70 |

### The same normal maps at mip 2 (`n_spec_mip2`, 30 textures)

Means:

| Setting | angle | RGB PSNR | alpha PSNR | alpha SSIM | alpha max err | exact-0 share (%) | exact-255 share (%) |
|---|---|---|---|---|---|---|---|
| BC3 rgbcx L18 -r | 3.560 | 31.62 | 44.29 | 0.9962 | 13.23 | 83.60 | 31.58 |
| BC7 w1 (today) | 2.212 | 35.40 | 34.79 | 0.9423 | 64.37 | 86.63 | 30.27 |
| BC7 w2 | 2.496 | 34.49 | 37.18 | 0.9664 | 49.47 | 89.06 | 32.62 |
| BC7 w4 | 2.788 | 33.67 | 39.58 | 0.9808 | 37.60 | 92.39 | 59.13 |
| BC7 w8 | 3.052 | 33.00 | 41.59 | 0.9886 | 25.70 | 92.56 | 54.53 |
| BC7 w16 | 3.270 | 32.44 | 42.95 | 0.9925 | 20.23 | 94.48 | 55.51 |

Where BC7 beats BC3:

| Setting | alpha PSNR wins | alpha SSIM wins | both | angle Δ vs BC3 | angle wins | alpha PSNR Δ | alpha SSIM Δ (×1000) |
|---|---|---|---|---|---|---|---|
| BC7 w1 (today) | 0/30 | 0/30 | 0/30 | +1.347 ° | 30/30 | -9.51 | -53.87 |
| BC7 w2 | 0/30 | 0/30 | 0/30 | +1.064 ° | 30/30 | -7.11 | -29.79 |
| BC7 w4 | 1/30 | 0/30 | 0/30 | +0.772 ° | 29/30 | -4.71 | -15.38 |
| BC7 w8 | 1/30 | 1/30 | 1/30 | +0.508 ° | 25/30 | -2.70 | -7.62 |
| BC7 w16 | 8/30 | 5/30 | 4/30 | +0.290 ° | 22/30 | -1.35 | -3.63 |

### Packed `_rmaos` with specular in alpha (`rmaos_spec`, 24 textures)

Means:

| Setting | R PSNR | G PSNR | B PSNR | RGB PSNR | alpha PSNR | alpha SSIM | alpha max err | exact-0 share (%) | exact-255 share (%) |
|---|---|---|---|---|---|---|---|---|---|
| BC3 rgbcx L18 -r | 37.54 | 51.51 | 38.49 | 38.64 | 58.98 | 0.9995 | 11.42 | 65.82 | 91.20 |
| BC7 w1 (today) | 44.00 | 53.12 | 44.64 | 45.27 | 49.92 | 0.9908 | 35.46 | 97.09 | 64.80 |
| BC7 w2 | 43.71 | 52.50 | 44.36 | 44.95 | 51.79 | 0.9939 | 28.58 | 96.44 | 67.65 |
| BC7 w4 | 43.39 | 51.72 | 44.08 | 44.62 | 53.36 | 0.9959 | 23.33 | 95.69 | 70.44 |
| BC7 w8 | 43.02 | 51.05 | 43.78 | 44.25 | 54.74 | 0.9972 | 17.67 | 95.14 | 72.16 |
| BC7 w16 | 42.58 | 50.50 | 43.42 | 43.82 | 56.07 | 0.9981 | 15.33 | 78.66 | 73.05 |

Where BC7 beats BC3:

| Setting | alpha PSNR wins | alpha SSIM wins | both | RGB PSNR Δ vs BC3 | RGB PSNR wins | alpha PSNR Δ | alpha SSIM Δ (×1000) |
|---|---|---|---|---|---|---|---|
| BC7 w1 (today) | 0/24 | 0/24 | 0/24 | +6.63 dB | 24/24 | -9.06 | -8.75 |
| BC7 w2 | 2/24 | 0/24 | 0/24 | +6.31 dB | 24/24 | -7.19 | -5.59 |
| BC7 w4 | 2/24 | 1/24 | 1/24 | +5.98 dB | 23/24 | -5.62 | -3.61 |
| BC7 w8 | 4/24 | 1/24 | 1/24 | +5.61 dB | 23/24 | -4.24 | -2.36 |
| BC7 w16 | 5/24 | 1/24 | 1/24 | +5.18 dB | 22/24 | -2.91 | -1.46 |

### Complex `_m` with height in alpha (`m_height`, 30 textures, proxy)

Means:

| Setting | R PSNR | G PSNR | B PSNR | RGB PSNR | alpha PSNR | alpha SSIM | alpha max err | exact-0 share (%) | exact-255 share (%) |
|---|---|---|---|---|---|---|---|---|---|
| BC3 rgbcx L18 -r | 37.94 | 65.98 | 37.51 | 38.67 | 53.81 | 0.9990 | 9.90 | 98.07 | 98.99 |
| BC3 NVTT -highest | 37.93 | 65.98 | 37.50 | 38.66 | 53.14 | 0.9988 | 10.67 | 100.00 | 100.00 |
| BC7 w1 (today) | 43.77 | 52.71 | 43.65 | 44.75 | 45.27 | 0.9850 | 36.13 | 74.98 | 86.43 |
| BC7 w2 | 43.24 | 51.38 | 43.03 | 44.11 | 47.02 | 0.9897 | 29.53 | 83.66 | 88.66 |
| BC7 w4 | 42.64 | 50.32 | 42.34 | 43.41 | 48.73 | 0.9929 | 21.77 | 93.91 | 90.89 |
| BC7 w8 | 42.03 | 49.61 | 41.62 | 42.70 | 50.27 | 0.9952 | 18.13 | 95.69 | 90.63 |
| BC7 w16 | 41.44 | 49.13 | 40.94 | 42.02 | 51.56 | 0.9969 | 14.27 | 97.13 | 93.80 |
| BC7 w1 + exact-opaque | 43.77 | 52.71 | 43.65 | 44.75 | 45.29 | 0.9851 | 36.13 | 74.98 | 90.99 |
| BC7 [4,1,2,1] | 46.09 | 48.83 | 43.49 | 45.10 | 43.14 | 0.9773 | 48.03 | 67.90 | 83.16 |

Where BC7 beats BC3:

| Setting | alpha PSNR wins (rgbcx / NVTT) | alpha SSIM wins | exact-0 wins | exact-255 wins | RGB Δ vs rgbcx | RGB wins |
|---|---|---|---|---|---|---|
| BC7 w1 (today) | 0/30 / 0/30 | 0/30 / 0/30 | 0/14 / 0/14 | 0/15 / 0/15 | +6.08 dB | 30/30 |
| BC7 w2 | 0/30 / 1/30 | 0/30 / 0/30 | 0/14 / 0/14 | 0/15 / 0/15 | +5.44 dB | 30/30 |
| BC7 w4 | 2/30 / 1/30 | 1/30 / 1/30 | 1/14 / 0/14 | 0/15 / 0/15 | +4.75 dB | 29/30 |
| BC7 w8 | 4/30 / 3/30 | 1/30 / 2/30 | 1/14 / 0/14 | 0/15 / 0/15 | +4.04 dB | 29/30 |
| BC7 w16 | 9/30 / 11/30 | 2/30 / 3/30 | 1/14 / 0/14 | 1/15 / 0/15 | +3.35 dB | 27/30 |
| BC7 w1 + exact-opaque | 0/30 / 0/30 | 0/30 / 0/30 | 0/14 / 0/14 | 0/15 / 0/15 | +6.08 dB | 30/30 |
| BC7 [4,1,2,1] | 0/30 / 0/30 | 0/30 / 0/30 | 0/14 / 0/14 | 0/15 / 0/15 | +6.43 dB | 30/30 |

The `m_height` rows are a proxy: a real `_rmaos` for RGB plus a real `_p` height written into alpha. Its Means table also lists NVTT BC3, BC7 w1 with exact-opaque, and a `[4,1,2,1]` weight mode that the owner dropped (Note CP12). In the `m_height` wins table, BC3 means rgbcx / NVTT.
