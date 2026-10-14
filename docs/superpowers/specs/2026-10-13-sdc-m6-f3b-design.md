# sdc M6 Track F3b: input alias, output rename and auto PGPatcher rename (design)

Base: `main` ddae78d, branch `claude/project-thread-o2n463`. Inputs: the controller's decisions `2026-10-13-sdc-m6-f3b-decisions.md` (D1-D8, with the owner's card answer for D1) and the research `docs/research/bulk-editing.md` §3-4. Where this spec adjusts a decision to fit the code, it says so in §9. Every `file:line` was read at ddae78d.

## 1. Intent

**The owner said (2026-10-13):**
- "F3b is input fixing (edit path, input rename), output rename and auto PGPatcher rename."
- Input rename is an alias in `sdc.toml`; source files are never touched (decision card).

**Success:**
- `wall.png` is a normal map saved without `_n`. Right-click it, *Rename input…*, type `wall_n`: the preview shows kind *colour → normal* and the output `wall_n.dds`. Apply writes `[inputs."textures/armor/wall.png"] as = "textures/armor/wall_n.png"`, the file joins the `wall` set as its normal, and `wall.png` on disk is unchanged. One Ctrl+Z takes it back.
- `sdc convert` on the same folder reads the alias too and writes the same `wall_n.dds`.
- Select twelve sets, *Rename outputs…*, find `old_` replace with `new_`: twelve rows preview, two that would collide with an existing set are red and Apply is disabled until they are excluded. Apply is one undo step and one plan.
- On a CS PBR set, *Rename output…* to `steel` writes the set's PGPatcher `rename = "steel"`; the outputs and the JSON land on `textures\pbr\<folder>\steel…`.
- No alias and no output name set: plans, outputs and goldens are byte-for-byte what ddae78d makes.

**Out of scope (D7):** renaming or moving files on disk; output folder edits; counters, tokens, trim and Title case; saved recipes; a CLI command to write aliases; aliases in presets or user configs.

## 2. Engine: input aliases (D1)

### 2.1 Config

- `ConfigFile` (crates/engine/src/presets.rs:136-158) gains `inputs: BTreeMap<String, InputAlias>`, `#[serde(default, skip_serializing_if = "BTreeMap::is_empty")]`, with `InputAlias { #[serde(rename = "as")] as_: String }` and `deny_unknown_fields`. Only a project config may hold it, the same check as `files`.
- Schema: `schema/config.schema.json` is regenerated; `schema_version` is unchanged (an optional table, old files load, `migrate` needs no step).
- Keys and values are relative to the folder holding `sdc.toml`, `/` or `\` separated (normalised to `/` when read).

### 2.2 Discovery

- New `discover::apply_read_as(files: &mut [InputFile], root: &Path, output_root: &Path, aliases: &BTreeMap<String, InputAlias>) -> Vec<Warning>`, called in `plan` right after `discover_reporting` and before `canonicalise_outputs` (crates/engine/src/api.rs:782-786). The planning-sequence comment (api.rs:807) gets a step 1a.
- `InputFile` (discover.rs:21-32) gains `real_rel: Option<PathBuf>`: `Some(original rel)` when an alias applied, else `None`. Serialised only when `Some`, so event and manifest JSON for unaliased files is unchanged.
- For each file whose `overrides::project_rel` matches an alias key (`path_key_matches`, so with or without extension, case-insensitive):
  - The alias path, made relative to the file's discovery folder (the folder `rel` is relative to), becomes `rel`, with the real extension (D1).
  - `game_path` is recomputed from `<root>/<alias>` (`discover::game_path`), and `output = output_root.join(rel).with_extension("dds")`.
  - `real_rel` keeps the old `rel`.
- `overrides::project_rel` (overrides.rs:57-86) uses `real_rel`'s file name when it is `Some`. That one change keeps `[files]` keys, `SetEdit` file keys and `OutputKey::File` keys on the real file (D1 identity). The folder part already comes from the real path.
- Rule globs (`policy::glob_match_input`) keep reading `rel` and `game_path`, so they see the alias: a rule for `*_n.png` matches an aliased `wall.png`. This is deliberate (the alias says what the file is) and is written in DECISIONS.

### 2.3 Codes

- `IN-ALIAS-INVALID` (Warning, batch-level), with the key and the reason: absolute or `..`; a different extension; outside the folder the file was found in; equal to another file's real or aliased path (case-insensitive); an empty name. The alias is not applied and the file is planned under its real name. (The GUI dialog never writes an invalid alias, so this only meets hand edits; see §9.)
- `IN-ALIAS-STALE` (Info, batch-level, like `SET-EDIT-STALE`): an `[inputs]` key that matched no file.
- Catalogue count +2. The codes doc and `schema/event.schema.json` follow the existing code-adding steps.

### 2.4 Version

- No `ENGINE_VERSION` bump (the rule in the F3a plan's Global Constraints): unaliased outputs are unchanged. An aliased file's output path is new, so it is converted as a new output anyway.

## 3. GUI: output rename (D2) and auto PGPatcher rename (D3)

### 3.1 Storage

- `SetEntry` (crates/gui/src/sets_doc.rs:28-41) gains `output_name: Option<String>`, a new reserved key `output_name` (sets_doc.rs:16-25), and `SetsDoc::set_output_name(id, Option<&str>)`. It is not a JSON setting, so `pgp::writer` never sees it.
- A CS PBR set never stores `output_name`. Its output rename writes the existing JSON key `rename` through `set_json_value` (crates/gui/src/model/sets.rs:1376), which already re-plans (`replans`, sets.rs:773).

### 3.2 Output paths

- `engine_inputs` (sets.rs:571-638) today returns before computing paths on non-CS targets (sets.rs:599-601). New order:
  1. For every set with an `output_name`, on every target: each member's mirrored output with the base name replaced. The rel is `<output dir of the member>/<output_name><suffix>.dds`, where the suffix is the member's canonical suffix (`output_suffix`, the same helper `output_candidates` uses). A packed output is keyed `OutputKey::Packed{set, suffix}`.
  2. Then, on CS only, today's PBR candidates and ownership rule, unchanged.
- A PBR set's own prefix wins over an `output_name` (it never has one, §3.1); a stale `output_name` left on a set that later becomes PBR is ignored and the set editor shows "Output name is ignored on a PBR set; use Rename" with a Clear button.

### 3.3 Mesh reference warning

- `model::meshes` (crates/gui/src/model/meshes.rs) already reads the scan database. New `AppModel::mesh_refs_to(game_path) -> usize`, a count of shapes and plugin records referencing that path. When the scan is not loaded it is 0 and no warning shows.
- The rename preview warns on an Outputs row whose old game path has refs (D2).

## 4. The rename plan (pure, D4)

New module `crates/gui/src/model/rename.rs`.

```rust
pub enum RenameMode { Inputs, Outputs }
pub struct Recipe { find: String, replace: String, regex: bool, prefix: String, suffix: String, case: Case }
pub enum Case { Keep, Lower, Upper }
pub struct RenameRow {
    pub key: String,            // item key (Inputs) or set id (Outputs)
    pub old: String, pub new: String,
    pub old_kind: Option<String>, pub new_kind: Option<String>, // Inputs only
    pub output: String,         // the output path after Apply
    pub included: bool,
    pub typed: Option<String>,  // a per-row override typed in the table (Inputs only)
    pub problem: Option<RenameProblem>, // blocks Apply
    pub warning: Option<String>,
    pub skipped: Option<String>, // "skipped (locked)"
}
pub fn plan_rename(m: &AppModel, mode: RenameMode, keys: &[String], recipe: &Recipe,
                   exclude: &BTreeSet<String>, typed: &BTreeMap<String, String>) -> Result<Vec<RenameRow>, RecipeError>;
```

- The base name is the part before the suffix sdc detected (`engine::group::split_suffix_with` with the registry's alias table). The operations apply to it in the D4 order, and the suffix and extension are re-attached. A typed override replaces the whole relative path (Inputs) and skips the recipe for that row.
- `RecipeError::Pattern(msg)` for a regex that does not compile; the dialog shows it in place.
- `new_kind` re-runs the engine's name-only kind rule (`suggest::suggest` on a stub `FileFacts`, or a smaller `engine::group::kind_of_name` if `suggest` needs analysis; the plan picks the one that needs no image).
- Checks (D4): collisions inside the batch and against every unselected file or set (case-insensitive, on both the input path and the output path), Windows-illegal names, 260-character output paths, empty names. The Windows-name check is a new `pgp::prefix::check_file_name` beside `check_rel_path` (prefix.rs:47), which it extends.
- The `regex` crate is added to `crates/gui/Cargo.toml`. It is the only new crate.

## 5. The dialog (D4)

- `Dialog::Rename { mode, keys, recipe, exclude, typed }` in `model::Dialog` (crates/gui/src/model/mod.rs:81). Drawn in `ui/dialogs.rs` as a resizable window titled "Rename N inputs" or "Rename outputs of N sets".
- Top: the mode toggle (switching keeps the recipe), then Find, Replace, Regex, Prefix, Suffix, Case, and "N of M matched" while Find is not empty.
- Table: Include | Old | New (editable in Inputs mode) | Kind | Output | Notes. Red rows block, amber rows warn, grey rows are excluded or skipped. Rows are in the displayed order of the selection.
- Buttons: Apply (Enter; disabled while any included row has a problem or the recipe has an error), Cancel (Esc). The recipe is kept in `AppModel::last_recipe` for the next open in this session.

## 6. Menu entries (D5)

- `MenuAction` (crates/gui/src/model/bulk.rs:813-844) gains `RenameInputs`, `RenameOutputs` and `ClearInputAlias`.
- `item_menu` (bulk.rs:968) appends "Rename input…" / "Rename inputs…" (F2), "Rename output…", and "Clear input alias" when any selected texture has one.
- `set_menu` (bulk.rs:1012) appends "Rename outputs…" (F2) and "Rename inputs…" (every member of the selected sets, packed outputs left out since they have no file).
- `run_menu` (bulk.rs:1122) opens `Dialog::Rename` or runs the clear. F2 opens Outputs mode for sets and Inputs mode for textures, under the same focus rules as Ctrl+A (F3a spec §2.2).
- Packed items (no input file) are not offered Inputs mode; in a mixed selection they are listed as skipped.

## 7. Apply, undo and re-plan (D6)

- `AppModel::apply_rename(rows)` runs through `batch_edit` (crates/gui/src/model/mod.rs:893): one snapshot of both documents, one `Command`, one `replan()`.
- Inputs mode writes `[inputs."<real rel>"] as = "<new rel>"` with `toml_edit` (the existing `sdc.toml` document code), and removes the entry when the new path equals the real one. *Clear input alias* removes the entries.
- Outputs mode writes `output_name` (non-PBR) or `rename` (CS PBR) per set, and removes it when the new base equals the mirrored one.
- Read-only documents: the existing notice, and nothing is written (D6).
- The texture tile and the settings panel header show "read as `wall_n.png`" under the file name of an aliased texture, with a Clear button.

## 8. Docs

- DECISIONS gets **Note AA — M6 Track F3b**: the rulings in §2-§7 and §9, the alias identity rule, rule globs seeing the alias, no version bump, and the owner check in §10.
- `docs/gui.md` gets a "Renaming" section: input aliases (what they do and don't do to disk), output rename, the PBR case.
- The CLI help for `sdc.toml` and the config docs describe `[inputs]`.

## 9. Adjustments to the decisions

- D1 said "matched like a `[files]` key". The key match is `path_key_matches` on `project_rel`, so it is exactly the `[files]` rule.
- D2's "explicit `output_paths` on every target" needed the early return in `engine_inputs` moved (§3.2). PBR candidates still run on CS only.
- D4's "new kind" column depends on a name-only kind rule existing in the engine; if it does not, the plan adds one in `group.rs` rather than calling the analysing path.

- D1 made an invalid alias a per-file Blocking code. The engine has no per-file block short of skipping the file, so it is a batch Warning and the alias is ignored (§2.3).

## 10. Owner checks (manual, after the code lands)

- One PBR set renamed through F3b in real PGPatcher: does the patched NIF get `\textures\pbr\…` from the leading `\` PGPatcher adds to `rename`, and does its file-existence check accept it? (Open since V3; F3b makes more sets use `rename`.)
- One aliased normal map in a real mod: the converted `_n` shows correctly in game.
