# Host plugins

Tessalith has optional plugins for four image programs. They save Skyrim SE/AE DDS textures with Tessalith's own encoders and rules, so a texture saved from a plugin gets the same bytes as the same choices in Tessalith. All of them use one shared library, `psdds.dll` (Photoshop links it in), so the rules for each option exist in one place.

The plugins save one image at a time. They do not read `sdc.toml`, sets or a mod scan. For whole folders, packing and TruePBR JSON, use Tessalith itself.

| Program | Version | Release zip | Saves | Opens DDS |
|---|---|---|---|---|
| Adobe Photoshop | 2020 or later | `TessalithDDS-<version>.zip` | Yes, 8, 16 and 32-bit | Yes, also HDR files |
| GIMP | 3.0.x (tested with 3.0.4) | `TessalithDDS-GIMP-<version>.zip` | Yes, 8, 16 and 32-bit | Yes, 8-bit only; HDR goes to GIMP's own DDS loader |
| Paint.NET | 5.1 or later | `TessalithDDS-PaintDotNet-<version>.zip` | Yes, 8-bit | No; Paint.NET uses its own reader |
| chaiNNer | a recent release | `TessalithDDS-chaiNNer-<version>.zip` | Yes, three nodes | No; chaiNNer's Load Image already reads DDS |

All of them are Windows x64 only. They are not code-signed, so Windows may block the zip or warn when you run it. Right-click the zip, choose **Properties**, tick **Unblock**, then unzip.

Each zip holds a README with the full guide. The same guides are in the repository: [Photoshop](../../photoshop/README.md), [GIMP](../../gimp/README.md), [Paint.NET](../../paintdotnet/README.md), [chaiNNer](../../chainner/README.md).

## Install

### Photoshop

1. Close Photoshop.
2. Copy `TessalithDDS.8bi` into Photoshop's `Plug-ins` folder, for example `C:\Program Files\Adobe\Adobe Photoshop 2025\Plug-ins\`. This needs administrator rights.
3. Start Photoshop. **Tessalith DDS** is now in the File > Save As and Save a Copy format lists, and File > Open reads `.dds` files.

To uninstall, delete the `.8bi`.

### GIMP

1. Close GIMP.
2. Copy the `tessalith-dds` folder from the zip into `%APPDATA%\GIMP\3.0\plug-ins\`. Or add the folder that holds it under Edit > Preferences > Folders > Plug-ins.
3. Start GIMP. Keep every file in the folder: `tessalith-dds.py`, `psdds_ctypes.py`, `tdds_pixels.py` and `psdds.dll`.

If **Tessalith DDS** is missing from the export types, `psdds.dll` did not load; GIMP's console says why. Fix the folder and copy it in again. GIMP remembers a plug-in until its `.py` file changes, so recopy the whole folder, not only the DLL. To uninstall, delete the `tessalith-dds` folder.

### Paint.NET

1. Close Paint.NET.
2. Unblock the zip, then unzip it so that the `TessalithDDS` folder sits in `Documents\paint.net App Files\FileTypes\`. That needs no administrator rights. `C:\Program Files\paint.net\FileTypes\` works too, with administrator rights.
3. Start Paint.NET.

To uninstall, delete the `TessalithDDS` folder.

### chaiNNer

1. Unzip the zip anywhere, keeping its folders. Unblock it first if Windows blocked it.
2. Close chaiNNer.
3. In the unzipped folder, run `powershell -ExecutionPolicy Bypass -File install.ps1`. It copies the nodes and `psdds.dll` into chaiNNer's newest app folder under `%LOCALAPPDATA%\chaiNNer` and prints the version. Nothing is built.
4. Start chaiNNer. The nodes are under **Image > Tessalith DDS**.

**Run `install.ps1` again after every chaiNNer update.** An update puts chaiNNer in a new folder and deletes the old one, nodes included. Nothing can warn you (chaiNNer loads nodes only from its own app folder): the **Tessalith DDS** group just disappears, and that is the sign to run it again. `install.ps1 -Uninstall` removes the nodes.

## The save options

Photoshop, GIMP and Paint.NET show the same options in a dialog when you save. The chaiNNer **Save DDS** node has the same inputs. Greyed options show the value that will be used.

| Option | What it does |
|---|---|
| **Preset** | Fills in every option below in one go. Five come with the plugin: Colour BC7, Colour BC1 (small, opaque), Normal BC7, Mask/data BC7 and Greyscale mask BC4. The list shows Custom when your options match no preset. |
| **Texture type** | Colour, Normal map or Linear mask. It sets the colour handling, the mip rules and which formats are offered, as in Tessalith's presets. |
| **sRGB** | Tags the file as sRGB. It changes only the tag, not the pixels or the mips. Off by default, because Skyrim's non-PBR textures are not sRGB-tagged. Greyed for Normal map, Linear mask, BC6H and RGBA16F. |
| **Format** | Colour: BC1, BC3, BC7 or RGBA8 (uncompressed). Normal map adds BC5. Linear mask adds BC4. For 16 and 32-bit images, Colour also offers BC6H and RGBA16F (see [HDR](#16-bit-32-bit-and-hdr)). |
| **Discard transparency** | Saves the texture opaque, whatever the pixels hold. Always on for BC1, BC4, BC5 and BC6H, which store no alpha. |
| **Alpha is transparency** | Off by default. See [Alpha](#alpha-data-or-transparency). |
| **Mipmaps** and **Filter** | A full mip chain, or none. The filter (Box, Tent, Mitchell, Kaiser) sets how mips are shrunk. It is greyed, showing Box, where the encoder uses Box anyway. |
| **Quality** | How hard the encoder works for the chosen format. Greyed for RGBA8. |

Your last choices are remembered for the next save. Photoshop and GIMP accept images up to 16384 x 16384, the most Skyrim supports; Paint.NET names its limit in the error message. Large images need a lot of free memory: saving at the largest size takes about 10 GB in Photoshop and Paint.NET. Close other programs or save at 8192 x 8192 if it fails.

User presets: Photoshop and Paint.NET can save and delete your own presets (**Save preset...**, **Delete**). They keep them in separate files under `%APPDATA%\Tessalith DDS\presets\`. GIMP has only the five built-in presets. chaiNNer has Custom and the built-in presets.

## Alpha: data or transparency

Alpha is kept only when some pixel is really transparent (alpha below 254 of 255), **Discard transparency** is off, and the format stores alpha (BC3, BC7, RGBA8, RGBA16F). Alpha of 254 or 255 counts as opaque. When an image's alpha is only 254 and 255 and Discard transparency is off, each plugin says so once per save: `alpha 254–255 only: written opaque`. Photoshop and Paint.NET show it as a line in the save dialog, GIMP in the status bar, and chaiNNer as a warning in its log.

When alpha is kept, **Alpha is transparency** says what it means:

- **Off (the default): alpha is data.** This is right for most Skyrim texture sets, where alpha is a mask, specular or height. The colour under fully transparent pixels is saved exactly as painted, and the mips use the filter you picked.
- **On: alpha is see-through.** Use it for cut-out or glass-like textures. The colour under fully transparent pixels is replaced by the nearest visible colour, so it does not bleed into the mips, and the mips are alpha-weighted with Box.

The option is greyed (Photoshop, GIMP, Paint.NET) or hidden (chaiNNer) when no alpha is kept. In Photoshop, presets saved by older plugin versions load with it off. Before this option existed, the plugins always treated alpha as transparency.

## 16-bit, 32-bit and HDR

**BC6H** (HDR) and **RGBA16F** (uncompressed half float) are for Community Shaders only. BC6H stores no alpha; RGBA16F keeps it. Their mips always use Box.

- **Photoshop.** 8, 16 and 32-bit RGB documents. A 16-bit document is encoded at full depth. BC6H and RGBA16F are offered for 16 and 32-bit Colour documents. For 32-bit documents, Colour is read as linear light (values above 1.0 are kept in BC6H and RGBA16F), while Normal map and Linear mask are raw data. Open reads BC6H, RGBA16F and RGBA32F files as 32-bit documents.
- **GIMP.** The image's precision sets the depth: 8-bit, 16-bit integer, or 32-bit for half, float and double images. BC6H and RGBA16F are offered for 16 and 32-bit Colour images. Float images are read as linear light. GIMP's own DDS plug-in opens HDR files, cube maps and texture arrays.
- **chaiNNer.** Colour offers BC6H and RGBA16F. chaiNNer images do not say what their pixels mean, so the **Linear HDR input** switch does. Off (default): the pixels are sRGB 0 to 1, like a PNG. On: they are linear light, like an EXR, and values above 1 are kept for BC6H and RGBA16F.
- **Paint.NET.** 8-bit only. It does not offer BC6H or RGBA16F.

## chaiNNer nodes

- **Save DDS (Tessalith)** saves one image with the options above.
- **RMAOS (Tessalith)** builds a TruePBR `_rmaos` from a roughness image and, if you like, metallic, AO and specular images. Each input has a channel picker, so one `_orm` image can feed three roles. A role with no image uses a value (metallic 0, AO 255, specular 255).
- **Save Normal (Tessalith)** saves a normal map as `<name>_n.dds`. Connect RMAOS to its RMAOS input to write `<name>_n.dds` and `<name>_rmaos.dds` together. **RMAOS on its own writes nothing**, because chaiNNer drops a node whose output goes nowhere.

The `_rmaos` layout is on the [Texture types](Texture-Types.md) page.

## Things to know

- **Layers.** Photoshop saves the merged image of the visible layers. GIMP saves a merged copy of the visible layers and never changes your image. Paint.NET flattens the image first (it asks). Keep your layered file.
- **Photoshop alpha.** The first alpha channel wins over layer transparency. Extra alpha channels and spot channels are not saved; the save dialog then says `Only the first alpha channel is written`. A layered document with no alpha channel and no transparency saves opaque, and the dialog says `No alpha channel: the DDS is written opaque`.
- **GIMP Overwrite.** The first File > Overwrite or Export to on a DDS you opened in this session shows the dialog, preset with the file's own format, sRGB tag and mipmaps (BC5 opens as Normal map, BC4 as Linear mask). After you confirm it once, later Overwrites of that image reuse those settings without asking.
- **A missing or wrong `psdds.dll`.** GIMP and Paint.NET show one message with the DLL's path and the ABI version the plugin needs: GIMP when you open or export, Paint.NET in its error box under a greyed options dialog. Reinstall the plugin. Paint.NET keeps a failed load until it restarts, so restart it after replacing the DLL.
- **Paint.NET Open.** Paint.NET always opens `.dds` with its own reader. To save with Tessalith again, use Save As once and pick **Tessalith DDS**; after that Ctrl+S uses it.
- **Opened files are 8-bit.** Photoshop and GIMP open the top mip only, as an 8-bit image, except Photoshop's HDR case above.

## Licences

| Part | Licence |
|---|---|
| Photoshop plugin | MIT, except the Tessalith engine code inside it |
| GIMP plug-in (Python files) | GPL-3.0-or-later |
| Paint.NET plugin | GPL-3.0-or-later, with an extra permission for Paint.NET |
| chaiNNer nodes | GPL-3.0-or-later |
| `psdds.dll` (GIMP, Paint.NET, chaiNNer) | MIT, except the Tessalith engine code inside it |
| Tessalith engine code (`engine` and `encoders-sys`) | MPL-2.0 |

Each zip has the licence texts and `THIRD_PARTY_NOTICES.md` for the encoder libraries. The Photoshop and Paint.NET plugins are not made by or endorsed by Adobe or dotPDN LLC.
