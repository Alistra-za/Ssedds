# Research behind the defaults

The defaults come from a literature review of BCn encoders, mip filtering and Skyrim's shaders, a local benchmark of about 12,500 encodes over three rounds, and a set of measured spikes. The per-decision record is [DECISIONS.md](../DECISIONS.md); the design is summarised in [How Tessalith came about](../research/design-overview.md). The full research pages, with their tables, are in [docs/research](../research/README.md).

## What matters most

- Picking the right format and colour space for a texture moves quality by 5 to 15 dB, and a Skyrim semantic mistake (wrong tag, lost alpha data) leaves the texture broken. The top BC7 encoders sit within 0.16 to 0.32 dB of each other. So the priorities are: correct format and tag per map type and target, then correct mips, then the best encoder at a practical speed.

## Encoders

- BC7: on the only independent multi-encoder benchmark (Aras Pranckevičius, 2020), bc7e "slow" averaged 47.40 dB luma PSNR against 46.28 dB for ISPC TC "slow", at about 10× the speed. Local runs put bc7e level 5 within ±0.03 dB of level 6 at 0.22 vs 0.28 s/Mpix, and level 4 below level 3 on gradients. Drove: bc7e at `veryslow` (level 5) as the default; level 4 is not offered.
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
