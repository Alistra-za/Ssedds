# FAQ

### What is Tessalith, and why is the command line called sdc?

Tessalith is the product name. The GUI is `tessalith.exe` and its window says Tessalith. The command line, config files and folders kept the project's working name: `sdc.exe`, `sdc.toml`, `sdc-project.toml`, `%APPDATA%\sdc`, `%LOCALAPPDATA%\sdc` and the `SDC_*` environment variables. Those are what you type.

### Does it run on Linux or macOS?

No. Tessalith is Windows-only (x86-64, built with MSVC). The GUI also uses Direct3D 11 and Windows system fonts.

### Is there an installer or a prebuilt download?

Not yet. Build from source; see [Installation](Installation.md).

### Which target should I pick?

`cs` if you play with Community Shaders, which is the default. `vanilla` for textures that must work without Community Shaders; for the complex material `_m` layout outside Community Shaders, use `vanilla` and set the set's shader to complex material. See [Presets and targets](Presets-and-Targets.md).

### Why are my files "unresolved" on the command line but fine in the GUI?

The GUI accepts High-confidence suggestions automatically; `sdc` needs `--accept-suggestions high`. See [Troubleshooting](Troubleshooting.md#every-file-is-unresolved-on-the-command-line).

### Can I convert a whole mod folder?

Yes. Point `sdc convert` (with `--recursive`) or the GUI at the folder. Files whose type cannot be worked out are reported, not guessed, so expect to answer some questions the first time. Your answers are saved in `sdc.toml` and apply on every later run.

### Will it re-encode my existing DDS files?

No, not by default. DDS inputs are skipped (`IN-DDS-SKIPPED`) because re-encoding loses quality. Set `reencode = true` on a file to convert it anyway. To fix only a broken header, use `sdc patch-header`.

### Does it change my source files?

No. Tessalith only writes to the output folder, `sdc.toml`, `sdc-project.toml` (GUI), the manifest under `<output>\_sdc\`, the scan database with its `scan.lock`, the 3D preview's shader cache under `%LOCALAPPDATA%\sdc\shader-cache`, temporary NVTT folders under `%TEMP%`, and its own settings (plus `gui.toml.bad`, a copy of a settings file it could not read). Renaming in the GUI writes aliases; files on disk are never renamed.

### Why BC7 for almost everything?

BC7 keeps the most quality at the same size as BC3. BC4 is used for single-channel maps, BC5 for two-channel data, BC6H for HDR under `cs`. BC1 is used only by the compact presets, for opaque colour.

### How do I get smaller files?

Use the `cs-compact` or `vanilla-compact` preset: BC1 for opaque colour, half size for other maps whose larger side is above 512. Or set `resize = "half"` and `format = "bc1"` yourself where you want them.

```powershell
sdc convert src --recursive --accept-suggestions high --preset cs-compact
```

In the GUI, set `preset = "cs-compact"` in `sdc.toml`; there is no preset picker yet.

### How do I make a quick test conversion?

Use the `fast-preview` preset. It encodes at `fast` effort and validates only the smallest mip. Do not release with it.

### My normal map is from Substance / Blender. Which green convention?

Skyrim uses DirectX. If your tool exported OpenGL normals, name the file `_normalgl` or set `channels.green_convention = "opengl"`, and Tessalith flips green on output. A file named `_n` is taken as DirectX.

### What does "exact opaque" mean?

bc7e can store fully opaque alpha so that it decodes as 254 instead of 255. That is harmless for most textures, but PGPatcher and ENB read the alpha of `_m` maps, so Tessalith guarantees exactly 255 there by default (`alpha.exact_opaque`).

### Why does my diffuse ask about alpha when the normal map does not?

Whether a colour map's alpha is a cutout, a blend or unused depends on the mesh, not the texture. Normal and data maps have a fixed meaning. Scan your mods in the GUI to let the meshes answer.

### Does the CLI use the GUI's set edits or mod scan?

No. The CLI reads `sdc.toml` but not `sdc-project.toml` or the scan database. Set moves, borrows, shader choices and output names made in the GUI's Sets column, alpha taken from scanned meshes, and the complex-grass auto-tag apply to GUI exports only. Per-texture decisions, which go into `sdc.toml`, apply to both.

### Does it write the PGPatcher JSON?

The GUI does, for PBR sets under `cs`, to `<export folder>\PBRNIFPatcher\<name>.json` after a complete export. The CLI does not.

### Does it need NVIDIA Texture Tools?

No. NVTT is an optional encoder for BC7 and BC6H. See [NVTT backend](NVTT-Backend.md).

### Which source formats work?

PNG, TGA, TIFF, EXR, HDR, BMP, JPEG, WebP, PSD/PSB (merged image, saved with Maximize Compatibility), and Krita/OpenRaster (merged image). GIMP `.xcf` and Paint.NET `.pdn` are not read; export to PNG or TGA first.

### Can I run conversions from a script or another tool?

Yes. `sdc convert --json` writes a JSON Lines event stream to stdout, and the exit code tells you the outcome. Schemas for events, the manifest, presets and configs come from `sdc presets schema`. See [CLI reference](CLI-Reference.md#json-event-stream).

### How do I convert only what changed?

Do nothing special: `--on-existing if-changed` is the default. It compares each output with its manifest record and re-encodes only what changed. See [Validation and output](Validation-and-Output.md#re-runs-and-if-changed).

### Where are my settings stored?

See the table in [GUI guide](GUI-Guide.md#where-settings-live).

### What licence is it under?

GPL-3.0-or-later. The vendored encoder libraries keep their own licences; see `crates/encoders-sys/vendor/README-VENDOR.md`.
