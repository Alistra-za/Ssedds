# BC3 against BC7 alpha weights

Written 2026-10-21 (commit dbf54808).

Whether BC7 with a higher alpha error weight can replace BC3 for textures with delicate alpha. Moved from the wiki's [Research](../wiki/Research.md) page.

Source: local study, 2026-10-21, and DECISIONS Note CP. It asks whether BC7 with a higher alpha error weight can replace BC3 for textures with delicate alpha, and keep BC7's better RGB. BC7 is the built-in bc7e at its default level (veryslow), with linear weights 1,1,1,w for w = 1, 2, 4, 8 and 16, and in a follow-up on 2026-10-22, 32 and 64. BC3 is the bundled rgbcx (level 18, `-r`). w1 is today's Linear. The dropdown offers Alpha ×2, ×8 and ×16, and Perceptual. The chart is in the [GUI guide](../wiki/GUI-Texture-Panel.md#bc7-weighting-against-bc3).

These tables copy the study's values. They leave out the 95% confidence intervals and the per-image worst cases that the study also reports. PSNR is in dB, alpha SSIM is 0 to 1, alpha max error is in codes, and exact-0 and exact-255 are the share of source texels at 0 or 255 that decode exactly. Each *Means* table is the mean over the class. Each *Where BC7 beats BC3* table counts the images where the BC7 setting beats BC3.

### Answer in one table

| Class | n | BC3 alternative to offer | What it gives against BC3 | Ranked options |
|---|---|---|---|---|
| `alpha_real` (colour + smooth alpha) | 96 | **BC7 w8** | Alpha PSNR better on 85/96 (mean +4.2 dB), alpha SSIM better on 77/96, max alpha error 3.5 codes lower, RGB +5.6 dB (better on 88/96), premultiplied +3.7 dB | 1. BC7 w8. 2. BC7 w4 (milder: SSIM 62/96, RGB +6.1 dB on 94/96). 3. BC3 kept as a manual choice for faint, low-contrast alpha such as fingerprint decals: no weight up to 16 closes them. bc7e perceptual is beaten by w8 on every mean. Where alpha must decode to exactly 0 and 255, NVTT-style BC3 still wins on every image. |
| `n_spec` (`_n`, specular in alpha), mip 0 and mip 2 | 30 + 30 | **BC7 w16**, a compromise | Normal error still 0.28° lower than BC3 (24/30). Specular is 1.1 dB below BC3 (BC7 wins 10/30) and alpha SSIM 0.993 against 0.997. Mip 2 gives the same picture. | 1. BC7 w16. 2. BC3, when specular fidelity matters more than the normal: +8.8 dB alpha over today's BC7, for 1.1° more normal error (+67%). 3. BC7 as today: the best normals, but specular at 37 dB with a mean worst-texel error of 102 codes. |
| `rmaos_spec` (`_rmaos`, specular in alpha) | 24 | **BC7 w8**, not BC3 | Against today's BC7: +4.8 dB specular and a mean worst-texel error of 35 → 18 codes, for −1.0 dB RGB. It is still +5.6 dB RGB over BC3. | 1. BC7 w8. 2. BC7 as today. 3. BC3 only if exact specular 255 matters: BC3 keeps 91% of 255 texels exact, BC7 65 to 73%. BC3 costs 6.5 dB on roughness and 6.2 dB on AO. |
| `m_height` (complex `_m`, height in alpha) | 30 (proxy) | **BC3 still** (see Follow-up) | Measured on a proxy: real `_rmaos` RGB plus real `_p` height. BC7 w16 is still −2.25 dB height (9/30 wins) and loses exact 0/255 on nearly every image, for +3.4 dB RGB. | 1. BC3. 2. BC7 w16. 3. BC7 today (−8.5 dB height). |

**Weights ×32 and ×64 (follow-up, 2026-10-22).** No weight up to 64 beats BC3 on alpha for every texture in any class. Alpha keeps rising with each doubling and never plateaus, and each doubling costs 0.25 to 0.55 dB of RGB or about 0.17° of normal angle.

- On `n_spec`, w64 passes BC3's mean specular, but its normals are by then worse than BC3's.
- On `rmaos_spec` and `m_height`, BC3 still leads on alpha SSIM on 23/24 and 27/30 textures.
- On `alpha_real`, w32 and w64 add only +0.3 and +0.45 dB of alpha over w16.

The dropdown stays at ×16. w32 and w64 are in the bench only.

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
| BC7 w32 (study only) | 45.84 | 52.23 | 52.23 | 0.9989 | 12.42 | 93.06 | 66.39 |
| BC7 w64 (study only) | 45.58 | 52.00 | 52.38 | 0.9990 | 12.24 | 93.08 | 66.37 |
| BC7 perceptual | 46.54 | 52.81 | 51.17 | 0.9983 | 14.39 | 92.70 | 65.27 |

Where BC7 beats BC3:

| Setting | alpha PSNR wins | alpha SSIM wins | both | RGB PSNR Δ vs BC3 | RGB PSNR wins | alpha PSNR Δ | alpha SSIM Δ (×1000) |
|---|---|---|---|---|---|---|---|
| BC7 w1 (today) | 61/96 | 30/96 | 30/96 | +7.40 dB | 96/96 | +0.69 | -2.65 |
| BC7 w2 | 73/96 | 46/96 | 46/96 | +6.74 dB | 95/96 | +2.14 | -0.77 |
| BC7 w4 | 77/96 | 62/96 | 62/96 | +6.13 dB | 94/96 | +3.32 | +0.17 |
| BC7 w8 | 85/96 | 77/96 | 77/96 | +5.61 dB | 88/96 | +4.16 | +0.67 |
| BC7 w16 | 86/96 | 80/96 | 80/96 | +5.18 dB | 84/96 | +4.69 | +0.97 |
| BC7 w32 (study only) | 87/96 | 82/96 | 82/96 | +4.84 dB | 82/96 | +4.98 | +1.14 |
| BC7 w64 (study only) | 87/96 | 82/96 | 82/96 | +4.59 dB | 79/96 | +5.14 | +1.24 |
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
| BC7 w32 (study only) | 2.706 | 33.95 | 46.02 | 0.9953 | 24.87 | 75.62 | 54.68 |
| BC7 w64 (study only) | 2.851 | 33.49 | 46.64 | 0.9963 | 19.47 | 74.28 | 53.82 |

Where BC7 beats BC3:

| Setting | alpha PSNR wins | alpha SSIM wins | both | angle Δ vs BC3 | angle wins | alpha PSNR Δ | alpha SSIM Δ (×1000) |
|---|---|---|---|---|---|---|---|
| BC7 w1 (today) | 0/30 | 0/30 | 0/30 | +1.124 ° | 30/30 | -8.83 | -42.94 |
| BC7 w2 | 0/30 | 0/30 | 0/30 | +0.938 ° | 29/30 | -6.78 | -27.09 |
| BC7 w4 | 1/30 | 0/30 | 0/30 | +0.725 ° | 29/30 | -4.67 | -15.95 |
| BC7 w8 | 3/30 | 1/30 | 1/30 | +0.498 ° | 27/30 | -2.70 | -8.30 |
| BC7 w16 | 10/30 | 3/30 | 3/30 | +0.283 ° | 24/30 | -1.11 | -3.70 |
| BC7 w32 (study only) | 17/30 | 6/30 | 6/30 | +0.100 ° | 16/30 | -0.04 | -1.37 |
| BC7 w64 (study only) | 21/30 | 13/30 | 13/30 | -0.045 ° | 11/30 | +0.58 | -0.34 |

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
| BC7 w32 (study only) | 3.460 | 31.94 | 43.80 | 0.9946 | 15.87 | 95.81 | 57.35 |
| BC7 w64 (study only) | 3.640 | 31.45 | 44.38 | 0.9956 | 14.40 | 94.86 | 57.27 |

Where BC7 beats BC3:

| Setting | alpha PSNR wins | alpha SSIM wins | both | angle Δ vs BC3 | angle wins | alpha PSNR Δ | alpha SSIM Δ (×1000) |
|---|---|---|---|---|---|---|---|
| BC7 w1 (today) | 0/30 | 0/30 | 0/30 | +1.347 ° | 30/30 | -9.51 | -53.87 |
| BC7 w2 | 0/30 | 0/30 | 0/30 | +1.064 ° | 30/30 | -7.11 | -29.79 |
| BC7 w4 | 1/30 | 0/30 | 0/30 | +0.772 ° | 29/30 | -4.71 | -15.38 |
| BC7 w8 | 1/30 | 1/30 | 1/30 | +0.508 ° | 25/30 | -2.70 | -7.62 |
| BC7 w16 | 8/30 | 5/30 | 4/30 | +0.290 ° | 22/30 | -1.35 | -3.63 |
| BC7 w32 (study only) | 13/30 | 9/30 | 9/30 | +0.100 ° | 18/30 | -0.49 | -1.59 |
| BC7 w64 (study only) | 19/30 | 12/30 | 12/30 | -0.080 ° | 13/30 | +0.09 | -0.59 |

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
| BC7 w32 (study only) | 42.05 | 50.09 | 43.01 | 43.32 | 57.31 | 0.9987 | 13.79 | 78.52 | 73.43 |
| BC7 w64 (study only) | 41.47 | 49.70 | 42.57 | 42.77 | 58.51 | 0.9991 | 13.12 | 78.14 | 74.13 |
| BC7 w8 + exact opaque | | | | 44.25 | 55.01 | 0.9973 | 17.67 | 95.14 | 91.64 |
| BC7 w16 + exact opaque | | | | 43.82 | 56.43 | 0.9982 | 15.33 | 78.66 | 92.53 |
| BC7 w64 + exact opaque (study only) | | | | 42.77 | 59.10 | 0.9992 | 13.12 | 78.14 | 93.60 |

Where BC7 beats BC3:

| Setting | alpha PSNR wins | alpha SSIM wins | both | RGB PSNR Δ vs BC3 | RGB PSNR wins | alpha PSNR Δ | alpha SSIM Δ (×1000) |
|---|---|---|---|---|---|---|---|
| BC7 w1 (today) | 0/24 | 0/24 | 0/24 | +6.63 dB | 24/24 | -9.06 | -8.75 |
| BC7 w2 | 2/24 | 0/24 | 0/24 | +6.31 dB | 24/24 | -7.19 | -5.59 |
| BC7 w4 | 2/24 | 1/24 | 1/24 | +5.98 dB | 23/24 | -5.62 | -3.61 |
| BC7 w8 | 4/24 | 1/24 | 1/24 | +5.61 dB | 23/24 | -4.24 | -2.36 |
| BC7 w16 | 5/24 | 1/24 | 1/24 | +5.18 dB | 22/24 | -2.91 | -1.46 |
| BC7 w32 (study only) | 6/24 | 1/24 | 1/24 | +4.68 dB | 22/24 | -1.67 | -0.83 |
| BC7 w64 (study only) | 11/24 | 1/24 | 1/24 | +4.13 dB | 22/24 | -0.47 | -0.42 |

Why BC3 keeps the lead on this class, even at w64:

- **BC3 gives alpha its own half of the block.** Its alpha is a separate 64-bit block with two 8-bit endpoints and 8 levels between them, so it never competes with colour.
- **BC7 fits all four channels into one 128-bit block.** Its alpha either shares the colour's indices (mode 6) or gets its own indices with only 4 levels (mode 5). The weight only changes which error the encoder prefers; it adds no bits.
- **The RGB here is three unrelated maps** (roughness, metal, AO), so colour already needs most of BC7's bits. That is also why BC7's RGB lead is so large (+6.6 dB at w1).
- **The specular is smooth and sits near 255.** BC4-style alpha fits that well: BC3's mean is 58.98 dB, and it keeps 91% of 255 texels exact against BC7's 65 to 74% at every weight.

BC3 pays 4 to 7 dB of RGB for it, mostly in roughness and AO. The mode explanation follows from the formats; the study did not count which modes bc7e chose.

**With Exact opaque alpha** (follow-up, 2026-10-22): the flag pins blocks whose alpha is 255 everywhere, which is 9% of blocks in this class.
- **Alpha ×8 + exact opaque** keeps 91.6% of specular-255 texels exact against BC3's 91.2%, and keeps +5.6 dB of RGB.
- At w64 the flag lifts mean alpha to 59.10 dB against BC3's 58.98, winning 12/24 images.
- BC3 still wins alpha SSIM on 22/24.
- The flag leaves RGB unchanged here (R/G/B columns left blank: same as the row without it).

Over all classes: no weight with the flag beats BC3 on every image. Exact-255 reaches BC3's level on `m_height` from w32 (99.6%). On `n_spec` the flag does nothing, because there are almost no all-255 blocks. On opaque `_rmaos` it costs 0.20 dB RGB.

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
| BC7 w32 (study only) | 40.94 | 48.82 | 40.33 | 41.44 | 52.46 | 0.9980 | 12.43 | 98.15 | 95.05 |
| BC7 w64 (study only) | 40.52 | 48.41 | 39.84 | 40.95 | 52.99 | 0.9984 | 11.70 | 98.76 | 95.27 |
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
| BC7 w32 (study only, rgbcx only) | 13/30 | 2/30 | — | — | +2.77 dB | 26/30 |
| BC7 w64 (study only, rgbcx only) | 16/30 | 3/30 | 11/14 | — | +2.28 dB | 24/30 |
| BC7 w1 + exact-opaque | 0/30 / 0/30 | 0/30 / 0/30 | 0/14 / 0/14 | 0/15 / 0/15 | +6.08 dB | 30/30 |
| BC7 [4,1,2,1] | 0/30 / 0/30 | 0/30 / 0/30 | 0/14 / 0/14 | 0/15 / 0/15 | +6.43 dB | 30/30 |

The `m_height` rows are a proxy: a real `_rmaos` for RGB plus a real `_p` height written into alpha. Its Means table also lists NVTT BC3, BC7 w1 with exact-opaque, and a `[4,1,2,1]` weight mode that the owner dropped (Note CP12). In the `m_height` wins table, BC3 means rgbcx / NVTT.
