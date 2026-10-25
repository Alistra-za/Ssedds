# Tessalith DDS nodes for chaiNNer

Three nodes, in the **Image > Tessalith DDS** group, that save Skyrim SE/AE DDS textures with Tessalith's own encoder. They use the same settings rules and write the same bytes as Tessalith. All rules live in `psdds.dll`; the Python here only adapts chaiNNer's images to it.

## Install

1. Build the DLL if you have not (`crates\psdds\build.ps1`).
2. Close chaiNNer, then run `chainner\install.ps1`. It copies the nodes, `psdds.dll` and the notices into the newest `%LOCALAPPDATA%\chaiNNer\app-*` folder and prints the psdds version.
3. `-Target <resources\src folder>` installs into a given folder instead; `-Uninstall` removes only the `tessalith_dds` folder.

**Re-run after every chaiNNer update.** An update installs a new `app-<version>` folder and deletes the old one, taking the nodes with it.

## Nodes

- **Save DDS (Tessalith)**: image, folder, subdirectory, image name, then the settings. *Preset* is Custom (every setting shown) or one of Tessalith's seed presets (the settings are hidden and the preset's values are used). Custom shows texture type, sRGB, discard transparency, mipmaps, mip filter, and a format with its quality list. Inputs that do not apply to the chosen type or format are hidden. Alpha is taken from the pixels: an image without used alpha is saved opaque. Existing files are overwritten unless *Skip existing files* is on. A file is written only after the encode succeeded.
- **RMAOS (Tessalith)**: builds a TruePBR `_rmaos` from up to four images: roughness, metallic, AO, specular. Each image input has a *channel* dropdown (R, G, B, A; hidden for a grey image), so one `_orm` image can feed three roles. An unconnected role gets a *fill* (Default, or Custom 0-255). Sizes must match; nothing is resized. Quality and Mipmaps are set here. The layout and the defaults are in the wiki's Texture-Types page. Specular alone builds nothing.
- **Save Normal (Tessalith)**: normal image, folder, subdirectory, base name, format, mipmaps, quality, skip existing, and an optional *RMAOS* input.

### RMAOS to Save Normal

RMAOS has no output file of its own. Connect its output to Save Normal's RMAOS input:

| Wiring | Written |
|---|---|
| Save Normal alone | `<name>_n.dds`, the same bytes as Save DDS with type Normal. |
| RMAOS connected to Save Normal | `<name>_n.dds` and `<name>_rmaos.dds` together; the `_n` uses the TruePBR normal profile (so it differs from the lone Save Normal), and the roughness bake runs when the engine's size rule allows. |
| RMAOS alone | **Nothing.** chaiNNer removes a node with no consumer. Connect it to Save Normal. |

With *Skip existing files*, the pair is skipped only when both files exist; otherwise both are written so they stay consistent. Discard transparency appears on Save Normal only while no RMAOS is connected.

### Limits

Size limits and other refusals come from psdds as one plain sentence in the node's error. Not reproduced from a project export: generated AO, and glossiness or `_orm`/fuzz roles (invert glossiness with chaiNNer's own nodes). **Load DDS is not in v1**: chaiNNer's Load Image already reads DDS.

## Licences

- The nodes (`tessalith_dds\*.py`) are **GPL-3.0-or-later** (chaiNNer is GPLv3): `LICENSE`.
- `psdds.dll`: the `psdds` crate is **MIT** (`LICENSE-psdds.txt`); the engine and encoders-sys code inside it are **MPL-2.0**. `THIRD_PARTY_NOTICES.md` (generated at install) lists every component and names the source repository and commit.
