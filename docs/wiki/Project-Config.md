# Project config (sdc.toml)

A project config records how one folder of textures is converted. It is a TOML file named `sdc.toml`, normally placed in the folder you convert. The CLI reads `sdc.toml` from the first input folder unless you pass `--config <file>`. The GUI reads and writes the `sdc.toml` in the folder you open.

There is also a user config at `%APPDATA%\sdc\config.toml`. It has the same format, minus `[files]` and `[inputs]`, and applies to every project. Put personal defaults there, such as a preferred target.

Unknown keys and values outside their allowed range are errors. The whole run stops with exit code 2 (CLI), or the GUI shows a banner and stays read-only until you fix the file and press Reload.

## A complete example

```toml
schema_version = 1
preset = "cs-quality"
target = "cs"

# Applies to every texture, before sub-type tables
[defaults]
encoder.effort = "veryslow"
mips.filter = "box"

# Applies to one sub-type
[subtypes.diffuse]
mips.filter = "kaiser"

[subtypes.normal]
channels.green_convention = "directx"

# Rules: first match per file wins
[[rules]]
match = { glob = "**/interface/**" }
overrides = { mips = { levels = "none" } }

[[rules]]
match = { glob = "**/foliage/**", suffix = "" , alpha = "varying" }
overrides = { alpha = { policy = "cutout", threshold = 100 }, mips = { coverage = "on" } }

[[rules]]
match = { glob = "**/*_mask.png" }
subtype = "generic_data"

# Per-file settings, keyed by path relative to this file's folder
[files."textures/armor/iron/iron.png"]
alpha.policy = "blend"

[files."textures/armor/iron/iron_rmaos"]
packing.fill.metallic = 255

[files."textures/armor/iron/iron_d.png"]
resize = "pot-down"
allow = ["DIM-NPOT"]

# Read a misnamed file under another name; the file on disk is not renamed
[inputs."textures/armor/iron/iron_normalmap.png"]
as = "textures/armor/iron/iron_n.png"
```

## Top-level keys

| Key | Type | Default | Meaning |
|---|---|---|---|
| `schema_version` | integer, 1 or more | 1 | Format version of this file. A version newer than this build supports is an error: `<file>: config schema_version N is newer than this sdc supports (1)`. |
| `preset` | string | the target's default | The preset to use, by id. A built-in id or one from `%APPDATA%\sdc\presets\*.toml`. See [Presets and targets](Presets-and-Targets.md). |
| `target` | `"cs"`, `"cpm"`, `"vanilla"` | from the preset, else `"cs"` | The target. The GUI's Mode menu writes this. |
| `[defaults]` | partial profile | none | Field values for every texture. |
| `[subtypes.<id>]` | partial profile | none | Field values for one sub-type. |
| `[[rules]]` | array of tables | none | Conditional settings. |
| `[files."<path>"]` | partial profile plus `allow` | none | Settings for one file. Project config only. |
| `[inputs."<path>"]` | table with `as` | none | Read a file under another name. Project config only. |

A user config that contains `[files]` or `[inputs]` is rejected: `[files] is only valid in a project sdc.toml, not in <path>`.

## How settings layer

Every texture's final settings (its profile) are built from layers. Later layers override earlier ones:

1. Built-in defaults, from the hidden `sdc-base` preset.
2. The selected preset and its base chain: all `[defaults]` tables from base to child, then all `[subtypes]` tables from base to child.
3. The user config: `[defaults]`, then `[subtypes.<id>]`.
4. The project config: `[defaults]`, then `[subtypes.<id>]`.
5. Rules: from the preset chain, then the user config, then the project config. In each of those sources, only the first rule that matches the file applies.
6. Facts from analysing the image, such as a High-confidence alpha policy (no alpha channel means `remove`).
7. The set shader's member settings, and alpha from scanned meshes (GUI only).
8. Facts from the file's suffix.
9. Accepted suggestions (`--accept-suggestions high`, or always in the GUI).
10. The file's `[files]` entry.
11. `--set` on the command line: unscoped first, then `subtype:` scoped, then `file:` scoped; then `--fill`.

`sdc presets show` and `sdc inspect --json` report the provenance of every field, so you can see which layer set it. The GUI shows it next to each value.

### Format and encoder

When a layer changes only `format`, the encoder follows it: BC1, BC3, BC4 and BC5 use `rgbcx`, BC6H uses `ispc_bc6h`, BC7 uses `bc7e`, and uncompressed formats use `uncompressed`. A backend that does not fit the new format is kept only if the same layer set both, or a more specific layer pinned it, in which case the profile is invalid (`CFG-INVALID`). So `defaults.encoder.backend = "nvtt"` applies to BC7 and BC6H textures and leaves BC4 and BC5 textures on rgbcx.

## [defaults] and [subtypes]

Both take a partial profile: any of the fields in the [field reference](#field-reference), written as nested tables or dotted keys. These two are the same:

```toml
[subtypes.diffuse]
alpha = { policy = "cutout", threshold = 100 }
```

```toml
[subtypes.diffuse]
alpha.policy = "cutout"
alpha.threshold = 100
```

Sub-type ids are listed on [Texture types](Texture-Types.md). `subtype` itself cannot be set in a sub-type table.

## [[rules]]

```toml
[[rules]]
match = { glob = "...", suffix = "...", set_has = "...", alpha = "...", subtypes = ["..."] }
subtype = "<sub-type id>"     # optional
overrides = { ... }           # optional partial profile
```

| Match key | Meaning |
|---|---|
| `glob` | Glob over the file's path, case-insensitive: the game path (from the last `textures` folder), the path relative to the input folder, or, for a glob without `/`, the file name. `*` does not cross `/`; `**` does. |
| `suffix` | The file's recognised suffix without the underscore, such as `"n"` or `"rmaos"`. `""` means no suffix (the colour map). |
| `set_has` | The file's texture set contains a member with this suffix. |
| `alpha` | The source's alpha content: `"none"` (no alpha channel, or varying alpha that scanned meshes decided to remove), `"constant255"`, `"varying"`. |
| `subtypes` | The file's sub-type is one of these. A rule with this key never decides a sub-type; it only adds settings once the sub-type is known. |

All given match keys must match. A rule with no match keys matches everything.

`subtype` names the sub-type for matching files. A rule's `subtype` decides the file's sub-type ahead of suggestions, but below a `--set subtype` or a `[files]` entry. When a file's sub-type was decided by something else, a rule that names a different `subtype` does not apply to it.

`overrides` is a partial profile applied to matching files.

Examples:

```toml
# Everything under a "decals" folder blends
[[rules]]
match = { glob = "**/decals/**", subtypes = ["diffuse"] }
overrides = { alpha = { policy = "blend" } }

# Files ending _ids.png are data
[[rules]]
match = { glob = "*_ids.png" }
subtype = "generic_data"

# BC1 for opaque colour maps
[[rules]]
match = { alpha = "none", subtypes = ["diffuse"] }
overrides = { format = "bc1" }

# In sets that have a parallax map, keep the diffuse at full effort
[[rules]]
match = { set_has = "p", suffix = "" }
overrides = { encoder = { effort = "slowest" } }
```

## [files]

Each key is a path relative to the folder that holds `sdc.toml`, with forward slashes. Matching ignores case and works with or without the file extension. A packed output, which has no source file of its own, is keyed by its output path without an extension:

```toml
[files."landscape/rock01_rmaos"]
packing.fill.specular = 64
```

An entry takes any field from the [field reference](#field-reference), including `subtype`, plus:

| Key | Meaning |
|---|---|
| `allow` | List of blocking codes to acknowledge for this file only, for example `allow = ["DIM-NPOT", "TAG-UNORM-PBR"]`. An unknown code is an error. |
| `reencode` | `true` to convert a DDS input instead of skipping it (`IN-DDS-SKIPPED`). Re-encoding a DDS loses quality (`LOSS-REENCODE`). |
| `rnm.target` | For an RNM layer file: which base normal it feeds, `"n"`, `"cnr"` or `"both"`. |

An entry that names no file under the current target is reported as `CFG-FILE-UNMATCHED` (info), and the GUI lists it under "Unmatched [files] entries" in the Advanced drawer so you can remove it.

The GUI writes only `target`, the `[files.*]` tables, `[inputs.*]` aliases and `defaults.packing.fill.*`. Everything else in your `sdc.toml`, including comments and order, is left as it was.

## [inputs]

```toml
[inputs."textures/armor/iron/iron_normalmap.png"]
as = "textures/armor/iron/iron_n.png"
```

The file is grouped, typed, named and written as if it were at the `as` path. The file on disk is not renamed. `as` is relative to the folder holding `sdc.toml` and keeps the same extension; you can leave the extension out. The file keeps its own `[files]` entry, under its real name.

An alias must stay under the project folder (no absolute path, no `..`), keep the file's extension, not be empty, and not equal another file's real or aliased path (compared case-insensitively). An alias that breaks these rules is ignored and reported as `IN-ALIAS-INVALID` (warning); the file is planned under its real name. An `[inputs]` key that matches no file is reported as `IN-ALIAS-STALE` (info). The GUI's rename dialog writes these entries in its Inputs mode.

## Field reference

Every field below can be set in `[defaults]`, `[subtypes.<id>]`, a rule's `overrides`, a `[files]` entry, or with `--set`. Defaults are those of the built-in base preset; a sub-type or preset may change them, as noted.

### Type and colour

| Field | Values | Default | Notes |
|---|---|---|---|
| `subtype` | a sub-type id | from the file name | Not settable in `[defaults]`, `[subtypes]` or `subtype:` scopes. |
| `color.content` | `srgb`, `linear` | `linear`; `srgb` for colour sub-types | What the pixels mean. Affects mip filtering. |
| `color.tag` | `srgb`, `unorm`, `n/a` | `unorm`; `srgb` for PBR colour under `cs`; `n/a` for HDR | The DDS format's `_SRGB` flag. `n/a` for BC6H and RGBA16F. See [Presets and targets](Presets-and-Targets.md#srgb-tagging). |

### Alpha

| Field | Values | Default | Notes |
|---|---|---|---|
| `alpha.policy` | `remove`, `cutout`, `blend`, `cutout_blend`, `data` | `remove`; `data` for packed and data maps | `remove` stores opaque alpha. `cutout` is alpha testing, `blend` alpha blending, `cutout_blend` both. `data` keeps alpha as numbers with no visual meaning. |
| `alpha.threshold` | 1 to 254 | 127 | The alpha-test threshold the mesh uses. Used for coverage preservation. |
| `alpha.compare` | `gt`, `ge` | `gt` | Greater than, or greater than or equal, as the mesh tests. |
| `alpha.exact_opaque` | `true`, `false` | `false`; `true` for `envmask` and `complex_material` | Guarantee that opaque alpha decodes as exactly 255 in BC7, not 254. |

### Channels

| Field | Values | Default | Notes |
|---|---|---|---|
| `channels.swizzle` | four characters from `r g b a 0 1` | none | Reorder channels before encoding, for example `"bgra"` or `"rrr1"`. Anything else is `CFG-INVALID`. |
| `channels.flip_green` | `true`, `false` | `false` | Invert the green channel of a normal map. |
| `channels.green_convention` | `directx`, `opengl` | unset | Which way the source normal map's green points. `opengl` turns on `flip_green`. A plain `_normal` file stays unresolved (`IN-GREEN-UNSTATED`) until this is set. An explicit `flip_green` that contradicts the convention gives `CFG-GREEN-CONFLICT`, and the explicit `flip_green` wins. |
| `channels.reconstruct_z` | `true`, `false` | `false` | Rebuild the normal's blue channel from red and green. |
| `channels.grayscale_source` | `r`, `luma` | `r` | How a grayscale value is read from a colour source: the red channel, or luminance. |

### Mipmaps

| Field | Values | Default | Notes |
|---|---|---|---|
| `mips.levels` | `full`, `none`, or a count | `full`; `none` for `ui_sky` and under `interface` and `sky` folders | Number of mip levels. |
| `mips.filter` | `box`, `tent`, `mitchell`, `kaiser` | `box` | Downsampling filter. Non-box filters are allowed only on sRGB colour content without alpha weighting (not `cutout`, `blend` or `cutout_blend`). HDR textures may use `box` or `tent`. An unscoped filter a texture cannot use falls back to box with `CFG-FILTER-IGNORED` (info); a scoped one is `CFG-INVALID`. |
| `mips.edge` | `wrap`, `clamp` | `wrap` | How tent, Mitchell and Kaiser read past the texture edge. Ignored by box. |
| `mips.normal` | `renormalize`, `renormalize_bake` | `renormalize`; `renormalize_bake` for `coat_normal` | How normal-map mips are built. `renormalize_bake` also bakes the normal detail lost in each mip into the roughness stored in the same texture (the coat roughness of a `_cnr`). |
| `mips.normal_base` | `renormalize`, `keep` | `renormalize` | Mip 0 of a normal map: stored at unit length, or as authored. |
| `mips.bake_clamp` | greater than 0, up to 1 | 0.25 | Limits how much roughness the variance bake can add (at most the square of this value). |
| `mips.coverage` | `auto`, `on`, `off` | `auto` | Alpha-coverage preservation. Only ever applies to `cutout` and `cutout_blend`, where `auto` and `on` both enable it. |

### Format and size

| Field | Values | Default | Notes |
|---|---|---|---|
| `format` | `bc1`, `bc3`, `bc4`, `bc5`, `bc6h_uf16`, `bc7`, `rgba8`, `r8`, `rg8`, `rgba16f` | `bc7`; see [Texture types](Texture-Types.md#formats) | `bc6h_uf16` and `rgba16f` only under `cs`. BC1 is opaque-only: any alpha policy other than `remove` gives `FMT-BC1-ALPHA` (blocking). `rgba8` is written as B8G8R8A8. |
| `header` | `auto`, `dx10`, `legacy` | `auto` | DDS header form. `auto` writes a legacy header for BC1, BC3 and RGBA8 without the `_SRGB` tag, and the DX10 header for everything else. `legacy` is valid only for formats that have a legacy form (such as BC1, BC3, BC4, BC5) and no `_SRGB` tag; otherwise it is `CFG-INVALID`. |
| `resize` | `none`, `pot-down`, `half` | `none` | `pot-down` shrinks each side to the largest power of two that fits (1000x600 becomes 512x512). `half` halves both sides when the larger side is above 512 (1024x512 becomes 512x256). A resized texture gets `DIM-RESIZED`. |

### Encoder

| Field | Values | Default | Notes |
|---|---|---|---|
| `encoder.backend` | `bc7e`, `rgbcx`, `ispc_bc6h`, `nvtt`, `uncompressed` | follows the format | `nvtt` works for BC7 and BC6H only. See [NVTT backend](NVTT-Backend.md). |
| `encoder.effort` | `ultrafast`, `veryfast`, `fast`, `basic`, `veryslow`, `slowest` | `veryslow` | bc7e quality level. For BC6H, `ultrafast` and `veryfast` map to veryfast, and `veryslow` and `slowest` to veryslow. |
| `encoder.rgbcx_level` | 0 to 18 | 18 | rgbcx quality level. Checked and used only where rgbcx is the backend (BC1, BC3, BC4, BC5). |
| `encoder.perceptual` | `true`, `false` | `false` | Perceptual error metric for BC7. Allowed only on colour sub-types whose alpha policy is not `data`; anywhere else it is an error (`FMT-PERCEPTUAL-DATA`). |
| `encoder.bc1_transparent_black` | `true`, `false` | `false` | BC1 output is opaque-only in this version, so `true` together with `format = "bc1"` is `CFG-INVALID`. |
| `encoder.bc45_search_radius` | `default`, `max`, `fast` | `default` | BC4/BC5 endpoint search. `max` searches a radius of 10, `fast` a radius of 4. |

### Validation

| Field | Values | Default | Notes |
|---|---|---|---|
| `validation.min_mip_psnr_db` | number | 30.0 | Minimum PSNR per mip before `VAL-MIP-ERROR`. |
| `validation.coverage_tolerance_pts` | number | 2.0 | Allowed alpha-coverage drift per mip, in percentage points, before `VAL-COVERAGE`. |
| `validation.complex_m_warn_alpha` | 0 to 255 | 247 | Complex-material alpha above which `VAL-M-COMPLEX-MARGIN` warns. |
| `validation.bc6h_min_peak_ratio` | number | 0.8 | BC6H peak-brightness ratio below which `VAL-BC6H-PEAK` warns. |
| `validation.pgpatcher_non_white_share` | number | 0.5 | Share of non-white texels in a plain `_m` above which `VAL-PGPATCHER-50` warns. |
| `validation.mips` | `all`, `smallest` | `all` | `smallest` runs the PSNR check on the smallest mip only (used by `fast-preview`). Coverage is always checked on every level. |

### Packing

| Field | Values | Default | Notes |
|---|---|---|---|
| `packing.enabled` | `true`, `false` | `true` | Build this packed output. `false` leaves its raw maps as packing inputs (`IN-PACKING-INPUT`). |
| `packing.roughness_default` | 0 to 1 | 0.5 | Roughness used when there is no roughness or glossiness source. |
| `packing.prefer` | `prepacked`, `raw` | `prepacked` | When a set has both a packed file (such as `_rmaos.png`) and raw maps: keep the packed file, or build from the raw maps (`PACK-CONFLICT`). |
| `packing.recipe` | `auto`, `rmaos`, `rfaos` | `auto` | Which `cs` recipe a set packs. `auto` builds `_rfaos` when a fuzz source exists, otherwise `_rmaos`. |
| `packing.fill.<role>` | 0 to 255 | unset | Value for a channel with no source map. Roles: `roughness`, `metallic`, `ao`, `specular`, `glossiness`, `height`, `opacity`, `fuzz`. Unset means the neutral default (AO 1, metallic 0, specular 1, roughness `roughness_default`, height 0). |

Packing fields belong on the packed output's key, for example `[files."textures/armor/iron/iron_rmaos"]`.

### RNM normal layers

These apply to a base normal (`_n`, `pbr_normal`, `landscape_normal`, or `_cnr`) that has layer files. See [Texture types](Texture-Types.md#rnm-normal-layers).

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
| `rnm.target` | `n`, `cnr`, `both` | from the layer's name | Per layer file only (`[files]` or `file:` `--set`): which base normal it feeds when a set has both `_n` and `_cnr`. |

### Re-encoding DDS inputs

| Field | Values | Default | Notes |
|---|---|---|---|
| `reencode` | `true`, `false` | `false` | Per file only (`[files]`, unscoped or `file:` `--set`). DDS inputs are skipped unless this is `true`. |

## Checking a config

```powershell
# Any error in sdc.toml stops the plan with exit 2 and names the problem
sdc inspect C:\Work\IronPlate --recursive --accept-suggestions high > $null

# The JSON Schema of the config file
sdc presets schema config
```

The repository also has the schema at `schema/config.schema.json`, which editors that understand JSON Schema for TOML can use for completion.
