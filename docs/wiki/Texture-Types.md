# Texture types

Tessalith decides how to encode each file from its sub-type: what the texture is for. The sub-type sets the format, the colour tag, the alpha handling and the mip settings. This page covers how files are named, grouped and typed, and what each type becomes.

## Input formats

| Extension | Notes |
|---|---|
| `.png`, `.tga`, `.tif`, `.tiff`, `.bmp`, `.jpg`, `.jpeg`, `.webp` | Read as stored. |
| `.exr`, `.hdr` | Float sources. A TruePBR emissive needs one (peaking above 1.0) to become BC6H. `hdr_sky` is BC6H whatever the source. An `.exr` with alpha is read as premultiplied; set `alpha.exr_straight = true` for one saved with straight alpha (in the GUI: the Problems row's **Straight alpha**, or the tick box in the texture's Alpha group). |
| `.psd`, `.psb` | The merged composite image only; layers are never read. RGB and Grayscale documents, 8, 16 and 32 bits per channel. The file must be saved with Maximize Compatibility on; without it the file is refused. Colour that Photoshop matted against white is un-matted. |
| `.kra`, `.ora` | Krita and OpenRaster: the `mergedimage.png` stored in the archive. Layers are never read. |
| `.dds` | Skipped by default (`IN-DDS-SKIPPED`). Set `reencode = true` on the file to convert it anyway, at a quality cost (`LOSS-REENCODE`). |

Other extensions inside a folder are ignored. Images larger than 16384 pixels on a side fail with `DIM-TOO-LARGE`. Embedded ICC profiles are ignored and the pixels are read as stored (`IN-ICC-IGNORED`). NaN or infinite samples are replaced (`IN-NONFINITE`).

## Texture sets

Files in the same output folder that share a base name form a texture set. The base name is the file name minus its suffix:

```text
textures\armor\iron\
  iron.png          set "iron", colour map (no suffix)
  iron_n.png        set "iron", suffix _n
  iron_m.png        set "iron", suffix _m
  iron_trim.png     set "iron_trim", colour map (_trim is not a suffix)
```

Some decisions depend on the whole set: whether it is a PBR set, which shader it is for, which raw maps get packed together, and whether a normal map's detail is baked into roughness.

## Suffixes

Suffixes are matched case-insensitively, longest first, so `_msn` wins over `_n` and `_envmask` over `_m`.

| Suffix | Suggested sub-type | Confidence |
|---|---|---|
| none, or `_d` | `diffuse`; `pbr_albedo` in a PBR set; `skin_albedo` in an Advanced skin set; `landscape_diffuse` when a folder in the path is named `landscape`; `ui_sky` under `interface` or `sky` folders; `hdr_sky` for a float source under `sky` | High (`hdr_sky` Medium) |
| `_n` | `normal`; `pbr_normal` in a PBR set; `landscape_normal` when a folder in the path is named `landscape` | High |
| `_msn` | `msn` (model-space normal) | High |
| `_s` | `msn_specular` when the set has `_msn`; `pbr_subsurface` in a PBR set; otherwise `subsurface_tint` | High, High, Low |
| `_m`, `_em`, `_envmask` | `envmask`; `complex_material` when the alpha minimum is below 251 | High for both |
| `_g` | `glow`; `pbr_emissive` in a PBR set | High |
| `_p` | `height` (parallax) | High |
| `_rmaos` | `rmaos` | High |
| `_rfaos` | `skin_rfaos` (Advanced skin) | High |
| `_cnr`, `_crn` | `coat_normal` | High |
| `_f` | `pbr_fuzz` | High |
| `_flow` | `flow` (hair flow) | High |
| `_b`, `_bl` | `backlight_tint` | High |
| `_sk` | `skin_sk` in an Advanced skin set; otherwise `backlight_tint` | High, Low |
| `_i` | `diffuse` (multilayer inner layer) | Medium |
| `_wet` | `skin_wet`, only in a `cs` Advanced skin set; elsewhere `_wet` is part of the base name | High |
| `_e`, or a file under `textures\cubemaps\` | `cubemap`; the six-face suffixes `_posx` to `_negz` (or `_px` to `_nz`) after the name are cut off first | High |

A set is a PBR set (under `cs`) when its path has a `pbr` folder, or it has an `_rmaos`, `_rfaos`, `_cnr` or `_f` member, or it has a raw roughness, glossiness, metallic or AO map. A set is an Advanced skin set when it has or builds an `_rfaos`; a set whose shader you set to Advanced Skin (GUI, or `--set set:<id>.shader=advanced_skin`) builds one unless `packing.recipe` says otherwise or it has only a metallic map, and its diffuse is `skin_albedo` either way (also when written as plain `diffuse`). In the GUI, pinning the set's Diffuse slot turns this off: the set then reads and packs as it would without the shader choice, unless its diffuse is already written as `skin_albedo`. In an Advanced skin set a plain `_f` is the skin fuzz mask: a greyscale map packed into the `_rfaos` G channel and not converted on its own. A `_f` never makes a set Advanced skin by itself, and one you give a sub-type converts alone as fuzz.

Medium and Low suggestions always need your confirmation. High suggestions are applied automatically in the GUI, and on the command line with `--accept-suggestions high`.

A file whose name does not fit these rules is read as a colour map. A normal map saved as `rock_nm.png` would become a diffuse. Fix it by renaming, by an `[inputs]` alias, or by setting `subtype` in a `[files]` entry.

### Exporter names

Tessalith also understands the names that texturing tools commonly export:

| Term | Read as | Written as |
|---|---|---|
| `_colormap`, `_basecolor`, `_base_color`, `_albedo`, `_diffuse`, `_color`, `_col` | Colour map | `<base>.dds` |
| `_normal`, `_nrm`, `_norm` | Normal map, green direction not stated | `<base>_n.dds` |
| `_normalgl` | Normal map, OpenGL green (flipped on output) | `<base>_n.dds` |
| `_normaldx` | Normal map, DirectX green | `<base>_n.dds` |
| `_emissive`, `_emission` | Emissive | `<base>_g.dds` |
| `_coat` | PBR coat colour (RGB) and strength (A) | `<base>_s.dds` |
| `_roughness`, `_rough` | Raw roughness | packed |
| `_metallic`, `_metalness`, `_metal` | Raw metallic | packed |
| `_ao`, `_ambientocclusion`, `_occlusion` | Raw AO | packed |
| `_orm` | Channel-packed AO (R), roughness (G), metallic (B) | packed |
| `_specular`, `_spec` | Raw specular | packed |
| `_glossiness`, `_gloss` | Raw glossiness | packed (inverted into roughness) |
| `_height`, `_displacement`, `_disp` | Raw height | packed |
| `_opacity` | Raw opacity | the colour texture's alpha ([helper](Texture-Packing.md#helper-textures)) |
| `_fuzz`, `_sheen` | Raw fuzz | packed |
| `_curves`, `_curvature` | Raw curvature (0.5 is flat) | read only by Also export as (Vanilla / Complex material) |
| `_cavity` | Raw cavity | read only by Also export as (Vanilla / Complex material) |

A normal map named only `_normal`, `_nrm` or `_norm` does not say which way its green channel points. Tessalith assumes DirectX, which is what Skyrim uses, converts it without a flip and notes `IN-GREEN-UNSTATED` as information. If the map is OpenGL, set `channels.green_convention = "opengl"` on the file (the GUI offers a *Treat as OpenGL* button on that note) or name it `_normalgl`.

You can add your own suffixes through a preset's `suffix_aliases`. See [Presets and targets](Presets-and-Targets.md#suffix-aliases).

## Sub-types

There are 30 sub-types in five categories.

| Sub-type | Category | Default format | Tag (`cs`) | Default alpha | Notes |
|---|---|---|---|---|---|
| `diffuse` | Color | BC7 | UNORM | remove | Alpha meaning comes from the mesh: you or a scan decide. |
| `landscape_diffuse` | Color | BC7 | UNORM | data | Alpha is the snow/height mask. BC1 is refused (`FMT-LANDSCAPE-BC1`). |
| `pbr_albedo` | Color | BC7 | sRGB | remove | `cs` only. |
| `pbr_emissive` | Color | BC6H for float sources above 1.0, else BC7 | n/a or sRGB | remove | `cs` only. `FMT-HDR-SOURCE` explains the choice. |
| `glow` | Color | BC7 | UNORM | remove | |
| `pbr_subsurface` | Color | BC7 | sRGB | data | `cs` only. Also the coat colour map. |
| `pbr_fuzz` | Color | BC7 | sRGB | data | `cs` only. |
| `subsurface_tint` | Color | BC7 | UNORM | remove | |
| `backlight_tint` | Color | BC7 | UNORM | remove | |
| `ui_sky` | Color | BC7 | UNORM | remove | No mipmaps. |
| `skin_albedo` | Color | BC7 | UNORM | remove | `cs` Advanced skin. UNORM because the CS skin shader reads it that way. |
| `normal` | Normal | BC7 | UNORM | data | Alpha often holds specular. |
| `landscape_normal` | Normal | BC7 | UNORM | data | |
| `pbr_normal` | Normal | BC7 | UNORM | remove | `cs` only. Vanilla specular in alpha is dropped (`ALPHA-PBR-NORMAL-DROPPED`). |
| `msn` | Normal | BC7 | UNORM | remove | Model-space normal. |
| `coat_normal` | Normal | BC7 | UNORM | data | `cs` only. Normal plus coat roughness in alpha; bakes its own detail into that roughness. |
| `skin_wet` | Normal | BC7, or BC4 for grey content | UNORM | data | `cs` Advanced skin. `SKIN-WET-FORM` says which form was read. |
| `envmask` | Grayscale | BC4 | UNORM | remove, exact opaque | Environment mask (`_m`). |
| `height` | Grayscale | BC4 | UNORM | remove | Parallax height (`_p`). |
| `msn_specular` | Grayscale | BC4 | UNORM | remove | `_s` beside an `_msn`. |
| `skin_sk` | Grayscale | BC4 | UNORM | remove | `cs` Advanced skin; red channel only. |
| `complex_material` | MixedPacked | BC7 | UNORM | data, exact opaque | Complex `_m`. Under every target (Note AT). |
| `rmaos` | MixedPacked | BC7 | UNORM | data | `cs` only. |
| `skin_rfaos` | MixedPacked | BC7 | UNORM | data | `cs` only. Also accepted as `rfaos`. |
| `flow` | MixedPacked | BC5 | UNORM | remove | `cs` only. Hair flow; at 32 px or less CS ignores it (`DIM-FLOW-SMALL`). |
| `generic_two_channel` | MixedPacked | BC5 | UNORM | remove | |
| `generic_data` | MixedPacked | BC7 | UNORM | data | Use for any data map Tessalith has no type for. |
| `complex_grass` | MixedPacked | BC7 only | UNORM only | cutout (when the alpha varies) | Always BC7 UNORM at its own size. See [Complex grass](Set-Shaders.md#complex-grass). |
| `hdr_sky` | Hdr | BC6H | n/a | | `cs` only. |
| `cubemap` | Color | BC7 (BC1 for `vanilla_metal` and `room`) | UNORM | remove | An `_e` environment cube: six faces, a cross, a strip, a 360° panorama, a mirror ball, an angular map or a single photo in, one cube DDS with a full mip chain out. BC1, BC3, BC7 or RGBA8 only. Colour is the game's 1.6 curve, not sRGB. See [Cubemaps](Cubemaps.md). |

Notes on the defaults:

- A single photo as a cubemap (`cubemap.layout = "photo"`) is an approximation: the photo becomes the front half of a mirrored fake panorama, and the cube is blurred by default (0.6). It gives soft reflections, not an exact sky.
- The "default alpha" is the starting point. Analysis changes it: a source with no alpha channel resolves to `remove`, and alpha whose minimum is 251 or more is treated as opaque (`ALPHA-NEAR-OPAQUE`). A colour map whose alpha really varies is undecided unless scanned meshes supply one.
- Exact opaque means opaque alpha decodes as exactly 255 in BC7 rather than 254. It is on for `_m` maps because PGPatcher and ENB read their alpha; turning it off for a BC7 `_m` is blocked (`ALPHA-EXACT-OFF-M`).
- Normal maps are never BC4 or BC5: the game would decode Z as -1 and lose alpha (`FMT-BC5-NORMAL`, blocking).

The full list of format, tag and target rules is on [Presets and targets](Presets-and-Targets.md).

### Formats

| Format | Id | Contents | Use |
|---|---|---|---|
| BC1 | `bc1` | RGB, opaque only | Opaque colour in the compact presets. |
| BC3 | `bc3` | RGBA | Available, not chosen by any built-in preset. |
| BC4 | `bc4` | One channel | Grayscale maps. |
| BC5 | `bc5` | Two channels | Flow and generic two-channel maps. |
| BC6H | `bc6h_uf16` | HDR RGB, unsigned | HDR emissive and sky, `cs` only. |
| BC7 | `bc7` | RGBA | Default for almost everything. |
| RGBA8 | `rgba8` | Uncompressed, written as B8G8R8A8 | |
| R8 | `r8` | Uncompressed, one channel | |
| RG8 | `rg8` | Uncompressed, two channels | |
| RGBA16F | `rgba16f` | Uncompressed half-float | `cs` only. |

Encoders: bc7e for BC7, rgbcx for BC1/BC3/BC4/BC5, the ISPC Texture Compressor for BC6H. NVTT can replace bc7e and the ISPC encoder for BC7 and BC6H; see [NVTT backend](NVTT-Backend.md).

## Packing, helpers and shaders

- How raw maps pack, helper textures and RNM normal layers: [Texture packing](Texture-Packing.md).
- Set shaders, height maps, generated maps, complex grass and the extra exports: [Shaders and height maps](Set-Shaders.md).

## Alpha policies

| Policy | Meaning | Mip handling |
|---|---|---|
| `remove` | No alpha; stored opaque. | |
| `cutout` | Alpha testing against `alpha.threshold` with `alpha.compare`. | Coverage preserved per mip (unless `mips.coverage = "off"`), so foliage does not thin out in the distance. |
| `blend` | Alpha blending. | Box-averaged opacity. |
| `cutout_blend` | Both. | Coverage preserved. |
| `data` | Alpha is data (specular, height, a mask). | Treated as a plain channel. |

Discarding varying alpha by accident is blocked (`LOSS-ALPHA-DATA`), except when you chose `remove` yourself or the file is a `pbr_normal`.
