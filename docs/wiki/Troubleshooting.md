# Troubleshooting

Not listed here? See [Known issues](Known-Issues.md) for open problems in this version.

## Common problems

### Every file is "unresolved" on the command line

```text
skip  C:/src/rock.png: SUG-LOW-CONFIDENCE sub-type 'diffuse' is a High-confidence suggestion (colour member, non-PBR set)
```

`sdc` does not apply suggestions by default. Add `--accept-suggestions high`, or record the sub-types in `sdc.toml`. The GUI accepts High-confidence suggestions automatically. See [CLI reference](CLI-Reference.md#why---accept-suggestions-high-matters).

### A colour map asks what its alpha means

A diffuse (or albedo, UI or skin colour) whose alpha varies cannot be decided from the image alone: the mesh decides whether it is a cutout, a blend or unused. Set `alpha.policy` on the file, answer the question in the GUI, or scan your mods so the GUI can read the meshes ([Mod scanning](Mod-Scanning.md)). Alpha whose minimum is 251 or more is treated as opaque and does not ask.

### A normal map reports IN-GREEN-UNSTATED

The file is named `_normal`, `_nrm` or `_norm`, which does not say whether its green channel is DirectX (Skyrim's convention) or OpenGL. It is converted as DirectX and this note says so. If it is really OpenGL, set it:

```toml
[files."textures/armor/iron/iron_normal.png"]
channels.green_convention = "opengl"   # or "directx"
```

Or rename the file `_normalgl`, `_normaldx` or `_n` (`_n` means DirectX). Nothing is blocked, so there is nothing to allow.

### A texture is black under torchlight

Look for `CHK-GREEN-GUESS` on its normal map (the ⓘ badge on the set card, or the texture's settings panel). A normal map whose green channel points the wrong way lights from the wrong side, and a face that should catch the torch looks black. The check reads the map's slopes and says it "looks OpenGL"; it is a hint, not a fix, and it is weak on straight ridges. If it is right, press **Treat as OpenGL (flip green)**, or set `channels.green_convention = "opengl"` on the file. A `CHK-Z-SIGN` row on the same map means its blue channel points the wrong way too.

### A surface looks too shiny or plastic

Look for `CHK-PLASTIC` on its `_n`. On a vanilla `_n` the alpha is the specular: with no alpha, or an alpha that is almost all white, the whole surface gets full specular. Add an alpha that varies, or ignore the warning when the mesh sets its own specular. It never applies to an `_msn`, and on hair and Advanced Skin the message says Community Shaders does not read that alpha. Also check `CHK-RANGE` on an `_rmaos` (extreme roughness) or on an albedo that is too bright; the preview's **Range** toggle shows which texels break the rule.

### Dark seams at a distance

Look for `CHK-UV-GUTTER`. UV islands that sit closer than 8 texels let a smaller mip blend one island's colour into its neighbour, which shows as dark or bright seams from far away. Move the islands apart in the mesh, or ignore it for a tightly packed atlas that looks fine in game. The warning needs a Meshes scan. **Check all UV coverage** in Advanced reads every colour texture of the folder; a texture that no scanned mesh uses raises nothing.

### UV coverage is greyed out

The overlay and the check read your Meshes scan, so scan first: scan your MO2 instance in the Meshes view. The toggle's hover gives the reason: a scan that is running or was stopped (let it finish, or scan again), no scan database, or a scan with no rows (an empty scan counts as none). Only colour textures (diffuse, PBR albedo, skin albedo) have UV coverage, and landscape diffuses are left out. The toggle is also off while a pin is compared ("Hidden while comparing") and after a failed read (its hover lists the errors), and **Check all UV coverage** stays off until the **UV coverage** check is turned on ("Turn on UV coverage check first").

### A texture was held for half size

With **Drop mip 0 when it adds nothing** on, Export holds back a texture whose top mip carries little detail (`CHK-LOW-DETAIL`) and asks **Export at half size?** at the end. Tick the ones to write at half size, or choose **Export all at full size**. An unticked texture is asked about again on the next export. If the box could not open (another dialog was open), the texture was written at full size and a note says so.

### A normal map came out as a diffuse

Its name has no normal suffix (for example `rock_nm.png`), so it was read as the colour map of a set called `rock_nm`. Rename it, add an `[inputs]` alias, or set `subtype = "normal"` in a `[files]` entry.

### A texture is blocked with DIM-NPOT

Its sides are not powers of two. Resize the source, set `resize = "pot-down"` on the file (the GUI's **Resize to a power of two** button does this), or acknowledge it with `allow = ["DIM-NPOT"]` or `--allow DIM-NPOT`.

### DDS files in the folder are skipped

DDS inputs are skipped by default (`IN-DDS-SKIPPED`) because re-encoding a compressed DDS loses quality. Use the source image instead, or set `reencode = true` on the file. To fix only a DDS header, use `sdc patch-header`.

### Existing outputs are not replaced (OUT-UNKNOWN-EXISTING)

The output folder holds a DDS that the manifest has no record of, for example from another tool or an older copy. `if-changed` never overwrites such a file. Delete it, use a fresh output folder, or run once with `--on-existing overwrite`.

### A packed channel uses a default value (PACK-DEFAULTED)

The set has no source for that channel, for example no specular map for `_rmaos`. Add the map, or set a fill: `--fill specular=128`, or `packing.fill.specular = 128` on the packed output's `[files]` key.

### The PBR set is not packed, or its raw maps are reported as unused

Raw maps are packed only under the target that reads them. Under `vanilla`, only metallic or specular builds an `_m`; roughness, AO and the rest are `PACK-UNUSED-INPUT`. An opacity map is not packed: it fills the colour texture's alpha (next entry). Check the target.

### My opacity map is not used

An `_opacity` file (or a mask you gave the role Opacity) fills the alpha of its set's colour texture. Its GUI row, in the Helpers group under the tiles, says "→ <texture> alpha" when it is used and "not used" with the reason when it is not; on the command line the reason is the `PACK-UNUSED-INPUT` message. The reasons, and what to do:

| Reason | Why | Fix |
|---|---|---|
| `x.png already has alpha` | A suggested helper only fills an empty alpha (none, fully opaque, or flat). Your colour PNG has a real alpha. | Assign the helper: in its panel pick the Opacity role again (it then reads *set by you*) or add the target, or set `helper.role = "opacity"` on the file. An assigned helper replaces the alpha. |
| `512x512, target 1024x1024: sizes differ` | The helper and the colour texture are not the same size. | Resize one of them. An assigned helper of another size fails the texture with `PACK-SIZE-MISMATCH` instead. |
| `no colour texture planned` | The set has no plain colour texture to fill (only a PBR or skin output that is not a target, or none at all), or the file did not join the set. | Check the set in the Sets column; give the file the same base name as the colour texture. |
| `diffuse.a is filled by <other file>` | Another helper fills that alpha. An assigned one beats a suggested one; otherwise the first by path wins. | Remove the target from the other helper, or assign this one. |
| `no targets chosen` | Its `helper.fills` is `[]`. | Reset Fills in the panel, or remove the key. |
| `<file> was not analysed` | The helper or the colour texture failed to load. | Fix the load error shown for that file. |

If the file is not listed as a helper at all, its name has no term Tessalith knows: select it and set its role, or use `--set "file:**/<name>.helper.role=opacity"`. To convert an opacity map as its own texture instead, set its role to *Not a helper* (`helper.role = "none"`). See [Texture types](Texture-Types.md#helper-textures).

### sRGB or UNORM is blocked

| Code | Fix |
|---|---|
| `TAG-UNORM-PBR` | A PBR colour map under `cs` must be `_SRGB`. Remove the `color.tag = "unorm"` override. |
| `TAG-SRGB-NONPBR` | Under `cs`, only PBR colour maps take `_SRGB`. Remove the `color.tag = "srgb"` override, or set the sub-type to the PBR one. |
| `TAG-SRGB-VANILLA` | `vanilla` has no `_SRGB`. |
| `TAG-SRGB-LOD` | Files under a `lod` folder must be UNORM. |

### The GUI is read-only

`sdc.toml` failed to load or was rejected. The banner names the problem. Fix the file in a text editor and press **Reload**. If `sdc-project.toml` failed to load, only sets and the JSON are read-only.

### The GUI does not start

It needs the Windows system font Segoe UI (`segoeui.ttf`) and says so if it is missing. Segoe UI Symbol, Segoe UI Emoji and Consolas are optional and skipped without a message. The 2D preview and the 3D view need a GPU; without one, the preview is off and everything else works.

### The 3D preview shows plain textures

The PGPatcher simulation needs the `cs` target and a PBR set, and a scanned mesh that uses the diffuse. Built-in shapes cannot match an entry's match keys. Meshes from Skyrim LE are not drawn. If "Community Shaders not found" shows, use **Shader folder…** to point at your Community Shaders mod folder.

### NVTT problems

See [NVTT backend](NVTT-Backend.md#codes).

### Leftover .sdc-*.tmp files

A hard abort (a second Ctrl-C, a crash) can leave temporary files in the output folders. They are safe to delete. Leftover `%TEMP%\sdc-nvtt-*` folders are also safe to delete.

## Error messages

| Message | Cause |
|---|---|
| `input not found: <path>` | An input path does not exist. |
| `<path> is a folder; give .dds files` | `validate` and `patch-header` take files, not folders. |
| `--mem-budget '<s>': must be greater than 0 (omit the flag for the default, 50% of RAM)` | `--mem-budget 0`. |
| `--mem-budget '<s>': use bytes or a G/M suffix` | A size ending in something other than a digit, `G` or `M`, such as `8Q`. |
| `--mem-budget '<s>': not a valid size` | A number that is not a whole number, such as `1.5G`. |
| `--fill '<s>': unknown role '<role>' (one of …)` | A misspelt fill role. |
| `--fill '<s>': the value must be a whole number 0..255` | A fill outside 0 to 255. |
| `--allow <X>: unknown code` | A misspelt code. |
| `--set '<s>': unknown field '<f>'` | A misspelt field name. |
| `--set '<s>': unknown sub-type '<id>'` | A misspelt sub-type. |
| `--set '<s>': a sub-type scope cannot set the sub-type` | `subtype:<id>.subtype=…`. |
| `--set '<s>': reencode takes true or false` | |
| `--set '<s>': a set scope sets only `shader`` | `set:<id>.<anything but shader>`. |
| `--subtype <id> is unavailable under <Target> (CFG-UNAVAILABLE)` | `validate` or `patch-header` with a sub-type the target lacks. |
| `unknown preset '<id>'` | A preset id that is not loaded. |
| `preset inheritance cycle at '<id>'` | Presets whose `base` chain loops. |
| `<file>: config schema_version N is newer than this sdc supports (1)` | A config written by a newer version. A preset gives `preset schema_version N is newer …`. |
| `[files] is only valid in a project sdc.toml, not in <path>` | `[files]` or `[inputs]` in the user config. |
| `schema all needs --out <dir>` | |
| `source changed since planning; re-run` | A source file changed between planning and encoding. Run again. |
| `sdc: internal error` | A crash inside `sdc` (exit 3). Please report it with the input that caused it. |

Scan errors are listed on [Mod scanning](Mod-Scanning.md#error-messages).

## Diagnostic codes

Every warning has a code and a severity:

| Severity | Effect |
|---|---|
| info | Recorded. Not shown in the CLI log (except `CFG-FILTER-IGNORED`). |
| warning | Shown; the file is converted. |
| blocking | The file is not converted until you acknowledge the code: `--allow <CODE>` for the batch, `allow = ["<CODE>"]` in the file's `[files]` entry, or the acknowledge button in the GUI. |
| error | The file fails. |

### Input

| Code | Severity | Meaning | Fix |
|---|---|---|---|
| `IN-DUPLICATE` | blocking | Two inputs would write one output, or two outputs land on one path (for example two packed outputs after a raw map moved to another set). The first keeps the path. | Exclude or rename one, or change an output path. |
| `IN-DDS-SKIPPED` | info | A DDS input is not re-encoded by default. | Use the source, or set `reencode = true`. |
| `IN-PACKING-INPUT` | warning | A raw map is not packed because its recipe is off (`packing.enabled = false`). | Enable packing, or give the file a sub-type. |
| `IN-GREEN-UNSTATED` | info | A plain `_normal`/`_nrm`/`_norm` does not state its green convention; it was converted as DirectX. | Set `channels.green_convention = "opengl"` if it is an OpenGL map. |
| `IN-DECODE` | error | The source cannot be read. | Fix or re-save the file. |
| `IN-NONFINITE` | warning | NaN or infinite samples were replaced. | Fix the source. |
| `IN-UNREADABLE` | warning | A file or folder under an input folder could not be read (access denied, broken junction, cloud placeholder) and was skipped. | Fix permissions, or ignore. |
| `IN-ICC-IGNORED` | info | An embedded ICC profile was ignored; pixels are read as stored. | Convert the source to sRGB or linear first if the profile matters. |
| `IN-ALIAS-INVALID` | warning | An `[inputs]` alias breaks the rules and was ignored. | Fix the `as` path. |
| `IN-ALIAS-STALE` | info | An `[inputs]` key matches no file. | Remove the entry. |
| `IN-FLOAT-LDR` | info | Retired; never emitted. Still accepted by `--allow`. | |

### Loss

| Code | Severity | Meaning | Fix |
|---|---|---|---|
| `LOSS-ALPHA-DATA` | blocking | Varying alpha would be discarded by `remove` or a format without alpha. Not raised for a `pbr_normal` or an explicit `alpha.policy = "remove"`. | Keep the alpha, choose `remove` explicitly, or acknowledge. |
| `LOSS-CLAMP-HDR` | warning | Float values outside 0 to 1 written to an LDR format are clamped. | Use an HDR format under `cs`, or accept. |
| `LOSS-BITDEPTH` | info | A 16-bit or float source goes to an 8-bit-endpoint format. | |
| `LOSS-REENCODE` | warning | A compressed DDS is being re-encoded. | Use the source image. |

### Channels

| Code | Severity | Meaning | Fix |
|---|---|---|---|
| `CHAN-NOT-GRAY` | warning | A grayscale sub-type's R, G and B differ by more than 4/255; only R (or luma) is kept. | Pick the channel or the sub-type. |
| `CHAN-CONSTANT-ALPHA-DATA` | info | `data` alpha is constant; `remove` would save bits. | |
| `CHAN-NORMAL-LENGTH` | warning | A normal source is far from unit length, or has no Z. | Use **Rebuild Z** on its Problems row (sets `channels.reconstruct_z`), or check the sub-type. |
| `CHAN-NORMAL-SHORT` | info | The normal source's mean length at mip 0 is below 0.9. Mip 0 is renormalised, or kept with `mips.normal_base = "keep"`. | |
| `CHAN-AO-INVERTED` | warning | The `_rmaos` B channel is mostly black; AO may be inverted. | Check the source. |
| `CHAN-GLOSS-AS-ROUGH` | warning | The `_rmaos` R channel is mostly high; it may be gloss, not roughness. | Invert it or confirm. |

### Format and tag

| Code | Severity | Meaning | Fix |
|---|---|---|---|
| `FMT-BC5-NORMAL` | blocking | BC4, BC5, R8 or RG8 on a normal map: Z decodes as -1 and alpha is lost. | Use BC7. |
| `FMT-LANDSCAPE-BC1` | blocking | BC1 on `landscape_diffuse` destroys the snow mask. | Use BC7 or BC3. |
| `FMT-BC1-ALPHA` | blocking | BC1 with an alpha policy other than `remove`; BC1 output is opaque only. | Use BC7 or BC3, or `remove`. |
| `FMT-PERCEPTUAL-DATA` | error | Perceptual encoding on a data map. | Turn `encoder.perceptual` off. |
| `FMT-HDR-SOURCE` | info | Why a `pbr_emissive` is BC6H (float source above 1.0) or BC7 sRGB. | Set `format` on the file to choose. |
| `TAG-SRGB-VANILLA` | error | `_SRGB` under `vanilla`. | UNORM. |
| `TAG-SRGB-NONPBR` | blocking | `_SRGB` on a non-PBR sub-type under `cs`; renders darker. | UNORM. |
| `TAG-UNORM-PBR` | blocking | UNORM on PBR colour; looks washed out with linear lighting off. | `_SRGB`. |
| `TAG-SRGB-LOD` | blocking | `_SRGB` under a `lod` folder; LOD shaders reject it. | UNORM. |
| `TAG-CONTENT-MISMATCH` | warning | Content and tag disagree: linear content tagged `_SRGB`, a colour sub-type with linear content, or a roughness member with sRGB content (then not variance-baked). | Check `color.content` and `color.tag`. |

### Alpha

| Code | Severity | Meaning | Fix |
|---|---|---|---|
| `ALPHA-EXACT-OFF-M` | blocking | `exact_opaque` turned off on a BC7 `_m`; decoded 254s may trip PGPatcher's 50% rule or ENB checks. | Keep it on, or acknowledge. |
| `ALPHA-254` | info | Opaque alpha decodes as 254 in places. Harmless to Community Shaders' 251/255 gate. | Turn `alpha.exact_opaque` on if the alpha is read. |
| `ALPHA-NEAR-OPAQUE` | info | Source alpha 251 to 254 with a minimum of 251 or more is treated as opaque and removed. | |
| `ALPHA-PBR-NORMAL-DROPPED` | info | Vanilla specular in a `pbr_normal`'s alpha was dropped; PBR normals do not use it. | |

### Configuration

| Code | Severity | Meaning | Fix |
|---|---|---|---|
| `CFG-UNAVAILABLE` | blocking | The sub-type or format is not available under the target. | Accept the fallback with `--allow CFG-UNAVAILABLE`, or change the sub-type or target. |
| `CFG-INVALID` | error | A value outside its range, or a combination that cannot be honoured (a backend that cannot write the format, `legacy` header on BC7 or `_SRGB`, a bad swizzle, `bc1_transparent_black` with BC1, a bad filter scope, and so on). In a config, preset, rule or `--set`, the run stops with exit 2; reached after layering, that file fails. | Fix the value. |
| `CFG-FILE-UNMATCHED` | info | A `[files]` key matches no file. | Fix or remove the entry. |
| `CFG-FILTER-IGNORED` | info | An unscoped non-box `mips.filter` that this texture cannot use; it uses box. Shown in the CLI log. | Scope the filter to colour maps, or ignore. |
| `CFG-PRESET-SKIPPED` | warning | A file in the user presets folder failed to load and was skipped. | Fix the file (`sdc presets validate`). |
| `CFG-GREEN-CONFLICT` | warning | An explicit `channels.flip_green` contradicts `channels.green_convention`; `flip_green` wins. | Remove one. |

### Dimensions

| Code | Severity | Meaning | Fix |
|---|---|---|---|
| `DIM-NPOT` | blocking | A side is not a power of two (checked after resizing). | `resize = "pot-down"`, or acknowledge. |
| `DIM-SMALL` | blocking | A side is under 4 pixels. A cube face under 4 is also `CFG-INVALID`, which can't be acknowledged. | Resize, or acknowledge (not for a cube). |
| `DIM-FLOW-SMALL` | warning | A `_flow` map is 32 px or smaller; Community Shaders ignores it. | Use 64 px or more. |
| `DIM-RESIZED` | info | The input was resized, with the old and new size. | |
| `DIM-TOO-LARGE` | error | A side is larger than 16384. | Downscale the source. |

### Cubemaps

| Code | Severity | Meaning | Fix |
|---|---|---|---|
| `CUBE-FACES` | error | A cube cannot be built. Six face files with two files for one face, a missing face, a face that is not square, faces of different sizes, or faces with different settings; or a layout that does not fit the image (`hstrip` on a 512×512 file). The message names the fault. Nothing is written for that cube. | Fix the files or the layout. See [Cubemaps](Cubemaps.md#six-images). |
| `CUBE-NO-MIPS` | warning | A cube with `mips.levels` of `none` or 1. It sparkles in game. | Leave `mips.levels` at `full`. |

### Sets and suggestions

| Code | Severity | Meaning | Fix |
|---|---|---|---|
| `SET-MISSING-COMPANION` | info | A PBR set lacks `_n` or a roughness member, an `_msn` lacks `_s`, or a `_flow` lacks `_n`. | Add it, or ignore. |
| `SET-COMPANION-OUTSIDE` | info | The missing companion is on disk but not in the batch, so there is no bake. | Include it. |
| `SET-COMPANION-DDS` | warning | The companion exists only as DDS, so there is no bake. | Supply the source, or opt the DDS in. |
| `SET-BAKE-RATIO` | warning | `_n` and the roughness member are not powers of two with one aspect; bake off. | Resize. |
| `SET-EDIT-STALE` | info | A set edit or `set:` override names a set or file that does not exist. | Fix or remove it. |
| `SUG-LOW-CONFIDENCE` | blocking | A sub-type or alpha meaning is unconfirmed. | Confirm it, or `--accept-suggestions high` for High ones. |
| `SUG-CONTRADICTION` | warning | The content contradicts the suffix. | Confirm or change the sub-type. |

### Validation

| Code | Severity | Meaning | Fix |
|---|---|---|---|
| `VAL-HEADER` | error | The written header fails the header rules. On `sdc validate`, also a cube with no mips, a 1×1 cube with a non-black +Y, a cube in an sRGB format, or a 2D file named `_e` or under `textures\cubemaps\` (warnings that fail the file; see [Validation and output](Validation-and-Output.md#cube-checks)). | Report it as a bug, or fix the cube. |
| `VAL-MIP-ERROR` | warning | A mip's PSNR is below `validation.min_mip_psnr_db`. | Check the source, raise the effort, or accept. |
| `VAL-ALPHA-GUARANTEE` | error | Decoded opaque alpha is below the encoder's guarantee. | Report it as a bug. |
| `VAL-COVERAGE` | warning | Cutout coverage drifted more than 2 points on a mip. | Adjust the threshold. |
| `VAL-M-COMPLEX-MARGIN` | warning | A complex `_m`'s smallest-mip alpha is 247/255 or more: within the margin below Community Shaders' 251/255 gate, or past it (complex off). | Adjust the height range. |
| `VAL-M-GRAYSCALE` | warning | A complex `_m`'s smallest mip is grey, so it reads as a plain env mask. | Check the channels. |
| `VAL-M-HEIGHT-CLIP` | warning | Large `_m` alpha areas at or below 4/255 or at or above 251/255: no parallax. | Remap the height. |
| `VAL-PGPATCHER-50` | warning | A plain `_m` with alpha is 50% or more non-white; PGPatcher would treat it as complex. | Use BC4, or turn `exact_opaque` on. |
| `VAL-BC6H-PEAK` | warning | A BC6H texture's peak ratio is below 0.8. | Try NVTT Production. |
| `VAL-GRASS-DETECT` | error | The encoded complex-grass corner texel still fails detection after a slowest re-encode. The file is written. | Fix the source corner. |

### Encoders and output

| Code | Severity | Meaning | Fix |
|---|---|---|---|
| `ENC-NVTT-MISSING` | blocking | NVTT chosen but not found. | Install it, give the path, or use the built-in encoder. |
| `ENC-NVTT-VERSION` | blocking | NVTT version other than 3.2.5. | Acknowledge. |
| `ENC-NVTT-FAILED` | error | `nvcompress` failed; no fallback. | Check NVTT, or use the built-in encoder. |
| `ENC-WEIGHTS-IGNORED` | warning | `encoder.bc7_weights` has no effect: the format is not BC7, the backend is not bc7e, or `encoder.perceptual` is on. | Remove `encoder.bc7_weights`, or turn perceptual off. |
| `OUT-UNKNOWN-EXISTING` | warning | An existing output with no manifest record was kept. | `--on-existing overwrite`. |
| `OUT-SIDECAR-KEPT` | warning | The `.dds.json` next to the output was not changed, and the new DDS was written. Either an old sidecar sdc wrote for the previous output could not be removed or replaced (locked or protected), or with `--sidecar` the file there is not one sdc wrote, so it was kept. | Unlock or delete the old `.dds.json` and run again (with `--sidecar` to write a new one). A file that is not sdc's is yours: move it away if you want the sidecar. |
| `DDS-HEADER-PATCHED` | info | `patch-header` (or the GUI) rewrote a header; the payload is byte-identical. | |
| `DDS-HEADER-UNPATCHABLE` | warning | The header cannot be fixed without re-encoding (unreadable, cubemap or array, a format Tessalith does not write, missing mips, BC1 with transparent texels, wrong block format). | Re-encode from the source. |

### Packing

| Code | Severity | Meaning | Fix |
|---|---|---|---|
| `PACK-DEFAULTED` | warning | A packed channel has no source and uses the neutral default. | Supply the map, set a fill, or accept. |
| `PACK-SIZE-MISMATCH` | error | Sources of one recipe differ in size. | Resize the sources. |
| `PACK-CONFLICT` | warning | A pre-packed file and raw maps target one output; the pre-packed file wins by default. | Remove one side, or set `packing.prefer`. |
| `PACK-UNUSED-INPUT` | warning | A raw map is used by no recipe under the target. | Change the target, set a sub-type, or remove it. |
| `PACK-RECIPE-CONFLICT` | warning | The `_rfaos` and `_rmaos` keys of one set disagree on `packing.recipe`; the `_rfaos` entry wins. | Keep the recipe on one key. |
| `PACK-RECIPE-UNBUILDABLE` | warning | `packing.recipe = "rfaos"` on a set with only a metallic map; `_rmaos` was built. | Add roughness, glossiness or AO, or drop the setting. |
| `PACK-ORM-OVERRIDDEN` | info | A separate map (such as `_roughness`) is used instead of the matching `_orm` channel, or an `_rfaos` drops the `_orm` metallic. | Nothing, or remove the separate map to use the `_orm` channel. |

### RNM, skin, shaders and grass

| Code | Severity | Meaning | Fix |
|---|---|---|---|
| `RNM-NO-BASE` | warning | A layer or mask has no base normal to feed. | Add the base, or set `rnm.target`. |
| `RNM-IGNORED-EYE` | warning | A layer in an eye set; eye normals are model-space. | Remove or move the layer. |
| `RNM-MASK-ORPHAN` | warning | A mask without its layer. | Add the layer, or remove the mask. |
| `RNM-AO-MISSING` | info | A `bent` layer in `ao_blend` mode has no AO source; the weight is uniform. | Add an AO map, or set `rnm.bent.ao = false`. |
| `RNM-LAYER-SHADOWED` | warning | Two files fill one layer of one base. | Rename or remove one, or change `rnm.target`. |
| `SKIN-WET-FORM` | info | Which form a `skin_wet` file was read as (BC4 height or BC7 normal with mask). | Set `format` to force one. |
| `SHADER-AUTO` | info | The set's shader was picked automatically, with the rule. | |
| `SHADER-UNAVAILABLE` | warning | The shader is not available under the target; it fell back. | Change the shader or target. |
| `SHADER-EXTRA-MAP` | warning | A member the shader does not use. It still converts. | |
| `SHADER-MISSING-MAP` | warning | A map the shader requires is missing. | Add it. |
| `GRASS-SHAPE` | error | A `complex_grass` source is not a power-of-two 1:2 image. | Supply e.g. 2048x4096. |
| `GRASS-TOP-OPAQUE` | warning | The top half is fully opaque: no cutout. | Check the alpha. |
| `GRASS-NORMALS` | warning | Under 99% of the bottom half is unit-length normals. | Check the bottom half is a normal map. |
| `GRASS-DETECT-SOURCE` | warning | The source's bottom-left texel already fails Community Shaders' detection test. | Fix the corner texel. |

### Artist checks

These come from the GUI's artist checks, plus `CHK-LOW-DETAIL`, which convert measures only when the GUI asks. They only warn: they never block an export or change "ready", and each has a switch in Advanced → Checks. The CLI raises none of them, but it honours a saved `drop_top_mip`.

| Code | Severity | Meaning | Fix |
|---|---|---|---|
| `CHK-RANGE` | warning | Albedo too dark or bright, metal that is not black or white, extreme roughness, a dark AO channel, dark metal, or a mostly bright emissive background. | Check the map; the preview's **Range** toggle shows the texels. |
| `CHK-PLASTIC` | warning | A vanilla `_n` with no alpha or an almost all-white alpha: flat, full specular. | Add a varying alpha, or ignore. |
| `CHK-GREEN-GUESS` | info | The normal map's slopes look like OpenGL green. | **Treat as OpenGL (flip green)**, or ignore. |
| `CHK-Z-SIGN` | warning | A normal map's blue channel points the wrong way. | Check the source. |
| `CHK-PARALLAX` | warning | A height map, judged against the set's neutral height (0.5, or 1.0 with a multilayer coat), uses little range, clips, is mostly white or is too steep. | Check the height map. |
| `CHK-PARALLAX-SCALE` | warning | A landscape set's displacement scale is more than 2x from the others. | Make the scales match. |
| `CHK-LOW-DETAIL` | warning | Mip 0 adds little detail over mip 1. | Export at half size, or ignore. |
| `CHK-UV-GUTTER` | warning | UV islands closer than 8 texels for more than 2 % of used texels. | Space the islands out. |

### Generated maps

These concern the `*_generated.png` files the GUI's **Generate from normal map…** writes.

| Code | Severity | Meaning | Fix |
|---|---|---|---|
| `GEN-REPLACED` | info | A generated file was skipped because a real map for the same slot is in the set. It counts as skipped on purpose. | Nothing; the real map wins. Delete the generated file if you no longer want it. |
| `GEN-STALE` | info | GUI only: the generated file's normal, height neutral or shader changed since it was made. | **Regenerate…**, or ignore. |
| `GEN-AO` | info | A packed `_rmaos` takes its blue channel from a generated AO (in place of `PACK-DEFAULTED`). | Nothing; a baked AO is better if you have one. |

## In-game checklist

Tessalith's validation checks the files, not the game. After installing converted textures, check them in game. These checks follow the in-game tests behind Tessalith's defaults, and each notes the tested result where one set a current default.

1. **Textures load.** Look at each converted object. If one is missing, compare the output path with the path the mesh uses (the GUI's Meshes view shows a mesh's texture slots).
2. **Brightness of PBR colour maps.** Under Community Shaders, compare PBR albedo, emissive (`_g`) and fuzz (`_f`) with linear lighting on and off. They are written `_SRGB` (or BC6H for HDR emissive), which was the tested result; a washed-out look points to a UNORM override (`TAG-UNORM-PBR`).
3. **Ordinary diffuse brightness.** A non-PBR diffuse that looks too dark under Community Shaders points to an `_SRGB` override (`TAG-SRGB-NONPBR`).
4. **Small textures.** Textures from 16 to 128 px with full mip chains loaded without crashes in testing. If a small texture crashes the game, report it.
5. **Complex `_m` maps (CS, or vanilla with the complex material shader).** Community Shaders turns complex material off when the smallest mip's alpha is 251/255 or more. Check parallax and complex shading at a distance. `VAL-M-COMPLEX-MARGIN` warns from 247/255.
6. **Plain `_m` maps and PGPatcher.** A plain `_m` stored with alpha whose decoded alpha is 50% or more non-white is treated as complex by PGPatcher. Tessalith keeps `_m` alpha exactly opaque by default (`exact_opaque`) and warns with `VAL-PGPATCHER-50`.
7. **Cutouts at a distance.** Foliage and fences with `cutout` keep their coverage in smaller mips. If they thin out or thicken, check the `alpha.threshold` against the mesh and look for `VAL-COVERAGE`.
8. **TruePBR sets.** Run PGPatcher after installing the JSON, then check that the PBR material shows on the intended meshes and not on others (watch for delete sections that hit your own sets).

To compare two variants of the same texture in game, convert them into two output folders and overlay one mod at a time:

```powershell
# Variant A: the defaults
sdc convert src -o out\a --recursive --target cs --accept-suggestions high

# Variant B: emissive as BC7 sRGB instead of BC6H (matters for float emissive sources)
sdc convert src -o out\b --recursive --target cs --accept-suggestions high --set "file:**/*_g.png.format=bc7" --set "file:**/*_g.png.color.tag=srgb"
```

Use `--dry-run` first to see what a command would do without writing files.
