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

The overlay and the check read your Meshes scan, so scan first: scan your MO2 instance in the Meshes view. The toggle's hover gives the reason: a scan that is running ("Wait for the Meshes scan to finish") or was stopped ("The Meshes scan was stopped: scan again to check UV coverage"), no scan database, or a scan with no rows (an empty scan counts as none). Only colour textures (diffuse, PBR albedo, skin albedo) have UV coverage, and landscape diffuses are left out. The toggle is also off while a pin is compared ("Hidden while comparing") and after a failed read (its hover lists the errors), and **Check all UV coverage** stays off until the **UV coverage** check is turned on ("Turn on UV coverage check first").

### Review asks "half or full size"

With **Measure whether full size adds anything** on, every export measures each texture's top mip and writes the full texture. A texture whose top mip carries little detail (`CHK-LOW-DETAIL`) is then asked about in Review before the next export: pick **Half size** or **Full size** on its row. Export waits until each row is answered. Half size is saved in `sdc.toml` when you press Export; Full size saves nothing, so the texture is asked about again on the next export. Close writes nothing.

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

If the file is not listed as a helper at all, its name has no term Tessalith knows, so it is an ordinary tile: select the tile and pick *Opacity* in **Helper role** in its settings (or write `helper.role = "opacity"` on its `[files]` entry in `sdc.toml`, or use `--set "file:**/<name>.helper.role=opacity"`). It then becomes a row in the Helpers group. To convert an opacity map as its own texture instead, set its role to *Not a helper* (`helper.role = "none"`). See [Texture types](Texture-Packing.md#helper-textures).

### sRGB or UNORM is blocked

| Code | Fix |
|---|---|
| `TAG-UNORM-PBR` | A PBR colour map under `cs` must be `_SRGB`. Remove the `color.tag = "unorm"` override. |
| `TAG-SRGB-NONPBR` | Under `cs`, only PBR colour maps take `_SRGB`. Remove the `color.tag = "srgb"` override, or set the sub-type to the PBR one. |
| `TAG-SRGB-VANILLA` | `vanilla` has no `_SRGB`. |
| `TAG-SRGB-LOD` | Files under a `lod` folder must be UNORM. |

### The GUI is read-only

`sdc.toml` failed to load or was rejected. The banner names the problem. Fix the file in a text editor and press **Reload**. If `sdc-project.toml` failed to load, only sets and the JSON are read-only; its red Problems row names the line and has **Open in editor** and **Reload**. A save refused because a file has Windows' read-only flag leaves a Problems row with **Clear read-only flag**.

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

What each code means and what to do about it: [Diagnostic code reference](Diagnostic-Codes.md).

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
