# BCn encoders: what the published evidence says

This page summarises a literature review of DDS/BCn encoders, done before any Tessalith code was written. It asked which encoder gives the best quality for each BC format, and how sure we can be. The review found that the published data had large gaps, so a local benchmark was run next. Its results are on [Local encoder benchmark](encoder-benchmark.md), and where the two disagree, the benchmark wins.

## The question

- Which encoder, at which setting, gives the highest quality for BC1, BC3, BC4, BC5, BC6H and BC7?
- How much does the encoder matter compared with the choice of format?
- How do alpha, perceptual weighting and rate-distortion optimisation (RDO) change the answer?

## How it was done

- Read the format specifications, encoder source code and official documentation.
- Collected every published measurement that compares encoders, and noted who made it.
- Covered bc7e and rgbcx (Rich Geldreich's bc7enc_rdo), Intel ISPC Texture Compressor, NVIDIA Texture Tools (NVTT) 2 and 3, DirectXTex/texconv, AMD Compressonator, Oodle Texture, ConvectionKernels, icbc, Ludicon Spark, Betsy and a few wrappers.

Each claim was labelled by its source: an independent measurement, a measurement by the encoder's author or a competitor, documentation or source code, an unsupported claim, or the review's own inference. PSNR values can only be compared inside one study. Studies differ in metric (RGB, luma, RGBA, RMSE, SSIMULACRA2), test images and whether perceptual modes were on.

## Main result: the format matters more than the encoder

- BC1 encoders were all within **0.66 dB** of each other in the 2020 independent benchmark by Aras Pranckevičius.
- BC3's colour block *is* a BC1 block, so BC3 never improves RGB over BC1. It only adds smooth alpha.
- On plain RGB PSNR the top BC7 encoders sat only a few tenths of a dB apart.
- Picking the wrong format costs far more (the local benchmark later measured 5 to 15 dB).

## BC7

### How the format works

Every BC7 block is 16 bytes for 16 texels and uses one of eight modes.

| Mode | Subsets | Partitions | Colour endpoints | Alpha | P-bits | Index bits |
|---|---|---|---|---|---|---|
| 0 | 3 | 16 | RGB 4.4.4 + p | none (255) | per endpoint | 3 |
| 1 | 2 | 64 | RGB 6.6.6 + p | none | shared per subset | 3 |
| 2 | 3 | 64 | RGB 5.5.5 | none | none | 2 |
| 3 | 2 | 64 | RGB 7.7.7 + p | none | per endpoint | 2 |
| 4 | 1 | – | RGB 5.5.5 | separate 6-bit | none | 2 and 3 |
| 5 | 1 | – | RGB 7.7.7 | separate 8-bit | none | 2 and 2 |
| 6 | 1 | – | RGBA 7.7.7.7 + p | combined | per endpoint | 4 |
| 7 | 2 | 64 | RGBA 5.5.5.5 + p | combined | per endpoint | 2 |

- Modes 0 to 3 have no alpha and decode it as 255.
- A block with even one texel below 255 cannot use modes 0 to 3. Mode 7 is then the only partitioned mode left. This is how alpha takes quality away from RGB, block by block.
- Mode 6 alone is not enough. In Geldreich's data (about 2018), a mode-6-only encode scored 42.8 dB average RGB PSNR against 45.8 to 46.0 dB with the partitioned modes.

### Published measurements

The only independent multi-encoder BC7 study is Aras Pranckevičius's (December 2020): 20 Unity textures, luma PSNR with perceptual modes on where available, 6-core i9-8950HK. NVTT, DirectXTex and Compressonator were not in it.

| Encoder / preset | Mpix/s | Average luma PSNR (dB) |
|---|---|---|
| ISPC TC basic | 4.53 | 46.07 |
| ISPC TC slow | 1.29 | 46.28 |
| bc7enc 64.4 (max) | 3.82 | 45.48 |
| bc7e fast | 30.97 | 46.18 |
| bc7e basic | 20.65 | 46.75 |
| bc7e slow | 13.04 | 47.40 |

- Per image, bc7e slow minus ISPC slow ranged from +3.10 dB to −0.77 dB (mean +1.13 dB). On the four normal maps the two were within ±0.26 dB.
- Part of bc7e's lead comes from its perceptual mode, which ISPC TC lacks, scored on a luma metric.

Geldreich's 2018 runs with perceptual off (31 images, linear RGB PSNR, author data) show how close the top settings get:

| Encoder | Setting | Time (s) | RGB PSNR (dB) |
|---|---|---|---|
| ISPC TC | slow | 355.3 | 46.77 |
| Geldreich's ISPC encoder | uber 1 | 86.4 | 46.72 |
| Geldreich's ISPC encoder | uber 4, 64 partitions, p-bit search | 292.5 | 46.96 |
| Basis BC7 | – | 365.5 | 47.16 |
| NVTT 2 AVPCL | – | 28,061.9 | 47.14 |

NVTT 2's AVPCL is not NVTT 3's encoder, so these numbers do not describe NVTT 3.

### What the review could not answer

- No public measurement compared NVTT 3 (Highest), Compressonator (`-Quality 1.0`), texconv and bc7e on one test set.
- No credible multi-encoder benchmark of BC7 with real alpha existed.
- NVTT 3 does not document what its "Highest" level does. NVIDIA staff did state that BC7 Production and Highest run on the CPU only.
- No study isolated the value of a proper p-bit search.

These gaps are why the local benchmark was run.

### Why "maximum quality" means different things

| Encoder at its top setting | Partitions fully evaluated | Endpoint search | Metric |
|---|---|---|---|
| ISPC TC `slow` | all 64 (modes 1/3) | 4 refinement iterations | linear |
| texconv `-bc x` | top 25% (16 of 64) | perturbation plus ±5 search; p-bits by majority vote | linear |
| bc7e `slowest` | top 4 | "uber" level 4, full p-bit search | linear or perceptual |
| Compressonator 1.0 | threshold-mapped, internals not examined | – | – |
| NVTT 3 Highest | undocumented, CPU only | – | – |

The review ranked the quality levers at top settings roughly as: whether partitioned modes and the mode 4/5 rotations are searched at all; how many partitions get a full evaluation; endpoint refinement depth; p-bit search; and last the metric, which changes what counts as best.

## Alpha in BC7

- An all-255 alpha costs nothing if the encoder classifies blocks one by one. bc7e, ISPC TC, DirectXTex and Compressonator all do.
- Stray alpha is the trap. bc7e sends any block whose lowest alpha is below 255 to its alpha path. A "nearly opaque" texture with alpha 254 loses modes 0 to 3 in those blocks. The review's advice was to strip or snap near-opaque alpha before encoding.
- None of the open encoders weights RGB error by alpha, so they spend bits on colour hidden under alpha 0.
- NVTT 3 before 3.2.2 (July 2023) had its opaque and transparent BC7 compressors swapped.

## BC1 and BC3

From Aras's 2020 DXTC data (same images and metric as his BC7 test):

| Encoder / level | Average luma PSNR (dB) | Mpix/s |
|---|---|---|
| rgbcx 18 | 38.47 | 4.7 |
| rgbcx 10 | 38.41 | 36 |
| icbc 8 | 38.30 | 156 |
| stb_dxt | 38.01 | 244 |
| ISPC | 37.81 | 654 |

- BC1's 3-colour mode decodes the fourth palette entry as transparent black. bc7enc_rdo turns this on by default.
- BC1 decoding is not bit-identical across GPU vendors. rgbcx can encode for an NVIDIA, AMD or "ideal" decoder.

## BC4 and BC5

- No cross-encoder BC4/BC5 benchmark existed. The review expected competent encoders to converge, because BC4 is a one-dimensional fit. The local benchmark showed that this is not true (see [the benchmark](encoder-benchmark.md#bc4-single-channel-masks)).
- Published advice puts tangent-space normals in BC5 with Z rebuilt in the shader. This does not apply to Skyrim; see [Design overview](design-overview.md#skyrim-texture-rules).

## BC6H

- Narkowicz's table (5 HDR images, RMSLE): ISPC TC slow/veryslow best on 4 of 5 images; DirectXTex worse than ISPC fast on every image, by up to about 1.8×.
- Ludicon's 2026 chart (27 HDR images, SSIMULACRA2, values read from an image, made by a competitor): Oodle about 91.0, Spark GPU about 90.5, ISPC TC about 88.6, Basis about 87.6, the "NVIDIA" series about 85.2.
- Unsigned BC6H (UF16) should be used for data that is never negative.

## Perceptual weighting

- bc7e's perceptual mode measures error in a scaled YCbCr space (default weights Y 128, Cr 64, Cb 16, A 256) and also skips modes 4 and 5 on opaque blocks.
- It helps sRGB colour on luma metrics (Geldreich reports up to 2 dB in luma terms).
- It harms normals, packed data and masks, where "luma" means nothing. DirectXTex's documentation also says to use uniform weighting when RGB holds other data.

## Rate-distortion optimisation

RDO always costs quality. In one published bc7enc_rdo run, PSNR fell from 42.25 dB with no RDO to 41.77 at λ = 0.1, 39.51 at λ = 1 and 29.18 at λ = 20. RDO only shrinks the file after LZ compression (for example inside an archive). It never reduces the DDS size or GPU memory.

## Maintenance status (October 2026)

| Encoder | Status |
|---|---|
| ISPC TC | Archived (last push 2024-09-23). |
| bc7e | Repository archived (last push 2026-01-28). Its latest code lives in bc7enc_rdo. |
| bc7enc_rdo | Not archived, but no code change since March 2021. |
| Compressonator | Last release 4.5.52 (31 January 2024). |
| NVTT 3 | Last release 3.2.5.1 / Exporter 2024.1.1 (15 August 2024). |
| DirectXTex | Active, but its 2024 to 2026 releases contain no BC encoder changes. |

Open-source BC7 quality has not moved since 2021. No fixes are expected from the leading open encoders, so a project that uses them should keep its own pinned copy.

## DDS container rules

- BC6H, BC7, any `_SRGB` format and arrays need the DX10 header. BC1 to BC3 are most compatible with legacy FourCCs.
- BC4 and BC5 have ambiguous legacy names (`ATI1`/`BC4U`, `ATI2`/`BC5U`).
- bc7enc_rdo never writes `_SRGB` formats and makes no mipmaps, so any tool that uses its encoders has to write the DDS file itself.

## What Tessalith does because of this

- **Correct format first.** The priority order in the design is: correct format and tag per map type, then correct mips, then the best encoder at a practical speed ([Design overview](design-overview.md)).
- **bc7e for BC7, rgbcx for BC1/BC3/BC4/BC5, ISPC TC for BC6H.** These are vendored and pinned, with SHA-256 hashes, in [`crates/encoders-sys/vendor/`](../../crates/encoders-sys/vendor/README-VENDOR.md). The default bc7e level came from the local benchmark, not from this review.
- **texconv and Compressonator are not used to encode.** texconv is used only as a test oracle for headers.
- **Perceptual mode is off by default** and refused except for colour maps whose alpha is not data (`FMT-PERCEPTUAL-DATA`, [`crates/engine/src/policy.rs`](../../crates/engine/src/policy.rs)).
- **No RDO.**
- **Stray alpha is removed before mips and encoding** when the policy is `remove` (the default for maps whose alpha is not used).
- **Tessalith writes its own DDS headers** ([`crates/engine/src/dds.rs`](../../crates/engine/src/dds.rs)): DX10 for BC6H, BC7 and every `_SRGB` format, legacy FourCC for BC1/BC3 UNORM under the `auto` header setting, and `ATI1`/`ATI2` never.
- **NVTT 3 is an optional external program** that the user installs; see [Local encoder benchmark](encoder-benchmark.md#what-tessalith-does-because-of-this) and the [NVTT backend](../wiki/NVTT-Backend.md) page.

## Sources

The main sources were the [Aras 2020 benchmark](https://aras-p.info/blog/2020/12/08/Texture-Compression-in-2020/), Geldreich's posts ([2018 showdown](http://richg42.blogspot.com/2018/04/bc7-showdown-ispctexcomp-vs-my-ispc.html), [showdown 2](http://richg42.blogspot.com/2018/04/bc7-showdown-2-basis-ispc-vs-nvidia.html), [2021/02](https://richg42.blogspot.com/2021/02/)), the [bc7e README](https://github.com/BinomialLLC/bc7e), [bc7enc_rdo](https://github.com/richgel999/bc7enc_rdo), [ISPC TC](https://github.com/GameTechDev/ISPCTextureCompressor), [DirectXTex](https://github.com/microsoft/DirectXTex/wiki/Compress), the [Compressonator docs](https://compressonator.readthedocs.io/en/latest/), [Microsoft's BC7 format reference](https://learn.microsoft.com/en-us/windows/win32/direct3d11/bc7-format), [GPURealTimeBC6H](https://github.com/knarkowicz/GPURealTimeBC6H), [Ludicon Spark 1.4](https://www.ludicon.com/castano/blog/2026/09/spark-1-4/) and NVIDIA's [Texture Tools pages and forum posts](https://developer.nvidia.com/gpu-accelerated-texture-compression).
