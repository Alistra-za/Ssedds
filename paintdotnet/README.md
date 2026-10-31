# Tessalith DDS for Paint.NET

A Paint.NET file type plugin that saves Skyrim SE/AE DDS textures, using the same encoders and rules as Tessalith. The plugin is GPL-3.0-or-later with an additional permission for Paint.NET (see `COPYING`). `psdds.dll`, which holds the encoders, is MIT (`LICENSE`), except Tessalith's engine code inside it (the `engine` and `encoders-sys` crates), which is under the Mozilla Public License 2.0; its source is at https://github.com/Alistra-za/Tessalith. Third-party licences are in `THIRD_PARTY_NOTICES.md`. Not made by or endorsed by dotPDN LLC.

## Requirements
- Windows x64 (ARM64 is not supported).
- Paint.NET 5.1 or later.

## Install
1. Close Paint.NET.
2. Unblock the zip first (right-click it, Properties, Unblock), because the plugin is unsigned.
3. Unzip so that the `TessalithDDS` folder sits in `Documents\paint.net App Files\FileTypes\` (no administrator rights needed) or in `C:\Program Files\paint.net\FileTypes\` (needs administrator rights).
4. Start Paint.NET.

To uninstall, delete the `TessalithDDS` folder.

## Save
Choose File > Save As and pick **Tessalith DDS** in the type list. Paint.NET flattens the layers first (it asks). The options dialog opens on every Save, Save As and Ctrl+S, preselected with that image's last choices (or your last choices for a new image). The preview pane shows the encoded result and the file size. Each change re-encodes the image, so slow BC7 settings make the dialog wait.

The options sit in one column, top to bottom, each label above its list:

- **Preset**: fills in all the options below in one go. See Presets.
- **Texture type**: Colour, Normal map or Linear mask. It sets the colour handling and mip rules, as in Tessalith's presets, and which formats are offered.
- **sRGB** (checkbox under the Texture type list): tags the file as sRGB; it changes only the tag, not the pixels or the mips. Off by default, because Skyrim's non-PBR textures are not sRGB-tagged. It is greyed and unticked for Normal and Linear mask.
- **Format**: the list depends on the texture type. Colour offers BC1, BC3, BC7 and uncompressed RGBA8; Normal map adds BC5; Linear mask also offers BC4. A format the type does not offer is replaced by BC7.
- **Discard transparency** (checkbox under the Format list): saves an opaque file whatever the pixels hold. It is ticked and greyed for BC1, BC4 and BC5, which store no alpha.
- **Alpha is transparency** (checkbox, under Discard transparency): off by default, so alpha is saved as data (a mask) and the colour under fully transparent pixels is kept exactly. Tick it for cut-out or see-through textures: the colour under transparent pixels is then filled from its neighbours and the mips are weighted by alpha, and the filter shows Box. It is greyed where no alpha is kept.
- **Mipmaps** (checkbox): a full mip chain, or none when unticked.
- **Filter**: how mips are shrunk. It is greyed when Mipmaps is unticked, and greyed showing Box where the encoder would use Box anyway (alpha kept with Alpha is transparency ticked).
- **Quality**: the encoder effort for the chosen format. Greyed for RGBA8.

Greyed controls show the value the encoder will use and remember your own pick. Until the first preview has measured the image, the dialog treats it as opaque. The name prompt and message boxes stay light under Paint.NET's dark theme.

## Alpha
Alpha is kept when some pixel's alpha is below 254 of 255, as in the Photoshop plugin, and the format stores alpha (BC3, BC7, RGBA8). Alpha of 254 or 255 counts as opaque: when the image's alpha is only 254 and 255 and Discard transparency is off, a line under the options says `alpha 254–255 only: written opaque` once the preview has measured the image (the line goes when Discard transparency is ticked). Tick **Discard transparency** to save an opaque file whatever the pixels hold.

## If psdds.dll is missing
If `psdds.dll` is missing from the plugin folder, is not the 64-bit build, or is the wrong version, the options dialog opens with its options greyed and Paint.NET's error box shows one message with the DLL's path and the ABI version the plugin needs. Cancel the dialog. Reinstall the plugin, then restart Paint.NET: it keeps a failed load until it restarts.

## Presets
The Preset list starts with Custom and five presets Tessalith provides. **Save preset...** stores the current choices under a name (up to 64 characters, where an emoji or other symbol outside the basic range counts as two; an existing name asks before replacing), and **Delete** removes the shown preset. Choosing Custom in the list keeps your settings and shows Custom until you pick or save a preset. Presets are kept in `%APPDATA%\Tessalith DDS\presets\paintdotnet.txt`, separate from the Photoshop plugin's. Deleting that file brings the five back.

## Open
Paint.NET always opens `.dds` files with its own DDS support, also for files Tessalith wrote, even if Tessalith DDS is picked in the Open dialog's type list. To re-save such a file with Tessalith, use Save As once and pick Tessalith DDS; after that Ctrl+S uses it.

## Limits and memory
Images up to the size the error message names are accepted. Saving the largest size needs about 10 GB of free RAM: the image is held as 16-byte float pixels, the encoder takes its own copy, plus its working buffers and Paint.NET's flattened image. This is the same figure as in the Photoshop guide. If memory runs short, save a smaller image.

## Support
Tessalith: https://github.com/Alistra-za/Tessalith
