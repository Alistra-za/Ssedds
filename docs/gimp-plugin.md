# Tessalith DDS plug-in for GIMP

The plugin's source, and every path and build command on this page, are on branch `host-plugins`, not `main`; see [Host plugins](wiki/Host-Plugins.md#where-the-plugins-live).

`tessalith-dds` is a GIMP 3 plug-in (Windows x64, GIMP 3.0.x) that opens and exports Skyrim DDS textures with Tessalith's encoders and rules. Its user guide is `gimp/README.md`; the rulings are Note DG in [DECISIONS.md](DECISIONS.md), on top of Note DC (Photoshop), DD (psdds.dll), DF (16-bit, 32-bit and HDR) and DH (Alpha is transparency).

## Layout
- `crates/psdds`: the shared DLL (`psdds.dll`, C ABI in `photoshop/psdds.h`, ABI 4). Every rule lives there: greying, formats per type, presets, alpha, clamp, size limits, encode, decode.
- `gimp/tessalith-dds/tessalith-dds.py`: the plug-in (registration, Open, Export, the dialog; the only file that imports `gi`).
- `gimp/tessalith-dds/psdds_ctypes.py`: the `ctypes` wrapper (ABI check on first use) and the pure, gi-free dialog logic (`apply_preset`, `match_preset`, `PresetState`, `plan_refresh`, `preset_event`).
- `gimp/tessalith-dds/tdds_pixels.py`: pure pixel helpers (`band_rows`, `precision_depth`).
- `gimp/tests/`: unit tests (`test_*.py`), the batch cases (`gimp_batch.py`, run inside `gimp-console` by `batch/run_batch.ps1`), `run.ps1`.
- `gimp/package.ps1`: the release zip.

## Tests
Always with GIMP's own Python and `-I` (never `python -`):
```
.\gimp\tests\run.ps1                # unit tests
.\gimp\tests\run.ps1 -Gimp          # unit tests, then the headless batch suite in gimp-console
.\gimp\tests\run.ps1 -KillProbe     # also proves the batch timeout kills gimp-console and its plug-in tree
```
`SDC_GIMP_DIR` overrides `C:\Program Files\GIMP 3`. The batch run uses a temporary profile (`GIMP3_DIRECTORY`, `GIMP3_CACHEDIR`) and sets `TDDS_NO_UI=1`, so any code path that would open a dialog fails fast. `gimp-console -i` does not stop a dialog on Windows (Note DG10), so never start the batch without that guard. The tests never touch `%APPDATA%\GIMP`. The dialog itself (layout, Reset, widget behaviour) is checked by hand, not headless.

## Build and package
```
.\crates\psdds\build.ps1            # psdds.dll into target\psdds\x86_64-pc-windows-msvc\release\
.\gimp\package.ps1                  # dist\TessalithDDS-GIMP-<version>.zip + .sha256.txt, with the package checks
```
`package.ps1` builds the DLL first, needs `cargo-about` (`cargo install cargo-about --locked`) for `THIRD_PARTY_NOTICES.md`, and fails when the zip's file list, the DLL bytes, the licence texts or the DLL's ABI are wrong. For development, `TESSALITH_PSDDS_DLL` points the wrapper at another DLL.

## Install
Install as [Host plugins](wiki/Host-Plugins.md#gimp) says. GIMP caches a plug-in's registration until its `.py` file changes, so after replacing only the DLL or after a failed load, change or recopy the `.py` file.

## Licence split
The licences are listed in [Host plugins](wiki/Host-Plugins.md#licences); the package carries `COPYING`, `LICENSE` and `THIRD_PARTY_NOTICES.md`, all GPL-3.0-compatible. The engine code inside `psdds.dll` is MPL-2.0 (DC41): nothing in the package may describe the engine as MIT.
