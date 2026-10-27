# Cubemaps

> **Coming in 1.1.0.** This page describes the planned cubemap support. Until it ships, Tessalith refuses cube DDS files.

A cubemap is six square pictures, one for each direction around a point. Skyrim uses them for reflections: the `_e` texture on Environment Map and Eye shaders (NIF slot 4), plus room reflections in caves and temples. Tessalith takes whatever you have and writes a correct cube DDS with a full mip chain.

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

Faces exported by a cube tool (texassemble, CubeMapGen, cmft) are usually stored this way already and need no setting.

### Inside the file

![The file starts with a header, then +X with all its mips, then −X, +Y, −Y, +Z and −Z, each with all its mips.](../img/cube-dds-order.svg)

Tessalith always writes:
- all six faces;
- every mip level down to 1×1, built across faces so no seams appear;
- a non-sRGB format, because Community Shaders converts the colour itself.

## Ways to make one

| You have | What it looks like | How Tessalith knows |
|---|---|---|
| [Six images](#six-images) | six squares | file names end in a face suffix |
| [A cross](#crosses-and-strips) | a T-shaped net, 4:3 or 3:4 | from the shape |
| [A strip](#crosses-and-strips) | six squares in a row, 6:1 or 1:6 | from the shape |
| [A panorama](#panorama) | a 2:1 wide photo | from the shape |
| [A mirror ball or angular map](#mirror-ball-and-angular-map) | a circle in a square | you set `cubemap.layout` |
| [One ordinary photo](#single-photo) | any picture | you set `cubemap.layout = "photo"` |
| [A cube DDS](#an-existing-cube-dds) | a `.dds` cube | the file says so |
| [Nothing: a dynamic marker](#dynamic-marker) | no picture | `sdc cubemap-marker` |

A file is treated as a cubemap when its name ends in `_e` (for example `chrome_e.png`) or it sits under a `cubemaps` folder.

A square image with no layout set is skipped, because it could be a mirror ball, an angular map or a photo. Set `cubemap.layout` for that file in `sdc.toml`:

```toml
[files."textures/cubemaps/studio_e.png"]
cubemap.layout = "mirror_ball"   # hcross, vcross, hstrip, vstrip, equirect, mirror_ball, angular, photo
```

### Six images

Name them with the base name plus a face suffix:

```text
textures\cubemaps\
  chrome_e_posx.png   east
  chrome_e_negx.png   west
  chrome_e_posy.png   north
  chrome_e_negy.png   south
  chrome_e_posz.png   sky
  chrome_e_negz.png   ground
```

They become one `chrome_e.dds`. A missing face is an error; Tessalith never guesses one. Turn faces with the settings in [How each face is turned](#how-each-face-is-turned).

### Crosses and strips

![Four layouts. Horizontal cross: +Y north on top; −X west, +Z sky, +X east, −Z ground across the middle; −Y south below. Vertical cross: the same, with −Z ground at the bottom, turned 180°. Horizontal strip and vertical strip: +X, −X, +Y, −Y, +Z, −Z in order.](../img/cube-layouts.svg)

Cells are read as stored faces: Tessalith never moves a face to another cell. In Skyrim, the sky goes in the **centre** cell of a cross (+Z), not the top one.

- A cross made by a tool that puts the sky on top shows the sky to the north in game. Rearrange the cells first, or export a panorama from that tool instead.
- Tools disagree on the vertical cross's bottom cell. If the ground comes out upside down, set `cubemap.vcross_negz_rotate = false`.
- A 4:3 photo named `_e` is not mistaken for a cross: a cross's empty cells must be flat, or the file is refused with a message pointing at `cubemap.layout`.

### Panorama

![A 2:1 panorama: sky on the top half, ground on the bottom half. The top row maps to straight up, the middle row to the horizon, the bottom row to straight down. The centre column is east at yaw 0. The left and right edges meet behind you.](../img/cube-panorama.svg)

Also called equirectangular or lat-long. This is what 360° cameras, HDRI sites and skybox generators produce. `.hdr` and `.exr` panoramas work too.
- **Turning:** `cubemap.yaw` turns the panorama (in degrees, clockwise seen from above), and `cubemap.mirror` swaps east and west.
- **HDR:** an HDR sky with a bright sun needs `cubemap.highlight_clamp`, or the sun blows out the whole reflection.

### Mirror ball and angular map

![Two circles. Mirror ball: the centre shows south, behind the camera; the outer ring shows north, straight ahead; up is at the top, east on the right. Angular map: the centre shows north, straight ahead; the dashed half-radius ring is 90° away; the outer ring is south.](../img/cube-ball-angular.svg)

Both are square images with the picture in a circle, so set the layout yourself:
- `mirror_ball` is a photo of a chrome ball;
- `angular` is a light probe.

Both assume the camera looked north. Use `cubemap.yaw` to turn the result.

### Single photo

![A wide strip: your photo in the middle half, mirrored copies on both sides, the top fading to the photo's top-row colour and the bottom fading to its bottom-row colour.](../img/cube-photo.svg)

One photo can't show what is behind you, so this mode makes a believable reflection rather than a true one. Tessalith builds the fake panorama shown above, makes a cube from it and blurs it hard (`cubemap.blur = 0.6` unless you set another value). It works well for shiny metal and glass, where the reflection only needs the right colours and light.

### An existing cube DDS

A cube DDS goes through the same filters, size and format settings as any other source. Like any DDS, it is skipped unless you set `reencode = true` on the file. That costs some quality, because the faces are decoded and encoded again.

### Dynamic marker

![Six one-pixel faces. +X holds a colour with roughness in alpha; +Y and every other face is black.](../img/cube-marker.svg)

Community Shaders (and ENB) read a 1×1 cube whose +Y is black as an instruction to reflect the live scene, not as a picture. The +X pixel then sets the reflection's colour (RGB) and roughness (alpha). Write one on purpose:

```text
sdc cubemap-marker --color 200,200,210 --roughness 0.3 -o textures\cubemaps\steel_e.dds
```

Tessalith never writes a picture cube at 1×1 or 2×2 by accident, so a real cube can't turn into a marker.

## Settings

All are optional and set per file or set in `sdc.toml`.

| Key | What it does |
|---|---|
| `cubemap.style` | Starting point for size, format and blur (below) |
| `cubemap.layout` | Source layout, when the shape doesn't settle it |
| `cubemap.face_size` | Face size in pixels (a power of two, at least 4) |
| `cubemap.yaw`, `cubemap.mirror` | Turn the cube about the up axis; swap east and west |
| `cubemap.face_rotate`, `cubemap.face_flip` | Turn or flip single faces: six entries in face order (0, 90, 180, 270; `h`, `v`) |
| `cubemap.vcross_negz_rotate` | Whether the vertical cross's bottom cell is turned 180° |
| `cubemap.blur` | 0 (sharp) to 1 (very soft) |
| `cubemap.exposure`, `brightness`, `contrast`, `midtones`, `saturation`, `tint` | Colour adjustments |
| `cubemap.highlight_clamp` | Caps bright HDR highlights before converting to LDR |
| `cubemap.ground_fill`, `cubemap.horizon` | Replace everything below the horizon with a colour or a gradient |

### Styles

| `cubemap.style` | Use it for | Starts at |
|---|---|---|
| `vanilla_metal` | armour, weapons, ore, glass | 32 px, BC1, strong blur |
| `room` | interior room reflections, caves | 128 px, BC1, medium blur |
| `eye` | eyes | 256 px, BC7, medium blur |
| `hd` | close-up shiny surfaces | 512 px, BC7, light blur |

Every value a style sets can be overridden by its own key.

## Checks

`sdc inspect` and validation flag these broken cubes:
- a cube with no mips (sparkles and looks pixelated in game);
- a flat 2D texture named `_e` or under `textures\cubemaps\` where a cube is expected;
- a 1×1 cube whose +Y isn't black (Community Shaders shows it as a nearly black static reflection);
- a cube in an sRGB format.
