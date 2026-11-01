# Cubemaps

> **Coming in 1.1.0.** This page describes cubemap support in the 1.1.0 release. In 1.0.0, Tessalith refuses cube DDS files.

A cubemap is six square pictures, one for each direction around a point. Skyrim uses them for reflections: the `_e` texture on Environment Map and Eye shaders (NIF slot 4), plus room reflections in caves and temples. Tessalith takes six faces, a cross, a strip, a 360° panorama, a mirror ball, an angular map or a single photo, and writes a correct cube DDS with a full mip chain.

## Which face is which

Skyrim's world has X pointing east, Y north and Z up. Each face is named after the direction it shows:

![Two diagrams. Seen from above: +Y north is face 2, +X east is face 0, −Y south is face 3, −X west is face 1. Seen from the side: +Z up, the sky, is face 4; −Z down, the ground, is face 5.](../img/cube-axes.svg)

| Face | Index | Shows | File suffix |
|---|---|---|---|
| +X | 0 | east | `_posx` or `_px` |
| −X | 1 | west | `_negx` or `_nx` |
| +Y | 2 | north | `_posy` or `_py` |
| −Y | 3 | south | `_negy` or `_ny` |
| +Z | 4 | **up: the sky** | `_posz` or `_pz` |
| −Z | 5 | **down: the ground** | `_negz` or `_nz` |

**The sky is +Z, not +Y.** Many tools and tutorials for other games put the sky on +Y. In Skyrim, +Y is north.

### How each face is turned

Each face is a square image, and each edge of that image points somewhere in the world. This was measured in game, and it is plain DirectX cube addressing:

![Six coloured squares, one per face, in file order. +X east: top edge points north, left edge points up. −X west: top north, left down. +Y north: top down, left west. −Y south: top up, left west. +Z sky: top north, left west. −Z ground: top north, left east.](../img/cube-faces.svg)

Compared with what you would see standing in the middle of the cube, every stored face is mirrored. If your six images are views from the centre (sky at the top for the four side views, north at the top for the sky and ground views), set these per face:

| Face | What to change |
|---|---|
| +X east | rotate 90° clockwise, then mirror left-right |
| −X west | rotate 90° clockwise, then flip top-bottom |
| +Y north | flip top-bottom |
| −Y south | mirror left-right |
| +Z sky | mirror left-right |
| −Z ground | mirror left-right |

Both settings are six-entry lists in face order (+X, −X, +Y, −Y, +Z, −Z), on the cube's entry:

```toml
[files."textures/cubemaps/chrome_e"]
cubemap.face_rotate = [90, 90, 0, 0, 0, 0]          # clockwise, applied first
cubemap.face_flip = ["h", "v", "v", "h", "h", "h"]  # h mirrors left-right, v flips top-bottom
```

They work on every layout, six separate files included. Faces exported by a cube tool (texassemble, CubeMapGen, cmft) are usually stored this way already and need no setting.

### Inside the file

![The file starts with a header, then +X with all its mips, then −X, +Y, −Y, +Z and −Z, each with all its mips.](../img/cube-dds-order.svg)

Tessalith always writes:
- all six faces;
- every mip level down to 1×1, built across faces so no seams appear;
- a UNORM format (BC1, BC3, BC7 or RGBA8). An sRGB tag on a cube is refused.

### Colour

Community Shaders reads a static cube with the game's own curve, `pow(colour, 1.6)`, not the sRGB curve. Tessalith does the same:
- A PNG, TGA or other normal image is read as game-gamma colour. Mips, blur and resizing are done in linear light, then stored back with the same 1.6 curve.
- A `.hdr` or `.exr` source is linear light already and is only stored with the curve.
- The setting still reads `color.content = "srgb"` for a cube. On a cube it means the game's 1.6 curve. Other texture types keep the real sRGB curve.
- The colour settings below (brightness, contrast, midtones, saturation, tint) also work on that curve, so a change looks the same in game.

## Ways to make one

The kinds of source a cube can be made from, and how each is read: [Cubemap sources](Cubemap-Sources.md).

## Settings

All are optional and set per file or in `sdc.toml`, under `[defaults]`, `[subtypes.cubemap]`, a rule or a `[files]` entry. They also work with `--set`. Ranges and defaults are in [Project config](Project-Config-Fields.md#cubemap).

| Key | What it does |
|---|---|
| `cubemap.style` | Starting point for size, format and blur (below) |
| `cubemap.layout` | Source layout, when the shape doesn't settle it |
| `cubemap.face_size` | Face size in pixels |
| `cubemap.yaw`, `cubemap.mirror` | Turn the cube about the up axis (degrees, -360 to 360); swap east and west |
| `cubemap.face_rotate`, `cubemap.face_flip` | Turn or flip single faces: six entries in face order (0, 90, 180, 270; `h`, `v`) |
| `cubemap.vcross_negz_rotate` | Turn the vertical cross's bottom cell ([values](Project-Config-Fields.md#cubemap)) |
| `cubemap.blur` | 0 (sharp) to 1 (very soft) |
| `cubemap.exposure`, `brightness`, `contrast`, `midtones`, `saturation`, `tint` | Colour adjustments |
| `cubemap.highlight_clamp` | Soft-compresses bright highlights so a chosen level becomes full white |
| `cubemap.ground_fill`, `cubemap.horizon` | Replace everything below the horizon with a colour or a gradient |

### Styles

| `cubemap.style` | Use it for | Starts at |
|---|---|---|
| `vanilla_metal` | armour, weapons, ore, glass | 32 px, BC1, strong blur |
| `room` | interior room reflections, caves | 128 px, BC1, medium blur |
| `eye` | eyes | 256 px, BC7, medium blur |
| `hd` | close-up shiny surfaces | 512 px, BC7, light blur |

Every value a style sets can be overridden by its own key. `vanilla_metal` also lowers saturation to 0.8.

### Ground fill and horizon

A panorama or photo often shows the photographer's feet or a patchy floor. `cubemap.ground_fill` covers everything below the horizon line:
- `"gradient"` takes the average colour at the horizon and darkens it towards straight down, to half at the bottom;
- `[r, g, b]` is a flat colour. The three numbers (0 to 255) are code values on the game's 1.6 curve, like `cubemap.tint`, not sRGB.

`cubemap.horizon` sets where the line sits, from -45 to 45 degrees (0 is level; positive is above it). It blends over about two degrees around the line. If `ground_fill` is not set, nothing is filled and `horizon` does nothing. A bad value is an error naming the two accepted forms.

```toml
[files."textures/cubemaps/studio_e.png"]
cubemap.ground_fill = [30, 28, 26]
cubemap.horizon = -10
```

### Order of work

Tessalith applies the settings in this order: import, face rotate and flip, yaw, mirror, exposure, highlight clamp and colour, ground fill, blur, mips.

### Blur

`cubemap.blur` blurs the cube evenly across the faces, so no seams show. It is a cone blur, not the GGX filter a game engine might use for rough metal. The default is no blur, unless a style sets one or the layout is `photo` (0.6).

### HDR sources

An `.hdr` or `.exr` face whose brightest value is above 1 and that has no `cubemap.highlight_clamp` gets `LOSS-CLAMP-HDR`. A bright sun would otherwise clip to white. Set `cubemap.highlight_clamp` to the level that should become full white. Brightness is rolled off smoothly toward it, so darker values come down a little too, and hue is kept.

## Checks

A cube has no mips: `CUBE-NO-MIPS` warns when `mips.levels` is `none` or 1. A cube without mips sparkles and looks pixelated in game.

`sdc inspect` and `sdc validate` flag these broken cube files (as `VAL-HEADER` warnings in `validate`; all but the second fail the file):
- a cube wider than 1 pixel with only one mip (a 1×1 marker is not flagged);
- a flat 2D texture whose name ends in `_e`, or that sits in `textures\cubemaps\`, where a cube is expected;
- a 1×1 cube whose +Y isn't black (Community Shaders shows it as a nearly black static reflection);
- a cube in an sRGB format.

The second check is a warning in 1.x: a 2D `_e` or `cubemaps\` file passes (exit 0), with the warning. The game reads those files as cubes, so a 2D one is a fault; pass `--strict-cubemaps` to fail it. Failing it by default would break existing use, so it waits for 2.0.

The messages read:

- `2D texture where a cubemap is expected (the game reads _e and textures\cubemaps\ files as cubes)`
- `cube has no mips: curved surfaces will sparkle; re-encode it with a full chain`
- `1x1 cube with a non-black +Y: Community Shaders treats it as a static, near-black reflection, not a dynamic-cubemap marker`
- `cube uses an sRGB format; Community Shaders expects UNORM cubes`

`sdc validate` judges a cube file as a `cubemap` unless you pass `--subtype`. A marker passes: its colour sits on +X and +Y stays black.

## In the GUI

The GUI controls for each of these settings are in [Cubemaps in the GUI](GUI-Helpers-RNM-and-Cubemaps.md#cubemaps).
