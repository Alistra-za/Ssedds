# Tessalith DDS for Adobe Photoshop

A Photoshop file-format plugin that opens and saves Skyrim SE/AE DDS textures, using the same encoders and rules as Tessalith. MIT licence (see `LICENSE`), except the Tessalith engine code inside it (the `engine` and `encoders-sys` crates), which is under the Mozilla Public License 2.0; its source is at https://github.com/Alistra-za/Tessalith. Third-party licences are in `THIRD_PARTY_NOTICES.md`. Not made by or endorsed by Adobe.

## Requirements
- Windows x64.
- Adobe Photoshop 2020 or later.

## Install
1. Close Photoshop.
2. Copy `TessalithDDS.8bi` into Photoshop's `Plug-ins` folder, for example `C:\Program Files\Adobe\Adobe Photoshop 2025\Plug-ins\`. Writing there needs administrator rights.
3. Start Photoshop. **Tessalith DDS** appears in the File > Save As and File > Save a Copy format lists, and Open recognises `.dds` files.

The plugin is unsigned, so Windows SmartScreen may warn when you download or unzip it. Choose "More info", then "Run anyway" (or unblock the zip in its file properties) before copying. To uninstall, delete the `.8bi`.

## Save
Choose File > Save As (or Save a Copy) > **Tessalith DDS**. The plugin saves the merged image that Photoshop itself composites from the visible layers, plus one alpha: the first alpha channel if the document has one (spot channels never count), else the transparency of the layers (or the merged image), else none. A painted alpha channel therefore wins over layer transparency, with or without a Background layer. If Photoshop cannot hand over the merged image, the save stops with "could not read the image from Photoshop, so nothing was saved" and an existing file is left as it was. Alpha is kept only when some pixel is actually transparent (below 254 of 255), so a fully opaque layer saves as an opaque texture. Layers, extra alpha channels and spot channels are not kept, so keep a PSD of layered work. The options dialog opens once Photoshop has handed over the image (a large image takes a moment), so it can show what the encoder will do with these pixels. It has:
- **Preset**: fills in all the options below in one go. The first time the dialog opens, it creates five presets: Colour BC7, Colour BC1 (small, opaque), Normal BC7, Mask/data BC7 and Greyscale mask BC4. **Save preset...** stores the current options under a name (1 to 64 characters; saving over an existing name asks first), and **Delete** removes the preset shown. Every preset can be replaced or deleted; with none left the list just shows Custom. The list shows Custom when the options match no preset. Presets live in `%APPDATA%\Tessalith DDS\presets\presets.txt`.
- **Texture type**: Colour, Normal map or Linear mask. It sets the colour handling and mip rules, as in Tessalith's presets, and which formats are offered. For 32-bit documents it also decides what the values mean:

  | Type | 32-bit values are | Encoding |
  |---|---|---|
  | Normal map | raw data (Marmoset writes raw vectors) | passed unchanged (clamped 0 to 1) to BC5, BC7 and the others, as a 16-bit document would be |
  | Linear mask | raw data | unchanged (clamped 0 to 1) |
  | Colour, BC6H or RGBA16F | linear light | stored as is; values above 1.0 are kept |
  | Colour, BC1, BC3, BC7 or RGBA8 | linear light | converted linear to sRGB, then encoded; values above 1.0 clip |

  16-bit documents of Colour type saved as BC6H or RGBA16F are read as sRGB and converted to linear first. Other 8-bit and 16-bit saves are unchanged.
- **sRGB** (checkbox beside the type): tags the file as sRGB; it changes only the tag, not the pixels or the mips (Colour mips are always filtered gamma-correctly). Off by default, because Skyrim's non-PBR textures are not sRGB-tagged. It is greyed and unticked for Normal and Linear mask (BC4 and BC5 have no sRGB variant either), and for BC6H and RGBA16F.
- **Format**: Colour offers BC1, BC3, BC7 and uncompressed RGBA8; Normal map adds BC5; Linear mask also offers BC4 (one channel). Colour also offers **BC6H (HDR)** and **RGBA16F (uncompressed)** for 16-bit and 32-bit documents only; an 8-bit document does not list them. These two formats are for Community Shaders (CS) only. BC6H has four quality levels (Veryfast, Fast, Basic, Veryslow) and stores no alpha; RGBA16F has no quality and keeps alpha. Their mips are always filtered with Box. A format the type does not offer (for example a remembered BC4 after switching to Colour) is replaced by BC7.
- **Discard transparency** (checkbox, under sRGB): saves the texture opaque even when the image has transparent pixels. Without it, transparency is kept when some pixel is actually transparent and the format stores alpha (BC3, BC7, RGBA8, RGBA16F). It is ticked and greyed for BC1, BC4, BC5 and BC6H, which store no alpha, at every bit depth.
- **Alpha is transparency** (checkbox, under Discard transparency; off by default): only where alpha is kept (the image has transparent pixels, Discard transparency is off and the format stores alpha); greyed otherwise. *Off*: alpha is data, as in most Skyrim texture sets (masks, specular, parallax): the colour under fully transparent pixels is saved exactly as painted, and the mips are filtered with the Filter you pick. *On*: alpha is see-through transparency: the colour under fully transparent pixels is replaced by the nearest visible colour (so it does not bleed into the mips) and the mips are alpha-weighted with Box. Presets and the remembered settings keep this choice; presets saved by an older version load with it off.
- **Mipmaps** (checkbox): a full mip chain, or none when unticked.
- **Filter** (beside Mipmaps): how mips are shrunk. It is greyed when Mipmaps is unticked, and greyed showing Box where the encoder would use Box anyway (for example when alpha is kept and *Alpha is transparency* is ticked).
- **Quality**: the encoder effort for the chosen format (BC7 effort, BC1/BC3 level, BC4/BC5 search). Greyed for RGBA8.

Your last choices are remembered and preselected the next time the dialog opens. The dialog opens on every interactive save, and Cancel there writes nothing; only actions and batches run with dialogs off skip it and use the remembered choices. Images up to 16384 x 16384 are accepted; Skyrim supports no more.

Documents can be 8-bit, 16-bit or 32-bit RGB. A 16-bit document is encoded at full depth, never squeezed to 8-bit first. A 32-bit document is saved as Photoshop gives it, per the table above.

Known gap: a normal map converted from 16-bit to 32-bit inside Photoshop is linearised by Photoshop, and the plugin then treats its values as raw data. Convert it back to 16-bit before saving. A normal map that was created as 32-bit (for example a Marmoset bake) is fine.

## Open
File > Open on a `.dds` gives an 8-bit RGB document (the top mip only), with transparency when the file has any (alpha of 254 or 255 everywhere counts as opaque, because Tessalith's own BC7 encoder may store opaque pixels as 254). HDR files (BC6H UF16 and SF16, RGBA16F, RGBA32F) open as 32-bit documents, top mip only, with values above 1.0 kept and no colour profile attached (Photoshop's 32-bit mode is linear); RGBA16F and RGBA32F have transparency only when some alpha is below 254 of 255. There is no 16-bit Open: no supported DDS format stores integers wider than 8 bits. Cube maps and texture arrays are not opened by this plugin; it leaves them to any other DDS plugin you have installed. The existing size cap on Open still applies, so an RGBA16F file above about 11.6k x 11.6k, or an RGBA32F file above about 8k x 8k, is refused.

## Memory
Large images need a lot of free RAM. Opening a 16384 x 16384 RGBA8 DDS needs about 3.4 GB beyond Photoshop's own use (the file, the decoded image and a copy of it exist together briefly), and saving one needs about 10 GB, also for 16-bit and 32-bit documents (the image is held as 16-byte float pixels, 4 GB, the encoder takes its own 4 GB copy, plus its working buffers and the encoded file). Saving very large images on a machine with little free RAM can still fail, with a "not enough memory" message or, in the worst case, a Photoshop crash. Close other programs or save at 8192 x 8192 or smaller if that happens.

## Support
Tessalith: https://github.com/Alistra-za/Tessalith
