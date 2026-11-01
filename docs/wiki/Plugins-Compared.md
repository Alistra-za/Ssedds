# Plugins compared

Each plugin saves one image as a Skyrim DDS through `psdds.dll`, which holds Tessalith's encoders and settings rules. Everything else on this page that a plugin does not do needs Tessalith itself (`tessalith.exe` or `sdc`).

Install steps, versions and the full options are on [Host plugins](Host-Plugins.md).

## Saving

| | Tessalith | Photoshop | GIMP | Paint.NET | chaiNNer |
|---|---|---|---|---|---|
| Settings come from | The file name's sub-type, the preset and `sdc.toml` | A dialog | A dialog | A dialog | Node inputs |
| Texture types | Every Skyrim sub-type ([Texture types](Texture-Types.md)) | Colour, Normal, Linear mask | Colour, Normal, Linear mask | Colour, Normal, Linear mask | Colour, Normal, Linear mask |
| BC1, BC3, BC4, BC5, BC7, RGBA8 | Yes | Yes | Yes | Yes | Yes |
| BC6H, RGBA16F | Yes, `cs` only | Colour, 16-bit and 32-bit documents | Colour, 16-bit and 32-bit images | No | Colour |
| R8, RG8 | Yes | No | No | No | No |
| Batch | Whole folder trees | Photoshop actions and Batch | No | No | chaiNNer's iterator nodes |
| Checks the file after writing | Yes, and records it in the manifest | No | No | No | No |
| Target (`cs` or `vanilla`) rules | Yes | No | No | No | No |
| Presets | Built-in and your own | Five built-in, plus your own | Five built-in | Five built-in, plus your own | Five built-in |
| NVTT encoder | Optional | No | No | No | No |

## Mipmaps

| | Tessalith | Photoshop | GIMP | Paint.NET | chaiNNer |
|---|---|---|---|---|---|
| Coverage-preserving mips for cutouts | Yes: every mip keeps the share of texels that pass the alpha test, so foliage and fences do not thin out at a distance. Checked on every level (`VAL-COVERAGE`) | No | No | No | No |
| Filters | Box, Tent, Mitchell, Kaiser; edges Wrap or Clamp | Box, Tent, Mitchell, Kaiser | Box, Tent, Mitchell, Kaiser | Box, Tent, Mitchell, Kaiser | Box, Tent, Mitchell, Kaiser |

## Packing and bakes

| | Tessalith | Photoshop | GIMP | Paint.NET | chaiNNer |
|---|---|---|---|---|---|
| **RMAOS (TruePBR, `cs`)** | | | | | |
| Pack `_rmaos` from separate roughness, metallic, AO and specular maps | Yes, also from `_orm` | No | No | No | Yes, RMAOS node |
| Variance bake: normal detail lost in each mip goes into the roughness mips | Yes | No | No | No | Yes, when RMAOS feeds Save Normal |
| **CPM (complex material `_m`, `vanilla`)** | | | | | |
| Pack the complex `_m` (environment, gloss, metal, height) | Yes | No | No | No | No |
| Gloss bake: normal detail lost in each mip goes into the `_m` gloss (opt-in) | Yes | No | No | No | No |
| **Normal maps** | | | | | |
| RNM layers: bump, detail and bent normals blended into the `_n` | Yes | No | No | No | No |

See [Texture types](Texture-Types.md) for the packed layouts, the variance bake, the gloss bake and RNM layers.

## 16-bit, 32-bit and HDR

| | Tessalith | Photoshop | GIMP | Paint.NET | chaiNNer |
|---|---|---|---|---|---|
| High bit-depth input | 16-bit PNG and TIFF, 16-bit and 32-bit PSD/PSB, EXR, HDR | 16-bit and 32-bit documents | 16-bit, half, float and double images | No, 8-bit only | Float images |
| Values above 1.0 (emissive) | Yes, to BC6H | Yes, from 32-bit to BC6H or RGBA16F | Yes, from float to BC6H or RGBA16F | No | Yes, with **Linear HDR input** on |
| 32-bit normal maps (such as a Marmoset bake) | Yes | Yes | Yes | No | Yes |

## Opening DDS files

| | Tessalith | Photoshop | GIMP | Paint.NET | chaiNNer |
|---|---|---|---|---|---|
| Open a DDS | No; `sdc inspect` reads the header | Yes, as 8-bit | Yes, as 8-bit | No (Paint.NET's own support opens it) | No (chaiNNer's own Load Image reads it) |
| Open HDR DDS (BC6H, RGBA16F, RGBA32F) | No | Yes, as 32-bit | No (GIMP's own DDS plug-in opens it) | No | No |

## Only in Tessalith

- Texture sets: grouping by name, sub-types from suffixes, output paths.
- Mod scanning: alpha and complex grass suggestions from your meshes and plugins ([Mod scanning](Mod-Scanning.md)).
- The set editor, 3D set preview and PGPatcher JSON editor ([GUI guide](GUI-Guide.md)).
- The command line, with a JSON event stream ([CLI reference](CLI-Reference.md)).

Licences: see [Host plugins](Host-Plugins.md#licences).
