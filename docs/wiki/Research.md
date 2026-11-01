# Research behind the defaults

The defaults come from a literature review of BCn encoders, mip filtering and Skyrim's shaders, a local benchmark of about 12,500 encodes over three rounds, and a set of measured spikes. The design is summarised in [How Tessalith came about](../research/design-overview.md). The full research pages, with their tables, are in [docs/research](../research/README.md).

## What matters most

- Picking the right format and colour space for a texture moves quality by 5 to 15 dB, and a Skyrim semantic mistake (wrong tag, lost alpha data) leaves the texture broken. The top BC7 encoders sit within 0.16 to 0.32 dB of each other. The priorities that follow are in the [encoder survey](../research/encoder-survey.md).

## Encoders

- BC7: on the only independent multi-encoder benchmark (Aras Pranckevičius, 2020), bc7e "slow" averaged 47.40 dB luma PSNR against 46.28 dB for ISPC TC "slow", at about 10× the speed. Local runs put bc7e level 5 within ±0.03 dB of level 6 at 0.22 vs 0.28 s/Mpix, and level 4 below level 3 on gradients. Drove: bc7e at `veryslow` (level 5) as the default; level 4 is not offered.
- BC7 `max` (bc7e with patch 0002) beat `veryslow` on a 150-texture Skyrim sample: +0.18 dB RGB on diffuse, +0.037° on normal maps and +0.24 dB on alpha, at 2.7 to 2.9× the time (about 3×). So `max` is an opt-in effort level, not the default.
- DirectXTex/texconv was the weakest BC7 encoder at every setting. Compressonator is unmaintained, never uses BC7 modes 4/5 and writes BC7 headers with required flags missing. Drove: neither is used to encode; texconv is a test oracle only.
- BC1: all tested BC1 encoders fell within 0.66 dB of each other, with rgbcx level 18 on top (38.47 dB). Drove: rgbcx 18 for BC1/BC3; when BC3 is picked is in the [encoder benchmark](../research/encoder-benchmark.md).
- Perceptual (luma-weighted) error raised normal-map angular error by 37%, cost 1.6 dB on packed data and 2.7 to 3.5 dB when alpha carries data. Drove: `encoder.perceptual` is off by default and locked off except for colour maps whose alpha is not data.
- RDO only trades quality for smaller archives (42.25 dB fell to 39.51 dB at λ = 1 in one published run) and never reduces GPU memory. Drove: no RDO.
- A scalar C++ port of bc7e (spike S4) was slower and less exact ([design overview](../research/design-overview.md)). Drove: bc7e ships as prebuilt objects.

## Formats and Skyrim conventions

- Community Shaders reads the `_n` blue channel directly and never rebuilds Z, so a BC5 `_n` decodes with Z = -1 and loses the specular alpha. Benchmarks that favour BC5 for normals rebuild Z, which Skyrim does not. Drove: normals are BC7, and BC4/BC5 on a normal map is a blocking warning ([where BC5 is used](../research/encoder-benchmark.md)).
- Vanilla diffuse stores gamma-encoded colour in UNORM files, and an `_SRGB` tag darkens it in vanilla shaders, CS non-PBR paths and LOD. TruePBR expects albedo tagged `_SRGB`. Drove: `_SRGB` only for TruePBR colour (albedo, `_g`, `_s`, `_f`) and only under `cs`; `_SRGB` under a `lod` path is flagged.
- BC1 alpha on cutouts lost anti-aliased edges and visible RGB quality against BC7 ([encoder benchmark](../research/encoder-benchmark.md)). Drove: BC1 is opaque only; the compact presets use it only for colour without alpha.
- Spike S2: small BC7 textures with full chains ran in game with no crash ([design overview](../research/design-overview.md)). Drove: no small-texture fallback to another format.
- Skyrim SE loads both legacy `BC4U`/`BC5U` and DX10 BC4/BC5 headers, unsigned only. Drove: the [header choice](../research/design-overview.md).

## Alpha

- Stray alpha in a source costs about 1 dB on real images and up to 17 dB on smooth ones. Drove: the `remove` policy drops the alpha channel before any mip or encode step.
- Even with alpha removed, bc7e decoded 254 instead of 255 on 28% of pixels of RGB-only input (ISPC 15%, texconv 2.4%, NVTT 0%), because its mode 6 picks the shared p-bit for RGB alone. Forcing 255 costs 0.5 to 1.0 dB RGB on average (spike S5). On 28 real `_m` files encoded without the fix, 7 had 40% or more of their alpha off 255 (spike S6). Drove: exact-opaque alpha is on by default only for `_m` and env masks, where CS, ENB and PGPatcher read the alpha, and turning it off on a BC7 `_m` needs an explicit acknowledgement.
- A RenderDoc read of the Community Shaders lighting shader (spike S8) showed the complex-material gate exactly: the smallest mip's alpha must be below 251/255, with a separate rule for grey env masks. Drove: the `_m` margin check warns at 247 and applies the shader's own grey rule; near-opaque source alpha uses the same 251 cut-off.
- Box-filtered alpha thins out alpha-tested foliage at distance. Coverage-preserving mips (Castaño's method, also the idea behind AlphaBloom's Auto Coverage) keep the share of texels that pass the alpha test the same at every level. Drove: Tessalith's own solver passes exactly mip 0's share at every level, splits ties in a Bayer order and keeps remapped texels two codes clear of the threshold so BC7 cannot flip them. Default threshold 127.

## Mips

- Gamma-encoded averaging, overshoot and ringing decide the mip filter ([mipmaps and alpha](../research/mipmaps-and-alpha.md)). No published PSNR benchmark of mip filters for game textures exists. Drove: box stays the default for everything, colour is filtered in linear light, and Kaiser is fixed at width 3, α 4 (the NVTT wiki found larger radii ring).
- Building every level from the previous one cost tent 1.1 to 2.7 dB and Mitchell 0.8 to 1.8 dB against a direct Lanczos-8 reference in local tests. Drove: tent, Mitchell and Kaiser build each level directly from mip 0; box stays recursive, where it is exact.

## Normal maps

- Averaging unit normals shortens them, and the lost length is exactly the variance the mip throws away. Unity HDRP, The Order: 1886 and Call of Duty: WWII convert it to extra GGX roughness and renormalise the stored normal. Drove: every normal level is renormalised, and in a TruePBR set the variance goes into roughness with a vMF fit, κ = (3L − L³)/(1 − L²), clamped at t = 0.25 (HDRP's `NORMALMAP_HIGHEST_VARIANCE`).
- Quantisation noise alone on a flat normal map lifts zero roughness at lower mips ([mipmaps and alpha](../research/mipmaps-and-alpha.md)). Drove: a noise floor at L = 0.999 that adds no variance (about a 2° tilt).

## BC4/BC5 search radius

- Upstream rgbcx modelled BC4 decoding with floored interpolants while the GPU and bcdec round to nearest, so a wider search could make results worse (BC5 at radius 10 scored 47.9 dB against 58.0 dB at radius 5). Patching rgbcx to round like the decoder fixed this, and after the patch a wider radius gained almost nothing. Drove: the default radius and the `fast` one. Full tables: [BC4/BC5 search radius](../research/encoder-benchmark.md#bc4bc5-search-radius).

## NVTT 3

- NVTT Production led the built-in encoders on gradients (+3.9 dB) and linear data (+0.3 to +0.4 dB) and kept BC6H highlight peaks at 95% or more, where ISPC BC6H cut one sun's peak to 22%. It took 40 to 60× the time. `-production` and `-highest` gave byte-identical output. Drove: NVTT is [opt-in](../research/encoder-benchmark.md), at Production only.
- NVTT's licence forbids use that would put it under an open-source licence. Drove: Tessalith never links or ships it and runs your installed `nvcompress.exe` as a separate process.
- Spike S7: feeding each pre-built level to its own `nvcompress -nomips` process matched a direct encode exactly at mip 0. BC7 Production runs on the CPU even with CUDA available, and `-nocuda` gave byte-identical output while saving about 0.13 s of CUDA start-up per process. Drove: Tessalith builds the mips and runs nvcompress without CUDA ([how it runs](NVTT-Backend.md#how-it-runs)).
- Spike S7b: nvcompress read an uncompressed DDS with a DX10 header as garbage (5.05 dB) but read the same pixels with a legacy header exactly, and BC6H encoded every level down to 1×1. Drove: levels are fed as legacy-header DDS (BGRA8 for BC7, RGBA16F for BC6H) and no fallback encoder is needed.

## Encoder measurements

The full data behind the charts in the README. "—" means not measured. The charts are drawn by `tools/readme-charts.py`.


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
| BC5 normals (RG PSNR, dB, n = 93) | 47.03 | 46.08 | 46.27 | 44.57 | 44.83 |
| BC5 normals (mean angle error) | 0.41° | 0.47° | 0.45° | 0.55° | 0.56° |
| BC6H HDR (multi-exposure PSNR, dB) | — | 48.00 | —† | 50.38 | 49.62 |
| BC6H brightest highlight kept (worst image) | — | 94% | 29%† | 22% | 29% |

rgbcx beats NVTT on BC1 on 96 of 100 images, and BC3 is a tie. On BC6H the texconv GPU path is slower than ISPC (0.41 against 0.19 s/Mpix) and 2.2 dB lower in mPSNR (4 HDR images).

† Original 4 HDRIs only; Compressonator's mPSNR there was 1.2 dB below ISPC.

## Studies

- How the benchmark encodes score on SSIM and VIF, besides PSNR: [SSIM and VIF on the encoder benchmark](../research/encoder-ssim-vif.md).
- Whether a weighted BC7 can replace BC3 for delicate alpha: [BC3 against BC7 alpha weights](../research/bc7-alpha-weights.md).
