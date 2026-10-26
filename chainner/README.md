# Tessalith DDS nodes for chaiNNer

Three nodes, in the **Image > Tessalith DDS** group, that save Skyrim SE/AE DDS textures with Tessalith's own encoder. They use the same settings rules and write the same bytes as Tessalith. All rules live in `psdds.dll`; the Python here only adapts chaiNNer's images to it.

## Install

1. Build the DLL if you have not (`crates\psdds\build.ps1`). `install.ps1` also generates the licence notices with `crates\psdds\notices.ps1`, which needs `cargo-about` (`cargo install cargo-about --locked`).
2. Close chaiNNer, then run `chainner\install.ps1`. It first stops any chaiNNer Python backend left running after chaiNNer closed (it holds `psdds.dll` open), then copies the nodes, `psdds.dll`, `COPYING`, `LICENSE` and `THIRD_PARTY_NOTICES.md` into the newest `%LOCALAPPDATA%\chaiNNer\app-*` folder and prints the psdds version.
3. `-Target <resources\src folder>` installs into a given folder instead; `-Uninstall` removes only the `tessalith_dds` folder.

**Re-run after every chaiNNer update.** An update installs a new `app-<version>` folder and deletes the old one, taking the nodes with it.

## Nodes

- **Save DDS (Tessalith)**: image, folder, subdirectory, image name, then the settings. *Preset* is Custom (every setting shown) or one of Tessalith's seed presets (the settings are hidden and the preset's values are used). Custom shows texture type, sRGB, discard transparency, mipmaps, mip filter, and a format with its quality list. Inputs that do not apply to the chosen type or format are hidden: *Discard transparency* is hidden for BC1, BC4, BC5 and BC6H, which store no alpha. *Alpha is transparency* (off by default) is shown only where alpha can be kept (BC3, BC7, RGBA8, RGBA16F with *Discard transparency* off). Off: alpha is data (a mask), and the colour under fully transparent pixels is saved exactly. On: alpha is see-through, so that colour is replaced by the nearest visible colour and the mips are alpha-weighted, as before this option existed. It only matters when the image has transparent pixels. **Colour** also offers **BC6H** and **RGBA16F**; Normal and Linear mask do not.
  - **Linear HDR input** (off by default) says what the pixels mean, because chaiNNer images are float32 and do not say. *Off*: sRGB-coded 0..1 (a PNG); values are clamped to 0..1, and BC6H and RGBA16F decode the stored sRGB coding to linear (a flat 0.5 reads back as 0.2140). Every other format writes the same bytes as before the switch existed. *On*: linear light (an EXR); values above 1 are kept for BC6H and RGBA16F, and 8-bit formats clip and convert. The rules come from psdds at depth 16 (off) or 32 (on). Alpha is taken from the pixels: an image without used alpha is saved opaque. Existing files are overwritten unless *Skip existing files* is on. A file is written only after the encode succeeded.
- **RMAOS (Tessalith)**: builds a TruePBR `_rmaos` from a roughness image (required) and up to three more: metallic, AO, specular. Each image input has a *channel* dropdown (R, G, B, A; hidden for a grey image), so one `_orm` image can feed three roles. An unconnected metallic, AO or specular role shows a *value* (0-255), preset to the engine's neutral default: metallic 0, AO 255, specular 255. Sizes must match; nothing is resized. Quality and Mipmaps are set here. The layout is in the wiki's Texture-Types page.
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

## Check after installing

Tests cannot see chaiNNer's window. After `install.ps1` (run by you, with chaiNNer closed), open chaiNNer and check:

1. The three nodes appear under **Image > Tessalith DDS** and the console shows no load error.
2. Wire RMAOS to Save Normal's RMAOS input: it connects (the type matches), and *Discard transparency* on Save Normal disappears. Disconnect and it returns.
3. On RMAOS, Roughness must be connected. A role with a 3- or 4-channel image shows its channel dropdown and hides its value; metallic, AO or specular with no image shows the value (0, 255, 255) and no dropdown.
4. The RMAOS node icon renders.
5. A paired save writes `<name>_n.dds` and `<name>_rmaos.dds`; the log says whether the roughness bake ran and lists any defaulted channels.
6. Save DDS with a seed preset hides the settings; Custom shows them again with their values.
7. After a chaiNNer update, re-run `install.ps1` and repeat step 1.

For developers: `TESSALITH_PSDDS_DLL=<path>` makes the nodes load that DLL instead of the one beside them (developer use only; unset it for normal use; `install.ps1` ignores it when checking the staged copy). `SDC_CHAINNER_SRC=<resources\src>` enables `chainner\tests\test_load_nodes.py`, which loads all nodes with `CHECK_LEVEL=error` in a scratch copy (the install is only read):
`& "$env:APPDATA\chaiNNer\python\python\python.exe" -I -m unittest discover -s chainner\tests -v`
## Licences

The nodes are GPL-3.0-or-later (`COPYING`; chaiNNer is GPLv3). `psdds.dll` is MIT (`LICENSE`); the engine and encoders-sys code inside it are MPL-2.0. Third-party notices are in `THIRD_PARTY_NOTICES.md` (generated at install; it names the source repository and commit).
