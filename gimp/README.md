# Tessalith DDS for GIMP

A GIMP 3 plug-in that opens and exports Skyrim SE/AE DDS textures, using the same encoders and rules as Tessalith. The plug-in's Python files are GPL-3.0-or-later (see `COPYING`). `psdds.dll`, which holds the encoders, is MIT (see `LICENSE`) except the Tessalith engine code inside it (the `engine` and `encoders-sys` crates), which is under the Mozilla Public License 2.0; its source is at https://github.com/Alistra-za/Tessalith. Third-party licences are in `THIRD_PARTY_NOTICES.md`.

## Requirements
- Windows x64.
- GIMP 3.0.x (tested with 3.0.4).

## Install
1. Close GIMP.
2. Copy the `tessalith-dds` folder from the zip into `%APPDATA%\GIMP\3.0\plug-ins\`. Or add the folder that holds it under Edit > Preferences > Folders > Plug-ins.
3. Start GIMP. The folder must keep all its files: `tessalith-dds.py`, `psdds_ctypes.py`, `tdds_pixels.py` and `psdds.dll`.

To uninstall, delete the `tessalith-dds` folder. If the export entry is missing after an install, `psdds.dll` could not be loaded (the reason is printed on GIMP's console); fix the folder and copy it in again, because GIMP keeps its plug-in registration until the plug-in file changes.

## Export
Choose File > Export As with a name ending in `.dds`, or pick **Tessalith DDS** in Select File Type. "DDS image" is GIMP's own exporter and stays available in that list. The dialog opens after GIMP has handed over the image (a large image takes a moment), so it can show what the encoder will do with these pixels. The plug-in exports a merged copy of the visible layers at the canvas size and never changes your image. It has:
- **Preset**: Custom, or one of five built-in presets: Colour BC7, Colour BC1 (small, opaque), Normal BC7, Mask/data BC7 and Greyscale mask BC4. Picking one fills in every option below. The list shows Custom when the options match no preset. There are no user presets in this version.
- **Texture type**: Colour, Normal or Linear mask. It sets the colour handling and mip rules, as in Tessalith's presets, and which formats are offered.
- **sRGB** (checkbox beside the type): tags the file as sRGB. It changes only the tag, not the pixels or the mips. Off by default, because Skyrim's non-PBR textures are not sRGB-tagged. It is greyed for Normal and Linear mask and for the HDR formats.
- **Format**: Colour offers BC1, BC3, BC7 and RGBA8 (uncompressed); Normal adds BC5; Linear mask also offers BC4. 16-bit and 32-bit images also offer **BC6H (HDR)** and **RGBA16F (uncompressed)** under Colour. An 8-bit image does not list them. These two formats are for Community Shaders only.
- **Discard transparency** (checkbox under sRGB): saves the texture opaque whatever the pixels hold. Without it, transparency is kept when some pixel is actually transparent (alpha below 254 of 255) and the format stores alpha (BC3, BC7, RGBA8, RGBA16F). It is ticked and greyed for BC1, BC4, BC5 and BC6H, which store no alpha.
- **Alpha is transparency** (checkbox under Discard transparency, off by default): by default the alpha channel is treated as data, as in a texture set's masks, so the colour under fully transparent pixels is saved exactly and mips are plain. Tick it when alpha is real transparency: the colour under transparent pixels is then filled in from its neighbours and the mips are alpha-weighted. It is greyed unless the transparency is kept (the image has transparent pixels, Discard transparency is off and the format stores alpha).
- **Mipmaps** (checkbox) and **Filter** (Box, Tent, Mitchell, Kaiser): a full mip chain or none. The filter is greyed when Mipmaps is off, and where the encoder would use Box anyway.
- **Quality**: the encoder effort for the chosen format. Greyed for RGBA8.

**Warning: File > Overwrite (or Export to) on a DDS you opened reuses your last export settings without asking; use File > Export As to choose the format.** A normal map you opened can therefore be written with the settings of the last texture you exported. GIMP's Overwrite and Export to never show the dialog.

### 16-bit and 32-bit images
The image's precision sets the bit depth: 8-bit images are encoded as 8-bit, 16-bit integer images as 16-bit, and half, float and double images as 32-bit. BC6H and RGBA16F are offered only for 16-bit and 32-bit images. Float images are read as linear light, so values above 1 are kept in BC6H and RGBA16F (for the other formats they clip). Normal and Linear mask images are read in the image's own encoding, with no colour conversion, because they hold data and not light. An 8-bit linear Normal or Linear mask image therefore saves different bytes than the same pixels would if read as sRGB.

Images up to 16384 x 16384 are accepted; Skyrim supports no more.

## Open
File > Open on a `.dds` gives an 8-bit RGB image (the top mip only), with an alpha channel when the file has any (alpha of 254 or 255 everywhere counts as opaque, because Tessalith's own BC7 encoder may store opaque pixels as 254). Tessalith is first in line for `.dds` files. HDR files (BC6H, RGBA16F, RGBA32F), cube maps and texture arrays are handed to GIMP's own DDS plug-in, so they open as before; this plug-in does not open HDR files itself.

## Memory
The image is held as float RGBA, 16 bytes a pixel: about 4 GiB at 16384 x 16384, plus the encoder's own copy and a working copy of the image, all while the dialog is open. Close other programs or export at 8192 x 8192 or smaller if that fails with a "not enough memory" message.

## Support
Tessalith: https://github.com/Alistra-za/Tessalith
