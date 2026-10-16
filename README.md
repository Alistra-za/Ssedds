<p align="center"><img src="docs/assets/tessalith-emblem.png" alt="Tessalith emblem" width="480"></p>

<h1 align="center">Tessalith</h1>

<p align="center">Converts texture source images to DDS for Skyrim SE/AE.</p>

---

Tessalith converts a folder of source images for Skyrim SE/AE. It works out what each file is from its name, groups files into material sets, packs loose maps into Skyrim's layouts, builds the mips and picks the format and colour tag each shader needs. The GUI (`tessalith.exe`) and the CLI (`sdc`) share one engine and one per-project `sdc.toml`, so both write the same bytes. Targets: `cs` (Community Shaders with TruePBR; default) and `vanilla` (which also packs the complex-material `_m` for a set given the complex material shader). It is for making new textures, not for fixing an installed game folder. Windows x64. Version 0.8.0 (see the [CHANGELOG](CHANGELOG.md)). GPL-3.0-or-later.

## Feature comparison

What each tool does for a Skyrim texture without extra scripting.

| Capability | Tessalith | texconv (DirectXTex) | NVIDIA Texture Tools Exporter | Compressonator | Cathedral Assets Optimizer |
|---|---|---|---|---|---|
| BC1, BC3, BC4, BC5, BC6H, BC7 output | ✅ | ✅ | ✅ | ✅ | ⚠️ [1] |
| Inputs: PNG, TGA, TIFF, EXR, PSD/PSB, Krita, OpenRaster | ✅ | ⚠️ [2] | ⚠️ [2] | ⚠️ [2] | ⚠️ [1] |
| `_SRGB` / UNORM tag chosen per map type and target | ✅ | ⚠️ [3] | ⚠️ [3] | ⚠️ [3] | ⚠️ [1] |
| Cutout alpha: coverage-preserving mips, on by default | ✅ | ⚠️ [4] | ⚠️ [4] | ⚠️ [5] | ⚠️ [5] |
| Exact-opaque alpha (255, not 254) on `_m` maps | ✅ | ⚠️ [6] | ✅ [6] | ⚠️ [5] | ⚠️ [5] |
| Normal mips renormalised | ✅ | ⚠️ [5] | ⚠️ [12] | ⚠️ [5] | ⚠️ [5] |
| Normal-mip variance baked into TruePBR roughness | ✅ | ❌ | ❌ | ❌ | ❌ |
| RNM normal-layer blending (`_bump`, `_detailnorm`, `_bent`) | ✅ | ❌ | ❌ | ❌ | ❌ |
| Loose maps packed into `_rmaos` / `_rfaos` / complex `_m` | ✅ | ⚠️ [7] | ❌ | ❌ | ❌ |
| TruePBR: PGPatcher JSON written per set | ✅ [8] | ❌ | ❌ | ❌ | ❌ |
| CS complex-material `_m` gate checked | ✅ | ❌ | ❌ | ❌ | ❌ |
| Complex grass (1:2 image, halves mipped apart) | ✅ | ❌ | ❌ | ❌ | ❌ |
| Mod scan: meshes decide alpha meaning and set shader | ✅ [8] | ❌ | ❌ | ❌ | ❌ |
| Decode-and-check after every encode, with Skyrim rules | ✅ | ❌ | ❌ | ⚠️ [9] | ❌ |
| Validate DDS files made by other tools | ✅ | ⚠️ [10] | ❌ | ⚠️ [9] | ⚠️ [1] |
| GUI with encoded-mip preview, pixel readout, pin and compare | ✅ | ❌ | ✅ | ✅ | ❌ |
| NiAlphaProperty (alpha blend/test) edited per TruePBR set | ✅ | ❌ | ❌ | ❌ | ❌ |
| Batch / command line | ✅ | ✅ | ✅ | ✅ | ✅ |
| NVTT as the BC7 / BC6H encoder | ⚠️ [11] | ❌ | ✅ | ❌ | ❌ |

1. CAO converts through DirectXTex with fixed rules for making existing mod files load. Not compared in detail.
2. Common formats; PSD, Krita and OpenRaster support varies. Not checked one by one.
3. You set the tag per file or per run.
4. An option you turn on (texconv `-keepcoverage`; NVTT has an alpha-coverage option in its library, exporter UI not checked), not set from the texture's role.
5. No documented option found. Not tested.
6. Local benchmark, RGB-only BC7: texconv decoded 254 on 2.4% of pixels, NVTT (`nvcompress`) on 0% ([Research](docs/wiki/Research.md#alpha)).
7. `texassemble` merges channels but knows no Skyrim layout.
8. GUI only.
9. Compares an encode with its source (PSNR, SSIM); no Skyrim rules.
10. `texdiag` reports header and pixel data; no Skyrim rules.
11. Optional, never bundled: runs your installed `nvcompress.exe` (3.2.5 tested) at Production, BC7 and BC6H only.
12. An option for normal maps (normalise mipmaps); not set from the texture's role.

PGPatcher (ParallaxGen) is left out: it patches meshes and reads the JSON Tessalith writes, so it complements rather than competes.

## What it does differently

- **Starts from what the file is.** Suffix and folder give a sub-type; the sub-type sets format, tag, alpha and mips. Unclear files are skipped until you decide them.
- **Follows Skyrim's shader rules.** `_SRGB` only on TruePBR colour under `cs`; BC4/BC5 on a normal map is blocked, since Skyrim reads Z directly.
- **Reads your load order.** A GUI scan of MO2 meshes and plugins gives each diffuse its alpha meaning.
- **Checks its own output.** Every file is decoded and checked; a manifest lets re-runs redo only what changed.

## RNM vs alternatives

Ways to put a detail normal on a base normal:

| Method | How it combines | Preserves | Artifacts | Cost | Use for |
|---|---|---|---|---|---|
| Linear / overlay | Per-channel add or overlay, renormalise | Neither map exactly | Flattened detail; overlay treats vectors as colour | Lowest | Faint detail, previews |
| Partial derivative (PD) / UDN | PD adds slopes (xy/z); UDN adds xy, keeps base z | Slopes at small angles | UDN flattens detail; both lose accuracy as the base tilts | Low | Real-time multi-layer shaders |
| Whiteout | Adds xy, multiplies z, renormalise | More detail than UDN | Still approximate on steep bases | Low | Cheap shader blends |
| RNM (Barré-Brisebois and Hill) | Rotates the detail into the base normal's frame | Detail as seen on the tilted surface; flat inputs leave the other unchanged | None in normal use; needs unit vectors | A little more | Highest quality: baking or high-end shaders |

What Tessalith does (`crates/engine/src/rnm/`):

- **Method:** RNM for `_bump`, `_bump2`, `_detailnorm` and `_detailnorm2`. Bump layers are height: slopes from the mean of RGB become normals first. `_bent` defaults to an AO blend: a renormalised linear mix weighted by strength × (1 − AO)^1.5 × mask, AO from `_ao` (R) or `_rmaos`/`_rfaos` (B). With no AO map the weight is uniform and `RNM-AO-MISSING` is reported. `rnm.bent.mode = "detail"` blends it with RNM instead.
- **Order and strength:** `bent` 0.35, `bump` 0.4, `bump2` 0.4, `detail` 0.6, `detail2` 0.3, in that order; `rnm.order` moves listed layers to the front. Strength scales the layer's slope (0 flat, 1 as authored); a mask's R channel scales it per texel.
- **Where:** into the set's `_n` or `_cnr`, after decode and green flip, before mips. Layers are resampled to the base size. Alpha is untouched, so `_cnr` coat roughness passes through. The TruePBR roughness bake reads the blended normal.

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

Design records: [docs/DECISIONS.md](docs/DECISIONS.md), [docs/RULINGS.md](docs/RULINGS.md), [docs/presets-changelog.md](docs/presets-changelog.md) and [docs/superpowers/](docs/superpowers/).

## Credits and licence

Tessalith is licensed under GPL-3.0-or-later; see [LICENSE](LICENSE). It builds on bc7e and rgbcx (Rich Geldreich / Binomial LLC), the Intel ISPC Texture Compressor and bcdec, among others. Third-party licences and the changes made to vendored code are listed in [THIRD_PARTY_NOTICES.md](THIRD_PARTY_NOTICES.md) and [LICENSES/](LICENSES/). Credits for ideas and prior work, including AlphaBloom's Auto Coverage, Ignacio Castaño's coverage-preserving mips and jiayev's TruePBR-Manager, are in [docs/CREDITS.md](docs/CREDITS.md).
