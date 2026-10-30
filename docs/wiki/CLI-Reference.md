# CLI reference

`sdc` is the command-line front end of Tessalith. It has six subcommands:

| Subcommand | What it does |
|---|---|
| [`convert`](#sdc-convert) | Convert images or folders to DDS. |
| [`inspect`](#sdc-inspect) | Show the plan for images, or the header and alpha statistics of DDS files. Writes nothing. |
| [`validate`](#sdc-validate) | Check existing DDS files against a target's rules. |
| [`presets`](#sdc-presets) | List, show, compare, export and check presets; print JSON schemas. |
| [`encoders`](#sdc-encoders) | Print the built-in encoder versions and look for NVTT. |
| [`patch-header`](#sdc-patch-header) | Fix DDS header faults that need no pixel change. |

`sdc --version` prints the version. `sdc --help` and `sdc <subcommand> --help` print the built-in help. The help text calls the program "Skyrim SE/AE DDS converter (working name)"; that is Tessalith.

Human-readable output from `convert` goes to stderr. The result verbs (`inspect`, `validate`, `presets`, `encoders`, `patch-header`) print their results to stdout. With `--json`, stdout carries only JSON.

## Shared batch flags

`convert` and `inspect` accept the same set of input and settings flags.

| Flag | Default | Meaning |
|---|---|---|
| `<inputs>...` | required | Image files or folders. Several are allowed. |
| `--recursive` | off | Walk folders recursively. Without it, only the files directly inside each folder are read. Files you name explicitly are always included. |
| `--target cs\|vanilla` | see below | The target game setup. |
| `--preset <id or file.toml>` | see below | Select a preset by id, or load a preset file and select the id it declares. |
| `--preset-file <file>` | none | Load a preset file without selecting it, for example one that declares suffix aliases. Repeatable. Loading an id that is already loaded is an error. |
| `--config <file>` | `sdc.toml` in the first input folder | The project config file. |
| `--set <override>` | none | Override a profile field. Repeatable. See [--set syntax](#--set-syntax). |
| `--fill ROLE=VALUE` | none | Fill value (0 to 255) for a packed channel that has no source map. Roles: `roughness`, `metallic`, `ao`, `specular`, `glossiness`, `height`, `opacity`, `fuzz`. Repeatable. Same as `--set packing.fill.<role>=<value>`, applied after every `--set`. |
| `--accept-suggestions none\|high` | `none` | `high` accepts every High-confidence sub-type suggestion. High-confidence alpha suggestions apply either way. With `none`, a file whose sub-type is only suggested stays unresolved. |
| `--allow <CODE>` | none | Acknowledge a blocking warning code for the whole batch. Repeatable. An unknown code is a usage error. |
| `--json` | off | Write JSON Lines events to stdout. Human output stays on stderr. |

How the target is chosen, highest first: `--target`, then `target` in the project `sdc.toml`, then `target` in the user config, then the selected preset's own target, then `cs`.

How the preset is chosen, highest first: `--preset`, then `preset` in the project `sdc.toml`, then `preset` in the user config, then the default for the target: `cs-quality` or `vanilla-quality`.

The user config is `%APPDATA%\sdc\config.toml`. User presets in `%APPDATA%\sdc\presets\*.toml` are always loaded. See [Presets and targets](Presets-and-Targets.md).

### Why `--accept-suggestions high` matters

Tessalith works out each file's sub-type from its name. A known suffix such as `_n` or `_rmaos`, a known exporter term such as `_albedo`, or no suffix at all (the colour map) gives a High-confidence suggestion. On the command line, suggestions are not applied unless you pass `--accept-suggestions high`. Without it, those files are reported as unresolved:

```text
skip  C:/Work/rock/rock.png: SUG-LOW-CONFIDENCE sub-type 'diffuse' is a High-confidence suggestion (colour member, non-PBR set)
```

Files decided by a rule, a `--set`, a `[files]` entry or a set shader are not affected. The GUI accepts High-confidence suggestions and writes each one into `sdc.toml` as a `[files]` entry, so a project saved from the GUI converts the same way here without the flag. For a folder the GUI has not saved, use `--accept-suggestions high` to match it.

What the GUI decides from a Meshes scan, `sdc` cannot: it never reads the scan database. The GUI writes those decisions into `sdc.toml` too (a sub-type the meshes' shader gives, grass found on a GRAS model, an alpha meaning from the meshes), so a saved project agrees. Two stay GUI-only: a mesh *Remove* on a texture whose alpha varies (`sdc` asks for its alpha meaning instead), and the environment-map `_m` the GUI's Also export derives from the meshes (`sdc convert` writes no `_m`).

### --set syntax

| Form | Applies to |
|---|---|
| `field=value` | Every file. |
| `subtype:<id>.field=value` | Every file of that sub-type. Cannot set `subtype`, `reencode`, `rnm.target` or `helper.*`. |
| `file:<glob>.field=value` | Files matching the glob. The only form for `helper.role`, `helper.fills`, `helper.read` and `helper.invert`: an unscoped `--set helper.role=ao` is refused ("helper.role is set per file: use file:<glob>.helper.role=<…>"), since it would make every image a helper. |
| `set:<set id>.shader=<shader>` | Picks the shader of a texture set. Only `shader` can be set this way. The set id is what `inspect` prints after `set` (the output folder relative to the output root, plus the lower-case base name). An id with no `/` matches every set with that base name. An id that names no set gives `SET-EDIT-STALE`. |

Values are parsed as TOML literals (`true`, `127`, `0.5`); anything that is not a valid literal is taken as a string, so you can write `format=bc1` without quotes. A misspelt field or sub-type is an error, never ignored.

File globs:

- Match case-insensitively against the file's game path (from the last `textures` folder), its path relative to the input folder, and, when the glob has no `/`, its file name alone.
- `*` does not cross `/`. Use `**` for any depth.
- Since the glob may contain dots, the field is taken as the longest known field name at the end of the key.

Examples:

```powershell
# Every texture at fast effort
sdc convert src --recursive --accept-suggestions high --set encoder.effort=fast

# BC7 with alpha weighted x8 (bc7e only; ENC-WEIGHTS-IGNORED elsewhere)
sdc convert src --recursive --accept-suggestions high --set encoder.bc7_weights=alpha8

# All diffuse maps as BC1 (a diffuse that keeps its alpha then raises FMT-BC1-ALPHA)
sdc convert src --recursive --accept-suggestions high --set subtype:diffuse.format=bc1

# One file is a cutout at threshold 100
sdc convert src --recursive --accept-suggestions high --set "file:**/fence01.png.alpha.policy=cutout" --set "file:**/fence01.png.alpha.threshold=100"

# A file name with no clue: tell Tessalith what it is
sdc convert src --recursive --set "file:**/mask_a.png.subtype=generic_data"

# Helper textures: a mask with no clue in its name is the diffuse's opacity, read from green;
# a specular map is converted on its own; a roughness map also fills the _cnr alpha
sdc convert src --recursive --accept-suggestions high --set "file:**/iron_mask.png.helper.role=opacity" --set "file:**/iron_mask.png.helper.read=g"
sdc convert src --recursive --accept-suggestions high --set "file:**/iron_specular.png.helper.role=none"
sdc convert src --recursive --accept-suggestions high --set 'file:**/iron_roughness.png.helper.fills=["rmaos.r", "cnr.a"]'

# Re-encode existing DDS inputs instead of skipping them
sdc convert src --recursive --set reencode=true

# Pick a set's shader
sdc convert src --recursive --accept-suggestions high --set "set:textures/armor/iron/iron.shader=envmap"
sdc convert src --recursive --accept-suggestions high --set "set:iron.shader=envmap"   # every set named iron
```

The full field list, with allowed values, is in [Project config](Project-Config.md#field-reference).

## sdc convert

```text
sdc convert [OPTIONS] <INPUTS>...
```

Plans the batch, then encodes every file whose status is ready.

| Flag | Default | Meaning |
|---|---|---|
| `-o, --output <dir>` | `.\sdc-out` | Output root. Each output goes to `<output>\<path relative to the input folder>` with a `.dds` extension. The output root is never read as input. With `[export] also` in the project `sdc.toml`, the folder the PBR, Vanilla and CM folders go in (see below). |
| `--jobs <n>` | `0` (all cores) | Worker threads. |
| `--mem-budget <size>` | 50% of RAM | Memory budget for files in flight. Bytes, or a `G`/`M` suffix (binary): `8G`, `512M`. `0` is an error; leave the flag out for the default. |
| `--on-existing if-changed\|skip\|overwrite` | `if-changed` | What to do when an output exists. See below. |
| `--dry-run` | off | Plan and report, write nothing. |
| `--sidecar` | off | Also write `<name>.dds.json` next to each output with its record. |
| `--nvtt <path>` | `PATH`, then the NVIDIA install folder | Path to `nvcompress.exe` for textures with `encoder.backend = "nvtt"`. NVTT is looked for only when a texture uses it. See [NVTT backend](NVTT-Backend.md). |
| `--bc1-decoder ideal\|ideal-round4\|nvidia\|amd` | `ideal` | Which hardware decoder model is used when BC1/BC3 output is decoded for validation. |

### Also export as Vanilla / CM

When the project `sdc.toml` has `[export] also` (see [Project Config](Project-Config.md#export)), `sdc convert` runs three passes and writes `<project> PBR`, `<project> Vanilla` and `<project> CM` under `-o`. `<project>` is the first input folder's name (or the folder of the first input file). The PBR folder is the plain cs export; the other two are converted from its PBR sets. Then it writes `<project> conversion-report.md` under `-o` and, with `zip = true`, the zips and `<project> SHA256SUMS.txt`.

- It needs the `cs` target: "Also export needs the cs mode: the PBR folder is the cs export" (exit 2).
- A bad `[export.legacy]` value or folder name is an error (exit 2) on this path only; a plain run does not check them.
- The CLI reads no Meshes scan, so it writes no vanilla env `_m`, and the report says "no: no scan".
- The GUI's **Copy CLI command** passes the Also export location as `-o`.

### --on-existing

| Value | Behaviour |
|---|---|
| `if-changed` | Re-encode only when the manifest record differs: source content, resolved settings, the texture type's encode version (bumped only when a release changes that type's output), encoder versions, NVTT version (only for outputs encoded with NVTT), BC1 decoder model (for BC1/BC3), a record that failed validation, or an output file whose size or modified time changed. An output with no manifest record is kept and reported as `OUT-UNKNOWN-EXISTING`. With `--sidecar`, a missing sidecar on an up-to-date output is written from the manifest record; the file re-encodes only if that write fails. |
| `skip` | Keep every existing output. Counted as skipped on purpose. |
| `overwrite` | Always re-encode. |

### Human log

Each line on stderr has one of these shapes:

```text
ok    <input> -> <output>
ok    <input> -> <output>  [VALIDATION FAILED]
skip  <input>: <CODE> <reason>
FAIL  <input> at <Stage>: <error>
warning <input>: <CODE> <message>
blocking <input>: <CODE> <message>
error <input>: <CODE> <message>
info  <input>: CFG-FILTER-IGNORED <message>
done: N converted, N up to date, N skipped (N on purpose), N failed, N failed validation, N cancelled
```

Warnings of severity warning or above are printed, plus the info code `CFG-FILTER-IGNORED`. Paths use forward slashes. Raw maps consumed by a packed output are not listed.

The `done` counters:

| Counter | Counts |
|---|---|
| converted | Files encoded and written in this run. |
| up to date | Outputs left alone by `if-changed`. |
| skipped | Files not converted for any other reason: unresolved, blocked, duplicate output, DDS input, existing output with no record, `--on-existing skip`, a pre-packed file replaced under `packing.prefer = "raw"` (`PACK-CONFLICT`), a generated file replaced by a real map (`GEN-REPLACED`). |
| on purpose | The part of skipped that you asked for: DDS inputs not opted in (`IN-DDS-SKIPPED`), `--on-existing skip`, a pre-packed file replaced under `packing.prefer = "raw"` (`PACK-CONFLICT`), a generated file replaced by a real map (`GEN-REPLACED`). |
| failed | Files that failed to load, resolve, encode or write. |
| failed validation | Files written whose validation reported an error. |
| cancelled | Files not started because you pressed Ctrl-C. |

### Cancelling

The first Ctrl-C prints:

```text
cancelling: finishing current bands... (Ctrl-C again to exit immediately)
```

Files that are running stop at their next band boundary and no new file starts. Nothing half-written is left behind, because every output is written to a temporary file in the target folder and then renamed into place. The cancelled files are listed in `batch_done`, and the exit code is 1. A Ctrl-C during planning is honoured too.

A second Ctrl-C exits at once with code 3, without waiting for running encodes. That can leave `.sdc-*.tmp` files in the output folders (delete them by hand) and, with `--json`, no `batch_done` event. A file renamed into place just before the abort may have no manifest record; the next run reports it as `OUT-UNKNOWN-EXISTING`, and `--on-existing overwrite` replaces it.

### Examples

```powershell
# Plain conversion of a mod's textures folder
sdc convert "C:\Mods\MyMod\textures" --recursive --accept-suggestions high -o "C:\Mods\MyMod-out"

# Vanilla output, smaller files
sdc convert src --recursive --accept-suggestions high --target vanilla --preset vanilla-compact

# A quick look at the result while you iterate
sdc convert src --recursive --accept-suggestions high --preset fast-preview -o preview-out

# Rebuild everything, 8 threads, 6 GiB budget
sdc convert src --recursive --accept-suggestions high --on-existing overwrite --jobs 8 --mem-budget 6G

# Set metallic to fully metallic on every packed output with no metallic map
sdc convert src --recursive --accept-suggestions high --fill metallic=255

# Acknowledge a non-power-of-two texture
sdc convert src --recursive --accept-suggestions high --allow DIM-NPOT
```

## sdc inspect

```text
sdc inspect [OPTIONS] <INPUTS>...
```

Takes the [shared batch flags](#shared-batch-flags) plus `--bc1-decoder`. Nothing is written. Missing inputs are a usage error (`input not found: <path>`).

For a DDS input it prints the header and an alpha report per mip:

```text
C:/out/textures/armor/iron/iron.dds
  format Bc7 srgb  1024x1024  11 mips  Dx10 header  cubemap false
  mip 0 1024x1024  alpha min 255 =255 100.0% =254 0.0% <254 0.0%
  mip 1 512x512  alpha min 255 =255 100.0% =254 0.0% <254 0.0%
  ...
```

Header problems are listed as `header issue:` lines. The `=254` column matters for BC7: an opaque texture whose alpha decodes as 254 instead of 255 can show as slightly transparent in some shaders.

For image inputs it runs the whole plan as a dry run and prints, per set:

```text
set <set id>  shader <shader> (<why>)
  <severity> <CODE> <message>
  pack <output> (<sub-type>) run|conflict|failed|disabled[: reason]
    R <source>  G <source>  B <source>  A <source>
```

and per file:

```text
<input path>
  suggested <sub-type> (<confidence> confidence: <reason>)
  sub-type <id>  format <format>  tag <srgb|unorm|n/a>  alpha <policy>  output <relative output path>
  status <ready|unresolved|blocked|error|packing_input|normal_layer> <reasons or codes>
  rnm <layer> <file> strength <s>[, mask <file>][, AO <file>]
  <severity> <CODE> <message>
```

Skipped and failed files are the ones `convert` would report, each as its path followed by an indented `skipped <CODE> <reason>` or `cannot be read: <error>` line. `inspect` never re-encodes DDS inputs, even with `reencode`; a DDS is always reported by its header.

Exit codes: 0, or 1 if an input could not be read, or 2 for a usage or config error.

## sdc validate

```text
sdc validate [OPTIONS] <DDS>...
```

Runs the post-encode checks on DDS files that already exist, including files made by other tools. Give `.dds` files; a folder is refused (`<path> is a folder; give .dds files`).

| Flag | Default | Meaning |
|---|---|---|
| `--target cs\|vanilla` | `cs` | Whose rules apply. |
| `--subtype <id>` | inferred from the file's suffix, its game path and, for formats with alpha, its mip-0 alpha | What the files are meant to be. A sub-type the target does not have is an error. |
| `--alpha-policy remove\|cutout\|blend\|cutout_blend\|data` | inferred | The alpha meaning to check against. Inferred as `remove` when every mip-0 alpha value is at least 254. Otherwise the sub-type's default, except that a sub-type whose default is `remove` is inferred as `data`. |
| `--exact-opaque` | off | Hold BC7 files under `remove` to Tessalith's own guarantee that opaque alpha decodes as exactly 255. Off by default because other encoders do not promise it. |
| `--strict-cubemaps` | off | Fail a flat 2D file the game reads as a cube (a name ending in `_e`, or under `textures\cubemaps\`). Without it that check is a warning and the file passes. See [Cubemaps](Cubemaps.md). |
| `--json` | off | One JSON object per file on stdout; the human report moves to stderr. |
| `--bc1-decoder <model>` | `ideal` | Decoder model for BC1/BC3. |

The format and `_SRGB` tag are taken from the file, so third-party files are judged on header and alpha rules rather than on Tessalith's preferred format. A cube DDS is judged as a `cubemap`, and a 2D file whose name ends in `_e` or that sits under `textures\cubemaps\` gets a warning and still passes; `--strict-cubemaps` fails it (the game reads it as a cube). See [Validation and output](Validation-and-Output.md#cube-checks). A format Tessalith never writes has only its header checked and is reported as not judged.

```text
ok    C:/out/rock.dds (diffuse, alpha remove)
FAIL  C:/out/rock_n.dds (normal)
  error VAL-HEADER ...
skip  C:/out/old.dds (diffuse)
  note: not judged: format not produced by sdc (Bc2)
validated 3: 1 ok, 1 failed validation, 0 unreadable, 1 not judged
```

Exit codes: 1 if any file could not be read, otherwise 4 if any failed, otherwise 0.

## sdc presets

```text
sdc presets [--preset-file <file>]... <COMMAND>
```

`--preset-file` loads extra preset files after the user presets folder. User presets that fail to load are skipped with `CFG-PRESET-SKIPPED` on stderr.

| Command | What it does |
|---|---|
| `list [--json]` | Lists selectable presets: id, version, target, description, suffix aliases, and the source file for user presets. |
| `show <id> [--subtype <id>] [--target <t>]` | Prints the preset's resolved profile for each sub-type (or one) as JSON, with the provenance of each field. |
| `diff <a> <b> [--subtype <id>] [--target <t>] [--json]` | Prints every field where two presets resolve differently, as `<subtype>.<field>: <a value> → <b value>`, then `(rules are not compared)`. Prints `no differences` when they match. |
| `export <id> [--out <file>] [--id <new id>]` | Writes the preset's own TOML under a new id (default `<id>-export`), keeping its `base`. Comments are dropped and keys may reorder. `--out` refuses the source file of a loaded preset. Suffix aliases are copied as they are, so rename or remove them before you install the copy beside the original. |
| `validate <files>...` | Checks preset files (migration, parse, ids, base, aliases, rules, every profile). Prints `ok <file>` or the problem. Exit 1 if any file has a problem. |
| `schema <which> [--out <dir>]` | Prints a JSON Schema: `profile`, `preset`, `config`, `event`, `manifest`, `dds-info`, `dds-validation`. `all` writes every schema to `--out` and requires it (`schema all needs --out <dir>`). |

Examples:

```powershell
sdc presets list
sdc presets show cs-quality --subtype diffuse
sdc presets diff cs-quality cs-compact --target cs
sdc presets export cs-quality --id my-cs --out "$env:APPDATA\sdc\presets\my-cs.toml"
sdc presets validate "$env:APPDATA\sdc\presets\my-cs.toml"
sdc presets schema all --out schemas
```

## sdc encoders

```text
sdc encoders [--nvtt <path>] [--bc1-decoder <model>] [--json]
```

Prints the versions of the linked encoders and whether NVTT is available:

```text
bc7e (bc7enc_rdo <commit>) patch <patch id or none>
rgbcx (bc7enc_rdo <commit>) patch <patch id or none>
ISPC Texture Compressor <version> (BC6H)
ISPC <version> objects; dispatched ISA on this CPU: <isa>
BC1 decoder model: ideal
NVTT: 3.2.5 at C:\Program Files\NVIDIA Corporation\NVIDIA Texture Tools\nvcompress.exe (tested)
```

The NVTT line is one of `(tested)`, `(untested: acknowledge with --allow ENC-NVTT-VERSION)` or `NVTT: not found (<reason>)`. `--json` prints one JSON object instead.

## sdc patch-header

```text
sdc patch-header (--out-dir <dir> | --in-place) [OPTIONS] <DDS>...
```

Fixes DDS header faults that need no change to the pixel data: mip count, flags, caps, linear size or pitch, reserved fields, a non-standard DDPF_NORMAL flag, the legacy or DX10 header form, the DX10 dimension, array and misc fields, the header and pixel-format size fields, and the `_SRGB` tag. Exactly one of `--out-dir` or `--in-place` is required.

| Flag | Meaning |
|---|---|
| `--out-dir <dir>` | Write each patched file into this folder under its own name. The input is never changed. |
| `--in-place` | Rewrite each input in place, atomically. |
| `--target cs\|vanilla` | Whose rules give the tag and mip expectation of `--subtype` (default `cs`). |
| `--subtype <id>` | What the files are meant to be. Sets the tag, and requires a full mip chain when that sub-type's profile has one. |
| `--tag srgb\|unorm` | Tag to write. Overrides the tag from `--subtype`. Without either, the file's tag is kept. |
| `--format <format>` | The block format the files must be; a file in another format is refused. |
| `--dry-run` | Report what would change, write nothing. |

Per file, it prints one of (with `--dry-run` a file that would change prints `<file>: would patch: <fixes>` instead of the `DDS-HEADER-PATCHED` line, and no code):

```text
<file>: header already valid; nothing to patch
<file>: DDS-HEADER-PATCHED: <what changed>
<file>: DDS-HEADER-UNPATCHABLE: <why>
<file>: unreadable: <error>
```

Example: a third-party PBR albedo saved without the `_SRGB` tag.

```powershell
sdc patch-header --out-dir fixed --subtype pbr_albedo textures\pbr\rock\rock.dds
```

Exit codes: 0; 1 if a file is unreadable or refused; 2 if `--out-dir` would write over an input or two inputs share a file name; 3 if a write fails (files written before it stay written).

## sdc cubemap-marker

```text
sdc cubemap-marker [--color <R,G,B>] [--roughness <0..1>] -o <file.dds>
```

Writes the 1×1 dynamic-cubemap marker that Community Shaders and ENB read as "reflect the live scene". The file is 152 bytes: a six-face RGBA8 cube with the colour on +X and every other face black. See [Cubemaps](Cubemaps.md#dynamic-marker).

| Flag | Default | Meaning |
|---|---|---|
| `--color <R,G,B>` | `0,0,0` | The reflection colour as raw code values, 0 to 255 each. Community Shaders reads it as `(code/255)^1.6`. |
| `--roughness <0..1>` | none | Stored in alpha as `min(round(r × 255), 254)`. Left out, alpha is 255 and Community Shaders uses its defaults. |
| `-o`, `--out <file>` | required | The DDS file to write. |

```powershell
sdc cubemap-marker --color 200,200,210 --roughness 0.3 -o textures\cubemaps\steel_e.dds
```

An existing file that Tessalith did not write, or cannot read, is kept, not overwritten.

Exit codes: 0 written; 1 kept (an existing file Tessalith did not write, or one it cannot read); 2 usage error (a bad colour or a roughness outside 0 to 1); 3 the write failed.

## Exit codes

| Code | Meaning |
|---|---|
| 0 | Success. Every file converted (or was up to date or skipped on purpose) and passed validation. |
| 1 | Partial. Some files were unresolved, blocked, failed, cancelled, or had an existing output with no manifest record. |
| 2 | Usage or configuration error: a bad flag, `--set` value, input path, preset or config file. Nothing was converted. |
| 3 | Fatal: an I/O failure on the output root or manifest, an internal error (`sdc: internal error`), or a second Ctrl-C. |
| 4 | Every file was handled, but at least one failed validation. |

When several apply, the highest-precedence one wins: 3, then 2, then 1, then 4, then 0. Intentional skips, up-to-date files and raw maps consumed by packing never cause exit 1.

Errors that stop a run before it starts print `sdc: <message>` to stderr. Examples:

```text
sdc: input not found: C:\Work\missing
sdc: --mem-budget '0': must be greater than 0 (omit the flag for the default, 50% of RAM)
sdc: --fill 'metal=1': unknown role 'metal' (one of roughness, metallic, ao, specular, glossiness, height, opacity, fuzz)
sdc: --allow FOO: unknown code
sdc: --set 'alpha.treshold=100': unknown field 'alpha.treshold'
sdc: unknown preset 'cs-qualty'
```

## JSON event stream

`convert --json` and `inspect --json` write one JSON object per line to stdout. The stream's schema is available with `sdc presets schema event`. The current event schema version is 2.

Every event has these fields:

| Field | Meaning |
|---|---|
| `schema` | Event schema version (2). |
| `seq` | Sequence number within the batch. Events are delivered in `seq` order. |
| `ts` | Timestamp, RFC 3339, UTC, millisecond precision. |
| `batch` | Batch id, the same for every event of one run. |
| `set` | The texture set's id, when the event belongs to a set. |
| `path` | The input file, forward slashes, when the event belongs to a file. |
| `job_key` | The output path relative to the output root. Absent on batch-level events. |
| `kind` | The event type, below. |

Event kinds and their extra fields:

| `kind` | Extra fields |
|---|---|
| `batch_started` | `target`, `preset`, `inputs` (count). |
| `file_discovered` | `output`. |
| `set_resolved` | `members`, `pbr`, `bake`, `recipes` (each: `output`, `subtype`, `channels` [R,G,B,A provenance], `status` run/conflict/failed/disabled, `reason`), `shader` (`shader`, `provenance` = `set`, `auto:<rule>` or `fallback`, `wanted`), `warnings`. |
| `file_planned` | `job`: the full resolved job, including the profile, provenance of every field, suggestions, warnings and status. |
| `file_started` | none. |
| `progress` | `stage`, `done`, `total`. |
| `warning` | `code`, `severity` (info/warning/blocking/error), `message`, `data`. |
| `file_done` | `output`, `validation` (`passed`, `levels`, `min_mip_psnr_db`, `alpha` per level, `coverage` per level, `bc6h_peak_ratio`). |
| `file_skipped` | `skip`, `code`, `reason`. |
| `file_failed` | `stage`, `error`. |
| `batch_done` | `converted`, `up_to_date`, `skipped`, `skipped_intentional`, `failed`, `validation_failed`, `cancelled` (list of inputs). |

`skip` values: `up_to_date`, `on_existing_skip`, `unresolved`, `blocked`, `duplicate`, `dds_input`, `packing_input`, `normal_layer`, `unknown_existing`, `pack_conflict`, `packed_source`.

`stage` values: `discover`, `group`, `load`, `analyse`, `resolve`, `preprocess`, `mipmaps`, `compress`, `write`, `validate`.

A `file_done` line, wrapped here for reading:

```json
{"schema":2,"seq":41,"ts":"2026-10-14T09:12:03.512Z","batch":"19a2f3c7e10-1f40-0",
 "set":"textures/armor/iron/iron","path":"C:/src/textures/armor/iron/iron_m.png",
 "job_key":"textures/armor/iron/iron_m.dds","kind":"file_done",
 "output":"C:/out/textures/armor/iron/iron_m.dds",
 "validation":{"passed":true,"levels":11,"min_mip_psnr_db":41.7,"alpha":[],"coverage":[],"bc6h_peak_ratio":null}}
```

Reading the stream in PowerShell:

```powershell
sdc convert src --recursive --accept-suggestions high --json 2>$null |
  ForEach-Object { $_ | ConvertFrom-Json } |
  Where-Object kind -eq 'file_failed' |
  Select-Object path, stage, error
```

`validate --json` writes a different record, one per file: `kind` = `dds_validation`, `schema` (1), `path`, `readable`, `subtype`, `alpha_policy`, `judged`, `note`, `passed`, `summary`, `warnings`. A file that cannot be read gets a shorter record: `kind`, `schema`, `path`, `readable` (false), `passed` (false) and `error`. `inspect --json` writes DDS info objects for DDS inputs (schema `dds-info`) and the event stream for images.
