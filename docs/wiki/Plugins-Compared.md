# Plugins compared

Tessalith has plugins for four other programs. Each saves one image as a Skyrim DDS with Tessalith's encoders and settings rules. All four use the same library, `psdds.dll`, so the formats, the greyed-out options and the quality levels match in every program. This page shows what each plugin offers and what only Tessalith itself (`tessalith.exe` and `sdc`) does.

| Plugin | Program | Requires | Status | User guide |
|---|---|---|---|---|
| Tessalith DDS | Adobe Photoshop | Photoshop 2020 or later | Released | `photoshop/README.md` |
| Tessalith DDS nodes | chaiNNer | chaiNNer, installed with `chainner\install.ps1` | Released | `chainner/README.md` |
| Tessalith DDS | GIMP | GIMP 3.0.x | In development, not released | `gimp/README.md` |
| Tessalith DDS | Paint.NET | Paint.NET 5.1 or later | In development, not released | `paintdotnet/README.md` |

All of them run on Windows x64 only.

## Which one to use

- **Use a plugin to save one texture** from the program you paint or edit it in. You choose the texture type and format yourself.
- **Use chaiNNer** for a chain of steps, such as upscaling then saving, or to build a TruePBR `_rmaos` from separate maps.
- **Use Tessalith for a whole mod.** It works out each file's type from its name, groups files into sets, packs maps, checks every result and keeps a project config you can run again.

## Saving

| | Tessalith | Photoshop | GIMP | Paint.NET | chaiNNer |
|---|---|---|---|---|---|
| What you choose | Nothing per file: the sub-type comes from the file name, with the preset and `sdc.toml` on top | Texture type and format in a dialog | Texture type and format in a dialog | Texture type and format in a dialog | Node inputs |
| Texture types | Every Skyrim sub-type (see [Texture types](Texture-Types.md)) | Colour, Normal map, Linear mask | Colour, Normal, Linear mask | Colour, Normal map, Linear mask | Colour, Normal, Linear mask, plus the RMAOS and Save Normal nodes |
| BC1, BC3, BC7, RGBA8 | Yes | Yes | Yes | Yes | Yes |
| BC4, BC5 | Yes | BC5 for Normal and Linear mask; BC4 for Linear mask | Same as Photoshop | Same as Photoshop | Same as Photoshop |
| BC6H, RGBA16F | Yes, `cs` target only | Colour, 16-bit and 32-bit documents | Colour, 16-bit and 32-bit images | No | Colour |
| R8, RG8 | Yes | No | No | No | No |
| Several layers | PSD/PSB, KRA and ORA: the merged image stored in the file. PSD needs Maximize Compatibility | The merged visible layers, plus the first alpha channel if there is one | A merged copy of the visible layers | Paint.NET flattens the image first (it asks) | Not applicable |
| Batch | A whole folder tree | Photoshop actions and File > Automate > Batch, using your last choices | No | No | chaiNNer's own iterator nodes |
| Preview of the result | 2D and 3D preview in the GUI | No | No | Yes, with the file size | No |
| Checks the file after writing | Yes: decodes and validates it, and records it in the manifest | No | No | No | No |
| Largest image | 16384 x 16384 | 16384 x 16384 | 16384 x 16384 | The size the error message names | Size limits come from psdds |

## Settings

| | Tessalith | Photoshop | GIMP | Paint.NET | chaiNNer |
|---|---|---|---|---|---|
| sRGB tag | Set by the target and the sub-type (for example `_SRGB` on TruePBR colour under `cs`) | Checkbox, off by default | Checkbox, off by default | Checkbox, off by default | Checkbox, off by default |
| Alpha | Per sub-type: removed, cutout, blend and others. Can also come from the meshes after a mod scan, and BC7 or BC3 is weighed for each texture | Kept when some pixel is below 254 of 255 and the format stores alpha. **Discard transparency** saves it opaque | Same as Photoshop | Same as Photoshop | Same as Photoshop. An image with no used alpha is saved opaque |
| Mipmaps | Box, Tent, Mitchell or Kaiser, edge mode Wrap or Clamp, with rules per sub-type | On or off, plus a filter. Greyed to Box where the encoder would use Box anyway | Same as Photoshop | Same as Photoshop | Same as Photoshop |
| Quality | Per format, in the preset or config | Per format | Per format | Per format | Per format |
| Presets | Built-in presets (`cs-quality`, `vanilla-quality` and more) and your own in `%APPDATA%\sdc\` | Five built-in, plus save and delete your own | Five built-in, no presets of your own | Five built-in, plus save and delete your own (kept apart from Photoshop's) | Custom or one of the five built-in |
| Settings remembered | `sdc.toml` per project | Your last choices | Your last export settings | The image's last choices | Saved in the chain |
| Target (`cs` or `vanilla`) | Yes; formats and tags are checked against it | No. BC6H and RGBA16F are marked as Community Shaders only | Same as Photoshop | No | No |
| NVTT encoder | Optional, for BC7 and BC6H | No | No | No | No |

## 16-bit, 32-bit and HDR

| | Tessalith | Photoshop | GIMP | Paint.NET | chaiNNer |
|---|---|---|---|---|---|
| High bit-depth input | 16-bit PNG and TIFF, 16-bit and 32-bit PSD/PSB, EXR, HDR | 8-bit, 16-bit and 32-bit RGB documents | 8-bit, 16-bit, half, float and double images | 8-bit only | Float images |
| Values above 1.0 (emissive) | Yes: a TruePBR emissive from an EXR or HDR source that goes above 1.0 becomes BC6H | Yes, from 32-bit documents saved as BC6H or RGBA16F | Yes, from float images saved as BC6H or RGBA16F | No | Yes, with **Linear HDR input** on |
| 32-bit normal maps (such as a Marmoset bake) | Yes | Yes, raw values passed to the encoder | Yes, read in the image's own encoding | No | Yes, at full precision |

## Opening DDS files

| | Tessalith | Photoshop | GIMP | Paint.NET | chaiNNer |
|---|---|---|---|---|---|
| Open a DDS | No. `sdc inspect` and `sdc validate` read the header. A DDS input is skipped unless you set `reencode = true` | Yes, top mip only, as an 8-bit document | Yes, top mip only, as an 8-bit image | No. Paint.NET's own DDS support opens it | No. chaiNNer's own Load Image reads DDS |
| Open HDR DDS (BC6H, RGBA16F, RGBA32F) | No | Yes, as a 32-bit document, values above 1.0 kept | Passed to GIMP's own DDS plug-in | No | No |
| Cube maps and texture arrays | No | Left to any other DDS plugin you have | Passed to GIMP's own DDS plug-in | No | No |

## Only in Tessalith

- **Naming and sets.** It finds each file's sub-type from its suffix (`_n`, `_rmaos`, `_m` and so on), groups files into texture sets and writes them to the right output paths. See [Texture types](Texture-Types.md).
- **Packing.** It packs separate roughness, metallic, AO and specular maps into the layout the target expects, including the complex material `_m` for Vanilla. chaiNNer's RMAOS node does the TruePBR `_rmaos` only.
- **Normal map tools.** It checks green-channel direction, blends RNM normal layers and generates maps from a normal map. See the [GUI guide](GUI-Guide.md).
- **Mod scanning.** It reads your MO2 instance or a folder of meshes and plugins, and uses them to suggest alpha and complex grass. See [Mod scanning](Mod-Scanning.md).
- **Set editor and TruePBR.** It has a set editor, a 3D set preview, and a TruePBR JSON editor.
- **Validation and the manifest.** Every file is decoded again, checked against the target's rules and recorded. See [Validation and output](Validation-and-Output.md).
- **Command line.** `sdc` runs the same project from a script, with a JSON event stream. See the [CLI reference](CLI-Reference.md).

## Licences

| | Licence |
|---|---|
| Tessalith | GPL-3.0-or-later |
| Photoshop plugin | MIT. The Tessalith engine code inside it is MPL-2.0 |
| GIMP plug-in | The Python files are GPL-3.0-or-later. `psdds.dll` is MIT, with the engine code inside it under MPL-2.0 |
| Paint.NET plugin | GPL-3.0-or-later with an additional permission for Paint.NET. `psdds.dll` is MIT, with the engine code inside it under MPL-2.0 |
| chaiNNer nodes | GPL-3.0-or-later. `psdds.dll` is MIT, with the engine code inside it under MPL-2.0 |

The Photoshop plugin cannot contain GPL code, because Adobe's SDK licence forbids it. The other plugins can.
