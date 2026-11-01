# Project config (sdc.toml)

A project config records how one folder of textures is converted. It is a TOML file named `sdc.toml`, normally placed in the folder you convert. The CLI reads `sdc.toml` from the first input folder unless you pass `--config <file>`. The GUI reads and writes the `sdc.toml` in the folder you open.

There is also a user config at `%APPDATA%\sdc\config.toml`. It has the same format, minus `[files]` and `[inputs]`, and applies to every project. Put personal defaults there, such as a preferred target.

Unknown keys, unknown values and values of the wrong type are errors. The whole run stops with exit code 2 (CLI), or the GUI shows a banner and stays read-only until you fix the file and press Reload. A value of the right type but outside its allowed range (for example `alpha.threshold = 0`) is reported per file as `CFG-INVALID`, and that file is not converted.

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

# Rules: every rule that matches applies, in order; a later rule overrides only the fields it sets
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
| `target` | `"cs"`, `"vanilla"` (`"cpm"` is an error) | from the preset, else `"cs"` | The target. The GUI's Mode menu writes this. |
| `[defaults]` | partial profile | none | Field values for every texture. |
| `[subtypes.<id>]` | partial profile | none | Field values for one sub-type. |
| `[[rules]]` | array of tables | none | Conditional settings. |
| `[files."<path>"]` | partial profile plus `allow` | none | Settings for one file. Project config only. |
| `[inputs."<path>"]` | table with `as` | none | Read a file under another name. Project config only. |
| `[export]` | table | none | Also export as Vanilla / CM. Project config only. See [[export]](#export). |

A user config that contains `[files]` or `[inputs]` is rejected: `[files] is only valid in a project sdc.toml, not in <path>`.

## How settings layer

Every texture's final settings (its profile) are built from layers. Later layers override earlier ones:

1. Built-in defaults, from the hidden `sdc-base` preset.
2. The selected preset and its base chain: all `[defaults]` tables from base to child, then all `[subtypes]` tables from base to child.
3. The user config: `[defaults]`, then `[subtypes.<id>]`.
4. The project config: `[defaults]`, then `[subtypes.<id>]`.
5. Rules: from the preset chain, then the user config, then the project config. In each of those sources, every rule that matches the file applies, in file order: a later rule overrides only the fields it sets, and the earlier rules' other fields stay. A field's provenance names the rule that last set it.
6. Facts from analysing the image, such as a High-confidence alpha policy (no alpha channel means `remove`).
7. The set shader's member settings, and alpha from scanned meshes (GUI only).
8. Facts from the file's suffix.
9. Accepted suggestions (`--accept-suggestions high`, or always in the GUI). A suggestion only picks the sub-type, never a field value. In that choice it ranks below `--set`, `[files]`, rules and a shader you chose for the set.
10. The file's `[files]` entry.
11. `--set` on the command line: unscoped first (`--fill` counts as an unscoped `--set` applied after the others), then `subtype:` scoped, then `file:` scoped. So a scoped `--set packing.fill.<role>` beats `--fill`.

`sdc presets show` and `sdc inspect --json` report the provenance of every field, so you can see which layer set it. The GUI shows it next to each value.

### Format and encoder

When a layer changes only `format`, the encoder follows it: BC1, BC3, BC4 and BC5 use `rgbcx`, BC6H uses `ispc_bc6h`, BC7 uses `bc7e`, and uncompressed formats use `uncompressed`. A backend that does not fit the new format is kept only if the same layer set both, or a pinning layer (a sub-type- or file-scoped `--set`, a rule, or `[files]`) set the backend at least as specifically as the format. Preset and config `[subtypes]` tables never pin. When the backend is kept, the profile is invalid (`CFG-INVALID`). So `defaults.encoder.backend = "nvtt"` applies to BC7 and BC6H textures and leaves BC4 and BC5 textures on rgbcx.

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

Sub-type ids are listed on [Texture types](Texture-Types.md). A `subtype` key in a sub-type table (or in `[defaults]`) is accepted but has no effect: the resolved sub-type overwrites it.

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
| `helper.role` | For an image: what it is as a [helper texture](Texture-Types.md#helper-textures). `"none"` (not a helper: convert it on its own); `"roughness"`, `"metallic"`, `"ao"`, `"specular"`, `"glossiness"`, `"height"`, `"opacity"`, `"fuzz"` (skin fuzz mask), `"orm"`, `"curves"`, `"cavity"`, `"fuzzweight"` (PBR `_f` weight), `"subsurfaceweight"` (PBR `_s` weight); or an RNM layer: `"bump"`, `"bump2"`, `"bumpmask"`, `"bumpmask2"`, `"detailnorm"`, `"detailnorm2"`, `"detailnormmask"`, `"detailnormmask2"`, `"bent"`, `"bentmask"`. Overrides the name and any `subtype`. Ignored on a DDS. |
| `helper.fills` | The channels the helper fills, replacing the suggestion. Alpha: `"diffuse.a"`, `"n.a"`, `"f.a"`, `"s.a"`, `"cnr.a"`. Packed: `"rmaos.r"` to `"rmaos.a"`, `"rfaos.r"` to `"rfaos.a"`, `"m.r"` to `"m.a"`, `"p.r"`. `[]` = not used. Not for RNM layers (use `rnm.target`). |
| `helper.read` | The helper's channel to read: `"r"`, `"g"`, `"b"`, `"a"` or `"luma"`. Unset reads R (or luminance where the recipe does). |
| `helper.invert` | `true` to flip what the helper feeds (one minus the value), for an inverted mask. It flips on top of the recipe's own reading, so leave it off for a gloss map in a roughness channel: `helper.role = "glossiness"` already inverts it there. |

The `helper.*` keys go on the helper file's own entry, never on the output it fills:

```toml
# A mask whose name says nothing: it is the diffuse's opacity, stored in green
[files."textures/armor/iron/iron_mask.png"]
helper.role = "opacity"
helper.read = "g"

# A roughness map that is also the coat roughness (the _cnr alpha), beside its usual _rmaos channel
[files."textures/armor/iron/iron_roughness.png"]
helper.fills = ["rmaos.r", "cnr.a"]

# Convert this specular map on its own instead
[files."textures/armor/iron/iron_specular.png"]
helper.role = "none"
```

Setting `helper.role` or `helper.fills` makes the helper *assigned*: it replaces what its channels already hold. Without them, a helper fills only empty channels. See [Texture types](Texture-Types.md#helper-textures).

An entry that names no file under the current target is reported as `CFG-FILE-UNMATCHED` (info), and the GUI lists it under "Unmatched [files] entries" in the Advanced drawer so you can remove it.

The GUI writes only `target`, the `[files.*]` tables, `[inputs.*]` aliases and `defaults.packing.fill.*`. Everything else in your `sdc.toml`, including comments and order, is left as it was.

## [inputs]

```toml
[inputs."textures/armor/iron/iron_normalmap.png"]
as = "textures/armor/iron/iron_n.png"
```

The file is grouped, typed, named and written as if it were at the `as` path. The file on disk is not renamed. `as` is relative to the folder holding `sdc.toml` and keeps the same extension; you can leave the extension out. The file keeps its own `[files]` entry, under its real name.

An alias must stay under the project folder (no absolute path, no `..`), keep the file's extension, not be empty, and not equal another file's real or aliased path (compared case-insensitively). An alias that breaks these rules is ignored and reported as `IN-ALIAS-INVALID` (warning); the file is planned under its real name. An `[inputs]` key that matches no file is reported as `IN-ALIAS-STALE` (info). The GUI's rename dialog writes these entries in its Inputs mode.

## [export]

```toml
[export]
also = ["vanilla", "cm"]
zip = true
skip = ["textures/pbr/armor/iron/iron"]

[export.names]
vanilla = "Iron Armour SE"

[export.legacy]
brightness = 0.9
```

"Also export as" Vanilla and Complex material (CM): besides the PBR (cs) export, write a vanilla folder and a CM folder converted from the PBR sets. Project `sdc.toml` only: a user config with `[export]` is rejected. Needs the `cs` target. The GUI writes this table (Review's *Also export as* column and Advanced → *Also export recipe*); `sdc convert` reads it (see [CLI Reference](CLI-Reference.md#sdc-convert)). Unknown keys are an error. A value out of range is an error when an Also export runs, naming the key (for example `export.legacy.brightness = 2: must be between 0 and 1.5`).

| Key | Default | Meaning |
|---|---|---|
| `also` | `[]` | `"vanilla"`, `"cm"` or both. Empty is a plain export. `"cm"` alone writes Vanilla too: CM installs on top of it. |
| `zip` | `false` | Also zip each folder that ran into `<folder>.zip`, with a line per zip in `<project> SHA256SUMS.txt`. |
| `skip` | `[]` | Set ids (as `--set set:<id>` names them) left out of Vanilla and CM. |
| `names.pbr`, `names.vanilla`, `names.cm` | `<project> PBR`, `<project> Vanilla`, `<project> CM` | The folder names. `<project>` is the opened folder's name (the CLI: the input folder's). Must be usable folder names, not a Windows device name, and all three different. |

`[export.legacy]` is the recipe (all numbers are fractions of the stored value; "gloss" is 1 − roughness):

| Key | Default | Range | Meaning |
|---|---|---|---|
| `brightness` | 0.9 | 0–1.5 | Diffuse = brightness × albedo × lerp(1, AO, AO strength). |
| `ao_strength` | 0.5 | 0–1 | How much AO darkens an object's diffuse. |
| `ao_strength_landscape` | 0.3 | 0–1 | The same for a PBR Landscape set. |
| `curves_opacity` | 0.4 | 0–1 | Opacity of a `_curves` map's soft light and a `_cavity` map's multiply on the diffuse. |
| `metal_darken` | 0.3 | 0–1 | The vanilla diffuse's factor at full metal (lerp(1, this, metal)). Landscape has no metal. |
| `spec_scale` | 0.66 | 0–2 | `_n` alpha = spec_scale × gloss × AO × the metal boost, clamped to 1. |
| `spec_metal_boost` | 2.0 | 1–3 | The `_n` alpha boost at full metal. |
| `landscape_spec` | 2.5 | 0–5 | A landscape `_n` alpha is multiplied by this before the clamp (no metal boost). |
| `cm_dielectric_env` | 0 | 0–1 | CM `_m` R = gloss × max(metal, this). 0 = no env on non-metals; 0.35 is the spec's option. |
| `keep_metal_brightness` | `false` | | The CM folder gets its own diffuses without the metal darkening. Off: CM holds only `_m` files (plus the complex-grass stack of each PBR grass set). |
| `keep_metal_strength` | 1.0 | 0–1 | How much of the metal darkening Keep metal brightness undoes. |

The CM `_m` is R = gloss × metal (see `cm_dielectric_env`), G = gloss (at least 5/255, so complex material stays on), B = metal, A = the `_p` height clamped to 5/255–250/255 (0 without a `_p`: parallax off). A landscape `_m` has R = B = 0. The vanilla env `_m` is metal × gloss, written only when a Meshes scan shows the set's meshes use the environment-map shader; the CLI reads no scan, so it writes none.

The GUI keeps the folders' location in `sdc-project.toml` as `[project] also_location` (by default the export folder); the CLI writes them under `-o`.

## Field reference

Every field below can be set in `[defaults]`, `[subtypes.<id>]`, a rule's `overrides`, a `[files]` entry, or with `--set`. Defaults are those of the built-in base preset; a sub-type or preset may change them, as noted.

### Type and colour

| Field | Values | Default | Notes |
|---|---|---|---|
| `subtype` | a sub-type id | from the file name | Refused in a `subtype:` scoped `--set`. In `[defaults]` and `[subtypes]` it is accepted but has no effect. |
| `color.content` | `srgb`, `linear` | `linear`; `srgb` for colour sub-types | What the pixels mean. Affects mip filtering. On a `cubemap` it stays `srgb` but means the game's `pow(c, 1.6)` curve, not the sRGB curve. |
| `color.tag` | `srgb`, `unorm`, `n/a` | `unorm`; `srgb` for PBR colour under `cs`; `n/a` for HDR | The DDS format's `_SRGB` flag. `n/a` for BC6H and RGBA16F. See [Presets and targets](Presets-and-Targets.md#srgb-tagging). |

### Alpha

| Field | Values | Default | Notes |
|---|---|---|---|
| `alpha.policy` | `remove`, `cutout`, `blend`, `cutout_blend`, `data` | `remove`; `data` for packed and data maps | `remove` stores opaque alpha. `cutout` is alpha testing, `blend` alpha blending, `cutout_blend` both. `data` keeps alpha as numbers with no visual meaning. |
| `alpha.threshold` | 1 to 254 | 127 | The alpha-test threshold the mesh uses. Used for coverage preservation. |
| `alpha.compare` | `gt`, `ge` | `gt` | Greater than, or greater than or equal, as the mesh tests. |
| `alpha.exact_opaque` | `true`, `false` | `false`; `true` for `envmask` and `complex_material` | Guarantee that opaque alpha decodes as exactly 255 in BC7, not 254. |
| `alpha.exr_straight` | `true`, `false` | `false` | An OpenEXR source with alpha is read as premultiplied (the OpenEXR standard) and its colour divided by alpha on load. `true` reads it as straight alpha, as stored. |

### Channels

| Field | Values | Default | Notes |
|---|---|---|---|
| `channels.swizzle` | four characters from `r g b a 0 1` | none | Reorder channels before encoding, for example `"bgra"` or `"rrr1"`. Anything else is `CFG-INVALID`. |
| `channels.flip_green` | `true`, `false` | `false` | Invert the green channel of a normal map. |
| `channels.green_convention` | `directx`, `opengl` | unset | Which way the source normal map's green points. `opengl` turns on `flip_green`. A plain `_normal` file with this unset is converted as DirectX and notes `IN-GREEN-UNSTATED` (info). An explicit `flip_green` that contradicts the convention gives `CFG-GREEN-CONFLICT`, and the explicit `flip_green` wins. |
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
| `encoder.effort` | `ultrafast`, `veryfast`, `fast`, `basic`, `veryslow`, `slowest`, `max` | `veryslow` | bc7e quality level. `max` is the sdc patch-0002 level from the [bc7e tuning study](../research/bc7e-tuning.md); a build whose bc7e objects lack the patch refuses it with `CFG-INVALID` (release builds carry it since 2026-10-18). `max` takes about 3× the time of `veryslow` (2.7 to 2.9× measured on Skyrim textures). For BC6H, `ultrafast` and `veryfast` map to veryfast and `veryslow` to veryslow; `slowest` and `max` both use a peak-preserving veryslow that keeps small, very bright highlights (a sun) near their source brightness. |
| `encoder.rgbcx_level` | 0 to 18 | 18 | rgbcx quality level. Checked and used only where rgbcx is the backend (BC1, BC3, BC4, BC5). |
| `encoder.perceptual` | `true`, `false` | `false` | Perceptual error metric for BC7. Allowed only on colour sub-types whose alpha policy is not `data`; anywhere else it is an error (`FMT-PERCEPTUAL-DATA`). |
| `encoder.bc7_weights` | `linear`, `alpha2`, `alpha8`, `alpha16` | unset | BC7 linear error weights for bc7e (R, G, B, A): `linear` 1,1,1,1, the same encode as unset (use it to override an inherited weighting for one file); `alpha2` 1,1,1,2; `alpha8` 1,1,1,8 (an alternative to BC3 for delicate alpha); `alpha16` 1,1,1,16 (a `_n` with specular in alpha). Unset is today's linear 1,1,1,1, and is not written to profiles or manifests. `encoder.perceptual = true` wins over it. On another format or backend, or with perceptual on, it has no effect and raises `ENC-WEIGHTS-IGNORED`. The GUI's *BC7 weighting* row edits both fields. |
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
| `rnm.target` | `n`, `cnr`, `both` | from the layer's name | Per layer file (`[files]` or `file:` `--set`), or an unscoped `--set` for every layer file; refused in a `subtype:` scope. Which base normal it feeds when a set has both `_n` and `_cnr`. |

### Cubemap

These apply to the `cubemap` sub-type (`_e` files and files under a `cubemaps` folder) and are ignored by every other. See [Cubemaps](Cubemaps.md).

| Field | Values | Default | Notes |
|---|---|---|---|
| `cubemap.style` | `vanilla_metal`, `room`, `eye`, `hd` | unset | Fills the keys below that nothing else set: `vanilla_metal` 32 px, BC1, blur 0.6, saturation 0.8; `room` 128 px, BC1, blur 0.35; `eye` 256 px, BC7, blur 0.35; `hd` 512 px, BC7, blur 0.1. |
| `cubemap.layout` | `faces`, `hcross`, `vcross`, `hstrip`, `vstrip`, `equirect`, `mirror_ball`, `angular`, `photo` | from the image shape | Only an exact shape is detected: 4:3 is `hcross`, 3:4 `vcross`, 6:1 `hstrip`, 1:6 `vstrip`, 2:1 `equirect`. `faces` is six separate files. `mirror_ball`, `angular` and `photo` are never detected: set them. An explicit `equirect`, `mirror_ball`, `angular` or `photo` takes any non-zero image size. A square image needs this set. A cube DDS must leave it unset. |
| `cubemap.face_size` | a power of two, 4 to 4096 | the largest power of two inside one source cell, at most 4096 | Face edge in pixels. The source cell is a cross or strip cell; a quarter of the width for `equirect`; half the width for `mirror_ball` and `angular`; half the photo's width for `photo`. |
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
| `cubemap.highlight_clamp` | 1 or more | unset | Soft-compresses brightness so that this linear level becomes full white; lower values are also pulled down a little, and hue is kept. For HDR sources. Colour filters run in this order: exposure, highlight clamp, brightness and contrast, midtones, saturation, tint. |
| `cubemap.ground_fill` | `"gradient"` or `[R, G, B]`, each 0 to 255 | unset (no fill) | Replaces everything below `cubemap.horizon`. `"gradient"` starts from the colour at the horizon and darkens it to half at straight down. A colour is read as code values on the game curve (`pow(c, 1.6)`), not sRGB. Unset means no fill, and `horizon` alone does nothing. |
| `cubemap.horizon` | −45 to 45 | 0 | Horizon elevation in degrees: 0 is level, positive is above it. Read only with `ground_fill`. |

The cube is built in this order: import, face rotate and flip, yaw, mirror, exposure, clamp and colour filters, ground fill, blur, mips.

A cube is written as BC1, BC3, BC7 or RGBA8 with the UNORM tag; any other `format` or an sRGB tag is `CFG-INVALID`. `mips.levels = "none"` raises `CUBE-NO-MIPS`.

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
