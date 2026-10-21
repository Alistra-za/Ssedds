# Texture types

Tessalith decides how to encode each file from its sub-type: what the texture is for. The sub-type sets the format, the colour tag, the alpha handling and the mip settings. This page covers how files are named, grouped and typed, and what each type becomes.

## Input formats

| Extension | Notes |
|---|---|
| `.png`, `.tga`, `.tif`, `.tiff`, `.bmp`, `.jpg`, `.jpeg`, `.webp` | Read as stored. |
| `.exr`, `.hdr` | Float sources. A TruePBR emissive needs one (peaking above 1.0) to become BC6H. `hdr_sky` is BC6H whatever the source. An `.exr` with alpha is read as premultiplied; set `alpha.exr_straight = true` for one saved with straight alpha. |
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
| `_e` | no suggestion; set the sub-type yourself | |

A set is a PBR set (under `cs`) when its path has a `pbr` folder, or it has an `_rmaos`, `_rfaos`, `_cnr` or `_f` member, or it has a raw roughness, glossiness, metallic or AO map. A set is an Advanced skin set when it has or builds an `_rfaos`.

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
| `_opacity` | Raw opacity | not packed by any recipe |
| `_fuzz`, `_sheen` | Raw fuzz | packed |
| `_curves`, `_curvature` | Raw curvature (0.5 is flat) | read only by Also export as (Vanilla / Complex material) |
| `_cavity` | Raw cavity | read only by Also export as (Vanilla / Complex material) |

A normal map named only `_normal`, `_nrm` or `_norm` does not say which way its green channel points. Tessalith assumes DirectX, which is what Skyrim uses, converts it without a flip and notes `IN-GREEN-UNSTATED` as information. If the map is OpenGL, set `channels.green_convention = "opengl"` on the file (the GUI offers a *Treat as OpenGL* button on that note) or name it `_normalgl`.

You can add your own suffixes through a preset's `suffix_aliases`. See [Presets and targets](Presets-and-Targets.md#suffix-aliases).

## Sub-types

There are 29 sub-types in five categories.

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
| `complex_grass` | MixedPacked | BC7 only | UNORM only | cutout (when the alpha varies) | Always BC7 UNORM at its own size. See [Complex grass](#complex-grass). |
| `hdr_sky` | Hdr | BC6H | n/a | | `cs` only. |

Notes on the defaults:

- The "default alpha" is the starting point. Analysis changes it: a source with no alpha channel resolves to `remove`, and alpha whose minimum is 251 or more is treated as opaque (`ALPHA-NEAR-OPAQUE`). A colour map whose alpha really varies needs a decision unless scanned meshes supply one.
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

## Packing raw maps

Raw single-channel maps (from the exporter names above) are not converted on their own. They are packed into the layout the target reads. One source per role is used; if a set has two roughness maps, the first by path wins and the other is reported as `PACK-UNUSED-INPUT`.

An `_orm` file counts as three raw maps at once: AO from its R channel, roughness from G and metallic from B. Every recipe below that reads roughness, AO or metallic takes them from it. A separate map for the same slot (an `_roughness` next to the `_orm`) wins that slot; an info note, `PACK-ORM-OVERRIDDEN`, says so. The same note says when an `_rfaos` (which has no metallic channel) leaves the `_orm` metallic unused. Slots an `_orm` does not hold (specular, height, fuzz) come from their own maps, the fill, or the default, as usual. So an `_rmaos` built from an `_orm` alone has `packing.fill.specular` in alpha, or 1 when no fill is set.

### Community Shaders (`cs`)

`_rmaos` is built when the set has a roughness, glossiness, AO or metallic map:

| Channel | Source | Default when missing |
|---|---|---|
| R | roughness, or 1 minus glossiness | `packing.roughness_default` (0.5) |
| G | metallic | 0 |
| B | AO | 1 |
| A | specular | 1 |

`_rfaos` (Advanced skin) replaces it when a fuzz map exists (`packing.recipe = "auto"`), or always with `packing.recipe = "rfaos"`:

| Channel | Source | Default when missing |
|---|---|---|
| R | roughness, or 1 minus glossiness | 0.5 |
| G | fuzz | 0 |
| B | AO | 1 |
| A | specular | 1 |

`packing.recipe = "rfaos"` on a set that only has a metallic map builds an `_rmaos` instead (`PACK-RECIPE-UNBUILDABLE`).

`_p` is built from a raw height map.

### Vanilla, complex material set

Under `vanilla`, a set whose shader is complex material (picked in the GUI, `--set set:<id>.shader=complex_material`, or chosen automatically because the set already has a complex-material `_m` file) builds the complex `_m` when it has a roughness, glossiness, metallic, height or `_orm` map. If the set already has an `_m` file, that file still wins unless `packing.prefer = "raw"`.

| Channel | Source | Default when missing |
|---|---|---|
| R | environment: metallic, else specular | 0 |
| G | glossiness, or 1 minus roughness | 1 minus `roughness_default` |
| B | metallic | 0 |
| A | height | 0 |

### Vanilla, any other set

`_m` is a BC4 environment mask, built from metallic (else specular) only when one of them exists. Other raw maps are not used (`PACK-UNUSED-INPUT`).

The recipe id of the complex `_m` in events and manifests is `cpm_m`, a name kept from the removed `cpm` target.

### Fills and defaults

A channel with no source gets the neutral default and a `PACK-DEFAULTED` warning that names the output and the channel. To choose the value yourself, set a fill (0 to 255) on the packed output:

```toml
[files."textures/armor/iron/iron_rmaos"]
packing.fill.metallic = 255     # fully metallic
packing.fill.specular = 128
```

or for a whole run with `--fill metallic=255`. In the GUI, set it on the packed output's card in the set editor.

If a set has both a ready-made packed file (`iron_rmaos.png`) and raw maps for it, the packed file wins and the raw maps are reported (`PACK-CONFLICT`). Set `packing.prefer = "raw"` to build from the raw maps instead. All sources of one recipe must be the same size (`PACK-SIZE-MISMATCH` otherwise). Set `packing.enabled = false` to leave the raw maps alone (`IN-PACKING-INPUT`).

## Variance bake

In a `cs` PBR set with an `_n` and an `_rmaos` or `_rfaos`, the detail that the normal map loses in each smaller mip is added to the roughness mips, so distant surfaces do not look too shiny. The amount is limited by `mips.bake_clamp`. A `_cnr` bakes its own detail into its coat roughness.

The bake needs both files in the batch, with power-of-two sizes of the same aspect. If not, it is skipped and reported: `SET-MISSING-COMPANION`, `SET-COMPANION-OUTSIDE` (the companion is on disk but not in the batch), `SET-COMPANION-DDS` (the companion only exists as DDS) or `SET-BAKE-RATIO`. A shared `_rmaos` (borrowed by several sets in the GUI) is never baked.

## RNM normal layers

A set can add detail normal maps that are blended into its base normal (`_n`, or `_cnr`) with reoriented normal mapping before the mips are built. Layer files are never written on their own.

| Layer | File suffixes | Mask suffixes | Default strength |
|---|---|---|---|
| bump | `_bump` | `_bumpmask`, `_bump_mask` | 0.4 |
| bump2 | `_bump2` | `_bumpmask2`, `_bump2_mask` | 0.4 |
| detail | `_detailnorm`, `_detail` | `_detailnormmask`, `_detail_mask` | 0.6 |
| detail2 | `_detailnorm2`, `_detail2` | `_detailnormmask2`, `_detail2_mask` | 0.3 |
| bent | `_bent` | `_bentmask`, `_bent_mask` | 0.35 |

Any of these can be prefixed `n_` or `cnr_` (for example `iron_cnr_bump.png`) to say which base it feeds. Without a prefix it feeds `_n`, or `_cnr` when the set has no `_n`. Override with `rnm.target` on the layer file.

The default blend order is bent, bump, bump2, detail, detail2. The `bent` layer by default weights itself by occlusion read from a raw `_ao` map, the R channel of an `_orm`, or the B channel of `_rmaos`/`_rfaos`; supply normal AO (white = open), Tessalith inverts it.

Settings are in [Project config](Project-Config.md#rnm-normal-layers). `sdc inspect` prints one `rnm` line per layer. Codes: `RNM-NO-BASE`, `RNM-IGNORED-EYE` (eye normals are model-space), `RNM-MASK-ORPHAN`, `RNM-LAYER-SHADOWED`, `RNM-AO-MISSING`.

## Set shaders

Each set is made for one Skyrim shader. The shader decides which maps the set should have, and it can change some members' types and settings.

| Shader | Required maps | Optional maps |
|---|---|---|
| `pbr` | albedo, `_n`, `_rmaos` | `_g` emissive, `_s` subsurface/coat, `_cnr`, `_f`, `_p` |
| `pbr_landscape` (cs only) | albedo (or landscape diffuse), `_n` | `_rmaos`, `_p` |
| `advanced_skin` | skin albedo, `_n`/`_msn`, `_rfaos` | `_sk`, `_wet` |
| `default` | diffuse, `_n` | `_b`/`_bl` |
| `envmap` | diffuse, `_n` | `_m`, `_e` |
| `complex_material` | diffuse, `_n`, complex `_m` | `_e` |
| `glow` | diffuse, `_n`, `_g` | |
| `parallax` | diffuse, `_n`, `_p` | |
| `skin` | diffuse, `_n`/`_msn` | `_sk`, `_s` (with `_msn`) |
| `hair` | diffuse, `_n` | `_flow` (`cs` only; vanilla hair does not read it) |
| `eye` | diffuse, `_n` (model-space) | `_sk`, `_m`, `_e` |
| `multilayer` | diffuse, `_n`, `_i` | `_m`, `_e`, `_b` |
| `landscape` | diffuse, `_n` | |
| `complex_grass` | the complex-grass texture | |

A missing required map gives `SHADER-MISSING-MAP`; a member the shader does not use gives `SHADER-EXTRA-MAP`. Both are warnings, and extra members still convert.

### How the shader is picked

You can pick a set's shader in the GUI's set editor, or with `--set set:<id>.shader=<shader>`. Otherwise the first rule that fits decides (`SHADER-AUTO` says which):

1. Advanced skin members or an `_rfaos`: `advanced_skin`.
2. A PBR set or PBR members: `pbr_grass` when a grass record uses the albedo (GUI, after a scan with plugins); under `cs`, `pbr_landscape` when the set is under `textures/landscape/`, has a `landscape` folder in its path, or has a Community Shaders texture-set block (GUI); otherwise `pbr`.
3. A complex-grass member: `complex_grass`.
4. A complex `_m`: `complex_material`.
5. A `_p`: `parallax`.
6. In the GUI, after a mesh scan: the shader most of the meshes using the diffuse have (a strict majority).
7. An `_m` env mask: `envmap`.
8. `_g`: `glow`. `_i`: `multilayer`. `_sk`: `skin`. `_flow`: `hair`, on both targets, unless the path has a `water` or `landscape` folder.
9. A `hair` folder in the path: `hair`. An `eyes` folder: `eye`.
10. Landscape textures: `landscape`.
11. Otherwise `default`.

A shader the target lacks falls back to `default` (`SHADER-UNAVAILABLE`).

### What a shader changes

| Shader | Effect on members |
|---|---|
| `hair` | Diffuse: `alpha.policy = "cutout_blend"` (cutout + blend) and `mips.coverage = "on"`. |
| `eye` | `_n` becomes `msn` (model-space). `_m` is BC4. |
| `landscape` | Colour becomes `landscape_diffuse` (alpha is the snow mask). |
| `pbr_landscape` | Nothing: members convert exactly as under `pbr`. |
| `multilayer` | Diffuse alpha is `data`. |
| `envmap` | `_m` is BC4. |
| `parallax` | `_p` is BC4. |
| `complex_grass` | Colour becomes `complex_grass`. |

In the GUI, when a sub-type changes this way, a format or tag you set that the new type does not allow is reset to the new type's default. On the command line the value you set stays, and the file is blocked or reports an error.

### Gloss bake for a complex `_m` (opt-in)

The same bake can also feed a complex-material `_m`'s green channel (its glossiness). It is off by default. Set `mips.normal = "renormalize_bake"` (the GUI's *Normal handling: Renormalise and bake*) on the set's `_n` to turn it on: every smaller mip of the `_m` gets the detail its normal map loses, so distant surfaces shimmer less. Mip 0 is not changed unless the normal is larger than the `_m`. Without the setting the `_m` is written as before. It needs the `_n` in the batch with power-of-two sizes of the same aspect, and otherwise reports `SET-BAKE-RATIO` and writes the `_m` unbaked. The choice lives in `sdc.toml`, so the CLI writes the same bytes. A project that already had this value on a vanilla `_n` changes its complex `_m` on the next export.

## Height maps and the artist checks

The Parallax check judges a height map against the neutral height (the value that shifts nothing) for the set's shader. White is raised and black is deepest on every path.

| Path | Neutral | Where the height lives |
|---|---|---|
| Vanilla parallax | 0.5 | `_p`, red |
| Complex material | 0.5 | `_m` alpha. Parallax is on only where alpha is strictly between 4/255 and 251/255; 4/255 or less keeps complex material on with no parallax |
| PBR parallax | 0.5 | `_p`, red |
| PBR multilayer with inner-layer (coat) parallax | 1.0 (white); black is the deepest recess and nothing is raised | `_p`, red |
| Landscape, vanilla and PBR | 0.5 per layer | The layer alpha or displacement map |
| Vanilla multilayer | Not a height map: the layer texture's alpha is a depth, so the height checks are skipped | The layer texture's alpha |

A mostly white `_p` is normal for the coat path and a warning on the 0.5 paths. Landscape sets whose `displacementScale` differs by more than 2x from the others are flagged too (`CHK-PARALLAX-SCALE`).

Other things the checks look at:
- **Green direction.** A normal map that curls like OpenGL when it is read as DirectX gets the Info hint `CHK-GREEN-GUESS`, which suggests *Treat as OpenGL (flip green)* and never flips by itself. The test is weak on straight ridges, so look at the map. A blue channel that points the wrong way raises `CHK-Z-SIGN`.
- **Plastic `_n` alpha.** On a vanilla `_n` the alpha is the specular. If it is missing or almost all white (at least 95 % of texels at 250 or more), the surface may look plastic (`CHK-PLASTIC`). It never fires on `pbr_normal` or `_msn`. Community Shaders does not read that alpha on hair or Advanced Skin, and the message says so.
- **`drop_top_mip`.** A per-file field (`[files."up.png"] drop_top_mip = true`, or `--set file:<glob>.drop_top_mip=true`) that writes mip 1 as the top level: half the size, one level fewer. It is off by default and absent from the profile hash then, so existing outputs do not change. A level is dropped from what is written only, so a PBR set's `_rmaos` bake is the same. It does nothing on a texture with no mip chain. The GUI writes it from the **Export at half size?** box and from Advanced → Half size.

## Complex grass

A complex-grass texture is one image twice as tall as it is wide, with power-of-two sides (for example 1024x2048): diffuse and alpha in the top half, normal and specular in the bottom half. Community Shaders detects it from the bottom-left texel.

- Set the set's shader to `complex_grass`, set `subtype = "complex_grass"` on the file's `[files]` entry, or let the GUI tag it after a scan that included plugins: a texture used by a grass (GRAS) record, 1:2, with a valid detection corner and a normal-map bottom half gets a High-confidence `complex_grass` suggestion, which the GUI accepts. The GUI's sub-type picker does not offer `complex_grass`.
- The CLI never tags it automatically. If you convert the same folder with `sdc convert`, add `subtype = "complex_grass"` to the file's `[files]` entry, or it is converted as a diffuse.
- Codes: `GRASS-SHAPE` (error: not 1:2 or not power of two), `GRASS-TOP-OPAQUE`, `GRASS-NORMALS`, `GRASS-DETECT-SOURCE` (the source corner already fails the CS test), and `VAL-GRASS-DETECT` (error: the encoded corner still fails after a slowest re-encode).

## Also export as Vanilla and CM

With `[export] also` (see [Project Config](Project-Config.md#export)), a cs export also writes a Vanilla folder and a Complex material (CM) folder. Each PBR set (by its shader in the PBR export) is converted from its albedo, `_n`, `_rmaos` (or raw roughness, metallic and AO maps) and `_p`; every other texture is copied from the PBR folder, or encoded when the copy would differ. The values below use the `[export.legacy]` defaults; gloss is 1 − roughness.

| Shader | Vanilla folder | CM folder |
|---|---|---|
| PBR | `<name>.dds` diffuse = 0.9 × albedo × lerp(1, AO, 0.5) × lerp(1, 0.3, metal); `_n` = the normal, alpha = 0.66 × gloss × AO × lerp(1, 2, metal); env `_m` = metal × gloss when the meshes use the environment-map shader (needs a Meshes scan) | `_m`: R = gloss × metal, G = gloss (at least 5/255), B = metal, A = `_p` height (5/255–250/255, 0 without a `_p`); with Keep metal brightness, also a diffuse without the metal darkening |
| PBR Landscape | landscape diffuse with AO strength 0.3 and no metal factor, alpha = the `_p`; landscape `_n`, alpha = 2.5 × 0.66 × gloss × AO; the set's `_p` (for Terrain Helper) | `_m` with R = B = 0 |
| PBR grass | the converted diffuse only | the complex-grass stack (diffuse on top, `_n` below) |

- **Install** Vanilla alone, or CM on top of Vanilla. With Keep metal brightness off, the CM folder holds only `_m` files (plus the complex-grass stack of each PBR grass set). Files an earlier export left in the folders are kept and listed in the report as "not written by this export"; delete them yourself.
- **`_curves` / `_curvature` and `_cavity`** maps (see [Suffixes](#suffixes)) shade the diffuse: a soft light with the curves map (0.5 = no change), then a multiply with the cavity map, both at `curves_opacity` (0.4). They are read only by Also export; a plain export writes nothing for them (`PACK-UNUSED-INPUT`).
- A set is not converted when a source it needs is a DDS (`DERIVE-DDS-SOURCE`), or when you skip it (`[export] skip`). The report lists such sets with the reason.
- The defaults were checked against mods that ship both a PBR and a vanilla version by the same author (DECISIONS Note CN).

## Alpha policies

| Policy | Meaning | Mip handling |
|---|---|---|
| `remove` | No alpha; stored opaque. | |
| `cutout` | Alpha testing against `alpha.threshold` with `alpha.compare`. | Coverage preserved per mip (unless `mips.coverage = "off"`), so foliage does not thin out in the distance. |
| `blend` | Alpha blending. | Box-averaged opacity. |
| `cutout_blend` | Both. | Coverage preserved. |
| `data` | Alpha is data (specular, height, a mask). | Treated as a plain channel. |

Discarding varying alpha by accident is blocked (`LOSS-ALPHA-DATA`), except when you chose `remove` yourself or the file is a `pbr_normal`.
