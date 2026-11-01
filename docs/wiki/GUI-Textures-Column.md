# Textures column

The left column of the Textures view: one tile per source file, its filters, sort and tile size. For anyone looking for a texture in a large folder.

## Textures (left)

One tile per file, with a thumbnail, its name and badges. An amber border means the texture still needs an answer ([Deciding](GUI-Texture-Panel.md#deciding)). Click a tile to select it, double-click it for the preview.

Helper files (raw AO, metallic, roughness, specular, opacity and other masks, RNM layers) are not tiles. They are rows in the **Helpers** group under the tiles; see [Helper textures](GUI-Helpers-RNM-and-Cubemaps.md#helper-textures). A cube made of six face files is one item, named `<output name> (6 faces)` ([Cubemaps](GUI-Helpers-RNM-and-Cubemaps.md#cubemaps)).

The **◀** on the column heading collapses the column to a thin strip; **▶** expands it.

## Filters

Above the search box is a row of pills: *Undecided*, *Warnings*, *Failed* and *Not in a set*, each with the number of textures it would show. Hover a pill for what it does. The pill row holds only filters; the Helpers group is not one.

After the pills comes **Filter ▾**. It opens a popup with five combos, and stays open while you use it:

| Combo | Values |
|---|---|
| *Sub-type* | *Any*, then the sub-types in the open folder. |
| *Output format* | *Any*, then the formats in the open folder. |
| *Encoder* | *Any*, bc7e, rgbcx, ispc for BC6H, NVTT, uncompressed (only those in use). |
| *Set membership* | *Any*, *In a set*, *Not in a set*. |
| *Sort* | See [Sort](#sort). |

The button reads **Filter (k) ▾** while k of the four filter combos are set. Sort is not a filter and is not counted.

All filters and the search box combine: a texture shows only if it passes every one.

When the filters hide every texture, the column says "No textures match the filters (N hidden)" and offers **Clear filters**. It resets the pills, the combos and the search, and keeps the sort and the tile size. A multi-selection keeps the textures a filter hides, and the heading says how many ("4 hidden by filter").

## Sort

The *Sort* combo orders the tiles:

- *Name* (the default): folder, then file.
- *Source size*: largest first.
- *Status*: errors, then warnings, then undecided, then the rest.

Ties go by name. The arrow keys, Shift+click ranges and Ctrl+A follow the order and the filters shown. Helper rows ignore the sort: they are always ordered by set, then name.

## Tile size

The **−** and **+** buttons after **Filter ▾** (hover *Smaller tiles*, *Larger tiles*) step the tiles by 16 points:

- from 64 points up to the thumbnail's own size, which is 128 points at 100 % scaling (fewer on a scaled screen, never below 96);
- 96 points to start;
- **+** is greyed out at the top.

A saved size above the top loads as it is. A saved size off the 16-point steps loads as it is and steps from there. Up and Down move by the number of tiles in a row, so they follow the size.

The filters, the sort and the tile size are remembered in `gui.toml` with the window layout ([GUI settings](GUI-Settings.md#where-settings-live)).

## Selecting

Click selects one tile. Ctrl+click, Shift+click and Ctrl+A select several; see [Bulk editing](GUI-Bulk-and-Rename.md#selecting-several).
