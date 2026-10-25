# Tessalith DDS for Adobe Photoshop

A Photoshop file-format plugin that opens and saves Skyrim SE/AE DDS textures, using the same encoders and rules as Tessalith. MIT licence (see `LICENSE`); third-party licences are in `THIRD_PARTY_NOTICES.md`. Not made by or endorsed by Adobe.

## Requirements
- Windows x64.
- Adobe Photoshop 2020 or later.

## Install
1. Close Photoshop.
2. Copy `TessalithDDS.8bi` into Photoshop's `Plug-ins` folder, for example `C:\Program Files\Adobe\Adobe Photoshop 2025\Plug-ins\`. Writing there needs administrator rights.
3. Start Photoshop. **Tessalith DDS** appears in the File > Save a Copy format list, and Open recognises `.dds` files.

The plugin is unsigned, so Windows SmartScreen may warn when you download or unzip it. Choose "More info", then "Run anyway" (or unblock the zip in its file properties) before copying. To uninstall, delete the `.8bi`.

## Save
Choose File > Save a Copy > **Tessalith DDS**. Photoshop 2022 and later list DDS (like other formats that cannot keep layers) only under Save a Copy; to have it under Save As too, turn on Preferences > File Handling > "Enable legacy Save As". The plugin saves the merged image: all visible layers flattened, plus one alpha (the layer transparency, else the first alpha channel). Layers, extra alpha channels and spot channels are not kept, so keep a PSD of layered work. The options dialog has:
- **Format**: BC1, BC3, BC4, BC5, BC7 or uncompressed RGBA8.
- **Texture type**: Colour, Normal map or Linear mask. It sets the colour handling and mip rules, as in Tessalith's presets.
- **sRGB**: tags the file as sRGB. It is greyed where the format or type has no sRGB variant (BC4, BC5, Normal, Linear mask).
- **Mip filter**: how mips are shrunk. It is greyed, showing Box, where the encoder would use Box anyway (for example when alpha is kept).
- **Mipmaps**: a full chain or none.
- **Quality**: the encoder effort for the chosen format (BC7 effort, BC1/BC3 level, BC4/BC5 search). Greyed for RGBA8.

Your last choices are remembered and preselected the next time the dialog opens. The dialog opens on every Save, Save As and Ctrl+S; only actions and batches run with dialogs off skip it and use the remembered choices. Images up to 16384 x 16384 are accepted; Skyrim supports no more.

## Open
File > Open on a `.dds` gives an 8-bit RGB document (the top mip only), with transparency when the file has any (alpha of 254 or 255 everywhere counts as opaque, because Tessalith's own BC7 encoder may store opaque pixels as 254). HDR files (BC6H, float formats), cube maps and texture arrays are not opened by this plugin; it leaves them to any other DDS plugin you have installed.

## Memory
Large images need a lot of free RAM. Opening a 16384 x 16384 RGBA8 DDS needs about 3.4 GB beyond Photoshop's own use (the file, the decoded image and a copy of it exist together briefly), and saving one needs about 10 GB (the image is held as 16-byte float pixels, 4 GB, the encoder takes its own 4 GB copy, plus its working buffers and the encoded file). Saving very large images on a machine with little free RAM can still fail, with a "not enough memory" message or, in the worst case, a Photoshop crash. Close other programs or save at 8192 x 8192 or smaller if that happens.

## Support
Tessalith: https://github.com/Alistra-za/Tessalith
