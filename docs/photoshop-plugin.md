# Tessalith DDS plugin for Photoshop

`TessalithDDS.8bi` is a Photoshop file-format plugin (Windows x64, Photoshop 2020+) that opens and saves Skyrim DDS textures with Tessalith's encoders and rules. Its user guide is `photoshop/README.md`; the rulings are Note DC in [DECISIONS.md](DECISIONS.md).

## Layout
- `crates/psdds`: a Rust static library with a C ABI (`photoshop/psdds.h`): settings rules (greying, the formats each texture type offers via `psdds_format_allowed`), encode, decode. The dialog asks psdds for every rule, so they exist once. It links only `engine` and `encoders-sys`.
- `photoshop/`: the C++ plugin (PiPL, Save dialog with presets, Open, row-band transfer), `build.ps1`, `package.ps1`. Save presets are a UTF-8 file, `%APPDATA%\Tessalith DDS\presets\presets.txt`, seeded from psdds's `psdds_preset_*` list on first use (Note DC, DC35 and DC37); tests point `TESSALITH_DDS_PRESETS_DIR` at a temp folder.

## Build
The Photoshop SDK is not in the repository. Set `PS_SDK` to the folder that holds `pluginsdk\`, or put it in `tools\adobe photoshop sdk 2026` of the main checkout.
```
.\photoshop\build.ps1            # Release .8bi in photoshop\build\Release\
.\photoshop\build.ps1 -Test      # console tests only (no SDK needed)
.\photoshop\package.ps1          # dist\TessalithDDS-<version>.zip + .sha256.txt
```
`package.ps1` needs `cargo-about` on PATH (`cargo install cargo-about --locked`, once per machine). It fails if cargo-about is missing or cannot run, if a dependency has a licence outside `photoshop/about.toml`, or if the plugin links a VC++ runtime DLL. Public CI cannot build the plugin; releases are built locally.

## Install
Copy `TessalithDDS.8bi` into Photoshop's `Plug-ins` folder (administrator rights needed). The plugin is unsigned, so SmartScreen may warn.

## Licence split
Tessalith is GPL-3.0-or-later. Adobe's SDK licence forbids linking the SDK with GPL-licensed code, so nothing in the plugin is under the GPL: `crates/psdds` and `photoshop/` are MIT, and Tessalith's own code in `crates/engine` and `crates/encoders-sys` is `MPL-2.0` (owner grant, Note DC; the vendored encoders keep their own licences). The plugin never links GPL, LGPL or AGPL code (modscan, nif, gui, ...). Copyright holder: Alistraza.
