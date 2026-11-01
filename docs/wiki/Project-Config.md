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
| `preset` | string | the target's default | The preset to use, by id. A built-in id or a user preset's. See [Presets and targets](Presets-and-Targets.md). |
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
| `helper.role` | For an image: what it is as a [helper texture](Texture-Packing.md#helper-textures). `"none"` (not a helper: convert it on its own); `"roughness"`, `"metallic"`, `"ao"`, `"specular"`, `"glossiness"`, `"height"`, `"opacity"`, `"fuzz"` (skin fuzz mask), `"orm"`, `"curves"`, `"cavity"`, `"fuzzweight"` (PBR `_f` weight), `"subsurfaceweight"` (PBR `_s` weight); or an RNM layer: `"bump"`, `"bump2"`, `"bumpmask"`, `"bumpmask2"`, `"detailnorm"`, `"detailnorm2"`, `"detailnormmask"`, `"detailnormmask2"`, `"bent"`, `"bentmask"`. Overrides the name and any `subtype`. Ignored on a DDS. |
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

Setting `helper.role` or `helper.fills` makes the helper *assigned*: it replaces what its channels already hold. Without them, a helper fills only empty channels. See [Texture types](Texture-Packing.md#helper-textures).

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
| `keep_metal_brightness` | `false` | | The CM folder gets its own diffuses without the metal darkening ([CM folder](Set-Shaders.md#also-export-as-vanilla-and-cm)). |
| `keep_metal_strength` | 1.0 | 0–1 | How much of the metal darkening Keep metal brightness undoes. |

What each folder's files hold is in [Also export as Vanilla and CM](Set-Shaders.md#also-export-as-vanilla-and-cm). The vanilla env `_m` needs a Meshes scan, so the CLI writes none.

The GUI keeps the folders' location in `sdc-project.toml` as `[project] also_location` (by default the export folder); the CLI writes them under `-o`.

## Field reference

Every field, its values, default and meaning: [Project config fields](Project-Config-Fields.md).

## Checking a config

```powershell
# Any error in sdc.toml stops the plan with exit 2 and names the problem
sdc inspect C:\Work\IronPlate --recursive --accept-suggestions high > $null

# The JSON Schema of the config file
sdc presets schema config
```

The repository also has the schema at `schema/config.schema.json`, which editors that understand JSON Schema for TOML can use for completion.
