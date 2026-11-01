# Shaders, height maps and extra exports

What a set's shader changes and how it is picked, the height-map checks and generated maps, complex grass, and the extra Vanilla and complex-material exports. For modders converting whole materials.

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

In the GUI, a format or tag the new type does not allow is reset ([Encoding follows type](GUI-Texture-Panel.md#editing)). On the command line the value you set stays, and the file is blocked or reports an error.

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
- **`drop_top_mip`.** A per-file field (`[files."up.png"] drop_top_mip = true`, or `--set file:<glob>.drop_top_mip=true`) that writes mip 1 as the top level: half the size, one level fewer. It is off by default and absent from the profile hash then, so existing outputs do not change. A level is dropped from what is written only, so a PBR set's `_rmaos` bake is the same. It does nothing on a texture with no mip chain. The GUI writes it from Review's half-size question (when you press Export) and from Advanced → Half size.

## Generated height maps

The GUI can integrate a set's normal map into a height map (**Generate from normal map…**). The result is an ordinary source file next to the normal:

| File | Used as |
|---|---|
| `<base>_p_generated.png` | The set's `_p` |
| `<base>_height_generated.png` | The raw height packed into a complex material `_m` alpha (vanilla target) |
| `<base>_ao_generated.png` | The AO packed into the `_rmaos` blue channel (PBR sets, `cs` target only) |
| `<base>_curves_generated.png` | The set's `_curves` (read only by Also export) |

- 16-bit grey PNG. `_generated` after a generated role (`_p`, `_height`, `_ao`, `_curves`) is stripped when grouping, so the file joins its set by its base name.
- **Two ranges:** 0.5 is "no shift" (Parallax, PBR, complex material, landscape); 1.0 is "no shift" for PBR multilayer with inner-layer parallax, where white is the top and nothing is raised.
- **A real map always wins.** A real `_p`, `_height` or other height source for the same slot makes the generated file skipped with `GEN-REPLACED` (Info), never `IN-DUPLICATE` or `PACK-CONFLICT`. An earlier export of the generated `_p` sitting in the input folder is not a rival.
- **Side effect:** a `_p` wins the shader auto-pick, so a default set becomes Parallax and PGPatcher patches its meshes; on a TruePBR set the `parallax` key turns on by itself (Note AJ).
- `GEN-STALE` (Info, GUI only) marks a generated file whose normal, neutral or shader changed since; it shows on the slot row's hover and as a Problems warning with **Regenerate…** ([Generate from normal map](GUI-Generate-From-Normal.md)).
- **Generated AO** holds raw visibility (1 = open), with no strength baked in. A real `_ao` wins over it, and so does a pre-packed `_rmaos`/`_rfaos` image whose blue channel already holds AO (or one beside a loose roughness, metallic, specular or `_orm`). A pre-packed image whose blue is flat white gets the generated AO in B, with R, G and A kept from the image. It is written at the size of the set's other `_rmaos` sources, so the packed `_rmaos` has one size, and `GEN-AO` (Info) replaces `PACK-DEFAULTED` for blue. A baked AO is better; a generated one cannot see the broad cavity shading a bake gives. If the set's normal has an RNM bent layer, the bent layer is weighted by this AO too, so the normal map changes.
- **Generated curvature** is the Laplacian of the same height, smoothed by a Gaussian of 2 texels at the solve size (up to 1024 px), stored 0.5-centred with peaks bright. It is written at the set's albedo size, because Also export's soft light reads both, and only for sets Also export derives (PBR, PBR landscape and PBR grass on `cs`, with an albedo and no DDS member). A real `_curves` or `_curvature` wins over it (`_cavity` is another map). A plain export does not use it.

## Complex grass

A complex-grass texture is one image twice as tall as it is wide, with power-of-two sides (for example 1024x2048): diffuse and alpha in the top half, normal and specular in the bottom half. Community Shaders detects it from the bottom-left texel.

- Set the set's shader to `complex_grass`, set `subtype = "complex_grass"` on the file's `[files]` entry, or let the GUI tag it after a scan that included plugins: a texture used by a grass (GRAS) record, 1:2, with a valid detection corner and a normal-map bottom half gets a High-confidence `complex_grass` suggestion, which the GUI accepts (and records in `sdc.toml`). The GUI's sub-type picker offers **Complex grass** where the set's shader is a grass shader or no `textures` folder is above the file. A diffuse with the complex grass shape that nothing tags gets an info row in Problems with **Tag as grass**, which writes `subtype = "complex_grass"`.
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
- **`_curves` / `_curvature` and `_cavity`** maps (see [Suffixes](Texture-Types.md#suffixes)) shade the diffuse: a soft light with the curves map (0.5 = no change), then a multiply with the cavity map, both at [`curves_opacity`](Project-Config.md#export). They are read only by Also export; a plain export writes nothing for them (`PACK-UNUSED-INPUT`).
- A set is not converted when a source it needs is a DDS (`DERIVE-DDS-SOURCE`), or when you skip it (`[export] skip`). The report lists such sets with the reason.
- The defaults were checked against mods that ship both a PBR and a vanilla version by the same author (DECISIONS Note CN).
