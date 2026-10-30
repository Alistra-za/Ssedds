# Validation and output

This page covers what Tessalith writes, how it checks each file, and how it decides what to rebuild on the next run.

## Output layout

Each output goes to `<output root>\<path relative to the input folder>` with a `.dds` extension, so the output tree mirrors the input tree. Exporter names are normalised: `iron_albedo.png` is written as `iron.dds`, `iron_normalgl.png` as `iron_n.dds`, and raw maps become packed outputs such as `iron_rmaos.dds`.

```text
C:\Work\IronPlate\                          C:\Work\IronPlate-out\
  textures\pbr\armor\ironplate\               textures\pbr\armor\ironplate\
    ironplate_albedo.png            ->          ironplate.dds
    ironplate_normal.png            ->          ironplate_n.dds
    ironplate_roughness.png         -\
    ironplate_metallic.png          ---->       ironplate_rmaos.dds
    ironplate_ao.png                -/
                                              _sdc\
                                                manifest.jsonl
```

| Output root | Default |
|---|---|
| `sdc convert` | `.\sdc-out` in the current directory, or `-o <dir>`. |
| `tessalith.exe` (GUI) | the folder you choose for the project (Review asks for one before it exports). The GUI refuses to write into the folder it reads. |

The output root and any `_sdc` folder are never read as input, so an output folder inside the source folder is safe.

Two inputs that would write the same output (for example `rock.png` and `rock.tga`) are both blocked with `IN-DUPLICATE`. Rename or remove one.

### Atomic writes

Every file is written to a temporary `.sdc-*.tmp` file in the target folder and then renamed into place. An interrupted run never leaves a half-written DDS. A hard abort (a second Ctrl-C, a crash) can leave `.sdc-*.tmp` files behind; delete them by hand.

### The DDS header

Tessalith writes its own DDS headers:

- BC1 and BC3 without `_SRGB`, and uncompressed RGBA8 without `_SRGB`, get a legacy header. Everything else (BC4, BC5, BC6H, BC7, any `_SRGB` format, R8, RG8, RGBA16F) gets a DX10 header. You can change this with the `header` field.
- The flags, caps, linear size and mip count are always set consistently, which some other tools get wrong.
- The non-standard `DDPF_NORMAL` flag is never written.

`sdc patch-header` can fix these header faults in DDS files made by other tools without re-encoding. See [CLI reference](CLI-Reference.md#sdc-patch-header).

## Validation

Every converted file is decoded again before it is written, and the decoded result is checked. `sdc validate` runs the checks that need no source on DDS files that already exist: `VAL-HEADER`, `VAL-ALPHA-GUARANTEE`, `ALPHA-254`, the `VAL-M-*` checks, `VAL-PGPATCHER-50` and `VAL-GRASS-DETECT`. Mip quality, cutout coverage and the BC6H peak need the encoder input, so `validate` cannot run them.

| Check | Rule | Code | Severity |
|---|---|---|---|
| Header | Flags, caps, mip count, format and header form are correct. | `VAL-HEADER` | error |
| Mip quality | Each mip's PSNR (one value, with the error pooled over the channels that matter) against the encoder input is at least `validation.min_mip_psnr_db` (30 dB). With `validation.mips = "smallest"`, only the smallest mip is checked. | `VAL-MIP-ERROR` | warning |
| Opaque alpha | With `remove`, decoded alpha meets the guarantee: at least 254 for any BC7 file without `exact_opaque` (NVTT's included), exactly 255 for BC7 with `exact_opaque` and for every other format. | `VAL-ALPHA-GUARANTEE` | error |
| Decoded 254 | Opaque alpha that decodes as 254 is reported for information. Community Shaders' complex-material gate is not affected by it. | `ALPHA-254` | info |
| Cutout coverage | With `cutout` or `cutout_blend`, each mip's alpha coverage stays within `validation.coverage_tolerance_pts` (2 points) of mip 0. Checked on levels of 256 texels or more. | `VAL-COVERAGE` | warning |
| Complex `_m` margin | The smallest mip's alpha is at or above `validation.complex_m_warn_alpha` (247), close to or past the 251/255 point where Community Shaders turns complex material off. | `VAL-M-COMPLEX-MARGIN` | warning |
| Complex `_m` grey | The smallest mip is grey and not black-with-height, so Community Shaders reads it as a plain env mask. | `VAL-M-GRAYSCALE` | warning |
| Complex `_m` height | Large alpha areas at or below 4/255 or at or above 251/255 give no parallax. | `VAL-M-HEIGHT-CLIP` | warning |
| PGPatcher 50% rule | A plain `_m` that stores alpha has a share of non-255 alpha at or above `validation.pgpatcher_non_white_share` (50%), so PGPatcher would treat it as complex. | `VAL-PGPATCHER-50` | warning |
| BC6H peak | The decoded peak divided by the source peak is at least `validation.bc6h_min_peak_ratio` (0.8). | `VAL-BC6H-PEAK` | warning |
| Complex grass corner | The encoded bottom-left texel still passes Community Shaders' detection test. | `VAL-GRASS-DETECT` | error |

A file with an error from validation is still written. It is reported as `ok ... [VALIDATION FAILED]`, counted under "failed validation", and makes `sdc convert` exit with 4 if nothing worse happened. The next `if-changed` run re-encodes it instead of calling it up to date.

### Cube checks

`sdc inspect` lists these as header issues, and `sdc validate` reports them as `VAL-HEADER` warnings that fail the file:

| Check | Rule |
|---|---|
| No mips | A cube wider than 1 pixel has more than one mip. |
| Marker +Y | A 1×1 cube has a black +Y face. A lit +Y reads in Community Shaders as a near-black static reflection, not a dynamic marker. |
| Format | A cube is UNORM, never an `_SRGB` format. |
| Cube expected | A file named `_e`, or under `textures\cubemaps\` (a `textures` folder directly followed by `cubemaps`), is a cube DDS, not a 2D one. |

The messages are quoted word for word under [Cubemaps](Cubemaps.md#checks). The last check changes the verdict for 2D files: a 2D `_e` or `cubemaps\` file now fails. Every other 2D file is judged as before. A cube is judged on all six faces, and each message names the face and mip. See [Cubemaps](Cubemaps.md).

### The BC1 decoder model

Different GPUs decode BC1 colour slightly differently. `--bc1-decoder` (and *BC1 decoder* in the GUI's Advanced drawer) picks which decoder rgbcx encodes BC1 blocks against and validation decodes with:

| Value | Model |
|---|---|
| `ideal` (default) | Exact interpolation. |
| `ideal-round4` | Ideal with rounding. |
| `nvidia` | NVIDIA hardware. |
| `amd` | AMD hardware. |

The model is recorded in every manifest record. Only BC1 and BC3 outputs compare it, so changing it re-encodes those on the next `if-changed` run.

## The manifest

Every output root has a manifest at `<output root>\_sdc\manifest.jsonl`. Each converted file appends one JSON line. When a file is converted again, a new line is appended; the last line for each output wins.

A record holds:

| Field | Contents |
|---|---|
| `schema`, `ts` | Record schema and time written. |
| `engine_version`, `engine_commit` | The Tessalith version and build. |
| `encoders`, `isa` | Encoder versions and patches, the NVTT version if used, and the CPU instruction set used. |
| `preset` | The preset and its chain. |
| `profile`, `profile_sha256`, `provenance` | The full resolved settings, their hash, and where each field came from. |
| `source`, `source_sha256`, `sources` | The input file and its hash; for packed or baked outputs, every source. |
| `output` | The output path relative to the output root. |
| `validation` | The validation summary. |
| `bc1_model` | The BC1 decoder model. |
| `shader` | The set's shader and why. |
| `output_stamp` | Size and modified time of the output as written. |

The JSON Schema is available with `sdc presets schema manifest`.

### Sidecar files

`sdc convert --sidecar` also writes `<name>.dds.json` next to each output with that file's record. With `if-changed`, an up-to-date output whose sidecar is missing gets its sidecar rewritten from the manifest.

## Re-runs and if-changed

`--on-existing if-changed` is the default. For each output that already exists, Tessalith looks up its manifest record. The output is up to date, and left alone, only if all of these match:

- the encoder versions (and the rgbcx patch for BC3/BC4/BC5);
- the NVTT version, for a texture encoded with NVTT;
- the output file's size and modified time;
- the BC1 decoder model, for BC1 and BC3;
- the record passed validation;
- the texture type's encode version (bumped only when a release changes that type's output);
- every source file's content (by SHA-256), including packing sources, the baked normal map and RNM layers;
- the resolved settings (by hash).

So editing a source, changing a setting in `sdc.toml` or a preset re-encodes exactly the files affected. A new Tessalith version re-encodes only the texture types whose output changed, and nothing when none did. Touching an output by hand (or replacing it) also re-encodes it.

### After an upgrade

Before the run, `sdc convert` prints what the new version re-encodes and why, one line per cause, and the GUI's Review lists the same lines as warnings to tick:

- `2 files re-encode: the normal encoder changed in 1.1.0`: that texture type's encode version moved;
- `3 files re-encode: their settings changed`: the resolved settings differ from what the older version recorded (for example, a changed default);
- `1 file re-encodes: new output type`: the plan writes a file the older version did not;
- `First export with 1.1.0: 20 files`: no manifest to compare with (a new output folder, or records from before encode versions existed). In the GUI this is a plain line, not a warning.

Only records an older version wrote are compared this way, so your own edits after the first export with the new version say nothing. Changed source files are never listed.

An existing output with no manifest record is never overwritten by `if-changed`. It is kept and reported as `OUT-UNKNOWN-EXISTING`, and the run exits with 1. Use `--on-existing overwrite` to replace it.

| `--on-existing` | Existing output with a current record | Changed record | No record |
|---|---|---|---|
| `if-changed` | kept (up to date) | re-encoded | kept, `OUT-UNKNOWN-EXISTING` |
| `skip` | kept | kept | kept |
| `overwrite` | re-encoded | re-encoded | re-encoded |

## Installing the output

Copy the contents of the output root into your mod so the files land under `Data\textures\...`. The `_sdc` folder is for Tessalith only; leave it out of the mod. For a TruePBR set, also install the JSON the GUI writes to `<export folder>\PBRNIFPatcher\<name>.json`, and run PGPatcher.
