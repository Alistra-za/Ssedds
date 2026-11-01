# Cubemap sources

Every kind of source a cubemap can be made from: six images, crosses and strips, panoramas, mirror balls, a single photo, an existing cube DDS and the dynamic marker. For modders making environment maps; the rest of the topic is on [Cubemaps](Cubemaps.md).

## Ways to make one

| You have | What it looks like | How Tessalith knows |
|---|---|---|
| [Six images](#six-images) | six squares | file names end in a face suffix |
| [A cross](#crosses-and-strips) | a T-shaped net, 4:3 or 3:4 | from the shape |
| [A strip](#crosses-and-strips) | six squares in a row, 6:1 or 1:6 | from the shape |
| [A panorama](#panorama) | a 2:1 wide photo | from the shape, exactly 2:1 |
| [A mirror ball or angular map](#mirror-ball-and-angular-map) | a circle in a picture | you set `cubemap.layout` |
| [One ordinary photo](#single-photo) | any picture | you set `cubemap.layout = "photo"` |
| [A cube DDS](#an-existing-cube-dds) | a `.dds` cube | the file says so |
| [Nothing: a dynamic marker](#dynamic-marker) | no picture | `sdc cubemap-marker` |

A file is treated as a cubemap when its name ends in `_e` (for example `chrome_e.png`) or it sits under a `cubemaps` folder. Tessalith suggests the `cubemap` type for these with High confidence.

Only a crisp shape is detected: 4:3, 3:4, 6:1, 1:6 and exactly 2:1 (a 4096×2049 image is not). A square image with no layout set is skipped (`SUG-LOW-CONFIDENCE`), because it could be a mirror ball, an angular map or a photo. Mirror balls, angular maps and photos are never detected: set `cubemap.layout` for that file in `sdc.toml`. You can set any layout yourself, and an explicit `equirect`, `mirror_ball`, `angular` or `photo` takes an image of any non-zero size.

```toml
[files."textures/cubemaps/studio_e.png"]
cubemap.layout = "mirror_ball"   # faces, hcross, vcross, hstrip, vstrip, equirect, mirror_ball, angular, photo
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

They become one `chrome_e.dds`. The six files are one set, so the shader check sees a single `_e`. A face tag on any other name is left alone: `rock_px.png` is its own texture.

Tessalith never guesses a face. These are errors (`CUBE-FACES`), and the faces are not converted:
- two files for one face;
- a missing face;
- a face that is not square;
- faces of different sizes;
- faces that resolve to different settings (one file has its own `[files]` entry, for example). Set things for all six with a rule or `[defaults]`, so they match.

### Crosses and strips

![Four layouts. Horizontal cross: +Y north on top; −X west, +Z sky, +X east, −Z ground across the middle; −Y south below. Vertical cross: the same, with −Z ground at the bottom, turned 180°. Horizontal strip and vertical strip: +X, −X, +Y, −Y, +Z, −Z in order.](../img/cube-layouts.svg)

The shape tells Tessalith the layout: 4:3 is a horizontal cross, 3:4 a vertical cross, 6:1 a horizontal strip and 1:6 a vertical strip. The match is exact. Cell positions are the ones texassemble uses.

Cells are read as stored faces: Tessalith never moves a face to another cell. In Skyrim, the sky goes in the **centre** cell of a cross (+Z), not the top one.

- A cross made by a tool that puts the sky on top shows the sky to the north in game. Rearrange the cells first, use six images, or export a panorama from that tool instead.
- Tools disagree on the vertical cross's bottom cell. Tessalith turns it 180° by default. If the ground comes out upside down, set `cubemap.vcross_negz_rotate = false`.
- A 4:3 photo named `_e` is not mistaken for a cross: a cross's empty cells must be flat, or the file is refused with a message pointing at `cubemap.layout`. A layout you set yourself is trusted.
- A layout that does not fit the image (`hstrip` on a 512×512 file) is `CUBE-FACES`.

### Panorama

![A 2:1 panorama: sky on the top half, ground on the bottom half. The top row maps to straight up, the middle row to the horizon, the bottom row to straight down. The centre column is east at yaw 0. The left and right edges meet behind you.](../img/cube-panorama.svg)

Also called equirectangular or lat-long. This is what 360° cameras, HDRI sites and skybox generators produce. `.hdr` and `.exr` panoramas work too. The centre column faces east, and the picture runs clockwise seen from above (north a quarter of the way in, south three quarters).

An exact 2:1 image is read as a panorama without any setting. Any other size needs `cubemap.layout = "equirect"`.

- **Checked in game:** a cube made from a panorama by Tessalith showed north, south, east and west where they belong, with no mirror and no swap.
- **Turning:** `cubemap.yaw` turns the cube (in degrees, clockwise seen from above, so 90 moves east to south), and `cubemap.mirror` swaps east and west.
- **HDR:** an HDR sky with a bright sun needs `cubemap.highlight_clamp`, or the sun blows out the whole reflection.

### Mirror ball and angular map

![Two circles. Mirror ball: the centre shows south, behind the camera; the outer ring shows north, straight ahead; up is at the top, east on the right. Angular map: the centre shows north, straight ahead; the dashed half-radius ring is 90° away; the outer ring is south.](../img/cube-ball-angular.svg)

Both are pictures with the scene in a circle. Set the layout yourself, because a square image could be anything:
- `mirror_ball` is a photo of a chrome ball;
- `angular` is a light probe.

Both assume the camera stood south of the ball looking north, with east on the right of the picture. Use `cubemap.yaw` to turn the result. The circle's radius is half the picture's width, which sets the default face size. The point directly behind the ball is stretched, as in any ball photo.

### Single photo

![A wide strip: your photo in the middle half, mirrored copies on both sides, the top fading to the photo's top-row colour and the bottom fading to its bottom-row colour.](../img/cube-photo.svg)

One photo can't show what is behind you, so this mode makes a believable reflection rather than a true one. Set `cubemap.layout = "photo"`. Tessalith builds the fake panorama shown above (the photo in front, mirrored copies behind, and the top and bottom fading to the average colour of the photo's top and bottom rows), makes a cube from it and blurs it hard: `cubemap.blur = 0.6` unless you set another value, even when a style sets one. It works well for shiny metal and glass, where the reflection only needs the right colours and light. The default face size is half the photo's width. The photo's centre faces east; `cubemap.yaw = -90` turns it to face north. A portrait photo (taller than wide) loses its top and bottom: only its middle rows, as many as it is wide, are used, so crop it yourself to choose which. In the GUI, Review lists it as a warning to acknowledge before the export: "Only the middle 1024×1024 of photo.png is used".

### Face size

The face size defaults to the largest power of two that fits one source cell ([`cubemap.face_size`](Project-Config-Fields.md#cubemap)). A 1536×1152 cross has 384-pixel cells and becomes 256. For a panorama the cell is a quarter of the width, for a mirror ball or angular map half the width, and for a photo half the photo's width. Larger values scale up. Tessalith notes `DIM-RESIZED` when the face differs from the cell. A cube is never checked for `DIM-NPOT`.

### An existing cube DDS

A cube DDS goes through the same size, filter, blur and format settings as any other source. Like any DDS, it is skipped (`IN-DDS-SKIPPED`) unless you set `reencode = true` on the file. That costs some quality, because the faces are decoded and encoded again.

- It is always read as its six mip-0 faces. `cubemap.layout` must be unset (or `hstrip`) for it.
- It can only be re-encoded as a cubemap.
- Cube arrays are refused, and `sdc patch-header` still refuses cubes.

### Dynamic marker

![Six one-pixel faces. +X holds a colour with roughness in alpha; +Y and every other face is black.](../img/cube-marker.svg)

Community Shaders (and ENB) read a 1×1 cube whose +Y is black as an instruction to reflect the live scene, not a picture, this is a dynamic cubemap. 
For Community Shaders only, The +X pixel then sets the reflection's colour (RGB) and roughness (alpha). 
For ENB PG Patcher already applies a dynamic cubemap with all features ENB supports.

Cli write command:

```text
sdc cubemap-marker --color 200,200,210 --roughness 0.3 -o textures\cubemaps\steel_e.dds
```

- `--color` is written as raw code values. Community Shaders reads F0 as `(code/255)^1.6`.
- The colour sits on the +X face. +Y stays black, so the file is always a dynamic marker.
- `--roughness` is 0 to 1 and is stored in alpha as `min(round(r × 255), 254)`. Leave it out and alpha is 255, which gives Community Shaders' defaults (F0 1, roughness 1/7).

The file is 152 bytes. See [CLI reference](CLI-Reference.md#sdc-cubemap-marker). Tessalith never writes a picture cube at 1×1 by accident, so a real cube can't turn into a marker.
