<p align="center"><img src="docs/assets/tessalith-emblem.png" alt="Tessalith emblem" width="480"></p>

<h1 align="center">Tessalith</h1>

<p align="center">Turns texture source images into DDS files that load and look right in Skyrim SE/AE.</p>

---

Tessalith is a texture converter for Skyrim Special and Anniversary Edition modders. You give it a folder of source images (PNG, TGA, TIFF, EXR, PSD and more). It works out what each file is from its name, groups the files into material sets, packs loose maps into the channel layouts Skyrim expects, builds the mips and encodes each texture to the format its shader needs.

It is meant for texture artists making new textures. It is not a tool for bulk "fixing" an installed game folder.

Tessalith ships two programs that share one engine: `sdc` (command line) and the Tessalith GUI (`tessalith.exe`). A folder you set up and export in the GUI converts to the same bytes with `sdc convert`, because both read the same per-project `sdc.toml`. There are three targets: `cs` (Community Shaders, including TruePBR; the default), `cpm` (CPM / ENB complex material) and `vanilla`.

Windows x64 only. Version 0.7.0. GPL-3.0-or-later.

## Features

### Encoding

- BC7 through bc7e (from bc7enc_rdo, Rich Geldreich), effort `ultrafast` to `slowest`, default `veryslow`.
- BC1, BC3, BC4 and BC5 through rgbcx: level 18 for BC1 and the BC3 colour block, search radius 5 for BC4, BC5 and the BC3 alpha block. BC6H (UF16) through Intel's ISPC Texture Compressor at `veryslow`.
- Uncompressed `rgba8`, `r8`, `rg8` and `rgba16f` outputs.
- Two local patches to the vendored encoders. One makes bc7e's opaque blocks decode to exactly 255 alpha when asked (used for `_m` maps). The other makes rgbcx's BC4/BC5 search round the way the GPU does, which gained about 3 dB on BC4 and 2.6 dB on BC5.
- Optional NVIDIA Texture Tools 3 backend for BC7 and BC6H. Tessalith runs a `nvcompress.exe` you installed yourself, one process per mip level at Production quality. It is never bundled and never a default.
- DDS headers written by Tessalith's own writer: by default legacy FourCC for BC1, BC3 and RGBA8 UNORM and DX10 for everything else (BC4/BC5 legacy only on request), `_SRGB` only for TruePBR colour under `cs`.

### Mips, size and alpha

- Full mip chains to 1×1, built in float. Colour is filtered in linear light and re-encoded.
- Box filter by default. Tent, Mitchell and Kaiser are available for sRGB colour without alpha weighting; tent also for HDR.
- Alpha-weighted mips for cutout and blended colour, with colour dilation under fully transparent texels.
- Alpha coverage preservation for alpha-tested textures (foliage, hair, grass), on by default for cutout and cutout + blend. Each mip passes exactly the same share of texels as mip 0, with a dither for tied values so thin strands do not vanish at distance.
- Normal maps renormalised at every level. In a TruePBR set, the variance the normal mips lose is baked into the `_rmaos`/`_rfaos` roughness. A `_cnr` coat normal bakes into its own coat roughness.
- Optional resize: down to a power of two, or half size when the larger side is above 512.
- Alpha policy per texture: remove, cutout, blend, cutout + blend, or data. Source alpha whose minimum is 251 or more is treated as opaque.

### Texture sets, packing and shader modes

- Skyrim suffixes (`_n`, `_m`, `_p`, `_g`, `_s`, `_rmaos`, `_rfaos`, `_cnr`, `_f`, `_flow`, `_msn` and more) and common exporter names (`_basecolor`, `_albedo`, `_normalgl`, `_roughness`, `_metallic`, `_ao`, ...) are recognised. Outputs always get Skyrim names.
- Loose single-channel maps in a set are packed per target: `_rmaos` or `_rfaos` plus `_p` for TruePBR, a complex `_m` for CPM. A channel with no source gets a neutral value or a fill value you choose (0 to 255).
- Shader modes per set: default, PBR, Advanced skin, envmap, complex material, glow, parallax, skin, hair, eye, multilayer, landscape and complex grass. The mode sets the slots and the defaults for its members (for example hair diffuse is cutout with coverage).
- Community Shaders Advanced skin sets: `_rfaos`, `_sk` and `_wet`, with the UNORM albedo that shader expects.
- RNM normal layers: `_bump`, `_bump2`, `_detailnorm`, `_detailnorm2` and `_bent` (plus masks) blend into the set's `_n` or `_cnr` before the mips, with per-layer strength and order. The `_bent` layer can blend in the set's AO.
- Complex grass: one 1:2 image, colour and alpha on top, normal and specular below. Each half is mipped on its own and the detection corner is checked after encoding.
- HDR emissive: a TruePBR `_g` from a float source that peaks above 1.0 becomes BC6H; any other TruePBR `_g` is BC7 sRGB.

### Inputs

- PNG (8 and 16 bit), TGA, TIFF (8, 16 and 32-bit float), OpenEXR, Radiance HDR, BMP, JPEG and WebP.
- Photoshop `.psd` and `.psb`, Krita `.kra` and OpenRaster `.ora`. Tessalith reads the flattened image the file stores; it never composites layers. A PSD composite that Photoshop matted against white is un-matted.
- DDS files are read for inspection, validation and preview. Re-encoding a DDS is opt-in per file.
- Input aliases in `sdc.toml` let a badly named file be read under a corrected name. Nothing on disk is renamed.

### Presets and project config

- Built-in presets: `cs-quality` (default), `cpm-quality`, `vanilla-quality`, `cs-compact`, `vanilla-compact` and `fast-preview`. Compact uses BC1 for opaque colour and half size for most other maps.
- User presets in `%APPDATA%\sdc\presets\*.toml`, with inheritance (`base`), rules and suffix aliases.
- `sdc presets list | show | diff | export | validate | schema` for working with them.
- Per-project `sdc.toml`: target, preset, defaults, rules, per-file settings (`[files]`) and input aliases (`[inputs]`). Every resolved value records where it came from.

### Validation and output records

- Every output is decoded and checked: PSNR per mip, alpha coverage, the smallest-mip alpha, the Community Shaders complex-material gate on `_m`, BC6H highlight peaks, and PGPatcher's non-white rule.
- A batch manifest in `<output>\_sdc\manifest.jsonl` records the engine and encoder versions, the resolved profile and the source hash. `--on-existing if-changed` (the default) re-encodes only what changed. Optional per-file `.dds.json` sidecars.
- `sdc validate` checks DDS files from any tool. `sdc patch-header` fixes header-only faults (mip count, flags, legacy/DX10, tag) without touching the pixel data.
- JSON Lines events (`--json`) and published JSON Schemas for the profile, preset, config, events, manifest and DDS reports.

### The GUI

- Three columns, left to right: textures, material sets, and the settings of the selection (each value shows its source).
- Side-by-side or split preview of source and encoded mips, with channel isolation, a difference heatmap and the same bytes Export will write.
- Bulk editing of several textures or sets, copy settings from one texture to others, a rename dialog for inputs and outputs, and undo (200 steps).
- Set editor with PGPatcher slot cards, shared and duplicated slots, slot locks and a shader picker.
- TruePBR settings per set, and a TruePBR JSON editor window over the whole PGPatcher JSON, including sections that delete trishapes. Export writes `<export>\PBRNIFPatcher\<name>.json`.
- 3D preview (switch from 2D): shows a real game mesh that uses the set, patched in memory the way PGPatcher 1.2.0 would patch it, drawn with your installed Community Shaders `Lighting.hlsl` and your CS settings.

### Mod scanning

- Reads a Mod Organizer 2 instance and profile (or a plain folder): loose files and SE BSA archives, every NIF shape's shader, flags and texture slots, and which mod's copy the game loads. PGPatcher output is tracked as a layer derived from its source mesh.
- Reads the active plugins: which records use a texture set or mesh, plugin overrides, files plugins name that are missing, files nothing uses. Records nothing reaches are hidden behind a toggle.
- The scan feeds the converter. A colour map takes its alpha meaning (blend, test, both, none, and the test threshold) from the meshes that use it. The meshes also vote for a set's shader. In the GUI, a grass texture used by a GRAS record that passes the complex-grass shape tests gets a High-confidence `complex_grass` suggestion (not under `vanilla`; the CLI never does this).
- PGPatcher interaction: a renamed TruePBR set writes a `rename` with `match_diffuse`, the form real PGPatcher 1.2.0 applies (checked on a real load order). JSON paths PGPatcher would never match block export.

### Not yet

- No prebuilt release, installer or CI yet; build from source.
- No preset picker in the GUI (set `preset = "<id>"` in `sdc.toml`).
- Texture edits inside the GUI (planned as M3c).
- GIMP `.xcf` and Paint.NET `.pdn` are not read, since they store no flattened image.
- Not planned: BC1 1-bit alpha, RDO, cubemap assembly, Legendary Edition output, Linux.

## Research behind the defaults

The defaults come from a literature review of BCn encoders, mip filtering and Skyrim's shaders, a local benchmark of about 1,770 encodes, and a set of measured spikes. The per-decision record is [docs/DECISIONS.md](docs/DECISIONS.md); the design is in [the design spec](docs/superpowers/specs/2026-10-08-skyrim-dds-converter-design.md).

### What matters most

- Picking the right format and colour space for a texture moves quality by 5 to 15 dB, and a Skyrim semantic mistake (wrong tag, lost alpha data) leaves the texture broken. The top BC7 encoders sit within 0.16 to 0.32 dB of each other. So the priorities are: correct format and tag per map type and target, then correct mips, then the best encoder at a practical speed.

### Encoders

- BC7: on the only independent multi-encoder benchmark (Aras Pranckevičius, 2020), bc7e "slow" averaged 47.40 dB luma PSNR against 46.28 dB for ISPC TC "slow", at about 10× the speed. Local runs put bc7e level 5 within ±0.03 dB of level 6 at 0.22 vs 0.28 s/Mpix, and level 4 below level 3 on gradients. Drove: bc7e at `veryslow` (level 5) as the default; level 4 is not offered.
- DirectXTex/texconv was the weakest BC7 encoder at every setting. Compressonator is unmaintained, never uses BC7 modes 4/5 and writes BC7 headers with required flags missing. Drove: neither is used to encode; texconv is a test oracle only.
- BC1: all tested BC1 encoders fell within 0.66 dB of each other, with rgbcx level 18 on top (38.47 dB). BC3 colour is a BC1 block, so BC3 never improves RGB over BC1. Drove: rgbcx 18 for BC1/BC3; BC3 is never picked for size savings, because it is the size of BC7.
- Perceptual (luma-weighted) error raised normal-map angular error by 37%, cost 1.6 dB on packed data and 2.7 to 3.5 dB when alpha carries data. Drove: `encoder.perceptual` is off by default and locked off except for colour maps whose alpha is not data.
- RDO only trades quality for smaller archives (42.25 dB fell to 39.51 dB at λ = 1 in one published run) and never reduces GPU memory. Drove: no RDO.
- A scalar C++ port of bc7e (spike S4) was 3.8 to 4.8× slower and missed the per-image tolerance on 1 of 11 images. Drove: bc7e ships as prebuilt ISPC objects, vendored with SHA-256 hashes, so building needs no ISPC.

### Formats and Skyrim conventions

- Community Shaders reads the `_n` blue channel directly and never rebuilds Z, so a BC5 `_n` decodes with Z = -1 and loses the specular alpha. Benchmarks that favour BC5 for normals rebuild Z, which Skyrim does not. Drove: normals are BC7; BC4/BC5 on a normal map is a blocking warning; BC5 is the default only for `_flow` and the user-picked `generic_two_channel`.
- Vanilla diffuse stores gamma-encoded colour in UNORM files, and an `_SRGB` tag darkens it in vanilla shaders, CS non-PBR paths and LOD. TruePBR expects albedo tagged `_SRGB`. Drove: `_SRGB` only for TruePBR colour (albedo, `_g`, `_s`, `_f`) and only under `cs`; `_SRGB` under a `lod` path is flagged.
- BC1 alpha on cutouts lost anti-aliased edges and measured 6.5 to 11.9 dB worse in visible RGB than BC7. Drove: BC1 is opaque only; the compact presets use it only for colour without alpha.
- Spike S2: 66 BC7 textures of 16 to 128 px with full chains ran in game with no crash. Drove: no small-texture fallback to another format.
- Skyrim SE loads both legacy `BC4U`/`BC5U` and DX10 BC4/BC5 headers, unsigned only. Drove: unsigned DX10 by default, legacy as an option, `ATI1`/`ATI2` never written.

### Alpha

- Stray alpha in a source costs about 1 dB on real images and up to 17 dB on smooth ones. Drove: the `remove` policy drops the alpha channel before any mip or encode step.
- Even with alpha removed, bc7e decoded 254 instead of 255 on 28% of pixels of RGB-only input (ISPC 15%, texconv 2.4%, NVTT 0%), because its mode 6 picks the shared p-bit for RGB alone. Forcing 255 costs 0.5 to 1.0 dB RGB on average (spike S5). On 28 real `_m` files encoded without the fix, 7 had 40% or more of their alpha off 255 (spike S6). Drove: exact-opaque alpha is on by default only for `_m` and env masks, where CS, ENB and PGPatcher read the alpha, and turning it off on a BC7 `_m` needs an explicit acknowledgement.
- A RenderDoc read of the Community Shaders lighting shader (spike S8) showed the complex-material gate exactly: the smallest mip's alpha must be below 251/255, with a separate rule for grey env masks. Drove: the `_m` margin check warns at 247 and applies the shader's own grey rule; near-opaque source alpha uses the same 251 cut-off.
- Box-filtered alpha thins out alpha-tested foliage at distance. Castaño's coverage-preserving mips and AlphaBloom's Auto Coverage fix this, but uniform scaling over- or under-shoots on sparse, tied alpha. Drove: Tessalith's own solver passes exactly mip 0's share at every level, splits ties in a Bayer order and keeps remapped texels two codes clear of the threshold so BC7 cannot flip them. Default threshold 127.

### Mips

- Averaging gamma-encoded values darkens thin bright detail; box is area-exact and never overshoots; sharper kernels help only LDR colour and ring on HDR and data. No published PSNR benchmark of mip filters for game textures exists. Drove: box stays the default for everything, colour is filtered in linear light, and Kaiser is fixed at width 3, α 4 (the NVTT wiki found larger radii ring).
- Building every level from the previous one cost tent 1.1 to 2.7 dB and Mitchell 0.8 to 1.8 dB against a direct Lanczos-8 reference in local tests. Drove: tent, Mitchell and Kaiser build each level directly from mip 0; box stays recursive, where it is exact.

### Normal maps

- Averaging unit normals shortens them, and the lost length is exactly the variance the mip throws away. Unity HDRP, The Order: 1886 and Call of Duty: WWII convert it to extra GGX roughness and renormalise the stored normal. Drove: every normal level is renormalised, and in a TruePBR set the variance goes into roughness with a vMF fit, κ = (3L − L³)/(1 − L²), clamped at t = 0.25 (HDRP's `NORMALMAP_HIGHEST_VARIANCE`).
- Plain 8-bit quantisation noise on a flat normal map gave 1 − L of 1e-5 to 1e-4 and lifted zero roughness to 0.04 to 0.10 at lower mips. Drove: a noise floor at L = 0.999 that adds no variance (about a 2° tilt).

### BC4/BC5 search radius

- Upstream rgbcx modelled BC4 decoding with floored interpolants while the GPU and bcdec round to nearest, so a wider search could make results worse (BC5 at radius 10 scored 47.9 dB against 58.0 dB at radius 5). Patching rgbcx to round like the decoder fixed this and raised the default-radius result by 3.07 dB (BC4) and 2.61 dB (BC5). After the patch, radius 10 gained only 0.12 dB (BC4) and 0.06 dB (BC5) at about 3× the time, and radius 4 lost under 0.06 dB at 0.7× the time. Drove: radius 5 stays the default, radius 4 is the `fast` setting used by the compact and preview presets. Full tables: [docs/spikes/bc45-search-radius.md](docs/spikes/bc45-search-radius.md).

### NVTT 3

- NVTT Production led the built-in encoders on gradients (+3.9 dB) and linear data (+0.3 to +0.4 dB) and kept BC6H highlight peaks at 95% or more, where ISPC BC6H cut one sun's peak to 22%. It took 40 to 60× the time. `-production` and `-highest` gave byte-identical output. Drove: NVTT is an opt-in quality option for BC7 and BC6H at Production only.
- NVTT's licence forbids use that would put it under an open-source licence. Drove: Tessalith never links or ships it and runs your installed `nvcompress.exe` as a separate process.
- Spike S7: feeding each pre-built level to its own `nvcompress -nomips` process matched a direct encode exactly at mip 0. BC7 Production runs on the CPU even with CUDA available, and `-nocuda` gave byte-identical output while saving about 0.13 s of CUDA start-up per process. Drove: Tessalith builds the mip chain itself and always passes `-nocuda`.
- Spike S7b: nvcompress read an uncompressed DDS with a DX10 header as garbage (5.05 dB) but read the same pixels with a legacy header exactly, and BC6H encoded every level down to 1×1. Drove: levels are fed as legacy-header DDS (BGRA8 for BC7, RGBA16F for BC6H) and no fallback encoder is needed. NVTT 3.2.5 is the tested version; others need confirmation.

## Quick start

### Requirements

- Windows x64.
- Rust stable with the `x86_64-pc-windows-msvc` target. `rust-toolchain.toml` selects it; the GUI crate needs Rust 1.95 or newer.
- Visual Studio Build Tools with the C++ workload (MSVC). `encoders-sys` compiles rgbcx and the ISPC TC wrapper with `cl.exe` and links the vendored ISPC objects. You do not need ISPC.
- A GPU with BC texture support for the GUI preview. Without one, the preview is off and everything else works.
- Optional: NVIDIA Texture Tools 3 (`nvcompress.exe`, 3.2.5 tested) for the NVTT backend.
- Optional, for the 3D preview: Community Shaders installed in your MO2 setup.

### Build

```
cargo build --release -p sdc -p sdc-gui
```

The binaries land in `target\x86_64-pc-windows-msvc\release\` as `sdc.exe` and `tessalith.exe`. Both link the C runtime statically, so each is a single exe.

### Command line

Look at what Tessalith would do with a folder, then convert it:

```
sdc inspect textures --recursive
sdc convert textures --recursive -o out --accept-suggestions high
```

`inspect` prints the sub-type, profile and warnings it proposes for each file. `convert` writes the DDS tree under `out` (default `sdc-out`) and a manifest under `out\_sdc\`. Files whose type or alpha meaning is unclear are skipped until you decide them, with `--set`, an `sdc.toml` entry, or the GUI. A blocking warning you accept is acknowledged with `--allow <CODE>`.

Other common flags: `--target cs|cpm|vanilla`, `--preset <id>`, `--fill metallic=255`, `--dry-run`, `--json`. To encode BC7 and BC6H with NVTT, add `--set encoder.backend=nvtt` (and `--nvtt <path to nvcompress.exe>` if it is not on `PATH` or in the NVIDIA install folder).

Check DDS files, from Tessalith or any other tool:

```
sdc validate out\textures\armor\iron\iron_n.dds --target cs
```

Exit codes: 0 done, 1 some files failed or were skipped, 2 usage or config error, 3 fatal, 4 validation failed.

### GUI

Run `tessalith.exe`, choose **Open folder…** (or drop a folder on the window), pick the target in the **Mode** menu, settle any texture with an amber border, then **Export**. Your decisions are saved to `sdc.toml` in that folder, and **Copy CLI command** gives the matching `sdc convert` line. The full guide is [docs/gui.md](docs/gui.md).

## Documentation

The wiki starts at [docs/wiki/Home.md](docs/wiki/Home.md).

- [Installation](docs/wiki/Installation.md)
- [Getting started](docs/wiki/Getting-Started.md)
- [CLI reference](docs/wiki/CLI-Reference.md)
- [Project config (`sdc.toml`)](docs/wiki/Project-Config.md)
- [Presets and targets](docs/wiki/Presets-and-Targets.md)
- [GUI guide](docs/wiki/GUI-Guide.md)
- [Texture types](docs/wiki/Texture-Types.md): shader modes, packing, TruePBR, RNM, complex grass
- [Mod scanning](docs/wiki/Mod-Scanning.md): MO2, NIF, plugins, PGPatcher
- [NVTT backend](docs/wiki/NVTT-Backend.md)
- [Validation and output](docs/wiki/Validation-and-Output.md)
- [Troubleshooting](docs/wiki/Troubleshooting.md)
- [FAQ](docs/wiki/FAQ.md)

Design records: [docs/DECISIONS.md](docs/DECISIONS.md), [docs/RULINGS.md](docs/RULINGS.md), [docs/presets-changelog.md](docs/presets-changelog.md) and the specs and plans under [docs/superpowers/](docs/superpowers/).

## Credits and licence

Tessalith is licensed under GPL-3.0-or-later; see [LICENSE](LICENSE). It builds on bc7e and rgbcx (Rich Geldreich / Binomial LLC), the Intel ISPC Texture Compressor and bcdec, among others. Third-party licences and the changes made to vendored code are listed in [THIRD_PARTY_NOTICES.md](THIRD_PARTY_NOTICES.md) and [LICENSES/](LICENSES/). Credits for ideas and prior work, including AlphaBloom's Auto Coverage, Ignacio Castaño's coverage-preserving mips and jiayev's TruePBR-Manager, are in [docs/CREDITS.md](docs/CREDITS.md).
