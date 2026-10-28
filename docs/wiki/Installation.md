# Installation

Most people should use the release zip. Build from source only if you want to change the code.

## The release zip

Download `Tessalith-<version>-win-x64.zip` from the [Releases page](https://github.com/Alistra-za/Tessalith/releases). `SHA256SUMS.txt` beside it lists the hashes of the zip and both programs, if you want to check them.

1. Unzip it anywhere. There is no installer and nothing to register.
2. Double-click `tessalith.exe` for the GUI, or open a terminal in the folder and run `sdc`.

The zip holds:

| File | What it is |
|---|---|
| `tessalith.exe` | The GUI. |
| `sdc.exe` | The command line. It uses the same engine and the same `sdc.toml`. |
| `README.txt` | A short start guide. |
| `CHANGELOG.md` | What changed in each version. |
| `LICENSE`, `LICENSES\`, `THIRD_PARTY_NOTICES.md`, `THIRD_PARTY_LICENSES.md` | Licence texts. |

What you need:

- Windows 10 or 11, x64.
- A graphics card for the GUI's preview windows. The CLI needs none.
- Optional: NVIDIA Texture Tools 3.2.5, as another BC7 / BC6H encoder. See [Optional: NVTT 3.2.5](#optional-nvtt-325).

The programs are not code-signed, so Windows SmartScreen may warn the first time. Click **More info**, then **Run anyway**.

To run `sdc` from any folder, add the unzipped folder to your `PATH`.

The plugins for Photoshop, GIMP, Paint.NET and chaiNNer come in their own zips. See [Host plugins](Host-Plugins.md).

## Building from source

### Requirements

| Requirement | Detail |
|---|---|
| Operating system | Windows, x86-64. The build target is fixed to `x86_64-pc-windows-msvc`. Other platforms are not supported. |
| Rust | Installed through `rustup`. The repository's `rust-toolchain.toml` selects the stable channel, the MSVC target, and the `clippy` and `rustfmt` components. |
| Minimum Rust version | 1.88 for the workspace. The GUI crate needs 1.95 or later because of its `eframe` 0.36.2 dependency. A current stable toolchain covers both. |
| C++ compiler | Microsoft Visual C++ Build Tools (MSVC) with C++17 support. The `encoders-sys` crate compiles the rgbcx encoder, the ISPC Texture Compressor host code and a small C++ shim. |
| Fonts (GUI only) | Segoe UI is required: `tessalith.exe` will not start without it. Segoe UI Symbol, Segoe UI Emoji and Consolas are used when present and skipped when missing. All four ship with Windows. |
| ISPC | Not needed for a normal build. See [ISPC objects](#ispc-objects). |
| NVTT | Optional. See [Optional: NVTT 3.2.5](#optional-nvtt-325). |

### Build steps

1. Install the Visual Studio Build Tools with the "Desktop development with C++" workload.
2. Install Rust with `rustup`. You do not need to pick a toolchain by hand: `rust-toolchain.toml` in the repository root makes `cargo` fetch and use the right one, including the MSVC target and components.
3. Clone the repository and build in release mode:

```powershell
git clone https://github.com/Alistra-za/Tessalith.git Tessalith
cd Tessalith
cargo build --release
```

`.cargo/config.toml` sets the build target to `x86_64-pc-windows-msvc` and links the C runtime statically (`+crt-static`), so the resulting programs do not need a separate Visual C++ redistributable.

The binaries land in the target-specific folder:

```text
target\x86_64-pc-windows-msvc\release\sdc.exe
target\x86_64-pc-windows-msvc\release\tessalith.exe
```

To build only one of them:

```powershell
cargo build --release -p sdc
cargo build --release -p sdc-gui
```

Copy the two `.exe` files wherever you like, or add the folder to your `PATH` so you can run `sdc` from any terminal. Check the build:

```powershell
sdc --version
sdc encoders
```

`sdc encoders` lists the encoder libraries that were linked in and which instruction set the ISPC code dispatched to on your CPU. If it runs, the native encoder layer works.

### Workspace layout

| Crate | Role |
|---|---|
| `crates/engine` | The conversion engine: discovery, grouping, config, packing, mips, encoding, validation. |
| `crates/encoders-sys` | Native encoders: bc7e and bc7enc_rdo's rgbcx, ISPC Texture Compressor (BC6H). |
| `crates/cli` | The `sdc` program. |
| `crates/gui` | The GUI, `tessalith.exe` (Cargo package `sdc-gui`). |
| `crates/render3d` | Direct3D 11 renderer for the GUI's 3D set preview and mesh view. |
| `crates/modscan`, `crates/nif`, `crates/esp`, `crates/vfs`, `crates/pgpsim` | Mod scanning: MO2 resolution, BSA archives, NIF meshes, ESP plugins, PGPatcher simulation. |
| `crates/bench` | Benchmarks. |

## ISPC objects

The bc7e BC7 encoder and the ISPC Texture Compressor are written in ISPC. Building them from source needs the ISPC compiler, so the repository ships them precompiled in `crates/encoders-sys/vendor/ispc-objs`. A normal `cargo build` links those objects and never runs ISPC.

Every vendored file is listed with its SHA-256 hash in `crates/encoders-sys/vendor/SHA256SUMS`. `crates/encoders-sys/vendor/README-VENDOR.md` describes where each piece comes from.

### tools\regen-ispc.ps1

Use this script only if you want to check the vendored objects or change the ISPC sources. It requires ISPC 1.31.0 exactly and refuses any other version.

```powershell
# Rebuild into a temporary folder and compare with the vendored objects
.\tools\regen-ispc.ps1 -Ispc C:\tools\ispc-1.31.0\bin\ispc.exe

# Rebuild and replace the vendored objects and headers
.\tools\regen-ispc.ps1 -Ispc C:\tools\ispc-1.31.0\bin\ispc.exe -Update

# With -Update: replace only the bc7e objects, leave the ISPC Texture Compressor objects alone
.\tools\regen-ispc.ps1 -Ispc C:\tools\ispc-1.31.0\bin\ispc.exe -Update -Bc7eOnly
```

| Parameter | Default | Meaning |
|---|---|---|
| `-Ispc` | `ispc.exe` (from `PATH`) | Path to the ISPC 1.31.0 compiler. |
| `-Bc7eSource` | `crates\encoders-sys\vendor\bc7e\bc7e.ispc` | The bc7e source to compile. |
| `-Update` | off | Copy the rebuilt objects and headers into `vendor\ispc-objs`. |
| `-Bc7eOnly` | off | With `-Update`, replace only the bc7e objects. |

The script compiles for x86-64 Windows with the targets SSE2, SSE4, AVX and AVX2. It runs from the repository root with repository-relative source paths so the output is reproducible, and it reports `MATCH` or `DIFF` for each object. It exits with 0 when every object matches.

### tools\hash-vendor.ps1

After you change anything under `crates\encoders-sys\vendor`, run this to rewrite `SHA256SUMS`:

```powershell
.\tools\hash-vendor.ps1
```

It takes no parameters. It hashes every vendored file except `SHA256SUMS` itself and writes sorted `<sha256>  <relative/path>` lines.

## Optional: NVTT 3.2.5

Tessalith can hand BC7 and BC6H encoding to NVIDIA Texture Tools' `nvcompress.exe`. This is optional and never the default. NVTT is not bundled; you install it yourself.

1. Download NVIDIA Texture Tools 3.2.5 from NVIDIA and install it. The default install puts `nvcompress.exe` in `C:\Program Files\NVIDIA Corporation\NVIDIA Texture Tools\`.
2. Check that Tessalith finds it:

```powershell
sdc encoders
```

The NVTT line reads one of:

```text
NVTT: 3.2.5 at C:/Program Files/NVIDIA Corporation/NVIDIA Texture Tools/nvcompress.exe (tested)
NVTT: 3.3.0 at D:/nvtt/nvcompress.exe (untested: acknowledge with --allow ENC-NVTT-VERSION)
NVTT: not found (<reason>)
```

If you installed it elsewhere, either add its folder to `PATH` or pass the path explicitly with `--nvtt <path to nvcompress.exe>`. In the GUI, use the NVTT section of the Advanced drawer.

Other NVTT versions work only after you acknowledge them with `--allow ENC-NVTT-VERSION`, because only 3.2.5 has been tested. See [NVTT backend](NVTT-Backend.md) for how to use it.

## Where Tessalith keeps its files

| Path | Contents | Written by |
|---|---|---|
| `%APPDATA%\sdc\config.toml` | User config: default target, preset and overrides for every project. | You |
| `%APPDATA%\sdc\presets\*.toml` | Your own presets. Loaded by the CLI and the GUI on every run. | You |
| `%APPDATA%\sdc\gui.toml` | GUI settings: recent folders, the default export folder, preview limits, threads, NVTT path, window layout. | `tessalith.exe` |
| `%LOCALAPPDATA%\sdc\scan.db` | Mesh and plugin scan database. | `tessalith.exe` |
| `%LOCALAPPDATA%\sdc\analysis-cache\` | What the GUI learned about each image, so a folder opened before reuses them instead of decoding again. Pruned to 200 MB when the GUI starts. Safe to delete. | `tessalith.exe` |
| `%LOCALAPPDATA%\sdc\logs\` | GUI crash logs, `crash-<date>-<time>.log`. The CLI writes none. | `tessalith.exe` |
| `<project>\sdc.toml` | Project config. | You or `tessalith.exe` |
| `<project>\sdc-project.toml` | GUI set definitions and TruePBR JSON settings. | `tessalith.exe` |
| `<output>\_sdc\manifest.jsonl` | Record of every converted file. | `sdc` and `tessalith.exe` |

## Uninstalling

Delete the unzipped folder (or the two executables) and, if you want, the `%APPDATA%\sdc` and `%LOCALAPPDATA%\sdc` folders. Nothing is registered with Windows.
