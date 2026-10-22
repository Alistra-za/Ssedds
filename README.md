<p align="center"><img src="docs/assets/tessalith-emblem.png" alt="Tessalith emblem" width="480"></p>

<h1 align="center">Tessalith</h1>

<p align="center">Converts texture source images to DDS for Skyrim SE/AE.</p>

---

Tessalith converts a folder of source images for Skyrim SE/AE. It works out what each file is from its name, groups files into material sets, packs loose maps into Skyrim's layouts, builds the mips and picks the format and colour tag each shader needs. The GUI (`tessalith.exe`) and the CLI (`sdc`) share one engine and one per-project `sdc.toml`, so both write the same bytes.

- **Targets:** `cs` (Community Shaders with TruePBR; default) and `vanilla` (which also packs the complex-material `_m` for a set given the complex material shader).
- **Scope:** it makes new textures. It does not fix an installed game folder.
- **Platform and version:** Windows x64, version 0.9.0 (see the [CHANGELOG](CHANGELOG.md)). GPL-3.0-or-later.

## Features

**Encoding and checks**
- BC1, BC3, BC4, BC5, BC6H and BC7 output, with the [best encoder for each format](#uses-the-best-encoder-available). Optional [NVTT backend](docs/wiki/NVTT-Backend.md) for BC7 / BC6H.
- BC7 weighting dropdown (Linear, Perceptual, Alpha ×2, ×8, ×16).
- Optional exact-opaque alpha 255 (forces BC7 mode 6's p-bit to 1; off by default).
- Every encode is decoded and checked against Skyrim's rules. DDS files made by other tools can be validated too.
- [Artist checks](docs/wiki/GUI-Guide.md#artist-checks): warnings for problems such as a weak or clipped height map. They never block an export.

**Maps and packing**
- Inputs: PNG, TGA, TIFF, EXR, PSD/PSB, Krita, OpenRaster.
- SRGB / UNORM (Linear) tag chosen per map type and target shader.
- Cutout alpha: coverage-preserving mips, on by default. Normal mips are renormalised, and their variance is baked into TruePBR roughness.
- Loose maps are picked up and packed into `_rmaos` / `_rfaos` / complex `_m`. Suffix-based channel remapping; ORM to RMAOS is instant. The complex-material `_m` gate is checked.
- Multiple layers and duplicate types (`_bump2`) just work.
- RNM normal-layer blending (`_bump`, `_detailnorm`, `_bent`, with optional masks `_bumpmask`, `_detailnormmask`, `_bentmask`). See [RNM normal layers](docs/wiki/Texture-Types.md#rnm-normal-layers).
- [Generate from normal map](docs/wiki/GUI-Guide.md#generate-from-normal-map): makes height, AO and curvature maps from a set's normal.
- Complex grass (1:2 image, halves mipped apart).

**Skyrim and mod aware**
- Mod scan: meshes decide alpha meaning and set shader. Fast: 100,000 NIFs in 14 seconds, 1,500 plugins in under 3. See [Mod scanning](docs/wiki/Mod-Scanning.md).
- TruePBR: PGPatcher JSON written per set, validated, with every option exposed, including the recent ones. PGPatcher `_m` sidecar `.json`.
- PBR Landscape: PBRTextureSets and PBRMaterialObjects JSONs built from the actual EDIDs in your load order.
- [Also export as Vanilla and Complex material](docs/wiki/GUI-Guide.md#also-export-as-vanilla-and-cm): converts PBR sets to those two formats.
- NiAlphaProperty (alpha blend/test) set in a familiar format.
- Doesn't need to be launched from MO2, but can be. From MO2 it fills in your MO2 location the first time, and skips MO2's virtual file system.

**GUI**
- Encoded-mip preview, pixel readout, pin and compare.
- [3D preview](docs/wiki/GUI-Guide.md#set-preview-3d) that matches Community Shaders' deferred rendering (**Accurate (deferred)**).
- Context-menu options for slots and lock commands.
- Colour picker with hex and RGB (0–255) inputs; outputs are converted to decimal.

**NIF editing (in the 3D view)**
- Edit NIF fields and shader flags.
- Animation clock, key editor, controllers and named clips, with an in-game preview.
- Export removes blocks that your edits left unused.

**Command line**
- Batch conversion, inspect and validate with `sdc`. See the [CLI reference](docs/wiki/CLI-Reference.md).

**What sets it apart.** It starts from what the file is: suffix and folder give a sub-type, and the sub-type sets format, tag, alpha and mips. Unclear files are skipped until you decide them. It blocks combinations Skyrim mishandles, such as `_SRGB` outside TruePBR colour under `cs`, or BC4/BC5 on a normal map. A manifest lets re-runs redo only what changed.

## Uses the best encoder available

BC7E for BC7, rgbcx for BC1–BC5, Intel ISPC for BC6H. Charts show the measured results; the scripts that draw them are in `tools/readme-charts.py`.

### BC7: speed against quality

![Scatter charts of BC7 encoders, speed on a log axis against quality. bc7e u5 and u6 are fast, about 0.2 to 0.3 s per megapixel, and best on real colour and alpha. NVTT Production, texconv CPU and Compressonator take 14 to 33 s and are no better. GPU encoders are fastest but lose 1.4 to 2.8 dB.](docs/img/encoder-speed-quality.svg)

‡ `max` is opt-in (`encoder.effort = "max"`). It was measured later, on a 150-texture Skyrim sample, not on the set above. It took 2.7× the time of u5 (`veryslow`) with the two run alternately per image, and 2.9× in separate runs, so about 0.6 s/Mpix on this scale. That matches the expected ~3×. Δ is against u6 on that sample: RGB on diffuse (images over 60 dB left out; +0.94 with all 60) and alpha PSNR on the 30 textures with alpha. Against u5 it gains +0.18 dB on diffuse, +0.037° on normal maps and +0.24 dB on alpha. A build ran during part of the alternating run (CPU about 14% busy before it, up to 6 rustc processes); the separate runs had the machine nearly idle. Details: [bc7e tuning](docs/research/bc7e-tuning.md).

### BC7: bc7e u5 against the other CPU encoders

![Seven small charts comparing bc7e u5, NVTT 3 Production, Compressonator q1.0, Intel ISPC slow and texconv. bc7e u5 matches the best on real colour and decals at 0.22 s per megapixel. NVTT and Compressonator win on synthetic gradients but take 14 to 33 s. texconv is last on every quality measure. bc7e u5 turns 28 percent of opaque input into alpha 254.](docs/img/encoder-bc7-measures.svg)

\* An optional patch (mode 6 p-bit override, exact opaque) is for when alpha 254 is not acceptable.

† Compressonator q1.0 was not re-measured on the wider set; its values come from the original 4–7 images. On those images it was 0.15 dB behind NVTT on packed data and 0.27° behind on normals. The alpha-254 and timing rows are also from the original run.

### Per format

![Six small charts of the best encoder per format. rgbcx is best on BC1, BC4 and BC5 and within 0.09 dB of NVTT 3 on BC3. Intel ISPC is best on BC6H multi-exposure PSNR, while NVTT 3 keeps the brightest highlight best, 94 percent against 22 percent for ISPC. rgbcx does not do BC6H.](docs/img/encoder-formats.svg)

rgbcx beats NVTT on BC1 on 96 of 100 images, and BC3 is a tie. The texconv GPU path is slower than ISPC on BC6H and loses 1.8 dB.

† Original 4 HDRIs only; Compressonator's mPSNR there was 1.2 dB below ISPC.

## Quick start

Build with Rust stable and the MSVC Build Tools ([Installation](docs/wiki/Installation.md)). This writes `sdc.exe` and `tessalith.exe` to `target\x86_64-pc-windows-msvc\release\`:

```
cargo build --release -p sdc -p sdc-gui
```

**GUI:** run `tessalith.exe`, **Open folder…**, pick the **Mode**, settle any amber-bordered texture, **Export**. Decisions go to `sdc.toml`; **Copy CLI command** gives the matching `sdc convert` line.

**CLI:**

```
sdc inspect textures --recursive
sdc convert textures --recursive -o out --accept-suggestions high
sdc validate out\textures\armor\iron\iron_n.dds --target cs
```

Exit codes: 0 done, 1 some files failed or were skipped, 2 usage or config error, 3 fatal, 4 validation failed.

**Unsigned exe.** The release exes are not code-signed, so Windows SmartScreen may warn on first run. Click **More info**, then **Run anyway**. This is an alpha: back up your mod folders first.

## Where your data lives

| What | Where |
|---|---|
| GUI settings | `%APPDATA%\sdc\gui.toml` |
| CLI/GUI user config | `%APPDATA%\sdc\config.toml` |
| User presets | `%APPDATA%\sdc\presets\` |
| Mod scan database | `%LOCALAPPDATA%\sdc\scan.db` |
| GUI crash logs | `%LOCALAPPDATA%\sdc\logs\crash-<date>-<time>.log` (the CLI writes none) |
| Per-folder settings | `sdc.toml` and `sdc-project.toml` in the folder you convert |
| Unsaved edits after a crash | `sdc.toml.recovery`, `sdc-project.toml.recovery`, beside the original |

## Known issues and changes

See [Known issues](docs/wiki/Known-Issues.md) and the [CHANGELOG](CHANGELOG.md).

## Documentation

| Start | Use | Reference |
|---|---|---|
| [Home](docs/wiki/Home.md) | [GUI guide](docs/wiki/GUI-Guide.md) | [CLI reference](docs/wiki/CLI-Reference.md) |
| [Installation](docs/wiki/Installation.md) | [Texture types](docs/wiki/Texture-Types.md) (packing, TruePBR, RNM, grass) | [Project config](docs/wiki/Project-Config.md) |
| [Getting started](docs/wiki/Getting-Started.md) | [Mod scanning](docs/wiki/Mod-Scanning.md) | [Presets and targets](docs/wiki/Presets-and-Targets.md) |
| [FAQ](docs/wiki/FAQ.md) | [NVTT backend](docs/wiki/NVTT-Backend.md) | [Validation and output](docs/wiki/Validation-and-Output.md) |
| [Troubleshooting](docs/wiki/Troubleshooting.md) | [Research behind the defaults](docs/wiki/Research.md) | |

Design records: [docs/DECISIONS.md](docs/DECISIONS.md), [docs/RULINGS.md](docs/RULINGS.md), [docs/presets-changelog.md](docs/presets-changelog.md) and the research pages in [docs/research/](docs/research/README.md).

## Credits and licence

Tessalith is licensed under GPL-3.0-or-later; see [LICENSE](LICENSE). It builds on bc7e and rgbcx (Rich Geldreich / Binomial LLC), the Intel ISPC Texture Compressor and bcdec, among others. Third-party licences and the changes made to vendored code are listed in [THIRD_PARTY_NOTICES.md](THIRD_PARTY_NOTICES.md) and [LICENSES/](LICENSES/). Credits for ideas and prior work, including AlphaBloom's Auto Coverage, Ignacio Castaño's coverage-preserving mips and jiayev's TruePBR-Manager, are in [docs/CREDITS.md](docs/CREDITS.md).
