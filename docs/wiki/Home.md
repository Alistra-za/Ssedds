# Tessalith wiki

Tessalith converts source images (PNG, TGA, TIFF, EXR, PSD and others) into DDS textures for Skyrim Special Edition and Anniversary Edition. It runs on Windows only and ships as two programs that share one conversion engine:

- `sdc`, the command-line tool.
- `tessalith.exe`, the desktop application (the window says Tessalith).

There are also optional plugins that save DDS straight from Photoshop, GIMP, Paint.NET and chaiNNer with the same encoders and rules. See [Host plugins](Host-Plugins.md).

Each project folder can hold an `sdc.toml` file that records how its textures are converted. The GUI writes it and both programs read it, so you can set a project up in the GUI and then run it from a script.

Tessalith builds for two targets:

| Target | Value | Use it when |
|---|---|---|
| Community Shaders | `cs` (default) | You play with Community Shaders, including TruePBR materials. |
| Vanilla | `vanilla` | You want textures that load without Community Shaders. A set given the complex material shader packs the complex `_m` (environment, gloss, metal, height). |

This wiki covers version 1.0.0. Tessalith is licensed GPL-3.0-or-later. Its conversion engine (the `engine` and `encoders-sys` crates) is MPL-2.0, and the plugins' shared `psdds.dll` is MIT except for that engine code inside it; see [Host plugins](Host-Plugins.md#licences).

## Pages

| Page | What it covers |
|---|---|
| [Installation](Installation.md) | The release zip, requirements, building from source, the encoder libraries, optional NVTT setup. |
| [Getting started](Getting-Started.md) | A first project from source folder to finished DDS files, in the CLI and in the GUI. |
| [CLI reference](CLI-Reference.md) | Every `sdc` subcommand and flag, exit codes, the JSON event stream. |
| [Project config](Project-Config.md) | Every `sdc.toml` key, its default, and how settings layer. |
| [Presets and targets](Presets-and-Targets.md) | The built-in presets, writing your own, and what each target allows. |
| [GUI guide](GUI-Guide.md) | Every panel in the GUI: the converter, set editor, bulk editing, 3D preview, PGPatcher JSON editor, settings. |
| [Texture types](Texture-Types.md) | Sub-types, file suffixes, naming, formats, packing and set shaders. |
| [Mod scanning](Mod-Scanning.md) | Scanning meshes and plugins from an MO2 instance or a folder, and what the scan feeds back. |
| [NIF editing](NIF-Editing.md) | Editing a mesh's shader, alpha, controllers and clips in the Meshes view, and exporting the edited NIF. |
| [Host plugins](Host-Plugins.md) | Saving DDS from Photoshop, GIMP, Paint.NET and chaiNNer: install, options, HDR. |
| [NVTT backend](NVTT-Backend.md) | Using NVIDIA Texture Tools 3.2.5 as an optional BC7/BC6H encoder. |
| [Validation and output](Validation-and-Output.md) | What gets checked after encoding, the output layout, the manifest, re-run behaviour. |
| [Troubleshooting](Troubleshooting.md) | Every diagnostic code, common errors, and the in-game checklist. |
| [FAQ](FAQ.md) | Short answers to common questions. |
| [Research behind the defaults](Research.md) | The benchmarks, spikes and literature behind each default. |

## Names you will see

The product is called Tessalith, and the GUI program is `tessalith.exe`. The command line, files and folders keep the working name `sdc`, and those are what you type:

| You type or see | What it is |
|---|---|
| `sdc` / `sdc.exe` | Command-line tool. Its `--help` text still calls itself a working name. |
| `tessalith` / `tessalith.exe` | Desktop application. |
| `sdc.toml` | Project config file. |
| `sdc-project.toml` | GUI-only set definitions, stored next to `sdc.toml`. |
| `%APPDATA%\sdc\` | User config, user presets and GUI settings. |
| `%LOCALAPPDATA%\sdc\scan.db` | Mesh and plugin scan database. |
| `<output>\_sdc\manifest.jsonl` | Record of every file written. |

## What Tessalith does with a folder

1. Finds every image in the input folders.
2. Groups files into texture sets by base name (`rock01.png`, `rock01_n.png`, `rock01_rmaos.png` form one set).
3. Works out a sub-type for each file from its suffix or name, for example `_n` is a normal map.
4. Resolves the settings for each file from built-in defaults, the preset, your config files, rules and command-line overrides.
5. Packs raw maps (roughness, metallic, AO and so on) into the packed layout the target expects.
6. Builds mipmaps, encodes with the chosen encoder, writes the DDS atomically.
7. Decodes the result again and validates it, then records it in the manifest.

Tessalith does not guess at files whose type it cannot work out. It reports them as unresolved, and you fix them by renaming or by adding a `[files]` entry. See [Texture types](Texture-Types.md).
