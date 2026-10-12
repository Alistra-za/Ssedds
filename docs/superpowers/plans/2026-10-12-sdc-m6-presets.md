# sdc M6 Track D — presets Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add three built-in presets (`cs-compact`, `vanilla-compact`, `fast-preview`), a user presets folder that loads everywhere, `sdc presets diff|export|validate`, a schema migration hook for presets, configs and manifest records, the six carried preset/rule/suggestion fixes, and a measured BC4/BC5 search-radius decision.

**Architecture:**
- **Registry.** `PresetRegistry` (`crates/engine/src/presets.rs`):
  - learns where each preset came from (`builtin`, a file path, `inline`);
  - keeps each preset's TOML table (for `export`);
  - refuses a second preset under an id that is already loaded;
  - strips a BOM;
  - loads a folder in two passes: it loads every file, then runs `check_preset` on each and drops failures with `CFG-PRESET-SKIPPED`;
  - validates one file for `presets validate`.
- **Migration.** A new `crates/engine/src/migrate.rs` holds one step table per schema family. `insert_toml`, `ConfigFile::parse`/`load` and `Manifest::load` call it before deserialising.
- **Rules.**
  - `RuleMatch` gains `subtypes` (the sub-type being resolved). It is the only engine extension the BC1 rule needs (ruling T2).
  - A rule that names a sub-type (`Rule::subtype`) applies its overrides only to that final sub-type (carried fix 3).
- **Profile fields.** `ResizeMode::Half` (`resize = "half"`) and `ValidationSpec::mips` (`all` | `smallest`). Both have defaults that are not serialised, so existing preset hashes and `default_profiles_hash_as_before` hold.
- **Plumbing.**
  - `BatchRequest::user_presets: Option<PathBuf>` (the folder) is set by the CLI (`args.rs`) and the GUI (`main.rs`) from `%APPDATA%\sdc\presets`. It is `None` in every test, exactly as `user_config` is.
  - `Engine::setup` loads the folder before `preset_files`. `plan_cached` emits the skip warnings after `batch_started`.
- **Presets.** Three new TOML files, embedded through the existing `include_str!` list. The CLI `presets` verbs build their registry the same way: built-ins, then the folder, then `--preset-file`.
- **Radius.** A bench subcommand measures BC4/BC5 radii on the read-only bench corpus and writes to a scratch folder. Fixed decision rules turn the numbers into preset values (Task 10).

**Tech Stack:** Rust 2021 workspace (`engine`, `cli` = `sdc`, `gui` = `sdc_gui`, `bench` = `sdc-bench`); serde/serde_json/schemars; toml 0.8; clap; egui 0.36.2; rgbcx through `encoders-sys`.

**Spec:** `docs/superpowers/specs/2026-10-12-sdc-m6-presets-design.md` (owner-approved, binding). This includes the owner correction to §2.2 (commit 956da58): `half` keeps the aspect ratio, the 512 floor applies to the larger side, and there is no POT step.

Background:
- the parent spec `docs/superpowers/specs/2026-10-08-skyrim-dds-converter-design.md`:
  - §3: presets, configs, precedence;
  - §3.7: suffix aliases;
  - §5.8: validation;
  - §10: codes;
- `docs/DECISIONS.md` Note S (Tracks B, C, A).

## Global Constraints

- **Base.** `main` at 956da58: the spec correction on top of f4450b9, with code identical to f4450b9. Line numbers below were read at f4450b9. If one has drifted, find the code by the quoted text, not by the number.
- **Branch.** The execution branch is `feat/m6-presets`, created by the controller from 956da58. Implementers do not create, switch or delete branches.
- **Counts.**
  - `Code::ALL.len()` is **73** on `main`. Task 1 makes it **75** (`CFG-PRESET-SKIPPED`, `CFG-GREEN-CONFLICT`).
  - The workspace version is **0.4.0** (`Cargo.toml:7`). Task 11 bumps it to **0.5.0**; spec §7 takes this version in every outcome.
- **Gates.** Run each one unpiped before every commit. All must pass:
  - `cargo fmt --all --check`
  - `cargo clippy --workspace --all-targets --offline -- -D warnings`
  - `cargo clippy --workspace --offline -- -D warnings`
  - `cargo test --workspace --offline`
- **Golden and `gui_repro`.** Both stay green. The task's report names any changed expectation and its reason.
- **Schema.** A task that changes a serialised type (profile, preset, config, event or manifest) regenerates the schemas with `cargo run -p sdc --offline -- presets schema all --out schema` and commits `schema/`.
- **Commits.**
  - Commit only explicit paths (`git add <path> …`, never `-A` or `.`).
  - End every message with `Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>`.
  - Never push.
- **Shell.** Never run `python -` or `python3 -` (stdin heredocs). This plan needs no Python.
- **Off-limits.**
  - `F:/ai/dds/bench` is read-only. Task 10 reads only `images2/mask_real` and `images2/normal_real`. Nothing is ever written under `F:/ai/dds/bench`: no `images*`, no `out*`, no `results*.csv`.
  - `G:/` is read-only.
  - The NIF team's code is off-limits: `crates/nif`, `crates/vfs`, `crates/modscan`, `crates/render3d`, `crates/pgpsim`, the `nif/*` branches, `F:/ai/dds/converter-nif` and DECISIONS Note Q.
- **Defaults are not serialised.**
  - `resize` (`none`) and `validation.mips` (`all`) use `#[serde(default, skip_serializing_if = …)]`.
  - So Tasks 3 and 4 do not change `PresetRegistry::sha256` of any existing preset, `profile_sha256` of any existing profile, or the manifest up-to-date check.
  - `RuleMatch::subtypes` is skipped when empty, for the same reason.
- **The three new presets** are `schema_version = 1`, `version = "1.0.0"`, and visible (not `hidden`). Their descriptions, verbatim from spec §2:
  - `cs-compact`: "CS – Compact: BC1 for opaque colour, every other map except emissive and sky at half size (min 512)."
  - `vanilla-compact`: "Vanilla – Compact: BC1 for opaque colour, every other map except emissive and sky at half size (min 512)."
  - `fast-preview`: "Fast preview: bc7e fast, smallest-mip validation only. For checking a mod quickly, not for release."
- **Out of scope (spec §1).**
  - There is no GUI preset picker, no CLI progress bar, and no scheduler or admission revisit.
  - The `*-quality` presets change only through the measured radius default (Task 10, decision rule Q).
- **Decisions.** Task 11 appends `## Note T — M6 Track D: presets (plan rulings, 2026-10-12)` at the end of `docs/DECISIONS.md`.
  - It goes after Note S's `### M6 Track A` subsection and its "Also (Track A)" bullet.
  - It has one bullet per ruling, with sub-ids `T1` … `T12` (the rulings below).

## Plan rulings

These settle the questions the spec left to the plan. Each one becomes a Note T bullet in Task 11.

- **T1 One `fast-preview`, with no target.**
  - **The registry does not require a target:**
    - `PresetFile::target` is `Option<Target>` (`presets.rs:96`);
    - `resolve_target` walks the chain and falls back to `cs` (`api.rs:443-466`);
    - `presets show` does `.or(preset.target).unwrap_or(Target::Cs)` (`verbs.rs:759-762`);
    - `sdc-base` itself has no target.
  - **So:** `fast-preview` (base `sdc-base`, no `target`) works for any `--target`.
  - **Nothing is lost today:** `cs-quality`, `vanilla-quality` and `cpm-quality` carry only identity keys (`presets/*.toml`), so basing on `sdc-base` loses no setting.
  - **Consequence:** if a `*-quality` preset gains settings later, `fast-preview` does not inherit them.
- **T2 BC1 rule mechanics: `RuleMatch` gains `subtypes`, and alpha matches on content.**
  - **No sub-type matcher exists.** `RuleMatch` (`presets.rs:59-73`) has only `glob`, `suffix`, `set_has` and `alpha`; `Rule::subtype` *assigns* a sub-type rather than matching one. So "each colour sub-type" cannot be written as a rule today.
  - **The extension.** The smallest extension that spec §2.1 allows is `RuleMatch::subtypes: Vec<Subtype>` (empty = any), matched against the sub-type being resolved.
  - **It never decides a sub-type.** `decide_subtype` is deciding the sub-type, so it cannot match a rule that names sub-types.
  - **What `alpha` matches.** `RuleMatch::alpha` matches `Analysis::alpha` (`AlphaContent`: `none`, `constant255`, `varying`; `policy.rs:170`), not the resolved policy.
  - **`alpha = "none"` alone misses opaque RGBA.** A fully opaque RGBA source is `constant255`. Its policy still resolves to `remove`:
    - analysis gives `remove` for `diffuse`, `pbr_albedo`, `ui_sky` and `skin_albedo` (`suggest_alpha`, `suggest.rs:194`);
    - `glow`, `subsurface_tint` and `backlight_tint` keep the base `remove`.
  - **So each compact preset has two rules:** `alpha = "none"` and `alpha = "constant255"`.
  - **No match on the resolved policy.** Rules are a layer that can itself set `alpha.policy`, so matching on the resolved policy would be circular.
  - **Consequence (recorded):**
    - An explicit non-`remove` policy on an opaque colour map under a compact preset (from `--set alpha.policy=cutout` or a user config) meets `format = bc1`. `FMT-BC1-ALPHA` (`policy.rs:1292`) blocks it and names the fix.
    - A source with varying alpha never matches, so it stays BC7 (Review Focus 5).
- **T3 Colour and data sub-types.** In `sdc-base` (`presets/sdc-base.toml`), these sub-types have `color.content = "srgb"`: `diffuse`, `landscape_diffuse`, `pbr_albedo`, `pbr_emissive`, `glow`, `pbr_subsurface`, `pbr_fuzz`, `subsurface_tint`, `backlight_tint`, `ui_sky`, `skin_albedo`. This is exactly `Category::Color` (`config.rs:290-291`).
  - **BC1 under the `alpha = "none"` rule (9):** `diffuse`, `pbr_albedo`, `glow`, `pbr_subsurface`, `pbr_fuzz`, `subsurface_tint`, `backlight_tint`, `ui_sky`, `skin_albedo`.
  - **BC1 under the `alpha = "constant255"` rule (7):** the same list without `pbr_subsurface` and `pbr_fuzz`.
    - Their base policy is `data`, and `suggest_alpha` gives them no `remove` on constant 255.
    - So BC1 on them would be `FMT-BC1-ALPHA` (Blocking).
  - **Excluded from BC1:**
    - `pbr_emissive`: the spec keeps it BC6H.
    - `landscape_diffuse`: `check_profile` blocks BC1 on it with `FMT-LANDSCAPE-BC1` (`policy.rs:1286-1290`), because BC1 destroys the snow/height alpha. It stays BC7 and, as BC7 colour, is halved (owner, 2026-10-12). This is a spec gap, recorded.
  - **BC7 colour also halves (owner, 2026-10-12):** every colour sub-type except `pbr_emissive` (BC6H) gets `resize = "half"` in its `[subtypes.<st>]` table: `diffuse`, `landscape_diffuse`, `pbr_albedo`, `glow`, `pbr_subsurface`, `pbr_fuzz`, `subsurface_tint`, `backlight_tint`, `ui_sky`, `skin_albedo` (10). Both BC1 rules also set `resize = "none"`, so opaque colour that goes to BC1 stays full size. Colour with alpha, `landscape_diffuse` and a constant-255 `pbr_subsurface` or `pbr_fuzz` stay BC7 and halve. `pbr_emissive` and `hdr_sky` stay full size. No new engine logic: a rule override beats the sub-type table.
  - **Data, `resize = "half"` (16):** every other sub-type: `normal`, `landscape_normal`, `pbr_normal`, `msn`, `coat_normal`, `skin_wet`, `envmask`, `height`, `msn_specular`, `skin_sk`, `complex_material`, `rmaos`, `skin_rfaos`, `flow`, `generic_two_channel`, `generic_data`.
    - `hdr_sky` is excluded from resize (owner, 2026-10-12): it is a visible sky colour image, treated like `pbr_emissive` (in neither the BC1 list nor the resize list).
- **T4 `half` (owner correction, 956da58).**
  - **The rule:** `target_dims((w, h), Half)` = `(max(w/2, 1), max(h/2, 1))` when `max(w, h) > 512`, else `(w, h)`.
  - **Shape:** both sides halve together, so the shape never changes.
  - **No POT step:** an NPOT source halves, and `DIM-NPOT` then judges the result as before.
  - **Constant:** `HALF_FLOOR = 512`.
  - **`DIM-RESIZED`** names the mode (`resize = half`).
  - **`job_bytes` admission** counts it through `target_dims`, unchanged (`api.rs:405`).
- **T5 `validation.mips = smallest`.**
  - **Skipped:** the per-mip PSNR check (`VAL-MIP-ERROR`) and the coverage check (`VAL-COVERAGE`) run on the last level only.
  - **Unchanged:** the header, format, alpha-guarantee, exact-opaque and `_m` checks.
  - **Coverage in practice:** the smallest mip is usually below `COVERAGE_MIN_TEXELS`, so coverage is effectively not checked under `fast-preview`. `summary.coverage` then holds one value.
- **T6 Where the injectable user presets path lives.**
  - **The field:** `BatchRequest::user_presets: Option<PathBuf>`, beside `user_config` (`api.rs:64-66`).
  - **Who sets it:**
    - the CLI, in `build_request`, from `args::user_presets_dir()` (`%APPDATA%\sdc\presets`);
    - the GUI, in `main.rs` (`AppModel::user_presets`);
    - the `presets` verbs, which call the same `args::user_presets_dir()`.
  - **Tests never read the real folder:** `BatchRequest::new` and `AppModel::new` leave it `None`, and the CLI tests already point `APPDATA` at a temporary folder.
  - **Folder loading (`PresetRegistry::add_dir`):**
    - It reads `*.toml` files (case-insensitive extension), non-recursively, in file-name order.
    - It loads every file first. Then each loaded id must pass `check_preset`.
    - A failure is removed and reported, and the check repeats, because removing a base orphans its children.
    - A missing folder loads nothing and reports nothing. An unreadable folder gives one `CFG-PRESET-SKIPPED` naming the folder.
  - **The same file reached twice** loads once (canonical path compare). For example, a folder file that is also passed as `--preset-file` or `--preset <file>`.
- **T7 Duplicate ids are refused** (carried fix 1).
  - `insert_toml` refuses any id that is already loaded, built-in or not, with `CFG-INVALID: preset id '<id>' from <new source> is already loaded from <old source>`.
    - In the folder, this is `CFG-PRESET-SKIPPED`.
    - From `--preset-file`, it is a configuration error (exit 2).
  - `is_builtin` becomes "loaded from the embedded list".
  - The branch where reloading a preset replaces its aliases is removed.
- **T8 Verbs.**
  - **`diff`:**
    - It compares profiles from presets alone, as `show` does. Rules are not part of the diff, so `cs-compact` vs `cs-quality` lists only the `resize` rows, not the BC1 rules. This is a spec gap, recorded.
    - Target: `--target`, else `a`'s chain target, else `b`'s, else `cs`.
  - **`export`:**
    - It writes under a new id (`<id>-export`, or `--id`). The original id is always loaded beside it, so `validate` would refuse the original id as a duplicate.
    - The output is the preset's own table as loaded (after migration), re-serialised by `toml`. Comments are not kept; `base` stays a link.
  - **`validate`:**
    - It reports the first problem per file, because the loader stops at the first.
    - A file that is also in the user folder is validated in place of its folder copy.
- **T9 Migration.**
  - **The tables in `migrate.rs`:**
    - presets: none (v1 is current);
    - config: none (v1 is current);
    - manifest records: v1 → v2, which sets `schema = 2` (the reader already tolerates the absent fields).
  - **The manifest version key** is `schema`, not `schema_version` (`manifest.rs:237`).
  - **Bad versions:** version 0 or a non-integer is refused like a missing one.
  - **The config error:** there is no `ConfigError` type, because config loading returns `PresetError`. So the spec's `ConfigError::NewerSchema` is `PresetError::NewerConfigSchema(file, version)`.
  - **Config files** gain an optional `schema_version`, not serialised when absent.
- **T10 Carried fixes.**
  - **#2 `merge_json`.** The two sentences of spec §6 item 2 disagree for a `null` patch on an absent key. The ruling follows the function's documented contract, "a `null` patch changes nothing":
    - an absent key with a `null` patch is not inserted;
    - an absent key with an object patch is inserted without its `null` leaves.
  - **#3 Rule sub-type.**
    - A rule whose `subtype` differs from the final sub-type does not apply, and the next rule of that source is tried.
    - This also drops a rule's overrides when `--allow CFG-UNAVAILABLE` replaces its sub-type with the fallback, because the rule targeted the unavailable sub-type.
  - **#4 Green conflict.** A contradiction now gets `CFG-GREEN-CONFLICT` (Warning), and an explicit `flip_green` is no longer overwritten for OpenGL (`policy.rs:602-607`).
  - **#5 Landscape.** This is already satisfied on `main`. Task 6 only pins it with tests.
    - `suggest.rs:63-64` uses `starts_with("textures/landscape/")` or a whole `landscape` path component (`has_component`, an exact match).
    - `shader.rs:518-522` does the same.
  - **#6 `hdr_sky`.** It now needs a `sky` component (`suggest.rs:126`). A float colour source under `interface/` is `ui_sky`.
- **T11 Radius decision rules** (spec §7, made exact).
  - **Measured:** radii 0, 1, 2, 3, 4, 5 (the current `default`) and 10 (`max`):
    - BC4 on `images2/mask_real` (channel R);
    - BC5 on `images2/normal_real` (R, G).
  - **Per format:** mean PSNR over the images, and total encode time (best of 3).
  - **Q (quality):** the `*-quality` default becomes `max` only if, for **both** BC4 and BC5:
    - mean PSNR(10) − mean PSNR(5) ≥ 0.10 dB, **and**
    - time(10) ≤ 1.5 × time(5).
    - One field covers both formats, so both must qualify.
  - **F (compact, fast preview):** take the smallest r in {0, 1, 2, 3, 4} with mean PSNR(5) − mean PSNR(r) ≤ 0.10 dB for **both** formats.
    - If one exists, add a new `SearchRadius::Fast` (`"fast"`) = r, and the three new presets use it.
    - Otherwise they use the current default. It is written explicitly (`"default"`) only if Q changed the base.
- **T12 Versions.**
  - `sdc-base` goes 1.1.0 → 1.2.0, only if Q changes it.
  - The workspace goes 0.4.0 → 0.5.0 in every outcome (new presets and fields).
  - Codes go 73 → 75.

## Review Focus

1. **A user preset file with a Windows BOM and CRLF line endings** must load, from the folder and from `--preset-file`.
   - Task 1 pins it: `a_preset_with_a_bom_and_crlf_loads`.
   - Task 2 keeps it through the migration parse and adds the config twin, `a_config_with_a_bom_and_crlf_loads`.
2. **A preset that bases on a user preset loaded later in file-name order** must resolve: load everything, then check chains.
   - Task 1 pins it: `the_user_folder_loads_everything_then_checks_and_skips_bad_files` (`a_child.toml` bases on `z_base.toml`).
3. **A compact run on a set with a packed output** must halve the assembled packed image as one image. It must never halve one packing input and not another.
   - Task 7 pins it: `compact_packed_rmaos_halves_as_one_image`. Only `rock_n.dds` and `rock_rmaos.dds` report `DIM-RESIZED`; the raw inputs never do.
4. **`presets export` of a preset with aliases** keeps its aliases and validates, in a registry where the original's base is loaded.
   - Task 8 pins it: `export_round_trips_and_keeps_aliases`.
5. **A `cs-compact` diffuse whose alpha is a cutout** stays BC7 and never becomes BC1.
   - Task 7 pins it: `compact_cutout_diffuse_stays_bc7`.
6. **A fully opaque RGBA colour map** (alpha constant 255, the common exporter output) becomes BC1 under compact. An opaque `pbr_subsurface` with alpha constant 255 is not blocked by `FMT-BC1-ALPHA`.
   - Task 7 pins both: `compact_constant_255_diffuse_is_bc1` and `compact_constant_255_subsurface_stays_bc7`.
7. **A landscape diffuse under compact** stays BC7 and halves (owner, 2026-10-12): no `FMT-LANDSCAPE-BC1`.
   - Task 7 pins it: `compact_landscape_diffuse_stays_bc7_and_halves`.

---

## File map

| File | Responsibility | Tasks |
|---|---|---|
| `crates/engine/src/presets.rs` | sources, tables, duplicate refusal, BOM, `add_dir`, `check_preset`, `remove`, `validate_file`, `export`; the migration call; `RuleMatch::subtypes`; `merge_json`; `ConfigFile::parse` and `schema_version`; `PresetError::NewerConfigSchema`; 3 new `BUILTIN` entries | 1, 2, 3, 7, 8 |
| `crates/engine/src/codes.rs` | `CFG-PRESET-SKIPPED`, `CFG-GREEN-CONFLICT`, count 75 | 1 |
| `crates/engine/src/migrate.rs` (new), `lib.rs` | migration tables and hooks | 2 |
| `crates/engine/src/manifest.rs` | `Manifest::load` calls the hook | 2 |
| `crates/engine/src/policy.rs` | `rule_matches`/`first_match` sub-type matching (#3), flip_green (#4), `DIM-RESIZED` text | 3, 4, 6 |
| `crates/engine/src/config.rs` | `ResizeMode::Half`, `ValidationMips`, `ValidationSpec::mips` (and `SearchRadius::Fast` if Task 10 rule F fires) | 4, 10 |
| `crates/engine/src/resize.rs` | `target_dims` for `Half` | 4 |
| `crates/engine/src/validate/mod.rs` | smallest-mip validation | 4 |
| `crates/engine/src/overrides.rs` | `validation.mips` in `FIELD_PATHS` | 4 |
| `crates/engine/src/api.rs` | `BatchRequest::user_presets`, the folder load in `setup`, the skip events | 5 |
| `crates/engine/src/suggest.rs` | `hdr_sky` needs `sky` (#6), the landscape pins (#5) | 6 |
| `presets/cs-compact.toml`, `presets/vanilla-compact.toml`, `presets/fast-preview.toml` (new) | the three presets | 7, 10 |
| `crates/cli/src/args.rs`, `verbs.rs`, `main.rs` | the folder path, `PresetsArgs`, the `list` source, `diff`, `export`, `validate`, the help | 5, 8, 11 |
| `crates/gui/src/main.rs`, `model/mod.rs`, `model/items.rs`, `ui/advanced.rs`, `ui/settings_panel.rs`, `project.rs` | the folder path, the loaded user presets in the inspector, the `half` option, the config parse | 2, 4, 9 |
| `crates/bench/src/radius.rs` (new), `lib.rs`, `main.rs` | the radius measurement | 10 |
| `crates/encoders-sys/src/rgbcx.rs`, `crates/engine/src/encode.rs` | `BC45_RADIUS_FAST` (rule F only) | 10 |
| tests: `crates/engine/tests/{presets,policy_checks,suffix_aliases,project_files,events_manifest,validate,golden}.rs`, `crates/engine/tests/user_presets.rs` (new), `crates/cli/tests/{cli_presets,cli_convert}.rs`, `crates/gui/tests/advanced.rs` | | 1–10 |
| `docs/DECISIONS.md`, `docs/presets-changelog.md` (new), `docs/gui.md`, `docs/spikes/bc45-search-radius.md` (new), `Cargo.toml`, `Cargo.lock` | Note T, the changelog, the GUI doc, the radius note, 0.5.0 | 10, 11 |

---

### Task 1: Engine — registry sources, duplicate refusal, BOM, the folder loader, `check_preset`, two codes

**Spec:** §3 (folder, bad files, duplicates), §6 item 1, Review Focus 1–2. Rulings T6 and T7.

**Files:**
- Modify: `crates/engine/src/presets.rs`. Change these:
  - the imports at `:1-12`;
  - `PresetRegistry` at `:352-359`;
  - `builtin` at `:362-368`;
  - `add_toml`, `insert_toml` at `:370-398`;
  - `add_file` at `:453-457`;
  - `is_builtin` at `:463-476`.
  New methods go after `selectable` (`:478-480`).
- Modify: `crates/engine/src/codes.rs`:
  - the module doc at `:1-11`;
  - two codes after `CfgFilterIgnored` (`:67`);
  - the tests at `:157-185`.
- Modify: `crates/engine/tests/policy_checks.rs:1236-1253`, where a user preset reuses `cs-quality`, now refused.
- Modify: `crates/engine/tests/suffix_aliases.rs:180-187`, where reloading `ultra` is now refused.
- Test: `crates/engine/tests/presets.rs` (new tests at the end).
- Regenerate: `schema/` (the event schema lists the codes).

**Interfaces:**
- Consumes: nothing new.
- Produces (later tasks rely on these exact names):
  - `pub const SOURCE_BUILTIN: &str = "builtin";` and `pub const SOURCE_INLINE: &str = "inline";`.
  - `impl PresetRegistry`:
    - `pub fn source(&self, id: &str) -> Option<&str>`: `builtin`, `inline` (`add_toml`) or the file path as given (`Path::display`).
    - `pub fn user_presets(&self) -> Vec<(&str, &str)>`: `(id, source)` for every preset whose source is not `builtin`, in id order.
    - `pub fn table(&self, id: &str) -> Option<&toml::Table>`: the preset's TOML table as loaded (Task 8's `export`).
    - `pub fn check_preset(&self, id: &str) -> Result<(), PresetError>`.
    - `pub fn add_dir(&mut self, dir: &Path) -> Vec<Warning>`: the user folder; each warning is `CFG-PRESET-SKIPPED`.
    - `pub(crate) fn remove(&mut self, id: &str)`.
    - `fn same_file(source: &str, path: &Path) -> bool` (private, module level).
  - `Code::CfgPresetSkipped = "CFG-PRESET-SKIPPED"` (Warning), with data `{"file", "reason"}`.
  - `Code::CfgGreenConflict = "CFG-GREEN-CONFLICT"` (Warning), with data `{"flip_green", "convention"}`. It is emitted in Task 6.

- [ ] **Step 1: Write the failing tests** (end of `crates/engine/tests/presets.rs`; add `use std::path::{Path, PathBuf};` to the imports)

```rust
const MINE: &str = "schema_version = 1\nid = \"mine\"\nversion = \"1.0.0\"\nbase = \"cs-quality\"\n";

fn write(dir: &Path, name: &str, text: &str) -> PathBuf {
    std::fs::create_dir_all(dir).unwrap();
    let p = dir.join(name);
    std::fs::write(&p, text).unwrap();
    p
}

/// Carried fix 1 (M6 Track D, Note T7): a second preset under a loaded id is refused, naming both sources.
#[test]
fn duplicate_and_builtin_ids_are_refused_naming_both_sources() {
    let mut r = PresetRegistry::builtin().unwrap();
    r.add_toml(MINE).unwrap();
    let e = r.add_toml(MINE).unwrap_err().to_string();
    assert!(e.starts_with("CFG-INVALID") && e.contains("'mine'") && e.contains("inline"), "{e}");
    let e = r
        .add_toml("schema_version = 1\nid = \"cs-quality\"\nversion = \"9\"\nbase = \"sdc-base\"\n")
        .unwrap_err()
        .to_string();
    assert!(e.contains("already loaded from builtin"), "{e}");
    assert!(r.is_builtin("cs-quality"));
    assert!(!r.is_builtin("mine"));
    assert_eq!(r.source("mine"), Some("inline"));
    assert_eq!(r.source("cs-quality"), Some("builtin"));
    assert_eq!(r.user_presets(), vec![("mine", "inline")]);
    assert_eq!(r.table("mine").unwrap()["base"].as_str(), Some("cs-quality"));
}

/// Review Focus 1: a Windows BOM and CRLF line endings load.
#[test]
fn a_preset_with_a_bom_and_crlf_loads() {
    let mut r = PresetRegistry::builtin().unwrap();
    let text = "\u{feff}schema_version = 1\r\nid = \"crlf\"\r\nversion = \"1.0.0\"\r\nbase = \"cs-quality\"\r\n[subtypes.diffuse]\r\nformat = \"bc1\"\r\n";
    assert_eq!(r.add_toml(text).unwrap(), "crlf");
    assert_eq!(
        r.profile("crlf", Target::Cs, Subtype::Diffuse).unwrap().format,
        Format::Bc1
    );
    let d = tempfile::tempdir().unwrap();
    write(d.path(), "bom.toml", &text.replace("\"crlf\"", "\"bomfile\""));
    assert!(r.add_dir(d.path()).is_empty());
    assert!(r.get("bomfile").is_some());
}

/// Spec §3 and Review Focus 2: everything loads first, then every user preset is checked; a bad one is skipped
/// with CFG-PRESET-SKIPPED, and so is a preset based on a skipped one.
#[test]
fn the_user_folder_loads_everything_then_checks_and_skips_bad_files() {
    let d = tempfile::tempdir().unwrap();
    let dir = d.path();
    write(dir, "a_child.toml", "schema_version = 1\nid = \"child\"\nversion = \"1\"\nbase = \"zbase\"\n");
    write(dir, "b_bad.toml", "this is not toml");
    write(dir, "c_dup.toml", "schema_version = 1\nid = \"cs-quality\"\nversion = \"1\"\nbase = \"sdc-base\"\n");
    write(dir, "d_orphan.toml", "schema_version = 1\nid = \"orphan\"\nversion = \"1\"\nbase = \"missing\"\n");
    write(dir, "e_badfield.toml", "schema_version = 1\nid = \"badfield\"\nversion = \"1\"\nbase = \"cs-quality\"\n[defaults]\nformat = \"bc9\"\n");
    write(dir, "f_on_orphan.toml", "schema_version = 1\nid = \"grandchild\"\nversion = \"1\"\nbase = \"orphan\"\n");
    write(dir, "notes.txt", "not a preset");
    write(&dir.join("sub"), "deep.toml", "schema_version = 1\nid = \"deep\"\nversion = \"1\"\nbase = \"cs-quality\"\n");
    write(dir, "z_base.toml", "schema_version = 1\nid = \"zbase\"\nversion = \"1\"\nbase = \"cs-quality\"\n[subtypes.diffuse]\nformat = \"bc1\"\n");
    let mut r = PresetRegistry::builtin().unwrap();
    let w = r.add_dir(dir);
    assert_eq!(
        r.profile("child", Target::Cs, Subtype::Diffuse).unwrap().format,
        Format::Bc1
    );
    let mut skipped: Vec<String> = w
        .iter()
        .map(|w| {
            assert_eq!((w.code, w.severity), (Code::CfgPresetSkipped, Severity::Warning));
            assert!(w.data["reason"].as_str().is_some_and(|s| !s.is_empty()), "{w:?}");
            let file = w.data["file"].as_str().unwrap();
            file.rsplit(['/', '\\']).next().unwrap().to_string()
        })
        .collect();
    skipped.sort();
    assert_eq!(
        skipped,
        ["b_bad.toml", "c_dup.toml", "d_orphan.toml", "e_badfield.toml", "f_on_orphan.toml"]
    );
    for id in ["orphan", "grandchild", "badfield", "deep"] {
        assert!(r.get(id).is_none(), "{id}");
    }
    assert!(r.is_builtin("cs-quality"));
    assert!(r.source("zbase").unwrap().ends_with("z_base.toml"));
    assert!(PresetRegistry::builtin()
        .unwrap()
        .add_dir(&dir.join("nope"))
        .is_empty());
}

/// Note T6: one file reached twice (the folder and --preset-file) loads once; a copy under the same id does not.
#[test]
fn the_same_file_twice_loads_once() {
    let d = tempfile::tempdir().unwrap();
    let p = write(d.path(), "mine.toml", MINE);
    let mut r = PresetRegistry::builtin().unwrap();
    assert!(r.add_dir(d.path()).is_empty());
    assert_eq!(r.add_file(&p).unwrap(), "mine");
    let copy = write(&d.path().join("copy"), "mine.toml", MINE);
    assert!(r
        .add_file(&copy)
        .unwrap_err()
        .to_string()
        .contains("already loaded from"));
}

#[test]
fn every_builtin_preset_passes_check_preset() {
    let r = PresetRegistry::builtin().unwrap();
    for p in r.selectable() {
        r.check_preset(&p.id).unwrap();
    }
    r.check_preset(BASE_ID).unwrap();
    let mut r = r;
    r.add_toml("schema_version = 1\nid = \"badrule\"\nversion = \"1\"\nbase = \"cs-quality\"\n[[rules]]\nmatch = { glob = \"**\" }\noverrides = { bogus = 1 }\n")
        .unwrap();
    assert!(r.check_preset("badrule").unwrap_err().to_string().contains("bogus"));
}
```

Add `use engine::codes::{Code, Severity};` if the file does not already import `Severity` (it imports both at `:1`).

- [ ] **Step 2: Run them and check that they fail**

Run: `cargo test -p engine --offline --test presets`
Expected: a compile error (`source`, `user_presets`, `table`, `add_dir`, `check_preset` and `Code::CfgPresetSkipped` not found).

- [ ] **Step 3: Add the two codes** (`codes.rs`)

After `CfgFilterIgnored = "CFG-FILTER-IGNORED" => Info,`:

```rust
    CfgPresetSkipped = "CFG-PRESET-SKIPPED" => Warning,
    CfgGreenConflict = "CFG-GREEN-CONFLICT" => Warning,
```

Change the end of the module doc from `… and the four \`SHADER-*\` codes (M6 Track A).` to `… and the four \`SHADER-*\` codes (M6 Track A) and CFG-PRESET-SKIPPED (a user preset folder file that failed to load, skipped) and CFG-GREEN-CONFLICT (an explicit \`flip_green\` that contradicts \`green_convention\`) (M6 Track D).` (keep the doc's line wrapping at 120 columns).

In `codes_serialise_as_their_strings_and_are_unique`, change `assert_eq!(Code::ALL.len(), 73);` to `75`, and add:

```rust
        assert_eq!(Code::CfgPresetSkipped.severity(), Severity::Warning);
        assert_eq!(Code::parse("CFG-GREEN-CONFLICT"), Some(Code::CfgGreenConflict));
```

- [ ] **Step 4: Implement the registry changes** (`presets.rs`)

Imports: change `use crate::codes::Code;` to `use crate::codes::{Code, Warning};` and `use std::path::Path;` to `use std::path::{Path, PathBuf};`.

Below `DEFAULT_PRESET`:

```rust
/// [`PresetRegistry::source`] of an embedded preset.
pub const SOURCE_BUILTIN: &str = "builtin";
/// [`PresetRegistry::source`] of a preset added as text ([`PresetRegistry::add_toml`]).
pub const SOURCE_INLINE: &str = "inline";
```

Replace the struct, `builtin`, `add_toml` and `insert_toml`:

```rust
#[derive(Debug, Clone, Default)]
pub struct PresetRegistry {
    presets: BTreeMap<String, PresetFile>,
    /// Alias → canonical suffix, over every loaded preset.
    aliases: BTreeMap<String, String>,
    /// Alias → id of the preset that declares it.
    alias_owner: BTreeMap<String, String>,
    /// Id → where the preset was loaded from: `builtin`, `inline` or a file path (M6 Track D, Note T7).
    sources: BTreeMap<String, String>,
    /// Id → the preset's TOML table as loaded (`presets export`, Note T8).
    tables: BTreeMap<String, toml::Table>,
}

impl PresetRegistry {
    pub fn builtin() -> Result<Self, PresetError> {
        let mut r = PresetRegistry::default();
        for text in BUILTIN {
            r.insert_toml(text, SOURCE_BUILTIN)?;
        }
        Ok(r)
    }

    /// Load a user preset from text. The internal base id `sdc-base` is reserved: it can be neither replaced nor
    /// extended (`CFG-INVALID`), so the error path of `policy::resolve_file` can always fall back on the built-in base.
    /// An id that is already loaded is refused (Note T7).
    pub fn add_toml(&mut self, text: &str) -> Result<String, PresetError> {
        self.insert_toml(text, SOURCE_INLINE)
    }

    fn insert_toml(&mut self, text: &str, source: &str) -> Result<String, PresetError> {
        // Review Focus 1: a Windows editor's BOM (CRLF is valid TOML).
        let text = text.strip_prefix('\u{feff}').unwrap_or(text);
        let table: toml::Table = toml::from_str(text).map_err(|e| PresetError::Parse(e.to_string()))?;
        let p: PresetFile = toml::Value::Table(table.clone())
            .try_into()
            .map_err(|e: toml::de::Error| PresetError::Parse(e.to_string()))?;
        if source != SOURCE_BUILTIN && p.id == BASE_ID {
            return Err(PresetError::Alias(format!(
                "CFG-INVALID: preset id '{BASE_ID}' is reserved for the built-in base profile"
            )));
        }
        if p.schema_version > SCHEMA_VERSION {
            return Err(PresetError::NewerSchema(p.schema_version));
        }
        if let Some(first) = self.sources.get(&p.id) {
            return Err(PresetError::Alias(format!(
                "CFG-INVALID: preset id '{}' from {source} is already loaded from {first}",
                p.id
            )));
        }
        self.check_aliases(&p)?;
        let id = p.id.clone();
        for (alias, canonical) in &p.suffix_aliases {
            self.aliases.insert(alias.clone(), canonical.clone());
            self.alias_owner.insert(alias.clone(), id.clone());
        }
        self.sources.insert(id.clone(), source.to_string());
        self.tables.insert(id.clone(), table);
        self.presets.insert(id.clone(), p);
        Ok(id)
    }
```

(The old "Reloading a preset id replaces its aliases" block is gone: an id can no longer be loaded twice.)

Replace `add_file`:

```rust
    /// Load a user preset file. The same file already loaded (e.g. from the user folder) is not loaded again
    /// (Note T6); another file under a loaded id is refused (Note T7).
    pub fn add_file(&mut self, path: &Path) -> Result<String, PresetError> {
        if let Some((id, _)) = self.sources.iter().find(|(_, s)| same_file(s, path)) {
            return Ok(id.clone());
        }
        let text = std::fs::read_to_string(path)
            .map_err(|e| PresetError::Parse(format!("{}: {e}", path.display())))?;
        self.insert_toml(&text, &path.display().to_string())
            .map_err(|e| match e {
                PresetError::Parse(m) => PresetError::Parse(format!("{}: {m}", path.display())),
                e => e,
            })
    }
```

Replace `is_builtin`:

```rust
    /// True when `id` was loaded from the embedded list (a user preset can never take a built-in id, Note T7).
    pub fn is_builtin(&self, id: &str) -> bool {
        self.source(id) == Some(SOURCE_BUILTIN)
    }
```

After `selectable`:

```rust
    /// Where `id` was loaded from: [`SOURCE_BUILTIN`], [`SOURCE_INLINE`] or the file path.
    pub fn source(&self, id: &str) -> Option<&str> {
        self.sources.get(id).map(String::as_str)
    }

    /// `(id, source)` of every preset not loaded from the embedded list, in id order.
    pub fn user_presets(&self) -> Vec<(&str, &str)> {
        self.sources
            .iter()
            .filter(|(_, s)| *s != SOURCE_BUILTIN)
            .map(|(id, s)| (id.as_str(), s.as_str()))
            .collect()
    }

    /// The preset's TOML table as loaded.
    pub fn table(&self, id: &str) -> Option<&toml::Table> {
        self.tables.get(id)
    }

    /// Forget a preset and its aliases (a user folder file that failed its check).
    pub(crate) fn remove(&mut self, id: &str) {
        self.presets.remove(id);
        self.sources.remove(id);
        self.tables.remove(id);
        let owner = &self.alias_owner;
        self.aliases
            .retain(|a, _| owner.get(a).map(String::as_str) != Some(id));
        self.alias_owner.retain(|_, o| o != id);
    }

    /// Spec §3/§4 (`presets validate`, the user folder): the chain resolves, every rule override names a known field,
    /// and every sub-type's profile, alone and with each of `id`'s own rules applied, deserialises, for each target
    /// the chain allows (its nearest `target`, else all three).
    pub fn check_preset(&self, id: &str) -> Result<(), PresetError> {
        let chain = self.chain(id)?;
        let own = *chain.last().expect("a chain holds its own preset");
        for (i, r) in own.rules.iter().enumerate() {
            let mut paths = vec![];
            leaf_paths(&r.overrides, "", &mut paths);
            if let Some(f) = paths
                .iter()
                .find(|f| !crate::overrides::FIELD_PATHS.contains(&f.as_str()))
            {
                return Err(PresetError::Invalid(format!(
                    "preset '{id}' rule #{i}: unknown field '{f}'"
                )));
            }
        }
        let targets = match chain.iter().rev().find_map(|p| p.target) {
            Some(t) => vec![t],
            None => vec![Target::Cs, Target::Cpm, Target::Vanilla],
        };
        for t in targets {
            for st in Subtype::ALL {
                let mut doc = Value::Object(Map::new());
                for (_, patch) in Self::layers_of(&chain, st) {
                    merge_json(&mut doc, &patch);
                }
                finish_profile(doc.clone(), t, st).map_err(|e| {
                    PresetError::Invalid(format!("preset '{id}', {} under {t:?}: {e}", st.id()))
                })?;
                for (i, r) in own.rules.iter().enumerate() {
                    let mut d = doc.clone();
                    merge_json(&mut d, &r.overrides);
                    finish_profile(d, t, st).map_err(|e| {
                        PresetError::Invalid(format!(
                            "preset '{id}' rule #{i}, {} under {t:?}: {e}",
                            st.id()
                        ))
                    })?;
                }
            }
        }
        Ok(())
    }

    /// The user presets folder (spec §3, Note T6): every `*.toml` directly in `dir`, in file-name order. Every file is
    /// loaded first, then each loaded preset must pass [`PresetRegistry::check_preset`], so a preset may base on one
    /// later in the order (Review Focus 2). A file that fails is skipped with CFG-PRESET-SKIPPED, and so is a preset
    /// based on a skipped one. A missing folder loads nothing.
    pub fn add_dir(&mut self, dir: &Path) -> Vec<Warning> {
        let skip = |file: &Path, reason: String| {
            Warning::new(
                Code::CfgPresetSkipped,
                format!("user preset {} skipped: {reason}", file.display()),
            )
            .with_data(serde_json::json!({ "file": file.display().to_string(), "reason": reason }))
        };
        let mut files: Vec<PathBuf> = match std::fs::read_dir(dir) {
            Ok(rd) => rd
                .filter_map(|e| e.ok().map(|e| e.path()))
                .filter(|p| {
                    p.is_file() && p.extension().is_some_and(|e| e.eq_ignore_ascii_case("toml"))
                })
                .collect(),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return vec![],
            Err(e) => return vec![skip(dir, e.to_string())],
        };
        files.sort();
        let mut warnings = vec![];
        let mut loaded = vec![];
        for f in files {
            match self.add_file(&f) {
                Ok(id) => loaded.push((id, f)),
                Err(e) => warnings.push(skip(&f, e.to_string())),
            }
        }
        // Removing a preset can orphan one based on it, so check again until nothing fails.
        while let Some((i, e)) = loaded
            .iter()
            .enumerate()
            .find_map(|(i, (id, _))| self.check_preset(id).err().map(|e| (i, e)))
        {
            let (id, f) = loaded.remove(i);
            self.remove(&id);
            warnings.push(skip(&f, e.to_string()));
        }
        warnings
    }
```

At module level (after `leaf_paths`):

```rust
/// Whether a registry source names the file `path` (both canonicalised; `builtin`/`inline` never do).
fn same_file(source: &str, path: &Path) -> bool {
    source != SOURCE_BUILTIN
        && source != SOURCE_INLINE
        && matches!(
            (std::fs::canonicalize(source), std::fs::canonicalize(path)),
            (Ok(a), Ok(b)) if a == b
        )
}
```

Keep `check_aliases` unchanged. Its `.filter(|o| **o != p.id)` is now redundant but harmless.

- [ ] **Step 5: Update the two tests that loaded an id twice**

In `crates/engine/tests/policy_checks.rs`, replace the block from `// a user preset loaded under a built-in id is not built in` to the end of `builtin_presets_never_confirm_mesh_alpha` with the following. It keeps the test's intent (a user preset's alpha policy counts as explicit) under a new id:

```rust
    // A user preset may not reuse a built-in id (M6 Track D, Note T7) …
    let mut reg = PresetRegistry::builtin().unwrap();
    assert!(reg
        .add_toml("schema_version = 1\nid = \"cs-quality\"\nversion = \"9.0.0\"\nbase = \"sdc-base\"\n")
        .is_err());
    assert!(reg.is_builtin("cs-quality"));
    // … and a user preset's alpha policy is a choice.
    reg.add_toml("schema_version = 1\nid = \"mine\"\nversion = \"9.0.0\"\nbase = \"sdc-base\"\n[subtypes.diffuse]\nalpha = { policy = \"blend\" }\n").unwrap();
    assert!(!reg.is_builtin("mine"));
    let overrides = sets(&["subtype=diffuse"]);
    let ctx = ResolveCtx {
        target: Target::Cs,
        registry: &reg,
        preset: "mine",
        user_config: None,
        project_config: None,
        overrides: &overrides,
        accept: AcceptSuggestions::High,
        allow: &allow,
        files: &NO_FILE_SCOPES,
        shaders: &NO_SHADERS,
    };
    let j = resolve_in(&ctx, "a/x.png", None, &img);
    assert_eq!(j.status, JobStatus::Ready, "{:?}", j.warnings);
}
```

In `crates/engine/tests/suffix_aliases.rs` (`registry_validates_and_looks_up_aliases`), replace the four lines from `// reloading the same preset id replaces its aliases without a self-conflict` to `assert_eq!(r.alias_for("rmaosmax"), Some(("rmaos", "ultra")));` with:

```rust
    // the same preset id cannot be loaded twice, so its aliases stay (M6 Track D, Note T7)
    assert!(r.add_toml(ULTRA).is_err());
    assert!(r
        .add_toml(&preset_with("ultra", "rmaosmax = \"rmaos\""))
        .is_err());
    assert_eq!(r.alias_table().len(), 3);
    assert_eq!(r.alias_for("rmaosultrahigh"), Some(("rmaos", "ultra")));
    assert_eq!(r.alias_for("rmaosmax"), None);
```

- [ ] **Step 6: Run the tests and check that they pass**

Run: `cargo test -p engine --offline --test presets --test policy_checks --test suffix_aliases` and `cargo test -p engine --offline --lib codes::`
Expected: PASS. Then run the whole workspace (`cargo test --workspace --offline`). Another test may load one id twice; `grep -rn "add_toml\|add_file" crates/*/tests` lists the callers. If one fails with "already loaded", give the second load its own id. Do not weaken the refusal. Name each such test in the report.

- [ ] **Step 7: Regenerate the schemas, run the gates, commit**

```bash
cargo run -p sdc --offline -- presets schema all --out schema
git add crates/engine/src/presets.rs crates/engine/src/codes.rs crates/engine/tests/presets.rs crates/engine/tests/policy_checks.rs crates/engine/tests/suffix_aliases.rs schema
git commit -m "feat(presets): preset sources, duplicate ids refused, BOM, user folder loader, check_preset; CFG-PRESET-SKIPPED, CFG-GREEN-CONFLICT (M6 Track D)

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

---

### Task 2: Engine — the migration hook (`migrate.rs`) and its three callers

**Spec:** §5. Ruling T9. Review Focus 1 (the config twin).

**Files:**
- Create: `crates/engine/src/migrate.rs`.
- Modify: `crates/engine/src/lib.rs`: add `pub mod migrate;` in alphabetical order, between `manifest` and `mips`.
- Modify: `crates/engine/src/presets.rs`:
  - the start of `insert_toml` (Task 1);
  - `PresetError` (`:33-49`), which gains a variant;
  - `ConfigFile` (`:114-135`), which gains `schema_version`;
  - `ConfigFile::load` (`:141-145`), which becomes `parse` plus `load`.
- Modify: `crates/engine/src/manifest.rs:327-343` (`Manifest::load`).
- Modify: `crates/gui/src/project.rs:190-192` (`ProjectDoc::config`): parse through `ConfigFile::parse`.
- Test: unit tests at the bottom of `migrate.rs`; `crates/engine/tests/presets.rs`; `crates/engine/tests/events_manifest.rs`.
- Regenerate: `schema/` (config gains `schema_version`).

**Interfaces:**
- Consumes: `presets::SCHEMA_VERSION` (1) and `manifest::MANIFEST_SCHEMA` (2).
- Produces:
  - `pub const CONFIG_SCHEMA: u32 = 1;`
  - `pub enum MigrateError { NoVersion(&'static str), Newer { key: &'static str, found: u32, current: u32 } }`. It derives `Debug, Clone, PartialEq, Eq, thiserror::Error`.
  - `pub fn migrate_preset(v: toml::Value) -> Result<toml::Value, MigrateError>`
  - `pub fn migrate_config(v: toml::Value) -> Result<toml::Value, MigrateError>`
  - `pub fn migrate_manifest_record(v: serde_json::Value) -> Result<serde_json::Value, MigrateError>`
  - `PresetError::NewerConfigSchema(String, u32)` (file, version): the spec's `ConfigError::NewerSchema`.
  - `ConfigFile::schema_version: Option<u32>`.
  - `pub fn ConfigFile::parse(text: &str, origin: &str) -> Result<ConfigFile, PresetError>`.

- [ ] **Step 1: Write the failing tests**

Bottom of the new `crates/engine/src/migrate.rs` (create the file with only this module for now, plus `pub mod migrate;` in `lib.rs`):

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn t(s: &str) -> toml::Value {
        toml::Value::Table(toml::from_str(s).unwrap())
    }

    #[test]
    fn presets_need_a_version_and_refuse_a_newer_one() {
        let v1 = t("schema_version = 1\nid = \"x\"");
        assert_eq!(migrate_preset(v1.clone()), Ok(v1));
        assert_eq!(
            migrate_preset(t("id = \"x\"")),
            Err(MigrateError::NoVersion("schema_version"))
        );
        assert_eq!(
            migrate_preset(t("schema_version = 0")),
            Err(MigrateError::NoVersion("schema_version"))
        );
        assert_eq!(
            migrate_preset(t("schema_version = \"1\"")),
            Err(MigrateError::NoVersion("schema_version"))
        );
        assert_eq!(
            migrate_preset(t("schema_version = 2")),
            Err(MigrateError::Newer { key: "schema_version", found: 2, current: 1 })
        );
    }

    #[test]
    fn configs_default_to_version_1() {
        let none = t("preset = \"cs-quality\"");
        assert_eq!(migrate_config(none.clone()), Ok(none));
        let v1 = t("schema_version = 1");
        assert_eq!(migrate_config(v1.clone()), Ok(v1));
        assert_eq!(
            migrate_config(t("schema_version = 7")),
            Err(MigrateError::Newer { key: "schema_version", found: 7, current: CONFIG_SCHEMA })
        );
    }

    #[test]
    fn manifest_records_step_from_1_to_2_and_newer_ones_are_refused() {
        assert_eq!(
            migrate_manifest_record(json!({"schema": 1, "output": "a.dds"})),
            Ok(json!({"schema": 2, "output": "a.dds"}))
        );
        let v2 = json!({"schema": 2, "output": "a.dds"});
        assert_eq!(migrate_manifest_record(v2.clone()), Ok(v2));
        assert!(matches!(
            migrate_manifest_record(json!({"schema": 3})),
            Err(MigrateError::Newer { found: 3, current: 2, .. })
        ));
        assert_eq!(
            migrate_manifest_record(json!({"output": "a.dds"})),
            Err(MigrateError::NoVersion("schema"))
        );
    }
}
```

End of `crates/engine/tests/presets.rs`:

```rust
/// Spec §5: a config's schema_version is optional (1), a newer one is refused naming the file.
#[test]
fn config_schema_version_is_optional_and_a_newer_one_names_the_file() {
    let d = tempfile::tempdir().unwrap();
    let p = d.path().join("sdc.toml");
    std::fs::write(&p, "schema_version = 1\npreset = \"cs-quality\"\n").unwrap();
    assert_eq!(ConfigFile::load(&p).unwrap().schema_version, Some(1));
    std::fs::write(&p, "schema_version = 2\n").unwrap();
    match ConfigFile::load(&p) {
        Err(e @ PresetError::NewerConfigSchema(_, 2)) => {
            assert!(e.to_string().contains("sdc.toml"), "{e}")
        }
        other => panic!("{other:?}"),
    }
    // Absent: not serialised back.
    let c = ConfigFile::parse("preset = \"cs-quality\"\n", "x").unwrap();
    assert!(serde_json::to_value(&c).unwrap().get("schema_version").is_none());
}

/// Review Focus 1, config twin.
#[test]
fn a_config_with_a_bom_and_crlf_loads() {
    let c = ConfigFile::parse("\u{feff}preset = \"cs-quality\"\r\n[defaults]\r\nformat = \"bc7\"\r\n", "user config").unwrap();
    assert_eq!(c.preset.as_deref(), Some("cs-quality"));
}

#[test]
fn a_newer_preset_is_still_newer_schema() {
    let mut r = PresetRegistry::builtin().unwrap();
    assert_eq!(
        r.add_toml("schema_version = 3\nid = \"x\"\n"),
        Err(PresetError::NewerSchema(3))
    );
    assert!(matches!(r.add_toml("id = \"x\"\nversion = \"1\"\n"), Err(PresetError::Parse(_))));
}
```

End of `crates/engine/tests/events_manifest.rs` (add `use engine::manifest::Manifest;` if it is not imported):

```rust
/// Spec §5: every manifest record passes the migration hook; a newer one is kept raw, a line without a schema is
/// not a record.
#[test]
fn manifest_records_pass_the_migration_hook() {
    let d = tempfile::tempdir().unwrap();
    let m = Manifest::at(d.path());
    std::fs::create_dir_all(m.path().parent().unwrap()).unwrap();
    std::fs::write(
        m.path(),
        "{\"schema\":1,\"output\":\"a.dds\"}\n{\"schema\":9,\"output\":\"b.dds\",\"x\":1}\n{\"output\":\"c.dds\"}\n",
    )
    .unwrap();
    let e = m.load().unwrap();
    assert_eq!(e["a.dds"].schema, 2);
    assert!(!e["a.dds"].is_newer_schema());
    assert!(e["b.dds"].is_newer_schema());
    assert_eq!(e["b.dds"].raw["x"], 1);
    assert!(!e.contains_key("c.dds"));
}
```

- [ ] **Step 2: Run them and check that they fail**

Run: `cargo test -p engine --offline --lib migrate::` and `cargo test -p engine --offline --test presets --test events_manifest`
Expected: compile errors (`migrate_preset`, `ConfigFile::parse`, `schema_version`, `NewerConfigSchema` not found). `manifest_records_pass_the_migration_hook` fails on `schema == 2` once the rest compiles.

- [ ] **Step 3: Implement `migrate.rs`** (above the test module)

```rust
//! M6 Track D (spec 2026-10-12-sdc-m6-presets-design.md §5, DECISIONS Note T9): one migration hook per schema
//! family. Each reads the stored version, applies the family's steps from that version up to the current one, and
//! returns the upgraded value; the loaders deserialise only what a hook returns. A later bump adds one named function
//! to its family's table.
use crate::manifest::MANIFEST_SCHEMA;
use crate::presets::SCHEMA_VERSION as PRESET_SCHEMA;

/// Version of `config.toml` / `sdc.toml`; an absent `schema_version` is 1.
pub const CONFIG_SCHEMA: u32 = 1;

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum MigrateError {
    #[error("missing or invalid {0} (a whole number from 1)")]
    NoVersion(&'static str),
    #[error("{key} {found} is newer than this sdc supports ({current})")]
    Newer {
        key: &'static str,
        found: u32,
        current: u32,
    },
}

type TomlStep = fn(toml::Value) -> toml::Value;
type JsonStep = fn(serde_json::Value) -> serde_json::Value;

/// Entry `i` upgrades version `i + 1` to `i + 2`. Presets: v1 is current.
const PRESET_STEPS: &[TomlStep] = &[];
/// Configs: v1 is current.
const CONFIG_STEPS: &[TomlStep] = &[];
/// Manifest records: v1 → v2.
const MANIFEST_STEPS: &[JsonStep] = &[manifest_v1_to_v2];

// Every table reaches its family's current version.
const _: () = assert!(PRESET_STEPS.len() as u32 == PRESET_SCHEMA - 1);
const _: () = assert!(CONFIG_STEPS.len() as u32 == CONFIG_SCHEMA - 1);
const _: () = assert!(MANIFEST_STEPS.len() as u32 == MANIFEST_SCHEMA - 1);

/// v1 → v2: the reader already tolerates every field v2 added (`ManifestEntry::from_value` reads leniently), so
/// only the version changes.
fn manifest_v1_to_v2(mut v: serde_json::Value) -> serde_json::Value {
    v["schema"] = 2.into();
    v
}

fn run<V>(
    mut v: V,
    from: u32,
    current: u32,
    key: &'static str,
    steps: &[fn(V) -> V],
) -> Result<V, MigrateError> {
    if from == 0 {
        return Err(MigrateError::NoVersion(key));
    }
    if from > current {
        return Err(MigrateError::Newer { key, found: from, current });
    }
    for step in &steps[(from - 1) as usize..(current - 1) as usize] {
        v = step(v);
    }
    Ok(v)
}

fn toml_version(v: &toml::Value, key: &'static str, absent: Option<u32>) -> Result<u32, MigrateError> {
    match v.get(key) {
        None => absent.ok_or(MigrateError::NoVersion(key)),
        Some(x) => x
            .as_integer()
            .and_then(|i| u32::try_from(i).ok())
            .ok_or(MigrateError::NoVersion(key)),
    }
}

/// A preset file's table; `schema_version` is required.
pub fn migrate_preset(v: toml::Value) -> Result<toml::Value, MigrateError> {
    let from = toml_version(&v, "schema_version", None)?;
    run(v, from, PRESET_SCHEMA, "schema_version", PRESET_STEPS)
}

/// A config file's table; an absent `schema_version` is 1.
pub fn migrate_config(v: toml::Value) -> Result<toml::Value, MigrateError> {
    let from = toml_version(&v, "schema_version", Some(1))?;
    run(v, from, CONFIG_SCHEMA, "schema_version", CONFIG_STEPS)
}

/// One manifest line; its version key is `schema`. A newer record is refused here and kept raw by the caller
/// (`ManifestEntry::is_newer_schema`).
pub fn migrate_manifest_record(v: serde_json::Value) -> Result<serde_json::Value, MigrateError> {
    let from = v
        .get("schema")
        .and_then(serde_json::Value::as_u64)
        .map(|s| u32::try_from(s).unwrap_or(u32::MAX))
        .ok_or(MigrateError::NoVersion("schema"))?;
    run(v, from, MANIFEST_SCHEMA, "schema", MANIFEST_STEPS)
}
```

- [ ] **Step 4: Call the hook from the three loaders**

`presets.rs`:
- Add the variant to `PresetError`, after `NewerSchema`:

```rust
    /// Spec §5 (`ConfigError::NewerSchema`, Note T9): a config file of a newer schema; names the file.
    #[error("{0}: config schema_version {1} is newer than this sdc supports ({})", crate::migrate::CONFIG_SCHEMA)]
    NewerConfigSchema(String, u32),
```

- In `insert_toml`, replace the two parse statements (the `table` and `p` lines from Task 1) with the following. Keep the BOM line above them:

```rust
        let raw: toml::Table = toml::from_str(text).map_err(|e| PresetError::Parse(e.to_string()))?;
        // Spec §5: the migration hook runs before deserialising.
        let migrated = crate::migrate::migrate_preset(toml::Value::Table(raw)).map_err(|e| match e {
            crate::migrate::MigrateError::Newer { found, .. } => PresetError::NewerSchema(found),
            e => PresetError::Parse(e.to_string()),
        })?;
        let toml::Value::Table(table) = migrated else {
            return Err(PresetError::Parse("a preset file is a TOML table".into()));
        };
        let p: PresetFile = toml::Value::Table(table.clone())
            .try_into()
            .map_err(|e: toml::de::Error| PresetError::Parse(e.to_string()))?;
```

  The `p.schema_version > SCHEMA_VERSION` check below it is now unreachable. Delete it: the hook refuses a newer version first.

- In `ConfigFile`, add this as the first field:

```rust
    /// Schema of this file (spec §5); absent = 1. Not written back when absent.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub schema_version: Option<u32>,
```

- Replace `ConfigFile::load`:

```rust
    pub fn load(path: &Path) -> Result<ConfigFile, PresetError> {
        let text = std::fs::read_to_string(path)
            .map_err(|e| PresetError::Parse(format!("{}: {e}", path.display())))?;
        Self::parse(&text, &path.display().to_string())
    }

    /// A config file's text (`origin` names it in errors): BOM stripped, the migration hook (spec §5), then
    /// deserialised.
    pub fn parse(text: &str, origin: &str) -> Result<ConfigFile, PresetError> {
        let text = text.strip_prefix('\u{feff}').unwrap_or(text);
        let bad = |m: String| PresetError::Parse(format!("{origin}: {m}"));
        let raw: toml::Table = toml::from_str(text).map_err(|e| bad(e.to_string()))?;
        let migrated = crate::migrate::migrate_config(toml::Value::Table(raw)).map_err(|e| match e {
            crate::migrate::MigrateError::Newer { found, .. } => {
                PresetError::NewerConfigSchema(origin.to_string(), found)
            }
            e => bad(e.to_string()),
        })?;
        migrated
            .try_into()
            .map_err(|e: toml::de::Error| bad(e.to_string()))
    }
```

  If a `ConfigFile { … }` literal does not end in `..Default::default()`, add `schema_version: None`. Check `api.rs:2302` and `tests/policy_checks.rs:1364`.

`manifest.rs`, in `Manifest::load`, replace the body of the `for line` loop:

```rust
        for line in io::BufReader::new(f).split(b'\n') {
            let Ok(raw) = serde_json::from_slice::<serde_json::Value>(&line?) else {
                continue;
            };
            // Spec §5: every record passes the migration hook; a newer one is kept raw (`is_newer_schema`).
            let raw = match crate::migrate::migrate_manifest_record(raw.clone()) {
                Ok(v) => v,
                Err(crate::migrate::MigrateError::Newer { .. }) => raw,
                Err(_) => continue,
            };
            if let Some(e) = ManifestEntry::from_value(raw) {
                map.insert(e.output.clone(), e);
            }
        }
```

`crates/gui/src/project.rs`, in `ProjectDoc::config`:

```rust
    /// The document as the engine parses it (migration hook included, spec §5).
    pub fn config(&self) -> Result<ConfigFile, String> {
        ConfigFile::parse(&self.doc.to_string(), "sdc.toml").map_err(|e| e.to_string())
    }
```

- [ ] **Step 5: Run the tests and check that they pass**

Run: `cargo test -p engine --offline --lib migrate::`, `cargo test -p engine --offline --test presets --test events_manifest`, then `cargo test --workspace --offline`.
Expected: PASS.
- `config_file_loads_and_rejects_unknown_keys` (`tests/presets.rs:217`) must still pass. Unknown keys are still `PresetError::Parse`, and the message still names the key.
- A test that read a stored v1 record's `schema` as 1 now sees 2. Change it to expect 2, and name it in the report.

- [ ] **Step 6: Regenerate the schemas, run the gates, commit**

```bash
cargo run -p sdc --offline -- presets schema all --out schema
git add crates/engine/src/migrate.rs crates/engine/src/lib.rs crates/engine/src/presets.rs crates/engine/src/manifest.rs crates/gui/src/project.rs crates/engine/tests/presets.rs crates/engine/tests/events_manifest.rs schema
git commit -m "feat(engine): migration hook per schema family; presets, configs and manifest records pass it before deserialising (M6 Track D spec §5)

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

---

### Task 3: Engine — `RuleMatch::subtypes`, rules re-matched on the final sub-type, `merge_json` without nulls

**Spec:** §2.1 (the smallest `RuleMatch` extension), §6 items 2 and 3 with their precedence-edge tests. Rulings T2 and T10 (#2, #3).

**Files:**
- Modify: `crates/engine/src/presets.rs`:
  - `RuleMatch` (`:59-73`), which gains `subtypes`;
  - `merge_json` (`:321-332`).
- Modify: `crates/engine/src/policy.rs`:
  - `rule_matches` (`:162-171`);
  - `first_match` (`:189-197`);
  - the call in `decide_subtype` (`:316`);
  - the call in `layered_profile_with` (`:522`).
- Test: `crates/engine/tests/presets.rs`, `crates/engine/tests/policy_checks.rs` (uses `run_with`, `:290`), `crates/engine/tests/project_files.rs` (uses `fixture`, `req`, `plan`, `job`, `prov`).
- Regenerate: `schema/` (preset and config schemas: `RuleMatch`).

**Interfaces:**
- Consumes: nothing new.
- Produces:
  - `RuleMatch::subtypes: Vec<Subtype>`. It is serialised as `subtypes`, has `#[serde(default, skip_serializing_if = "Vec::is_empty")]`, and empty means any. Task 7's presets use it.
  - `pub fn rule_matches(m: &RuleMatch, f: &FileFacts, st: Option<Subtype>) -> bool`. `st` is the sub-type being resolved; `None` means it is being decided.

- [ ] **Step 1: Write the failing tests**

`crates/engine/tests/presets.rs`:

```rust
/// Carried fix 2 (Note T10): a null patch neither erases nor adds a key.
#[test]
fn null_patches_never_add_absent_keys() {
    let mut a = json!({"format": "bc7"});
    merge_json(
        &mut a,
        &json!({"header": null, "alpha": {"policy": null, "threshold": 5}, "format": null}),
    );
    assert_eq!(a, json!({"format": "bc7", "alpha": {"threshold": 5}}));
}

/// Note T2: `match.subtypes` round-trips and is not serialised when empty (existing preset hashes hold).
#[test]
fn rule_match_subtypes_is_optional() {
    let r: Rule = toml::from_str("match = { alpha = \"none\", subtypes = [\"diffuse\", \"glow\"] }\noverrides = { format = \"bc1\" }\n").unwrap();
    assert_eq!(r.matcher.subtypes, vec![Subtype::Diffuse, Subtype::Glow]);
    let plain = serde_json::to_value(RuleMatch::default()).unwrap();
    assert!(plain.get("subtypes").is_none());
}
```

`crates/engine/tests/policy_checks.rs` (at the end; `run_with` resolves `x/<rel>` under `cs` with `--accept-suggestions high` and an opaque RGBA source):

```rust
fn project_rules(rules: &str) -> ConfigFile {
    ConfigFile::parse(rules, "sdc.toml").unwrap()
}

/// Carried fix 3 (Note T10): a rule's sub-type then a `--set` sub-type — the rule's overrides no longer apply.
#[test]
fn a_set_subtype_drops_the_overrides_of_a_rule_that_named_another_subtype() {
    let reg = PresetRegistry::builtin().unwrap();
    let project = project_rules(
        "[[rules]]\nmatch = { glob = \"**/r/*.png\" }\nsubtype = \"height\"\noverrides = { validation = { min_mip_psnr_db = 31.0 } }\n",
    );
    let j = run_with(&reg, "cs-quality", Some(&project), "r/x.png", None, &[]);
    assert_eq!(j.profile.subtype, Subtype::Height);
    assert_eq!(j.profile.validation.min_mip_psnr_db, 31.0);
    let j = run_with(&reg, "cs-quality", Some(&project), "r/x.png", None, &["subtype=diffuse"]);
    assert_eq!(j.profile.subtype, Subtype::Diffuse);
    assert_eq!(j.profile.validation.min_mip_psnr_db, 30.0);
    assert!(!matches!(j.provenance.get("validation.min_mip_psnr_db"), Some(Provenance::Rule(_))));
}

/// Carried fix 3: a `--set` sub-type then a rule for the new sub-type — that rule applies, by `subtype` or by
/// `match.subtypes`; the first rule of a source that names another sub-type is skipped for the next one.
#[test]
fn a_rule_for_the_new_subtype_applies_after_a_set_subtype() {
    let reg = PresetRegistry::builtin().unwrap();
    let project = project_rules(concat!(
        "[[rules]]\nmatch = { glob = \"**/r/*.png\" }\nsubtype = \"height\"\noverrides = { validation = { min_mip_psnr_db = 31.0 } }\n",
        "[[rules]]\nmatch = { glob = \"**/r/*.png\", subtypes = [\"diffuse\"] }\noverrides = { validation = { min_mip_psnr_db = 32.0 } }\n",
    ));
    let j = run_with(&reg, "cs-quality", Some(&project), "r/x.png", None, &["subtype=diffuse"]);
    assert_eq!(j.profile.validation.min_mip_psnr_db, 32.0);
    assert_eq!(j.provenance["validation.min_mip_psnr_db"].to_string(), "rule:project#1");
    // A rule that names sub-types in `match` never decides one.
    let only = project_rules("[[rules]]\nmatch = { glob = \"**/r/*.png\", subtypes = [\"diffuse\"] }\nsubtype = \"height\"\n");
    let j = run_with(&reg, "cs-quality", Some(&only), "r/x.png", None, &[]);
    assert_ne!(j.provenance["subtype"].to_string(), "rule:project#0");
}
```

`crates/engine/tests/project_files.rs`:

```rust
/// Carried fix 3: a `[files]` sub-type beats a rule's sub-type, and that rule's overrides then do not apply; a
/// `[files]` field beats a rule's field for the same sub-type.
#[test]
fn files_entries_beat_rules_and_drop_rules_for_another_subtype() {
    let d = fixture(concat!(
        "[[rules]]\nmatch = { glob = \"**/landscape/rock01.png\" }\nsubtype = \"glow\"\noverrides = { alpha = { threshold = 90 } }\n",
        "[files.\"landscape/rock01.png\"]\nsubtype = \"diffuse\"\n",
    ));
    let p = plan(req(&d));
    let j = job(&p, "landscape/rock01.dds");
    assert_eq!(prov(j, "subtype"), "project_file");
    assert_eq!(j.profile.alpha.threshold, 127);
    let d = fixture(concat!(
        "[[rules]]\nmatch = { glob = \"**/landscape/rock01.png\" }\nsubtype = \"diffuse\"\noverrides = { alpha = { threshold = 90 } }\n",
        "[files.\"landscape/rock01.png\"]\nalpha.threshold = 100\n",
    ));
    let p = plan(req(&d));
    let j = job(&p, "landscape/rock01.dds");
    assert_eq!(j.profile.alpha.threshold, 100);
    assert_eq!(prov(j, "alpha.threshold"), "project_file");
}
```

- [ ] **Step 2: Run them and check that they fail**

Run: `cargo test -p engine --offline --test presets --test policy_checks --test project_files`
Expected:
- a compile error (`RuleMatch::subtypes` not found);
- once that compiles: failures in `null_patches_never_add_absent_keys`, `a_set_subtype_drops_the_overrides_of_a_rule_that_named_another_subtype` (31.0) and `files_entries_beat_rules_and_drop_rules_for_another_subtype` (90).

- [ ] **Step 3: Implement**

`presets.rs`, in `RuleMatch`, after `alpha`:

```rust
    /// M6 Track D (DECISIONS Note T2): the sub-type being resolved is one of these; empty = any. Matched only when the
    /// profile is layered, so a rule that names sub-types here never decides a sub-type.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub subtypes: Vec<Subtype>,
```

`presets.rs`, replace `merge_json`:

```rust
/// Deep-merge `patch` into `base`: objects merge key by key, other values replace, and a `null` patch changes
/// nothing: it neither erases a value nor adds an absent key (carried fix 2, DECISIONS Note T10).
pub fn merge_json(base: &mut Value, patch: &Value) {
    match (base, patch) {
        (_, Value::Null) => {}
        (Value::Object(b), Value::Object(p)) => {
            for (k, v) in p {
                match b.get_mut(k) {
                    Some(slot) => merge_json(slot, v),
                    None => {
                        if let Some(v) = without_nulls(v) {
                            b.insert(k.clone(), v);
                        }
                    }
                }
            }
        }
        (b, p) => *b = p.clone(),
    }
}

/// `v` without its `null` leaves; `None` for `null` itself.
fn without_nulls(v: &Value) -> Option<Value> {
    match v {
        Value::Null => None,
        Value::Object(m) => Some(Value::Object(
            m.iter()
                .filter_map(|(k, x)| Some((k.clone(), without_nulls(x)?)))
                .collect(),
        )),
        _ => Some(v.clone()),
    }
}
```

`policy.rs`, replace `rule_matches` and `first_match`:

```rust
/// `st` is the sub-type being resolved, or `None` while it is being decided: a rule with `match.subtypes` then never
/// matches (DECISIONS Note T2).
pub fn rule_matches(m: &RuleMatch, f: &FileFacts, st: Option<Subtype>) -> bool {
    m.glob.as_deref().is_none_or(|g| glob_match(g, f))
        && m.suffix
            .as_deref()
            .is_none_or(|s| f.suffix.unwrap_or("").eq_ignore_ascii_case(s))
        && m.set_has
            .as_deref()
            .is_none_or(|s| f.set.suffixes.contains(&s.to_ascii_lowercase()))
        && m.alpha.is_none_or(|a| a == f.analysis.alpha)
        && (m.subtypes.is_empty() || st.is_some_and(|s| m.subtypes.contains(&s)))
}
```

```rust
/// The first rule of one source that matches. While layering sub-type `st`, a rule that names another sub-type
/// (`Rule::subtype`) does not match, so a `--set`/`[files]` sub-type drops it (carried fix 3, Note T10).
fn first_match<'a>(
    rules: &'a [Rule],
    f: &FileFacts,
    st: Option<Subtype>,
    ignore_alpha_rules: bool,
) -> Option<(usize, &'a Rule)> {
    rules.iter().enumerate().find(|(_, r)| {
        !(ignore_alpha_rules && r.matcher.alpha.is_some())
            && st.is_none_or(|s| r.subtype.is_none_or(|x| x == s))
            && rule_matches(&r.matcher, f, st)
    })
}
```

In `decide_subtype` change `first_match(rules, f, false)` to `first_match(rules, f, None, false)`. In `layered_profile_with` change `first_match(rules, f, ignore_alpha_rules)` to `first_match(rules, f, Some(st), ignore_alpha_rules)`.

If a `RuleMatch { … }` literal lacks `..Default::default()`, add `subtypes: vec![]` (`grep -rn "RuleMatch {" crates`).

- [ ] **Step 4: Run the tests and check that they pass**

Run: `cargo test -p engine --offline --test presets --test policy_checks --test project_files`, then `cargo test --workspace --offline`.
Expected: PASS.
- A test that relied on a rule's overrides surviving a `--set subtype` change, or a `CFG-UNAVAILABLE` fallback, now fails. That is the carried fix: change its expectation and name it in the report.
- A test that relied on `merge_json` writing `null` for an absent key now fails. Change it the same way and name it.

- [ ] **Step 5: Regenerate the schemas, run the gates, commit**

```bash
cargo run -p sdc --offline -- presets schema all --out schema
git add crates/engine/src/presets.rs crates/engine/src/policy.rs crates/engine/tests/presets.rs crates/engine/tests/policy_checks.rs crates/engine/tests/project_files.rs schema
git commit -m "feat(rules): match.subtypes; rules re-matched on the final sub-type; merge_json never adds null keys (M6 Track D, carried fixes 2-3)

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

---

### Task 4: Engine — `resize = "half"` and `validation.mips = "smallest"`

**Spec:** §2.2 (owner correction 956da58), §2.3, §9 (`default_profiles_hash_as_before` stays green). Rulings T4 and T5.

**Files:**
- Modify: `crates/engine/src/config.rs`:
  - `:75` (`ResizeMode`);
  - `:426-434` (`ValidationSpec`);
  - `:447` (the `resize` doc).
- Modify: `crates/engine/src/resize.rs:1-16` (the module doc and `target_dims`).
- Modify: `crates/engine/src/policy.rs:1446-1458` (the `DIM-RESIZED` message names the mode).
- Modify: `crates/engine/src/validate/mod.rs`:
  - the PSNR loop at `:206-246`;
  - the coverage loop at `:312-334`.
- Modify: `crates/engine/src/overrides.rs:158`: add `"validation.mips"` after `"validation.pgpatcher_non_white_share"`.
- Modify: `crates/gui/src/ui/settings_panel.rs:157-160`: add the `half` option.
- Test: unit tests in `resize.rs`; `crates/engine/tests/validate.rs`; `crates/engine/tests/golden.rs:2840-2846` (`default_profiles_hash_as_before`).
- Regenerate: `schema/` (profile, preset).

**Interfaces:**
- Consumes: nothing new.
- Produces:
  - `ResizeMode::Half`, serialised `"half"`.
  - `pub const HALF_FLOOR: u32 = 512;` in `resize.rs`.
  - `cfg_enum!(#[derive(Default)] ValidationMips, "lowercase" { #[default] All, Smallest });` with `impl ValidationMips { pub fn is_all(&self) -> bool }`.
  - `ValidationSpec::mips: ValidationMips` (`#[serde(default, skip_serializing_if = "ValidationMips::is_all")]`).

- [ ] **Step 1: Write the failing tests**

`resize.rs`, in `mod tests`:

```rust
    /// M6 Track D (owner, 2026-10-12): both sides halve together when the larger is above 512; the shape never changes.
    #[test]
    fn half_halves_both_sides_when_the_larger_is_above_512() {
        let t = |w, h| target_dims((w, h), ResizeMode::Half);
        assert_eq!(t(2048, 2048), (1024, 1024));
        assert_eq!(t(1024, 512), (512, 256));
        assert_eq!(t(512, 256), (512, 256));
        assert_eq!(t(512, 512), (512, 512));
        assert_eq!(t(513, 100), (256, 50));
        assert_eq!(t(1000, 600), (500, 300));
        assert_eq!(t(1024, 1), (512, 1));
        assert_eq!(t(64, 64), (64, 64));
    }
```

`crates/engine/tests/validate.rs` (it uses the file's `profile`, `dds`, `run`, `flat` and `codes` helpers):

```rust
/// M6 Track D (Note T5): `validation.mips = smallest` checks PSNR and coverage on the last level only.
#[test]
fn smallest_mip_validation_checks_only_the_last_level() {
    let mut p = profile(Subtype::Diffuse);
    let (b, _) = dds(&p, &[flat(8, [10, 10, 10, 255]), flat(4, [10, 10, 10, 255])]);
    let other = [
        EncoderInput::Ldr(vec![[200, 90, 30, 255]; 64]),
        EncoderInput::Ldr(vec![[200, 90, 30, 255]; 16]),
    ];
    let levels = |r: &ValidationReport| -> Vec<u64> {
        r.warnings
            .iter()
            .filter(|w| w.code == Code::ValMipError)
            .map(|w| w.data["level"].as_u64().unwrap())
            .collect()
    };
    assert_eq!(levels(&run(&b, &p, Some(&other), None, None)), [0, 1]);
    p.validation.mips = ValidationMips::Smallest;
    assert_eq!(levels(&run(&b, &p, Some(&other), None, None)), [1]);

    let mut p = profile(Subtype::Diffuse);
    p.alpha.policy = AlphaPolicy::Cutout;
    let a_flat = |n: u32| flat(n, [128, 128, 128, 0]);
    let (b, inputs) = dds(&p, &[a_flat(32), a_flat(16)]);
    let r = run(&b, &p, Some(&inputs), Some(1.0), None);
    assert_eq!(codes(&r).iter().filter(|c| **c == Code::ValCoverage).count(), 2);
    p.validation.mips = ValidationMips::Smallest;
    let r = run(&b, &p, Some(&inputs), Some(1.0), None);
    assert_eq!(codes(&r).iter().filter(|c| **c == Code::ValCoverage).count(), 1);
    assert_eq!(r.summary.coverage.len(), 1);
}
```

`golden.rs`, in `default_profiles_hash_as_before`, after `assert!(p["mips"].get("edge").is_none(), "{id}: {p}");`:

```rust
        assert!(p["validation"].get("mips").is_none(), "{id}: {p}");
```

- [ ] **Step 2: Run them and check that they fail**

Run: `cargo test -p engine --offline --lib resize::` and `cargo test -p engine --offline --test validate`
Expected: compile errors (`ResizeMode::Half`, `ValidationMips` not found).

- [ ] **Step 3: Implement**

`config.rs`:

```rust
cfg_enum!(#[derive(Default)] ResizeMode, "kebab-case" { #[default] None, PotDown, Half });
```

After the `ResizeMode` impl:

```rust
cfg_enum!(#[derive(Default)] ValidationMips, "lowercase" { #[default] All, Smallest });
impl ValidationMips {
    pub fn is_all(&self) -> bool {
        *self == ValidationMips::All
    }
}
```

In `ValidationSpec`, after `pgpatcher_non_white_share`:

```rust
    /// M6 Track D (DECISIONS Note T5): `smallest` runs the per-mip PSNR and coverage checks on the smallest mip
    /// only (`fast-preview`). Not serialised at its default, so existing profile hashes hold.
    #[serde(default, skip_serializing_if = "ValidationMips::is_all")]
    pub mips: ValidationMips,
```

If a `ValidationSpec { … }` literal exists outside serde (`grep -rn "ValidationSpec {" crates`), add `mips: ValidationMips::All`.

Change the `resize` doc on `Profile` (`:447`) to: `/// Preprocess step 6 (spec §5.4); \`pot-down\` = largest power of two ≤ each axis, \`half\` = both axes halved when the larger is above 512 (M6 Track D).`

`resize.rs`, change the first line of the module doc to: `//! Resize (spec §5.4 step 6, S-mips-6; M6 Track D T4): \`resize = pot-down\` takes each axis to the largest power of two that fits, \`resize = half\` halves both axes when the larger is above 512,`. Keep the rest of the doc. Replace `target_dims`:

```rust
/// The larger side a `half` resize keeps (M6 Track D spec §2.2).
pub const HALF_FLOOR: u32 = 512;

/// The size `resize` makes of a `w`×`h` image: `pot-down` takes the largest power of two ≤ each axis; `half` halves
/// both axes (integer division, never below 1) when the larger one is above [`HALF_FLOOR`], so the shape never changes
/// and there is no power-of-two step (owner, 2026-10-12).
pub fn target_dims((w, h): (u32, u32), mode: ResizeMode) -> (u32, u32) {
    let prev_pow2 = |v: u32| 1u32 << (31 - v.max(1).leading_zeros());
    match mode {
        ResizeMode::None => (w, h),
        ResizeMode::PotDown => (prev_pow2(w), prev_pow2(h)),
        ResizeMode::Half if w.max(h) > HALF_FLOOR => ((w / 2).max(1), (h / 2).max(1)),
        ResizeMode::Half => (w, h),
    }
}
```

`policy.rs`, in the `DIM-RESIZED` warning (`:1446-1458`), name the mode:

```rust
    let (ow, oh) = crate::resize::target_dims((a.width, a.height), p.resize);
    let mode = serde_json::to_value(p.resize).unwrap_or_default();
    let resized = ((ow, oh) != (a.width, a.height)).then(|| {
        Warning::new(
            DimResized,
            format!(
                "{}x{} resized to {ow}x{oh} (resize = {})",
                a.width,
                a.height,
                mode.as_str().unwrap_or("?")
            ),
        )
        .with_data(json!({ "from": [a.width, a.height], "to": [ow, oh] }))
    });
```

`validate/mod.rs`:
- Add `ValidationMips` to the `crate::config` import.
- Before the per-mip loop (`for (l, ((lw, lh, d), e)) in decoded.iter().zip(enc).enumerate()`):

```rust
        // M6 Track D (Note T5): fast preview checks PSNR on the smallest mip only.
        let last = decoded.len().saturating_sub(1);
        let smallest_only = v.mips == ValidationMips::Smallest;
```

- Inside that loop, wrap only the PSNR statements in `if !smallest_only || l == last { … }`: the `let q = psnr(e, d, &ch);` line, the `s.min_mip_psnr_db = …` line, and the `if q < v.min_mip_psnr_db { … }` block. The texel-count checks and the exact-opaque block check stay outside it.
- In the coverage loop (`for (l, (lw, lh, px)) in ldr.iter().enumerate()`), add this as the first statement:

```rust
                    if v.mips == ValidationMips::Smallest && l + 1 != ldr.len() {
                        continue;
                    }
```

`overrides.rs`: add `"validation.mips",` after `"validation.pgpatcher_non_white_share",`.

`settings_panel.rs`, in the `"resize"` options:

```rust
        "resize" => vec![
            opt(json!("none"), "None"),
            opt(json!("pot-down"), "Down to a power of two"),
            opt(json!("half"), "Half size (larger side above 512)"),
        ],
```

- [ ] **Step 4: Run the tests and check that they pass**

Run: `cargo test -p engine --offline --lib resize::`, `cargo test -p engine --offline --test validate`, `cargo test -p engine --offline --test golden default_profiles_hash_as_before`, then `cargo test --workspace --offline`.
Expected: PASS.
- `resize = pot-down` messages keep their wording apart from the mode name: "(resize = pot-down)".
- If a GUI test lists the resize options, add `half` to it and name it in the report.

- [ ] **Step 5: Regenerate the schemas, run the gates, commit**

```bash
cargo run -p sdc --offline -- presets schema all --out schema
git add crates/engine/src/config.rs crates/engine/src/resize.rs crates/engine/src/policy.rs crates/engine/src/validate/mod.rs crates/engine/src/overrides.rs crates/gui/src/ui/settings_panel.rs crates/engine/tests/validate.rs crates/engine/tests/golden.rs schema
git commit -m "feat(engine): resize = half (both sides, larger above 512) and validation.mips = smallest; defaults not serialised (M6 Track D spec §2.2-2.3)

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

---

### Task 5: Engine and CLI — the user presets folder on every run; `--preset-file` duplicates are errors

**Spec:** §3 (load on every CLI run and in the GUI, the injectable path, bad files, duplicates). Ruling T6.

**Files:**
- Modify: `crates/engine/src/api.rs`:
  - `BatchRequest` (`:54-104`): a new field after `user_config`;
  - `BatchRequest::new` (`:108-134`);
  - `Setup` (`:570-577`);
  - `setup` (`:596-668`);
  - `plan_cached` (`:684-718`): destructure, and emit after `BatchStarted`.
- Modify: `crates/cli/src/args.rs`:
  - a new `user_presets_dir()`;
  - `build_request` (`:327-329`);
  - the `--preset`/`--preset-file` help (`:108-113`).
- Create: `crates/engine/tests/user_presets.rs`.
- Test: `crates/cli/tests/cli_convert.rs` (uses `Fx`, which already points `APPDATA` at `<tmp>/appdata`).

**Interfaces:**
- Consumes (Task 1): `PresetRegistry::add_dir(&mut self, &Path) -> Vec<Warning>` and `add_file`'s duplicate refusal.
- Produces:
  - `BatchRequest::user_presets: Option<PathBuf>`: the folder. `None` from `BatchRequest::new`.
  - `pub fn args::user_presets_dir() -> Option<PathBuf>`: `%APPDATA%\sdc\presets`. Task 8's verbs use it.
  - `CFG-PRESET-SKIPPED` events: `EventKind::Warning` with no path, emitted right after `batch_started`.

- [ ] **Step 1: Write the failing tests**

Create `crates/engine/tests/user_presets.rs`:

```rust
//! M6 Track D spec §3: the user presets folder through `Engine::plan` (`BatchRequest::user_presets`).
use engine::api::*;
use engine::codes::Code;
use engine::config::{Format, Target};
use engine::events::*;
use image::{ImageBuffer, Rgb};
use std::path::Path;
use std::sync::Arc;

fn fixture() -> tempfile::TempDir {
    let d = tempfile::tempdir().unwrap();
    let p = d.path().join("in/textures/a/wall.png");
    std::fs::create_dir_all(p.parent().unwrap()).unwrap();
    ImageBuffer::from_fn(32, 32, |x, y| Rgb([x as u8 * 7, y as u8 * 7, 90]))
        .save(p)
        .unwrap();
    d
}

fn write(dir: &Path, name: &str, text: &str) -> std::path::PathBuf {
    std::fs::create_dir_all(dir).unwrap();
    let p = dir.join(name);
    std::fs::write(&p, text).unwrap();
    p
}

fn request(d: &Path) -> BatchRequest {
    let mut r = BatchRequest::new(vec![d.join("in")], d.join("out"), Target::Cs);
    r.recursive = true;
    r.accept = engine::policy::AcceptSuggestions::High;
    r.overrides = vec![engine::overrides::parse_set("subtype=diffuse").unwrap()];
    r
}

fn plan(req: BatchRequest) -> (Result<Plan, EngineError>, Vec<EngineEvent>) {
    let sink = Arc::new(VecSink::default());
    let rep = Reporter::new(sink.clone(), "t");
    let p = Engine::new().unwrap().plan(req, &rep, &CancelToken::default());
    (p, sink.take())
}

const MINE: &str = "schema_version = 1\nid = \"mine\"\nversion = \"1.0.0\"\nbase = \"cs-quality\"\n[subtypes.diffuse]\nformat = \"bc1\"\n";

/// A folder preset is selectable with no flag; a bad one is skipped with CFG-PRESET-SKIPPED and the run continues.
#[test]
fn folder_presets_are_selectable_and_bad_ones_are_skipped() {
    let d = fixture();
    let dir = d.path().join("presets");
    write(&dir, "mine.toml", MINE);
    write(&dir, "zz_bad.toml", "schema_version = 1\nid = \"bad\"\nversion = \"1\"\nbase = \"nope\"\n");
    let mut req = request(d.path());
    req.user_presets = Some(dir.clone());
    req.preset = Some("mine".into());
    let (p, events) = plan(req);
    let p = p.unwrap();
    assert_eq!(p.preset_id, "mine");
    assert_eq!(p.jobs[0].profile.format, Format::Bc1);
    assert!(p.registry.source("mine").unwrap().ends_with("mine.toml"));
    let skipped: Vec<_> = events
        .iter()
        .filter_map(|e| match &e.kind {
            EventKind::Warning(w) if w.code == Code::CfgPresetSkipped => Some(w.clone()),
            _ => None,
        })
        .collect();
    assert_eq!(skipped.len(), 1, "{skipped:?}");
    assert!(skipped[0].data["file"].as_str().unwrap().ends_with("zz_bad.toml"));
    // No folder given: nothing user-side is loaded.
    let mut req = request(d.path());
    req.preset = Some("mine".into());
    assert!(matches!(plan(req).0, Err(EngineError::Usage(_))));
}

/// Spec §3: a `--preset-file` whose id is already loaded is an error naming both files; the same file twice is not.
#[test]
fn a_preset_file_duplicate_is_an_error_and_the_same_file_is_not() {
    let d = fixture();
    let dir = d.path().join("presets");
    let folder_file = write(&dir, "mine.toml", MINE);
    let other = write(&d.path().join("elsewhere"), "mine.toml", MINE);
    let mut req = request(d.path());
    req.user_presets = Some(dir.clone());
    req.preset_files = vec![other];
    match plan(req).0 {
        Err(EngineError::Config(m)) => {
            assert!(m.contains("elsewhere") && m.contains("presets"), "{m}")
        }
        other => panic!("{other:?}"),
    }
    let mut req = request(d.path());
    req.user_presets = Some(dir);
    req.preset_files = vec![folder_file];
    req.preset = Some("mine".into());
    assert!(plan(req).0.is_ok());
}
```

`crates/cli/tests/cli_convert.rs` (end):

```rust
/// M6 Track D spec §3: a preset in `%APPDATA%\sdc\presets\` is usable by `--preset <id>` with no other flag.
#[test]
fn a_user_folder_preset_is_usable_by_id() {
    let fx = Fx::new();
    fx.rgb("wall.png");
    fx.write(
        "appdata/sdc/presets/mine.toml",
        "schema_version = 1\nid = \"mine\"\nversion = \"1.0.0\"\nbase = \"cs-quality\"\n[subtypes.diffuse]\nformat = \"bc1\"\n",
    );
    let o = fx.convert(&["--preset", "mine", DIFFUSE[0], DIFFUSE[1]]);
    assert_eq!(code(&o), 0, "{}", text(&o.stderr));
    assert_eq!(fx.format("textures/a/wall.dds"), DdsPixelFormat::Bc1 { srgb: false });
}
```

- [ ] **Step 2: Run them and check that they fail**

Run: `cargo test -p engine --offline --test user_presets` and `cargo test -p sdc --offline --test cli_convert a_user_folder_preset`
Expected: a compile error (`user_presets` is not a field), and the CLI test fails with "unknown preset 'mine'" (exit 2).

- [ ] **Step 3: Implement**

`api.rs`, `BatchRequest`, after `user_config`:

```rust
    /// The user presets folder (`%APPDATA%\sdc\presets`, M6 Track D spec §3): every `*.toml` in it is loaded before
    /// `preset_files`; a bad one is skipped with CFG-PRESET-SKIPPED. `None` (tests) loads nothing.
    pub user_presets: Option<PathBuf>,
```

`BatchRequest::new`: add `user_presets: None,` after `user_config: None,`.

`Setup`: add the field `preset_skips: Vec<Warning>,` (import `crate::codes::Warning` if `api.rs` does not already).

`setup`, at the start, replace:

```rust
        let mut registry = self.registry.clone();
        for f in &req.preset_files {
            registry.add_file(f).map_err(config_err)?;
        }
```

with:

```rust
        let mut registry = self.registry.clone();
        // Spec §3: the user folder first (bad files skipped), then `--preset-file` (a bad one is an error).
        let preset_skips = req
            .user_presets
            .as_deref()
            .map(|d| registry.add_dir(d))
            .unwrap_or_default();
        for f in &req.preset_files {
            registry.add_file(f).map_err(config_err)?;
        }
```

and add `preset_skips,` to the returned `Setup { … }`.

`plan_cached`: destructure `preset_skips` from `self.setup(&req)?`. Right after the `rep.emit(None, None, EventKind::BatchStarted { … });` call:

```rust
        for w in preset_skips {
            rep.emit(None, None, EventKind::Warning(w));
        }
```

`args.rs`, above `build_request`:

```rust
/// `%APPDATA%\sdc\presets`, the user presets folder (M6 Track D spec §3); `None` when APPDATA is unset.
pub fn user_presets_dir() -> Option<PathBuf> {
    std::env::var_os("APPDATA").map(|a| PathBuf::from(a).join("sdc").join("presets"))
}
```

In `build_request`, after the `r.user_config = …;` statement:

```rust
    r.user_presets = user_presets_dir();
```

Help text in `BatchArgs`:

```rust
    /// Preset id (built-in, or from %APPDATA%\sdc\presets\*.toml), or a path to a preset .toml file.
    #[arg(long)]
    pub preset: Option<String>,
    /// Load a preset .toml without selecting it, after the user presets folder, e.g. one declaring suffix aliases
    /// (repeatable). An id that is already loaded is an error.
    #[arg(long = "preset-file")]
    pub preset_file: Vec<PathBuf>,
```

- [ ] **Step 4: Run the tests and check that they pass**

Run: `cargo test -p engine --offline --test user_presets`, `cargo test -p sdc --offline --test cli_convert`, then `cargo test --workspace --offline`.
Expected: PASS. Every other CLI test sets `APPDATA` to an empty or missing folder, so no extra preset loads there.

- [ ] **Step 5: Run the gates, then commit**

```bash
git add crates/engine/src/api.rs crates/cli/src/args.rs crates/engine/tests/user_presets.rs crates/cli/tests/cli_convert.rs
git commit -m "feat: user presets folder (%APPDATA%\\sdc\\presets) on every run; CFG-PRESET-SKIPPED for bad files; --preset-file duplicates are errors (M6 Track D spec §3)

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

---

### Task 6: Engine — the remaining carried fixes: `CFG-GREEN-CONFLICT`, landscape pins, `hdr_sky` only under `sky`

**Spec:** §6 items 4, 5 and 6. Ruling T10.

**Files:**
- Modify: `crates/engine/src/policy.rs`:
  - `:601-607`: the OpenGL flip no longer overwrites an explicit `flip_green`;
  - `resolve_decided` at `:1714-1720`: push the conflict warning;
  - a new function `green_conflict` next to `filter_ignored_notice` (`:633`).
- Modify: `crates/engine/src/suggest.rs:126-127`.
- Test: `crates/engine/tests/policy_checks.rs` (uses `run`, `opaque`, `codes`); unit tests in `suggest.rs` (uses `ctx`).

**Interfaces:**
- Consumes: `Code::CfgGreenConflict` (Task 1) and `field_explicit` (`policy.rs:446`).
- Produces: no new public names.

- [ ] **Step 1: Write the failing tests**

`policy_checks.rs`:

```rust
/// Carried fix 4 (Note T10): an explicit flip_green that contradicts green_convention is kept and warned.
#[test]
fn an_explicit_flip_green_against_the_convention_warns_and_wins() {
    let n = |sets: &[&str]| run(Target::Cs, "textures/a/x_n.png", Some("n"), &["n"], opaque(), sets, &[]);
    let j = n(&["channels.green_convention=opengl", "channels.flip_green=false"]);
    assert!(!j.profile.channels.flip_green);
    assert!(codes(&j).contains(&Code::CfgGreenConflict), "{:?}", j.warnings);
    let w = j.warnings.iter().find(|w| w.code == Code::CfgGreenConflict).unwrap();
    assert_eq!(w.data, serde_json::json!({"flip_green": false, "convention": "opengl"}));
    let j = n(&["channels.green_convention=directx", "channels.flip_green=true"]);
    assert!(j.profile.channels.flip_green);
    assert!(codes(&j).contains(&Code::CfgGreenConflict));
    // Agreeing values and the convention alone raise nothing.
    for sets in [
        &["channels.green_convention=opengl"][..],
        &["channels.green_convention=opengl", "channels.flip_green=true"],
        &["channels.green_convention=directx"],
    ] {
        let j = n(sets);
        assert!(!codes(&j).contains(&Code::CfgGreenConflict), "{sets:?}");
    }
    assert!(n(&["channels.green_convention=opengl"]).profile.channels.flip_green);
}
```

`suggest.rs`, in `mod tests` (`ctx(target, suffix, path, set, alpha)` exists at `:223`):

```rust
    /// Carried fix 5: only a whole `landscape` component (or textures/landscape/) is landscape.
    #[test]
    fn landscape_needs_a_whole_path_component() {
        let none = BTreeSet::new();
        let first = |p: &str| suggest(&ctx(Target::Cs, Some("n"), p, &none, AlphaContent::None))[0].subtype;
        assert_eq!(first("textures/landscape/x_n.png"), Subtype::LandscapeNormal);
        assert_eq!(first("textures/mod/landscape/x_n.png"), Subtype::LandscapeNormal);
        assert_ne!(first("textures/landscapes_old/x_n.png"), Subtype::LandscapeNormal);
        assert_ne!(first("textures/mylandscape/x_n.png"), Subtype::LandscapeNormal);
    }

    /// Carried fix 6: hdr_sky only under a `sky` component, as its reason says.
    #[test]
    fn hdr_sky_needs_a_sky_component() {
        let none = BTreeSet::new();
        let mut f = ctx(Target::Cs, None, "textures/interface/x.exr", &none, AlphaContent::None);
        f.is_float = true;
        assert_eq!(suggest(&f)[0].subtype, Subtype::UiSky);
        let mut f = ctx(Target::Cs, None, "textures/sky/x.exr", &none, AlphaContent::None);
        f.is_float = true;
        assert_eq!(suggest(&f)[0].subtype, Subtype::HdrSky);
    }
```

- [ ] **Step 2: Run them and check that they fail**

Run: `cargo test -p engine --offline --test policy_checks an_explicit_flip_green` and `cargo test -p engine --offline --lib suggest::`
Expected:
- `an_explicit_flip_green_against_the_convention_warns_and_wins` FAILS: flip_green is true and no warning.
- `hdr_sky_needs_a_sky_component` FAILS: `HdrSky` under `interface/`.
- `landscape_needs_a_whole_path_component` PASSES already (ruling T10 #5: `main` already matches whole components). It pins the behaviour; record that in the report.

- [ ] **Step 3: Implement**

`policy.rs`, replace the OpenGL block in `layered_profile_with`:

```rust
    // Spec §3.2, §5.4: a stated OpenGL convention is flipped to DirectX in Preprocess (Task 28 reads flip_green). An
    // explicit `flip_green` wins; `resolve_decided` reports a contradiction as CFG-GREEN-CONFLICT (carried fix 4).
    if takes_green(st)
        && profile.channels.green_convention == Some(GreenConvention::Opengl)
        && !field_explicit(ctx, &prov, "channels.flip_green")
    {
        profile.channels.flip_green = true;
        if let Some(src) = prov.get("channels.green_convention").cloned() {
            prov.insert("channels.flip_green".into(), src);
        }
    }
```

Next to `filter_ignored_notice`:

```rust
/// Carried fix 4 (DECISIONS Note T10): an explicit `channels.flip_green` that contradicts the resolved
/// `green_convention` (OpenGL needs the flip, DirectX does not). The explicit field is kept.
fn green_conflict(
    ctx: &ResolveCtx,
    p: &Profile,
    prov: &BTreeMap<String, Provenance>,
) -> Option<Warning> {
    let convention = p.channels.green_convention?;
    if !takes_green(p.subtype) || !field_explicit(ctx, prov, "channels.flip_green") {
        return None;
    }
    let wants = convention == GreenConvention::Opengl;
    (p.channels.flip_green != wants).then(|| {
        let id = serde_json::to_value(convention).unwrap_or_default();
        Warning::new(
            Code::CfgGreenConflict,
            format!(
                "channels.flip_green = {} contradicts green_convention = {} (from {}); flip_green is kept",
                p.channels.flip_green,
                id.as_str().unwrap_or("?"),
                prov.get("channels.green_convention")
                    .map_or("builtin".to_string(), ToString::to_string)
            ),
        )
        .with_data(json!({ "flip_green": p.channels.flip_green, "convention": convention }))
    })
}
```

In `resolve_decided`, after the `warnings.extend(check_profile(…));` statement:

```rust
    warnings.extend(green_conflict(ctx, &profile, &provenance));
```

`suggest.rs`, replace the `hdr_sky` arm:

```rust
        // Carried fix 6: HDR sky only under a `sky` component (a float colour map under interface/ is ui_sky).
        Some("d") | None if has_component(c.game_path, "sky") && c.is_float => {
            vec![s(HdrSky, Medium, "float source under sky/")]
        }
```

- [ ] **Step 4: Run the tests and check that they pass**

Run: `cargo test -p engine --offline --test policy_checks` and `cargo test -p engine --offline --lib suggest::`, then `cargo test --workspace --offline`.
Expected: PASS. A golden or CLI test that expected `flip_green = true` despite an explicit `false` has met carried fix 4. Change its expectation and name it in the report.

- [ ] **Step 5: Run the gates, then commit**

```bash
git add crates/engine/src/policy.rs crates/engine/src/suggest.rs crates/engine/tests/policy_checks.rs
git commit -m "fix(policy): CFG-GREEN-CONFLICT and an explicit flip_green wins; hdr_sky only under sky/; landscape match pinned (M6 Track D carried fixes 4-6)

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

---

### Task 7: The three built-in presets and their goldens

**Spec:** §2 (the table), §2.1–2.3, §9 (goldens), Review Focus 3 and 5. Rulings T1, T2, T3.

**Files:**
- Create: `presets/cs-compact.toml`, `presets/vanilla-compact.toml`, `presets/fast-preview.toml`.
- Modify: `crates/engine/src/presets.rs:26-31`: `BUILTIN` gains 3 entries and becomes `[&str; 7]`.
- Modify: `crates/engine/tests/presets.rs:104-114` (`preset_targets_and_default_preset_agree`: the selectable list).
- Test: `crates/engine/tests/presets.rs`; `crates/engine/tests/golden.rs` (new cases at the end, after the shader cases; reuse `put`, `rgb`, `rgba`, `gray_ramp`, `flat_normal`, `planned`, `assert_profile`, `warnings`, `request`, `Out`).

**Interfaces:**
- Consumes:
  - `RuleMatch::subtypes` (Task 3);
  - `ResizeMode::Half` and `ValidationMips::Smallest` (Task 4);
  - `check_preset` (Task 1).
- Produces: the preset ids `cs-compact`, `vanilla-compact` and `fast-preview`. Task 8 (CLI), Task 10 (radius values) and Task 11 (changelog) rely on these files.

- [ ] **Step 1: Write the failing tests**

`crates/engine/tests/presets.rs`. Change the selectable list in `preset_targets_and_default_preset_agree`:

```rust
    assert_eq!(
        ids,
        ["cpm-quality", "cs-compact", "cs-quality", "fast-preview", "vanilla-compact", "vanilla-quality"]
    );
```

and add:

```rust
/// M6 Track D spec §2 (Notes T1–T3): the three new presets as profiles from presets alone (rules are not applied by
/// `profile`; the BC1 rules are checked by the goldens).
#[test]
fn compact_and_fast_preview_resolve_as_designed() {
    let r = PresetRegistry::builtin().unwrap();
    for (id, target) in [("cs-compact", Some(Target::Cs)), ("vanilla-compact", Some(Target::Vanilla)), ("fast-preview", None)] {
        let p = r.get(id).unwrap();
        assert_eq!((p.schema_version, p.version.as_str(), p.hidden, p.target), (1, "1.0.0", false, target));
        r.check_preset(id).unwrap();
    }
    let data = [
        Subtype::Normal, Subtype::LandscapeNormal, Subtype::PbrNormal, Subtype::Msn, Subtype::CoatNormal,
        Subtype::SkinWet, Subtype::Envmask, Subtype::Height, Subtype::MsnSpecular, Subtype::SkinSk,
        Subtype::ComplexMaterial, Subtype::Rmaos, Subtype::SkinRfaos, Subtype::Flow, Subtype::GenericTwoChannel,
        Subtype::GenericData, Subtype::HdrSky,
    ];
    for st in Subtype::ALL {
        let p = r.profile("cs-compact", Target::Cs, st).unwrap();
        let want = if data.contains(&st) { ResizeMode::Half } else { ResizeMode::None };
        assert_eq!(p.resize, want, "{st:?}");
        // Formats are the quality preset's: BC1 comes from the rules only.
        assert_eq!(p.format, r.profile("cs-quality", Target::Cs, st).unwrap().format, "{st:?}");
    }
    let compact = r.get("cs-compact").unwrap();
    assert_eq!(compact.rules.len(), 2);
    assert_eq!(compact.rules[0].matcher.alpha, Some(AlphaContent::None));
    assert_eq!(compact.rules[0].matcher.subtypes.len(), 9);
    assert_eq!(compact.rules[1].matcher.alpha, Some(AlphaContent::Constant255));
    assert_eq!(compact.rules[1].matcher.subtypes.len(), 7);
    for rule in &compact.rules {
        assert!(!rule.matcher.subtypes.contains(&Subtype::LandscapeDiffuse));
        assert!(!rule.matcher.subtypes.contains(&Subtype::PbrEmissive));
        assert_eq!(rule.overrides, json!({"format": "bc1"}));
    }
    assert_eq!(r.get("vanilla-compact").unwrap().rules, compact.rules);
    for t in [Target::Cs, Target::Vanilla, Target::Cpm] {
        let p = r.profile("fast-preview", t, Subtype::Diffuse).unwrap();
        assert_eq!(p.encoder.effort, Effort::Fast);
        assert_eq!(p.validation.mips, ValidationMips::Smallest);
        assert_eq!(p.resize, ResizeMode::None);
    }
}
```

`crates/engine/tests/golden.rs`, the helpers (add once, after `plan_shader`):

```rust
/// Plan only, under `preset`.
fn plan_preset(dir: &Path, target: Target, preset: &str, sets: &[&str]) -> Out {
    let sink = Arc::new(VecSink::default());
    let rep = Reporter::new(sink.clone(), "golden");
    let mut req = request(dir, target, sets, &[]);
    req.preset = Some(preset.into());
    Engine::new()
        .unwrap()
        .plan(req, &rep, &CancelToken::default())
        .unwrap();
    Out {
        summary: BatchSummary::default(),
        events: sink.take(),
    }
}

fn rgb_wh(w: u32, h: u32) -> DynamicImage {
    DynamicImage::ImageRgb8(ImageBuffer::from_fn(w, h, |x, y| {
        Rgb([(x * 4) as u8, (y * 4) as u8, 90])
    }))
}

/// `DIM-RESIZED`'s `to` of every planned job, by output path (`/` separators).
fn resized(o: &Out) -> BTreeMap<String, (u64, u64)> {
    o.events
        .iter()
        .filter_map(|e| match &e.kind {
            EventKind::FilePlanned { job } => {
                let w = job.warnings.iter().find(|w| w.code == Code::DimResized)?;
                let to = &w.data["to"];
                Some((
                    job.output.to_string_lossy().replace('\\', "/"),
                    (to[0].as_u64()?, to[1].as_u64()?),
                ))
            }
            _ => None,
        })
        .collect()
}
```

(Add `use std::collections::BTreeMap;` if `golden.rs` does not import it.)

The cases. Each is its own `#[test]`. "fmt" is the planned profile's `format` and "prov" is its `provenance.format`. Every file is under `in/`.

| Test | Target, preset | Files, `--set` | Expect |
|---|---|---|---|
| `compact_opaque_diffuse_is_bc1` | cs, `cs-compact` | `textures/a/wall.png` `rgb(64)` | `assert_profile(.., "a/wall.png", "diffuse", "remove", false, "bc1")`; prov `rule:cs-compact#0`; `warnings(&o)` has no `FmtBc1Alpha` |
| `compact_constant_255_diffuse_is_bc1` | cs, `cs-compact` | `textures/a/wall.png` `rgba(64, \|x, y\| [(x * 4) as u8, (y * 4) as u8, 90, 255])` | fmt `bc1`, prov `rule:cs-compact#1`, policy `remove` |
| `compact_diffuse_with_alpha_stays_bc7` | cs, `cs-compact` | `textures/a/glass.png` `rgba(64, \|x, _\| [90, 90, 90, (x * 4) as u8])`; `--set file:**/glass.png.alpha.policy=blend` | `assert_profile(.., "a/glass.png", "diffuse", "blend", false, "bc7")`; prov is not a `rule:cs-compact#…` (a `FilePlanned` event is emitted whatever the status, `api.rs:1338-1346`) |
| `compact_cutout_diffuse_stays_bc7` (RF5) | cs, `cs-compact` | `textures/a/leaf.png` `rgba(64, \|x, _\| [40, 120, 40, if x < 32 { 0 } else { 255 }])`; `--set file:**/leaf.png.alpha.policy=cutout` | `assert_profile(.., "a/leaf.png", "diffuse", "cutout", false, "bc7")`; no `FmtBc1Alpha` |
| `compact_constant_255_subsurface_stays_bc7` (RF6) | cs, `cs-compact` | `textures/a/skin_s.png` constant-255 RGBA as above; `--set file:**/skin_s.png.subtype=pbr_subsurface` | policy `data`, fmt `bc7`, no `FmtBc1Alpha` |
| `compact_landscape_diffuse_stays_bc7_and_halves` (RF7) | cs, `cs-compact` | `textures/landscape/dirt.png` `rgb(1024)` | sub-type `landscape_diffuse`, fmt `bc7`; `resized(&o)` = `{"./dirt.dds": (512, 512)}`; no `FmtLandscapeBc1` |
| `compact_2048_diffuse_with_alpha_plans_1024_bc7` | cs, `cs-compact` | `textures/a/glass.png` `rgba(2048, |x, _| [90, 90, 90, (x % 256) as u8])`; `--set file:**/glass.png.alpha.policy=blend` | fmt `bc7`; `resized(&o)` = `{"./glass.dds": (1024, 1024)}`; profile `resize` = `"half"` |
| `compact_2048_opaque_diffuse_stays_2048_bc1` | cs, `cs-compact` | `textures/a/wall.png` `rgb(2048)` | fmt `bc1`, prov `rule:cs-compact#0`; `resized(&o)` is empty; profile `resize` = `"none"` |
| `compact_2048_normal_plans_1024` | cs, `cs-compact` | `textures/a/wall_n.png` `flat_normal(2048)` | `resized(&o)` = `{".../wall_n.dds": (1024, 1024)}`; profile `resize` = `"half"` |
| `compact_512_mask_stays_512` | cs, `cs-compact` | `textures/a/wall_m.png` `gray_ramp(512)`; `--set file:**/wall_m.png.subtype=envmask` | `resized(&o)` empty; profile `resize` = `"half"`; fmt `bc4` |
| `compact_1024x512_plans_512x256` | cs, `cs-compact` | `textures/a/wide_n.png` `rgb_wh(1024, 512)`; `--set file:**/wide_n.png.subtype=normal` | `resized` = `(512, 256)`; no `DimNpot` |
| `compact_512x256_stays_512x256` | cs, `cs-compact` | `textures/a/wide_n.png` `rgb_wh(512, 256)`; same `--set` | `resized(&o)` empty |
| `compact_packed_rmaos_halves_as_one_image` (RF3) | cs, `cs-compact` | `textures/p/rock.png` `rgb(64)`, `rock_n.png` `flat_normal(1024)`, `rock_roughness.png` `gray_ramp(1024)`, `rock_ao.png` `gray_ramp(1024)` | the keys of `resized(&o)` end with exactly `p/rock_n.dds` and `p/rock_rmaos.dds`, both `(512, 512)`; no planned job whose input ends with `_roughness.png` or `_ao.png` reports `DimResized` |
| `vanilla_compact_opaque_diffuse_is_bc1` | vanilla, `vanilla-compact` | `textures/a/wall.png` `rgb(64)` | fmt `bc1`, prov `rule:vanilla-compact#0` |
| `fast_preview_is_fast_with_smallest_mip_validation` | cs, then vanilla; `fast-preview` | `textures/a/wall.png` `rgb(64)` | `profile.encoder.effort == "fast"`, `profile.validation.mips == "smallest"`, fmt `bc7` |

Write each one like this:

```rust
#[test]
fn compact_opaque_diffuse_is_bc1() {
    let d = tempfile::tempdir().unwrap();
    put(d.path(), "textures/a/wall.png", rgb(64));
    let o = plan_preset(d.path(), Target::Cs, "cs-compact", &[]);
    assert_profile(&o, "a/wall.png", "diffuse", "remove", false, "bc1");
    assert_eq!(
        planned(&o, "a/wall.png")["provenance"]["format"].as_str(),
        Some("rule:cs-compact#0")
    );
    assert!(!warnings(&o).contains(&Code::FmtBc1Alpha));
}

#[test]
fn compact_packed_rmaos_halves_as_one_image() {
    let d = tempfile::tempdir().unwrap();
    put(d.path(), "textures/p/rock.png", rgb(64));
    put(d.path(), "textures/p/rock_n.png", flat_normal(1024));
    put(d.path(), "textures/p/rock_roughness.png", gray_ramp(1024));
    put(d.path(), "textures/p/rock_ao.png", gray_ramp(1024));
    let o = plan_preset(d.path(), Target::Cs, "cs-compact", &[]);
    let r = resized(&o);
    let mut tails: Vec<&str> = r.keys().map(|k| k.rsplit('/').next().unwrap()).collect();
    tails.sort();
    assert_eq!(tails, ["rock_n.dds", "rock_rmaos.dds"], "{r:?}");
    assert!(r.values().all(|&to| to == (512, 512)), "{r:?}");
}
```

The fixtures behave as follows. If the engine disagrees with one, fix the engine code, not the expectation, unless a fixture premise below is wrong; if so, say which one in the report.
- `rgb(64)` has no alpha, so `AlphaContent::None` matches rule #0.
- A constant-255 RGBA source is `constant255`, so it matches rule #1.
- The `_roughness`/`_ao` raw maps are consumed by the `_rmaos` recipe under cs (spec §6.4). The packed image is resized in `preprocess_with` as one image (`pipeline.rs:236`).

- [ ] **Step 2: Run them and check that they fail**

Run: `cargo test -p engine --offline --test presets compact_and_fast` and `cargo test -p engine --offline --test golden compact_ fast_preview vanilla_compact`
Expected: FAIL. `get("cs-compact")` is `None`, and the plans fail with "unknown preset 'cs-compact'".

- [ ] **Step 3: Write the three presets**

`presets/cs-compact.toml`:

```toml
schema_version = 1
id = "cs-compact"
version = "1.0.0"
base = "cs-quality"
target = "cs"
description = "CS – Compact: BC1 for opaque colour, every other map except emissive and sky at half size (min 512)."

# BC1 for opaque colour maps (DECISIONS Note T2/T3). BC3 is never used: it is the size of BC7.
# Colour with no alpha channel. Not landscape_diffuse (FMT-LANDSCAPE-BC1) nor pbr_emissive (BC6H). BC1 colour stays full size (resize none beats the sub-type table below).
[[rules]]
match = { alpha = "none", subtypes = ["diffuse", "pbr_albedo", "glow", "pbr_subsurface", "pbr_fuzz", "subsurface_tint", "backlight_tint", "ui_sky", "skin_albedo"] }
overrides = { format = "bc1", resize = "none" }

# Colour whose alpha is constant 255 and whose policy resolves to remove (pbr_subsurface and pbr_fuzz keep data).
[[rules]]
match = { alpha = "constant255", subtypes = ["diffuse", "pbr_albedo", "glow", "subsurface_tint", "backlight_tint", "ui_sky", "skin_albedo"] }
overrides = { format = "bc1", resize = "none" }

# Data maps and BC7 colour at half size: both sides halve when the larger is above 512 (spec §2.2).
[subtypes.normal]
resize = "half"
[subtypes.landscape_normal]
resize = "half"
[subtypes.pbr_normal]
resize = "half"
[subtypes.msn]
resize = "half"
[subtypes.coat_normal]
resize = "half"
[subtypes.skin_wet]
resize = "half"
[subtypes.envmask]
resize = "half"
[subtypes.height]
resize = "half"
[subtypes.msn_specular]
resize = "half"
[subtypes.skin_sk]
resize = "half"
[subtypes.complex_material]
resize = "half"
[subtypes.rmaos]
resize = "half"
[subtypes.skin_rfaos]
resize = "half"
[subtypes.flow]
resize = "half"
[subtypes.generic_two_channel]
resize = "half"
[subtypes.generic_data]
resize = "half"
[subtypes.diffuse]
resize = "half"
[subtypes.landscape_diffuse]
resize = "half"
[subtypes.pbr_albedo]
resize = "half"
[subtypes.glow]
resize = "half"
[subtypes.pbr_subsurface]
resize = "half"
[subtypes.pbr_fuzz]
resize = "half"
[subtypes.subsurface_tint]
resize = "half"
[subtypes.backlight_tint]
resize = "half"
[subtypes.ui_sky]
resize = "half"
[subtypes.skin_albedo]
resize = "half"
```

`presets/vanilla-compact.toml` is identical apart from these four lines:

```toml
id = "vanilla-compact"
base = "vanilla-quality"
target = "vanilla"
description = "Vanilla – Compact: BC1 for opaque colour, every other map except emissive and sky at half size (min 512)."
```

(Write the whole file, rules and all 26 `[subtypes.*]` tables (16 data, 10 colour) included. The PBR and skin sub-types are unavailable under vanilla, but the tables are harmless and keep the two files in step.)

`presets/fast-preview.toml`:

```toml
schema_version = 1
id = "fast-preview"
version = "1.0.0"
base = "sdc-base"
description = "Fast preview: bc7e fast, smallest-mip validation only. For checking a mod quickly, not for release."

# No target: the batch target decides (DECISIONS Note T1).
[defaults]
encoder = { effort = "fast" }
validation = { mips = "smallest" }
```

`presets.rs`:

```rust
const BUILTIN: [&str; 7] = [
    include_str!("../../../presets/sdc-base.toml"),
    include_str!("../../../presets/cs-quality.toml"),
    include_str!("../../../presets/cpm-quality.toml"),
    include_str!("../../../presets/vanilla-quality.toml"),
    include_str!("../../../presets/cs-compact.toml"),
    include_str!("../../../presets/vanilla-compact.toml"),
    include_str!("../../../presets/fast-preview.toml"),
];
```

- [ ] **Step 4: Run the tests and check that they pass**

Run: `cargo test -p engine --offline --test presets`, `cargo test -p engine --offline --test golden`, then `cargo test --workspace --offline`.
Expected: PASS. Existing goldens are unchanged, because the quality presets did not change.
- A CLI or GUI test that lists every selectable preset may need the three new ids. `cli_presets.rs` only checks `contains`. Name any such change in the report.

- [ ] **Step 5: Run the gates, then commit**

```bash
git add presets/cs-compact.toml presets/vanilla-compact.toml presets/fast-preview.toml crates/engine/src/presets.rs crates/engine/tests/presets.rs crates/engine/tests/golden.rs
git commit -m "feat(presets): cs-compact, vanilla-compact (BC1 opaque colour, every other map but emissive and sky at half size) and fast-preview (bc7e fast, smallest-mip validation) (M6 Track D spec §2)

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

---

### Task 8: `sdc presets` — the folder and `--preset-file` on every verb, `list` sources, `diff`, `export`, `validate`

**Spec:** §4. §1's success criteria for the verbs and for `convert --preset cs-compact`. Review Focus 4. Ruling T8.

**Files:**
- Modify: `crates/engine/src/presets.rs`: new `PresetRegistry::validate_file` and `PresetRegistry::export`.
- Modify: `crates/cli/src/args.rs`:
  - `Command::Presets` (`:32-34`);
  - `PresetsCmd` (`:239-259`);
  - a new `PresetsArgs`.
- Modify: `crates/cli/src/main.rs:63` (no change in text if the variant keeps its name; the argument type changes).
- Modify: `crates/cli/src/verbs.rs:717-820` (`presets`).
- Test: `crates/engine/tests/presets.rs` (export, validate_file); `crates/cli/tests/cli_presets.rs`; `crates/cli/tests/cli_convert.rs` (the §1 compact criterion).

**Interfaces:**
- Consumes:
  - `source`, `table`, `remove`, `check_preset`, `add_dir`, `same_file` (Task 1);
  - `args::user_presets_dir` (Task 5).
- Produces:
  - `pub fn PresetRegistry::validate_file(&self, path: &Path) -> Vec<String>`: empty means clean.
  - `pub fn PresetRegistry::export(&self, id: &str, new_id: &str) -> Result<String, PresetError>`.
  - `pub struct args::PresetsArgs { pub preset_file: Vec<PathBuf>, pub cmd: PresetsCmd }`.
  - `PresetsCmd::Diff { a, b, subtype, target, json }`, `PresetsCmd::Export { id, out, new_id }` and `PresetsCmd::Validate { files }`.

- [ ] **Step 1: Write the failing engine tests** (`crates/engine/tests/presets.rs`)

```rust
const ULTRA_ALIASES: &str = "schema_version = 1\nid = \"ultra\"\nversion = \"2.0.0\"\nbase = \"cs-compact\"\n[subtypes.diffuse]\nalpha = { threshold = 100 }\n[suffix_aliases]\nrmaosultrahigh = \"rmaos\"\n";

/// Spec §4 and Review Focus 4: an export keeps its `base` link and aliases, validates where the original's base is loaded,
/// and resolves equal to the original for every sub-type.
#[test]
fn export_round_trips_and_keeps_aliases() {
    let mut r = PresetRegistry::builtin().unwrap();
    r.add_toml(ULTRA_ALIASES).unwrap();
    let text = r.export("ultra", "ultra-export").unwrap();
    let out: PresetFile = toml::from_str(&text).unwrap();
    assert_eq!(out.id, "ultra-export");
    assert_eq!(out.base.as_deref(), Some("cs-compact"));
    assert_eq!(out.suffix_aliases.get("rmaosultrahigh").map(String::as_str), Some("rmaos"));
    // Validate beside the built-ins only: the original's alias would conflict (Note T8).
    let d = tempfile::tempdir().unwrap();
    let f = write(d.path(), "export.toml", &text);
    let clean = PresetRegistry::builtin().unwrap();
    assert_eq!(clean.validate_file(&f), Vec::<String>::new());
    let mut both = PresetRegistry::builtin().unwrap();
    let id = both.add_file(&f).unwrap();
    for st in Subtype::ALL {
        assert_eq!(
            both.profile(&id, Target::Cs, st).unwrap(),
            r.profile("ultra", Target::Cs, st).unwrap(),
            "{st:?}"
        );
    }
}

/// Spec §4: the default export is the preset's own table (base stays a link) under the new id.
#[test]
fn default_export_keeps_the_base_link_under_a_new_id() {
    let r = PresetRegistry::builtin().unwrap();
    let text = r.export("cs-compact", "my-compact").unwrap();
    let p: PresetFile = toml::from_str(&text).unwrap();
    assert_eq!((p.id.as_str(), p.base.as_deref()), ("my-compact", Some("cs-quality")));
    assert_eq!(p.rules, r.get("cs-compact").unwrap().rules);
    assert!(matches!(r.export("nope", "x"), Err(PresetError::Unknown(_))));
}

/// Spec §4: validate names the first problem; a folder file validates in place of its own copy.
#[test]
fn validate_file_reports_problems_and_accepts_a_folder_file() {
    let d = tempfile::tempdir().unwrap();
    let mut r = PresetRegistry::builtin().unwrap();
    let mine = write(d.path(), "mine.toml", MINE);
    assert!(r.add_dir(d.path()).is_empty());
    assert!(r.validate_file(&mine).is_empty(), "a folder file validates against itself");
    for (name, text, says) in [
        ("dup.toml", "schema_version = 1\nid = \"cs-quality\"\nversion = \"1\"\n", "already loaded"),
        ("nobase.toml", "schema_version = 1\nid = \"x\"\nversion = \"1\"\nbase = \"nope\"\n", "nope"),
        ("unknown.toml", "schema_version = 1\nid = \"y\"\nversion = \"1\"\nbogus = 1\n", "bogus"),
        ("newer.toml", "schema_version = 9\nid = \"z\"\n", "newer"),
        ("rule.toml", "schema_version = 1\nid = \"w\"\nversion = \"1\"\nbase = \"cs-quality\"\n[[rules]]\nmatch = { subtypes = [\"nope\"] }\n", "nope"),
    ] {
        let f = write(&d.path().join("v"), name, text);
        let p = r.validate_file(&f);
        assert_eq!(p.len(), 1, "{name}: {p:?}");
        assert!(p[0].contains(says), "{name}: {p:?}");
    }
}
```

- [ ] **Step 2: Run them and check that they fail**

Run: `cargo test -p engine --offline --test presets export validate_file`
Expected: a compile error (`export`, `validate_file` not found).

- [ ] **Step 3: Implement the engine side** (`presets.rs`, after `add_dir`)

```rust
    /// `sdc presets validate` (spec §4): load `path` into a copy of this registry (without a preset already loaded
    /// from that same file, so a user folder file validates in place of itself) and check it. One line per problem;
    /// loading stops at the first, so a file has at most one (Note T8). Empty = clean.
    pub fn validate_file(&self, path: &Path) -> Vec<String> {
        let mut r = self.clone();
        let own = r
            .sources
            .iter()
            .find(|(_, s)| same_file(s, path))
            .map(|(id, _)| id.clone());
        if let Some(id) = own {
            r.remove(&id);
        }
        match r.add_file(path) {
            Ok(id) => r.check_preset(&id).err().map(|e| e.to_string()).into_iter().collect(),
            Err(e) => vec![e.to_string()],
        }
    }

    /// `sdc presets export` (spec §4, Note T8): `id`'s own table as loaded (`base` stays a link) under `new_id`, so it
    /// loads beside the original.
    pub fn export(&self, id: &str, new_id: &str) -> Result<String, PresetError> {
        let mut t = self
            .tables
            .get(id)
            .cloned()
            .ok_or_else(|| PresetError::Unknown(id.to_string()))?;
        t.insert("id".into(), toml::Value::String(new_id.to_string()));
        toml::to_string(&t).map_err(|e| PresetError::Invalid(e.to_string()))
    }
```

Run `cargo test -p engine --offline --test presets` and check that it passes (PASS).

- [ ] **Step 4: Write the failing CLI tests**

`crates/cli/tests/cli_presets.rs`:
- Add a helper that runs `sdc` with `APPDATA` set to a given folder.
- Add tests for each verb.
- Leave the existing `sdc()` helper as is: it points `APPDATA` at a missing folder.

```rust
fn sdc_in(appdata: &std::path::Path, args: &[&str]) -> (i32, String, String) {
    let o = Command::new(env!("CARGO_BIN_EXE_sdc"))
        .env("APPDATA", appdata)
        .args(args)
        .output()
        .unwrap();
    (
        o.status.code().unwrap(),
        String::from_utf8_lossy(&o.stdout).into_owned(),
        String::from_utf8_lossy(&o.stderr).into_owned(),
    )
}

const MINE: &str = "schema_version = 1\nid = \"mine\"\nversion = \"1.0.0\"\nbase = \"cs-quality\"\n[subtypes.diffuse]\nformat = \"bc1\"\n";

fn appdata_with_mine() -> tempfile::TempDir {
    let d = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(d.path().join("sdc/presets")).unwrap();
    std::fs::write(d.path().join("sdc/presets/mine.toml"), MINE).unwrap();
    std::fs::write(d.path().join("sdc/presets/broken.toml"), "nope").unwrap();
    d
}

/// Spec §4: `list` shows a folder preset with its source; `--json` has `source` on every row.
#[test]
fn list_shows_user_presets_with_their_source() {
    let d = appdata_with_mine();
    let (code, out, err) = sdc_in(d.path(), &["presets", "list"]);
    assert_eq!(code, 0, "{err}");
    let line = out.lines().find(|l| l.starts_with("mine")).unwrap();
    assert!(line.contains("mine.toml"), "{line}");
    assert!(out.contains("cs-compact") && out.contains("fast-preview"), "{out}");
    assert!(err.contains("CFG-PRESET-SKIPPED") && err.contains("broken.toml"), "{err}");
    let (_, out, _) = sdc_in(d.path(), &["presets", "list", "--json"]);
    let rows: Vec<serde_json::Value> = out.lines().map(|l| serde_json::from_str(l).unwrap()).collect();
    let src = |id: &str| rows.iter().find(|r| r["id"] == id).unwrap()["source"].clone();
    assert_eq!(src("cs-quality"), "builtin");
    assert!(src("mine").as_str().unwrap().ends_with("mine.toml"));
}

/// Spec §3: `--preset-file` works on every verb, and a duplicate id is an error (exit 2).
#[test]
fn preset_file_on_the_verbs_and_duplicates() {
    let d = appdata_with_mine();
    let extra = d.path().join("extra.toml");
    std::fs::write(&extra, MINE.replace("\"mine\"", "\"extra\"")).unwrap();
    let (code, out, _) = sdc_in(d.path(), &["presets", "list", "--preset-file", extra.to_str().unwrap()]);
    assert_eq!(code, 0);
    assert!(out.lines().any(|l| l.starts_with("extra")), "{out}");
    let dup = d.path().join("dup.toml");
    std::fs::write(&dup, MINE).unwrap();
    let (code, _, err) = sdc_in(d.path(), &["presets", "show", "mine", "--preset-file", dup.to_str().unwrap()]);
    assert_eq!(code, 2, "{err}");
    assert!(err.contains("already loaded"), "{err}");
}

/// Spec §4: `diff` prints `<subtype>.<field>: a → b`, `no differences`, or JSON rows.
#[test]
fn diff_prints_only_differences() {
    let d = appdata_with_mine();
    let (code, out, err) = sdc_in(d.path(), &["presets", "diff", "cs-quality", "mine"]);
    assert_eq!(code, 0, "{err}");
    assert_eq!(out.trim(), "diffuse.format: \"bc7\" → \"bc1\"");
    let (code, out, _) = sdc_in(d.path(), &["presets", "diff", "cs-quality", "cs-quality"]);
    assert_eq!((code, out.trim()), (0, "no differences"));
    let (code, out, _) = sdc_in(d.path(), &["presets", "diff", "cs-quality", "cs-compact", "--subtype", "normal", "--json"]);
    assert_eq!(code, 0);
    let rows: serde_json::Value = serde_json::from_str(&out).unwrap();
    assert_eq!(rows, serde_json::json!([{"subtype": "normal", "field": "resize", "a": null, "b": "half"}]));
    assert_eq!(sdc_in(d.path(), &["presets", "diff", "cs-quality", "nope"]).0, 2);
}

/// Spec §4: export writes files that validate; validate exits 1 on a bad file.
#[test]
fn export_then_validate() {
    let d = appdata_with_mine();
    let out = d.path().join("x.toml");
    let (code, _, err) = sdc_in(d.path(), &["presets", "export", "mine", "--out", out.to_str().unwrap()]);
    assert_eq!(code, 0, "{err}");
    let text = std::fs::read_to_string(&out).unwrap();
    assert!(text.contains("id = \"mine-export\"") && text.contains("base = \"cs-quality\""), "{text}");
    let renamed = d.path().join("packed.toml");
    let (code, _, _) = sdc_in(d.path(), &["presets", "export", "cs-compact", "--id", "packed", "--out", renamed.to_str().unwrap()]);
    assert_eq!(code, 0);
    assert!(std::fs::read_to_string(&renamed).unwrap().contains("id = \"packed\""));
    let (code, stdout, _) = sdc_in(d.path(), &["presets", "export", "fast-preview"]);
    assert_eq!(code, 0);
    assert!(stdout.contains("id = \"fast-preview-export\""), "{stdout}");
    let (code, stdout, _) = sdc_in(d.path(), &["presets", "validate", out.to_str().unwrap(), renamed.to_str().unwrap()]);
    assert_eq!(code, 0, "{stdout}");
    assert_eq!(stdout.lines().filter(|l| l.starts_with("ok ")).count(), 2, "{stdout}");
    let bad = d.path().join("bad.toml");
    std::fs::write(&bad, "schema_version = 1\nid = \"b\"\nversion = \"1\"\nbase = \"nope\"\n").unwrap();
    let (code, stdout, _) = sdc_in(d.path(), &["presets", "validate", out.to_str().unwrap(), bad.to_str().unwrap()]);
    assert_eq!(code, 1);
    assert!(stdout.lines().any(|l| l.starts_with(bad.to_str().unwrap()) && l.contains("nope")), "{stdout}");
}
```

`crates/cli/tests/cli_convert.rs` (spec §1 success criterion):

```rust
/// M6 Track D spec §1: `--preset cs-compact` writes an opaque colour map as BC1 and halves a data map (min 512).
#[test]
fn cs_compact_writes_bc1_colour_and_half_size_normals() {
    let fx = Fx::new();
    fx.rgb("wall.png");
    ImageBuffer::from_fn(1024, 1024, |_, _| Rgb([128u8, 128, 255]))
        .save(fx.p("in/textures/a/wall_n.png"))
        .unwrap();
    let o = fx.convert(&["--preset", "cs-compact", "--accept-suggestions", "high"]);
    assert_eq!(code(&o), 0, "{}", text(&o.stderr));
    assert_eq!(fx.format("textures/a/wall.dds"), DdsPixelFormat::Bc1 { srgb: false });
    let n = read_dds(&std::fs::read(fx.dds("textures/a/wall_n.dds")).unwrap()).unwrap();
    assert_eq!((n.width, n.height), (512, 512));
}
```

- [ ] **Step 5: Run them and check that they fail**

Run: `cargo test -p sdc --offline --test cli_presets` and `cargo test -p sdc --offline --test cli_convert cs_compact`
Expected:
- The `cli_presets` tests FAIL: `diff`, `export` and `validate` are unknown subcommands (exit 2), and `list` has no `source`.
- `cs_compact_writes_bc1_colour_and_half_size_normals` should already PASS through Tasks 5 and 7. It is the §1 criterion; keep it.

- [ ] **Step 6: Implement the CLI side**

`args.rs`: replace the `Presets` variant and its attribute in `Command`:

```rust
    /// Inspect, compare, export and validate presets, and print schemas. User presets in %APPDATA%\sdc\presets\*.toml
    /// are always loaded.
    Presets(PresetsArgs),
```

Add the struct, and extend `PresetsCmd`:

```rust
#[derive(Args)]
pub struct PresetsArgs {
    /// Load a preset .toml after the user presets folder (repeatable). An id that is already loaded is an error.
    #[arg(long = "preset-file", global = true)]
    pub preset_file: Vec<PathBuf>,
    #[command(subcommand)]
    pub cmd: PresetsCmd,
}
```

```rust
    /// Print the fields where two presets resolve differently: `<subtype>.<field>: <a> → <b>`.
    Diff {
        a: String,
        b: String,
        #[arg(long)]
        subtype: Option<String>,
        #[arg(long, value_enum)]
        target: Option<TargetArg>,
        #[arg(long)]
        json: bool,
    },
    /// Write a preset's own TOML (keeping its `base` link) under a new id (default `<id>-export`).
    Export {
        id: String,
        #[arg(long)]
        out: Option<PathBuf>,
        /// Id of the exported preset.
        #[arg(long = "id")]
        new_id: Option<String>,
    },
    /// Check preset files (migration, parse, ids, base, aliases, rules, every profile). Exit 1 if any has a problem.
    Validate {
        #[arg(required = true)]
        files: Vec<PathBuf>,
    },
```

Change the `List` doc to `/// List selectable presets, built-in and user (with their source file).`

`verbs.rs`:
- Change the signature, and build the registry: built-ins, then the folder, then the files. Import `crate::args::{user_presets_dir, PresetsArgs}` and `engine::presets::leaf_paths`.

```rust
/// `sdc presets …`. Result verb: the report goes to stdout (DECISIONS Note J); skipped user presets go to stderr.
pub fn presets(args: PresetsArgs) -> Result<i32, EngineError> {
    let engine = Engine::new()?;
    let mut reg = engine.registry().clone();
    if let Some(dir) = user_presets_dir() {
        for w in reg.add_dir(&dir) {
            eprintln!("{}: {}", w.code.as_str(), w.message);
        }
    }
    for f in &args.preset_file {
        reg.add_file(f)
            .map_err(|e| EngineError::Config(e.to_string()))?;
    }
    let reg = &reg;
    match args.cmd {
```

- In `List`, add the source. In the JSON object, add `"source": reg.source(&p.id).unwrap_or("builtin"),`. In the text line, append `  from <source>` for a non-built-in preset:

```rust
                    let from = match reg.source(&p.id) {
                        Some(s) if s != engine::presets::SOURCE_BUILTIN => format!("  from {s}"),
                        _ => String::new(),
                    };
```

  Then use `"{:<18} {:<8} {:<8} {}{aliases}{from}"`.

- Add the three arms:

```rust
        PresetsCmd::Diff { a, b, subtype, target, json } => {
            let usage = |e: engine::presets::PresetError| EngineError::Usage(e.to_string());
            reg.chain(&a).map_err(usage)?;
            reg.chain(&b).map_err(usage)?;
            let target = target
                .map(Target::from)
                .or_else(|| reg.chain(&a).ok()?.iter().rev().find_map(|p| p.target))
                .or_else(|| reg.chain(&b).ok()?.iter().rev().find_map(|p| p.target))
                .unwrap_or(Target::Cs);
            let subtypes: Vec<Subtype> = match subtype {
                Some(s) => vec![serde_json::from_value(Value::String(s.clone()))
                    .map_err(|_| EngineError::Usage(format!("--subtype {s}: unknown sub-type")))?],
                None => Subtype::ALL.to_vec(),
            };
            let mut rows = vec![];
            for st in subtypes {
                let pa = serde_json::to_value(reg.profile(&a, target, st).map_err(usage)?).unwrap_or_default();
                let pb = serde_json::to_value(reg.profile(&b, target, st).map_err(usage)?).unwrap_or_default();
                let mut fields = vec![];
                leaf_paths(&pa, "", &mut fields);
                leaf_paths(&pb, "", &mut fields);
                fields.sort();
                fields.dedup();
                let at = |v: &Value, f: &str| f.split('.').try_fold(v, |v, k| v.get(k)).cloned().unwrap_or(Value::Null);
                for f in fields {
                    let (va, vb) = (at(&pa, &f), at(&pb, &f));
                    if va != vb {
                        rows.push(json!({ "subtype": st, "field": f, "a": va, "b": vb }));
                    }
                }
            }
            if json {
                println!("{}", Value::Array(rows));
            } else if rows.is_empty() {
                println!("no differences");
            } else {
                for r in &rows {
                    println!(
                        "{}.{}: {} \u{2192} {}",
                        r["subtype"].as_str().unwrap_or_default(),
                        r["field"].as_str().unwrap_or_default(),
                        r["a"],
                        r["b"]
                    );
                }
            }
            Ok(0)
        }
        PresetsCmd::Export { id, out, new_id } => {
            let new_id = new_id.unwrap_or_else(|| format!("{id}-export"));
            let text = reg
                .export(&id, &new_id)
                .map_err(|e| EngineError::Usage(e.to_string()))?;
            match out {
                Some(p) => std::fs::write(&p, &text)
                    .map_err(|e| EngineError::Fatal(format!("{}: {e}", p.display())))?,
                None => print!("{text}"),
            }
            Ok(0)
        }
        PresetsCmd::Validate { files } => {
            let mut bad = false;
            for f in &files {
                let problems = reg.validate_file(f);
                if problems.is_empty() {
                    println!("ok {}", f.display());
                }
                for p in &problems {
                    println!("{}: {p}", f.display());
                }
                bad |= !problems.is_empty();
            }
            Ok(i32::from(bad))
        }
```

  The existing `Show` and `Schema` arms are unchanged; they now read `reg`, which includes the folder and the files.

`main.rs:63`: `Command::Presets(c) => verbs::presets(c),` is unchanged; `c` is now `PresetsArgs`.

- [ ] **Step 7: Run the tests and check that they pass**

Run: `cargo test -p sdc --offline --test cli_presets --test cli_convert`, then `cargo test --workspace --offline`.
Expected: PASS.
- The `diff` row for `resize` has `"a": null`: `none` is not serialised, so `leaf_paths` sees the field only on `b`. This matches the JSON in the test.
- `presets_show_reports_provenance_and_rejects_bad_arguments` still passes: `show` behaves as before.

- [ ] **Step 8: Run the gates, then commit**

```bash
git add crates/engine/src/presets.rs crates/engine/tests/presets.rs crates/cli/src/args.rs crates/cli/src/verbs.rs crates/cli/src/main.rs crates/cli/tests/cli_presets.rs crates/cli/tests/cli_convert.rs
git commit -m "feat(cli): sdc presets diff, export, validate; list shows user preset sources; --preset-file on every presets verb (M6 Track D spec §4)

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

---

### Task 9: GUI — the user presets folder and the loaded user presets in the preset inspector

**Spec:** §3 (the GUI loads the folder, and the inspector shows the loaded user presets' ids and files; there is no picker). Ruling T6.

**Files:**
- Modify: `crates/gui/src/main.rs:9-12, 24` (the path, set on the model).
- Modify: `crates/gui/src/model/mod.rs`:
  - `AppModel` fields (`:150-152`);
  - `AppModel::new`/placeholder initialisers (`:231`);
  - `request` (`:383`);
  - `apply_plan` (`:998`).
- Modify: `crates/gui/src/model/items.rs:81, 300-306` (`PlanView`).
- Modify: `crates/gui/src/ui/advanced.rs:131-140` (`inspector`).
- Test: `crates/gui/tests/advanced.rs`.

**Interfaces:**
- Consumes:
  - `PresetRegistry::user_presets()` (Task 1);
  - `BatchRequest::user_presets` (Task 5);
  - `Plan::registry` (`api.rs:189`).
- Produces:
  - `AppModel::user_presets: Option<PathBuf>`: the folder. It is set by `main` and `None` in tests.
  - `AppModel::loaded_user_presets: Vec<(String, String)>`: `(id, source file)` from the last plan.
  - `PlanView::user_presets: Vec<(String, String)>`.

- [ ] **Step 1: Write the failing test** (`crates/gui/tests/advanced.rs`)

```rust
/// M6 Track D spec §3: the GUI loads the user presets folder and the inspector lists what it loaded.
#[test]
fn the_inspector_lists_loaded_user_presets() {
    let d = tempfile::tempdir().unwrap();
    let presets = d.path().join("user-presets");
    std::fs::create_dir_all(&presets).unwrap();
    std::fs::write(
        presets.join("mine.toml"),
        "schema_version = 1\nid = \"mine\"\nversion = \"1.0.0\"\nbase = \"cs-quality\"\n",
    )
    .unwrap();
    let mut m = opened(d.path());
    assert!(m.loaded_user_presets.is_empty());
    m.user_presets = Some(presets.clone());
    m.replan();
    assert!(m.wait_idle(Duration::from_secs(60)));
    assert_eq!(m.loaded_user_presets.len(), 1);
    assert_eq!(m.loaded_user_presets[0].0, "mine");
    assert!(m.loaded_user_presets[0].1.ends_with("mine.toml"));
    m.settings.advanced_open = true;
    m.select_item("landscape/leaf.png");
    let mut h = harness(m);
    h.run();
    h.get_by_label_contains("mine.toml");
}
```

- [ ] **Step 2: Run it and check that it fails**

Run: `cargo test -p sdc-gui --offline --test advanced the_inspector_lists_loaded_user_presets`
Expected: a compile error (`loaded_user_presets`, `user_presets` not found).

- [ ] **Step 3: Implement**

`main.rs`:

```rust
/// `%APPDATA%\sdc\presets`, the CLI's user presets folder (M6 Track D spec §3).
fn user_presets_path() -> Option<PathBuf> {
    std::env::var_os("APPDATA").map(|a| PathBuf::from(a).join("sdc").join("presets"))
}
```

After `m.user_config = user_config_path();`, add `m.user_presets = user_presets_path();`.

`model/mod.rs`, after the `user_config` field:

```rust
    /// `%APPDATA%\sdc\presets`, set by `main`; `None` in tests so a developer's own presets cannot leak in.
    pub user_presets: Option<PathBuf>,
    /// `(id, file)` of the user presets the last plan loaded (the preset inspector lists them).
    pub loaded_user_presets: Vec<(String, String)>,
```

Initialise both (`user_presets: None, loaded_user_presets: vec![],`) wherever `user_config: None` is initialised. In `request`, after `req.user_config = self.user_config.clone();`, add `req.user_presets = self.user_presets.clone();`. In `apply_plan`, after `self.batch_preset = view.preset_id;`, add `self.loaded_user_presets = view.user_presets;`.

`model/items.rs`, `PlanView`, after `preset_id`:

```rust
    /// `(id, file)` of every user preset the plan's registry loaded.
    pub user_presets: Vec<(String, String)>,
```

and where `PlanView` is built:

```rust
            user_presets: plan
                .registry
                .user_presets()
                .into_iter()
                .map(|(id, src)| (id.to_string(), src.to_string()))
                .collect(),
```

`ui/advanced.rs`, in `inspector`, after the `Batch preset:` label:

```rust
        if model.loaded_user_presets.is_empty() {
            ui.label(
                egui::RichText::new("No user presets (add .toml files to %APPDATA%\\sdc\\presets).")
                    .small()
                    .weak(),
            );
        } else {
            ui.label("User presets:");
            for (id, file) in &model.loaded_user_presets {
                ui.label(egui::RichText::new(format!("{id} — {file}")).small());
            }
        }
```

If another struct literal builds `PlanView`, add `user_presets: vec![]` to it (`grep -rn "PlanView {" crates/gui`).

- [ ] **Step 4: Run the tests and check that they pass**

Run: `cargo test -p sdc-gui --offline --test advanced`, then `cargo test --workspace --offline`.
Expected: PASS. `the_open_drawer_shows_its_sections` still passes; the new label is extra text.

- [ ] **Step 5: Run the gates, then commit**

```bash
git add crates/gui/src/main.rs crates/gui/src/model/mod.rs crates/gui/src/model/items.rs crates/gui/src/ui/advanced.rs crates/gui/tests/advanced.rs
git commit -m "feat(gui): load the user presets folder and list the loaded user presets in the preset inspector (M6 Track D spec §3)

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

---

### Task 10: BC4/BC5 search radius — measure on the bench corpus, then apply the decision rules

**Spec:** §7. Ruling T11. This is a measurement task. Its numbers decide what Steps 6–8 write into the presets, through fixed rules.

**Files:**
- Create: `crates/bench/src/radius.rs`.
- Modify: `crates/bench/src/lib.rs` (`pub mod radius;`).
- Modify: `crates/bench/src/main.rs` (the `Radius45` subcommand in `Cmd` and its `match` arm).
- Create: `docs/spikes/bc45-search-radius.md`.
- Then, depending on the outcome (Steps 6–8):
  - `presets/cs-compact.toml`, `presets/vanilla-compact.toml`, `presets/fast-preview.toml`;
  - rule F: `crates/encoders-sys/src/rgbcx.rs:5-8`, `crates/engine/src/config.rs:93`, `crates/engine/src/encode.rs:51-56`, `crates/gui/src/ui/settings_panel.rs:181-186`;
  - rule Q: `presets/sdc-base.toml`;
  - `crates/engine/tests/presets.rs`;
  - `schema/` (rule F changes the profile schema).

**Hard limits for this task:**
- Read only `F:/ai/dds/bench/images2/mask_real/*.png` and `F:/ai/dds/bench/images2/normal_real/*.png`.
- Write only to the new scratch folder `F:/ai/dds/converter/target/bench/radius45` (git-ignored through `target/`). It must not exist before Step 4. If it does, stop and report.
- Never write anything under `F:/ai/dds/bench`: no `images3`, no `out3`, no `results*.csv`.
- `G:/` is read-only.
- `radius::check_out` refuses an output inside the bench root.

**Interfaces:**
- Consumes:
  - `encoders_sys::{bc4_encode_block, bc5_encode_block, gather_blocks}`;
  - `engine::decode::{decode_level, Decoded}`;
  - `engine::dds::DdsPixelFormat`;
  - `sdc_bench::corpus::{bench_root, load_png_rgba8, set_files, stem}`;
  - `sdc_bench::metrics::{channel_psnr, mean}`.
- Produces:
  - `pub fn radius::check_out(root: &Path, out: &Path) -> anyhow::Result<()>`
  - `pub fn radius::measure(root: &Path, out: &Path) -> anyhow::Result<Vec<Row>>`
  - `pub fn radius::summary(rows: &[Row]) -> Vec<(&'static str, u32, f64, f64)>`: (format, radius, mean PSNR dB, total seconds).
  - `pub fn radius::decide(summary: &[(&str, u32, f64, f64)]) -> (bool, Option<u32>)`: (rule Q fires, rule F radius).
  - If rule F fires: `SearchRadius::Fast` (`"fast"`) and `encoders_sys::BC45_RADIUS_FAST`.

- [ ] **Step 1: Write the failing tests** (bottom of the new `crates/bench/src/radius.rs`)

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_output_must_be_outside_the_bench_root() {
        let root = Path::new("F:/ai/dds/bench");
        assert!(check_out(root, Path::new("F:/ai/dds/bench/out3/x")).is_err());
        assert!(check_out(root, Path::new("F:/AI/dds/Bench/images3")).is_err());
        assert!(check_out(root, Path::new("F:/ai/dds/bench2/x")).is_ok());
        assert!(check_out(root, Path::new("F:/ai/dds/converter/target/bench/radius45")).is_ok());
    }

    fn both(v: &[(u32, f64, f64)]) -> Vec<(&'static str, u32, f64, f64)> {
        ["bc4", "bc5"]
            .iter()
            .flat_map(|f| v.iter().map(move |&(r, p, t)| (*f, r, p, t)))
            .collect()
    }

    /// DECISIONS Note T11: Q needs >= 0.10 dB at <= 1.5x time on both formats; F is the smallest radius below 5
    /// within 0.10 dB of radius 5 on both formats.
    #[test]
    fn decision_rules() {
        let a = both(&[(0, 40.0, 1.0), (1, 40.5, 1.0), (2, 40.8, 1.0), (3, 40.95, 1.0), (4, 41.0, 1.0), (5, 41.0, 1.0), (10, 41.2, 1.4)]);
        assert_eq!(decide(&a), (true, Some(3)));
        let b = both(&[(0, 30.0, 1.0), (1, 30.0, 1.0), (2, 30.0, 1.0), (3, 30.0, 1.0), (4, 40.0, 1.0), (5, 41.0, 1.0), (10, 41.2, 2.0)]);
        assert_eq!(decide(&b), (false, None));
        let mut c = a.clone();
        c.retain(|s| !(s.0 == "bc5" && s.1 == 10));
        c.push(("bc5", 10, 41.05, 1.0));
        assert!(!decide(&c).0, "BC5 gains only 0.05 dB");
    }
}
```

and `pub mod radius;` in `crates/bench/src/lib.rs`.

- [ ] **Step 2: Run them and check that they fail**

Run: `cargo test -p sdc-bench --offline --lib radius::`
Expected: a compile error (`check_out`, `decide` not found).

- [ ] **Step 3: Implement `radius.rs` and the subcommand** (above the test module)

```rust
//! M6 Track D spec §7 (DECISIONS Note T11): BC4/BC5 search radius against PSNR and encode time on the bench corpus.
//! Inputs (`images2/mask_real` as BC4 on R, `images2/normal_real` as BC5 on R and G) are only read. The CSV goes to
//! `<out>/radius45.csv`, and `out` must not be inside the bench root.
use crate::corpus::{load_png_rgba8, set_files, stem};
use crate::metrics::{channel_psnr, mean};
use anyhow::{bail, Result};
use encoders_sys::{bc4_encode_block, bc5_encode_block, gather_blocks};
use engine::dds::DdsPixelFormat;
use engine::decode::{decode_level, Decoded};
use rayon::prelude::*;
use std::path::{Path, PathBuf};
use std::time::Instant;

/// 5 is the current `default` (`BC45_RADIUS_DEFAULT`), 10 is `max`.
pub const RADII: [u32; 7] = [0, 1, 2, 3, 4, 5, 10];

#[derive(Debug, Clone)]
pub struct Row {
    pub format: &'static str,
    pub image: String,
    pub radius: u32,
    pub psnr: f64,
    pub secs: f64,
}

/// Refuse an output folder inside the bench root (spec §7: nothing is written there). Windows paths are
/// case-insensitive, so both are compared lower-cased, component by component.
pub fn check_out(root: &Path, out: &Path) -> Result<()> {
    let norm = |p: &Path| -> Result<PathBuf> {
        Ok(PathBuf::from(std::path::absolute(p)?.to_string_lossy().to_lowercase()))
    };
    if norm(out)?.starts_with(norm(root)?) {
        bail!(
            "{} is inside the bench root {}; use a scratch folder",
            out.display(),
            root.display()
        );
    }
    Ok(())
}

fn encode(format: &str, blocks: &[[u8; 4]], r: u32) -> Vec<u8> {
    blocks
        .par_chunks(16)
        .flat_map_iter(|b| {
            let b: &[[u8; 4]; 16] = b.try_into().expect("16 texels per block");
            if format == "bc4" {
                bc4_encode_block(b, 0, r).to_vec()
            } else {
                bc5_encode_block(b, r).to_vec()
            }
        })
        .collect()
}

pub fn measure(root: &Path, out: &Path) -> Result<Vec<Row>> {
    check_out(root, out)?;
    let mut rows = vec![];
    for (format, set, pf, ch) in [
        ("bc4", "mask_real", DdsPixelFormat::Bc4, &[0usize][..]),
        ("bc5", "normal_real", DdsPixelFormat::Bc5, &[0usize, 1][..]),
    ] {
        for src in set_files(root, "images2", set, "png")? {
            let img = load_png_rgba8(&src)?;
            let blocks = gather_blocks(&img.px, img.width, img.height);
            for r in RADII {
                let (mut secs, mut data) = (f64::INFINITY, vec![]);
                for _ in 0..3 {
                    let t = Instant::now();
                    data = encode(format, &blocks, r);
                    secs = secs.min(t.elapsed().as_secs_f64());
                }
                let Decoded::Ldr(dec) = decode_level(pf, img.width, img.height, &data, None)? else {
                    bail!("{format} did not decode to LDR");
                };
                rows.push(Row { format, image: stem(&src), radius: r, psnr: channel_psnr(&img.px, &dec, ch), secs });
            }
        }
    }
    std::fs::create_dir_all(out)?;
    let mut csv = String::from("format,image,radius,psnr_db,secs\n");
    for r in &rows {
        csv += &format!("{},{},{},{:.4},{:.6}\n", r.format, r.image, r.radius, r.psnr, r.secs);
    }
    std::fs::write(out.join("radius45.csv"), csv)?;
    Ok(rows)
}

/// Per format and radius: (format, radius, mean PSNR dB, total seconds).
pub fn summary(rows: &[Row]) -> Vec<(&'static str, u32, f64, f64)> {
    let mut v = vec![];
    for format in ["bc4", "bc5"] {
        for r in RADII {
            let sel: Vec<&Row> = rows.iter().filter(|x| x.format == format && x.radius == r).collect();
            if !sel.is_empty() {
                let p: Vec<f64> = sel.iter().map(|x| x.psnr).collect();
                v.push((format, r, mean(&p), sel.iter().map(|x| x.secs).sum()));
            }
        }
    }
    v
}

/// DECISIONS Note T11. Q: radius 10 gains >= 0.10 dB over 5 at <= 1.5x the time, on both formats. F: the smallest
/// radius below 5 that loses <= 0.10 dB against 5, on both formats.
pub fn decide(summary: &[(&str, u32, f64, f64)]) -> (bool, Option<u32>) {
    let at = |f: &str, r: u32| summary.iter().find(|s| s.0 == f && s.1 == r).map(|s| (s.2, s.3));
    let both = |p: &dyn Fn(&str) -> Option<bool>| ["bc4", "bc5"].iter().all(|f| p(f) == Some(true));
    let q = both(&|f| {
        let ((p5, t5), (p10, t10)) = (at(f, 5)?, at(f, 10)?);
        Some(p10 - p5 >= 0.10 && t10 <= 1.5 * t5)
    });
    let fast = [0, 1, 2, 3, 4].into_iter().find(|&r| {
        both(&|f| {
            let ((p5, _), (pr, _)) = (at(f, 5)?, at(f, r)?);
            Some(p5 - pr <= 0.10)
        })
    });
    (q, fast)
}
```

`crates/bench/src/main.rs`. In `enum Cmd`:

```rust
    /// M6 Track D spec §7: BC4/BC5 search radius against PSNR and encode time (images2 mask_real, normal_real;
    /// read-only). Writes <out>/radius45.csv.
    Radius45 {
        /// A scratch folder outside the bench root.
        #[arg(long)]
        out: std::path::PathBuf,
    },
```

In `match cmd`:

```rust
        Cmd::Radius45 { out } => match sdc_bench::radius::measure(&root, &out) {
            Ok(rows) => {
                let s = sdc_bench::radius::summary(&rows);
                println!("format radius mean_psnr_db total_secs");
                for (f, r, p, t) in &s {
                    println!("{f} {r} {p:.4} {t:.4}");
                }
                let (q, fast) = sdc_bench::radius::decide(&s);
                println!("rule Q (quality default -> max): {q}; rule F (fast radius): {fast:?}");
                ExitCode::SUCCESS
            }
            Err(e) => {
                eprintln!("radius45: {e:#}");
                ExitCode::FAILURE
            }
        },
```

Run: `cargo test -p sdc-bench --offline --lib radius::`. Expected: PASS. Run the gates, then commit:

```bash
git add crates/bench/src/radius.rs crates/bench/src/lib.rs crates/bench/src/main.rs
git commit -m "feat(bench): radius45 measures BC4/BC5 search radius vs PSNR and time into a scratch folder (M6 Track D spec §7)

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

- [ ] **Step 4: Measure** (from `F:/ai/dds/converter`; confirm first that `target/bench/radius45` does not exist)

Run: `cargo run -p sdc-bench --release --offline -- radius45 --out target/bench/radius45`
Expected:
- 14 summary rows (2 formats × 7 radii);
- one decision line, `rule Q (quality default -> max): <bool>; rule F (fast radius): <None | Some(r)>`;
- `target/bench/radius45/radius45.csv`.

Afterwards, `git status --short` must show no change outside `crates/` and `docs/`. Nothing in `F:/ai/dds/bench` may change; check it with `git -C F:/ai/dds/bench status --short` if that folder is a repository, or by comparing the folder modification times of `images3`, `out3` and `results` before and after.

- [ ] **Step 5: Write `docs/spikes/bc45-search-radius.md`**

Write it in this layout:

```markdown
# BC4/BC5 search radius (M6 Track D spec §7, DECISIONS Note T11)

Date: 2026-10-12. Command: `cargo run -p sdc-bench --release --offline -- radius45 --out target/bench/radius45`.
Corpus (read-only): `F:/ai/dds/bench/images2/mask_real` (BC4, R; <n> images), `images2/normal_real` (BC5, R+G; <n> images).
Encoder: rgbcx `encode_bc4_hq`/`encode_bc5_hq` through `encoders-sys`; decoder: bcdec_rs. Time: best of 3, all threads.

| format | radius | mean PSNR (dB) | Δ vs 5 (dB) | time (s) | × time of 5 |
|---|---|---|---|---|---|
| (14 rows from the run) |

## Decision
- Rule Q (quality default becomes `max` if +0.10 dB at ≤ 1.5× time on both formats): <fires / does not fire>, because <numbers>.
- Rule F (smallest radius < 5 within 0.10 dB of 5 on both formats): <r / none>, because <numbers>.
- Applied: <what Steps 6–8 changed>.
```

Fill in every `<…>` from the run's output and the CSV. The `<…>` fields here are data slots for this step, not plan placeholders.

- [ ] **Step 6: Apply rule F**

**If rule F printed `Some(r)`:**
- `crates/encoders-sys/src/rgbcx.rs`, after `BC45_RADIUS_MAX`, add the following. `r` is the literal number rule F printed:

```rust
/// Spec value `fast` (M6 Track D, DECISIONS Note T11): the smallest radius within 0.1 dB of `default` on the bench
/// corpus (docs/spikes/bc45-search-radius.md).
pub const BC45_RADIUS_FAST: u32 = r;
```

- `config.rs:93`: `cfg_enum!(SearchRadius, "lowercase" { Default, Max, Fast });`
- `encode.rs` `radius`: add `SearchRadius::Fast => BC45_RADIUS_FAST,`, and import it beside `BC45_RADIUS_DEFAULT`.
- `settings_panel.rs`, in the `"encoder.bc45_search_radius"` options: add `opt(json!("fast"), "Fast (preview)"),` after `Maximum`.
- `presets/cs-compact.toml` and `presets/vanilla-compact.toml`: add this after the `description` line, before the rules:

```toml
[defaults]
encoder = { bc45_search_radius = "fast" }
```

- `presets/fast-preview.toml`: `encoder = { effort = "fast", bc45_search_radius = "fast" }`.
- `crates/engine/tests/presets.rs`, in `compact_and_fast_preview_resolve_as_designed`, add:

```rust
    for id in ["cs-compact", "vanilla-compact", "fast-preview"] {
        let t = r.get(id).unwrap().target.unwrap_or(Target::Cs);
        assert_eq!(r.profile(id, t, Subtype::Envmask).unwrap().encoder.bc45_search_radius, SearchRadius::Fast);
    }
    assert_eq!(r.profile("cs-quality", Target::Cs, Subtype::Envmask).unwrap().encoder.bc45_search_radius, SearchRadius::Default);
```

**If rule F printed `None`:** change no code for F. Add to the same test:

```rust
    for id in ["cs-compact", "vanilla-compact", "fast-preview"] {
        let t = r.get(id).unwrap().target.unwrap_or(Target::Cs);
        assert_eq!(
            r.profile(id, t, Subtype::Envmask).unwrap().encoder.bc45_search_radius,
            r.profile(engine::presets::default_preset_for(t), t, Subtype::Envmask).unwrap().encoder.bc45_search_radius
        );
    }
```

- [ ] **Step 7: Apply rule Q**

**If rule Q printed `true`:**
- `presets/sdc-base.toml`:
  - change `bc45_search_radius = "default"` to `"max"` in the `encoder` line of `[defaults]`;
  - change `version = "1.1.0"` to `"1.2.0"`.
- If rule F printed `None`, the compact presets and `fast-preview` must keep the *old* default (spec §7: "or the current default if none qualifies"). Write `bc45_search_radius = "default"` into them, as in Step 6 but with `"default"`.
- Update the Step 6 assertions to match: `cs-quality` expects `SearchRadius::Max`.
- `builtin_defaults_match_spec_table_4_2` (`tests/presets.rs:33`) may assert `default`. Change it to `max` and name it in the report.

**If rule Q printed `false`:** change nothing. `cs-quality` keeps `SearchRadius::Default`.

- [ ] **Step 8: Run the tests, regenerate the schemas if rule F fired, run the gates, commit**

Run: `cargo test --workspace --offline`. Expected: PASS.

```bash
cargo run -p sdc --offline -- presets schema all --out schema   # only if rule F fired
git add docs/spikes/bc45-search-radius.md presets crates/engine/tests/presets.rs
# rule F only:
git add crates/encoders-sys/src/rgbcx.rs crates/engine/src/config.rs crates/engine/src/encode.rs crates/gui/src/ui/settings_panel.rs schema
git commit -m "feat(presets): BC4/BC5 search radius from the bench measurement (rule Q: <fires|holds>, rule F: <r|none>) (M6 Track D spec §7)

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

Fill in the commit subject's `<…>` with the outcome. Report the 14 summary rows and both rule outcomes to the controller: Task 11 copies them into Note T11 and the changelog.

---

### Task 11: Docs — DECISIONS Note T, `docs/presets-changelog.md`, CLI help and `docs/gui.md`, workspace version 0.5.0

**Spec:** §7 (version), §8 (docs). Rulings T1–T12.

**Files:**
- Modify: `docs/DECISIONS.md`. Append `## Note T — M6 Track D: presets (plan rulings, 2026-10-12)` at the end of the file, after Note S's last bullet ("**Also (Track A).** …"). Start it with this paragraph: "Rulings of `docs/superpowers/plans/2026-10-12-sdc-m6-presets.md` on spec `docs/superpowers/specs/2026-10-12-sdc-m6-presets-design.md` (owner-approved 2026-10-12; §2.2 corrected by the owner in 956da58)." Then add one bullet per ruling, in the form `- **T<n> <title> (owner / plan, 2026-10-12).** <ruling>`:
  - **T1 Preset ids.** `cs-compact` (base `cs-quality`, target cs) and `vanilla-compact` (base `vanilla-quality`, target vanilla). There is one `fast-preview` (base `sdc-base`, no target): the registry needs no target, and the batch target decides. It does not inherit later `*-quality` settings.
  - **T2 BC1 rule mechanics.**
    - `RuleMatch` had no sub-type matcher, so it gained `match.subtypes` (matched while layering, never while deciding a sub-type).
    - `match.alpha` matches the source's alpha content, so there are two rules: `none`, and `constant255` for sub-types whose policy resolves to `remove`.
    - No resolved-policy matcher was added (it would be circular).
    - An explicit non-`remove` policy on an opaque colour map under compact is `FMT-BC1-ALPHA`.
    - `bc1_transparent_black` stays false.
  - **T3 The colour and data lists**, exactly as in the plan.
    - `landscape_diffuse` is excluded from BC1 (`FMT-LANDSCAPE-BC1`); like every BC7 colour sub-type it halves (owner, 2026-10-12). BC1 colour stays full size (its rules set `resize = "none"`).
    - `pbr_emissive` (BC6H) and `hdr_sky` stay full size.
    - `pbr_subsurface` and `pbr_fuzz` are BC1 only without alpha.
    - `hdr_sky` excluded from resize (owner, 2026-10-12).
  - **T4 `resize = half`** (owner, 2026-10-12): both sides halve together when the larger is above 512 (`HALF_FLOOR`); the shape is kept; there is no POT step; each side is at least 1 px; `DIM-RESIZED` names the mode; admission counts it like `pot-down`.
  - **T5 `validation.mips = smallest`:** PSNR and coverage on the last mip only. The default `all` is not serialised.
  - **T6 The user presets folder.**
    - Location: `%APPDATA%\sdc\presets\*.toml`, non-recursive, in file-name order.
    - Load order: all files load first, then each preset is checked, so a base may come later in the order. Failures are skipped with `CFG-PRESET-SKIPPED`, repeatedly, so orphans go too.
    - Path: `BatchRequest::user_presets`, set by the CLI and the GUI, `None` in tests.
    - The same file reached twice loads once.
  - **T7 Duplicate ids are refused** (carried fix 1), naming both sources: `CFG-PRESET-SKIPPED` in the folder, an error from `--preset-file`. `is_builtin` means "loaded from the embedded list".
  - **T8 Verbs.**
    - `diff` compares profiles from presets alone (rules are not shown: spec gap).
    - `export` writes the preset's own TOML (`base` stays a link) under a new id (`<id>-export` or `--id`); no flattening option (owner, 2026-10-12).
    - `validate` reports the first problem per file, and validates a folder file in place of itself.
  - **T9 Migration table.** Presets none, config none, manifest records v1 → v2 (`schema` set to 2; the manifest's key is `schema`). Version 0 is refused. `PresetError::NewerConfigSchema(file, v)` is the spec's `ConfigError::NewerSchema`. Config files accept an optional `schema_version`.
  - **T10 Carried fixes.**
    - #2: `merge_json`, where a `null` patch never adds a key (the spec's two sentences disagreed; the function's contract won).
    - #3: a rule whose `subtype` is not the final sub-type does not apply. This includes a `CFG-UNAVAILABLE` fallback.
    - #4: `CFG-GREEN-CONFLICT`; the explicit `flip_green` wins.
    - #5: the landscape match was already whole-component; pinned by tests.
    - #6: `hdr_sky` only under a `sky` component.
  - **T11 BC4/BC5 search radius.** Copy the decision lines and the summary table from `docs/spikes/bc45-search-radius.md` (Task 10): rule Q's outcome with the BC4 and BC5 numbers, rule F's outcome and value, and what changed.
  - **T12 Versions.** Codes 73 → 75. `sdc-base` 1.1.0 → 1.2.0 only if rule Q fired. Workspace 0.4.0 → 0.5.0 so `if-changed` re-encodes.
- Create: `docs/presets-changelog.md` (the carried R14 item):

```markdown
# Built-in presets changelog

One entry per built-in preset version. A version changes when anything the preset resolves to changes; the workspace
version changes with it so `--on-existing if-changed` re-encodes (spec §3.6).

## 2026-10-12 — sdc 0.5.0 (M6 Track D)

- **cs-compact 1.0.0** (new; base cs-quality, target cs): BC1 for opaque colour maps (no alpha, or alpha constant
  255 where the policy resolves to remove); colour maps that stay BC7 and data maps (normals, masks, packed and single-channel maps) at half size,
  both sides, when the larger side is above 512; opaque colour that becomes BC1, emissive and sky stay full size. BC4/BC5 search radius: <from Task 10>.
- **vanilla-compact 1.0.0** (new; base vanilla-quality, target vanilla): the same for Vanilla/ENB.
- **fast-preview 1.0.0** (new; base sdc-base, any target): bc7e `fast`, per-mip PSNR and coverage validation on the
  smallest mip only. BC4/BC5 search radius: <from Task 10>. For checking a mod quickly, not for release.
- **sdc-base**: <"unchanged (1.1.0)" if rule Q did not fire; "1.2.0: BC4/BC5 search radius default → max (+<x> dB
  BC4, +<y> dB BC5 at <t>× time)" if it did>. cs-quality, vanilla-quality and cpm-quality inherit it.

## Earlier versions

- **sdc-base 1.1.0**, **cs-quality 1.0.0**, **vanilla-quality 1.0.0**, **cpm-quality 1.0.0**: as shipped before M6
  Track D (no changelog was kept).
```

  The `<…>` fields are filled from Task 10's report; they are data, not plan placeholders.
- Modify: `docs/gui.md`.
  - In "Where settings live" (`:96-98`), add this sentence: "User presets in `%APPDATA%\sdc\presets\*.toml` are loaded by the GUI and the command line alike; a file that fails to load is skipped. The preset inspector in the Advanced drawer lists the user presets that loaded, with their files. There is no preset picker yet: select a user preset in `sdc.toml` (`preset = \"<id>\"`) or the user config."
  - In the Advanced drawer sentence (`:20`), change "holds the preset inspector" to "holds the preset inspector (with the loaded user presets)".
- Modify: `Cargo.toml`, `[workspace.package] version`, `0.4.0` → **`0.5.0`**. Then `cargo build --workspace --offline` updates `Cargo.lock`.
- Test: `crates/cli/tests/cli_presets.rs`. The help of `convert` and `presets` names the folder (spec §8, "the CLI help mentions the user presets folder"; the strings were added in Tasks 5 and 8).

- [ ] **Step 1: Write the failing test** (`cli_presets.rs`)

```rust
/// M6 Track D spec §8: the CLI help names the user presets folder.
#[test]
fn help_names_the_user_presets_folder() {
    for args in [&["presets", "--help"][..], &["convert", "--help"]] {
        let (code, out, _) = stdout_of(args);
        assert_eq!(code, 0);
        assert!(out.contains(r"%APPDATA%\sdc\presets"), "{args:?}: {out}");
    }
}
```

- [ ] **Step 2: Run it.** Run: `cargo test -p sdc --offline --test cli_presets help_names`.
  - Expected: PASS, if Tasks 5 and 8 used the help strings above.
  - If it fails, the help strings are missing: add them as written in Task 5 (`--preset`) and Task 8 (`Command::Presets` doc). clap prints the variant doc in `sdc presets --help`.
- [ ] **Step 3: Write the DECISIONS, changelog and gui.md edits**, as listed above.
- [ ] **Step 4: Bump the version.** Run: `cargo build --workspace --offline`. Expected: `git diff --stat Cargo.lock` shows only the workspace crates' version lines changed.
- [ ] **Step 5: Run the gates (unpiped).** Expected: all green. `ENGINE_VERSION` follows `CARGO_PKG_VERSION`; no test pins `0.4.0`. If one does, update it and name it in the report.
- [ ] **Step 6: Commit (two commits, as Track A did)**

```bash
git add docs/DECISIONS.md docs/presets-changelog.md docs/gui.md crates/cli/tests/cli_presets.rs
git commit -m "docs: DECISIONS Note T (T1-T12), presets changelog, user presets folder in gui.md and CLI help (M6 Track D)

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
git add Cargo.toml Cargo.lock
git commit -m "chore: workspace version 0.5.0 (new presets and fields; if-changed re-encodes) (M6 Track D)

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

---

## Spec coverage

| Spec | Task |
|---|---|
| §1 success: `convert --preset cs-compact` gives BC1 opaque colour at full size and half-size everything else (except emissive and sky) | 7 (goldens), 8 (`cs_compact_writes_bc1_colour_and_half_size_normals`) |
| §1 success: folder preset in `list`, `--preset <id>`, GUI with no flag | 5, 8, 9 |
| §1 success: `diff`, `export`, `validate` on built-in and user presets | 8 |
| §1 success: every schema family migrates before deserialising | 2 |
| §2 three presets, embedded, v1/1.0.0, visible; the target question | 7; ruling T1 |
| §2.1 BC1 colour, colour with alpha stays BC7, never BC3, `bc1_transparent_black` false, the `RuleMatch` question | 3 (`subtypes`), 7; rulings T2, T3 |
| §2.2 `ResizeMode::Half` (owner correction), `DIM-RESIZED`, admission, data sub-types, packed outputs | 4, 7; rulings T3, T4 |
| §2.3 fast preview: effort fast, `validation.mips`, not serialised | 4, 7, 10 (radius); ruling T5 |
| §3 folder location, order, every verb and GUI, injectable path, `--preset-file` after the folder, bad files, duplicates, GUI inspector | 1, 5, 8, 9; rulings T6, T7 |
| §4 `list` source, `diff`, `export` (`--id`, stdout), `validate` | 1 (`check_preset`), 8; ruling T8 |
| §5 `migrate.rs`, three functions, step tables, newer behaviour, config `schema_version`, callers | 2; ruling T9 |
| §6 items 1–6, two new codes, count 75 | 1 (#1, codes), 3 (#2, #3), 6 (#4, #5, #6); ruling T10 |
| §7 radius measurement, report, decision rules, version | 10, 11; ruling T11 |
| §8 Note T, presets changelog, CLI help, gui.md | 11 |
| §9 goldens, presets tests, migrate tests, carried tests, `default_profiles_hash_as_before` | 7, 8, 2, 3, 6, 4 |
| §10 review focus 1–5 | 1, 1, 7, 8, 7 (Review Focus 1–5 above) |

## Spec gaps found while planning

1. **`RuleMatch` cannot match a sub-type at all** (§2.1 assumed only the alpha question). This is resolved by the allowed smallest extension, `match.subtypes` (T2).
2. **`alpha = "none"` misses opaque RGBA (`constant255`).** This is resolved with a second rule. An explicit non-`remove` policy on an opaque colour map under compact blocks with `FMT-BC1-ALPHA` (T2).
3. **`landscape_diffuse` cannot be BC1** (`FMT-LANDSCAPE-BC1`, Blocking). It is excluded from the colour rule and halves as BC7 colour (T3).
4. **`hdr_sky` is excluded from resize (owner, 2026-10-12).** It is a visible sky colour image, like `pbr_emissive` (T3).
5. **There is no `ConfigError` type.** The plan uses `PresetError::NewerConfigSchema` (T9).
6. **The manifest version key is `schema`, not `schema_version`** (T9).
7. **Carried fix #2's two sentences disagree** for a `null` patch on an absent key. The function's documented contract wins (T10).
8. **Carried fix #5 is already done on `main`.** Tests only (T10).
9. **`SearchRadius` is an enum (`default` = 5, `max` = 10), with no numeric knob**, so a measured smaller radius needs a new `fast` variant (T11).
10. **`diff` resolves presets alone (as `show` does), so it never shows rule differences**, such as the compact BC1 rules (T8).
11. **`export` must change the id**, or `validate` refuses the file as a duplicate of the loaded original (T8).
