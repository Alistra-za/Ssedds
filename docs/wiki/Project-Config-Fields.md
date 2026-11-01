# Project config fields

Every `sdc.toml` field with its values, default and meaning. A lookup page: how settings layer, rules and `[files]` are on [Project config](Project-Config.md).

Every field below can be set in `[defaults]`, `[subtypes.<id>]`, a rule's `overrides`, a `[files]` entry, or with `--set`. Defaults are those of the built-in base preset; a sub-type or preset may change them, as noted.

## Type and colour

| Field | Values | Default | Notes |
|---|---|---|---|
| `subtype` | a sub-type id | from the file name | Refused in a `subtype:` scoped `--set`. In `[defaults]` and `[subtypes]` it is accepted but has no effect. |
| `color.content` | `srgb`, `linear` | `linear`; `srgb` for colour sub-types | What the pixels mean. Affects mip filtering. On a `cubemap` it stays `srgb` but means the game's `pow(c, 1.6)` curve, not the sRGB curve. |
| `color.tag` | `srgb`, `unorm`, `n/a` | `unorm`; `srgb` for PBR colour under `cs`; `n/a` for HDR | The DDS format's `_SRGB` flag. `n/a` for BC6H and RGBA16F. See [Presets and targets](Presets-and-Targets.md#srgb-tagging). |

## Alpha

| Field | Values | Default | Notes |
|---|---|---|---|
| `alpha.policy` | `remove`, `cutout`, `blend`, `cutout_blend`, `data` | `remove`; `data` for packed and data maps | `remove` stores opaque alpha. `cutout` is alpha testing, `blend` alpha blending, `cutout_blend` both. `data` keeps alpha as numbers with no visual meaning. |
| `alpha.threshold` | 1 to 254 | 127 | The alpha-test threshold the mesh uses. Used for coverage preservation. |
| `alpha.compare` | `gt`, `ge` | `gt` | Greater than, or greater than or equal, as the mesh tests. |
| `alpha.exact_opaque` | `true`, `false` | `false`; `true` for `envmask` and `complex_material` | Guarantee that opaque alpha decodes as exactly 255 in BC7, not 254. |
| `alpha.exr_straight` | `true`, `false` | `false` | An OpenEXR source with alpha is read as premultiplied (the OpenEXR standard) and its colour divided by alpha on load. `true` reads it as straight alpha, as stored. |

## Channels

| Field | Values | Default | Notes |
|---|---|---|---|
| `channels.swizzle` | four characters from `r g b a 0 1` | none | Reorder channels before encoding, for example `"bgra"` or `"rrr1"`. Anything else is `CFG-INVALID`. |
| `channels.flip_green` | `true`, `false` | `false` | Invert the green channel of a normal map. |
| `channels.green_convention` | `directx`, `opengl` | unset | Which way the source normal map's green points. `opengl` turns on `flip_green`. A plain `_normal` file with this unset is converted as DirectX and notes `IN-GREEN-UNSTATED` (info). An explicit `flip_green` that contradicts the convention gives `CFG-GREEN-CONFLICT`, and the explicit `flip_green` wins. |
| `channels.reconstruct_z` | `true`, `false` | `false` | Rebuild the normal's blue channel from red and green. |
| `channels.grayscale_source` | `r`, `luma` | `r` | How a grayscale value is read from a colour source: the red channel, or luminance. |

## Mipmaps

| Field | Values | Default | Notes |
|---|---|---|---|
| `mips.levels` | `full`, `none`, or a count | `full`; `none` for `ui_sky` and under `interface` and `sky` folders | Number of mip levels. |
| `mips.filter` | `box`, `tent`, `mitchell`, `kaiser` | `box` | Downsampling filter. Non-box filters are allowed only on sRGB colour content without alpha weighting (not `cutout`, `blend` or `cutout_blend`). HDR textures may use `box` or `tent`. An unscoped filter a texture cannot use falls back to box with `CFG-FILTER-IGNORED` (info); a scoped one is `CFG-INVALID`. |
| `mips.edge` | `wrap`, `clamp` | `wrap` | How tent, Mitchell and Kaiser read past the texture edge. Ignored by box. |
| `mips.normal` | `renormalize`, `renormalize_bake` | `renormalize`; `renormalize_bake` for `coat_normal` | How normal-map mips are built. `renormalize_bake` also bakes the normal detail lost in each mip into the roughness stored in the same texture (the coat roughness of a `_cnr`). |
| `mips.normal_base` | `renormalize`, `keep` | `renormalize` | Mip 0 of a normal map: stored at unit length, or as authored. |
| `mips.bake_clamp` | greater than 0, up to 1 | 0.25 | Limits how much roughness the variance bake can add (at most the square of this value). |
| `mips.coverage` | `auto`, `on`, `off` | `auto` | Alpha-coverage preservation. Only ever applies to `cutout` and `cutout_blend`, where `auto` and `on` both enable it. |

## Format and size

| Field | Values | Default | Notes |
|---|---|---|---|
| `format` | `bc1`, `bc3`, `bc4`, `bc5`, `bc6h_uf16`, `bc7`, `rgba8`, `r8`, `rg8`, `rgba16f` | `bc7`; see [Texture types](Texture-Types.md#formats) | `bc6h_uf16` and `rgba16f` only under `cs`. BC1 is opaque-only: any alpha policy other than `remove` gives `FMT-BC1-ALPHA` (blocking). `rgba8` is written as B8G8R8A8. |
| `header` | `auto`, `dx10`, `legacy` | `auto` | DDS header form. `auto` writes a legacy header for BC1, BC3 and RGBA8 without the `_SRGB` tag, and the DX10 header for everything else. `legacy` is valid only for formats that have a legacy form (such as BC1, BC3, BC4, BC5) and no `_SRGB` tag; otherwise it is `CFG-INVALID`. |
| `resize` | `none`, `pot-down`, `half` | `none` | `pot-down` shrinks each side to the largest power of two that fits (1000x600 becomes 512x512). `half` halves both sides when the larger side is above 512 (1024x512 becomes 512x256). A resized texture gets `DIM-RESIZED`. |

## Encoder

| Field | Values | Default | Notes |
|---|---|---|---|
| `encoder.backend` | `bc7e`, `rgbcx`, `ispc_bc6h`, `nvtt`, `uncompressed` | follows the format | `nvtt` works for BC7 and BC6H only. See [NVTT backend](NVTT-Backend.md). |
| `encoder.effort` | `ultrafast`, `veryfast`, `fast`, `basic`, `veryslow`, `slowest`, `max` | `veryslow` | bc7e quality level. `max` is the sdc patch-0002 level from the [bc7e tuning study](../research/bc7e-tuning.md); a build whose bc7e objects lack the patch refuses it with `CFG-INVALID` (release builds carry it since 2026-10-18). `max` takes about 3× the time of `veryslow` (2.7 to 2.9× measured on Skyrim textures). For BC6H, `ultrafast` and `veryfast` map to veryfast and `veryslow` to veryslow; `slowest` and `max` both use a peak-preserving veryslow that keeps small, very bright highlights (a sun) near their source brightness. |
| `encoder.rgbcx_level` | 0 to 18 | 18 | rgbcx quality level. Checked and used only where rgbcx is the backend (BC1, BC3, BC4, BC5). |
| `encoder.perceptual` | `true`, `false` | `false` | Perceptual error metric for BC7. Allowed only on colour sub-types whose alpha policy is not `data`; anywhere else it is an error (`FMT-PERCEPTUAL-DATA`). |
| `encoder.bc7_weights` | `linear`, `alpha2`, `alpha8`, `alpha16` | unset | BC7 linear error weights for bc7e (R, G, B, A): `linear` 1,1,1,1, the same encode as unset (use it to override an inherited weighting for one file); `alpha2` 1,1,1,2; `alpha8` 1,1,1,8 (an alternative to BC3 for delicate alpha); `alpha16` 1,1,1,16 (a `_n` with specular in alpha). Unset is today's linear 1,1,1,1, and is not written to profiles or manifests. `encoder.perceptual = true` wins over it. On another format or backend, or with perceptual on, it has no effect and raises `ENC-WEIGHTS-IGNORED`. The GUI's *BC7 weighting* row edits both fields. |
| `encoder.bc1_transparent_black` | `true`, `false` | `false` | BC1 output is opaque-only in this version, so `true` together with `format = "bc1"` is `CFG-INVALID`. |
| `encoder.bc45_search_radius` | `default`, `max`, `fast` | `default` | BC4/BC5 endpoint search. `max` searches a radius of 10, `fast` a radius of 4. |

## Validation

| Field | Values | Default | Notes |
|---|---|---|---|
| `validation.min_mip_psnr_db` | number | 30.0 | Minimum PSNR per mip before `VAL-MIP-ERROR`. |
| `validation.coverage_tolerance_pts` | number | 2.0 | Allowed alpha-coverage drift per mip, in percentage points, before `VAL-COVERAGE`. |
| `validation.complex_m_warn_alpha` | 0 to 255 | 247 | Complex-material alpha above which `VAL-M-COMPLEX-MARGIN` warns. |
| `validation.bc6h_min_peak_ratio` | number | 0.8 | BC6H peak-brightness ratio below which `VAL-BC6H-PEAK` warns. |
| `validation.pgpatcher_non_white_share` | number | 0.5 | Share of non-white texels in a plain `_m` above which `VAL-PGPATCHER-50` warns. |
| `validation.mips` | `all`, `smallest` | `all` | `smallest` runs the PSNR check on the smallest mip only (used by `fast-preview`). Coverage is always checked on every level. |

## Packing

| Field | Values | Default | Notes |
|---|---|---|---|
| `packing.enabled` | `true`, `false` | `true` | Build this packed output. `false` leaves its raw maps as packing inputs (`IN-PACKING-INPUT`). |
| `packing.roughness_default` | 0 to 1 | 0.5 | Roughness used when there is no roughness or glossiness source. |
| `packing.prefer` | `prepacked`, `raw` | `prepacked` | When a set has both a packed file (such as `_rmaos.png`) and raw maps: keep the packed file, or build from the raw maps (`PACK-CONFLICT`). |
| `packing.recipe` | `auto`, `rmaos`, `rfaos` | `auto` | Which `cs` recipe a set packs. `auto` builds `_rfaos` when a fuzz source exists, otherwise `_rmaos`. |
| `packing.fill.<role>` | 0 to 255 | unset | Value for a channel with no source map. Roles: `roughness`, `metallic`, `ao`, `specular`, `glossiness`, `height`, `opacity`, `fuzz`. Unset means the neutral default (AO 1, metallic 0, specular 1, roughness `roughness_default`, height 0). |

Packing fields belong on the packed output's key, for example `[files."textures/armor/iron/iron_rmaos"]`.

## RNM normal layers

These apply to a base normal (`_n`, `pbr_normal`, `landscape_normal`, or `_cnr`) that has layer files. See [Texture types](Texture-Packing.md#rnm-normal-layers).

| Field | Values | Default | Notes |
|---|---|---|---|
| `rnm.enabled` | `true`, `false` | `true` | Blend the layers into the base normal. |
| `rnm.order` | list of `bump`, `bump2`, `detail`, `detail2`, `bent` | `["bent", "bump", "bump2", "detail", "detail2"]` | Blend order. Missing ids are appended in the default order. |
| `rnm.<layer>.strength` | 0 to 2 | bump 0.4, bump2 0.4, detail 0.6, detail2 0.3, bent 0.35 | Layer strength. `<layer>` is `bump`, `bump2`, `detail`, `detail2` or `bent`. |
| `rnm.<layer>.flip_green` | `true`, `false` | the base's `channels.flip_green` | Invert this layer's green channel. |
| `rnm.<layer>.mask` | `true`, `false` | `true` | Use the layer's mask file when one exists. |
| `rnm.bent.mode` | `ao_blend`, `detail` | `ao_blend` | Blend the bent normal weighted by occlusion, or like a detail layer. |
| `rnm.bent.ao_power` | 0.1 to 8 | 1.5 | Power on the occlusion (1 minus AO). |
| `rnm.bent.ao` | `true`, `false` | `true` | Read the AO source (a raw `_ao` map, or the B channel of `_rmaos`/`_rfaos`); `false` uses a uniform weight. Supply normal AO (white = open); Tessalith inverts it. |
| `rnm.target` | `n`, `cnr`, `both` | from the layer's name | Per layer file (`[files]` or `file:` `--set`), or an unscoped `--set` for every layer file; refused in a `subtype:` scope. Which base normal it feeds when a set has both `_n` and `_cnr`. |

## Cubemap

These apply to the `cubemap` sub-type (`_e` files and files under a `cubemaps` folder) and are ignored by every other. See [Cubemaps](Cubemaps.md).

| Field | Values | Default | Notes |
|---|---|---|---|
| `cubemap.style` | `vanilla_metal`, `room`, `eye`, `hd` | unset | Fills the keys below that nothing else set: `vanilla_metal` 32 px, BC1, blur 0.6, saturation 0.8; `room` 128 px, BC1, blur 0.35; `eye` 256 px, BC7, blur 0.35; `hd` 512 px, BC7, blur 0.1. |
| `cubemap.layout` | `faces`, `hcross`, `vcross`, `hstrip`, `vstrip`, `equirect`, `mirror_ball`, `angular`, `photo` | from the image shape | How each layout is detected: [Cubemap sources](Cubemap-Sources.md#ways-to-make-one). `faces` is six separate files. A cube DDS must leave it unset. |
| `cubemap.face_size` | a power of two, 4 to 4096 | the largest power of two inside one source cell, at most 4096 | Face edge in pixels ([source cell per layout](Cubemap-Sources.md#face-size)). |
| `cubemap.yaw` | −360 to 360 | 0 | Turns the cube about world up, in degrees. Positive is clockwise seen from above, the way Skyrim headings turn: 90 moves east to south. |
| `cubemap.mirror` | `true`, `false` | `false` | Swaps world east and west. |
| `cubemap.face_rotate` | six of 0, 90, 180, 270 | `[0, 0, 0, 0, 0, 0]` | Clockwise turn per face, in the order +X, −X, +Y, −Y, +Z, −Z. Applied before the flip. Works on every layout. |
| `cubemap.face_flip` | six of `"none"`, `"h"`, `"v"` | all `"none"` | `h` mirrors left-right, `v` flips top-bottom. |
| `cubemap.vcross_negz_rotate` | `true`, `false` | `true` | Turn a vertical cross's bottom cell (−Z) by 180°. Other layouts ignore it. |
| `cubemap.blur` | 0 to 1 | none (0), the style's, or 0.6 for `photo` | Cone blur across the faces. A `photo` layout blurs at 0.6 unless you set this. |
| `cubemap.exposure` | −10 to 10 | 0 | Stops, in linear light. 1 doubles the light. |
| `cubemap.brightness` | −1 to 1 | 0 | Added to each channel, on the game curve. |
| `cubemap.contrast` | −1 to 1 | 0 | Scales around mid-grey by (1 + contrast), on the game curve. |
| `cubemap.midtones` | 0.1 to 10 | 1 | A power curve on the game curve: above 1 brightens the mid-tones, below 1 darkens them, 1 is off. |
| `cubemap.saturation` | 0 to 2 | 1 | 0 is grey. |
| `cubemap.tint` | `[R, G, B]`, each 0 to 255 | unset | Each channel is multiplied by its value / 255, on the game curve (255, 255, 255 changes nothing). |
| `cubemap.highlight_clamp` | 1 or more | unset | Soft-compresses brightness so that this linear level becomes full white; lower values are also pulled down a little, and hue is kept. For HDR sources. |
| `cubemap.ground_fill` | `"gradient"` or `[R, G, B]`, each 0 to 255 | unset (no fill) | Replaces everything below `cubemap.horizon`. `"gradient"` starts from the colour at the horizon and darkens it to half at straight down. A colour is read as code values on the game curve (`pow(c, 1.6)`), not sRGB. Unset means no fill, and `horizon` alone does nothing. |
| `cubemap.horizon` | −45 to 45 | 0 | Horizon elevation in degrees: 0 is level, positive is above it. Read only with `ground_fill`. |

The order the cube is built in is in [Cubemaps](Cubemaps.md#order-of-work).

A cube is written as BC1, BC3, BC7 or RGBA8 with the UNORM tag; any other `format` or an sRGB tag is `CFG-INVALID`. `mips.levels = "none"` raises `CUBE-NO-MIPS`.

## Re-encoding DDS inputs

| Field | Values | Default | Notes |
|---|---|---|---|
| `reencode` | `true`, `false` | `false` | Per file only (`[files]`, unscoped or `file:` `--set`). DDS inputs are skipped unless this is `true`. |
