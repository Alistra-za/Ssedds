# Presets and targets

A target says which renderer the textures are for. A preset is a named bundle of settings. Every conversion has exactly one of each.

## Targets

| Target | Id | What it allows |
|---|---|---|
| Community Shaders | `cs` | Everything: TruePBR materials, Advanced skin, `_SRGB` tags on PBR colour maps, BC6H and RGBA16F for HDR data, every set shader. The default. |
| Vanilla | `vanilla` | Bethesda shaders, plus the complex-material `_m` (environment, gloss, metal, height in one map) for a set whose shader is complex material, and complex grass. No `_SRGB`, no BC6H, no PBR. |

Before the first release there was a third target, `cpm`. It is gone: a vanilla set with the complex material shader packs the same `_m`. `cpm` is an error in `--target`, `sdc.toml`, the user config, `--preset cpm-quality` and `base = "cpm-quality"`. A user preset file with `target = "cpm"` is skipped with a `CFG-PRESET-SKIPPED` warning that names `cpm`.

How the target is chosen, highest first:

1. `--target` on the command line.
2. `target` in the project `sdc.toml` (the GUI's Mode menu writes this).
3. `target` in the [user config](Project-Config.md).
4. The selected preset's own `target`.
5. `cs`.

### Sub-types per target

Under `cs` every sub-type is available. Under `vanilla`, some are not:

| Sub-type | `vanilla` | Fallback with `--allow CFG-UNAVAILABLE` |
|---|---|---|
| `pbr_albedo` | no | `diffuse` |
| `pbr_emissive` | no | `glow` |
| `pbr_normal` | no | `normal` |
| `pbr_subsurface`, `pbr_fuzz`, `coat_normal`, `rmaos`, `skin_rfaos` | no | none |
| `skin_albedo`, `skin_sk`, `skin_wet` | no | none |
| `flow`, `hdr_sky` | no | none |

A file whose sub-type is unavailable is blocked with `CFG-UNAVAILABLE`. When a fallback exists, `--allow CFG-UNAVAILABLE` (or the GUI's acknowledge button) converts it as the fallback. When none exists, change the sub-type or the target.

`complex_grass` is accepted under both targets, and a GRAS diffuse is tagged automatically under both. Only Community Shaders or ENB read it in game.

### sRGB tagging

The DDS format's `_SRGB` flag tells the game to decode colours from sRGB. Getting it wrong makes textures look too dark or washed out.

| Rule | Code if broken |
|---|---|
| Under `cs`, PBR colour maps (`pbr_albedo`, `pbr_emissive` as BC7, `pbr_subsurface`, `pbr_fuzz`) are tagged `_SRGB`. | `TAG-UNORM-PBR` (blocking) |
| Under `cs`, every other sub-type is UNORM. This includes ordinary `diffuse` and `skin_albedo`. | `TAG-SRGB-NONPBR` (blocking) |
| Under `vanilla`, nothing is tagged `_SRGB`. | `TAG-SRGB-VANILLA` (error) |
| Anything under a `lod` folder is UNORM; LOD shaders reject `_SRGB`. | `TAG-SRGB-LOD` (blocking) |
| BC6H and RGBA16F have no tag (`n/a`). | |

### HDR formats

BC6H (`bc6h_uf16`) and RGBA16F exist only under `cs`. A `pbr_emissive` becomes BC6H only when its source is a float image (EXR or HDR) that peaks above 1.0; otherwise it is BC7 with the `_SRGB` tag. The info code `FMT-HDR-SOURCE` says which and why. `hdr_sky` is BC6H.

### Packed maps per target

Raw maps (`_roughness`, `_metallic`, `_ao` and so on) are packed differently per target:

| Target | Packed output | Channels |
|---|---|---|
| `cs` | `_rmaos` | R roughness (or 1 minus glossiness), G metallic, B AO, A specular |
| `cs` | `_rfaos` (Advanced skin, when a fuzz source exists or the set's shader is Advanced Skin) | R roughness, G fuzz, B AO, A specular |
| `cs` | `_p` | height |
| `vanilla`, set shader complex material | `_m` | R environment, G gloss, B metallic, A height; built from any roughness, glossiness, metallic, height or `_orm` source |
| `vanilla`, any other set | `_m` (BC4) | environment mask ([when it is built](Texture-Packing.md#vanilla-any-other-set)) |

Under vanilla, pick **Complex Material** as the set's shader (GUI) or pass `--set set:<id>.shader=complex_material` (CLI) to get the complex `_m` from raw maps. A set that already has a complex-material `_m` file picks the shader by itself, and that file is kept unless `packing.prefer = "raw"`.

See [Texture types](Texture-Packing.md#packing-raw-maps) for the details.

### Set shaders per target

| Shader | `cs` | `vanilla` |
|---|---|---|
| `pbr`, `pbr_grass`, `pbr_landscape`, `advanced_skin` | yes | no |
| `default`, `envmap`, `complex_material`, `glow`, `parallax`, `skin`, `hair`, `eye`, `multilayer`, `landscape`, `complex_grass` | yes | yes |

A shader the target does not have falls back to `default`, with `SHADER-UNAVAILABLE`. See [Texture types](Set-Shaders.md#set-shaders).

## Built-in presets

| Id | Version | Target | Description |
|---|---|---|---|
| `cs-quality` | 1.0.0 | `cs` | The default. Community Shaders including TruePBR; bc7e at `veryslow`; BC6H for HDR emissive. Adds nothing to the base defaults. |
| `cs-compact` | 1.0.0 | `cs` | Smaller files. BC1 for opaque colour maps at full size; every other listed sub-type at half size when its larger side is above 512; BC4/BC5 search radius `fast`. |
| `vanilla-quality` | 1.0.0 | `vanilla` | Bethesda shaders. No `_SRGB`, no BC6H. |
| `vanilla-compact` | 1.0.0 | `vanilla` | The vanilla version of `cs-compact`. |
| `fast-preview` | 1.0.0 | none (uses the batch target) | bc7e `fast`, BC4/BC5 search radius `fast`, PSNR validation on the smallest mip only. For checking a mod quickly, not for release. |

All of them inherit from `sdc-base` (version 1.1.0), a hidden preset that holds the per-sub-type defaults. It is not listed by `presets list`, and a user preset cannot use its id.

How the preset is chosen, highest first:

1. `--preset` on the command line.
2. `preset` in the project `sdc.toml`.
3. `preset` in the user config.
4. The default for the target: `cs-quality` or `vanilla-quality`.

In the GUI, the header's **Preset** menu picks the preset (it writes `preset = "<id>"` to `sdc.toml`; it lists the presets of the current Mode), and **Presets…** opens the preset creator, which makes and edits user presets without writing TOML by hand (see the GUI guide). The Advanced drawer's Preset inspector shows the batch preset and lists the user presets that loaded.

### What the compact presets do

`cs-compact` and `vanilla-compact` differ from their quality presets in three ways:

1. Colour maps that are opaque become BC1 at full size. This applies to `diffuse`, `pbr_albedo`, `glow`, `pbr_subsurface`, `pbr_fuzz`, `subsurface_tint`, `backlight_tint`, `ui_sky` and `skin_albedo` when the source has no alpha channel (or scanned meshes decided to remove the alpha), and to the same list minus `pbr_subsurface` and `pbr_fuzz` when the alpha is constant 255. `landscape_diffuse` is never BC1 (its alpha is the snow mask), and `pbr_emissive` stays BC6H or BC7.
2. Every other listed sub-type gets `resize = "half"`: both sides halve when the larger side is above 512. Emissive and HDR sky are not resized.
3. `encoder.bc45_search_radius = "fast"`.

BC3 is never used for compact output, because it is the same size as BC7.

Compare any two presets field by field:

```powershell
sdc presets diff cs-quality cs-compact --subtype diffuse
```

### Built-in rules

`sdc-base` contains two rules that every preset inherits: any file under an `interface` folder or a `sky` folder gets `mips.levels = "none"`.

## Writing your own preset

A preset is a TOML file. Put it in `%APPDATA%\sdc\presets\` and both `sdc` and the GUI load it on every run. Or load it for one run with `--preset-file <file>` (load only) or `--preset <file.toml>` (load and select).

```toml
schema_version = 1
id = "my-armor"
version = "1.0.0"
base = "cs-quality"
target = "cs"
description = "Armour textures: slowest bc7e, cutout diffuse with coverage, Kaiser mips on colour."

[defaults]
encoder = { effort = "slowest" }

[subtypes.diffuse]
alpha = { policy = "cutout", threshold = 128 }
mips = { coverage = "on", filter = "kaiser" }

[subtypes.pbr_albedo]
mips = { filter = "kaiser" }

[[rules]]
match = { glob = "**/cloth/**", subtypes = ["diffuse"] }
overrides = { alpha = { policy = "blend" } }

[suffix_aliases]
rmaosultrahigh = "rmaos"
```

| Key | Required | Meaning |
|---|---|---|
| `schema_version` | yes | 1. |
| `id` | yes | The preset's id. Must not clash with an id that is already loaded, built-in or user. |
| `version` | yes | Your version string. |
| `base` | in practice | The preset this one inherits from, such as `cs-quality` or `sdc-base`. Every sub-type's profile must resolve completely, so a preset without a `base` would have to supply every field itself; `sdc presets validate` reports what is missing. A cycle is an error (`preset inheritance cycle at '<id>'`). |
| `target` | no | The preset's target, used when nothing else sets one. Leave it out for a target-neutral preset like `fast-preview`. |
| `hidden` | no | `true` keeps the preset out of `presets list`. |
| `description` | no | Shown by `presets list`. |
| `[defaults]` | no | Partial profile for every sub-type. |
| `[subtypes.<id>]` | no | Partial profile for one sub-type. |
| `[[rules]]` | no | Same format as in `sdc.toml`. See [Project config](Project-Config.md#rules). |
| `[suffix_aliases]` | no | Extra file-name suffixes, as `alias = "canonical suffix"`. |

The `[defaults]`, `[subtypes]` and `[[rules]]` tables take the same fields as `sdc.toml`; see the [field reference](Project-Config.md#field-reference).

Inheritance and rule order follow [How settings layer](Project-Config.md#how-settings-layer).

### Suffix aliases

A suffix alias makes Tessalith read another suffix as a canonical one. With `rmaosultrahigh = "rmaos"`, a file named `iron_rmaosultrahigh.png` joins the `iron` set as its `_rmaos`.

- The alias must be lower-case ASCII letters and digits, with no underscore.
- It must not be a canonical suffix (such as `n` or `rmaos`) or an exporter vocabulary term (such as `albedo` or `roughness`).
- It must map to a canonical suffix.
- Two loaded presets cannot declare the same alias.
- A file named with an alias is resolved with the declaring preset's settings chain, so the alias's preset can carry settings for those files.

A preset that mainly declares aliases, loaded with `--preset-file` or from the user presets folder:

```toml
schema_version = 1
id = "my-aliases"
version = "1.0.0"
base = "cs-quality"

[suffix_aliases]
rmaosultrahigh = "rmaos"
normalmap = "n"
```

`sdc presets list` shows each preset's aliases.

### Checking and installing

```powershell
# Check a preset file
sdc presets validate my-armor.toml

# See what it resolves to for one sub-type
sdc presets --preset-file my-armor.toml show my-armor --subtype diffuse

# Start from a built-in preset
sdc presets export cs-compact --id my-compact --out my-compact.toml
```

A file in the user presets folder that fails to load is skipped with `CFG-PRESET-SKIPPED` (a warning naming the file and the reason), and the run continues. A file given with `--preset-file` that fails to load is an error.

### Preset versions and re-encoding

Each output's manifest record stores a hash of the resolved settings. If you change a preset, any texture whose resolved settings changed is re-encoded on the next `if-changed` run; textures whose settings did not change are left alone. See [Validation and output](Validation-and-Output.md#re-runs-and-if-changed).
