# Tessalith DDS plugin for Paint.NET

The plugin's source, and every path and build command on this page, are on branch `host-plugins`, not `main`; see [Host plugins](wiki/Host-Plugins.md#where-the-plugins-live).

`TessalithDds.dll` is a Paint.NET 5.1+ file type plugin (Windows x64) that saves Skyrim DDS textures with Tessalith's encoders and rules. Its user guide is `paintdotnet/README.md`; the rulings are the Paint.NET note in [DECISIONS.md](../.claude/DECISIONS.md) (Note DI).

## Layout
- `paintdotnet/src/Core/`: the host-free code (P/Invoke and ABI check for `psdds.dll`, pixel conversion, dialog model, preset store and selection). It names no `PaintDotNet` type.
- `paintdotnet/src/`: the Paint.NET binding (file type, save token, WinForms save widget, name prompt).
- `paintdotnet/tests/`: xunit tests that compile `src/Core/*.cs` directly.
- `psdds.dll` comes from `crates/psdds` and carries every rule (greying, formats per type, alpha, size limit). The C# keeps only display labels.

## Build
```
.\paintdotnet\build.ps1            # Release: builds psdds.dll (crates\psdds\build.ps1), then the plugin
.\paintdotnet\build.ps1 -Test      # also runs the xunit tests (output in paintdotnet\tests\last-test.log)
.\paintdotnet\package.ps1          # dist\TessalithDDS-PaintDotNet-<version>.zip + .sha256.txt
```
Paint.NET's `PaintDotNet.*.dll` are read at build time from `PDN_DIR` (default `C:\Program Files\paint.net`) and never copied. The shared build writes `psdds.dll` to `target\psdds\x86_64-pc-windows-msvc\release\psdds.dll`. `package.ps1` needs `cargo-about` on PATH and fails if any `PaintDotNet*.dll` is staged. The zip holds seven files under `TessalithDDS\`.

## Why the tests avoid PaintDotNet assemblies
Paint.NET's assemblies are ReadyToRun-compiled against its private runtime, so loading them under the machine's .NET aborts with an MVID mismatch. The test project therefore references none of them and compiles only `Core`.

## Licence split
The plugin C# is GPL-3.0-or-later with a section 7 additional permission for Paint.NET (`paintdotnet/COPYING`). `psdds.dll` is separate: the `psdds` crate is MIT (`paintdotnet/LICENSE`), and the `engine` and `encoders-sys` code in it is MPL-2.0 (Note DC41), with the source pointer in the shipped `THIRD_PARTY_NOTICES.md`. Copyright holder: Alistraza.

Run package.ps1 with pwsh (PowerShell 7): Windows PowerShell 5.1 can write backslash entry names into the zip.