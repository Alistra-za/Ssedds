# SSIM and VIF on the encoder benchmark

Written 2026-10-21 (commit dbf54808).

The encoder benchmark's encodes rescored with Gaussian SSIM and multi-scale VIF. Moved from the wiki's [Research](../wiki/Research.md) page.

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
