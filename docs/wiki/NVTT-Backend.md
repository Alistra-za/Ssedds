# NVTT backend

Tessalith can hand BC7 and BC6H encoding to NVIDIA Texture Tools (`nvcompress.exe`) at its Production quality level, instead of the built-in bc7e and ISPC encoders. It is an optional quality setting for individual textures. It is never the default, and NVTT is never bundled: you install it yourself, and Tessalith runs it as a separate program.

| | Built-in | NVTT |
|---|---|---|
| Formats | All | BC7 and BC6H only |
| Speed | Normal | About 40 to 60 times slower |
| Installed with Tessalith | Yes | No; install NVTT 3.2.5 yourself |
| Previews | Exact | The GUI previews with the built-in encoder |

## Installing

Install NVIDIA Texture Tools 3.2.5, the tested version. See [Installation](Installation.md#optional-nvtt-325).

## How Tessalith finds nvcompress

1. An explicit path: `--nvtt <path>` on the command line, or the NVTT path in the GUI's Advanced drawer. If the explicit path does not exist, NVTT counts as not found; there is no fallback to the other locations.
2. `nvcompress.exe` on `PATH`.
3. `C:\Program Files\NVIDIA Corporation\NVIDIA Texture Tools\nvcompress.exe`.

Tessalith runs the program once to read its version from the banner and to check that its usage text lists every flag it needs. This probe happens only when a texture in the batch uses NVTT, so a batch that does not use it never starts `nvcompress`.

Check what it finds:

```powershell
sdc encoders
sdc encoders --nvtt D:\tools\nvtt\nvcompress.exe
```

```text
NVTT: 3.2.5 at C:/Program Files/NVIDIA Corporation/NVIDIA Texture Tools/nvcompress.exe (tested)
```

The path is printed with forward slashes.

In the GUI, the NVTT section of the Advanced drawer shows the same status. **Choose nvcompress.exe…** sets a path, **Automatic** goes back to `PATH` and the default folder. The status is re-checked when the window regains focus.

## Using it

### In sdc.toml

Set the backend on the textures that should use it:

```toml
# One texture
[files."textures/armor/iron/iron.png"]
encoder.backend = "nvtt"

# Every PBR albedo
[subtypes.pbr_albedo]
encoder.backend = "nvtt"

# Everything NVTT can encode
[defaults]
encoder.backend = "nvtt"
```

In `[defaults]` (or a preset's defaults), the backend applies only where it fits: BC7 and BC6H textures use NVTT; every other format keeps its own default backend (rgbcx for BC1, BC3, BC4 and BC5, `uncompressed` for the uncompressed formats). If you set `nvtt` on a specific texture together with a format NVTT cannot write, that texture's profile is invalid (`CFG-INVALID`).

### On the command line

```powershell
sdc convert src --recursive --accept-suggestions high --set "file:**/iron.png.encoder.backend=nvtt"
sdc convert src --recursive --accept-suggestions high --set subtype:pbr_emissive.encoder.backend=nvtt --nvtt D:\nvtt\nvcompress.exe
```

### In the GUI

Select a texture and set **Encoder** in the Compression group to **NVTT Production**. The row is:

- locked for BC1 to BC5 ("NVTT encodes BC7 and BC6H only"),
- hidden for uncompressed formats,
- locked with "NVTT not found" while the Advanced drawer says NVTT is not found. A texture already on NVTT can still be switched back.

Changing the format away from BC7 or BC6H returns the texture to the built-in encoder.

The 2D preview of an NVTT texture uses the built-in encoder and says "Preview uses the built-in encoder; the export uses NVTT."

**Copy CLI command** does not include the NVTT path you chose in the GUI. Add `--nvtt <path>` to the copied command.

## Codes

| Code | Severity | Meaning | What to do |
|---|---|---|---|
| `ENC-NVTT-MISSING` | blocking | A texture uses NVTT but `nvcompress.exe` was not found. | Install NVTT, give the path with `--nvtt` or in Advanced, or switch the texture back to the built-in encoder. |
| `ENC-NVTT-VERSION` | blocking | The NVTT found is not 3.2.5. | Acknowledge with `--allow ENC-NVTT-VERSION` (or in the GUI) to use it anyway. |
| `ENC-NVTT-FAILED` | error | `nvcompress` could not start, exited with an error, or wrote no or a wrong-sized result. | The file fails at the compress stage. There is no fallback to the built-in encoder. Check the NVTT install, or switch the texture back. |

## How it runs

- Tessalith builds the mip chain itself, as for the built-in encoders. Each level is handed to `nvcompress` separately as a small legacy-header DDS, with the flags `-silent -nomips -production` plus `-bc7` or `-bc6`. For BC7, `-noalpha` is passed when the alpha policy is `remove` and `-alpha` otherwise. `-nocuda` is always passed; the output is the same with and without CUDA.
- Intermediate files go to a folder `%TEMP%\sdc-nvtt-<process id>-<n>`, one per file, removed when the file finishes, fails or is cancelled. A crash can leave one behind; it holds only intermediate images and is safe to delete.
- One `nvcompress` process runs at a time.
- Cancelling the batch stops the running `nvcompress` process. There is no timeout.
- The NVTT version is recorded in the manifest. With `--on-existing if-changed`, a different NVTT version re-encodes the textures that used it.

## When to use it

If `VAL-BC6H-PEAK` fires (a BC6H texture keeps less of the source peak than `validation.bc6h_min_peak_ratio`; the warning reads "BC6H keeps N% of the source peak"), NVTT Production is worth trying for that texture; the warning itself does not mention NVTT. Otherwise, use it on textures where you have compared both results and prefer NVTT's, keeping in mind that it is much slower.
