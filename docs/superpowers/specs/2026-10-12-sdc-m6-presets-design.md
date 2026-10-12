# sdc M6 Track D — presets: design

Owner-approved in chat on 2026-10-12, section by section. Base: `main` 2666bd1 (Tracks A, B and C, the NIF merge and the column fix are all merged). M5 (NVTT as an external tool) follows this track.

## 1. Intent

**What the owner asked for (2026-10-12):**
- Compact presets: a smaller format for colour maps, and a downscale for data maps. "BC3 is the same size as BC7, so it is not a good example." Data maps halve, with a 512 px floor.
- A user presets folder that loads automatically everywhere.
- Schema migrations: build the hook now.
- Close the carried preset and rule bugs, the suggestion nits, and the BC4/BC5 search-radius revisit.

**Assumptions the owner accepted:**
- Fast preview is the plan's definition: bc7e `fast`, validation of the smallest mip only.
- Normal maps count as data maps.
- The GUI gets no preset picker in this track.

**Success:**
- `sdc convert --preset cs-compact` writes opaque colour maps as BC1 and data maps at half size (never below 512).
- A preset dropped into `%APPDATA%\sdc\presets\` appears in `sdc presets list` and is usable by `--preset <id>` and in the GUI with no flag.
- `presets diff`, `presets export` and `presets validate` work on built-in and user presets.
- Every schema family goes through a migration step before deserialising.
- The carried items listed in §6 are closed by tests.

**Out of scope:**
- a GUI preset picker;
- the CLI progress bar;
- the scheduler and admission revisit;
- changing what the existing `*-quality` presets produce, except for the measured search-radius default in §7.

## 2. New built-in presets

The new files sit in `presets/` and are embedded with the existing `include_str!` list in `crates/engine/src/presets.rs`. Each is `schema_version = 1`, `version = "1.0.0"`, and is visible (not `hidden`).

| id | base | target | Description |
|---|---|---|---|
| `cs-compact` | `cs-quality` | `cs` | "CS – Compact: BC1 for opaque colour, data maps at half size (min 512)." |
| `vanilla-compact` | `vanilla-quality` | `vanilla` | "Vanilla – Compact: BC1 for opaque colour, data maps at half size (min 512)." |
| `fast-preview` | `sdc-base` | none (any target) | "Fast preview: bc7e fast, smallest-mip validation only. For checking a mod quickly, not for release." |

`fast-preview` gets no `target`, so the batch target decides availability. If the registry requires a target, add one `fast-preview` per target instead (`cs-fast-preview`, `vanilla-fast-preview`) and record that in Note T. The plan checks this first.

### 2.1 Compact: BC1 for opaque colour maps

- **Colour sub-types:** every sub-type whose `color.content` resolves to `srgb` in `sdc-base`, except `pbr_emissive`, which stays BC6H.
- **The rule:** preset `[[rules]]` matching `alpha = "none"` (the existing `RuleMatch::alpha`: the source has no alpha, or analysis decided `remove`) and each colour sub-type set `format = "bc1"`.
- **Colour with alpha:** stays at the inherited `bc7`. BC3 is never used, because it is the same size as BC7.
- **No new engine logic,** unless the plan finds that `RuleMatch::alpha` cannot express "resolved alpha policy is `remove`". In that case the smallest extension to `RuleMatch` is allowed and recorded in Note T.
- **`bc1_transparent_black`** stays `false`.

### 2.2 Compact: data maps at half size

- **The new mode:** `ResizeMode::Half`, serialised as `"half"`, alongside Track C's `none` / `pot-down`.
- **Behaviour:**
  - each dimension is halved (integer division);
  - a dimension already at or below 512 is not halved;
  - a dimension above 512 never goes below 512 (`max(d/2, 512)`).
  - The result keeps the source's aspect ratio wherever the floor allows. A non-POT source is first handled as `pot-down` would handle it, then halved, so outputs stay POT.
- **The path:** it reuses Track C's resize in `preprocess_with` and emits `DIM-RESIZED`. It also counts in `api::job_bytes` admission like `pot-down`.
- **Data sub-types:** every sub-type that is not a colour sub-type (§2.1) and not `pbr_emissive`. This covers normals (`normal`, `landscape_normal`, `pbr_normal`, `msn`, `coat_normal`), `envmask`, `complex_material`, `height`, `rmaos`, skin data maps and the other single-channel or packed maps.
- **Packed outputs** (channel packing, §6.4 of the main spec) follow their recipe's output sub-type.
- **Where it is set:** in the compact preset's `[subtypes.<st>] resize = "half"`.

### 2.3 Fast preview

- **Encoder:** `encoder = { effort = "fast", bc45_search_radius = <§7 fast value> }`.
- **Validation:** a new field, `validation.mips = "all" | "smallest"`, with default `"all"`. When it is `"smallest"`, the per-mip PSNR and coverage checks run only on the smallest mip. All other validation is unchanged.
- **Serialisation:** the default is not serialised, as with Track C's defaults, so existing preset hashes and the `default_profiles_hash_as_before` golden do not change.

## 3. User presets folder

- **Where and when:** `%APPDATA%\sdc\presets\*.toml`, non-recursive, in file-name order. It loads on every CLI run (`convert`, `plan`, `inspect`, every `presets` verb) and in the GUI, beside the existing user config at `%APPDATA%\sdc\config.toml`. Tests never read the real folder; the path is injectable, as the user config path is.
- **`--preset-file`** (repeatable) still adds one-off files after the folder.
- **Bad files:** a folder file that fails to parse or validate is skipped. It gets the new code **`CFG-PRESET-SKIPPED`** (Warning), with data `{"file", "reason"}`, and the run continues. A bad `--preset-file` stays a hard error (`CFG-INVALID`), because the user named it.
- **Duplicate ids:** a user or `--preset-file` preset whose id equals a built-in id or an already loaded id is refused, with a message naming both sources.
  - In the folder, that is `CFG-PRESET-SKIPPED`.
  - On `--preset-file`, it is an error.
  - This replaces `add_toml`'s silent overwrite (a carried item).
- **GUI:** the existing preset inspector (`gui/src/ui/advanced.rs`) shows the loaded user presets' ids and files. There is no picker.

## 4. `sdc presets` verbs

`list`, `show`, `diff`, `export` and `validate` all take `--preset-file <file>` (repeatable) and see the user folder.

- **`list`:** user presets appear with their source file. In `--json`, each preset gets a new key `"source": "builtin" | "<path>"`.
- **`diff <a> <b> [--subtype <st>] [--target <t>] [--json]`:**
  - resolves both presets through their chains, as `show` does, for every sub-type (or the one given), and prints only the fields that differ: `<subtype>.<field.path>: <a value> → <b value>`;
  - with no difference, prints `no differences` and exits 0;
  - `--json` gives `[{"subtype", "field", "a", "b"}]`.
- **`export <id> [--out <file>] [--flat]`:**
  - **Default:** the preset's own TOML as loaded. For a built-in, that is its embedded text, so `base =` stays a link.
  - **`--flat`:** a standalone file. It has no `base`, `[defaults]` is the resolved defaults, `[subtypes.*]` holds each sub-type's resolved difference from those defaults, and the rules and aliases come from the chain in order.
  - The new id is `<id>-export` unless `--id <new>` is given. The file must pass `validate`.
  - Without `--out`, it writes to stdout.
- **`validate <file>…`:**
  - for each file, it checks the migration (§5), the parse (`deny_unknown_fields`), the reserved and duplicate ids against built-ins, that `base` exists, the alias checks (`check_aliases`), each rule's sub-type and fields, and that the chain resolves for every target the preset allows;
  - prints `ok <file>`, or one line per problem: `<file>: <reason>`;
  - exits 0 if all files are clean, 1 otherwise.

## 5. Migration hook

- **Where:** a new `crates/engine/src/migrate.rs` with one function per schema family:
  - `migrate_preset(v: toml::Value) -> Result<toml::Value, MigrateError>`;
  - `migrate_config(v: toml::Value) -> Result<toml::Value, MigrateError>`;
  - `migrate_manifest_record(v: serde_json::Value) -> Result<serde_json::Value, MigrateError>`.
- **What each does:** reads `schema_version` (missing = 1 for config; required for presets as today), applies steps `from → from+1` until it reaches the current version, and returns the upgraded value. Each step is a named function in a table, so a later bump adds one entry.
- **Today's steps:**
  - presets: none (v1 is current);
  - config: none (v1 is current);
  - manifest records: v1 → v2 is the identity, because the reader already tolerates the absent fields. The step exists so the table is exercised.
- **Newer than current:** unchanged behaviour. Presets give `PresetError::NewerSchema`, manifest records are kept raw (`is_newer_schema`), and config gives a new `ConfigError::NewerSchema` naming the file.
- **Config files** (`config.toml`, `sdc.toml`) gain an optional `schema_version` key, accepted by `deny_unknown_fields`.
- **Callers:** every loader calls the hook before deserialising: `insert_toml`, the config loaders, and the manifest reader.

## 6. Carried fixes

1. **`add_toml` duplicate or built-in id:** refused (§3).
2. **`merge_json` with an absent key:** it no longer inserts `null` for keys missing from the base. A patch key is inserted with its value, and `null` is written only if the patch says `null`.
3. **Rules vs a `--set` sub-type change:** when `--set subtype:` or `--set file:<glob>.subtype=` changes a file's sub-type, the rule overrides are re-matched against the final sub-type. Rules that named the old sub-type no longer apply.
   - Tests pin the precedence edges: a rule sub-type then `--set` sub-type, `--set` sub-type then a rule for the new sub-type, and `[files]` vs a rule.
4. **`flip_green` vs `green_convention`:** an explicit `channels.flip_green` that contradicts the file's resolved `green_convention` gets the new code **`CFG-GREEN-CONFLICT`** (Warning), with data `{"flip_green", "convention"}`. The explicit field still wins.
5. **Landscape suffix match:** narrowed to a whole path component equal to `landscape`, or a path starting with `textures/landscape/`. So `landscapes_old/` and `mylandscape/` no longer match.
6. **`hdr_sky`:** suggested only for a path with a `sky` component, matching its reason text. It is no longer suggested under `interface/`.

New codes: `CFG-PRESET-SKIPPED`, `CFG-GREEN-CONFLICT`. Code count 73 → 75.

## 7. BC4/BC5 search radius

- **Measure** `bc45_search_radius` 5 against 10, and the lowest useful value for speed, on the existing bench corpus:
  - inputs are read-only;
  - nothing is written to `images3`/`out3` or `results*.csv`;
  - G:/ is read-only;
  - output goes to a new scratch folder.
- **Report:** PSNR and encode time per radius, in a short `docs/spikes/` note.
- **Decision rules:**
  - Quality presets keep the current default unless the larger radius gains ≥ 0.1 dB mean, with ≤ 1.5× encode time.
  - Compact and Fast preview take the smallest radius whose PSNR loss is ≤ 0.1 dB against the current default, or the current default if none qualifies.
- **If a `*-quality` default changes,** the workspace version goes to 0.5.0, so `if-changed` re-encodes. Otherwise version 0.5.0 is still taken for the new presets and fields.

## 8. Docs

- `docs/DECISIONS.md`: a new **Note T (M6 Track D)** with one bullet per ruling: preset ids, BC1 rule mechanics, the half-resize floor, the smallest-mip validation, the folder load order and duplicate refusal, the verbs, the migration table, each carried fix, and the radius decision with numbers.
- `docs/presets-changelog.md` (new; the carried R14 item): one entry per built-in preset version, starting with the three new presets and any changed default.
- The CLI help and `docs/gui.md` (preset inspector) mention the user presets folder.

## 9. Tests

- **Goldens:** one plan case per new preset:
  - `cs-compact` diffuse with no alpha gives `bc1`;
  - with alpha gives `bc7`;
  - a 2048 normal plans 1024;
  - a 512 mask stays 512;
  - a 1024×512 map plans 512×512;
  - `fast-preview` gives effort `fast` and `validation.mips = smallest`.
- **Presets:**
  - folder load (good, bad and duplicate files);
  - `--preset-file` duplicate error;
  - the `list` source key;
  - `diff` output and JSON;
  - `export` default and `--flat` round-trip, which validates and resolves equal to the original;
  - `validate` exit codes.
- **Migrate:** one test per family, where an old or absent version passes through the hook and a newer one is refused or kept raw as today.
- **Carried:** one regression test per item in §6.
- **`default_profiles_hash_as_before`:** stays green, because the new fields default to non-serialised values.

## 10. Review focus

1. A user preset folder with a file that has a Windows BOM or CRLF line endings must load.
2. A preset that bases on a user preset loaded later in file-name order must resolve. Load everything first, then resolve chains.
3. A Compact run on a set with a packed output must not downscale one packing input and not another.
4. `presets export --flat` of a preset with aliases must keep its aliases.
5. A `cs-compact` diffuse whose alpha is analysed as `cutout` must stay BC7 and never become BC1 with 1-bit alpha.
