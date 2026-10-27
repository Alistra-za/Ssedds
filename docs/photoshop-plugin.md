# Tessalith DDS plugin for Photoshop

`TessalithDDS.8bi` is a Photoshop file-format plugin (Windows x64, Photoshop 2020+) that opens and saves Skyrim DDS textures with Tessalith's encoders and rules. Its user guide is `photoshop/README.md`; the rulings are Note DC (the plugin) and Note DF (16-bit, 32-bit and HDR: 32-bit RGB documents, BC6H and RGBA16F under Colour for 16/32-bit documents, Open of HDR files as 32-bit) in [DECISIONS.md](DECISIONS.md).

## Layout
- `crates/psdds`: a Rust static library (and, for other hosts, a DLL; see below) with a C ABI (`photoshop/psdds.h`): settings rules (greying, the formats each texture type offers via `psdds_format_allowed`), encode, decode. The dialog asks psdds for every rule, so they exist once. It links only `engine` and `encoders-sys`.
- `photoshop/`: the C++ plugin (PiPL, Save dialog with presets, Open, row-band transfer), `build.ps1`, `package.ps1`. Save presets are a UTF-8 file, `%APPDATA%\Tessalith DDS\presets\presets.txt`, seeded from psdds's `psdds_preset_*` list on first use (Note DC, DC35 and DC37); tests point `TESSALITH_DDS_PRESETS_DIR` at a temp folder.

## Build
The Photoshop SDK is not in the repository. Set `PS_SDK` to the folder that holds `pluginsdk\`, or put it in `tools\adobe photoshop sdk 2026` of the main checkout.
```
.\photoshop\build.ps1            # Release .8bi in photoshop\build\Release\
.\photoshop\build.ps1 -Test      # console tests only (no SDK needed)
.\photoshop\package.ps1          # dist\TessalithDDS-<version>.zip + .sha256.txt
```
`package.ps1` needs `cargo-about` on PATH (`cargo install cargo-about --locked`, once per machine). It fails if cargo-about is missing or cannot run, if a dependency has a licence outside `crates/psdds/about.toml`, or if the plugin links a VC++ runtime DLL. Public CI cannot build the plugin; releases are built locally.

## psdds.dll for other hosts
`crates/psdds` is also built as a DLL so other host plugins (Paint.NET, GIMP, chaiNNer) share the same rules. Build it with `.\crates\psdds\build.ps1` (add `-Test` to run the DLL host check); it writes `target\psdds\x86_64-pc-windows-msvc\release\psdds.dll` and fails if the DLL depends on a VC++ runtime or its exports differ from `photoshop/psdds.h`. The ABI is `PSDDS_ABI_VERSION` (4: the per-save "Alpha is transparency" choice, `alpha_mode` and `alpha_mode_enabled`, Note DH; 3 added BC6H, RGBA16F and the depth-aware exports `psdds_format_offered`, `psdds_resolve_depth`, `psdds_encode_depth`, `psdds_decode_depth`, `psdds_dds_openable_float`, Note DF): a host loads only a DLL whose `psdds_abi_version()` equals its own, and ships its own copy. Third-party notices for any psdds binary come from `crates\psdds\notices.ps1 -Binary <name> -Product <name> -Out <file>`. The rulings are Note DD in [DECISIONS.md](DECISIONS.md); the C ABI overview is `crates/psdds/README.md`.
## Install
Copy `TessalithDDS.8bi` into Photoshop's `Plug-ins` folder (administrator rights needed). The plugin is unsigned, so SmartScreen may warn.

## Licence split
Tessalith is GPL-3.0-or-later. Adobe's SDK licence forbids linking the SDK with GPL-licensed code, so nothing in the plugin is under the GPL: `crates/psdds` and `photoshop/` are MIT, and Tessalith's own code in `crates/engine` and `crates/encoders-sys` is `MPL-2.0` (owner grant, Note DC; the vendored encoders keep their own licences). The plugin never links GPL, LGPL or AGPL code (modscan, nif, gui, ...). Copyright holder: Alistraza.
