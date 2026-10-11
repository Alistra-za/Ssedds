# sdc M3b: set editor and PGPatcher JSON, implementation plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** `sdc-gui` lets a user build texture sets by hand, edit each set's TruePBR material settings, and export the DDS files together with one PGPatcher JSON that patches the meshes as expected.

**Architecture:**
- **Engine:** `BatchRequest` gains two additive fields. The CLI never sets them.
  - `set_edits` changes the automatic grouping. It is applied right after grouping, so packing follows the user's sets.
  - `output_paths` sets an explicit output path per source file or per packed recipe. It is applied after the set keys are fixed and before outputs are compared for duplicates.
- **GUI:**
  - keeps the user's sets and JSON settings in `sdc-project.toml`, next to `sdc.toml`;
  - turns them into `set_edits` and `output_paths` on every plan;
  - owns a pure-Rust PGPatcher model (`pgp/`): key table, write rules, mutual exclusion, prefixes, writer and validator;
  - records every edit in one snapshot-based undo history.

**Tech stack:** Rust 2021 workspace. eframe/egui 0.36.2 and egui_kittest 0.36.2; toml_edit 0.25; serde_json; `sha2`, `hex` and (dev) `walkdir`: workspace dependencies already in `Cargo.lock`, added to the GUI crate with `.workspace = true`. **No new downloads:** JSON validation is an in-tree validator (Task 9).

**Spec:** `docs/superpowers/specs/2026-10-10-sdc-m3b-set-editor-design.md` (owner-approved 2026-10-10). Also consumes the "Carried from M3a" list of `docs/superpowers/plans/2026-10-08-skyrim-dds-converter-phase0-2.md` (its "fix first" item is Task 1).

## Global constraints

- Branch `feat/m3b-set-editor` from `main` (6d3be3c or later). Never commit to `main`. Never push.
- **Downloads:** none are needed. If a step seems to need a crate that isn't already in `Cargo.lock`, stop and report `NEEDS_CONTEXT`.
- `cargo fmt --all`, `cargo clippy --workspace --all-targets -- -D warnings` and `cargo test --workspace` must all pass before each commit. Git Bash needs `export PATH="$HOME/.cargo/bin:$PATH"`.
- Never run `python -`/`python3 -` heredoc probes. They hang on this machine.
- Commit only the files you changed, by explicit path (no `git add -A`). End each commit message with `Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>`.
- The engine changes are additive. With `set_edits` empty and `output_paths` empty, every existing test, including `crates/engine/tests/golden.rs` and `crates/cli/tests/gui_repro.rs`, must pass unchanged. Do not edit those tests to make them pass.
- A change to a type that appears in `schema/*.json` needs the schemas regenerated: `cargo run -p sdc -- presets schema all --out schema`. Commit the regenerated files.
- The GUI model (`crates/gui/src/model/**`, `crates/gui/src/pgp/**`, `crates/gui/src/sets_doc.rs`) has no egui types.
- `crates/gui/src/ui/**` only draws and maps input to model calls.
- Licence: sdc is GPL-3.0-or-later. PGPatcher's `pbr.schema.json` is GPL-3.0 (verified 2026-10-10 from `hakasapl/PGPatcher` `LICENSE`). Vendoring it is compatible, and needs a THIRD_PARTY_NOTICES line (Task 6).
- TruePBR-Manager (CC BY-NC) and Skyking (reference only) are feature ideas only: no code, assets or tables are copied from them.
- User-facing text follows the existing GUI tone: plain words, no jargon beyond the field names.

## Rulings made while planning (they refine the approved spec; record each one in DECISIONS Note P in Task 16)

| # | Ruling | Why | Cost if wrong |
|---|---|---|---|
| P1 | The engine takes **`output_paths: BTreeMap<OutputKey, PathBuf>`** (explicit output paths) instead of the spec's per-set `SetOutput { prefix }` | The approved ownership rule (§3.3) places a shared output at the *owner's* prefix, which may not be the set that holds its sources. A per-set prefix cannot say that; explicit paths can, and the engine stays free of PGPatcher concepts | One engine field shape; the GUI computes every path |
| P2 | The engine `SetEdit` has `Create`, `Rename`, `Move` and `Remove`. **Delete and Order are GUI-only.** Deleting a created set drops its edits, so its files return to automatic grouping. Order lives only in `sdc-project.toml` | The engine needs neither: order only decides JSON order and ownership, both computed in the GUI | None |
| P3 | Delete is offered only for sets the user created. Deleting an untouched automatic set would be a no-op, so it is disabled with the reason | The spec allows deleting any set, but an automatic set would just regroup | One disabled menu item |
| P4 | A borrowed slot on vanilla/CPM becomes a **GUI file copy** of the owner's exported DDS after the run. It is listed in the drawer, and is not tracked by the manifest or `if-changed` | The engine maps one input to one output | If manifest tracking is wanted later, this moves into the engine (M6) |
| P5 | A set is a **PBR set** when its diffuse member's resolved sub-type is `pbr_albedo` (`job.profile.subtype`), not the `set_resolved.pbr` flag | The flag is computed from the path and suffixes, which user-built sets outside `textures/pbr/` break | None |
| P6 | **Slot numbers are PGPatcher's 1-based `slotN`:** 1 diffuse, 2 normal, 3 emissive `_g`, 4 height `_p`, 5 cubemap, 6 RMAOS, 7 coat normal `_cnr` / fuzz `_f`, 8 subsurface `_s` | The schema numbers slots this way. The spec's "slot 6 (TX06)" for `_cnr`/`_f` meant TX06, the 0-based name of `slot7` | None |
| P7 | JSON-only edits (material values, features, defaults, `json_name`) **do not re-plan**. Set edits, borrow changes, match and `rename` changes, `[files]` edits and Mode do | Match and `rename` change output paths; material values don't | Over-planning was already an M3a carry |
| P8 | A `sdc-project.toml` that fails to parse makes **set and JSON editing read-only**, with a banner. `[files]` edits in `sdc.toml` keep working | The two files are independent | None |
| P9 | Undo is **snapshot-based**: a command stores the text of both documents before and after. Duplicate-source commands also store the copied file and its SHA-256 | Small TOML files; one mechanism for every edit kind | Memory grows with very long sessions (bounded at 200 steps) |
| P10 | Colours are stored in `sdc-project.toml` as 0–255 integers, and converted to 0–1 decimals (4 places) only by the writer | The UI edits 0–255 (owner) | None |
| P11 | The written slot-command path for a borrowed output is `textures\pbr\…\<file>.dds` (with `textures\`, which is valid and unambiguous) | The schema adds `textures\` when it is missing, so both forms work | None |
| P12 | JSON validation uses an in-tree validator (`pgp/validate.rs`) for the draft-07 subset the vendored schema uses: `type`, `properties`, `additionalProperties`, `required`, `items`, `minItems`, `maxItems`, `$ref`, `allOf`, `anyOf`, `oneOf`, `not`, `if`/`then`/`else` | No download. Validation is a safety net that should never fire. A test asserts the vendored schema uses no other keyword | A future schema keyword makes that test fail, which is the signal to extend the validator |
| P13 | Invalid path characters (`<>:"|?*`, control characters), `..`, a drive letter or a leading `\` **block export**. Paths are not silently "sanitised" | A rewritten path is no longer the one PGPatcher looks for | None |
| P15 | "Not in a set" is a clickable count in the Textures header, not an automatically switched filter | Switching the filter on automatically would hide textures on open and change M3a's behaviour | One click |
| P14 | `gui_repro` (GUI export == `sdc convert`) keeps its guarantee for **non-PBR sets only**. A PBR set on CS is written under `textures\pbr\` by the GUI alone, because the JSON is GUI-only (owner). The test decides rock01 as `Diffuse`, so it stays unchanged | Owner: "PG Patcher is GUI only" | None |

## Review focus

Most likely first. Each item's test is in the task named.

1. **A file renamed or deleted on disk after the user put it in a set.** The `[[sets]]` entry stays. The set shows red "missing (`<file>`)", the edit is reported as `SET-EDIT-STALE`, and nothing is silently dropped (Tasks 2 and 10).
2. **Two sets whose matches derive the same prefix**, e.g. `landscape\rock01` from two source folders. Export is blocked, naming both sets and the path, before the engine sees the collision. The GUI sends only the first path, so the plan is stable and the sets don't flicker. The engine-level `IN-DUPLICATE` test in Task 3 stays as the guard (Tasks 3, 10 and 11).
3. **A `rename` or slot path containing `..`, a drive letter or a leading `\`.** Export is blocked with the field named, and no file is written outside the export folder (Tasks 7 and 11).
4. **Undo after `sdc-project.toml` was edited outside the GUI.** Undo restores the in-memory document; the next Save raises the existing "changed on disk" dialog naming `sdc-project.toml`, and doesn't overwrite silently (Tasks 4 and 5).
5. **Cancelling an export midway.** No JSON is written or replaced, and no vanilla copies are made (Task 12).

## Interface registry (names every task must use)

**Engine**, `crates/engine/src/sets.rs` (new), re-exported from `engine::sets`:
- `enum SetEdit { Create { set: String }, Rename { from: String, to: String }, Move { file: String, to: String }, Remove { file: String } }`
  - serde: `#[serde(tag = "op", rename_all = "snake_case")]`.
  - Set fields are **set ids**; `file` fields are `[files]` keys.
- `enum OutputKey { File(String), Packed { set: String, suffix: String } }`
  - Derives `Ord`. `suffix` is the recipe suffix (`rmaos`, `rfaos`, `p`, `m`).
- `struct StaleEdit { edit: SetEdit, reason: String }`
- `fn set_id(output_root: &Path, key: &str) -> String`, and its inverse `fn set_key(output_root: &Path, id: &str) -> String`.
- `fn apply_set_edits(sets: &mut Vec<TextureSet>, files: &[InputFile], root: Option<&Path>, output_root: &Path, aliases: &BTreeMap<String, String>, edits: &[SetEdit]) -> Vec<StaleEdit>`
- `fn apply_output_paths(files: &mut [InputFile], root: Option<&Path>, output_root: &Path, paths: &BTreeMap<OutputKey, PathBuf>)`
- `fn packed_outputs(output_root: &Path, paths: &BTreeMap<OutputKey, PathBuf>) -> BTreeMap<(String, String), PathBuf>`
  - Keyed by (set **key**, suffix); values are absolute paths.
- `group::duplicate_outputs(files: &[InputFile]) -> Vec<Vec<usize>>`, extracted from `group_sets_with`.
- `BatchRequest { …, set_edits: Vec<SetEdit>, output_paths: BTreeMap<OutputKey, PathBuf> }`
  - Both default to empty in `BatchRequest::new`. `output_paths` values are relative to `output_root`.
- `Code::SetEditStale = "SET-EDIT-STALE" => Info`.
  - Warning data: `{ "edit": <SetEdit as JSON>, "reason": "…" }`.
  - Emitted with no job and no set.
- `pack::plan::plan_packing(…, packed_outputs: &BTreeMap<(String, String), PathBuf>)`: a new last parameter.

**GUI PGPatcher model**, `crates/gui/src/pgp/`:
- `schema.rs` (Task 6):
  - `SCHEMA_JSON`, `SCHEMA_URL`;
  - `KeyKind { AlwaysBool, OnlyTrue, Tri, Number, Text, Rgb, Rgba, Slot, Object }`;
  - `Group { Match, Material, Features, Emissive, Parallax, Subsurface, Multilayer, Glint, Fuzz, Vertex, Locks, Uv, Slots }`;
  - `KeySpec { key, kind, group, advanced, label }`;
  - `KEYS` (schema order, then `glint.*`, `fuzz.*`), `REMOVED`, `NOT_EXPOSED`, `spec(key)`.
- `settings.rs` (Tasks 4 and 6):
  - `type Settings = BTreeMap<String, Value>`: dotted schema paths; colours are 0–255 arrays;
  - `Feature { Emissive, Parallax, Subsurface, Hair, Multilayer, Glint, Fuzz }` (derives `Ord`);
  - `FEATURE_KEYS`, `feature_key(f)`, `feature_on(s, f)`, `blocked_by(s, f) -> Option<Feature>`, `effective(defaults, own)`, `GLINT_DEFAULTS`.
- `prefix.rs` (Task 7):
  - `MatchMode { Diffuse, Normal, PathContains, Slot(u8) }`, `match_mode(s)`;
  - `slot_suffixes(n)`;
  - `derive_match_diffuse(rel_dir, stem)`;
  - `prefix_of(s, derived) -> Result<String, PrefixError>`, which returns a backslash path such as `textures\pbr\landscape\rock01`;
  - `pitfalls(s)`, `check_rel_path(p, field)`, `output_rel(prefix, suffix) -> PathBuf`, `PrefixError(pub String)`.
- `writer.rs` (Task 8):
  - `EntryInput { settings, slots: BTreeMap<u8, String>, derived_match: String }` (derives `Default`);
  - `Node { Scalar, Obj, Arr }` with `to_value()`;
  - `entry_node(defaults, e)`, `document(defaults, entries) -> Node`, `to_text(&Node) -> String`.
- `validate.rs` (Task 9): `validate(&Value) -> Result<(), Vec<String>>`, `KEYWORDS`.

**GUI project file**, `crates/gui/src/sets_doc.rs` (Task 4):
- `FILE_NAME = "sdc-project.toml"`.
- `SetsDoc`, with:
  - `open`, `reload`, `path`, `is_dirty`, `read_only`, `error`, `changed_on_disk`, `save(force) -> Result<(), project::SaveError>`;
  - `text`, `restore`;
  - `json_name`, `set_json_name`, `last_json_sha256`, `set_last_json_sha256`;
  - `order`, `set_order`;
  - `defaults`, `set_default`;
  - `entries`, `create_set`, `delete_set`, `rename_set`, `add_member`, `remove_member`, `set_borrow`, `set_value`.
- `SetEntry { id, created, renamed_from, add, remove, borrow: BTreeMap<u8, String>, settings }`.
- `project.rs` (Task 4):
  - `ProjectDoc::{text, restore}`;
  - `SaveError` text no longer names a file (the model prefixes it);
  - `pub(crate)` helpers `json_to_toml`, `toml_to_json`, `dotted_table`, `sub_table`, `set_value`, `find_key`.
- Model (Task 4):
  - `AppModel.sets_doc: Option<SetsDoc>`;
  - `DocKind { SdcToml, Project }` with `file()`;
  - `Dialog::ExternalChange(DocKind)`, `resolve_external_change(which, overwrite)`.

**GUI history**, `crates/gui/src/model/history.rs` (Task 5):
- `Snapshot { sdc_toml, project }`;
- `CreatedFile { path, source, sha256 }`;
- `Command { label, before, after, merge: Option<String>, created_file: Option<CreatedFile> }`;
- `History { undo, redo }`, with `LIMIT = 200`, `push`, `undo_step`, `redo_step`, `refuse_undo`, `refuse_redo`, `clear`, `can_undo`, `can_redo`;
- `sha256_of(&Path) -> Option<String>`.
- `AppModel.history`, and the methods `edit_docs(label, merge, replan, f: FnOnce(&mut ProjectDoc, &mut SetsDoc))`, `undo`, `redo`, `can_undo`, `can_redo`. Every edit goes through `edit_docs`.

**GUI sets model**, `crates/gui/src/model/sets.rs` (Task 10):
- `SlotKind { Diffuse, Normal, Emissive, Height, Rmaos, Coat, Fuzz, Subsurface }`, with `ALL`, `slot_number()` (P6), `suffix()` and `label()`.
- `SlotFill { Empty, Own { item }, Borrowed { from, item } }`.
- `SetView { key, id, label, members, slots, pbr, created, missing, owns, derived_match, prefix, prefix_error, settings }`.
- Functions: `set_views(&AppModel)` (display order), `slot_of(&TextureItem)`, `slot_conflict(&[SlotKind], SlotKind)`, `engine_inputs(&AppModel) -> (Vec<SetEdit>, BTreeMap<OutputKey, PathBuf>)` (drops a second output on one path), `output_candidates(&AppModel, &[SetView]) -> Vec<(String, OutputKey, PathBuf)>` (keeps them, for `path_collisions`).
- Enums:
  - `DropChoice { Move, Share, Duplicate }`;
  - `DropOutcome { Joined, AskReplace { existing }, AskChoice { from }, Refused(String) }`;
  - `SlotCommand { FromSet, Path(String), Clear }`.
- `AppModel` methods:
  - `drop_on_set` and `drop_choice` (both return `DropOutcome`);
  - `drop_replace`;
  - `drop_new_set` (returns the new id);
  - `reorder_set`;
  - `rename_set` and `set_slot_command` (both return `Result<(), String>`);
  - `delete_set` (returns `bool`);
  - `remove_member`;
  - `set_json_value(Option<&str>, key, Option<Value>)`.
- `AppModel` fields: `last_inputs`, `auto_replans`.

**GUI export**, `crates/gui/src/model/pbr_export.rs` (Tasks 11–12):
- `ExportIssue { set_id, set_key, text }`;
- `export_blocks`, `export_asks`, `json_entries`, `json_name`;
- `PendingJson { path, entries, defaults, copies }`, `prepare`;
- `Finish { Written(String), Overwrite(String), Nothing }`, `finish`, `overwrite`.
- `Dialog::{ExportBlocked, ExportWarnings, OverwriteJson}`.
- `AppModel` methods: `proceed_with_warnings`, `resolve_overwrite_json`.
- `AppModel` fields: `pending_json`, `pending_text`.
- `ExportModel.notes`.

**GUI UI** (Tasks 13–15):
- `ui::sets_column::{draw, DragItem, DragSet, DropState, Pending}`;
- `ui::set_editor::draw`;
- `ui::slot_commands::{draw, SlotCmdState}`;
- `ui::pgp_fields::{panel, advanced}`;
- `Selection::Defaults`;
- the `AppModel` methods `set_format`, `set_opaque`, `is_opaque`, `set_json_name`.

---
## File structure

| File | Status | Responsibility |
|---|---|---|
| `crates/gui/src/model/mod.rs` | modify | Task 1 fix; `request()` fills `set_edits`/`output_paths`; `edit_docs`; sets doc lifecycle |
| `crates/engine/src/sets.rs` | create | `SetEdit`, `OutputKey`, `apply_set_edits`, `apply_output_paths`, `packed_outputs`, `set_id`/`set_key` |
| `crates/engine/src/group.rs` | modify | Extract `duplicate_outputs` |
| `crates/engine/src/api.rs` | modify | `BatchRequest` fields; the planning sequence in `plan_cached` |
| `crates/engine/src/pack/plan.rs` | modify | `plan_packing` takes `packed_outputs` |
| `crates/engine/src/codes.rs` | modify | `SET-EDIT-STALE` |
| `crates/engine/tests/set_edits.rs` | create | Engine tests for Tasks 2–3 |
| `schema/*.json` | regenerate | New code |
| `crates/gui/src/sets_doc.rs` | create | `sdc-project.toml` |
| `crates/gui/src/project.rs` | modify | `text()`/`restore()`; file-named `SaveError` |
| `crates/gui/src/model/history.rs` | create | Undo/redo |
| `crates/gui/src/model/sets.rs` | create | Set views, ownership, drop rules, engine inputs |
| `crates/gui/src/pgp/{mod,schema,settings,prefix,writer,validate}.rs` | create | PGPatcher model |
| `crates/gui/assets/pbr.schema.json` | create | Vendored schema (copy of `F:/ai/dds/gui reference/ref/pbr.schema.json`) |
| `crates/gui/src/model/pbr_export.rs` | create | Export checks, JSON write, vanilla copies |
| `crates/gui/src/ui/sets_column.rs` | create | Editable Sets column (replaces `columns::sets`) |
| `crates/gui/src/ui/set_editor.rs` | create | Layout A (replaces `set_view.rs`, which is deleted) |
| `crates/gui/src/ui/pgp_fields.rs` | create | JSON field widgets (bool, tri-state, number, colour, text) |
| `crates/gui/src/ui/slot_commands.rs` | create | Slot-commands dialog |
| `crates/gui/src/ui/{mod,header,dialogs,advanced,columns}.rs` | modify | Wiring |
| `crates/gui/tests/{sets_doc,pgp,sets_model,history,pbr_export,set_editor_ui,m3b_flow}.rs` | create | Tests |
| `THIRD_PARTY_NOTICES.md`, `docs/gui.md`, `docs/DECISIONS.md`, phase plan | modify | Task 16 |

---

### Task 1: Fix the stale `after_export` (M3a carry, spec §8)

**Files:**
- Modify: `crates/gui/src/model/mod.rs` (`open_folder`, `resolve_export_running`)
- Test: `crates/gui/tests/edges.rs` (append)

**Interfaces:**
- Consumes: `AppModel::{request_close, request_open, resolve_export_running, open_folder, sender}`, `WorkerMsg::ExportDone`, `AfterExport`.
- Produces: no API change. `after_export` is cleared by "Keep running" and by `open_folder`; a new answered request replaces the pending one.

**The defect:**
1. The user asks to close while an export runs and chooses "Cancel export and close", so `after_export = Close`.
2. While the export winds down, they ask to open a folder and choose **Keep running**. `after_export` stays `Close`.
3. When the export ends, the window closes, although the last answer said to keep going.

- [ ] **Step 1: Write the failing tests.** Append to `crates/gui/tests/edges.rs`. The file already has `mod support; use support::*;`; reuse it. Add these imports if missing: `use sdc_gui::model::{AfterExport, Dialog}; use sdc_gui::worker::WorkerMsg;`.

```rust
/// Pretend an export is running: a token the model treats as the running export (no engine work starts).
fn fake_running(m: &mut sdc_gui::model::AppModel) {
    m.export.running = Some(engine::api::CancelToken::default());
}

fn finish_export(m: &mut sdc_gui::model::AppModel) {
    m.sender()
        .send(WorkerMsg::ExportDone(Ok(engine::api::BatchSummary::default())))
        .unwrap();
    m.poll();
}

#[test]
fn keep_running_after_a_cancelled_close_clears_the_pending_close() {
    let d = tempfile::tempdir().unwrap();
    let mut m = opened(d.path());
    fake_running(&mut m);
    assert!(!m.request_close());
    assert_eq!(m.dialog, Some(Dialog::ExportRunning(AfterExport::Close)));
    m.resolve_export_running(AfterExport::Close, true); // cancel and close
    // The export is still winding down; now the user asks to open another folder and keeps the export running.
    let other = d.path().join("other");
    std::fs::create_dir_all(&other).unwrap();
    assert!(!m.request_open(other.clone()));
    m.resolve_export_running(AfterExport::Open(other), false); // keep running
    finish_export(&mut m);
    assert!(!m.take_close_ready(), "the stale close must not fire");
    assert_ne!(m.root.as_deref(), Some(d.path().join("other").as_path()));
}

#[test]
fn a_second_answered_request_replaces_the_first() {
    let d = tempfile::tempdir().unwrap();
    let mut m = opened(d.path());
    fake_running(&mut m);
    m.request_close();
    m.resolve_export_running(AfterExport::Close, true);
    let other = d.path().join("other2");
    std::fs::create_dir_all(&other).unwrap();
    m.request_open(other.clone());
    m.resolve_export_running(AfterExport::Open(other.clone()), true);
    finish_export(&mut m);
    // Exactly the latest action runs: the folder opens, the window does not close.
    assert!(!m.take_close_ready());
    assert_eq!(m.root.as_deref(), Some(other.as_path()));
    // A later export end triggers nothing.
    fake_running(&mut m);
    finish_export(&mut m);
    assert!(!m.take_close_ready());
    assert_eq!(m.root.as_deref(), Some(other.as_path()));
}

#[test]
fn opening_a_folder_clears_a_pending_action() {
    let d = tempfile::tempdir().unwrap();
    let mut m = opened(d.path());
    fake_running(&mut m);
    m.request_close();
    m.resolve_export_running(AfterExport::Close, true);
    // The export ends without a message (e.g. the worker's channel closed), and the user opens a folder directly.
    m.export.running = None;
    m.open_folder(mod_folder(&d.path().join("again")));
    fake_running(&mut m);
    finish_export(&mut m);
    assert!(!m.take_close_ready());
}
```

`export.running` and `root` are `pub` already (M3a). `take_close_ready`, `request_open` and `resolve_export_running` are `pub`.

- [ ] **Step 2: Run the tests and confirm they fail.**

Run: `cargo test -p sdc-gui --test edges keep_running a_second_answered opening_a_folder_clears`
Expected:
- `keep_running_after_a_cancelled_close_clears_the_pending_close` FAILS (`take_close_ready()` is true);
- `opening_a_folder_clears_a_pending_action` FAILS;
- `a_second_answered_request_replaces_the_first` passes already (`after_export = Some(action)` replaces). Keep it as the regression guard.

- [ ] **Step 3: Fix `crates/gui/src/model/mod.rs`.** In `resolve_export_running`, clear the pending action on "Keep running":

```rust
    pub fn resolve_export_running(&mut self, action: AfterExport, cancel_and_continue: bool) {
        self.dialog = None;
        if !cancel_and_continue {
            // "Keep running" withdraws any earlier "cancel and …": the user now wants the export to finish and
            // nothing to happen after it (spec §8).
            self.after_export = None;
            return;
        }
        // A newer answer replaces an older one; it never stacks.
        self.after_export = Some(action);
        if self.export.running.is_some() {
            self.cancel_export();
        } else {
            // It ended while the dialog was up.
            self.run_after_export();
        }
    }
```

In `open_folder`, as its first statement:

```rust
        // Whatever was pending for the previous folder no longer applies (spec §8).
        self.after_export = None;
```

- [ ] **Step 4: Run the tests and confirm they pass.**

Run: `cargo test -p sdc-gui --test edges`
Expected: all pass.

- [ ] **Step 5: Run the gates and commit.**

```bash
cargo fmt --all && cargo clippy --workspace --all-targets -- -D warnings && cargo test -p sdc-gui
git add crates/gui/src/model/mod.rs crates/gui/tests/edges.rs
git commit -m "fix(gui): Keep running and opening a folder clear a pending after-export action (M3b §8)

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

---

### Task 2: Engine `set_edits` (spec §2.1)

**Files:**
- Create: `crates/engine/src/sets.rs`
- Modify:
  - `crates/engine/src/lib.rs` (`pub mod sets;`);
  - `crates/engine/src/group.rs` (extract `duplicate_outputs`);
  - `crates/engine/src/api.rs` (field, `new`, sequence);
  - `crates/engine/src/codes.rs` (code and module doc);
  - `crates/cli/src/human.rs` only if its exhaustive match over `Code` needs a line (check with `cargo build`).
- Regenerate: `schema/*.json`
- Test: `crates/engine/tests/set_edits.rs`

**Interfaces:**
- Consumes:
  - `group::{TextureSet, split_suffix_with, group_sets_with}`;
  - `overrides::{project_rel, path_key_matches}`;
  - `api::project_root(&BatchRequest) -> Option<PathBuf>`.
- Produces (registry): `SetEdit`, `StaleEdit`, `set_id`, `set_key`, `apply_set_edits`, `group::duplicate_outputs`, `BatchRequest::set_edits`, `Code::SetEditStale`.

**The planning sequence after this task and Task 3.** It is load-bearing. Write it as a comment block in `plan_cached`:
1. `discover`, then `canonicalise_outputs` (unchanged).
2. `group_sets_with(&files, aliases)` gives the automatic sets. **Its `dups` are ignored from now on.**
3. `apply_set_edits`. **Set keys are fixed from here on**: output overrides never change a key.
4. `apply_output_paths` (Task 3).
5. `dups = group::duplicate_outputs(&files)`, so two sets writing one file are caught after the paths change.
6. Predicted packing suffixes, then the existing skip/work loop, then packing with `packed_outputs` (Task 3).

- [ ] **Step 1: Write the failing tests.** Create `crates/engine/tests/set_edits.rs`:

```rust
//! M3b Task 2–3: GUI set edits and output paths (spec §2.1). The CLI never sets them.
use engine::api::*;
use engine::codes::Code;
use engine::config::Target;
use engine::events::*;
use engine::sets::*;
use image::{ImageBuffer, Luma, Rgb};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tempfile::TempDir;

fn rgb(dir: &Path, rel: &str, c: [u8; 3]) {
    let p = dir.join(rel);
    std::fs::create_dir_all(p.parent().unwrap()).unwrap();
    ImageBuffer::from_fn(64, 64, |x, _| Rgb([c[0].wrapping_add(x as u8), c[1], c[2]]))
        .save(p)
        .unwrap();
}

fn gray(dir: &Path, rel: &str, v: u8) {
    let p = dir.join(rel);
    std::fs::create_dir_all(p.parent().unwrap()).unwrap();
    ImageBuffer::from_fn(64, 64, |x, _| Luma([v.wrapping_add(x as u8)]))
        .save(p)
        .unwrap();
}

/// `in/landscape/rock01{,_n}.png`, `in/landscape/rock02{,_n}.png`, and loose `in/stone/stone_roughness.png` +
/// `stone_ao.png` (they pack `stone/stone_rmaos` on their own).
fn fixture() -> TempDir {
    let d = tempfile::tempdir().unwrap();
    let r = d.path().join("in");
    rgb(&r, "landscape/rock01.png", [90, 60, 30]);
    rgb(&r, "landscape/rock01_n.png", [128, 128, 255]);
    rgb(&r, "landscape/rock02.png", [60, 90, 30]);
    rgb(&r, "landscape/rock02_n.png", [128, 128, 255]);
    gray(&r, "stone/stone_roughness.png", 120);
    gray(&r, "stone/stone_ao.png", 200);
    d
}

fn req(d: &TempDir) -> BatchRequest {
    let mut r = BatchRequest::new(vec![d.path().join("in")], d.path().join("out"), Target::Cs);
    r.recursive = true;
    r
}

fn plan_events(req: BatchRequest) -> (Plan, Vec<EngineEvent>) {
    let sink = Arc::new(VecSink::default());
    let rep = Reporter::new(sink.clone(), "t");
    let p = Engine::new()
        .unwrap()
        .plan(req, &rep, &CancelToken::default())
        .unwrap();
    (p, sink.take())
}

fn set_of(p: &Plan, tail: &str) -> String {
    p.jobs
        .iter()
        .find(|j| j.input.to_string_lossy().replace('\\', "/").ends_with(tail))
        .unwrap_or_else(|| panic!("no job for {tail}"))
        .set_key
        .clone()
}

fn packed_outputs_of(p: &Plan) -> Vec<String> {
    let mut v: Vec<String> = p
        .jobs
        .iter()
        .filter(|j| j.pack.is_some())
        .map(|j| j.output.to_string_lossy().replace('\\', "/"))
        .collect();
    v.sort();
    v
}

#[test]
fn ids_and_keys_round_trip() {
    let root = Path::new("C:/Out");
    let key = set_key(root, "Landscape/Rock01");
    assert_eq!(key, "c:/out/landscape/rock01");
    assert_eq!(set_id(root, &key), "landscape/rock01");
    // A key not under the root is its own id.
    assert_eq!(set_id(root, "d:/x/y"), "d:/x/y");
}

#[test]
fn no_edits_change_nothing() {
    let d = fixture();
    let (a, _) = plan_events(req(&d));
    let mut r = req(&d);
    r.set_edits = vec![];
    let (b, _) = plan_events(r);
    let outs = |p: &Plan| {
        let mut v: Vec<_> = p.jobs.iter().map(|j| (j.output.clone(), j.set_key.clone())).collect();
        v.sort();
        v
    };
    assert_eq!(outs(&a), outs(&b));
}

#[test]
fn moving_raw_maps_moves_the_packed_output_into_the_target_set() {
    let d = fixture();
    let (before, _) = plan_events(req(&d));
    assert!(packed_outputs_of(&before)
        .iter()
        .any(|o| o.ends_with("stone/stone_rmaos.dds")));
    let mut r = req(&d);
    r.set_edits = vec![
        SetEdit::Move { file: "stone/stone_roughness.png".into(), to: "landscape/rock02".into() },
        SetEdit::Move { file: "stone/stone_ao.png".into(), to: "landscape/rock02".into() },
    ];
    let (after, ev) = plan_events(r);
    let rock02 = set_of(&after, "landscape/rock02.png");
    let packed: Vec<_> = after.jobs.iter().filter(|j| j.pack.is_some()).collect();
    assert_eq!(packed.len(), 1);
    assert_eq!(packed[0].set_key, rock02, "the recipe now belongs to rock02's set");
    // The set_resolved event of rock02 lists the moved raw maps.
    let members = ev
        .iter()
        .find_map(|e| match &e.kind {
            EventKind::SetResolved { members, .. } if e.set.as_deref() == Some(rock02.as_str()) => {
                Some(members.clone())
            }
            _ => None,
        })
        .unwrap();
    assert!(members.iter().any(|m| m.ends_with("stone/stone_roughness.png")));
    assert!(ev.iter().all(|e| !matches!(&e.kind, EventKind::Warning(w) if w.code == Code::SetEditStale)));
}

#[test]
fn create_then_move_builds_a_new_set_and_remove_splits_a_member_off() {
    let d = fixture();
    let mut r = req(&d);
    r.set_edits = vec![
        SetEdit::Create { set: "mine/boulder".into() },
        SetEdit::Move { file: "landscape/rock01.png".into(), to: "mine/boulder".into() },
        SetEdit::Remove { file: "landscape/rock02_n.png".into() },
    ];
    let (p, _) = plan_events(r);
    let root = d.path().join("out");
    assert_eq!(set_id(&root, &set_of(&p, "landscape/rock01.png")), "mine/boulder");
    let n2 = set_of(&p, "landscape/rock02_n.png");
    assert_ne!(n2, set_of(&p, "landscape/rock02.png"));
    assert_eq!(set_id(&root, &n2), "landscape/rock02_n");
}

#[test]
fn rename_keeps_members_and_changes_the_key() {
    let d = fixture();
    let mut r = req(&d);
    r.set_edits = vec![SetEdit::Rename { from: "landscape/rock01".into(), to: "landscape/boulder".into() }];
    let (p, _) = plan_events(r);
    let root = d.path().join("out");
    assert_eq!(set_id(&root, &set_of(&p, "landscape/rock01.png")), "landscape/boulder");
    assert_eq!(set_id(&root, &set_of(&p, "landscape/rock01_n.png")), "landscape/boulder");
}

#[test]
fn a_stale_edit_is_reported_and_skipped() {
    let d = fixture();
    let mut r = req(&d);
    r.set_edits = vec![
        SetEdit::Move { file: "landscape/gone.png".into(), to: "landscape/rock01".into() },
        SetEdit::Move { file: "landscape/rock02.png".into(), to: "nowhere/set".into() },
        SetEdit::Rename { from: "landscape/rock01".into(), to: "landscape/rock02".into() },
    ];
    let (p, ev) = plan_events(r);
    let stale: Vec<_> = ev
        .iter()
        .filter_map(|e| match &e.kind {
            EventKind::Warning(w) if w.code == Code::SetEditStale => Some(w.clone()),
            _ => None,
        })
        .collect();
    assert_eq!(stale.len(), 3, "{stale:?}");
    assert_eq!(stale[0].data["edit"]["op"], "move");
    assert!(stale[0].data["reason"].as_str().unwrap().contains("gone.png"));
    // Nothing moved.
    assert_ne!(set_of(&p, "landscape/rock02.png"), set_of(&p, "landscape/rock01.png"));
}
```

- [ ] **Step 2: Run the tests and confirm they fail.**

Run: `cargo test -p engine --test set_edits`
Expected: compile errors (`engine::sets` and `BatchRequest::set_edits` don't exist).

- [ ] **Step 3: Add the code.** In `crates/engine/src/codes.rs`:
  - Add a line after `SetBakeRatio`: `SetEditStale = "SET-EDIT-STALE" => Info,`
  - Extend the module doc's list with: `SET-EDIT-STALE (a GUI set edit that names a file or set that no longer exists, M3b)`.

  Then fix any exhaustive `match` over `Code` that the compiler flags (the CLI's human output may have one).

- [ ] **Step 4: Extract `duplicate_outputs` in `crates/engine/src/group.rs`.** It keeps the current behaviour of `group_sets_with`:

```rust
/// Groups of two or more inputs that map to one output path, compared case-insensitively. DDS inputs take no part:
/// they are skipped, not written. Called again after GUI output paths are applied (M3b), so it is separate.
pub fn duplicate_outputs(files: &[InputFile]) -> Vec<Vec<usize>> {
    let mut outs: BTreeMap<String, Vec<usize>> = BTreeMap::new();
    for (i, f) in files.iter().enumerate() {
        if f.kind != InputKind::Dds {
            outs.entry(fold(&f.output)).or_default().push(i);
        }
    }
    outs.into_values().filter(|v| v.len() > 1).collect()
}
```

In `group_sets_with`, delete the `outs` map and its `entry` line, and end with `(sets.into_values().collect(), duplicate_outputs(files))`.

- [ ] **Step 5: Create `crates/engine/src/sets.rs`** (Task 3 adds the output-path half):

```rust
//! GUI set edits and output paths (M3b spec §2.1, DECISIONS P1–P2). Applied after the automatic grouping: edits
//! first (set keys are fixed from then on), output paths second, then outputs are compared for duplicates again.
//! The CLI never sets either.
use crate::discover::InputFile;
use crate::group::{split_suffix_with, TextureSet};
use crate::overrides::{path_key_matches, project_rel};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

/// One change to the automatic grouping. Sets are named by id (the set key without the output root in front),
/// files by their `[files]` key (project-relative, compared case-insensitively).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "op", rename_all = "snake_case")]
pub enum SetEdit {
    /// A new, empty set.
    Create { set: String },
    /// The set keeps its members under a new id.
    Rename { from: String, to: String },
    /// The file leaves its set and joins `to`.
    Move { file: String, to: String },
    /// The file leaves its set and becomes a set of its own.
    Remove { file: String },
}

/// An edit that could not apply: it names a file or set that is not there (it is reported, never fatal).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StaleEdit {
    pub edit: SetEdit,
    pub reason: String,
}

fn fold(p: &Path) -> String {
    p.to_string_lossy().replace('\\', "/").to_lowercase()
}

/// The id of a set: its key without the output root in front (`landscape/rock01`). A key not under the root is
/// returned whole.
pub fn set_id(output_root: &Path, key: &str) -> String {
    let root = fold(output_root);
    let root = root.trim_end_matches('/');
    key.strip_prefix(root)
        .and_then(|r| r.strip_prefix('/'))
        .map_or_else(|| key.to_string(), str::to_string)
}

/// The key of the set with id `id`: lower-case, '/'-separated, under the output root (as `group_sets_with` builds
/// keys).
pub fn set_key(output_root: &Path, id: &str) -> String {
    let root = fold(output_root);
    let id = id.replace('\\', "/").to_lowercase();
    format!("{}/{}", root.trim_end_matches('/'), id.trim_matches('/'))
}

fn stem_lower(f: &InputFile) -> String {
    f.rel
        .file_stem()
        .unwrap_or_default()
        .to_string_lossy()
        .to_lowercase()
}

/// Apply `edits` in order. Returns the edits that named something missing; they change nothing. Sets left with no
/// members are dropped (the GUI shows an empty created set from its own file). The result is sorted by key, as
/// `group_sets_with` returns it.
pub fn apply_set_edits(
    sets: &mut Vec<TextureSet>,
    files: &[InputFile],
    root: Option<&Path>,
    output_root: &Path,
    aliases: &BTreeMap<String, String>,
    edits: &[SetEdit],
) -> Vec<StaleEdit> {
    let file_of = |k: &str| -> Option<usize> {
        let root = root?;
        files
            .iter()
            .position(|f| project_rel(root, f).is_some_and(|r| path_key_matches(k, &r)))
    };
    let pos = |sets: &Vec<TextureSet>, id: &str| {
        let key = set_key(output_root, id);
        sets.iter().position(|s| s.key == key)
    };
    let mut touched: BTreeSet<String> = BTreeSet::new();
    let mut stale = vec![];
    for e in edits {
        let r: Result<(), String> = match e {
            SetEdit::Create { set } => {
                // Idempotent: a created set whose name the automatic grouping also produced (e.g. after Duplicate
                // source wrote `<set>_rough.png`) is that set.
                if pos(sets, set).is_some() {
                    Ok(())
                } else {
                    sets.push(TextureSet {
                        key: set_key(output_root, set),
                        members: vec![],
                        suffixes: BTreeSet::new(),
                    });
                    Ok(())
                }
            }
            SetEdit::Rename { from, to } => match (pos(sets, from), pos(sets, to)) {
                (None, _) => Err(format!("no set '{from}'")),
                (Some(_), Some(_)) => Err(format!("a set '{to}' already exists")),
                (Some(i), None) => {
                    sets[i].key = set_key(output_root, to);
                    touched.insert(sets[i].key.clone());
                    Ok(())
                }
            },
            SetEdit::Move { file, to } => match (file_of(file), pos(sets, to)) {
                (None, _) => Err(format!("no file '{file}'")),
                (_, None) => Err(format!("no set '{to}'")),
                (Some(f), Some(t)) => {
                    for s in sets.iter_mut() {
                        if s.members.contains(&f) {
                            touched.insert(s.key.clone());
                            s.members.retain(|&m| m != f);
                        }
                    }
                    sets[t].members.push(f);
                    touched.insert(sets[t].key.clone());
                    Ok(())
                }
            },
            SetEdit::Remove { file } => match file_of(file) {
                None => Err(format!("no file '{file}'")),
                Some(f) => {
                    for s in sets.iter_mut() {
                        if s.members.contains(&f) {
                            touched.insert(s.key.clone());
                            s.members.retain(|&m| m != f);
                        }
                    }
                    let dir = files[f].output.parent().map(fold).unwrap_or_default();
                    let base = format!("{dir}/{}", stem_lower(&files[f]));
                    let mut key = base.clone();
                    let mut n = 2;
                    while sets.iter().any(|s| s.key == key) {
                        key = format!("{base}~{n}");
                        n += 1;
                    }
                    touched.insert(key.clone());
                    sets.push(TextureSet {
                        key,
                        members: vec![f],
                        suffixes: BTreeSet::new(),
                    });
                    Ok(())
                }
            },
        };
        if let Err(reason) = r {
            stale.push(StaleEdit {
                edit: e.clone(),
                reason,
            });
        }
    }
    // A touched set's suffixes come from its members, as `group_sets_with` derives them.
    for s in sets.iter_mut().filter(|s| touched.contains(&s.key)) {
        s.members.sort_unstable();
        s.suffixes = s
            .members
            .iter()
            .filter_map(|&i| split_suffix_with(&stem_lower(&files[i]), aliases).suffix)
            .collect();
    }
    sets.retain(|s| !s.members.is_empty());
    sets.sort_by(|a, b| a.key.cmp(&b.key));
    stale
}
```

Note: `split_suffix_with` takes the stem as it is on disk in `group_sets_with`. Lower-casing it is safe, because suffixes are compared lower-case (check `split_suffix_with`'s doc; if it returns the suffix lower-cased already, this is identical). Add `pub mod sets;` to `lib.rs`.

- [ ] **Step 6: Wire it into `crates/engine/src/api.rs`.**
  - Add to `BatchRequest`, after `file_allow`:

    ```rust
    /// GUI set edits (M3b spec §2.1): applied in order after the automatic grouping. Empty for the CLI.
    pub set_edits: Vec<crate::sets::SetEdit>,
    ```

    and `set_edits: vec![],` in `BatchRequest::new`.
  - In `plan_cached`:
    - Compute the project root **before** grouping. The `file_scopes` block below uses the same expression, so bind it once and reuse it there: `let scope_root = project_root(&req).or_else(|| req.inputs.first().filter(|p| p.is_dir()).cloned());`
    - Replace `let (mut sets, dups) = group_sets_with(&files, registry.alias_table());` with:

```rust
        // M3b planning sequence (spec §2.1, DECISIONS P1): 1 automatic groups, 2 GUI set edits (keys are fixed from
        // here on), 3 GUI output paths, 4 duplicate outputs compared on the final paths.
        let (mut sets, _) = group_sets_with(&files, registry.alias_table());
        let stale = crate::sets::apply_set_edits(
            &mut sets,
            &files,
            scope_root.as_deref(),
            &req.output_root,
            registry.alias_table(),
            &req.set_edits,
        );
        for s in &stale {
            rep.emit(
                None,
                None,
                EventKind::Warning(
                    Warning::new(
                        Code::SetEditStale,
                        format!("set edit skipped: {}", s.reason),
                    )
                    .with_data(serde_json::json!({ "edit": s.edit, "reason": s.reason })),
                ),
            );
        }
        let dups = crate::group::duplicate_outputs(&files);
```

  - Further down, use `root: scope_root.clone(),` in `FileScopes`.

- [ ] **Step 7: Run the engine tests and confirm they pass.**

Run: `cargo test -p engine --test set_edits && cargo test -p engine`
Expected: all pass. `golden.rs` is unchanged and green.

- [ ] **Step 8: Regenerate the schemas, then run every gate.**

```bash
cargo run -p sdc -- presets schema all --out schema
cargo fmt --all && cargo clippy --workspace --all-targets -- -D warnings && cargo test --workspace
```

Expected: `schema/event.schema.json` and `schema/dds-validation.schema.json` gain `SET-EDIT-STALE`; all tests pass.

- [ ] **Step 9: Commit.**

```bash
git add crates/engine/src/sets.rs crates/engine/src/lib.rs crates/engine/src/group.rs crates/engine/src/api.rs crates/engine/src/codes.rs crates/engine/tests/set_edits.rs schema/
git commit -m "feat(engine): GUI set edits applied after automatic grouping; SET-EDIT-STALE (M3b §2.1)

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

Add any CLI file the compiler made you touch to `git add` explicitly.

---

### Task 3: Engine `output_paths` (spec §2.1, DECISIONS P1)

**Files:**
- Modify:
  - `crates/engine/src/sets.rs` (add `OutputKey`, `apply_output_paths`, `packed_outputs`);
  - `crates/engine/src/api.rs` (field, `new`, steps 3–4 of the sequence, the `plan_packing` call);
  - `crates/engine/src/pack/plan.rs` (`plan_packing` parameter; override `vin.output`).
- Every other call site of `plan_packing` (find them with `grep -rn "plan_packing(" crates/`) passes `&BTreeMap::new()`.
- Test: `crates/engine/tests/set_edits.rs` (append)

**Interfaces:**
- Consumes: Task 2's `sets.rs`, `set_key`, `duplicate_outputs`.
- Produces: `OutputKey`, `apply_output_paths`, `packed_outputs`, `BatchRequest::output_paths`, `plan_packing(…, packed_outputs)`.

- [ ] **Step 1: Write the failing tests.** Append to `crates/engine/tests/set_edits.rs`:

```rust
use std::collections::BTreeMap;

fn outputs(p: &Plan) -> Vec<String> {
    let mut v: Vec<String> = p
        .jobs
        .iter()
        .map(|j| j.output.to_string_lossy().replace('\\', "/"))
        .collect();
    v.sort();
    v
}

#[test]
fn output_paths_rewrite_files_and_packed_outputs_but_not_set_keys() {
    let d = fixture();
    let (before, _) = plan_events(req(&d));
    let key_before = set_of(&before, "landscape/rock01.png");
    let mut r = req(&d);
    r.set_edits = vec![
        SetEdit::Move { file: "stone/stone_roughness.png".into(), to: "landscape/rock01".into() },
        SetEdit::Move { file: "stone/stone_ao.png".into(), to: "landscape/rock01".into() },
    ];
    r.output_paths = BTreeMap::from([
        (OutputKey::File("landscape/rock01.png".into()), PathBuf::from("textures/pbr/landscape/rock01.dds")),
        (OutputKey::File("landscape/rock01_n.png".into()), PathBuf::from("textures/pbr/landscape/rock01_n.dds")),
        (
            OutputKey::Packed { set: "landscape/rock01".into(), suffix: "rmaos".into() },
            PathBuf::from("textures/pbr/landscape/rock01_rmaos.dds"),
        ),
    ]);
    let (p, _) = plan_events(r);
    let o = outputs(&p);
    for tail in [
        "out/textures/pbr/landscape/rock01.dds",
        "out/textures/pbr/landscape/rock01_n.dds",
        "out/textures/pbr/landscape/rock01_rmaos.dds",
    ] {
        assert!(o.iter().any(|x| x.ends_with(tail)), "{tail} missing from {o:?}");
    }
    assert_eq!(set_of(&p, "landscape/rock01.png"), key_before, "keys never follow output paths");
}

#[test]
fn two_files_given_one_output_path_are_duplicates() {
    let d = fixture();
    let mut r = req(&d);
    r.output_paths = BTreeMap::from([
        (OutputKey::File("landscape/rock01.png".into()), PathBuf::from("textures/pbr/x/rock.dds")),
        (OutputKey::File("landscape/rock02.png".into()), PathBuf::from("textures/pbr/x/rock.dds")),
    ]);
    let (p, _) = plan_events(r);
    let dup: Vec<_> = p.skipped.iter().filter(|s| s.code == Some(Code::InDuplicate)).collect();
    assert_eq!(dup.len(), 2, "{:?}", p.skipped);
}

#[test]
fn an_output_path_for_a_missing_file_is_ignored() {
    let d = fixture();
    let (a, _) = plan_events(req(&d));
    let mut r = req(&d);
    r.output_paths = BTreeMap::from([(OutputKey::File("gone.png".into()), PathBuf::from("x.dds"))]);
    let (b, _) = plan_events(r);
    assert_eq!(outputs(&a), outputs(&b));
}
```

- [ ] **Step 2: Run the tests and confirm they fail.**

Run: `cargo test -p engine --test set_edits output_paths two_files_given an_output_path`
Expected: compile errors (`OutputKey` and `output_paths` don't exist).

- [ ] **Step 3: Add to `crates/engine/src/sets.rs`:**

```rust
/// What an explicit output path applies to (DECISIONS P1).
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OutputKey {
    /// A source file, by its `[files]` key.
    File(String),
    /// A packed output of a set: the set id and the recipe suffix (`rmaos`, `rfaos`, `p`, `m`).
    Packed { set: String, suffix: String },
}

/// Give each file named by an `OutputKey::File` the output `output_root/<path>`. Keys that match no file are
/// ignored (the GUI computes them from the previous plan; a vanished file is reported by its set edit, if any).
pub fn apply_output_paths(
    files: &mut [InputFile],
    root: Option<&Path>,
    output_root: &Path,
    paths: &BTreeMap<OutputKey, PathBuf>,
) {
    let Some(root) = root else { return };
    for (k, rel) in paths {
        let OutputKey::File(key) = k else { continue };
        for f in files.iter_mut() {
            if project_rel(root, f).is_some_and(|r| path_key_matches(key, &r)) {
                f.output = output_root.join(rel);
            }
        }
    }
}

/// The packed-output paths by (set key, recipe suffix), absolute, for `plan_packing`.
pub fn packed_outputs(
    output_root: &Path,
    paths: &BTreeMap<OutputKey, PathBuf>,
) -> BTreeMap<(String, String), PathBuf> {
    paths
        .iter()
        .filter_map(|(k, rel)| match k {
            OutputKey::Packed { set, suffix } => Some((
                (set_key(output_root, set), suffix.to_lowercase()),
                output_root.join(rel),
            )),
            OutputKey::File(_) => None,
        })
        .collect()
}
```

- [ ] **Step 4: Wire it in.**
  - **`api.rs`:** add to `BatchRequest`, after `set_edits`:

    ```rust
    /// GUI output paths (M3b, DECISIONS P1), relative to `output_root`. Empty for the CLI.
    pub output_paths: BTreeMap<crate::sets::OutputKey, PathBuf>,
    ```

    and `output_paths: BTreeMap::new(),` in `new`.
  - **`plan_cached`:** between the stale-edit loop and `let dups = …`:

    ```rust
    crate::sets::apply_output_paths(
        &mut files,
        scope_root.as_deref(),
        &req.output_root,
        &req.output_paths,
    );
    let packed_outputs = crate::sets::packed_outputs(&req.output_root, &req.output_paths);
    ```

    `files` must be `let mut files` already (it is, for `canonicalise_outputs`). The `file_discovered` events are emitted *before* grouping with the mirrored output. **Move that emit loop to after `apply_output_paths`**, so the events carry the final output and job keys stay consistent. This changes no CLI output, because with no paths the outputs are identical.
  - **The `plan_packing` call:** add `&packed_outputs` as the last argument.
  - **`pack/plan.rs`:**
    - add the parameter `packed_outputs: &BTreeMap<(String, String), PathBuf>`, with a doc line: "explicit GUI output paths of packed outputs by (set key, suffix), M3b";
    - after `let vin = virtual_input(members[&sources[0]], recipe.kind);`, write:

      ```rust
      let mut vin = virtual_input(members[&sources[0]], recipe.kind);
      if let Some(p) = packed_outputs.get(&(set.key.clone(), recipe.kind.suffix().to_string())) {
          vin.output = p.clone();
      }
      ```

      and drop the old `let vin` line.

- [ ] **Step 5: Run the tests and confirm they pass.**

Run: `cargo test -p engine --test set_edits && cargo test -p engine && cargo test -p sdc`
Expected: all pass; `gui_repro` and `golden` are unchanged.

- [ ] **Step 6: Run the gates and commit.** `OutputKey` isn't in a schema (`BatchRequest` isn't serialized), so there is nothing to regenerate. Run the schema drift test anyway; it is part of `cargo test --workspace`.

```bash
cargo fmt --all && cargo clippy --workspace --all-targets -- -D warnings && cargo test --workspace
git add crates/engine/src/sets.rs crates/engine/src/api.rs crates/engine/src/pack/plan.rs crates/engine/tests/set_edits.rs
git commit -m "feat(engine): explicit GUI output paths per file and per packed recipe; duplicates compared after (M3b §2.1, P1)

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

Add every test file whose `plan_packing` call you updated.

---
### Task 4: `sdc-project.toml` (spec §7.1, DECISIONS P8)

**Files:**
- Create: `crates/gui/src/sets_doc.rs`
- Modify:
  - `crates/gui/src/lib.rs` (`pub mod sets_doc;`);
  - `crates/gui/src/project.rs` (`text`, `restore`; `SaveError` messages without a file name);
  - `crates/gui/src/model/mod.rs` (the `sets_doc` field, open/save/dirty, external change per file, banner);
  - `crates/gui/src/ui/dialogs.rs` (external-change text names the file).
- Test: `crates/gui/tests/sets_doc.rs`

**Interfaces:**
- Consumes: `project::{ProjectDoc, SaveError}`, `engine::policy::SourceStamp`.
- Produces:
  - `SetsDoc` and `SetEntry` (registry);
  - `pgp::settings::Settings` is needed here as a type alias. This task creates `crates/gui/src/pgp/mod.rs` with `pub mod settings;` and `settings.rs` holding only `pub type Settings = std::collections::BTreeMap<String, serde_json::Value>;` (Task 6 fills the rest);
  - `AppModel.sets_doc: Option<SetsDoc>`;
  - `Dialog::ExternalChange(DocKind)`, with `pub enum DocKind { SdcToml, Project }`;
  - `AppModel::resolve_external_change(which: DocKind, overwrite: bool)`.

**File shape** (the spec §7.1 shape, with an explicit order list; the GUI writes only these keys):

```toml
[project]
json_name = "rock-pack"
last_json_sha256 = "…"
order = ["landscape/rock02", "landscape/rock01"]   # written on the first reorder; sets not listed follow, by id

[defaults]
specular_level = 0.04

[[sets]]
id = "landscape/rock02"          # the set id (engine key without the export root)
created = true                   # only on sets the user made (P3)
renamed_from = "landscape/x"     # only after a rename of an automatic set
add = ["stone/rock02_rough.png"] # files moved into this set ([files] keys)
remove = ["landscape/rock02_s.png"]  # files split off into a set of their own
borrow = { slot6 = "landscape/rock01" }  # slot → the set whose finished output it uses
match_diffuse = "landscape\\rock02"
parallax = true
glint.screen_space_scale = 1.5   # nested schema keys as dotted keys
```

The spec's `key =` becomes `id =`, the name the engine uses (`set_id`). `order` is the explicit order. Without it, an edit on one automatic set would reorder the sets.

- [ ] **Step 1: Write the failing tests.** Create `crates/gui/tests/sets_doc.rs`:

```rust
mod support;
use sdc_gui::sets_doc::SetsDoc;
use serde_json::json;

fn doc(text: Option<&str>) -> (tempfile::TempDir, SetsDoc) {
    let d = tempfile::tempdir().unwrap();
    if let Some(t) = text {
        std::fs::write(d.path().join("sdc-project.toml"), t).unwrap();
    }
    let s = SetsDoc::open(d.path());
    (d, s)
}

#[test]
fn a_missing_file_is_an_empty_clean_document() {
    let (_d, s) = doc(None);
    assert!(!s.is_dirty() && !s.read_only());
    assert!(s.entries().is_empty());
    assert_eq!(s.json_name(), None);
}

#[test]
fn entries_read_back_with_their_settings_flattened() {
    let (_d, s) = doc(Some(
        "[project]\njson_name = \"pack\"\norder = [\"b/two\", \"a/one\"]\n\n[defaults]\nspecular_level = 0.04\n\n\
         [[sets]]\nid = \"a/one\"\ncreated = true\nadd = [\"x/y.png\"]\nborrow = { slot6 = \"b/two\" }\n\
         parallax = true\nglint.screen_space_scale = 1.5\nemissive_color = [255, 128, 0]\n",
    ));
    assert_eq!(s.json_name().as_deref(), Some("pack"));
    assert_eq!(s.order(), vec!["b/two".to_string(), "a/one".into()]);
    assert_eq!(s.defaults()["specular_level"], json!(0.04));
    let e = &s.entries()[0];
    assert_eq!(e.id, "a/one");
    assert!(e.created);
    assert_eq!(e.add, vec!["x/y.png".to_string()]);
    assert_eq!(e.borrow.get(&6).map(String::as_str), Some("b/two"));
    assert_eq!(e.settings["parallax"], json!(true));
    assert_eq!(e.settings["glint.screen_space_scale"], json!(1.5));
    assert_eq!(e.settings["emissive_color"], json!([255, 128, 0]));
    assert!(!e.settings.contains_key("id") && !e.settings.contains_key("add"));
}

#[test]
fn edits_preserve_comments_and_round_trip() {
    let (d, mut s) = doc(Some("# my notes\n[[sets]]\nid = \"a/one\" # keep me\nparallax = true\n"));
    s.set_value("a/one", "displacement_scale", Some(json!(0.4)));
    s.set_value("a/one", "glint.screen_space_scale", Some(json!(1.5)));
    s.set_value("b/two", "hair", Some(json!(true))); // creates an entry
    s.set_value("a/one", "parallax", None); // removes the key
    s.add_member("a/one", "x/y.png");
    s.set_borrow("a/one", 6, Some("b/two"));
    s.set_json_name(Some("pack"));
    assert!(s.is_dirty());
    s.save(false).unwrap();
    let text = std::fs::read_to_string(d.path().join("sdc-project.toml")).unwrap();
    assert!(text.contains("# my notes") && text.contains("# keep me"), "{text}");
    let again = SetsDoc::open(d.path());
    let a = again.entries().into_iter().find(|e| e.id == "a/one").unwrap();
    assert_eq!(a.settings["displacement_scale"], json!(0.4));
    assert_eq!(a.settings["glint.screen_space_scale"], json!(1.5));
    assert!(!a.settings.contains_key("parallax"));
    assert_eq!(a.add, vec!["x/y.png".to_string()]);
    assert_eq!(a.borrow.get(&6).map(String::as_str), Some("b/two"));
    assert!(again.entries().iter().any(|e| e.id == "b/two"));
}

#[test]
fn create_rename_delete_and_order() {
    let (_d, mut s) = doc(None);
    s.create_set("mine/one");
    s.create_set("mine/two");
    s.rename_set("mine/one", "mine/uno");
    assert!(s.entries().iter().any(|e| e.id == "mine/uno" && e.created && e.renamed_from.is_none()));
    s.set_order(&["mine/two".to_string(), "mine/uno".into()]);
    assert_eq!(s.order(), vec!["mine/two".to_string(), "mine/uno".into()]);
    s.delete_set("mine/two");
    assert!(!s.entries().iter().any(|e| e.id == "mine/two"));
    assert_eq!(s.order(), vec!["mine/uno".to_string()], "a deleted set leaves the order list");
    // Renaming an automatic set (no entry yet) records where it came from.
    s.rename_set("landscape/rock01", "landscape/boulder");
    let b = s.entries().into_iter().find(|e| e.id == "landscape/boulder").unwrap();
    assert_eq!(b.renamed_from.as_deref(), Some("landscape/rock01"));
    assert!(!b.created);
}

#[test]
fn a_bad_file_is_read_only_and_never_written() {
    let (d, mut s) = doc(Some("[[sets]\nid = "));
    assert!(s.read_only());
    assert!(s.error().unwrap().contains("sdc-project.toml"));
    s.set_value("a", "hair", Some(json!(true)));
    assert!(!s.is_dirty());
    assert!(s.save(false).is_err());
    assert_eq!(std::fs::read_to_string(d.path().join("sdc-project.toml")).unwrap(), "[[sets]\nid = ");
}

#[test]
fn text_and_restore_round_trip_and_mark_dirty() {
    let (_d, mut s) = doc(Some("[[sets]]\nid = \"a/one\"\n"));
    let before = s.text();
    s.set_value("a/one", "hair", Some(json!(true)));
    let after = s.text();
    s.restore(&before);
    assert_eq!(s.text(), before);
    assert!(s.is_dirty(), "an undo is an unsaved change");
    s.restore(&after);
    assert_eq!(s.entries()[0].settings["hair"], json!(true));
}

#[test]
fn an_outside_change_blocks_a_plain_save() {
    let (d, mut s) = doc(Some("[[sets]]\nid = \"a/one\"\n"));
    s.set_value("a/one", "hair", Some(json!(true)));
    std::thread::sleep(std::time::Duration::from_millis(20));
    std::fs::write(d.path().join("sdc-project.toml"), "# changed elsewhere\n").unwrap();
    assert!(s.changed_on_disk());
    assert_eq!(s.save(false), Err(sdc_gui::project::SaveError::ChangedOnDisk));
    s.save(true).unwrap();
}
```

- [ ] **Step 2: Run the tests and confirm they fail.**

Run: `cargo test -p sdc-gui --test sets_doc`
Expected: compile error (no `sets_doc` module).

- [ ] **Step 3: In `crates/gui/src/project.rs`, make `SaveError`'s text file-neutral and add `text`/`restore` to `ProjectDoc`.**

```rust
impl std::fmt::Display for SaveError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            SaveError::ReadOnly => f.write_str("the file is read-only until its error is fixed and it is reloaded"),
            SaveError::ChangedOnDisk => f.write_str("the file changed on disk since it was loaded"),
            SaveError::Io(e) => write!(f, "could not save: {e}"),
        }
    }
}
```

Then grep for the old strings and update every test that asserts them, prefixing the file name where the model shows them (see Step 5): `grep -rn "sdc.toml changed on disk\|could not save sdc.toml\|sdc.toml is read-only until" crates/gui`.

Add to `impl ProjectDoc`:

```rust
    /// The document as text (undo snapshots, DECISIONS P9).
    pub fn text(&self) -> String {
        self.doc.to_string()
    }

    /// Replace the document with `text` (an undo or redo). It is an unsaved change. A read-only document is left
    /// alone: its edits never happened.
    pub fn restore(&mut self, text: &str) {
        if self.read_only() {
            return;
        }
        if let Ok(doc) = text.parse::<DocumentMut>() {
            self.doc = doc;
            self.dirty = true;
        }
    }
```

- [ ] **Step 4: Create `crates/gui/src/pgp/mod.rs` and a minimal `pgp/settings.rs`** (Task 6 extends it):

```rust
// crates/gui/src/pgp/mod.rs
//! PGPatcher TruePBR JSON (M3b spec §5): key table, write rules, prefixes, writer, validator. No egui types.
pub mod settings;
```

```rust
// crates/gui/src/pgp/settings.rs
//! A set's (or the defaults') JSON settings, by schema key path (`glint.screen_space_scale`). Colours are 0–255
//! integer arrays here; the writer converts them (DECISIONS P10).
pub type Settings = std::collections::BTreeMap<String, serde_json::Value>;
```

Add `pub mod pgp;` and `pub mod sets_doc;` to `lib.rs`.

- [ ] **Step 5: Create `crates/gui/src/sets_doc.rs`.** It reuses the `json_to_toml`/`toml_to_json` helpers: make those two functions in `project.rs` `pub(crate)`, and also `dotted_table`, `sub_table`, `set_value` and `find_key`.

```rust
//! `sdc-project.toml` (M3b spec §7.1): the user's sets and JSON settings, next to `sdc.toml`. Format-preserving
//! like `ProjectDoc`; written only on Save or Export. A file that does not parse makes set and JSON editing
//! read-only (DECISIONS P8); `sdc.toml` editing is unaffected.
use crate::pgp::settings::Settings;
use crate::project::{dotted_table, json_to_toml, set_value, sub_table, toml_to_json, SaveError};
use engine::policy::SourceStamp;
use serde_json::Value as Json;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use toml_edit::{Array, ArrayOfTables, DocumentMut, Item, Table, TableLike, Value};

pub const FILE_NAME: &str = "sdc-project.toml";
/// Keys of a `[[sets]]` table that are not JSON settings.
const RESERVED: [&str; 6] = ["id", "created", "renamed_from", "add", "remove", "borrow"];

#[derive(Debug, Clone, PartialEq, Default)]
pub struct SetEntry {
    pub id: String,
    pub created: bool,
    pub renamed_from: Option<String>,
    pub add: Vec<String>,
    pub remove: Vec<String>,
    /// Slot number (1–8) → the id of the set whose finished output this set uses.
    pub borrow: BTreeMap<u8, String>,
    pub settings: Settings,
}

pub struct SetsDoc {
    path: PathBuf,
    doc: DocumentMut,
    disk: Option<SourceStamp>,
    dirty: bool,
    error: Option<String>,
}

/// Flatten a table into `prefix.key` paths; values only (nested tables recurse).
fn flatten(t: &dyn TableLike, prefix: &str, out: &mut Settings) {
    for (k, item) in t.iter() {
        let path = if prefix.is_empty() { k.to_string() } else { format!("{prefix}.{k}") };
        match item.as_table_like() {
            Some(sub) if !item.is_value() || item.as_inline_table().is_some() => flatten(sub, &path, out),
            _ => {
                if let Some(v) = toml_to_json(item) {
                    out.insert(path, v);
                }
            }
        }
    }
}

fn strings(item: Option<&Item>) -> Vec<String> {
    item.and_then(Item::as_array)
        .map(|a| a.iter().filter_map(|v| v.as_str().map(str::to_string)).collect())
        .unwrap_or_default()
}

fn entry_of(t: &Table) -> Option<SetEntry> {
    let id = t.get("id")?.as_str()?.to_string();
    let mut settings = Settings::new();
    for (k, item) in t.iter() {
        if RESERVED.contains(&k) {
            continue;
        }
        match item.as_table_like() {
            Some(sub) if item.as_value().is_none() || item.as_inline_table().is_some() => flatten(sub, k, &mut settings),
            _ => {
                if let Some(v) = toml_to_json(item) {
                    settings.insert(k.to_string(), v);
                }
            }
        }
    }
    let borrow = t
        .get("borrow")
        .and_then(Item::as_table_like)
        .map(|b| {
            b.iter()
                .filter_map(|(k, v)| {
                    let n: u8 = k.strip_prefix("slot")?.parse().ok()?;
                    Some((n, v.as_str()?.to_string()))
                })
                .collect()
        })
        .unwrap_or_default();
    Some(SetEntry {
        id,
        created: t.get("created").and_then(Item::as_bool).unwrap_or(false),
        renamed_from: t.get("renamed_from").and_then(Item::as_str).map(str::to_string),
        add: strings(t.get("add")),
        remove: strings(t.get("remove")),
        borrow,
        settings,
    })
}

impl SetsDoc {
    pub fn open(root: &Path) -> SetsDoc {
        let mut d = SetsDoc {
            path: root.join(FILE_NAME),
            doc: DocumentMut::new(),
            disk: None,
            dirty: false,
            error: None,
        };
        d.reload();
        d
    }

    pub fn reload(&mut self) {
        self.dirty = false;
        self.error = None;
        self.doc = DocumentMut::new();
        self.disk = SourceStamp::of(&self.path);
        match std::fs::read_to_string(&self.path) {
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(e) => self.error = Some(format!("{}: {e}", self.path.display())),
            Ok(text) => match text.parse::<DocumentMut>() {
                Ok(doc) => self.doc = doc,
                Err(e) => self.error = Some(format!("{}: {e}", self.path.display())),
            },
        }
    }

    pub fn path(&self) -> &Path {
        &self.path
    }
    pub fn is_dirty(&self) -> bool {
        self.dirty
    }
    pub fn read_only(&self) -> bool {
        self.error.is_some()
    }
    pub fn error(&self) -> Option<&str> {
        self.error.as_deref()
    }
    pub fn changed_on_disk(&self) -> bool {
        SourceStamp::of(&self.path) != self.disk
    }

    pub fn text(&self) -> String {
        self.doc.to_string()
    }

    /// Replace the document with `text` (undo/redo): an unsaved change. Ignored while read-only.
    pub fn restore(&mut self, text: &str) {
        if self.read_only() {
            return;
        }
        if let Ok(doc) = text.parse::<DocumentMut>() {
            self.doc = doc;
            self.dirty = true;
        }
    }

    pub fn save(&mut self, force: bool) -> Result<(), SaveError> {
        if self.read_only() {
            return Err(SaveError::ReadOnly);
        }
        if !force && self.changed_on_disk() {
            return Err(SaveError::ChangedOnDisk);
        }
        let tmp = self.path.with_extension("toml.sdc-tmp");
        std::fs::write(&tmp, self.doc.to_string())
            .and_then(|_| std::fs::rename(&tmp, &self.path))
            .map_err(|e| SaveError::Io(e.to_string()))?;
        self.disk = SourceStamp::of(&self.path);
        self.dirty = false;
        Ok(())
    }

    fn project_str(&self, key: &str) -> Option<String> {
        self.doc.get("project")?.get(key)?.as_str().map(str::to_string)
    }

    fn set_project(&mut self, key: &str, v: Option<Value>) {
        if self.read_only() {
            return;
        }
        let Some(p) = sub_table(self.doc.as_table_mut(), "project", || Item::Table(Table::new())) else {
            return;
        };
        match v {
            Some(v) => set_value(p, key, v),
            None => {
                p.remove(key);
            }
        }
        self.dirty = true;
    }

    pub fn json_name(&self) -> Option<String> {
        self.project_str("json_name")
    }
    pub fn set_json_name(&mut self, name: Option<&str>) {
        self.set_project("json_name", name.map(Value::from));
    }
    pub fn last_json_sha256(&self) -> Option<String> {
        self.project_str("last_json_sha256")
    }
    pub fn set_last_json_sha256(&mut self, sha: &str) {
        self.set_project("last_json_sha256", Some(Value::from(sha)));
    }

    pub fn order(&self) -> Vec<String> {
        strings(self.doc.get("project").and_then(|p| p.get("order")))
    }
    pub fn set_order(&mut self, ids: &[String]) {
        let mut arr = Array::new();
        for id in ids {
            arr.push(id.as_str());
        }
        self.set_project("order", Some(Value::Array(arr)));
    }

    pub fn defaults(&self) -> Settings {
        let mut out = Settings::new();
        if let Some(t) = self.doc.get("defaults").and_then(Item::as_table_like) {
            flatten(t, "", &mut out);
        }
        out
    }

    pub fn set_default(&mut self, key: &str, v: Option<Json>) {
        if self.read_only() {
            return;
        }
        let Some(t) = sub_table(self.doc.as_table_mut(), "defaults", || Item::Table(Table::new())) else {
            return;
        };
        if write_path(t, key, v) {
            self.dirty = true;
        }
        if self.doc.get("defaults").and_then(Item::as_table_like).is_some_and(TableLike::is_empty) {
            self.doc.remove("defaults");
        }
    }

    pub fn entries(&self) -> Vec<SetEntry> {
        self.doc
            .get("sets")
            .and_then(Item::as_array_of_tables)
            .map(|a| a.iter().filter_map(entry_of).collect())
            .unwrap_or_default()
    }

    fn tables_mut(&mut self) -> &mut ArrayOfTables {
        if !self.doc.get("sets").is_some_and(Item::is_array_of_tables) {
            self.doc.insert("sets", Item::ArrayOfTables(ArrayOfTables::new()));
        }
        self.doc["sets"].as_array_of_tables_mut().expect("just made")
    }

    /// The table of set `id`, created at the end when missing (ids compare case-insensitively).
    fn table_mut(&mut self, id: &str) -> &mut Table {
        let lower = id.to_lowercase();
        let tables = self.tables_mut();
        let i = tables
            .iter()
            .position(|t| t.get("id").and_then(Item::as_str).is_some_and(|s| s.to_lowercase() == lower));
        let i = match i {
            Some(i) => i,
            None => {
                let mut t = Table::new();
                t.insert("id", toml_edit::value(id));
                tables.push(t);
                tables.len() - 1
            }
        };
        tables.get_mut(i).expect("index in range")
    }

    pub fn create_set(&mut self, id: &str) {
        if self.read_only() {
            return;
        }
        self.table_mut(id).insert("created", toml_edit::value(true));
        self.dirty = true;
    }

    /// Drop set `id`'s entry and remove it from the order list (DECISIONS P2: its files return to automatic grouping).
    pub fn delete_set(&mut self, id: &str) {
        if self.read_only() {
            return;
        }
        let lower = id.to_lowercase();
        if let Some(tables) = self.doc.get_mut("sets").and_then(Item::as_array_of_tables_mut) {
            tables.retain(|t| !t.get("id").and_then(Item::as_str).is_some_and(|s| s.to_lowercase() == lower));
        }
        let order: Vec<String> = self.order().into_iter().filter(|o| o.to_lowercase() != lower).collect();
        if !self.order().is_empty() {
            self.set_order(&order);
        }
        self.dirty = true;
    }

    /// Rename set `from` to `to`. A created set just changes its id; an automatic set records `renamed_from`. Other
    /// entries that borrow from `from`, and the order list, follow.
    pub fn rename_set(&mut self, from: &str, to: &str) {
        if self.read_only() {
            return;
        }
        let created = self.entries().iter().any(|e| e.id.eq_ignore_ascii_case(from) && e.created);
        let already = self.entries().iter().find(|e| e.id.eq_ignore_ascii_case(from)).and_then(|e| e.renamed_from.clone());
        let t = self.table_mut(from);
        t.insert("id", toml_edit::value(to));
        if !created {
            t.insert("renamed_from", toml_edit::value(already.unwrap_or_else(|| from.to_string())));
        }
        let order: Vec<String> = self
            .order()
            .into_iter()
            .map(|o| if o.eq_ignore_ascii_case(from) { to.to_string() } else { o })
            .collect();
        if !order.is_empty() {
            self.set_order(&order);
        }
        for e in self.entries() {
            for (slot, src) in &e.borrow {
                if src.eq_ignore_ascii_case(from) {
                    self.set_borrow(&e.id, *slot, Some(to));
                }
            }
        }
        self.dirty = true;
    }

    fn list_edit(&mut self, id: &str, key: &str, file: &str, add: bool) {
        if self.read_only() {
            return;
        }
        let t = self.table_mut(id);
        let mut items = strings(t.get(key));
        items.retain(|f| !f.eq_ignore_ascii_case(file));
        if add {
            items.push(file.to_string());
        }
        if items.is_empty() {
            t.remove(key);
        } else {
            let mut arr = Array::new();
            for f in &items {
                arr.push(f.as_str());
            }
            t.insert(key, toml_edit::value(arr));
        }
        self.dirty = true;
    }

    /// `file` moves into set `id` (a `Move` edit); it is taken out of every other entry's `add` and `remove`.
    pub fn add_member(&mut self, id: &str, file: &str) {
        for e in self.entries() {
            if !e.id.eq_ignore_ascii_case(id) {
                if e.add.iter().any(|f| f.eq_ignore_ascii_case(file)) {
                    self.list_edit(&e.id, "add", file, false);
                }
                if e.remove.iter().any(|f| f.eq_ignore_ascii_case(file)) {
                    self.list_edit(&e.id, "remove", file, false);
                }
            }
        }
        self.list_edit(id, "add", file, true);
    }

    /// `file` leaves set `id`: it is dropped from `id`'s `add` when it was moved in, else recorded in `remove`.
    pub fn remove_member(&mut self, id: &str, file: &str) {
        let moved_in = self
            .entries()
            .iter()
            .any(|e| e.id.eq_ignore_ascii_case(id) && e.add.iter().any(|f| f.eq_ignore_ascii_case(file)));
        if moved_in {
            self.list_edit(id, "add", file, false);
        } else {
            self.list_edit(id, "remove", file, true);
        }
    }

    pub fn set_borrow(&mut self, id: &str, slot: u8, from: Option<&str>) {
        if self.read_only() {
            return;
        }
        let t = self.table_mut(id);
        let Some(b) = sub_table(t, "borrow", || Item::Value(Value::InlineTable(Default::default()))) else {
            return;
        };
        let key = format!("slot{slot}");
        match from {
            Some(f) => set_value(b, &key, Value::from(f)),
            None => {
                b.remove(&key);
            }
        }
        if t.get("borrow").and_then(Item::as_table_like).is_some_and(TableLike::is_empty) {
            t.remove("borrow");
        }
        self.dirty = true;
    }

    /// Set (or with `None` remove) JSON setting `key` (a dotted schema path) of set `id`.
    pub fn set_value(&mut self, id: &str, key: &str, v: Option<Json>) {
        if self.read_only() {
            return;
        }
        let t = self.table_mut(id);
        if write_path(t, key, v) {
            self.dirty = true;
        }
    }
}

/// Write `v` at dotted `path` under `t` (sub-tables as dotted keys), or remove it with `None` (and any sub-table it
/// leaves empty). True when something changed.
fn write_path(t: &mut dyn TableLike, path: &str, v: Option<Json>) -> bool {
    let parts: Vec<&str> = path.split('.').collect();
    let (last, dirs) = parts.split_last().expect("one part at least");
    match v {
        Some(v) => {
            let Some(v) = json_to_toml(&v) else { return false };
            let mut cur: &mut dyn TableLike = t;
            for d in dirs {
                match sub_table(cur, d, dotted_table) {
                    Some(next) => cur = next,
                    None => return false,
                }
            }
            set_value(cur, last, v);
            true
        }
        None => {
            fn remove(t: &mut dyn TableLike, parts: &[&str]) -> bool {
                if parts.len() == 1 {
                    return t.remove(parts[0]).is_some();
                }
                let Some(sub) = t.get_mut(parts[0]).and_then(Item::as_table_like_mut) else {
                    return false;
                };
                let hit = remove(sub, &parts[1..]);
                if sub.is_empty() {
                    t.remove(parts[0]);
                }
                hit
            }
            remove(t, &parts)
        }
    }
}
```

Implementation notes:
- `flatten`'s two match guards handle both `[defaults.glint]`-style tables and dotted keys; both arrive as table-like items. If the compiler flags the guard as redundant, simplify it to `Some(sub) => flatten(sub, &path, out)` for any item that `as_table_like()` accepts and is not an array. Keep the test green.
- `ArrayOfTables::retain` exists in toml_edit 0.25. If it doesn't in the locked version, rebuild the array without the matching tables.

- [ ] **Step 6: Wire the model (`crates/gui/src/model/mod.rs`).**
  - Add `pub sets_doc: Option<crate::sets_doc::SetsDoc>,` (initialised to `None` in `placeholder`).
  - Add a `DocKind` enum: `#[derive(Debug, Clone, Copy, PartialEq, Eq)] pub enum DocKind { SdcToml, Project }` with `pub fn file(self) -> &'static str { match self { DocKind::SdcToml => "sdc.toml", DocKind::Project => "sdc-project.toml" } }`.
  - Change `Dialog::ExternalChange` to `Dialog::ExternalChange(DocKind)`.
  - In `open_folder`, after the `ProjectDoc` lines:

    ```rust
    let sets_doc = crate::sets_doc::SetsDoc::open(&root);
    if let Some(e) = sets_doc.error() {
        // P8: a bad sdc-project.toml makes sets and JSON read-only; sdc.toml keeps working.
        let line = format!("{e} — sets and PBR JSON settings are read-only until it is fixed and reloaded");
        self.banner = Some(match self.banner.take() {
            Some(b) => format!("{b}\n{line}"),
            None => line,
        });
    }
    self.sets_doc = Some(sets_doc);
    ```

  - `is_dirty`: `self.project.as_ref().is_some_and(ProjectDoc::is_dirty) || self.sets_doc.as_ref().is_some_and(crate::sets_doc::SetsDoc::is_dirty)`.
  - `save`: save `sdc.toml` first, then `sdc-project.toml`. Map each error to a dialog that names its file: `ChangedOnDisk` → `Dialog::ExternalChange(DocKind::…)`, other errors → `Dialog::Error(format!("{}: {e}", kind.file()))`. Return the first error. A read-only `sdc-project.toml` is skipped (not an error), as its edits never happened. Same for `sdc.toml`, which today returns `ReadOnly`: keep that behaviour unchanged.
  - `resolve_external_change(which: DocKind, overwrite: bool)`:
    - for `SdcToml`, do what the old body did;
    - for `Project`, overwrite with `save(true)`, or reload `sets_doc` and `replan()`.

    Update every caller (`ui/dialogs.rs` and tests: `grep -rn "ExternalChange\|resolve_external_change" crates/gui`).
  - `resolve_open_unsaved(…, save=false)` and `CloseUnsaved` → Discard must also `reload()` `sets_doc`.

- [ ] **Step 7: In `crates/gui/src/ui/dialogs.rs`**, have the external-change dialog take `which` and print `"{file} changed on disk since it was loaded."`, with `file = which.file()`. Its buttons call `model.resolve_external_change(which, …)`.

- [ ] **Step 8: Add a model test** to `crates/gui/tests/sets_doc.rs`:

```rust
#[test]
fn the_model_opens_saves_and_reports_both_files() {
    use sdc_gui::model::{Dialog, DocKind};
    let d = tempfile::tempdir().unwrap();
    let mut m = support::opened(d.path());
    let root = m.root.clone().unwrap();
    assert!(!m.is_dirty());
    m.sets_doc.as_mut().unwrap().set_value("landscape/rock01", "hair", Some(json!(true)));
    assert!(m.is_dirty());
    m.save().unwrap();
    assert!(root.join("sdc-project.toml").exists());
    // An outside change to sdc-project.toml names that file.
    m.sets_doc.as_mut().unwrap().set_value("landscape/rock01", "hair", Some(json!(false)));
    std::thread::sleep(std::time::Duration::from_millis(20));
    std::fs::write(root.join("sdc-project.toml"), "# elsewhere\n").unwrap();
    assert!(m.save().is_err());
    assert_eq!(m.dialog, Some(Dialog::ExternalChange(DocKind::Project)));
}
```

Put `mod support;` at the top of `crates/gui/tests/sets_doc.rs` (first line), as the other test files do.

- [ ] **Step 9: Run the tests.**

Run: `cargo test -p sdc-gui`
Expected: all pass, including the M3a tests updated for `ExternalChange(DocKind::SdcToml)` and the file-neutral `SaveError` text.

- [ ] **Step 10: Run the gates and commit.**

```bash
cargo fmt --all && cargo clippy --workspace --all-targets -- -D warnings && cargo test -p sdc-gui
git add crates/gui/src/sets_doc.rs crates/gui/src/pgp/mod.rs crates/gui/src/pgp/settings.rs crates/gui/src/lib.rs crates/gui/src/project.rs crates/gui/src/model/mod.rs crates/gui/src/ui/dialogs.rs crates/gui/tests/sets_doc.rs
git commit -m "feat(gui): sdc-project.toml (sets, JSON settings, order) next to sdc.toml; dialogs name the file (M3b §7.1)

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

Add every other test file you updated for the `ExternalChange`/`SaveError` changes, by name.

---

### Task 5: Undo and redo (spec §7.2, DECISIONS P9)

**Files:**
- Create: `crates/gui/src/model/history.rs`
- Modify:
  - `crates/gui/src/model/mod.rs`: `pub mod history;`, a `history` field, `edit_docs`, and routing every existing edit through it (`set_field`, `clear_field`, `decide`, `set_target`, `set_fill`, `remove_unmatched`);
  - `crates/gui/src/app.rs`: Ctrl+Z / Ctrl+Y / Ctrl+Shift+Z keys, next to the existing Ctrl+S handling. Find it with `grep -n "Key::S" crates/gui/src`.
- Test: `crates/gui/tests/history.rs`

**Interfaces:**
- Consumes: `ProjectDoc::{text, restore}`, `SetsDoc::{text, restore}` (Task 4).
- Produces (registry): `Snapshot`, `Command`, `History`; `AppModel::{undo, redo, can_undo, can_redo, edit_docs}`; `AppModel.history: History`.

- [ ] **Step 1: Write the failing tests.** Create `crates/gui/tests/history.rs`:

```rust
mod support;
use sdc_gui::model::history::{Command, History, Snapshot};
use serde_json::json;
use support::*;

fn snap(a: &str, b: &str) -> Snapshot {
    Snapshot { sdc_toml: a.into(), project: b.into() }
}

fn cmd(label: &str, before: Snapshot, after: Snapshot, merge: Option<&str>) -> Command {
    Command { label: label.into(), before, after, merge: merge.map(str::to_string), created_file: None }
}

#[test]
fn history_undoes_redoes_merges_and_caps() {
    let mut h = History::default();
    h.push(cmd("a", snap("0", ""), snap("1", ""), None));
    h.push(cmd("slider", snap("1", ""), snap("2", ""), Some("x")));
    h.push(cmd("slider", snap("2", ""), snap("3", ""), Some("x")));
    assert_eq!(h.undo.len(), 2, "two steps with the same merge key are one");
    let c = h.undo_step().unwrap();
    assert_eq!((c.before.sdc_toml.as_str(), c.after.sdc_toml.as_str()), ("1", "3"));
    assert!(h.can_redo());
    h.push(cmd("b", snap("1", ""), snap("9", ""), None));
    assert!(!h.can_redo(), "a new edit drops the redo list");
    for i in 0..(History::LIMIT + 10) {
        h.push(cmd("n", snap(&i.to_string(), ""), snap("x", ""), None));
    }
    assert_eq!(h.undo.len(), History::LIMIT);
}

#[test]
fn every_kind_of_edit_undoes_and_redoes() {
    let d = tempfile::tempdir().unwrap();
    let mut m = opened(d.path());
    let item = m.items.iter().find(|i| i.name == "rock01.png").unwrap().key.clone();
    // A [files] edit (sdc.toml).
    m.set_field(&item, "mips.levels", json!(3));
    assert_eq!(m.project.as_ref().unwrap().file_value(&item, "mips.levels"), Some(json!(3)));
    m.undo();
    assert_eq!(m.project.as_ref().unwrap().file_value(&item, "mips.levels"), None);
    m.redo();
    assert_eq!(m.project.as_ref().unwrap().file_value(&item, "mips.levels"), Some(json!(3)));
    // Mode.
    m.set_target(engine::config::Target::Vanilla);
    assert_eq!(m.project.as_ref().unwrap().target(), Some(engine::config::Target::Vanilla));
    m.undo();
    assert_eq!(m.project.as_ref().unwrap().target(), None);
    // A sets-doc edit.
    m.edit_docs("hair", None, false, |_, s| s.set_value("landscape/rock01", "hair", Some(json!(true))));
    assert!(m.sets_doc.as_ref().unwrap().entries().iter().any(|e| e.settings.contains_key("hair")));
    m.undo();
    assert!(!m.sets_doc.as_ref().unwrap().entries().iter().any(|e| e.settings.contains_key("hair")));
    assert!(m.is_dirty(), "undo back past the save point stays an unsaved change");
}

#[test]
fn undo_after_an_outside_edit_never_overwrites_silently() {
    use sdc_gui::model::{Dialog, DocKind};
    let d = tempfile::tempdir().unwrap();
    let mut m = opened(d.path());
    let root = m.root.clone().unwrap();
    m.edit_docs("hair", None, false, |_, s| s.set_value("landscape/rock01", "hair", Some(json!(true))));
    m.save().unwrap();
    std::thread::sleep(std::time::Duration::from_millis(20));
    std::fs::write(root.join("sdc-project.toml"), "# edited elsewhere\n").unwrap();
    m.undo();
    assert!(m.save().is_err());
    assert_eq!(m.dialog, Some(Dialog::ExternalChange(DocKind::Project)), "Review focus 4");
    assert_eq!(std::fs::read_to_string(root.join("sdc-project.toml")).unwrap(), "# edited elsewhere\n");
}

#[test]
fn opening_another_folder_clears_the_history() {
    let d = tempfile::tempdir().unwrap();
    let mut m = opened(d.path());
    m.set_target(engine::config::Target::Vanilla);
    assert!(m.can_undo());
    m.save().unwrap();
    m.open_folder(mod_folder(&d.path().join("b")));
    assert!(!m.can_undo() && !m.can_redo());
}

#[test]
fn an_edit_that_changes_nothing_is_not_a_step() {
    let d = tempfile::tempdir().unwrap();
    let mut m = opened(d.path());
    m.edit_docs("noop", None, false, |_, _| {});
    assert!(!m.can_undo());
}
```

- [ ] **Step 2: Run the tests and confirm they fail.**

Run: `cargo test -p sdc-gui --test history`
Expected: compile error (no `history` module).

- [ ] **Step 3: Create `crates/gui/src/model/history.rs`:**

```rust
//! Undo and redo over every edit (M3b spec §7.2, DECISIONS P9): a command keeps the text of `sdc.toml` and
//! `sdc-project.toml` before and after. Opening another folder clears it; Export and Save are not steps.
use std::path::PathBuf;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Snapshot {
    pub sdc_toml: String,
    pub project: String,
}

/// A file a command created (Duplicate source, spec §3.4): its path, its source, and its SHA-256 when created.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CreatedFile {
    pub path: PathBuf,
    pub source: PathBuf,
    pub sha256: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Command {
    pub label: String,
    pub before: Snapshot,
    pub after: Snapshot,
    /// Consecutive commands with the same `Some` key are one step (a slider drag, typing in one field).
    pub merge: Option<String>,
    pub created_file: Option<CreatedFile>,
}

#[derive(Debug, Default)]
pub struct History {
    pub undo: Vec<Command>,
    pub redo: Vec<Command>,
}

impl History {
    pub const LIMIT: usize = 200;

    pub fn push(&mut self, c: Command) {
        self.redo.clear();
        if let (Some(last), Some(k)) = (self.undo.last_mut(), c.merge.as_ref()) {
            if last.merge.as_ref() == Some(k) && last.created_file.is_none() && c.created_file.is_none() {
                last.after = c.after;
                return;
            }
        }
        self.undo.push(c);
        if self.undo.len() > Self::LIMIT {
            self.undo.remove(0);
        }
    }

    /// The command to undo (moved to the redo list).
    pub fn undo_step(&mut self) -> Option<Command> {
        let c = self.undo.pop()?;
        self.redo.push(c.clone());
        Some(c)
    }

    /// The command to redo (moved back to the undo list).
    pub fn redo_step(&mut self) -> Option<Command> {
        let c = self.redo.pop()?;
        self.undo.push(c.clone());
        Some(c)
    }

    /// Put back a step whose undo or redo was refused.
    pub fn refuse_undo(&mut self) {
        if let Some(c) = self.redo.pop() {
            self.undo.push(c);
        }
    }
    pub fn refuse_redo(&mut self) {
        if let Some(c) = self.undo.pop() {
            self.redo.push(c);
        }
    }

    pub fn clear(&mut self) {
        self.undo.clear();
        self.redo.clear();
    }
    pub fn can_undo(&self) -> bool {
        !self.undo.is_empty()
    }
    pub fn can_redo(&self) -> bool {
        !self.redo.is_empty()
    }
}
```

- [ ] **Step 4: Change the model to use it.** In `crates/gui/src/model/mod.rs`:
  - add `pub mod history;` and the field `pub history: history::History` (`Default` in `placeholder`);
  - in `open_folder`, call `self.history.clear();`.

Replace the private `edit` with:

```rust
    fn snapshot(&self) -> history::Snapshot {
        history::Snapshot {
            sdc_toml: self.project.as_ref().map(ProjectDoc::text).unwrap_or_default(),
            project: self.sets_doc.as_ref().map(crate::sets_doc::SetsDoc::text).unwrap_or_default(),
        }
    }

    /// Every edit goes through here (spec §7.2): run `f` on both documents, record one undo step when anything
    /// changed, and plan again when `replan` (DECISIONS P7: JSON-only edits do not).
    pub fn edit_docs(
        &mut self,
        label: &str,
        merge: Option<String>,
        replan: bool,
        f: impl FnOnce(&mut ProjectDoc, &mut crate::sets_doc::SetsDoc),
    ) {
        let (Some(p), Some(s)) = (self.project.as_mut(), self.sets_doc.as_mut()) else {
            return;
        };
        let before = history::Snapshot { sdc_toml: p.text(), project: s.text() };
        f(p, s);
        let after = history::Snapshot { sdc_toml: p.text(), project: s.text() };
        if after == before {
            return;
        }
        self.history.push(history::Command { label: label.into(), before, after, merge, created_file: None });
        if replan {
            self.replan();
        }
    }

    /// The old `edit`: an `sdc.toml` edit (read-only `sdc.toml` ignores it, as before), re-planned.
    fn edit(&mut self, label: &str, f: impl FnOnce(&mut ProjectDoc)) {
        if self.project.as_ref().is_none_or(ProjectDoc::read_only) {
            return;
        }
        self.edit_docs(label, None, true, |p, _| f(p));
    }

    fn apply_snapshot(&mut self, s: &history::Snapshot) {
        if let Some(p) = self.project.as_mut() {
            p.restore(&s.sdc_toml);
        }
        if let Some(d) = self.sets_doc.as_mut() {
            d.restore(&s.project);
        }
        self.replan();
    }

    pub fn can_undo(&self) -> bool {
        self.history.can_undo()
    }
    pub fn can_redo(&self) -> bool {
        self.history.can_redo()
    }

    /// Undo one step. A step that created a file (Duplicate source) deletes it first, and only if it is unchanged
    /// (spec §3.4); otherwise the undo is refused with the reason.
    pub fn undo(&mut self) {
        let Some(c) = self.history.undo_step() else { return };
        if let Some(cf) = &c.created_file {
            if let Err(why) = remove_created(cf) {
                self.history.refuse_undo();
                self.dialog = Some(Dialog::Error(why));
                return;
            }
        }
        self.apply_snapshot(&c.before);
    }

    /// Redo one step. A step that created a file copies it again from its source.
    pub fn redo(&mut self) {
        let Some(c) = self.history.redo_step() else { return };
        if let Some(cf) = &c.created_file {
            if let Err(e) = std::fs::copy(&cf.source, &cf.path) {
                self.history.refuse_redo();
                self.dialog = Some(Dialog::Error(format!("could not copy {} again: {e}", cf.source.display())));
                return;
            }
        }
        self.apply_snapshot(&c.after);
    }
```

Add this free function in the same file:

```rust
/// Delete a file a command created, if it still has the SHA-256 it was created with.
fn remove_created(cf: &history::CreatedFile) -> Result<(), String> {
    match crate::model::sets::sha256_of(&cf.path) {
        None => Ok(()), // already gone
        Some(h) if h == cf.sha256 => {
            std::fs::remove_file(&cf.path).map_err(|e| format!("could not delete {}: {e}", cf.path.display()))
        }
        Some(_) => Err(format!(
            "{} has changed since it was duplicated, so undo keeps it. Delete it yourself if you no longer need it.",
            cf.path.display()
        )),
    }
}
```

`sets::sha256_of` arrives in Task 10. **For this task**, define it temporarily in `history.rs`:

```rust
pub fn sha256_of(p: &std::path::Path) -> Option<String> {
    use sha2::{Digest, Sha256};
    Some(hex::encode(Sha256::digest(std::fs::read(p).ok()?)))
}
```

Reference it as `history::sha256_of`. Add `sha2.workspace = true` and `hex.workspace = true` to `crates/gui/Cargo.toml` `[dependencies]`; both are already in `Cargo.lock`, so nothing is downloaded. Task 10 calls the same function: it stays in `history.rs`, and the registry name `sets::sha256_of` becomes `history::sha256_of`. The Task 10 text uses that path.

Route the existing edits through `edit` with labels:
- `set_field` → `"Change {field}"`;
- `clear_field` → `"Reset {field}"`;
- `decide` → `"Decide"`;
- `set_target` → `"Mode"`;
- `set_fill` → `"Fill value"`;
- `remove_unmatched` → `"Remove entry"`.

The call shape is `self.edit(&format!("Change {f}"), move |p| p.set_file_value(&k, &f, &value))`.

- [ ] **Step 5: Wire the keys in `crates/gui/src/app.rs`.** Next to the existing Ctrl+S handling, use the same input API the file already uses:
  - Ctrl+Z → `model.undo()`;
  - Ctrl+Y or Ctrl+Shift+Z → `model.redo()`.
  - **Ignore the keys while `model.dialog.is_some()`** (the M3a carry noted that Ctrl+S already has this problem; don't repeat it).
  - **Ignore them while a text field has focus** (`ctx.wants_keyboard_input()`), so typing keeps its own undo.

- [ ] **Step 6: Run the tests.**

Run: `cargo test -p sdc-gui`
Expected: all pass.

- [ ] **Step 7: Run the gates and commit.**

```bash
cargo fmt --all && cargo clippy --workspace --all-targets -- -D warnings && cargo test -p sdc-gui
git add crates/gui/src/model/history.rs crates/gui/src/model/mod.rs crates/gui/src/app.rs crates/gui/Cargo.toml Cargo.lock crates/gui/tests/history.rs
git commit -m "feat(gui): one undo/redo history over every edit, snapshot-based (M3b §7.2, P9)

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

---
### Task 6: The PGPatcher key table and settings rules (spec §5.1–5.4)

**Files:**
- Create: `crates/gui/assets/pbr.schema.json` (a byte-for-byte copy of `F:/ai/dds/gui reference/ref/pbr.schema.json`), `crates/gui/src/pgp/schema.rs`
- Modify: `crates/gui/src/pgp/mod.rs`, `crates/gui/src/pgp/settings.rs` (from Task 4), `THIRD_PARTY_NOTICES.md`
- Test: `crates/gui/tests/pgp.rs`

**Interfaces:**
- Consumes: `pgp::settings::Settings` (Task 4).
- Produces (registry): `SCHEMA_JSON`, `SCHEMA_URL`, `KeyKind`, `Group`, `KeySpec`, `KEYS`, `REMOVED`, `NOT_EXPOSED`, `spec`, `Feature`, `feature_on`, `blocked_by`, `effective`, `GLINT_DEFAULTS`, `FEATURE_KEYS`.

**Write rules this table encodes** (spec §5.2, owner decisions in the design conversation):

| Kind | Keys | Written |
|---|---|---|
| `AlwaysBool` | `emissive`, `parallax`, `subsurface` | Always, as the effective `true`/`false` (absent means `false`) |
| `OnlyTrue` | `hair`, `multilayer`, `subsurface_foliage`, `coat_diffuse`, `coat_normal`, `coat_parallax`, `fuzz.texture` | Only when `true` |
| `Object` | `glint`, `fuzz` | The object, only when the feature is on (`Settings["glint"] == true`) |
| `Tri` | every `lock_*`, `vertex_colors`, `zbuffer_write` | Only when set, as `true` or `false` |
| `Number`, `Text`, `Rgb`, `Rgba` | the rest | Only when set |
| `Slot` | `slot1`–`slot8` | From slot commands and borrowed slots (Task 8) |

Not written at all:
- `REMOVED`: `pbr`, `env_mapping`, `env_map_scale`, `env_map_scale_mult`;
- `NOT_EXPOSED`: `delete` (spec §9) and `texture` (the old name of `match_diffuse`).

- [ ] **Step 1: Write the failing tests.** Create `crates/gui/tests/pgp.rs`:

```rust
use sdc_gui::pgp::schema::*;
use sdc_gui::pgp::settings::*;
use serde_json::{json, Value};

fn entry_properties() -> Vec<String> {
    let s: Value = serde_json::from_str(SCHEMA_JSON).unwrap();
    s["definitions"]["entry"]["properties"].as_object().unwrap().keys().cloned().collect()
}

#[test]
fn every_schema_key_is_handled_exactly_once() {
    let mut handled: Vec<String> = KEYS.iter().filter(|k| !k.key.contains('.')).map(|k| k.key.to_string()).collect();
    handled.extend(REMOVED.iter().map(|s| s.to_string()));
    handled.extend(NOT_EXPOSED.iter().map(|s| s.to_string()));
    let mut props = entry_properties();
    props.sort();
    handled.sort();
    assert_eq!(handled, props, "a schema key without a rule (or a rule for a key the schema lacks)");
    // Nested keys of glint and fuzz.
    let s: Value = serde_json::from_str(SCHEMA_JSON).unwrap();
    for obj in ["glint", "fuzz"] {
        for k in s["definitions"]["entry"]["properties"][obj]["properties"].as_object().unwrap().keys() {
            assert!(spec(&format!("{obj}.{k}")).is_some(), "{obj}.{k}");
        }
    }
}

#[test]
fn keys_follow_schema_order() {
    let props = entry_properties();
    let order: Vec<usize> = KEYS
        .iter()
        .filter(|k| !k.key.contains('.'))
        .map(|k| props.iter().position(|p| p == k.key).unwrap())
        .collect();
    assert!(order.windows(2).all(|w| w[0] < w[1]), "KEYS must list keys in schema order: {order:?}");
}

#[test]
fn kinds_match_the_owner_rules() {
    for k in ["emissive", "parallax", "subsurface"] {
        assert_eq!(spec(k).unwrap().kind, KeyKind::AlwaysBool, "{k}");
    }
    for k in ["hair", "multilayer"] {
        assert_eq!(spec(k).unwrap().kind, KeyKind::OnlyTrue, "{k}");
    }
    for k in ["lock_diffuse", "lock_cnr", "vertex_colors", "zbuffer_write"] {
        assert_eq!(spec(k).unwrap().kind, KeyKind::Tri, "{k}");
    }
    assert_eq!(spec("emissive_color").unwrap().kind, KeyKind::Rgba);
    assert!(spec("delete").is_none() && spec("pbr").is_none());
}

fn s(pairs: &[(&str, Value)]) -> Settings {
    pairs.iter().map(|(k, v)| (k.to_string(), v.clone())).collect()
}

#[test]
fn mutual_exclusion_follows_the_owner_table() {
    use Feature::*;
    let hair = s(&[("hair", json!(true))]);
    for f in [Multilayer, Glint, Fuzz, Emissive, Parallax, Subsurface] {
        assert_eq!(blocked_by(&hair, f), Some(Hair), "{f:?}");
    }
    let ml = s(&[("multilayer", json!(true))]);
    for f in [Subsurface, Glint, Fuzz, Hair] {
        assert_eq!(blocked_by(&ml, f), Some(Multilayer), "{f:?}");
    }
    assert_eq!(blocked_by(&ml, Emissive), None);
    assert_eq!(blocked_by(&ml, Parallax), None);
    let glint = s(&[("glint", json!(true))]);
    assert_eq!(blocked_by(&glint, Fuzz), Some(Glint));
    assert_eq!(blocked_by(&glint, Multilayer), Some(Glint));
    let ss = s(&[("subsurface", json!(true))]);
    assert_eq!(blocked_by(&ss, Multilayer), Some(Subsurface));
    assert_eq!(blocked_by(&ss, Emissive), None);
    // A feature never blocks itself, and off features block nothing.
    assert_eq!(blocked_by(&hair, Hair), None);
    assert_eq!(blocked_by(&s(&[("hair", json!(false))]), Emissive), None);
}

#[test]
fn effective_overlays_own_values_on_defaults() {
    let d = s(&[("specular_level", json!(0.04)), ("parallax", json!(true))]);
    let own = s(&[("parallax", json!(false))]);
    let e = effective(&d, &own);
    assert_eq!(e["specular_level"], json!(0.04));
    assert_eq!(e["parallax"], json!(false));
}
```

- [ ] **Step 2: Run the tests and confirm they fail.**

Run: `cargo test -p sdc-gui --test pgp`
Expected: compile errors (no `pgp::schema` module, no `Feature`).

- [ ] **Step 3: Vendor the schema.**

```bash
mkdir -p crates/gui/assets
cp "F:/ai/dds/gui reference/ref/pbr.schema.json" crates/gui/assets/pbr.schema.json
```

Add this line to `THIRD_PARTY_NOTICES.md`, under the GUI section:

> `crates/gui/assets/pbr.schema.json` — PGPatcher PBR JSON schema, © hakasapl and contributors, GPL-3.0 (https://github.com/hakasapl/PGPatcher). Vendored unmodified, fetched from `main` on 2026-10-09; used to validate exported JSON.

- [ ] **Step 4: Create `crates/gui/src/pgp/schema.rs`:**

```rust
//! The PGPatcher PBR schema keys and how the GUI treats each (M3b spec §5.1–5.2). `KEYS` lists the entry keys in
//! schema order (the writer writes in this order), then the nested `glint.*` and `fuzz.*` keys.
pub const SCHEMA_JSON: &str = include_str!("../../assets/pbr.schema.json");
pub const SCHEMA_URL: &str = "https://raw.githubusercontent.com/hakasapl/PGPatcher/main/schema/pbr.schema.json";

/// Never written: removed by PGPatcher 2.0.0.
pub const REMOVED: &[&str] = &["pbr", "env_mapping", "env_map_scale", "env_map_scale_mult"];
/// Never written: `delete` is a later round (spec §9); `texture` is the old name of `match_diffuse`.
pub const NOT_EXPOSED: &[&str] = &["texture", "delete"];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KeyKind {
    /// On/off, always written (absent = false).
    AlwaysBool,
    /// On/off, written only when on.
    OnlyTrue,
    /// Not set / on / off; written only when set.
    Tri,
    Number,
    Text,
    /// 0–255 RGB in the GUI; written as 0–1.
    Rgb,
    /// As `Rgb`, written with a fourth component 1.0.
    Rgba,
    /// A slot command (`slotN`).
    Slot,
    /// `glint` / `fuzz`: on/off; when on, the object with its `<key>.*` values is written.
    Object,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Group {
    Match,
    Material,
    Features,
    Emissive,
    Parallax,
    Subsurface,
    Multilayer,
    Glint,
    Fuzz,
    Vertex,
    Locks,
    Uv,
    Slots,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct KeySpec {
    pub key: &'static str,
    pub kind: KeyKind,
    pub group: Group,
    /// Shown in the Advanced drawer rather than the main view.
    pub advanced: bool,
    pub label: &'static str,
}

const fn k(key: &'static str, kind: KeyKind, group: Group, advanced: bool, label: &'static str) -> KeySpec {
    KeySpec { key, kind, group, advanced, label }
}

use Group as G;
use KeyKind as K;

pub const KEYS: &[KeySpec] = &[
    k("match_diffuse", K::Text, G::Match, false, "Match diffuse"),
    k("match_normal", K::Text, G::Match, true, "Match normal"),
    k("path_contains", K::Text, G::Match, true, "Path contains"),
    k("match1", K::Text, G::Match, true, "Match slot 1 (diffuse)"),
    k("match2", K::Text, G::Match, true, "Match slot 2 (normal)"),
    k("match3", K::Text, G::Match, true, "Match slot 3 (glow)"),
    k("match4", K::Text, G::Match, true, "Match slot 4 (parallax)"),
    k("match5", K::Text, G::Match, true, "Match slot 5 (cubemap)"),
    k("match6", K::Text, G::Match, true, "Match slot 6 (env mask)"),
    k("match7", K::Text, G::Match, true, "Match slot 7 (multilayer)"),
    k("match8", K::Text, G::Match, true, "Match slot 8 (backlight)"),
    k("nif_filter", K::Text, G::Match, true, "Mesh path contains"),
    k("rename", K::Text, G::Match, false, "Rename"),
    k("smooth_angle", K::Number, G::Uv, true, "Smooth angle"),
    k("auto_uv", K::Number, G::Uv, true, "Auto UV"),
    k("uv_scale", K::Number, G::Uv, true, "UV scale"),
    k("vertex_colors", K::Tri, G::Vertex, true, "Vertex colours"),
    k("vertex_color_lum_mult", K::Number, G::Vertex, true, "Vertex colour luminance ×"),
    k("vertex_color_sat_mult", K::Number, G::Vertex, true, "Vertex colour saturation ×"),
    k("zbuffer_write", K::Tri, G::Vertex, true, "Z-buffer write"),
    k("lock_diffuse", K::Tri, G::Locks, true, "Lock diffuse"),
    k("lock_normal", K::Tri, G::Locks, true, "Lock normal"),
    k("emissive", K::AlwaysBool, G::Features, false, "Emissive"),
    k("lock_emissive", K::Tri, G::Locks, true, "Lock emissive"),
    k("emissive_scale", K::Number, G::Emissive, false, "Emissive scale"),
    k("emissive_color", K::Rgba, G::Emissive, false, "Emissive colour"),
    k("parallax", K::AlwaysBool, G::Features, false, "Parallax"),
    k("lock_parallax", K::Tri, G::Locks, true, "Lock parallax"),
    k("displacement_scale", K::Number, G::Parallax, false, "Displacement scale"),
    k("cubemap", K::Text, G::Uv, true, "Cubemap path"),
    k("lock_cubemap", K::Tri, G::Locks, true, "Lock cubemap"),
    k("lock_rmaos", K::Tri, G::Locks, true, "Lock RMAOS"),
    k("specular_level", K::Number, G::Material, false, "Specular level"),
    k("roughness_scale", K::Number, G::Material, false, "Roughness scale"),
    k("subsurface", K::AlwaysBool, G::Features, false, "Subsurface"),
    k("subsurface_foliage", K::OnlyTrue, G::Subsurface, false, "Foliage subsurface"),
    k("subsurface_color", K::Rgb, G::Subsurface, false, "Subsurface colour"),
    k("subsurface_opacity", K::Number, G::Subsurface, false, "Subsurface opacity"),
    k("lock_subsurface", K::Tri, G::Locks, true, "Lock subsurface"),
    k("hair", K::OnlyTrue, G::Features, false, "Hair"),
    k("multilayer", K::OnlyTrue, G::Features, false, "Multilayer (coat)"),
    k("coat_color", K::Rgb, G::Multilayer, true, "Coat colour"),
    k("coat_specular_level", K::Number, G::Multilayer, false, "Coat specular level"),
    k("coat_roughness", K::Number, G::Multilayer, false, "Coat roughness"),
    k("coat_strength", K::Number, G::Multilayer, false, "Coat strength"),
    k("coat_diffuse", K::OnlyTrue, G::Multilayer, false, "Coat diffuse"),
    k("coat_normal", K::OnlyTrue, G::Multilayer, false, "Coat normal (_cnr)"),
    k("coat_parallax", K::OnlyTrue, G::Multilayer, false, "Coat parallax"),
    k("lock_cnr", K::Tri, G::Locks, true, "Lock coat normal"),
    k("inner_uv_scale", K::Number, G::Multilayer, true, "Inner UV scale"),
    k("glint", K::Object, G::Features, false, "Glint"),
    k("fuzz", K::Object, G::Features, false, "Fuzz"),
    k("slot1", K::Slot, G::Slots, false, "Slot 1"),
    k("slot2", K::Slot, G::Slots, false, "Slot 2"),
    k("slot3", K::Slot, G::Slots, false, "Slot 3"),
    k("slot4", K::Slot, G::Slots, false, "Slot 4"),
    k("slot5", K::Slot, G::Slots, false, "Slot 5"),
    k("slot6", K::Slot, G::Slots, false, "Slot 6"),
    k("slot7", K::Slot, G::Slots, false, "Slot 7"),
    k("slot8", K::Slot, G::Slots, false, "Slot 8"),
    k("glint.screen_space_scale", K::Number, G::Glint, false, "Screen-space scale"),
    k("glint.log_microfacet_density", K::Number, G::Glint, false, "Log microfacet density"),
    k("glint.microfacet_roughness", K::Number, G::Glint, false, "Microfacet roughness"),
    k("glint.density_randomization", K::Number, G::Glint, false, "Density randomisation"),
    k("fuzz.texture", K::OnlyTrue, G::Fuzz, false, "Fuzz texture (_f)"),
    k("fuzz.color", K::Rgb, G::Fuzz, false, "Fuzz colour"),
    k("fuzz.weight", K::Number, G::Fuzz, false, "Fuzz weight"),
];

pub fn spec(key: &str) -> Option<&'static KeySpec> {
    KEYS.iter().find(|k| k.key == key)
}
```

**Check the schema order before trusting this list.** `keys_follow_schema_order` fails if the vendored schema orders a key differently. If it does, move the line in `KEYS` to match; don't change the test.

- [ ] **Step 5: Fill in `crates/gui/src/pgp/settings.rs`:**

```rust
//! A set's (or the defaults') JSON settings, by schema key path (`glint.screen_space_scale`). Colours are 0–255
//! integer arrays here; the writer converts them (DECISIONS P10). Features and their mutual exclusion (spec §5.4).
use serde_json::Value;

pub type Settings = std::collections::BTreeMap<String, Value>;

/// Glint values a freshly enabled glint starts from (spec §4.3; TruePBR-Manager's widget values, which it then
/// overwrote with zeros).
pub const GLINT_DEFAULTS: [(&str, f64); 4] = [
    ("glint.screen_space_scale", 1.5),
    ("glint.log_microfacet_density", 40.0),
    ("glint.microfacet_roughness", 0.015),
    ("glint.density_randomization", 2.0),
];

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Feature {
    Emissive,
    Parallax,
    Subsurface,
    Hair,
    Multilayer,
    Glint,
    Fuzz,
}

/// The settings key that turns each feature on.
pub const FEATURE_KEYS: [(Feature, &str); 7] = [
    (Feature::Emissive, "emissive"),
    (Feature::Parallax, "parallax"),
    (Feature::Subsurface, "subsurface"),
    (Feature::Hair, "hair"),
    (Feature::Multilayer, "multilayer"),
    (Feature::Glint, "glint"),
    (Feature::Fuzz, "fuzz"),
];

pub fn feature_key(f: Feature) -> &'static str {
    FEATURE_KEYS.iter().find(|(g, _)| *g == f).map(|(_, k)| *k).expect("every feature has a key")
}

pub fn feature_on(s: &Settings, f: Feature) -> bool {
    s.get(feature_key(f)).and_then(Value::as_bool).unwrap_or(false)
}

/// Pairs that cannot both be on (owner, 2026-10-10). The relation is symmetric.
const EXCLUSIVE: [(Feature, Feature); 10] = {
    use Feature::*;
    [
        (Hair, Multilayer),
        (Hair, Glint),
        (Hair, Fuzz),
        (Hair, Emissive),
        (Hair, Parallax),
        (Hair, Subsurface),
        (Multilayer, Subsurface),
        (Multilayer, Glint),
        (Multilayer, Fuzz),
        (Glint, Fuzz),
    ]
};

/// The feature, currently on, that rules `f` out; `None` when `f` may be turned on. The control for `f` is greyed
/// with "off: <feature> is on" while this is `Some`.
pub fn blocked_by(s: &Settings, f: Feature) -> Option<Feature> {
    EXCLUSIVE.iter().find_map(|&(a, b)| {
        if a == f && feature_on(s, b) {
            Some(b)
        } else if b == f && feature_on(s, a) {
            Some(a)
        } else {
            None
        }
    })
}

/// `own` over `defaults` (the JSON `default` block applies to every entry that does not set the key).
pub fn effective(defaults: &Settings, own: &Settings) -> Settings {
    let mut out = defaults.clone();
    out.extend(own.iter().map(|(k, v)| (k.clone(), v.clone())));
    out
}
```

Update `pgp/mod.rs` to `pub mod schema; pub mod settings;`.

- [ ] **Step 6: Run the tests.**

Run: `cargo test -p sdc-gui --test pgp`
Expected: all pass. If `keys_follow_schema_order` fails, reorder `KEYS` as Step 4 says.

- [ ] **Step 7: Run the gates and commit.**

```bash
cargo fmt --all && cargo clippy --workspace --all-targets -- -D warnings && cargo test -p sdc-gui
git add crates/gui/assets/pbr.schema.json crates/gui/src/pgp/ THIRD_PARTY_NOTICES.md crates/gui/tests/pgp.rs
git commit -m "feat(gui): PGPatcher key table, write kinds and feature exclusion; vendor pbr.schema.json (M3b §5.1–5.4)

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

---

### Task 7: Match modes and output prefixes (spec §5.5)

**Files:**
- Create: `crates/gui/src/pgp/prefix.rs`
- Modify: `crates/gui/src/pgp/mod.rs` (`pub mod prefix;`)
- Test: `crates/gui/tests/pgp.rs` (append)

**Interfaces:**
- Consumes: `Settings`.
- Produces (registry):
  - `MatchMode` and `match_mode(&Settings) -> MatchMode`;
  - `derive_match_diffuse(rel_dir: &str, stem: &str) -> String`;
  - `prefix_of(s: &Settings, derived: &str) -> Result<String, PrefixError>`;
  - `pitfalls(s: &Settings) -> Vec<String>`;
  - `check_rel_path(p: &str, field: &str) -> Result<(), PrefixError>`;
  - `PrefixError(String)`;
  - `slot_suffixes(n: u8) -> &'static [&'static str]`;
  - `output_rel(prefix: &str, suffix: &str) -> PathBuf`.

**Rules** (gap review, read from PGPatcher `main` on 2026-10-09; owner decisions of 2026-10-09):
- **Paths are written with backslashes.** Comparisons ignore case, and a leading `textures\` on a value is optional.
- **`Diffuse` mode:** `match_diffuse` if set, else the derived value.
  - The prefix is `textures\pbr\` plus the value, without any extension.
  - If the value already starts with `pbr\`, it isn't doubled.
  - `rename` replaces the matched part: the prefix is `textures\pbr\` plus `rename`.
- **`Normal` mode:** as Diffuse, with the value's slot-2 suffix removed using the `match2` list (`_msn`, `_n`).
  - [Assessment] PGPatcher documents `match_normal` with `match_diffuse`. Stripping the normal suffix is the reading that gives the same prefix as the matched diffuse. The owner's in-game exit check confirms it.
- **`PathContains` mode:** the prefix comes from the derived diffuse path. `rename` works as in Diffuse.
- **`Slot(n)` mode:**
  1. Strip the extension.
  2. Strip the **first** suffix of slot `n`'s list that the value ends with (lists below, checked in this order).
  3. Make it `textures\pbr\…`.
  4. `rename`, if set, is the **whole** prefix, used exactly as written.
- **Suffix lists:**
  - 1: `_d`, `mask`;
  - 2: `_msn`, `_n`;
  - 3: `_g`, `_sk`;
  - 4: `_p`;
  - 5: `_e`;
  - 6: `_em`, `_envmask`, `_m`, `_rmaos`;
  - 7: `_cnr`, `_f`, `_i`, `_s`;
  - 8: `_b`, `_bl`, `_flow`.
- **Blocked (`PrefixError`; spec §6.3 #4, Review focus 3):**
  - an empty value;
  - `..` as a component;
  - a drive letter (`X:`);
  - a leading `\` or `/`;
  - characters Windows doesn't allow in names: `<>:"|?*` and control characters.

  **Ruling P13** (record in DECISIONS Note P): invalid characters **block export**. The spec's "sanitise" is not applied silently, because a changed path would no longer be the one PGPatcher looks for.
- **Pitfalls** (warnings; Export asks Proceed anyway / Cancel):
  - `match8` whose value ends in `_s` (`match8 does not strip _s; use match7 for a subsurface map`);
  - `rename` in `Slot` mode that doesn't start with `textures\` (`rename on a matchN entry must be a full path such as textures\pbr\…`).

- [ ] **Step 1: Write the failing tests.** Append to `crates/gui/tests/pgp.rs`:

```rust
use sdc_gui::pgp::prefix::*;

#[test]
fn derived_match_drops_textures_pbr_and_the_extension() {
    assert_eq!(derive_match_diffuse("landscape", "rock01"), r"landscape\rock01");
    assert_eq!(derive_match_diffuse("textures/landscape", "rock01"), r"landscape\rock01");
    assert_eq!(derive_match_diffuse("Textures/PBR/Landscape", "Rock01"), r"Landscape\Rock01");
    assert_eq!(derive_match_diffuse("", "rock01"), "rock01");
}

#[test]
fn diffuse_mode_prefixes_and_renames() {
    let p = |pairs: &[(&str, Value)], derived: &str| prefix_of(&s(pairs), derived).unwrap();
    assert_eq!(p(&[], r"landscape\rock01"), r"textures\pbr\landscape\rock01");
    assert_eq!(p(&[("match_diffuse", json!(r"textures\landscape\rock02.dds"))], "x"), r"textures\pbr\landscape\rock02");
    assert_eq!(p(&[("match_diffuse", json!(r"pbr\landscape\rock02"))], "x"), r"textures\pbr\landscape\rock02", "never doubled");
    assert_eq!(p(&[("rename", json!(r"landscape\boulder"))], r"landscape\rock01"), r"textures\pbr\landscape\boulder");
    assert_eq!(p(&[("match_normal", json!(r"landscape\rock01_n.dds"))], "x"), r"textures\pbr\landscape\rock01");
    assert_eq!(p(&[("path_contains", json!("rock"))], r"landscape\rock01"), r"textures\pbr\landscape\rock01");
}

#[test]
fn slot_modes_strip_the_first_suffix_of_their_own_list() {
    let p = |key: &str, v: &str| prefix_of(&s(&[(key, json!(v))]), "unused").unwrap();
    assert_eq!(p("match2", r"architecture\whiterun\wrplank01_n.dds"), r"textures\pbr\architecture\whiterun\wrplank01");
    assert_eq!(p("match1", r"actors\facemask.dds"), r"textures\pbr\actors\face", "match1 strips a bare 'mask'");
    assert_eq!(p("match1", r"x\rock_n.dds"), r"textures\pbr\x\rock_n", "a suffix on the wrong slot stays");
    assert_eq!(p("match8", r"x\leaf_s.dds"), r"textures\pbr\x\leaf_s", "match8 does not strip _s");
    assert_eq!(p("match7", r"x\leaf_s.dds"), r"textures\pbr\x\leaf");
    assert_eq!(p("match6", r"x\iron_m.dds"), r"textures\pbr\x\iron");
    // rename replaces the whole prefix in slot modes.
    let r = prefix_of(&s(&[("match2", json!(r"a\b_n.dds")), ("rename", json!(r"textures\pbr\c\d"))]), "x").unwrap();
    assert_eq!(r, r"textures\pbr\c\d");
}

#[test]
fn bad_paths_block() {
    for bad in [r"..\up", r"C:\abs", r"\lead", "a|b", r"a\..\b"] {
        assert!(prefix_of(&s(&[("match_diffuse", json!(bad))]), "x").is_err(), "{bad:?}");
        assert!(prefix_of(&s(&[("rename", json!(bad))]), "x").is_err(), "{bad:?}");
        assert!(check_rel_path(bad, "slot3").is_err(), "{bad:?}");
    }
    assert!(check_rel_path("", "slot3").is_err());
    // An empty match_diffuse falls back to the derived match; an empty derived match blocks.
    assert!(prefix_of(&s(&[]), "").is_err());
    assert!(check_rel_path(r"landscape\white.dds", "slot3").is_ok());
}

#[test]
fn pitfalls_are_reported_not_fixed() {
    let w = pitfalls(&s(&[("match8", json!(r"x\leaf_s.dds"))]));
    assert!(w.iter().any(|m| m.contains("match8")), "{w:?}");
    let w = pitfalls(&s(&[("match2", json!(r"a\b_n.dds")), ("rename", json!(r"c\d"))]));
    assert!(w.iter().any(|m| m.contains("full path")), "{w:?}");
    assert!(pitfalls(&s(&[("match_diffuse", json!("a"))])).is_empty());
}

#[test]
fn output_paths_use_the_canonical_suffix() {
    assert_eq!(output_rel(r"textures\pbr\landscape\rock01", ""), std::path::PathBuf::from("textures/pbr/landscape/rock01.dds"));
    assert_eq!(output_rel(r"textures\pbr\landscape\rock01", "rmaos"), std::path::PathBuf::from("textures/pbr/landscape/rock01_rmaos.dds"));
}
```

- [ ] **Step 2: Run the tests and confirm they fail.**

Run: `cargo test -p sdc-gui --test pgp`
Expected: compile errors.

- [ ] **Step 3: Create `crates/gui/src/pgp/prefix.rs`:**

```rust
//! Match modes and the output prefix PGPatcher derives (M3b spec §5.5, gap review "How match1–match8 build the
//! output prefix"). Quirks are kept on purpose: the files must land where PGPatcher looks.
use super::settings::Settings;
use serde_json::Value;
use std::path::PathBuf;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PrefixError(pub String);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MatchMode {
    Diffuse,
    Normal,
    PathContains,
    Slot(u8),
}

/// The first alternative match key that is set decides the mode; none set means `match_diffuse` (spec §4.4).
pub fn match_mode(s: &Settings) -> MatchMode {
    let set = |k: &str| s.get(k).and_then(Value::as_str).is_some_and(|v| !v.is_empty());
    if set("match_normal") {
        return MatchMode::Normal;
    }
    if set("path_contains") {
        return MatchMode::PathContains;
    }
    (1..=8u8)
        .find(|n| set(&format!("match{n}")))
        .map_or(MatchMode::Diffuse, MatchMode::Slot)
}

pub fn slot_suffixes(n: u8) -> &'static [&'static str] {
    match n {
        1 => &["_d", "mask"],
        2 => &["_msn", "_n"],
        3 => &["_g", "_sk"],
        4 => &["_p"],
        5 => &["_e"],
        6 => &["_em", "_envmask", "_m", "_rmaos"],
        7 => &["_cnr", "_f", "_i", "_s"],
        8 => &["_b", "_bl", "_flow"],
        _ => &[],
    }
}

fn backslash(p: &str) -> String {
    p.replace('/', "\\")
}

fn strip_ci<'a>(s: &'a str, prefix: &str) -> &'a str {
    if s.len() >= prefix.len() && s.is_char_boundary(prefix.len()) && s[..prefix.len()].eq_ignore_ascii_case(prefix) {
        &s[prefix.len()..]
    } else {
        s
    }
}

fn strip_ext(s: &str) -> &str {
    match s.rsplit_once('.') {
        Some((stem, ext)) if !ext.contains('\\') && !stem.is_empty() => stem,
        _ => s,
    }
}

/// Reject what could escape the export folder or that Windows cannot name (DECISIONS P13).
pub fn check_rel_path(p: &str, field: &str) -> Result<(), PrefixError> {
    let err = |why: &str| Err(PrefixError(format!("{field}: {why} ({p:?})")));
    let b = backslash(p.trim());
    if b.is_empty() {
        return err("empty");
    }
    if b.starts_with('\\') {
        return err("must be relative, not start with a backslash");
    }
    if b.as_bytes().get(1) == Some(&b':') {
        return err("must be relative, not name a drive");
    }
    for c in b.split('\\') {
        if c == ".." {
            return err("must not contain '..'");
        }
        if c.chars().any(|ch| "<>:\"|?*".contains(ch) || ch.is_control()) {
            return err("contains a character Windows does not allow in names");
        }
    }
    Ok(())
}

/// `rel_dir` (the diffuse's folder under the opened folder, '/'-separated) and its output stem → `match_diffuse`:
/// without a leading `textures/` and `pbr/`, with backslashes, case kept.
pub fn derive_match_diffuse(rel_dir: &str, stem: &str) -> String {
    let dir = rel_dir.replace('\\', "/");
    let dir = strip_ci(&dir, "textures/");
    let dir = if dir.eq_ignore_ascii_case("textures") { "" } else { dir };
    let dir = strip_ci(dir, "pbr/");
    let dir = if dir.eq_ignore_ascii_case("pbr") { "" } else { dir };
    let dir = dir.trim_matches('/');
    if dir.is_empty() {
        stem.to_string()
    } else {
        backslash(&format!("{dir}/{stem}"))
    }
}

/// `textures\pbr\` + `rest` (a relative path that may already start with `textures\` and/or `pbr\`).
fn under_pbr(rest: &str) -> String {
    let r = strip_ci(rest, "textures\\");
    let r = strip_ci(r, "pbr\\");
    format!("textures\\pbr\\{r}")
}

fn text<'a>(s: &'a Settings, k: &str) -> Option<&'a str> {
    s.get(k).and_then(Value::as_str).filter(|v| !v.trim().is_empty())
}

/// The output prefix (backslashes, no extension) of a set with effective settings `s`; `derived` is the derived
/// `match_diffuse`.
pub fn prefix_of(s: &Settings, derived: &str) -> Result<String, PrefixError> {
    let rename = text(s, "rename").map(backslash);
    if let Some(r) = &rename {
        check_rel_path(r, "rename")?;
    }
    match match_mode(s) {
        MatchMode::Diffuse | MatchMode::PathContains | MatchMode::Normal => {
            let mode = match_mode(s);
            let (field, value) = match mode {
                MatchMode::Normal => ("match_normal", text(s, "match_normal").unwrap_or_default().to_string()),
                MatchMode::Diffuse => ("match_diffuse", text(s, "match_diffuse").map_or_else(|| derived.to_string(), str::to_string)),
                _ => ("match_diffuse", derived.to_string()),
            };
            let value = backslash(value.trim());
            check_rel_path(&value, field)?;
            if let Some(r) = rename {
                return Ok(under_pbr(&r));
            }
            let mut v = strip_ext(&value).to_string();
            if mode == MatchMode::Normal {
                v = strip_slot_suffix(&v, 2);
            }
            Ok(under_pbr(&v))
        }
        MatchMode::Slot(n) => {
            let field = format!("match{n}");
            let value = backslash(text(s, &field).unwrap_or_default().trim());
            check_rel_path(&value, &field)?;
            if let Some(r) = rename {
                return Ok(r);
            }
            Ok(under_pbr(&strip_slot_suffix(strip_ext(&value), n)))
        }
    }
}

fn strip_slot_suffix(v: &str, n: u8) -> String {
    let lower = v.to_lowercase();
    for suf in slot_suffixes(n) {
        if lower.ends_with(suf) {
            return v[..v.len() - suf.len()].to_string();
        }
    }
    v.to_string()
}

/// Warnings that are never "corrected" (owner, 2026-10-09).
pub fn pitfalls(s: &Settings) -> Vec<String> {
    let mut out = vec![];
    if let Some(v) = text(s, "match8") {
        if strip_ext(&v.to_lowercase()).ends_with("_s") {
            out.push("match8 does not strip _s (its list is the old backlight one); use match7 for a subsurface map".into());
        }
    }
    if matches!(match_mode(s), MatchMode::Slot(_)) {
        if let Some(r) = text(s, "rename") {
            if !strip_ci(&backslash(r), "textures\\").len().lt(&backslash(r).len()) {
                out.push("rename on a matchN entry replaces the whole prefix, so it must be a full path such as textures\\pbr\\…".into());
            }
        }
    }
    out
}

/// The output path, relative to the export folder, of the file with canonical `suffix` (`""` for the diffuse).
pub fn output_rel(prefix: &str, suffix: &str) -> PathBuf {
    let name = if suffix.is_empty() { format!("{prefix}.dds") } else { format!("{prefix}_{suffix}.dds") };
    PathBuf::from(name.replace('\\', "/"))
}
```

The `rename` check in `pitfalls` reads awkwardly. Write it plainly:

```rust
let starts_with_textures = backslash(r).to_lowercase().starts_with("textures\\");
if !starts_with_textures { out.push(…) }
```

The test expectations are what binds.

- [ ] **Step 4: Run the tests.**

Run: `cargo test -p sdc-gui --test pgp`
Expected: all pass.

- [ ] **Step 5: Run the gates and commit.**

```bash
cargo fmt --all && cargo clippy --workspace --all-targets -- -D warnings && cargo test -p sdc-gui
git add crates/gui/src/pgp/prefix.rs crates/gui/src/pgp/mod.rs crates/gui/tests/pgp.rs
git commit -m "feat(gui): PGPatcher match modes and output prefixes with the per-slot suffix table (M3b §5.5)

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

---

### Task 8: The JSON writer (spec §5.2, §5.3, §5.6)

**Files:**
- Create: `crates/gui/src/pgp/writer.rs`
- Modify: `crates/gui/src/pgp/mod.rs`
- Test: `crates/gui/tests/pgp.rs` (append)

**Interfaces:**
- Consumes: `KEYS`, `KeyKind`, `Settings`, `effective`, `blocked_by`, `feature_on`, `Feature`, `GLINT_DEFAULTS`, `match_mode`, `MatchMode`, `SCHEMA_URL`.
- Produces (registry):
  - `EntryInput { settings: Settings, slots: BTreeMap<u8, String>, derived_match: String }`;
  - `Node` (an ordered JSON tree);
  - `entry_node`, `document`, `to_text`, `Node::to_value`.

**Rules:**
- **Keys** are written in `KEYS` order.
- **The entry writes its own settings, not the defaults.** PGPatcher copies the `default` block into entries. `AlwaysBool` keys are the exception: they are always written with the **effective** value (own, else default, else `false`).
- **A blocked feature is dropped** (computed on the effective settings):
  - `AlwaysBool` → `false`;
  - `OnlyTrue` → omitted;
  - `Object` → omitted.
- **Dependent keys** are written only while their feature is effectively on:
  - Emissive group ⇐ `emissive`;
  - Parallax ⇐ `parallax`;
  - Subsurface ⇐ `subsurface`;
  - Multilayer ⇐ `multilayer`;
  - Glint ⇐ glint;
  - Fuzz ⇐ fuzz.
- **Match:**
  - In `Diffuse` mode, `match_diffuse` is written: own if set, else `derived_match`.
  - In any other mode, `match_diffuse` is omitted.
- **Colours:** the components `c` become `round(c / 255, 4)`. `Rgba` adds `1.0`.
- **Glint:** when on, its object holds every `glint.*` value, own or default, filled from `GLINT_DEFAULTS` where missing.
- **Fuzz:** when on, the object holds the `fuzz.*` keys that are set (`texture` only when `true`).
- **Slots:** `slots` (computed by the caller) are written as `slotN`, after any `slotN` in settings. The caller has already merged commands and borrowed slots.
- **The `default` block:** the defaults' own keys, with the same per-kind rules but with no derived match and no slot commands (match fields, `rename` and slots are per set only, spec §4.5).
- **Document:** `{ "$schema": SCHEMA_URL, "default": {…}, "entries": [ … ] }`, with 2-space indentation, `\n` line endings and a final newline.

- [ ] **Step 1: Write the failing tests.** Append to `crates/gui/tests/pgp.rs`:

```rust
use sdc_gui::pgp::writer::*;
use std::collections::BTreeMap;

fn entry(pairs: &[(&str, Value)], derived: &str) -> EntryInput {
    EntryInput { settings: s(pairs), slots: BTreeMap::new(), derived_match: derived.into() }
}

fn written(e: &EntryInput, defaults: &Settings) -> Value {
    document(defaults, std::slice::from_ref(e)).to_value()["entries"][0].clone()
}

#[test]
fn a_plain_entry_writes_the_match_and_the_three_always_written_features() {
    let v = written(&entry(&[], r"landscape\rock01"), &Settings::new());
    assert_eq!(
        v,
        json!({ "match_diffuse": r"landscape\rock01", "emissive": false, "parallax": false, "subsurface": false })
    );
}

#[test]
fn defaults_feed_always_written_features_but_are_not_copied() {
    let d = s(&[("parallax", json!(true)), ("specular_level", json!(0.04))]);
    let v = written(&entry(&[], "x"), &d);
    assert_eq!(v["parallax"], json!(true));
    assert!(v.get("specular_level").is_none(), "the default block carries it");
    let doc = document(&d, &[entry(&[], "x")]).to_value();
    assert_eq!(doc["default"], json!({ "parallax": true, "specular_level": 0.04 }));
    assert_eq!(doc["$schema"], json!(sdc_gui::pgp::schema::SCHEMA_URL));
}

#[test]
fn colours_convert_and_emissive_gets_alpha_one() {
    let v = written(
        &entry(&[("emissive", json!(true)), ("emissive_color", json!([255, 128, 0])), ("emissive_scale", json!(2.0))], "x"),
        &Settings::new(),
    );
    assert_eq!(v["emissive_color"], json!([1.0, 0.502, 0.0, 1.0]));
    assert_eq!(v["emissive_scale"], json!(2.0));
}

#[test]
fn dependent_keys_need_their_feature() {
    let v = written(&entry(&[("emissive_scale", json!(2.0)), ("displacement_scale", json!(0.4))], "x"), &Settings::new());
    assert!(v.get("emissive_scale").is_none() && v.get("displacement_scale").is_none());
}

#[test]
fn blocked_features_are_not_written() {
    let v = written(&entry(&[("hair", json!(true)), ("parallax", json!(true)), ("multilayer", json!(true))], "x"), &Settings::new());
    assert_eq!(v["hair"], json!(true));
    assert_eq!(v["parallax"], json!(false));
    assert!(v.get("multilayer").is_none());
}

#[test]
fn glint_is_filled_from_its_defaults_and_fuzz_writes_what_is_set() {
    let v = written(&entry(&[("glint", json!(true)), ("glint.microfacet_roughness", json!(0.02))], "x"), &Settings::new());
    assert_eq!(
        v["glint"],
        json!({ "screen_space_scale": 1.5, "log_microfacet_density": 40.0, "microfacet_roughness": 0.02, "density_randomization": 2.0 })
    );
    let v = written(&entry(&[("fuzz", json!(true)), ("fuzz.texture", json!(true)), ("fuzz.color", json!([0, 0, 0]))], "x"), &Settings::new());
    assert_eq!(v["fuzz"], json!({ "texture": true, "color": [0.0, 0.0, 0.0] }));
}

#[test]
fn other_match_modes_omit_match_diffuse_and_tri_state_writes_only_when_set() {
    let v = written(&entry(&[("match2", json!(r"a\b_n.dds")), ("lock_rmaos", json!(false))], "x"), &Settings::new());
    assert!(v.get("match_diffuse").is_none());
    assert_eq!(v["match2"], json!(r"a\b_n.dds"));
    assert_eq!(v["lock_rmaos"], json!(false));
    assert!(v.get("lock_diffuse").is_none());
}

#[test]
fn slots_and_key_order() {
    let mut e = entry(&[("specular_level", json!(0.04))], "x");
    e.slots.insert(6, r"textures\pbr\a\rock01_rmaos.dds".into());
    e.slots.insert(3, String::new());
    let text = to_text(&document(&Settings::new(), &[e]));
    let pos = |k: &str| text.find(&format!("\"{k}\"")).unwrap_or_else(|| panic!("{k} in {text}"));
    assert!(pos("match_diffuse") < pos("emissive") && pos("emissive") < pos("specular_level"));
    assert!(pos("specular_level") < pos("slot3") && pos("slot3") < pos("slot6"));
    assert!(text.ends_with("}\n") && text.contains("\n  \"default\": {}") , "{text}");
    assert!(text.contains(r#""slot6": "textures\\pbr\\a\\rock01_rmaos.dds""#), "{text}");
}
```

- [ ] **Step 2: Run the tests and confirm they fail.**

Run: `cargo test -p sdc-gui --test pgp`
Expected: compile errors.

- [ ] **Step 3: Create `crates/gui/src/pgp/writer.rs`:**

```rust
//! The PGPatcher JSON document (M3b spec §5.2–5.3, §5.6): `{ "$schema", "default", "entries" }`, keys in schema
//! order. serde_json's map sorts keys, so the document is an ordered tree (`Node`) printed by `to_text`.
use super::schema::{KeyKind, Group, KEYS, SCHEMA_URL};
use super::settings::{blocked_by, effective, feature_on, Feature, Settings, GLINT_DEFAULTS};
use super::prefix::{match_mode, MatchMode};
use serde_json::{json, Value};
use std::collections::BTreeMap;

#[derive(Debug, Clone, PartialEq)]
pub enum Node {
    Scalar(Value),
    Obj(Vec<(String, Node)>),
    Arr(Vec<Node>),
}

impl Node {
    pub fn to_value(&self) -> Value {
        match self {
            Node::Scalar(v) => v.clone(),
            Node::Arr(a) => Value::Array(a.iter().map(Node::to_value).collect()),
            Node::Obj(o) => Value::Object(o.iter().map(|(k, v)| (k.clone(), v.to_value())).collect()),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Default)]
pub struct EntryInput {
    /// The set's own settings (not merged with the defaults).
    pub settings: Settings,
    /// Final slot paths (commands and borrowed slots, already merged by the caller): slot → path, `""` clears.
    pub slots: BTreeMap<u8, String>,
    pub derived_match: String,
}

fn colour(v: &Value, alpha: bool) -> Option<Node> {
    let mut out: Vec<Node> = v
        .as_array()?
        .iter()
        .take(3)
        .map(|c| {
            let x = c.as_f64().unwrap_or(0.0) / 255.0;
            Node::Scalar(json!((x * 10_000.0).round() / 10_000.0))
        })
        .collect();
    if out.len() != 3 {
        return None;
    }
    if alpha {
        out.push(Node::Scalar(json!(1.0)));
    }
    Some(Node::Arr(out))
}

fn group_feature(g: Group) -> Option<Feature> {
    Some(match g {
        Group::Emissive => Feature::Emissive,
        Group::Parallax => Feature::Parallax,
        Group::Subsurface => Feature::Subsurface,
        Group::Multilayer => Feature::Multilayer,
        Group::Glint => Feature::Glint,
        Group::Fuzz => Feature::Fuzz,
        _ => return None,
    })
}

fn feature_of_key(key: &str) -> Option<Feature> {
    super::settings::FEATURE_KEYS.iter().find(|(_, k)| *k == key).map(|(f, _)| *f)
}

/// Whether feature `f` is written as on. Two exclusive features can both be on only through a default plus an own
/// value (the UI greys the second); then exactly one is written, by precedence: hair, then multilayer, then glint,
/// then fuzz; emissive and parallax yield to hair, subsurface to hair and multilayer.
fn live(eff: &Settings, f: Feature) -> bool {
    use Feature::*;
    let on = |g| feature_on(eff, g);
    let hair = on(Hair);
    let ml = on(Multilayer) && !hair;
    let glint = on(Glint) && !hair && !ml;
    match f {
        Hair => hair,
        Multilayer => ml,
        Glint => glint,
        Fuzz => on(Fuzz) && !hair && !ml && !glint,
        Emissive => on(Emissive) && !hair,
        Parallax => on(Parallax) && !hair,
        Subsurface => on(Subsurface) && !hair && !ml,
    }
}

/// The entries of one object: `own` written under the rules; `eff` decides features; `derived` is the derived match
/// (`None` for the default block); `slots` final slot paths.
fn object(own: &Settings, eff: &Settings, derived: Option<&str>, slots: &BTreeMap<u8, String>) -> Vec<(String, Node)> {
    let mut out = vec![];
    let mode = match_mode(eff);
    for k in KEYS.iter().filter(|k| !k.key.contains('.')) {
        if let Some(f) = group_feature(k.group) {
            if !live(eff, f) {
                continue;
            }
        }
        let v = own.get(k.key);
        let node = match k.kind {
            KeyKind::AlwaysBool => {
                if derived.is_none() && v.is_none() {
                    None // the default block writes only what it sets
                } else {
                    let f = feature_of_key(k.key).expect("always-written keys are features");
                    Some(Node::Scalar(json!(live(eff, f))))
                }
            }
            KeyKind::OnlyTrue => {
                let on = v.and_then(Value::as_bool) == Some(true);
                let ok = feature_of_key(k.key).is_none_or(|f| live(eff, f));
                (on && ok).then(|| Node::Scalar(json!(true)))
            }
            KeyKind::Object => {
                let f = feature_of_key(k.key).expect("objects are features");
                if !live(eff, f) || (derived.is_none() && v.is_none()) {
                    None
                } else {
                    Some(Node::Obj(sub_object(k.key, own, eff)))
                }
            }
            KeyKind::Tri | KeyKind::Number => v.filter(|v| !v.is_null()).map(|v| Node::Scalar(v.clone())),
            KeyKind::Text => {
                if k.key == "match_diffuse" {
                    match (derived, mode) {
                        (Some(d), MatchMode::Diffuse) => Some(Node::Scalar(json!(v.and_then(Value::as_str).filter(|s| !s.is_empty()).unwrap_or(d)))),
                        _ => None,
                    }
                } else if derived.is_none() && k.group == Group::Match {
                    None // match fields are per set only
                } else {
                    v.and_then(Value::as_str).filter(|s| !s.is_empty()).map(|s| Node::Scalar(json!(s)))
                }
            }
            KeyKind::Rgb => v.and_then(|v| colour(v, false)),
            KeyKind::Rgba => v.and_then(|v| colour(v, true)),
            KeyKind::Slot => {
                let n: u8 = k.key[4..].parse().expect("slotN");
                slots.get(&n).map(|p| Node::Scalar(json!(p)))
            }
        };
        if let Some(n) = node {
            out.push((k.key.to_string(), n));
        }
    }
    out
}

fn sub_object(obj: &str, own: &Settings, eff: &Settings) -> Vec<(String, Node)> {
    let mut out = vec![];
    for k in KEYS.iter().filter(|k| k.key.starts_with(&format!("{obj}."))) {
        let name = &k.key[obj.len() + 1..];
        let v = own.get(k.key).or_else(|| eff.get(k.key));
        let node = match k.kind {
            KeyKind::Number => v.cloned().or_else(|| {
                GLINT_DEFAULTS.iter().find(|(d, _)| *d == k.key).map(|(_, x)| json!(x))
            }).map(Node::Scalar),
            KeyKind::OnlyTrue => (v.and_then(Value::as_bool) == Some(true)).then(|| Node::Scalar(json!(true))),
            KeyKind::Rgb => v.and_then(|v| colour(v, false)),
            _ => None,
        };
        if let Some(n) = node {
            out.push((name.to_string(), n));
        }
    }
    out
}

pub fn entry_node(defaults: &Settings, e: &EntryInput) -> Node {
    let eff = effective(defaults, &e.settings);
    Node::Obj(object(&e.settings, &eff, Some(&e.derived_match), &e.slots))
}

pub fn document(defaults: &Settings, entries: &[EntryInput]) -> Node {
    let default = Node::Obj(object(defaults, defaults, None, &BTreeMap::new()));
    Node::Obj(vec![
        ("$schema".into(), Node::Scalar(json!(SCHEMA_URL))),
        ("default".into(), default),
        ("entries".into(), Node::Arr(entries.iter().map(|e| entry_node(defaults, e)).collect())),
    ])
}

/// Two-space indented JSON, `\n` line endings, a final newline. Scalars print as serde_json prints them.
pub fn to_text(doc: &Node) -> String {
    fn go(n: &Node, ind: usize, out: &mut String) {
        let pad = |i: usize| "  ".repeat(i);
        match n {
            Node::Scalar(v) => out.push_str(&serde_json::to_string(v).expect("scalars serialise")),
            Node::Arr(a) if a.iter().all(|x| matches!(x, Node::Scalar(_))) => {
                // Short scalar arrays (colours) on one line.
                out.push('[');
                for (i, x) in a.iter().enumerate() {
                    if i > 0 {
                        out.push_str(", ");
                    }
                    go(x, ind, out);
                }
                out.push(']');
            }
            Node::Arr(a) => {
                out.push_str("[\n");
                for (i, x) in a.iter().enumerate() {
                    out.push_str(&pad(ind + 1));
                    go(x, ind + 1, out);
                    out.push_str(if i + 1 < a.len() { ",\n" } else { "\n" });
                }
                out.push_str(&pad(ind));
                out.push(']');
            }
            Node::Obj(o) if o.is_empty() => out.push_str("{}"),
            Node::Obj(o) => {
                out.push_str("{\n");
                for (i, (k, v)) in o.iter().enumerate() {
                    out.push_str(&pad(ind + 1));
                    out.push_str(&serde_json::to_string(k).expect("keys serialise"));
                    out.push_str(": ");
                    go(v, ind + 1, out);
                    out.push_str(if i + 1 < o.len() { ",\n" } else { "\n" });
                }
                out.push_str(&pad(ind));
                out.push('}');
            }
        }
    }
    let mut s = String::new();
    go(doc, 0, &mut s);
    s.push('\n');
    s
}
```

`blocked_by` is not imported by the writer (it is for the UI); remove it from the `use` line if clippy flags it. Add this test for the precedence:

```rust
#[test]
fn a_conflict_through_defaults_keeps_the_stronger_feature() {
    let d = s(&[("multilayer", json!(true))]);
    let v = written(&entry(&[("subsurface", json!(true))], "x"), &d);
    assert_eq!(v["subsurface"], json!(false));
    let doc = document(&d, &[entry(&[], "x")]).to_value();
    assert_eq!(doc["default"]["multilayer"], json!(true));
}
```


- [ ] **Step 4: Run the tests.**

Run: `cargo test -p sdc-gui --test pgp`
Expected: all pass.

- [ ] **Step 5: Run the gates and commit.**

```bash
cargo fmt --all && cargo clippy --workspace --all-targets -- -D warnings && cargo test -p sdc-gui
git add crates/gui/src/pgp/writer.rs crates/gui/src/pgp/mod.rs crates/gui/tests/pgp.rs
git commit -m "feat(gui): PGPatcher JSON writer: write rules, colours, glint defaults, schema key order (M3b §5.2–5.6)

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

---

### Task 9: Validate exported JSON against the vendored schema (spec §5.6, DECISIONS P12)

**Files:**
- Create: `crates/gui/src/pgp/validate.rs`
- Modify: `crates/gui/src/pgp/mod.rs`
- Test: `crates/gui/tests/pgp.rs` (append)

**Interfaces:**
- Consumes: `SCHEMA_JSON`, `writer::document`.
- Produces: `validate(doc: &Value) -> Result<(), Vec<String>>`, and `KEYWORDS: &[&str]`, the keywords it implements.

- [ ] **Step 1: Write the failing tests.** Append to `crates/gui/tests/pgp.rs`:

```rust
use sdc_gui::pgp::validate::*;

fn keywords_in(v: &Value, out: &mut std::collections::BTreeSet<String>, under_properties: bool) {
    match v {
        Value::Object(m) => {
            for (k, x) in m {
                if !under_properties {
                    out.insert(k.clone());
                }
                keywords_in(x, out, k == "properties" || k == "definitions");
            }
        }
        Value::Array(a) => a.iter().for_each(|x| keywords_in(x, out, false)),
        _ => {}
    }
}

#[test]
fn the_validator_covers_every_keyword_the_schema_uses() {
    let schema: Value = serde_json::from_str(sdc_gui::pgp::schema::SCHEMA_JSON).unwrap();
    let mut used = std::collections::BTreeSet::new();
    keywords_in(&schema, &mut used, false);
    let annotations = ["$schema", "$id", "title", "description", "examples", "default", "definitions", "properties"];
    for k in used {
        assert!(
            KEYWORDS.contains(&k.as_str()) || annotations.contains(&k.as_str()),
            "the vendored schema uses '{k}', which validate.rs does not implement (DECISIONS P12)"
        );
    }
}

#[test]
fn written_documents_validate_and_bad_ones_do_not() {
    let e = entry(&[("emissive", json!(true)), ("emissive_color", json!([255, 0, 0])), ("glint", json!(true))], "a");
    validate(&document(&s(&[("specular_level", json!(0.04))]), &[e]).to_value()).unwrap();
    // An entry with no match key, a removed key, an unknown key, a wrong type, a short colour.
    for bad in [
        json!({ "default": {}, "entries": [ { "emissive": true } ] }),
        json!({ "default": {}, "entries": [ { "match_diffuse": "a", "pbr": true } ] }),
        json!({ "default": {}, "entries": [ { "match_diffuse": "a", "nope": 1 } ] }),
        json!({ "default": {}, "entries": [ { "match_diffuse": 3 } ] }),
        json!({ "default": {}, "entries": [ { "match_diffuse": "a", "subsurface_color": [1, 1] } ] }),
        json!({ "entries": [] }),
    ] {
        assert!(validate(&bad).is_err(), "{bad}");
    }
}
```

- [ ] **Step 2: Run the tests and confirm they fail.**

Run: `cargo test -p sdc-gui --test pgp`
Expected: compile errors.

- [ ] **Step 3: Create `crates/gui/src/pgp/validate.rs`:**

```rust
//! A draft-07 subset validator for the vendored PGPatcher schema (DECISIONS P12): exactly the keywords that schema
//! uses. A test fails if the schema starts using another one. Validation is a safety net that should never fire:
//! the writer builds documents from the same key table.
use super::schema::SCHEMA_JSON;
use serde_json::Value;
use std::sync::OnceLock;

pub const KEYWORDS: &[&str] = &[
    "type", "properties", "additionalProperties", "required", "items", "minItems", "maxItems", "$ref", "allOf",
    "anyOf", "oneOf", "not", "if", "then", "else",
];

fn schema() -> &'static Value {
    static S: OnceLock<Value> = OnceLock::new();
    S.get_or_init(|| serde_json::from_str(SCHEMA_JSON).expect("the vendored schema parses"))
}

fn resolve<'a>(root: &'a Value, r: &str) -> Option<&'a Value> {
    let path = r.strip_prefix("#/")?;
    path.split('/').try_fold(root, |v, part| v.get(part))
}

fn type_ok(t: &str, v: &Value) -> bool {
    match t {
        "object" => v.is_object(),
        "array" => v.is_array(),
        "string" => v.is_string(),
        "number" => v.is_number(),
        "integer" => v.is_i64() || v.is_u64() || v.as_f64().is_some_and(|f| f.fract() == 0.0),
        "boolean" => v.is_boolean(),
        "null" => v.is_null(),
        _ => false,
    }
}

fn check(root: &Value, s: &Value, v: &Value, at: &str, errs: &mut Vec<String>) {
    let Some(s) = s.as_object() else {
        if s == &Value::Bool(false) {
            errs.push(format!("{at}: not allowed"));
        }
        return;
    };
    if let Some(r) = s.get("$ref").and_then(Value::as_str) {
        match resolve(root, r) {
            Some(t) => check(root, t, v, at, errs),
            None => errs.push(format!("{at}: unresolved {r}")),
        }
    }
    if let Some(t) = s.get("type") {
        let ok = match t {
            Value::String(t) => type_ok(t, v),
            Value::Array(ts) => ts.iter().filter_map(Value::as_str).any(|t| type_ok(t, v)),
            _ => true,
        };
        if !ok {
            errs.push(format!("{at}: expected {t}"));
            return;
        }
    }
    if let (Some(req), Some(o)) = (s.get("required").and_then(Value::as_array), v.as_object()) {
        for r in req.iter().filter_map(Value::as_str) {
            if !o.contains_key(r) {
                errs.push(format!("{at}: missing '{r}'"));
            }
        }
    }
    if let Some(o) = v.as_object() {
        let props = s.get("properties").and_then(Value::as_object);
        for (k, x) in o {
            match props.and_then(|p| p.get(k)) {
                Some(ps) => check(root, ps, x, &format!("{at}.{k}"), errs),
                None => {
                    if s.get("additionalProperties") == Some(&Value::Bool(false)) {
                        errs.push(format!("{at}: unknown key '{k}'"));
                    }
                }
            }
        }
    }
    if let Some(a) = v.as_array() {
        if let Some(n) = s.get("minItems").and_then(Value::as_u64) {
            if (a.len() as u64) < n {
                errs.push(format!("{at}: needs at least {n} items"));
            }
        }
        if let Some(n) = s.get("maxItems").and_then(Value::as_u64) {
            if (a.len() as u64) > n {
                errs.push(format!("{at}: needs at most {n} items"));
            }
        }
        if let Some(items) = s.get("items") {
            for (i, x) in a.iter().enumerate() {
                check(root, items, x, &format!("{at}[{i}]"), errs);
            }
        }
    }
    let passes = |sub: &Value| {
        let mut e = vec![];
        check(root, sub, v, at, &mut e);
        e.is_empty()
    };
    if let Some(all) = s.get("allOf").and_then(Value::as_array) {
        for sub in all {
            check(root, sub, v, at, errs);
        }
    }
    if let Some(any) = s.get("anyOf").and_then(Value::as_array) {
        if !any.iter().any(passes) {
            errs.push(format!("{at}: matches none of anyOf"));
        }
    }
    if let Some(one) = s.get("oneOf").and_then(Value::as_array) {
        let n = one.iter().filter(|x| passes(x)).count();
        if n != 1 {
            errs.push(format!("{at}: matches {n} of oneOf, needs exactly 1"));
        }
    }
    if let Some(not) = s.get("not") {
        if passes(not) {
            errs.push(format!("{at}: must not match 'not'"));
        }
    }
    if let Some(cond) = s.get("if") {
        let branch = if passes(cond) { s.get("then") } else { s.get("else") };
        if let Some(b) = branch {
            check(root, b, v, at, errs);
        }
    }
}

pub fn validate(doc: &Value) -> Result<(), Vec<String>> {
    let root = schema();
    let mut errs = vec![];
    check(root, root, doc, "$", &mut errs);
    if errs.is_empty() {
        Ok(())
    } else {
        Err(errs)
    }
}
```

Add `pub mod validate;` (and `pub mod writer;`, `pub mod prefix;` if not yet added) to `pgp/mod.rs`.

- [ ] **Step 4: Run the tests.**

Run: `cargo test -p sdc-gui --test pgp`
Expected: all pass.

`not: {}` (the removed keys) is an empty schema that every value passes, so `not` fails, as intended. If `the_validator_covers_every_keyword_the_schema_uses` names a keyword, add it to `check` and `KEYWORDS` with a test case.

- [ ] **Step 5: Run the gates and commit.**

```bash
cargo fmt --all && cargo clippy --workspace --all-targets -- -D warnings && cargo test -p sdc-gui
git add crates/gui/src/pgp/validate.rs crates/gui/src/pgp/mod.rs crates/gui/tests/pgp.rs
git commit -m "feat(gui): in-tree validator for the vendored PGPatcher schema (M3b §5.6, P12)

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

---
### Task 10: The set model: views, ownership, engine inputs, drop rules (spec §3, §6.1)

**Files:**
- Create: `crates/gui/src/model/sets.rs`
- Modify:
  - `crates/gui/src/model/mod.rs`: `pub mod sets;`;
  - `request()` fills `set_edits`/`output_paths`;
  - `apply_plan` re-plans once when the computed inputs changed (see "Two-pass planning");
  - the field `last_inputs`.
- Test: `crates/gui/tests/sets_model.rs`

**Interfaces:**
- Consumes:
  - `engine::sets::{SetEdit, OutputKey, set_id}`;
  - `engine::group::split_suffix`;
  - `engine::config::Subtype`;
  - `SetsDoc`/`SetEntry` (Task 4);
  - `AppModel::edit_docs` and `history::{CreatedFile, sha256_of}` (Task 5);
  - `pgp::settings::{Settings, effective}` and `pgp::prefix::{derive_match_diffuse, prefix_of, output_rel, check_rel_path, PrefixError}` (Tasks 6–7).
- Produces (registry):
  - `SlotKind` (`slot_number`, `suffix`, `label`, `ALL`), `SlotFill`, `SetView`;
  - `set_views`, `slot_of`, `slot_conflict`, `engine_inputs`, `output_candidates`;
  - `DropChoice`, `DropOutcome`, `SlotCommand`;
  - the `AppModel` methods `drop_on_set`, `drop_choice`, `drop_replace`, `drop_new_set`, `reorder_set`, `rename_set`, `delete_set`, `remove_member`, `set_json_value`, `set_slot_command`.

**Rules:**
- **Display order:** ids in `SetsDoc::order()` that still exist, then every other set by id. Created sets with no members yet are included (their `key` is `""`).
- **Slot of a texture** (by resolved sub-type; raw packing sources and items without a job have none):

  | Sub-types | Slot |
  |---|---|
  | `PbrAlbedo`, `Diffuse`, `LandscapeDiffuse` | Diffuse |
  | `PbrNormal`, `Normal`, `LandscapeNormal`, `Msn` | Normal |
  | `PbrEmissive`, `Glow` | Emissive |
  | `Height` | Height |
  | `Rmaos`, `Rfaos` | Rmaos |
  | `CoatNormal` | Coat |
  | `PbrFuzz` | Fuzz |
  | `PbrSubsurface` | Subsurface |

- **PBR set (P5):** its Diffuse slot holds an item whose job's `profile.subtype` is `PbrAlbedo`.
- **Ownership (spec §3.3):**
  - A finished output is used by its source set (the set that holds the item) and by every set that borrows it.
  - Its **owner** is the first set, in display order, among those users **that is a PBR set**. If none is a PBR set, there is no owner and the output keeps its mirrored M3a path.
  - On CS, the output's path is `output_rel(owner_prefix, slot.suffix())`.
- **Engine inputs:**
  - **Edits**, in this order across all entries:
    1. `Create` for created sets;
    2. `Rename { from: renamed_from, to: id }`;
    3. `Move { file, to: id }` for every `add`;
    4. `Remove { file }` for every `remove`.
  - **Output paths:** only on the CS target, and only for owned outputs of PBR sets whose prefix is `Ok`.
    - For an image item: `OutputKey::File(item.key)`.
    - For a packed item: `OutputKey::Packed { set: <source set id>, suffix: <recipe suffix> }`. The suffix is `job.pack.kind.suffix()`; if `PackJob` exposes no `kind`, take it from the output stem after the last `_`.
- **Two-pass planning:** output paths need the previous plan's sub-types (PBR detection, slots).
  - After each plan, `apply_plan` computes `engine_inputs(self)`. If they differ from the inputs that plan was made with, it re-plans.
  - At most **two** automatic re-plans in a row (`auto_replans` counter, reset by any user edit), so a bug can never loop.
- **Drop rules (spec §3.1, §3.3):** see `drop_on_set` below.

- [ ] **Step 1: Write the failing tests.** First, add a fixture helper to `crates/gui/tests/support/mod.rs`. Without it, rock01's diffuse and normal still need decisions in the M3a fixture:

```rust
/// The M3a fixture with rock01 decided as a TruePBR set: `pbr_albedo` diffuse, `pbr_normal` normal (DirectX green).
/// leaf.png and odd.png stay undecided, as in M3a.
pub fn pbr_opened(dir: &Path) -> AppModel {
    use engine::config::{GreenConvention, Subtype};
    use sdc_gui::model::Answer;
    let mut m = opened(dir);
    m.decide("landscape/rock01.png", Answer::Subtype(Subtype::PbrAlbedo));
    m.decide("landscape/rock01_normal.png", Answer::Subtype(Subtype::PbrNormal));
    m.decide("landscape/rock01_normal.png", Answer::Green(GreenConvention::Directx));
    assert!(m.wait_idle(Duration::from_secs(60)), "planning did not finish");
    m
}
```

`crates/cli/tests/gui_repro.rs` decides rock01 as `Diffuse`. Its sets are then not PBR sets, the GUI sends no output paths, and the GUI-vs-CLI comparison stays valid unchanged.

Then create `crates/gui/tests/sets_model.rs`:

```rust
mod support;
use sdc_gui::model::sets::*;
use serde_json::json;
use std::time::Duration;
use support::*;

fn idle(m: &mut sdc_gui::model::AppModel) {
    assert!(m.wait_idle(Duration::from_secs(60)));
}

fn view<'a>(v: &'a [SetView], id: &str) -> &'a SetView {
    v.iter().find(|s| s.id == id).unwrap_or_else(|| panic!("no set {id} in {:?}", v.iter().map(|s| &s.id).collect::<Vec<_>>()))
}

fn key_of(m: &sdc_gui::model::AppModel, name: &str) -> String {
    m.items.iter().find(|i| i.name == name).unwrap_or_else(|| panic!("no item {name}")).key.clone()
}

#[test]
fn the_fixture_set_has_its_slots_a_derived_match_and_pbr_output_paths() {
    let d = tempfile::tempdir().unwrap();
    let mut m = pbr_opened(d.path());
    idle(&mut m); // the second (output-path) plan
    let v = set_views(&m);
    let rock = view(&v, "landscape/rock01");
    assert!(rock.pbr, "rock01 packs _rmaos, so its diffuse resolves pbr_albedo");
    assert!(matches!(rock.slots.get(&SlotKind::Diffuse), Some(SlotFill::Own { .. })));
    assert!(matches!(rock.slots.get(&SlotKind::Normal), Some(SlotFill::Own { .. })));
    assert!(matches!(rock.slots.get(&SlotKind::Rmaos), Some(SlotFill::Own { .. })));
    assert_eq!(rock.derived_match.as_deref(), Some(r"landscape\rock01"));
    assert_eq!(rock.prefix.as_deref(), Some(r"textures\pbr\landscape\rock01"));
    let (_, paths) = engine_inputs(&m);
    use engine::sets::OutputKey;
    assert_eq!(
        paths.get(&OutputKey::File(key_of(&m, "rock01.png"))),
        Some(&std::path::PathBuf::from("textures/pbr/landscape/rock01.dds"))
    );
    assert_eq!(
        paths.get(&OutputKey::Packed { set: "landscape/rock01".into(), suffix: "rmaos".into() }),
        Some(&std::path::PathBuf::from("textures/pbr/landscape/rock01_rmaos.dds"))
    );
    // The plan the model holds now writes there.
    let diffuse = m.items.iter().find(|i| i.name == "rock01.png").unwrap();
    assert!(diffuse.job.as_ref().unwrap().output.to_string_lossy().replace('\\', "/").ends_with("textures/pbr/landscape/rock01.dds"));
}

#[test]
fn vanilla_gets_no_output_paths() {
    let d = tempfile::tempdir().unwrap();
    let mut m = pbr_opened(d.path());
    m.set_target(engine::config::Target::Vanilla);
    idle(&mut m);
    assert!(engine_inputs(&m).1.is_empty());
}

#[test]
fn dropping_a_diffuse_on_a_set_with_one_asks_to_replace() {
    let d = tempfile::tempdir().unwrap();
    let mut m = pbr_opened(d.path());
    idle(&mut m);
    let leaf = key_of(&m, "leaf.png");
    let out = m.drop_on_set(&leaf, "landscape/rock01");
    assert_eq!(out, DropOutcome::AskReplace { existing: key_of(&m, "rock01.png") });
    m.drop_replace(&leaf, "landscape/rock01");
    idle(&mut m);
    let v = set_views(&m);
    let rock = view(&v, "landscape/rock01");
    assert!(matches!(rock.slots.get(&SlotKind::Diffuse), Some(SlotFill::Own { item }) if *item == leaf));
    // The replaced diffuse became a set of its own; one undo puts both back.
    m.undo();
    idle(&mut m);
    let v = set_views(&m);
    assert!(matches!(view(&v, "landscape/rock01").slots.get(&SlotKind::Diffuse), Some(SlotFill::Own { item }) if *item == key_of(&m, "rock01.png")));
}

#[test]
fn new_set_share_and_reorder() {
    let d = tempfile::tempdir().unwrap();
    let mut m = pbr_opened(d.path());
    idle(&mut m);
    let odd = key_of(&m, "odd.png");
    let id = m.drop_new_set(&odd);
    idle(&mut m);
    assert!(set_views(&m).iter().any(|s| s.id == id && s.created));
    let rmaos = m.items.iter().find(|i| i.name.ends_with("rmaos")).unwrap().key.clone();
    assert_eq!(m.drop_on_set(&rmaos, &id), DropOutcome::AskChoice { from: "landscape/rock01".into() });
    assert_eq!(m.drop_choice(&rmaos, &id, DropChoice::Share), DropOutcome::Joined);
    idle(&mut m);
    let v = set_views(&m);
    assert!(matches!(view(&v, &id).slots.get(&SlotKind::Rmaos), Some(SlotFill::Borrowed { from, .. }) if from == "landscape/rock01"));
    // Reorder puts the new set first.
    m.reorder_set(&id, 0);
    assert_eq!(set_views(&m)[0].id, id);
    assert_eq!(m.sets_doc.as_ref().unwrap().order()[0], id);
}

#[test]
fn duplicating_a_raw_source_copies_it_and_undo_deletes_it_only_when_unchanged() {
    let d = tempfile::tempdir().unwrap();
    let mut m = pbr_opened(d.path());
    idle(&mut m);
    let odd = key_of(&m, "odd.png");
    let id = m.drop_new_set(&odd); // e.g. "landscape/odd-2"
    idle(&mut m);
    let rough = key_of(&m, "rock01_roughness.png");
    assert_eq!(m.drop_on_set(&rough, &id), DropOutcome::AskChoice { from: "landscape/rock01".into() });
    assert!(matches!(m.drop_choice(&rough, &id, DropChoice::Share), DropOutcome::Refused(_)), "raw maps are not shareable");
    assert_eq!(m.drop_choice(&rough, &id, DropChoice::Duplicate), DropOutcome::Joined);
    let base = id.rsplit('/').next().unwrap().to_string();
    let copy = m.root.clone().unwrap().join(format!("landscape/{base}_roughness.png"));
    assert!(copy.exists(), "{copy:?}");
    m.undo();
    assert!(!copy.exists(), "an unchanged copy is deleted by undo");
    m.redo();
    assert!(copy.exists(), "redo copies it again");
    std::fs::write(&copy, b"changed").unwrap();
    m.undo();
    assert!(copy.exists(), "a changed copy is kept");
    assert!(matches!(m.dialog, Some(sdc_gui::model::Dialog::Error(_))));
}

#[test]
fn coat_and_fuzz_cannot_share_a_set() {
    assert!(slot_conflict(&[SlotKind::Fuzz], SlotKind::Coat).is_some());
    assert!(slot_conflict(&[SlotKind::Coat], SlotKind::Fuzz).is_some());
    assert!(slot_conflict(&[SlotKind::Diffuse, SlotKind::Normal], SlotKind::Coat).is_none());
}

#[test]
fn a_member_that_vanished_shows_as_missing_and_is_reported() {
    let d = tempfile::tempdir().unwrap();
    let mut m = pbr_opened(d.path());
    idle(&mut m);
    m.edit_docs("t", None, true, |_, s| s.add_member("landscape/rock01", "landscape/gone.png"));
    idle(&mut m);
    let v = set_views(&m);
    assert_eq!(view(&v, "landscape/rock01").missing, vec!["landscape/gone.png".to_string()]);
    assert!(m.sets_doc.as_ref().unwrap().entries().iter().any(|e| e.add.contains(&"landscape/gone.png".to_string())), "never dropped");
}

#[test]
fn edits_go_creates_and_renames_first_then_moves_then_removes() {
    let d = tempfile::tempdir().unwrap();
    let mut m = pbr_opened(d.path());
    m.edit_docs("t", None, false, |_, s| {
        s.add_member("mine/new", "landscape/leaf.png");
        s.create_set("mine/new");
        s.remove_member("landscape/rock01", "landscape/rock01_normal.png");
    });
    let (edits, _) = engine_inputs(&m);
    use engine::sets::SetEdit;
    assert!(matches!(&edits[0], SetEdit::Create { set } if set == "mine/new"));
    assert!(matches!(&edits[1], SetEdit::Move { to, .. } if to == "mine/new"));
    assert!(matches!(edits.last().unwrap(), SetEdit::Remove { .. }));
}

#[test]
fn json_only_edits_do_not_replan_but_match_edits_do() {
    let d = tempfile::tempdir().unwrap();
    let mut m = pbr_opened(d.path());
    idle(&mut m);
    m.set_json_value(Some("landscape/rock01"), "specular_level", Some(json!(0.05)));
    assert!(!m.busy(), "a material value does not plan");
    m.set_json_value(Some("landscape/rock01"), "match_diffuse", Some(json!(r"landscape\boulder")));
    assert!(m.busy(), "a match changes output paths, so it plans");
    idle(&mut m);
    assert_eq!(view(&set_views(&m), "landscape/rock01").prefix.as_deref(), Some(r"textures\pbr\landscape\boulder"));
}
```

The `busy()` assertions depend on `replan` starting synchronously, which it does (it sets `planning` before spawning).

- [ ] **Step 2: Run the tests and confirm they fail.**

Run: `cargo test -p sdc-gui --test sets_model`
Expected: compile errors.

- [ ] **Step 3: Create `crates/gui/src/model/sets.rs`.** Before writing `job_suffix`, confirm `PackJob`'s fields with `grep -n "pub struct PackJob" -A 15 crates/engine/src/pack/mod.rs`.

```rust
//! The user's texture sets (M3b spec §3, §6.1): views for the Sets column and set editor, shared-slot ownership,
//! the engine inputs (`set_edits`, `output_paths`) and the drop rules. No egui types.
use super::history::{sha256_of, CreatedFile};
use super::items::{ItemKind, ItemStatus, TextureItem};
use super::{AppModel, Dialog};
use crate::pgp::prefix::{check_rel_path, derive_match_diffuse, output_rel, prefix_of};
use crate::pgp::settings::{effective, Settings};
use crate::sets_doc::SetEntry;
use engine::config::{Subtype, Target};
use engine::overrides::path_key_matches;
use engine::sets::{set_id, OutputKey, SetEdit};
use serde_json::Value;
use std::collections::BTreeMap;
use std::path::PathBuf;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum SlotKind {
    Diffuse,
    Normal,
    Emissive,
    Height,
    Rmaos,
    Coat,
    Fuzz,
    Subsurface,
}

impl SlotKind {
    pub const ALL: [SlotKind; 8] = [
        SlotKind::Diffuse,
        SlotKind::Normal,
        SlotKind::Rmaos,
        SlotKind::Height,
        SlotKind::Emissive,
        SlotKind::Subsurface,
        SlotKind::Coat,
        SlotKind::Fuzz,
    ];
    /// PGPatcher's 1-based `slotN` (DECISIONS P6).
    pub fn slot_number(self) -> u8 {
        match self {
            SlotKind::Diffuse => 1,
            SlotKind::Normal => 2,
            SlotKind::Emissive => 3,
            SlotKind::Height => 4,
            SlotKind::Rmaos => 6,
            SlotKind::Coat | SlotKind::Fuzz => 7,
            SlotKind::Subsurface => 8,
        }
    }
    /// The canonical file suffix under the prefix (`""` for the diffuse).
    pub fn suffix(self) -> &'static str {
        match self {
            SlotKind::Diffuse => "",
            SlotKind::Normal => "n",
            SlotKind::Emissive => "g",
            SlotKind::Height => "p",
            SlotKind::Rmaos => "rmaos",
            SlotKind::Coat => "cnr",
            SlotKind::Fuzz => "f",
            SlotKind::Subsurface => "s",
        }
    }
    pub fn label(self) -> &'static str {
        match self {
            SlotKind::Diffuse => "Diffuse",
            SlotKind::Normal => "Normal _n",
            SlotKind::Emissive => "Emissive _g",
            SlotKind::Height => "Height _p",
            SlotKind::Rmaos => "RMAOS",
            SlotKind::Coat => "Coat _cnr",
            SlotKind::Fuzz => "Fuzz _f",
            SlotKind::Subsurface => "Subsurface _s",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SlotFill {
    Empty,
    Own { item: String },
    /// A finished output of set `from`, used here through a slot command.
    Borrowed { from: String, item: String },
}

#[derive(Debug, Clone, PartialEq)]
pub struct SetView {
    /// The engine key (`""` for a created set with no members yet).
    pub key: String,
    pub id: String,
    pub label: String,
    /// Item keys of every member, packed outputs and raw sources included.
    pub members: Vec<String>,
    pub slots: BTreeMap<SlotKind, SlotFill>,
    pub pbr: bool,
    pub created: bool,
    /// `add`/`remove` entries of `sdc-project.toml` that match no file (Review focus 1).
    pub missing: Vec<String>,
    /// Slots whose finished output this set owns while another set borrows it.
    pub owns: Vec<SlotKind>,
    pub derived_match: Option<String>,
    /// The output prefix, or why there is none (shown, and an export block for a PBR set on CS).
    pub prefix: Option<String>,
    pub prefix_error: Option<String>,
    /// The set's own JSON settings.
    pub settings: Settings,
}

pub fn slot_of(item: &TextureItem) -> Option<SlotKind> {
    if matches!(item.status, ItemStatus::PackedInto(_)) {
        return None;
    }
    let st = item.job.as_ref()?.profile.subtype;
    Some(match st {
        Subtype::PbrAlbedo | Subtype::Diffuse | Subtype::LandscapeDiffuse => SlotKind::Diffuse,
        Subtype::PbrNormal | Subtype::Normal | Subtype::LandscapeNormal | Subtype::Msn => SlotKind::Normal,
        Subtype::PbrEmissive | Subtype::Glow => SlotKind::Emissive,
        Subtype::Height => SlotKind::Height,
        Subtype::Rmaos | Subtype::Rfaos => SlotKind::Rmaos,
        Subtype::CoatNormal => SlotKind::Coat,
        Subtype::PbrFuzz => SlotKind::Fuzz,
        Subtype::PbrSubsurface => SlotKind::Subsurface,
        _ => return None,
    })
}

/// Why `incoming` cannot join a set holding `existing`, if it cannot (spec §4.1: `_cnr` and `_f` share slot 7).
pub fn slot_conflict(existing: &[SlotKind], incoming: SlotKind) -> Option<String> {
    let clash = match incoming {
        SlotKind::Coat => existing.contains(&SlotKind::Fuzz),
        SlotKind::Fuzz => existing.contains(&SlotKind::Coat),
        _ => false,
    };
    clash.then(|| "a set cannot hold both a coat normal (_cnr) and a fuzz map (_f): both use slot 7".to_string())
}

fn entry_of<'a>(entries: &'a [SetEntry], id: &str) -> Option<&'a SetEntry> {
    entries.iter().find(|e| e.id.eq_ignore_ascii_case(id))
}

fn dir_of(key: &str) -> &str {
    key.rsplit_once('/').map_or("", |(d, _)| d)
}

/// Every set, in display order, with its slots, ownership and prefix.
pub fn set_views(m: &AppModel) -> Vec<SetView> {
    let root = m.export_root();
    let entries = m.sets_doc.as_ref().map(|d| d.entries()).unwrap_or_default();
    let defaults = m.sets_doc.as_ref().map(|d| d.defaults()).unwrap_or_default();
    let order = m.sets_doc.as_ref().map(|d| d.order()).unwrap_or_default();
    let item = |k: &str| m.items.iter().find(|i| i.key == k);

    let mut views: Vec<SetView> = m
        .sets
        .iter()
        .map(|s| {
            let id = set_id(&root, &s.key);
            let mut slots = BTreeMap::new();
            for mk in &s.members {
                if let Some(slot) = item(mk).and_then(slot_of) {
                    slots.entry(slot).or_insert(SlotFill::Own { item: mk.clone() });
                }
            }
            SetView {
                key: s.key.clone(),
                label: id.clone(),
                id,
                members: s.members.clone(),
                slots,
                pbr: false,
                created: false,
                missing: vec![],
                owns: vec![],
                derived_match: None,
                prefix: None,
                prefix_error: None,
                settings: Settings::new(),
            }
        })
        .collect();
    // Created sets that have no members yet.
    for e in entries.iter().filter(|e| e.created) {
        if !views.iter().any(|v| v.id.eq_ignore_ascii_case(&e.id)) {
            views.push(SetView {
                key: String::new(),
                label: e.id.clone(),
                id: e.id.clone(),
                members: vec![],
                slots: BTreeMap::new(),
                pbr: false,
                created: true,
                missing: vec![],
                owns: vec![],
                derived_match: None,
                prefix: None,
                prefix_error: None,
                settings: Settings::new(),
            });
        }
    }
    // Order: the saved order first, then the rest by id.
    views.sort_by(|a, b| {
        let pos = |v: &SetView| order.iter().position(|o| o.eq_ignore_ascii_case(&v.id)).unwrap_or(usize::MAX);
        pos(a).cmp(&pos(b)).then_with(|| a.id.cmp(&b.id))
    });
    // Entry data: created flag, settings, missing members, borrowed slots.
    let snapshot: Vec<(String, BTreeMap<SlotKind, SlotFill>)> = views.iter().map(|v| (v.id.clone(), v.slots.clone())).collect();
    for v in &mut views {
        let Some(e) = entry_of(&entries, &v.id) else { continue };
        v.created = e.created;
        v.settings = e.settings.clone();
        for f in e.add.iter().chain(&e.remove) {
            if !m.items.iter().any(|i| path_key_matches(f, &i.key)) {
                v.missing.push(f.clone());
            }
        }
        for (slot, from) in &e.borrow {
            let Some((_, src)) = snapshot.iter().find(|(id, _)| id.eq_ignore_ascii_case(from)) else {
                v.missing.push(format!("slot{slot} from {from}"));
                continue;
            };
            let hit = src.iter().find(|(k, f)| k.slot_number() == *slot && matches!(f, SlotFill::Own { .. }));
            if let Some((kind, SlotFill::Own { item })) = hit {
                v.slots.insert(*kind, SlotFill::Borrowed { from: from.clone(), item: item.clone() });
            }
        }
    }
    // PBR, derived match, prefix.
    for v in &mut views {
        let diffuse = match v.slots.get(&SlotKind::Diffuse) {
            Some(SlotFill::Own { item: k } | SlotFill::Borrowed { item: k, .. }) => item(k),
            _ => None,
        };
        v.pbr = diffuse.and_then(|i| i.job.as_ref()).is_some_and(|j| j.profile.subtype == Subtype::PbrAlbedo);
        v.derived_match = diffuse.map(|i| {
            let stem = i.job.as_ref().and_then(|j| j.output.file_stem()).map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
            derive_match_diffuse(dir_of(&i.key), &stem)
        });
        let eff = effective(&defaults, &v.settings);
        match prefix_of(&eff, v.derived_match.as_deref().unwrap_or("")) {
            Ok(p) => v.prefix = Some(p),
            Err(e) => v.prefix_error = Some(e.0),
        }
    }
    // Ownership marks.
    let owners = owners(&views);
    for v in &mut views {
        v.owns = owners
            .iter()
            .filter(|((_, users), owner)| owner.as_deref() == Some(v.id.as_str()) && users.len() > 1)
            .filter_map(|((item, _), _)| v.slots.iter().find(|(_, f)| matches!(f, SlotFill::Own { item: i } | SlotFill::Borrowed { item: i, .. } if i == item)).map(|(k, _)| *k))
            .collect();
    }
    views
}

/// For each used output item: (item, the ids of the sets using it in display order) → the owner id, the first PBR
/// set among them (spec §3.3); `None` when no user is a PBR set.
fn owners(views: &[SetView]) -> Vec<((String, Vec<String>), Option<String>)> {
    let mut users: BTreeMap<String, Vec<(String, bool)>> = BTreeMap::new();
    for v in views {
        for f in v.slots.values() {
            if let SlotFill::Own { item } | SlotFill::Borrowed { item, .. } = f {
                users.entry(item.clone()).or_default().push((v.id.clone(), v.pbr));
            }
        }
    }
    users
        .into_iter()
        .map(|(item, us)| {
            let owner = us.iter().find(|(_, pbr)| *pbr).map(|(id, _)| id.clone());
            ((item, us.into_iter().map(|(id, _)| id).collect()), owner)
        })
        .collect()
}

fn job_suffix(i: &TextureItem) -> String {
    i.job
        .as_ref()
        .and_then(|j| j.output.file_stem())
        .map(|s| s.to_string_lossy().rsplit_once('_').map_or(String::new(), |(_, x)| x.to_lowercase()))
        .unwrap_or_default()
}

/// The engine inputs of the current documents and plan (spec §2.1, DECISIONS P1–P2).
pub fn engine_inputs(m: &AppModel) -> (Vec<SetEdit>, BTreeMap<OutputKey, PathBuf>) {
    let entries = m.sets_doc.as_ref().map(|d| d.entries()).unwrap_or_default();
    let mut edits = vec![];
    for e in entries.iter().filter(|e| e.created) {
        edits.push(SetEdit::Create { set: e.id.clone() });
    }
    for e in &entries {
        if let Some(from) = &e.renamed_from {
            edits.push(SetEdit::Rename { from: from.clone(), to: e.id.clone() });
        }
    }
    for e in &entries {
        for f in &e.add {
            edits.push(SetEdit::Move { file: f.clone(), to: e.id.clone() });
        }
    }
    for e in &entries {
        for f in &e.remove {
            edits.push(SetEdit::Remove { file: f.clone() });
        }
    }
    let mut paths = BTreeMap::new();
    if m.target != Target::Cs {
        return (edits, paths);
    }
    let views = set_views(m);
    let mut seen = std::collections::BTreeSet::new();
    for (_, key, rel) in output_candidates(m, &views) {
        // Two outputs on one path: keep the first in display order. Export is blocked by `path_collisions` (Task 11),
        // and the engine never sees the collision, so the plan is stable. (Sending both would make the engine skip
        // both as IN-DUPLICATE, the sets would stop being PBR sets, the paths would vanish, and the two-pass planning
        // would flip between two plans.)
        if seen.insert(rel.to_string_lossy().replace('\\', "/").to_lowercase()) {
            paths.insert(key, rel);
        }
    }
    (edits, paths)
}

/// Every owned output's explicit path, sorted by its owner's display position: (owner id, key, path relative to
/// the export root). Duplicate paths are kept here; `engine_inputs` drops them and `path_collisions` reports them.
pub fn output_candidates(m: &AppModel, views: &[SetView]) -> Vec<(String, OutputKey, PathBuf)> {
    let pos = |id: &str| views.iter().position(|v| v.id == id).unwrap_or(usize::MAX);
    let mut out = vec![];
    for ((item_key, users), owner) in owners(views) {
        let Some(owner) = owner else { continue };
        let Some(ov) = views.iter().find(|v| v.id == owner) else { continue };
        let Some(prefix) = &ov.prefix else { continue };
        let Some(item) = m.items.iter().find(|i| i.key == item_key) else { continue };
        let Some(slot) = slot_of(item) else { continue };
        let rel = output_rel(prefix, slot.suffix());
        // The set holding the item (its source set) names a packed output.
        let source = users
            .iter()
            .find(|id| views.iter().any(|v| &v.id == *id && matches!(v.slots.get(&slot), Some(SlotFill::Own { item }) if *item == item_key)))
            .cloned()
            .unwrap_or(owner.clone());
        let key = if item.kind == ItemKind::Packed {
            OutputKey::Packed { set: source, suffix: job_suffix(item) }
        } else {
            OutputKey::File(item.key.clone())
        };
        out.push((owner, key, rel));
    }
    out.sort_by_key(|(o, _, _)| pos(o));
    out
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DropChoice {
    Move,
    Share,
    Duplicate,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DropOutcome {
    Joined,
    AskReplace { existing: String },
    AskChoice { from: String },
    Refused(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SlotCommand {
    FromSet,
    Path(String),
    Clear,
}

/// Match-related keys change output paths, so editing them re-plans (DECISIONS P7).
fn replans(key: &str) -> bool {
    key.starts_with("match") || key == "path_contains" || key == "rename"
}

impl AppModel {
    fn view_of(&self, id: &str) -> Option<SetView> {
        set_views(self).into_iter().find(|v| v.id.eq_ignore_ascii_case(id))
    }

    fn set_of_item(&self, item: &str) -> Option<SetView> {
        set_views(self).into_iter().find(|v| v.members.iter().any(|k| k == item))
    }

    fn sets_editable(&self) -> bool {
        self.sets_doc.as_ref().is_some_and(|d| !d.read_only())
    }

    /// A texture dropped on set `set` (spec §3.1, §3.3).
    pub fn drop_on_set(&mut self, item: &str, set: &str) -> DropOutcome {
        if !self.sets_editable() {
            return DropOutcome::Refused("sdc-project.toml is read-only".into());
        }
        let Some(target) = self.view_of(set) else {
            return DropOutcome::Refused(format!("no set {set}"));
        };
        let from = self.set_of_item(item);
        if from.as_ref().is_some_and(|f| f.id == target.id) {
            return DropOutcome::Joined;
        }
        let incoming = self.item(item).and_then(slot_of);
        if let Some(why) = incoming.and_then(|s| slot_conflict(&target.slots.keys().copied().collect::<Vec<_>>(), s)) {
            return DropOutcome::Refused(why);
        }
        // A texture that belongs to a real set (more than itself, or one the user touched) asks what to do.
        if let Some(f) = &from {
            let edited = self.sets_doc.as_ref().is_some_and(|d| entry_of(&d.entries(), &f.id).is_some());
            if f.members.len() > 1 || edited || f.created {
                return DropOutcome::AskChoice { from: f.id.clone() };
            }
        }
        self.drop_choice(item, set, DropChoice::Move)
    }

    /// The answer to `AskChoice` (or a plain move). `Move` may still ask to replace an occupied slot.
    pub fn drop_choice(&mut self, item: &str, set: &str, choice: DropChoice) -> DropOutcome {
        let Some(target) = self.view_of(set) else {
            return DropOutcome::Refused(format!("no set {set}"));
        };
        let slot = self.item(item).and_then(slot_of);
        match choice {
            DropChoice::Move => {
                if let Some(SlotFill::Own { item: existing } | SlotFill::Borrowed { item: existing, .. }) =
                    slot.and_then(|s| target.slots.get(&s)).cloned()
                {
                    return DropOutcome::AskReplace { existing };
                }
                let (id, it) = (target.id.clone(), item.to_string());
                self.edit_docs(&format!("Move into {id}"), None, true, move |_, s| s.add_member(&id, &it));
                DropOutcome::Joined
            }
            DropChoice::Share => {
                let Some(slot) = slot else {
                    return DropOutcome::Refused(
                        "only a finished texture can be shared; duplicate the source to use it in two sets".into(),
                    );
                };
                let Some(from) = self.set_of_item(item) else {
                    return DropOutcome::Refused("the texture is in no set".into());
                };
                let (id, n, fid) = (target.id.clone(), slot.slot_number(), from.id.clone());
                self.edit_docs(&format!("Share into {id}"), None, true, move |_, s| s.set_borrow(&id, n, Some(&fid)));
                DropOutcome::Joined
            }
            DropChoice::Duplicate => match self.duplicate_into(item, &target.id) {
                Ok(()) => DropOutcome::Joined,
                Err(e) => DropOutcome::Refused(e),
            },
        }
    }

    /// The answer to `AskReplace`: the occupant leaves (it becomes a set of its own), `item` moves in. One step.
    pub fn drop_replace(&mut self, item: &str, set: &str) {
        let Some(target) = self.view_of(set) else { return };
        let slot = self.item(item).and_then(slot_of);
        let existing = slot.and_then(|s| target.slots.get(&s)).cloned();
        let (id, it) = (target.id.clone(), item.to_string());
        self.edit_docs(&format!("Replace in {id}"), None, true, move |_, s| {
            match existing {
                Some(SlotFill::Own { item: old }) => s.remove_member(&id, &old),
                Some(SlotFill::Borrowed { .. }) => {
                    if let Some(n) = slot.map(SlotKind::slot_number) {
                        s.set_borrow(&id, n, None);
                    }
                }
                _ => {}
            }
            s.add_member(&id, &it);
        });
    }

    /// A texture dropped on the empty area: a new set named after its base name (spec §3.1). Returns the new id.
    pub fn drop_new_set(&mut self, item: &str) -> String {
        let stem = std::path::Path::new(item).file_stem().unwrap_or_default().to_string_lossy().into_owned();
        // Exporter vocabulary first (`rock01_roughness` → `rock01`), then the Skyrim suffixes (`rock01_n`).
        let base = engine::group::split_vocab(&stem).map_or_else(|| engine::group::split_suffix(&stem).0, |(b, _)| b);
        let dir = dir_of(item);
        let first = if dir.is_empty() { base.clone() } else { format!("{dir}/{base}") };
        let views = set_views(self);
        let mut id = first.clone();
        let mut n = 2;
        while views.iter().any(|v| v.id.eq_ignore_ascii_case(&id)) {
            id = format!("{first}-{n}");
            n += 1;
        }
        let (i, it) = (id.clone(), item.to_string());
        self.edit_docs(&format!("New set {id}"), None, true, move |_, s| {
            s.create_set(&i);
            s.add_member(&i, &it);
        });
        id
    }

    /// Copy `item`'s source next to it under set `set`'s base name and move the copy in (spec §3.4).
    fn duplicate_into(&mut self, item: &str, set: &str) -> Result<(), String> {
        let src = self.item(item).map(|i| i.input.clone()).ok_or("no such texture")?;
        let root = self.root.clone().ok_or("no folder open")?;
        let stem = src.file_stem().unwrap_or_default().to_string_lossy().into_owned();
        let ext = src.extension().map(|e| e.to_string_lossy().into_owned()).unwrap_or_default();
        // Exporter vocabulary first (`rock01_roughness` → `rock01`), then the Skyrim suffixes (`rock01_n`).
        let base = engine::group::split_vocab(&stem).map_or_else(|| engine::group::split_suffix(&stem).0, |(b, _)| b);
        let tail = &stem[base.len()..]; // e.g. "_roughness"
        let set_base = set.rsplit('/').next().unwrap_or(set).to_string();
        let dir = src.parent().ok_or("the source has no folder")?.to_path_buf();
        let name = |b: &str| dir.join(format!("{b}{tail}.{ext}"));
        let mut dst = name(&set_base);
        let mut n = 2;
        while dst.exists() {
            dst = name(&format!("{set_base}-{n}"));
            n += 1;
        }
        std::fs::copy(&src, &dst).map_err(|e| format!("could not copy {}: {e}", src.display()))?;
        let sha256 = sha256_of(&dst).ok_or("could not read the copy")?;
        let key = dst
            .strip_prefix(&root)
            .map(|p| p.to_string_lossy().replace('\\', "/"))
            .map_err(|_| "the copy is outside the folder".to_string())?;
        let id = set.to_string();
        self.edit_docs(&format!("Duplicate into {id}"), None, true, move |_, s| s.add_member(&id, &key));
        if let Some(last) = self.history.undo.last_mut() {
            last.created_file = Some(CreatedFile { path: dst, source: src, sha256 });
        }
        Ok(())
    }

    pub fn reorder_set(&mut self, id: &str, to: usize) {
        let mut ids: Vec<String> = set_views(self).into_iter().map(|v| v.id).collect();
        let Some(from) = ids.iter().position(|i| i.eq_ignore_ascii_case(id)) else { return };
        let moved = ids.remove(from);
        ids.insert(to.min(ids.len()), moved);
        // Order changes ownership, so output paths (P7): plan again.
        self.edit_docs("Reorder sets", None, true, move |_, s| s.set_order(&ids));
    }

    /// Rename set `id` to `new_id`; refused (returns the reason) when the name is unusable or taken.
    pub fn rename_set(&mut self, id: &str, new_id: &str) -> Result<(), String> {
        let new_id = new_id.trim().trim_matches('/').replace('\\', "/");
        check_rel_path(&new_id, "set name").map_err(|e| e.0)?;
        if set_views(self).iter().any(|v| v.id.eq_ignore_ascii_case(&new_id)) {
            return Err(format!("a set {new_id} already exists"));
        }
        let (a, b) = (id.to_string(), new_id.clone());
        self.edit_docs(&format!("Rename {id}"), None, true, move |_, s| s.rename_set(&a, &b));
        Ok(())
    }

    /// Delete a set the user created (DECISIONS P2–P3); false (nothing done) for an automatic set.
    pub fn delete_set(&mut self, id: &str) -> bool {
        if !self.view_of(id).is_some_and(|v| v.created) {
            return false;
        }
        let i = id.to_string();
        self.edit_docs(&format!("Delete {id}"), None, true, move |_, s| s.delete_set(&i));
        true
    }

    pub fn remove_member(&mut self, id: &str, item: &str) {
        let (i, it) = (id.to_string(), item.to_string());
        self.edit_docs(&format!("Remove from {id}"), None, true, move |_, s| s.remove_member(&i, &it));
    }

    /// A JSON value of set `set`, or of the defaults with `None`. Material values do not plan; match fields do (P7).
    pub fn set_json_value(&mut self, set: Option<&str>, key: &str, v: Option<Value>) {
        let replan = replans(key);
        let (s_id, k) = (set.map(str::to_string), key.to_string());
        let merge = Some(format!("{}:{key}", set.unwrap_or("defaults")));
        self.edit_docs(&format!("Change {key}"), merge, replan, move |_, d| match &s_id {
            Some(id) => d.set_value(id, &k, v),
            None => d.set_default(&k, v),
        });
    }

    /// A slot command (spec §4.2). A path is checked first; the reason comes back when it is refused.
    pub fn set_slot_command(&mut self, set: &str, slot: u8, cmd: SlotCommand) -> Result<(), String> {
        let key = format!("slot{slot}");
        let v = match cmd {
            SlotCommand::FromSet => None,
            SlotCommand::Clear => Some(Value::from("")),
            SlotCommand::Path(p) => {
                check_rel_path(&p, &key).map_err(|e| e.0)?;
                if !p.to_lowercase().ends_with(".dds") {
                    return Err(format!("{key}: the path must end in .dds"));
                }
                Some(Value::from(p.replace('/', "\\")))
            }
        };
        self.set_json_value(Some(set), &key, v);
        Ok(())
    }
}

// Keep `Dialog` imported for the duplicate-undo path in mod.rs; remove this line if unused.
#[allow(unused_imports)]
use Dialog as _;
```

Drop the final `use Dialog as _` and its `#[allow]` if nothing needs `Dialog` here; it only exists so the import list compiles while you work.

- [ ] **Step 4: Wire `request()` and two-pass planning in `crates/gui/src/model/mod.rs`.**
  - Add the fields `last_inputs: Option<(Vec<engine::sets::SetEdit>, BTreeMap<engine::sets::OutputKey, PathBuf>)>` and `auto_replans: u8` (initialise them in `placeholder`).
  - In `request()`, after `req.mem_budget = …`:

    ```rust
    let (edits, paths) = sets::engine_inputs(self);
    req.set_edits = edits;
    req.output_paths = paths;
    ```

  - In `replan()`, record the inputs used: `self.last_inputs = Some((req.set_edits.clone(), req.output_paths.clone()));`. Bind `req` before `spawn_plan` consumes it.
  - In `edit_docs`, set `self.auto_replans = 0;` when a step is recorded.
  - At the end of `apply_plan`, **before** the `export_after_plan` block:

    ```rust
    // Output paths depend on this plan's sub-types (DECISIONS P1): plan once more when they changed. Bounded.
    let now = sets::engine_inputs(self);
    if self.last_inputs.as_ref() != Some(&now) && self.auto_replans < 2 {
        self.auto_replans += 1;
        self.replan();
        return;
    }
    self.auto_replans = 0;
    ```

    The `export_after_plan` flag survives the `return`. Export starts when the final plan lands.
  - In `open_folder`, reset `self.last_inputs = None; self.auto_replans = 0;`.

- [ ] **Step 5: Run the tests.**

Run: `cargo test -p sdc-gui --test sets_model && cargo test -p sdc-gui`
Expected: all pass.

`pbr_opened` decides rock01 as `pbr_albedo`, so `rock.pbr` holds by P5. If an assertion about slots fails, print `m.items` (name, `job.profile.subtype`, status). Fix the mapping in `slot_of`, not the expectation, unless the sub-type itself differs from the table above.

- [ ] **Step 6: Run the gates and commit.**

```bash
cargo fmt --all && cargo clippy --workspace --all-targets -- -D warnings && cargo test -p sdc-gui
git add crates/gui/src/model/sets.rs crates/gui/src/model/mod.rs crates/gui/tests/sets_model.rs
git commit -m "feat(gui): set views, shared-slot ownership, engine set edits and output paths, drop rules (M3b §3, §6.1)

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

---
### Task 11: Export checks (spec §6.3)

**Files:**
- Create: `crates/gui/src/model/pbr_export.rs`
- Modify:
  - `crates/gui/src/model/mod.rs`: `pub mod pbr_export;`; `Dialog::ExportBlocked`, `Dialog::ExportWarnings`; `export_checks` runs the new checks first; `proceed_with_warnings()`;
  - `crates/gui/src/ui/dialogs.rs`: the two dialogs, with clickable issues.
- Test: `crates/gui/tests/pbr_export.rs`

**Interfaces:**
- Consumes:
  - `sets::{set_views, SetView, SlotKind, SlotFill, engine_inputs}`;
  - `pgp::settings::{effective, feature_on, Feature, Settings}`;
  - `pgp::prefix::{pitfalls, check_rel_path}`;
  - `pgp::writer::{document, EntryInput}`;
  - `pgp::validate::validate`.
- Produces:
  - `#[derive(Debug, Clone, PartialEq, Eq)] pub struct ExportIssue { pub set_id: String, pub set_key: String, pub text: String }`;
  - `fn export_blocks(m: &AppModel) -> Vec<ExportIssue>`, `fn path_collisions(m: &AppModel) -> Vec<ExportIssue>` and `fn export_asks(m: &AppModel) -> Vec<ExportIssue>`;
  - `fn json_entries(m: &AppModel) -> Vec<(SetView, EntryInput)>` (also used by Task 12);
  - `fn json_name(m: &AppModel) -> String`;
  - `Dialog::{ExportBlocked(Vec<ExportIssue>), ExportWarnings(Vec<ExportIssue>)}`;
  - `AppModel::proceed_with_warnings()`.

**What blocks export.** All of these apply on CS, to PBR sets. Slot clashes apply to every set.

1. **A feature is on but its texture is missing.**

   | Feature | Missing slot |
   |---|---|
   | emissive | Emissive `_g` |
   | parallax | Height `_p` |
   | subsurface | Subsurface `_s` |
   | `multilayer` with `coat_normal` | Coat `_cnr` |
   | fuzz with `fuzz.texture` | Fuzz `_f` |

   Text: `"<set>: emissive is on but the set has no emissive texture (_g)"`.
2. **A slot clash.**
   - The set holds both Coat and Fuzz: `"<set>: _cnr and _f both use slot 7"`.
   - Two mutually exclusive features are on through defaults plus own values: `"<set>: hair and parallax can't both be on"`. Use `blocked_by` on the effective settings.
3. **Two outputs with one path.** Two owned outputs (`sets::output_candidates`) map to the same path, compared case-insensitively: `"<set A> and <set B> both write <path> (<key A> and <key B>)"`. `engine_inputs` sends only the first, so the engine never sees the collision (Review focus 2).
4. **The prefix or a path is unusable.**
   - A PBR set whose prefix is an error: `"<set>: <prefix_error>"`.
   - A `slotN` setting that is not empty and fails `check_rel_path`.
   - The JSON name contains a character Windows doesn't allow.
5. **The JSON fails schema validation:** `"PGPatcher JSON does not validate: <first error>"`.

**What asks "Proceed anyway / Cancel".**
- `pitfalls(effective settings)` of every PBR set: `"<set>: <warning>"`.
- The existing undecided-texture check (M3a N3) follows as before.

- [ ] **Step 1: Write the failing tests.** Create `crates/gui/tests/pbr_export.rs`:

```rust
mod support;
use sdc_gui::model::pbr_export::*;
use sdc_gui::model::Dialog;
use serde_json::json;
use std::time::Duration;
use support::*;

fn ready(d: &std::path::Path) -> sdc_gui::model::AppModel {
    let mut m = pbr_opened(d);
    assert!(m.wait_idle(Duration::from_secs(60)));
    m
}

#[test]
fn a_clean_project_has_no_blocks() {
    let d = tempfile::tempdir().unwrap();
    let m = ready(d.path());
    assert_eq!(export_blocks(&m), vec![]);
}

#[test]
fn a_feature_without_its_texture_blocks() {
    let d = tempfile::tempdir().unwrap();
    let mut m = ready(d.path());
    m.set_json_value(Some("landscape/rock01"), "emissive", Some(json!(true)));
    let b = export_blocks(&m);
    assert!(b.iter().any(|i| i.set_id == "landscape/rock01" && i.text.contains("emissive")), "{b:?}");
    m.export_clicked();
    assert!(matches!(m.dialog, Some(Dialog::ExportBlocked(_))));
}

#[test]
fn exclusive_features_through_defaults_block() {
    let d = tempfile::tempdir().unwrap();
    let mut m = ready(d.path());
    m.set_json_value(None, "hair", Some(json!(true)));
    m.set_json_value(Some("landscape/rock01"), "parallax", Some(json!(true)));
    let b = export_blocks(&m);
    assert!(b.iter().any(|i| i.text.contains("hair") && i.text.contains("parallax")), "{b:?}");
}

#[test]
fn a_bad_rename_blocks_and_names_the_field() {
    let d = tempfile::tempdir().unwrap();
    let mut m = ready(d.path());
    m.set_json_value(Some("landscape/rock01"), "rename", Some(json!(r"..\escape")));
    assert!(m.wait_idle(Duration::from_secs(60)));
    let b = export_blocks(&m);
    assert!(b.iter().any(|i| i.text.contains("rename") && i.text.contains("..")), "{b:?}");
}

#[test]
fn two_sets_deriving_one_prefix_block() {
    let d = tempfile::tempdir().unwrap();
    let mut m = ready(d.path());
    // Point leaf's set at rock01's match: both diffuse outputs land on rock01.dds.
    let leaf = m.items.iter().find(|i| i.name == "leaf.png").unwrap().key.clone();
    m.set_field(&leaf, "subtype", json!("pbr_albedo"));
    assert!(m.wait_idle(Duration::from_secs(60)));
    m.set_json_value(Some("landscape/leaf"), "match_diffuse", Some(json!(r"landscape\rock01")));
    assert!(m.wait_idle(Duration::from_secs(60)));
    let b = export_blocks(&m);
    assert!(b.iter().any(|i| i.text.contains("both write")), "{b:?}");
}

#[test]
fn pitfalls_ask_and_proceeding_continues_to_the_undecided_check() {
    let d = tempfile::tempdir().unwrap();
    let mut m = ready(d.path());
    m.set_json_value(Some("landscape/rock01"), "match8", Some(json!(r"x\leaf_s.dds")));
    assert!(m.wait_idle(Duration::from_secs(60)));
    assert!(!export_asks(&m).is_empty());
    m.export_clicked();
    assert!(matches!(m.dialog, Some(Dialog::ExportWarnings(_))), "{:?}", m.dialog);
    m.proceed_with_warnings();
    // The fixture has undecided textures: the M3a question comes next.
    assert!(matches!(m.dialog, Some(Dialog::ProceedAnyway(_))), "{:?}", m.dialog);
}

#[test]
fn the_json_of_the_fixture_validates_and_names_the_folder() {
    let d = tempfile::tempdir().unwrap();
    let m = ready(d.path());
    let entries = json_entries(&m);
    assert!(entries.iter().any(|(v, _)| v.id == "landscape/rock01"));
    let doc = sdc_gui::pgp::writer::document(&Default::default(), &entries.iter().map(|(_, e)| e.clone()).collect::<Vec<_>>());
    sdc_gui::pgp::validate::validate(&doc.to_value()).unwrap();
    assert_eq!(json_name(&m), "textures", "the opened folder's name");
}
```

- [ ] **Step 2: Run the tests and confirm they fail.**

Run: `cargo test -p sdc-gui --test pbr_export`
Expected: compile errors.

- [ ] **Step 3: Create `crates/gui/src/model/pbr_export.rs`:**

```rust
//! Export of PBR sets (M3b spec §6): the checks that block or ask, the JSON entries, and (Task 12) writing the JSON
//! and the vanilla/CPM copies. No egui types.
use super::sets::{engine_inputs, set_views, SetView, SlotFill, SlotKind};
use super::AppModel;
use crate::pgp::prefix::{check_rel_path, pitfalls};
use crate::pgp::settings::{blocked_by, effective, feature_on, Feature, Settings};
use crate::pgp::validate::validate;
use crate::pgp::writer::{document, EntryInput};
use engine::config::Target;
use serde_json::Value;
use std::collections::BTreeMap;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExportIssue {
    pub set_id: String,
    /// The engine key, for selecting the set (empty for a created set with no members).
    pub set_key: String,
    pub text: String,
}

fn issue(v: &SetView, text: String) -> ExportIssue {
    ExportIssue { set_id: v.id.clone(), set_key: v.key.clone(), text }
}

fn defaults(m: &AppModel) -> Settings {
    m.sets_doc.as_ref().map(|d| d.defaults()).unwrap_or_default()
}

/// The JSON file name (without `.json`): the project's, else the opened folder's name.
pub fn json_name(m: &AppModel) -> String {
    m.sets_doc
        .as_ref()
        .and_then(|d| d.json_name())
        .filter(|n| !n.trim().is_empty())
        .or_else(|| m.root.as_ref().and_then(|r| r.file_name()).map(|n| n.to_string_lossy().into_owned()))
        .unwrap_or_else(|| "sdc".into())
}

/// The final slot paths of a set: borrowed slots point at the owner's file; explicit `slotN` settings win.
fn slots_of(v: &SetView, paths: &BTreeMap<engine::sets::OutputKey, std::path::PathBuf>, m: &AppModel) -> BTreeMap<u8, String> {
    let mut out = BTreeMap::new();
    for (kind, fill) in &v.slots {
        let SlotFill::Borrowed { item, .. } = fill else { continue };
        let Some(it) = m.item(item) else { continue };
        let norm = |p: &std::path::Path| p.to_string_lossy().replace('\\', "/").to_lowercase();
        let path = paths.iter().find_map(|(k, p)| match k {
            engine::sets::OutputKey::File(f) if *f == it.key => Some(p.clone()),
            engine::sets::OutputKey::Packed { .. } if it.kind == super::items::ItemKind::Packed => {
                // A packed item's path is the one its job writes (after the two-pass plan the job carries it): the
                // whole path, so two sets with the same base name in different folders cannot be confused.
                let job_out = it.job.as_ref().map(|j| norm(&j.output));
                (job_out == Some(norm(&m.export_root().join(p)))).then(|| p.clone())
            }
            _ => None,
        });
        if let Some(p) = path {
            out.insert(kind.slot_number(), p.to_string_lossy().replace('/', "\\"));
        }
    }
    for n in 1..=8u8 {
        if let Some(Value::String(s)) = v.settings.get(&format!("slot{n}")) {
            out.insert(n, s.clone());
        }
    }
    out
}

/// Every PBR set's JSON entry, in display order (CS only; empty otherwise).
pub fn json_entries(m: &AppModel) -> Vec<(SetView, EntryInput)> {
    if m.target != Target::Cs {
        return vec![];
    }
    let (_, paths) = engine_inputs(m);
    set_views(m)
        .into_iter()
        .filter(|v| v.pbr)
        .map(|v| {
            let e = EntryInput {
                settings: v.settings.clone(),
                slots: slots_of(&v, &paths, m),
                derived_match: v.derived_match.clone().unwrap_or_default(),
            };
            (v, e)
        })
        .collect()
}

const NEEDS: [(Feature, SlotKind, &str, &str); 3] = [
    (Feature::Emissive, SlotKind::Emissive, "emissive", "an emissive texture (_g)"),
    (Feature::Parallax, SlotKind::Height, "parallax", "a height texture (_p)"),
    (Feature::Subsurface, SlotKind::Subsurface, "subsurface", "a subsurface texture (_s)"),
];

fn has(v: &SetView, s: SlotKind) -> bool {
    !matches!(v.slots.get(&s), None | Some(SlotFill::Empty))
}

pub fn export_blocks(m: &AppModel) -> Vec<ExportIssue> {
    let mut out = vec![];
    let d = defaults(m);
    let views = set_views(m);
    for v in &views {
        if has(v, SlotKind::Coat) && has(v, SlotKind::Fuzz) {
            out.push(issue(v, format!("{}: _cnr and _f both use slot 7", v.id)));
        }
        if m.target != Target::Cs || !v.pbr {
            continue;
        }
        let eff = effective(&d, &v.settings);
        for (f, slot, name, what) in NEEDS {
            if feature_on(&eff, f) && blocked_by(&eff, f).is_none() && !has(v, slot) {
                out.push(issue(v, format!("{}: {name} is on but the set has no {what}", v.id)));
            }
        }
        if feature_on(&eff, Feature::Multilayer) && eff.get("coat_normal") == Some(&Value::Bool(true)) && !has(v, SlotKind::Coat) {
            out.push(issue(v, format!("{}: coat normal is on but the set has no coat normal texture (_cnr)", v.id)));
        }
        if feature_on(&eff, Feature::Fuzz) && eff.get("fuzz.texture") == Some(&Value::Bool(true)) && !has(v, SlotKind::Fuzz) {
            out.push(issue(v, format!("{}: fuzz texture is on but the set has no fuzz texture (_f)", v.id)));
        }
        for (f, _) in crate::pgp::settings::FEATURE_KEYS {
            if let Some(b) = feature_on(&eff, f).then(|| blocked_by(&eff, f)).flatten() {
                if f < b {
                    out.push(issue(v, format!("{}: {} and {} can't both be on", v.id, name_of(f), name_of(b))));
                }
            }
        }
        if let Some(e) = &v.prefix_error {
            out.push(issue(v, format!("{}: {e}", v.id)));
        }
        for n in 1..=8u8 {
            if let Some(Value::String(p)) = v.settings.get(&format!("slot{n}")) {
                if !p.is_empty() {
                    if let Err(e) = check_rel_path(p, &format!("slot{n}")) {
                        out.push(issue(v, format!("{}: {}", v.id, e.0)));
                    }
                }
            }
        }
    }
    out.extend(path_collisions(m));
    if m.target == Target::Cs {
        let name = json_name(m);
        if let Err(e) = check_rel_path(&name, "JSON name") {
            out.push(ExportIssue { set_id: String::new(), set_key: String::new(), text: e.0 });
        }
        let entries: Vec<EntryInput> = json_entries(m).into_iter().map(|(_, e)| e).collect();
        if !entries.is_empty() {
            if let Err(errs) = validate(&document(&defaults(m), &entries).to_value()) {
                out.push(ExportIssue {
                    set_id: String::new(),
                    set_key: String::new(),
                    text: format!("PGPatcher JSON does not validate: {}", errs[0]),
                });
            }
        }
    }
    out
}

fn name_of(f: Feature) -> &'static str {
    crate::pgp::settings::feature_key(f)
}

/// Two owned outputs on one path, computed before `engine_inputs` drops the second (Task 10), so the engine never
/// sees the collision and Export is blocked here (Review focus 2).
pub fn path_collisions(m: &AppModel) -> Vec<ExportIssue> {
    if m.target != Target::Cs {
        return vec![];
    }
    let views = set_views(m);
    let name = |k: &engine::sets::OutputKey| match k {
        engine::sets::OutputKey::File(f) => f.clone(),
        engine::sets::OutputKey::Packed { set, suffix } => format!("{set} ({suffix})"),
    };
    let mut seen: BTreeMap<String, (String, engine::sets::OutputKey)> = BTreeMap::new();
    let mut out = vec![];
    for (owner, k, p) in super::sets::output_candidates(m, &views) {
        let lower = p.to_string_lossy().replace('\\', "/").to_lowercase();
        match seen.get(&lower) {
            Some((first_owner, first)) => {
                let v = views.iter().find(|v| v.id == owner);
                out.push(ExportIssue {
                    set_id: owner.clone(),
                    set_key: v.map(|v| v.key.clone()).unwrap_or_default(),
                    text: format!("{first_owner} and {owner} both write {} ({} and {})", p.display(), name(first), name(&k)),
                });
            }
            None => {
                seen.insert(lower, (owner, k));
            }
        }
    }
    out
}

pub fn export_asks(m: &AppModel) -> Vec<ExportIssue> {
    if m.target != Target::Cs {
        return vec![];
    }
    let d = defaults(m);
    set_views(m)
        .iter()
        .filter(|v| v.pbr)
        .flat_map(|v| pitfalls(&effective(&d, &v.settings)).into_iter().map(move |w| issue(v, format!("{}: {w}", v.id))))
        .collect()
}
```

The block "two outputs with one path" names the colliding keys. That is also Review focus 2: the engine reports `IN-DUPLICATE` for the same collision, and this check stops Export before it starts.

`path_collisions` must also block an owned output whose path equals the mirrored output of an item that has no explicit path (compare against each such item's `job_key`, case-folded with `/`). Task 10's `engine_inputs` dedupe compares owned candidates only, and the engine does not re-check redirected packed outputs (Task 10 review, Minor 8).

- [ ] **Step 4: Wire the dialogs.** In `crates/gui/src/model/mod.rs`, add `pub mod pbr_export;` and the `Dialog` variants:

```rust
    /// Export cannot start: these must be fixed first (M3b §6.3).
    ExportBlocked(Vec<pbr_export::ExportIssue>),
    /// Export can start after a "Proceed anyway" (M3b §6.3: matchN pitfalls).
    ExportWarnings(Vec<pbr_export::ExportIssue>),
```

In `export_checks`, between the export-folder check and the undecided check:

```rust
        let blocks = pbr_export::export_blocks(self);
        if !blocks.is_empty() {
            self.dialog = Some(Dialog::ExportBlocked(blocks));
            return;
        }
        let asks = pbr_export::export_asks(self);
        if !asks.is_empty() {
            self.dialog = Some(Dialog::ExportWarnings(asks));
            return;
        }
        self.undecided_check();
```

Move the existing undecided part (`let n = self.undecided(); … self.start_export();`) into `fn undecided_check(&mut self)`. Then add:

```rust
    /// The answer "Proceed anyway" to `ExportWarnings`: on to the undecided-texture question, then the export.
    pub fn proceed_with_warnings(&mut self) {
        self.dialog = None;
        self.undecided_check();
    }
```

In `crates/gui/src/ui/dialogs.rs`, add the two dialogs. Use the existing `modal` helper; each issue is a link.

```rust
        Dialog::ExportBlocked(issues) => export_issues(ctx, model, &issues, false),
        Dialog::ExportWarnings(issues) => export_issues(ctx, model, &issues, true),
```

```rust
fn export_issues(ctx: &egui::Context, model: &mut AppModel, issues: &[crate::model::pbr_export::ExportIssue], can_proceed: bool) {
    let (mut proceed, mut close, mut select) = (false, false, None);
    modal(ctx, if can_proceed { "export-warnings" } else { "export-blocked" }, |ui| {
        ui.heading(if can_proceed { "Check before exporting" } else { "Export is blocked" });
        ui.label(if can_proceed {
            "These may not do what you expect. PGPatcher's rules are applied as written; nothing is changed for you."
        } else {
            "Fix these first. Click one to select its set."
        });
        egui::ScrollArea::vertical().max_height(260.0).show(ui, |ui| {
            for i in issues {
                if i.set_key.is_empty() {
                    ui.label(&i.text);
                } else if ui.link(&i.text).clicked() {
                    select = Some(i.set_key.clone());
                }
            }
        });
        ui.horizontal(|ui| {
            if can_proceed {
                proceed = ui.button("Proceed anyway").clicked();
                close = ui.button("Cancel").clicked();
            } else {
                close = ui.button("Close").clicked();
            }
        });
    });
    if let Some(k) = select {
        model.dialog = None;
        model.select_set(&k);
    } else if proceed {
        model.proceed_with_warnings();
    } else if close {
        model.dialog = None;
    }
}
```

- [ ] **Step 5: Run the tests.**

Run: `cargo test -p sdc-gui --test pbr_export && cargo test -p sdc-gui`
Expected: all pass. M3a export tests still reach `ProceedAnyway`, because the fixture has no blocks.

**If `a_clean_project_has_no_blocks` fails,** the fixture has a real block. For example, rock01's prefix could collide with another set. Print the issues and fix the cause in the model, not the test.

- [ ] **Step 6: Run the gates and commit.**

```bash
cargo fmt --all && cargo clippy --workspace --all-targets -- -D warnings && cargo test -p sdc-gui
git add crates/gui/src/model/pbr_export.rs crates/gui/src/model/mod.rs crates/gui/src/ui/dialogs.rs crates/gui/tests/pbr_export.rs
git commit -m "feat(gui): export checks for PBR sets: blocks and Proceed-anyway warnings (M3b §6.3)

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

---

### Task 12: Write the JSON and the vanilla/CPM copies after export (spec §6.1, §6.2, §6.4, DECISIONS P4)

**Files:**
- Modify:
  - `crates/gui/src/model/pbr_export.rs`: `PendingJson`, `prepare`, `finish`;
  - `crates/gui/src/model/mod.rs`: `start_export` stores the pending JSON; `ExportDone` finishes it; `Dialog::OverwriteJson`; `resolve_overwrite_json`;
  - `crates/gui/src/model/export.rs`: `pub notes: Vec<String>` on `ExportModel`;
  - `crates/gui/src/ui/drawer.rs`: show the notes under the summary;
  - `crates/gui/src/ui/dialogs.rs`: the overwrite dialog.
- Test: `crates/gui/tests/pbr_export.rs` (append)

**Interfaces:**
- Consumes: Task 11's `json_entries`, `json_name`; `ExportModel::rows`, `RowState`; `history::sha256_of`; `SetsDoc::{last_json_sha256, set_last_json_sha256, save}`.
- Produces:
  - `pub struct PendingJson { pub path: PathBuf, pub entries: Vec<(String /*set id*/, Vec<String> /*job keys*/, EntryInput)>, pub defaults: Settings, pub copies: Vec<(PathBuf, PathBuf)> }`;
  - `fn prepare(m: &AppModel) -> Option<PendingJson>`;
  - `enum Finish { Written(String /*sha*/), Overwrite(String /*text*/), Nothing }`;
  - `fn finish(p: &PendingJson, rows: &[Row], last_sha: Option<&str>, notes: &mut Vec<String>) -> Finish`;
  - `AppModel.pending_json: Option<PendingJson>`;
  - `Dialog::OverwriteJson(PathBuf)`;
  - `AppModel::resolve_overwrite_json(overwrite: bool)`.

**Rules:**
- **When the JSON is written:** only after an export that ended `Ok`, with no cancelled files (`summary.cancelled.is_empty()`) and no `export.error`. Otherwise nothing is written and no copies are made, and the note `"PGPatcher JSON not written: the export was cancelled"` (or `…failed`) is added (Review focus 5).
- **Which entries go in:** a set's entry is included only when every job key it depends on has a row in state `Done { .. }` or `UpToDate`. A set depends on the outputs of its own slots and of its borrowed slots. Any other set gets `"not in JSON: <set> (<first row state label>)"`.
- **Overwriting:**
  - The document is the included entries, validated (Task 11 already blocked invalid ones; validate again, and on failure write nothing and add the note).
  - When the target file exists and its SHA-256 differs from `last_json_sha256` (or there is no record), ask with `Dialog::OverwriteJson(path)`. The rendered text waits in `self.pending_text`.
  - Otherwise write atomically (`<name>.json.sdc-tmp`, then rename).
- **After writing:** call `sets_doc.set_last_json_sha256(sha)` and save `sdc-project.toml` straight away, so the record matches the disk. A save error becomes the usual dialog.
- **Vanilla/CPM copies (P4):** after a successful export, for each borrowed slot, copy the owner's output DDS (`item.job.output`) to the borrowing set's path: `<borrower diffuse output dir>/<borrower diffuse stem>_<slot suffix>.dds`. The note is `"copied <from> → <to>"`. A missing source gets `"could not copy … (not exported)"`.

- [ ] **Step 1: Write the failing tests.** Append to `crates/gui/tests/pbr_export.rs`:

```rust
fn export_and_wait(m: &mut sdc_gui::model::AppModel) {
    m.export_clicked();
    if matches!(m.dialog, Some(Dialog::ProceedAnyway(_))) {
        m.proceed_anyway();
    }
    assert!(m.wait_idle(Duration::from_secs(120)), "export did not finish");
}

fn json_path(m: &sdc_gui::model::AppModel) -> std::path::PathBuf {
    m.export_root().join("PBRNIFPatcher").join(format!("{}.json", json_name(m)))
}

#[test]
fn export_writes_a_valid_json_and_records_its_hash() {
    let d = tempfile::tempdir().unwrap();
    let mut m = ready(d.path());
    m.set_json_value(Some("landscape/rock01"), "specular_level", Some(json!(0.05)));
    export_and_wait(&mut m);
    let p = json_path(&m);
    let text = std::fs::read_to_string(&p).unwrap_or_else(|e| panic!("{p:?}: {e}; notes {:?}", m.export.notes));
    let v: serde_json::Value = serde_json::from_str(&text).unwrap();
    sdc_gui::pgp::validate::validate(&v).unwrap();
    let e = v["entries"].as_array().unwrap().iter().find(|e| e["match_diffuse"] == json!(r"landscape\rock01")).unwrap();
    assert_eq!(e["specular_level"], json!(0.05));
    assert!(m.export_root().join("textures/pbr/landscape/rock01_rmaos.dds").exists());
    let sha = m.sets_doc.as_ref().unwrap().last_json_sha256().unwrap();
    assert_eq!(Some(sha), sdc_gui::model::history::sha256_of(&p));
    assert!(!m.is_dirty(), "the hash record was saved");
}

#[test]
fn a_hand_edited_json_asks_before_overwriting() {
    let d = tempfile::tempdir().unwrap();
    let mut m = ready(d.path());
    export_and_wait(&mut m);
    let p = json_path(&m);
    std::fs::write(&p, "{ \"edited\": true }\n").unwrap();
    m.set_json_value(Some("landscape/rock01"), "specular_level", Some(json!(0.06)));
    export_and_wait(&mut m);
    assert_eq!(m.dialog, Some(Dialog::OverwriteJson(p.clone())));
    assert!(std::fs::read_to_string(&p).unwrap().contains("edited"));
    m.resolve_overwrite_json(true);
    assert!(std::fs::read_to_string(&p).unwrap().contains("0.06"));
}

#[test]
fn a_cancelled_export_writes_no_json() {
    let d = tempfile::tempdir().unwrap();
    let mut m = ready(d.path());
    m.export_clicked();
    if matches!(m.dialog, Some(Dialog::ProceedAnyway(_))) {
        m.proceed_anyway();
    }
    m.cancel_export();
    assert!(m.wait_idle(Duration::from_secs(120)));
    if m.export.summary.as_ref().is_some_and(|s| !s.cancelled.is_empty()) || m.export.error.is_some() {
        assert!(!json_path(&m).exists());
        assert!(m.export.notes.iter().any(|n| n.contains("not written")), "{:?}", m.export.notes);
    }
    // (If the export finished before the cancel landed, there is nothing to check: the test is about the cancel path.)
}

#[test]
fn finish_leaves_out_sets_whose_outputs_did_not_export() {
    use sdc_gui::model::export::{Row, RowState};
    use sdc_gui::pgp::writer::EntryInput;
    let dir = tempfile::tempdir().unwrap();
    let p = PendingJson {
        path: dir.path().join("PBRNIFPatcher/x.json"),
        entries: vec![
            ("a/one".into(), vec!["a/one.dds".into()], EntryInput { derived_match: "a\\one".into(), ..Default::default() }),
            ("a/two".into(), vec!["a/two.dds".into()], EntryInput { derived_match: "a\\two".into(), ..Default::default() }),
        ],
        defaults: Default::default(),
        copies: vec![],
    };
    let rows = vec![
        Row { job_key: "a/one.dds".into(), path: String::new(), state: RowState::Done { passed: true }, warnings: vec![] },
        Row { job_key: "a/two.dds".into(), path: String::new(), state: RowState::Failed("boom".into()), warnings: vec![] },
    ];
    let mut notes = vec![];
    let r = finish(&p, &rows, None, &mut notes);
    assert!(matches!(r, Finish::Written(_)), "{r:?}");
    let text = std::fs::read_to_string(&p.path).unwrap();
    assert!(text.contains(r#""a\\one""#) && !text.contains(r#""a\\two""#), "{text}");
    assert!(notes.iter().any(|n| n.contains("not in JSON: a/two")), "{notes:?}");
}
```

- [ ] **Step 2: Run the tests and confirm they fail.**

Run: `cargo test -p sdc-gui --test pbr_export`
Expected: compile errors.

- [ ] **Step 3: Add to `crates/gui/src/model/pbr_export.rs`:**

```rust
use super::export::{Row, RowState};
use super::history::sha256_of;
use crate::pgp::writer::to_text;
use std::path::PathBuf;

#[derive(Debug, Clone)]
pub struct PendingJson {
    pub path: PathBuf,
    /// Set id, the job keys its entry depends on, and the entry.
    pub entries: Vec<(String, Vec<String>, EntryInput)>,
    pub defaults: Settings,
    /// Vanilla/CPM borrowed-slot copies: (owner's output, borrower's path), absolute (DECISIONS P4).
    pub copies: Vec<(PathBuf, PathBuf)>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Finish {
    Written(String),
    /// The file was edited outside the GUI: ask, with this text ready to write.
    Overwrite(String),
    Nothing,
}

/// What the export about to start will write afterwards (computed when it starts, so edits made while it runs do
/// not change it).
pub fn prepare(m: &AppModel) -> Option<PendingJson> {
    let job_key = |k: &str| m.item(k).map(|i| i.job_key.clone()).filter(|j| !j.is_empty());
    let deps = |v: &SetView| -> Vec<String> {
        v.slots
            .values()
            .filter_map(|f| match f {
                SlotFill::Own { item } | SlotFill::Borrowed { item, .. } => job_key(item),
                SlotFill::Empty => None,
            })
            .collect()
    };
    let mut copies = vec![];
    if m.target != Target::Cs {
        for v in set_views(m) {
            let Some(SlotFill::Own { item: d }) = v.slots.get(&SlotKind::Diffuse) else { continue };
            let Some(dout) = m.item(d).and_then(|i| i.job.as_ref()).map(|j| j.output.clone()) else { continue };
            for (kind, f) in &v.slots {
                let SlotFill::Borrowed { item, .. } = f else { continue };
                let Some(src) = m.item(item).and_then(|i| i.job.as_ref()).map(|j| j.output.clone()) else { continue };
                let stem = dout.file_stem().unwrap_or_default().to_string_lossy().into_owned();
                let name = if kind.suffix().is_empty() { format!("{stem}.dds") } else { format!("{stem}_{}.dds", kind.suffix()) };
                copies.push((src, dout.with_file_name(name)));
            }
        }
        return Some(PendingJson { path: PathBuf::new(), entries: vec![], defaults: Settings::new(), copies });
    }
    let entries: Vec<_> = json_entries(m).into_iter().map(|(v, e)| (v.id.clone(), deps(&v), e)).collect();
    if entries.is_empty() {
        return None;
    }
    Some(PendingJson {
        path: m.export_root().join("PBRNIFPatcher").join(format!("{}.json", json_name(m))),
        entries,
        defaults: defaults(m),
        copies,
    })
}

fn exported(rows: &[Row], key: &str) -> Result<(), String> {
    match rows.iter().find(|r| r.job_key.eq_ignore_ascii_case(key)) {
        Some(Row { state: RowState::Done { .. } | RowState::UpToDate, .. }) => Ok(()),
        Some(r) => Err(r.state.label()),
        None => Err("not exported".into()),
    }
}

fn write_atomic(path: &std::path::Path, text: &str) -> std::io::Result<()> {
    if let Some(d) = path.parent() {
        std::fs::create_dir_all(d)?;
    }
    let tmp = path.with_extension("json.sdc-tmp");
    std::fs::write(&tmp, text)?;
    std::fs::rename(&tmp, path)
}

/// After a successful export: the copies, then the JSON of the sets that fully exported (spec §6.1, §6.4).
pub fn finish(p: &PendingJson, rows: &[Row], last_sha: Option<&str>, notes: &mut Vec<String>) -> Finish {
    for (src, dst) in &p.copies {
        match std::fs::copy(src, dst) {
            Ok(_) => notes.push(format!("copied {} → {}", src.display(), dst.display())),
            Err(e) => notes.push(format!("could not copy {} ({e}; not exported?)", src.display())),
        }
    }
    if p.entries.is_empty() {
        return Finish::Nothing;
    }
    let mut kept = vec![];
    for (id, deps, e) in &p.entries {
        match deps.iter().map(|k| exported(rows, k)).find_map(Result::err) {
            None => kept.push(e.clone()),
            Some(why) => notes.push(format!("not in JSON: {id} ({why})")),
        }
    }
    if kept.is_empty() {
        notes.push("PGPatcher JSON not written: no PBR set exported completely".into());
        return Finish::Nothing;
    }
    let doc = document(&p.defaults, &kept);
    if let Err(errs) = validate(&doc.to_value()) {
        notes.push(format!("PGPatcher JSON not written: it does not validate ({})", errs[0]));
        return Finish::Nothing;
    }
    let text = to_text(&doc);
    let on_disk = sha256_of(&p.path);
    if on_disk.is_some() && on_disk.as_deref() != last_sha {
        return Finish::Overwrite(text);
    }
    match write_atomic(&p.path, &text) {
        Ok(()) => {
            notes.push(format!("PGPatcher JSON written: {} ({} entries)", p.path.display(), kept.len()));
            sha256_of(&p.path).map_or(Finish::Nothing, Finish::Written)
        }
        Err(e) => {
            notes.push(format!("PGPatcher JSON not written: {e}"));
            Finish::Nothing
        }
    }
}

/// Write `text` over an existing (hand-edited) JSON: the answer "Overwrite" to `Dialog::OverwriteJson`.
pub fn overwrite(path: &std::path::Path, text: &str, notes: &mut Vec<String>) -> Option<String> {
    match write_atomic(path, text) {
        Ok(()) => {
            notes.push(format!("PGPatcher JSON written: {}", path.display()));
            sha256_of(path)
        }
        Err(e) => {
            notes.push(format!("PGPatcher JSON not written: {e}"));
            None
        }
    }
}
```

The test constructs `EntryInput { derived_match, ..Default::default() }`; `EntryInput` derives `Default` (Task 8). `Row` and `RowState` must be `pub` (they are) with `pub` fields (they are). Add `pub mod export;` re-export if `model::export` isn't `pub` (it is, `pub mod export`).

- [ ] **Step 4: Wire `crates/gui/src/model/mod.rs`.**
  - Add the fields `pending_json: Option<pbr_export::PendingJson>` and `pending_text: Option<String>`.
  - Add `Dialog::OverwriteJson(PathBuf)`.
  - In `start_export`, before spawning: `self.pending_json = pbr_export::prepare(self);`.
  - In `handle_more` → `ExportDone`, after `self.export.running = None;` and the summary/error handling, before `run_after_export`:

```rust
                if let Some(p) = self.pending_json.take() {
                    let ok = self.export.error.is_none()
                        && self.export.summary.as_ref().is_some_and(|s| s.cancelled.is_empty());
                    if !ok {
                        self.export.notes.push("PGPatcher JSON not written: the export was cancelled or failed".into());
                    } else {
                        let last = self.sets_doc.as_ref().and_then(|d| d.last_json_sha256());
                        match pbr_export::finish(&p, &self.export.rows, last.as_deref(), &mut self.export.notes) {
                            pbr_export::Finish::Written(sha) => self.record_json_sha(&sha),
                            pbr_export::Finish::Overwrite(text) => {
                                self.pending_text = Some(text);
                                self.dialog = Some(Dialog::OverwriteJson(p.path.clone()));
                                self.pending_json = Some(p);
                            }
                            pbr_export::Finish::Nothing => {}
                        }
                    }
                }
```

  - Add:

```rust
    /// Record the hash of the JSON just written and save `sdc-project.toml`, so the record matches the disk.
    fn record_json_sha(&mut self, sha: &str) {
        if let Some(d) = self.sets_doc.as_mut() {
            d.set_last_json_sha256(sha);
            if let Err(e) = d.save(false) {
                self.dialog = Some(Dialog::Error(format!("sdc-project.toml: {e}")));
            }
        }
    }

    /// The answer to `Dialog::OverwriteJson`.
    pub fn resolve_overwrite_json(&mut self, overwrite: bool) {
        self.dialog = None;
        let (Some(p), Some(text)) = (self.pending_json.take(), self.pending_text.take()) else { return };
        if overwrite {
            if let Some(sha) = pbr_export::overwrite(&p.path, &text, &mut self.export.notes) {
                self.record_json_sha(&sha);
            }
        } else {
            self.export.notes.push(format!("PGPatcher JSON kept as edited: {}", p.path.display()));
        }
    }
```

  - **The overwrite dialog must not be dropped by `run_after_export`.** That function clears only `ExportRunning` dialogs, so this one survives. Add a test assertion if you change that function.
  - **`record_json_sha` and the dirty flag.** `record_json_sha` saves only `sdc-project.toml`. Before Export, `start_export` has already saved both files (`self.save()`), so the model is clean afterwards (`!m.is_dirty()` in the test).

- [ ] **Step 5: Add the UI pieces.**
  - `crates/gui/src/model/export.rs`: add `pub notes: Vec<String>,` to `ExportModel`. `Default` gives an empty list.
  - `crates/gui/src/ui/drawer.rs`: after the summary/error lines, `for n in &model.export.notes { ui.label(egui::RichText::new(n).small()); }`.
  - `crates/gui/src/ui/dialogs.rs`: `Dialog::OverwriteJson(path)` shows "`<file>` was edited outside sdc since the last export." with the buttons **Overwrite edited JSON** (`model.resolve_overwrite_json(true)`) and **Keep it** (`model.resolve_overwrite_json(false)`).

- [ ] **Step 6: Run the tests.**

Run: `cargo test -p sdc-gui --test pbr_export && cargo test -p sdc-gui`
Expected: all pass.

- [ ] **Step 7: Run the gates and commit.**

```bash
cargo fmt --all && cargo clippy --workspace --all-targets -- -D warnings && cargo test -p sdc-gui && cargo test -p sdc --test gui_repro
git add crates/gui/src/model/pbr_export.rs crates/gui/src/model/mod.rs crates/gui/src/model/export.rs crates/gui/src/ui/drawer.rs crates/gui/src/ui/dialogs.rs crates/gui/tests/pbr_export.rs
git commit -m "feat(gui): write the PGPatcher JSON after export (complete sets only), overwrite prompt, vanilla copies (M3b §6)

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

---
### Task 13: The editable Sets column (spec §3.1)

**Files:**
- Create: `crates/gui/src/ui/sets_column.rs`, which replaces `columns::sets` (delete that function and its now-unused `Click` enum).
- Modify:
  - `crates/gui/src/ui/columns.rs`: texture tiles and set members become drag sources;
  - `crates/gui/src/ui/mod.rs`: `pub mod sets_column;`; `State.drops: sets_column::DropState`; the Sets column calls `sets_column::draw`; `Selection::Defaults` routing;
  - `crates/gui/src/model/mod.rs`: `Selection::Defaults`.
- Test: `crates/gui/tests/set_editor_ui.rs` (new; Tasks 14–15 append)

**Interfaces:**
- Consumes:
  - `model::sets::{set_views, SetView, SlotFill, DropOutcome, DropChoice}`;
  - the `AppModel::{drop_on_set, drop_choice, drop_replace, drop_new_set, reorder_set, rename_set, delete_set, remove_member, select_set}` methods;
  - `pbr_export::export_blocks`.
- Produces:
  - `pub struct DragItem(pub String)` (an item key) and `pub struct DragSet(pub String)` (a set id), the egui drag payloads;
  - `pub struct DropState { pub pending: Option<Pending>, pub renaming: Option<(String, String)> }`;
  - `pub enum Pending { Choice { item, set, from }, Replace { item, set, existing, choice }, Refused(String) }`;
  - `Selection::Defaults`.

**Behaviour** (layout from the approved mockup `sets-column.html`):
- **A "Defaults" card** is pinned at the top. Clicking it selects `Selection::Defaults` (Task 15 draws its editor).
- **Each set pill**, in `set_views` order, shows:
  - a `≡` handle (drag it onto another pill to reorder: `reorder_set(dragged, index_of_target)`);
  - its id, in bold;
  - its badges:
    - `PBR → JSON`, when `pbr` and the target is CS;
    - else `vanilla set (no JSON)`;
    - `⚠ n`: the sum of its members' warnings;
    - a red `blocked`, when `export_blocks` names it;
    - `owns <slot>` for each `owns`;
    - `borrowed from <set>` for each borrowed slot;
    - a red `missing: <file>` for each `missing`;
  - its member thumbnails (click selects, double-click expands, drag = `DragItem`);
  - its recipe lines (kept from M3a).
- **Dropping a `DragItem` on a pill** calls `drop_on_set`:
  - `Joined`: done.
  - `AskChoice`: a small modal with the choices Move it here / Share (borrow its finished output) / Duplicate the source for this set / Cancel → `drop_choice`. If the result is `AskReplace`, chain to the replace modal.
  - `AskReplace`: a modal "Replace `<existing name>` / Cancel" → `drop_replace(item, set, choice)`, with the choice that led there (`Move` for a plain drop; Share and Duplicate also ask when the slot is filled). Its `Err(why)` opens the `Refused` modal.
  - `Refused(why)`: a modal with the reason.
- **Dropping a `DragItem` on the dashed "Drop here to start a new set" area** at the bottom calls `drop_new_set` (it returns `Option<String>`; `None` means nothing was done).
- **Remove member** calls `remove_member`, which returns `Result<(), String>`; an `Err` opens the `Refused` modal.
- **Right-clicking a pill** gives:
  - Rename… (an inline text field in the menu; Enter calls `rename_set`, and an error is shown under the field);
  - Edit match… (`select_set`; the editor shows the Match group at the top);
  - Remove member (a submenu listing the members);
  - Delete set: enabled only for created sets. Otherwise it is disabled, with the hover text "Only sets you created can be deleted; an automatic set comes back from the file names" (P3);
  - Move up / Move down.
- **While `sdc-project.toml` is read-only (P8)**, drag targets and the menu are disabled, with the banner's reason as hover text.

- [ ] **Step 1: Write the failing UI smoke tests.** Create `crates/gui/tests/set_editor_ui.rs`:

```rust
mod support;
use egui_kittest::kittest::Queryable;
use egui_kittest::Harness;
use sdc_gui::model::{AppModel, Selection};
use support::*;

fn harness(model: AppModel) -> Harness<'static, AppModel> {
    let h = Harness::builder()
        .with_size(eframe::egui::vec2(1400.0, 900.0))
        .build_ui_state(|ui, model: &mut AppModel| sdc_gui::ui::draw(ui, model), model);
    sdc_gui::fonts::install_system_fonts(&h.ctx).expect("system fonts");
    h
}

#[test]
fn the_sets_column_shows_defaults_badges_and_the_new_set_area() {
    let d = tempfile::tempdir().unwrap();
    let mut h = harness(pbr_opened(d.path()));
    h.run();
    h.get_by_label("Defaults");
    h.get_by_label("landscape/rock01");
    h.get_by_label_contains("PBR → JSON");
    h.get_by_label_contains("Drop here to start a new set");
    h.get_by_label("Defaults").click();
    h.run();
    assert_eq!(h.state().selection, Selection::Defaults);
}
```

Dragging isn't simulated in kittest. The drop rules are covered by `sets_model.rs` (Task 10). This test covers the drawing and the Defaults selection.

- [ ] **Step 2: Run the test and confirm it fails.**

Run: `cargo test -p sdc-gui --test set_editor_ui`
Expected: FAIL: there is no "Defaults" label, and `Selection::Defaults` doesn't exist.

- [ ] **Step 3: Add `Selection::Defaults`** to `model/mod.rs`. In `apply_plan`'s "gone" check, `Selection::Defaults` is never gone. Fix every exhaustive `match` the compiler flags; in `ui/mod.rs` `centre`, temporarily route `Defaults` to a label "Defaults" (Task 15 replaces it).

- [ ] **Step 4: Make the tiles and members drag sources.** In `crates/gui/src/ui/columns.rs`:
  - the tile: `.interact(egui::Sense::click_and_drag())`, then `resp.dnd_set_drag_payload(crate::ui::sets_column::DragItem(key.clone()));`;
  - `member()`: allocate with `egui::Sense::click_and_drag()`, and return the response (the caller sets the payload).

- [ ] **Step 5: Create `crates/gui/src/ui/sets_column.rs`:**

```rust
//! The editable Sets column (M3b spec §3.1): a Defaults card, set pills in project order with badges, drag and drop
//! of textures onto sets (and onto an empty area for a new set), drag-to-reorder by the ≡ handle, and a context
//! menu. Drawing only: every change is a model call, made after drawing.
use super::columns::{border_of, member, status_text, AMBER, RED};
use super::thumbs::ThumbCache;
use crate::model::pbr_export::export_blocks;
use crate::model::sets::{set_views, DropChoice, DropOutcome, SetView, SlotFill};
use crate::model::{AppModel, Selection};
use eframe::egui;
use engine::config::Target;

/// Drag payload: a texture, by item key.
#[derive(Debug, Clone)]
pub struct DragItem(pub String);
/// Drag payload: a set, by id (reordering).
#[derive(Debug, Clone)]
pub struct DragSet(pub String);

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Pending {
    Choice { item: String, set: String, from: String },
    /// `choice` is what the replace answers: `drop_replace(item, set, choice)`.
    Replace { item: String, set: String, existing: String, choice: DropChoice },
    Refused(String),
}

#[derive(Debug, Default)]
pub struct DropState {
    pub pending: Option<Pending>,
    /// (set id, text being typed) while renaming from the context menu.
    pub renaming: Option<(String, String)>,
    pub rename_error: Option<String>,
}

enum Act {
    Select(String),
    SelectItem(String),
    Expand(String),
    Defaults,
    Drop(String, String),
    NewSet(String),
    Reorder(String, usize),
    Rename(String, String),
    Delete(String),
    RemoveMember(String, String),
}

fn badge(ui: &mut egui::Ui, text: &str, color: Option<egui::Color32>) {
    let t = egui::RichText::new(text).small();
    ui.label(match color {
        Some(c) => t.color(c),
        None => t.weak(),
    });
}

pub fn draw(ui: &mut egui::Ui, model: &mut AppModel, thumbs: &mut ThumbCache, st: &mut DropState) {
    ui.heading("Sets");
    ui.separator();
    let editable = model.sets_doc.as_ref().is_some_and(|d| !d.read_only());
    let views = set_views(model);
    let blocked: Vec<String> = export_blocks(model).into_iter().map(|i| i.set_id).collect();
    let mut acts = vec![];
    let defaults_selected = model.selection == Selection::Defaults;
    let r = egui::Frame::group(ui.style())
        .corner_radius(12.0)
        .stroke(if defaults_selected { ui.visuals().selection.stroke } else { ui.visuals().widgets.noninteractive.bg_stroke })
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.label(egui::RichText::new("Defaults").strong());
            ui.label(egui::RichText::new("JSON values every set inherits").small().weak());
        })
        .response
        .interact(egui::Sense::click());
    if r.clicked() {
        acts.push(Act::Defaults);
    }
    egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
        for (index, v) in views.iter().enumerate() {
            pill(ui, model, thumbs, st, v, index, editable, blocked.contains(&v.id), &mut acts);
        }
        // The new-set drop area.
        let (resp, payload) = ui.dnd_drop_zone::<DragItem, ()>(egui::Frame::group(ui.style()).corner_radius(12.0), |ui| {
            ui.set_width(ui.available_width());
            ui.label(egui::RichText::new("Drop here to start a new set").color(AMBER));
        });
        let _ = resp;
        if let Some(p) = payload {
            if editable {
                acts.push(Act::NewSet(p.0.clone()));
            }
        }
    });
    pending_modal(ui.ctx(), model, st);
    for a in acts {
        match a {
            Act::Select(k) => model.select_set(&k),
            Act::SelectItem(k) => model.select_item(&k),
            Act::Expand(k) => model.expand(&k),
            Act::Defaults => model.selection = Selection::Defaults,
            Act::Drop(item, set) => match model.drop_on_set(&item, &set) {
                DropOutcome::Joined => {}
                DropOutcome::AskChoice { from } => st.pending = Some(Pending::Choice { item, set, from }),
                DropOutcome::AskReplace { existing } => {
                    st.pending = Some(Pending::Replace { item, set, existing, choice: DropChoice::Move })
                }
                DropOutcome::Refused(why) => st.pending = Some(Pending::Refused(why)),
            },
            Act::NewSet(item) => {
                // `None`: nothing was done (read-only file, or a packed map, which follows its raw maps).
                let _ = model.drop_new_set(&item);
            }
            Act::Reorder(id, to) => model.reorder_set(&id, to),
            Act::Rename(id, new) => match model.rename_set(&id, &new) {
                Ok(()) => {
                    st.renaming = None;
                    st.rename_error = None;
                }
                Err(e) => st.rename_error = Some(e),
            },
            Act::Delete(id) => {
                model.delete_set(&id);
            }
            Act::RemoveMember(id, item) => {
                if let Err(why) = model.remove_member(&id, &item) {
                    st.pending = Some(Pending::Refused(why));
                }
            }
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn pill(
    ui: &mut egui::Ui,
    model: &AppModel,
    thumbs: &mut ThumbCache,
    st: &mut DropState,
    v: &SetView,
    index: usize,
    editable: bool,
    blocked: bool,
    acts: &mut Vec<Act>,
) {
    let selected = matches!(&model.selection, Selection::Set(k) if !k.is_empty() && *k == v.key);
    let stroke = if selected { ui.visuals().selection.stroke } else { ui.visuals().widgets.noninteractive.bg_stroke };
    let (zone, dropped) = ui.dnd_drop_zone::<DragItem, ()>(egui::Frame::group(ui.style()).corner_radius(12.0).stroke(stroke), |ui| {
        ui.set_width(ui.available_width());
        ui.horizontal(|ui| {
            let handle = ui.add(egui::Label::new("≡").sense(egui::Sense::drag()));
            handle.dnd_set_drag_payload(DragSet(v.id.clone()));
            ui.label(egui::RichText::new(&v.label).strong());
        });
        ui.horizontal_wrapped(|ui| {
            if v.pbr && model.target == Target::Cs {
                badge(ui, "PBR → JSON", None);
            } else {
                badge(ui, "vanilla set (no JSON)", None);
            }
            let warnings: usize = v.members.iter().filter_map(|k| model.item(k)).map(|i| i.warnings.len()).sum();
            if warnings > 0 {
                badge(ui, &format!("⚠ {warnings}"), Some(AMBER));
            }
            if blocked {
                badge(ui, "blocked", Some(RED));
            }
            for s in &v.owns {
                badge(ui, &format!("owns {}", s.label()), None);
            }
            for f in v.slots.values() {
                if let SlotFill::Borrowed { from, .. } = f {
                    badge(ui, &format!("borrowed from {from}"), Some(AMBER));
                }
            }
            for m in &v.missing {
                badge(ui, &format!("missing: {m}"), Some(RED));
            }
        });
        ui.horizontal_wrapped(|ui| {
            for mk in &v.members {
                let Some(item) = model.item(mk) else { continue };
                let sel = matches!(&model.selection, Selection::Item(k) if k == mk);
                let r = member(ui, item, sel, thumbs);
                r.dnd_set_drag_payload(DragItem(mk.clone()));
                if r.double_clicked() {
                    acts.push(Act::Expand(mk.clone()));
                } else if r.clicked() {
                    acts.push(Act::SelectItem(mk.clone()));
                }
            }
        });
        for set in model.sets.iter().filter(|s| s.key == v.key) {
            for recipe in &set.recipes {
                let (word, color) = status_text(recipe.status);
                let t = egui::RichText::new(format!("{}: {word}", recipe.subtype.id())).small();
                ui.label(match color {
                    Some(c) => t.color(c),
                    None => t.weak(),
                });
            }
        }
    });
    let resp = zone.response.interact(egui::Sense::click());
    if resp.clicked() && !v.key.is_empty() {
        acts.push(Act::Select(v.key.clone()));
    }
    if let Some(p) = dropped {
        if editable {
            acts.push(Act::Drop(p.0.clone(), v.id.clone()));
        }
    }
    if let Some(p) = resp.dnd_release_payload::<DragSet>() {
        if editable && p.0 != v.id {
            acts.push(Act::Reorder(p.0.clone(), index));
        }
    }
    let _ = border_of; // shared helper kept in columns.rs
    resp.context_menu(|ui| {
        if !editable {
            ui.disable();
        }
        match &mut st.renaming {
            Some((id, text)) if *id == v.id => {
                let r = ui.text_edit_singleline(text);
                if r.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                    acts.push(Act::Rename(v.id.clone(), text.clone()));
                }
                if let Some(e) = &st.rename_error {
                    ui.colored_label(RED, e);
                }
            }
            _ => {
                if ui.button("Rename…").clicked() {
                    st.renaming = Some((v.id.clone(), v.id.clone()));
                    st.rename_error = None;
                }
            }
        }
        if ui.button("Edit match…").clicked() && !v.key.is_empty() {
            acts.push(Act::Select(v.key.clone()));
            ui.close();
        }
        ui.menu_button("Remove member", |ui| {
            for mk in &v.members {
                let name = model.item(mk).map_or(mk.as_str(), |i| i.name.as_str());
                if ui.button(name).clicked() {
                    acts.push(Act::RemoveMember(v.id.clone(), mk.clone()));
                    ui.close();
                }
            }
        });
        let del = ui.add_enabled(v.created || v.key.is_empty(), egui::Button::new("Delete set"));
        let del = del.on_disabled_hover_text("Only sets you created (or whose files are gone) can be deleted; an automatic set comes back from the file names");
        if del.clicked() {
            acts.push(Act::Delete(v.id.clone()));
            ui.close();
        }
        if ui.button("Move up").clicked() && index > 0 {
            acts.push(Act::Reorder(v.id.clone(), index - 1));
            ui.close();
        }
        if ui.button("Move down").clicked() {
            acts.push(Act::Reorder(v.id.clone(), index + 1));
            ui.close();
        }
    });
}

/// What a button in the drop modal asked for; applied after the modal is drawn (the modal only reads the model).
enum ModalAnswer {
    Choice(DropChoice),
    Replace,
    Close,
}

fn pending_modal(ctx: &egui::Context, model: &mut AppModel, st: &mut DropState) {
    let Some(p) = st.pending.clone() else { return };
    let mut answer = None;
    egui::Modal::new(egui::Id::new("set-drop")).show(ctx, |ui| {
        ui.set_max_width(420.0);
        match &p {
            Pending::Choice { item, from, .. } => {
                let name = model.item(item).map_or(item.clone(), |i| i.name.clone());
                ui.heading(format!("{name} is in {from}"));
                if ui.button("Move it here").clicked() {
                    answer = Some(ModalAnswer::Choice(DropChoice::Move));
                }
                if ui.button("Share: use its finished texture here").clicked() {
                    answer = Some(ModalAnswer::Choice(DropChoice::Share));
                }
                if ui.button("Duplicate the source for this set").clicked() {
                    answer = Some(ModalAnswer::Choice(DropChoice::Duplicate));
                }
                if ui.button("Cancel").clicked() {
                    answer = Some(ModalAnswer::Close);
                }
            }
            Pending::Replace { set, existing, .. } => {
                let name = model.item(existing).map_or(existing.clone(), |i| i.name.clone());
                ui.heading("Replace?");
                ui.label(format!("{set} already has {name} in that slot."));
                ui.horizontal(|ui| {
                    if ui.button(format!("Replace {name}")).clicked() {
                        answer = Some(ModalAnswer::Replace);
                    }
                    if ui.button("Cancel").clicked() {
                        answer = Some(ModalAnswer::Close);
                    }
                });
            }
            Pending::Refused(why) => {
                ui.heading("Can't do that");
                ui.label(why);
                if ui.button("OK").clicked() {
                    answer = Some(ModalAnswer::Close);
                }
            }
        }
    });
    let Some(answer) = answer else { return };
    st.pending = match (answer, p) {
        (ModalAnswer::Choice(c), Pending::Choice { item, set, .. }) => match model.drop_choice(&item, &set, c) {
            DropOutcome::AskReplace { existing } => Some(Pending::Replace { item, set, existing, choice: c }),
            DropOutcome::Refused(why) => Some(Pending::Refused(why)),
            _ => None,
        },
        // The replace answers the choice that led to it (Move, Share or Duplicate); a refusal shows its reason.
        (ModalAnswer::Replace, Pending::Replace { item, set, choice, .. }) => {
            model.drop_replace(&item, &set, choice).err().map(Pending::Refused)
        }
        _ => None,
    };
}
```

Implementation notes:
- **A created set with no members yet** has `key == ""`: it can't be selected (the set editor is keyed by engine key), so its JSON settings can't be edited until a texture is dropped in. Its pill says so in a weak line, "Drop a texture here to start", and clicking it does nothing. This is acceptable for M3b.
- **An entry whose files have all gone** (Task 10, spec §7.1) also has `key == ""`, with `created == false` and a non-empty `missing`. Its pill is drawn the same way, without the "Drop a texture here" line; its `missing` badges say why. Drops on it come back `Refused`, and "Delete set" is enabled for it (`delete_set` accepts `created || key.is_empty()`).
- **"Not in a set" (spec §3.1 Proposal, ruling P15):** the Textures column header gets a clickable amber `"<n> not in a set"`, next to the existing "need a decision" count. Clicking it turns on `model.filter.not_in_set`. Switching the filter on automatically would hide textures on open and change M3a's behaviour. Count with the same `in_set` rule `visible_items` uses. Add this to `columns::textures`.
- **egui API names** (`dnd_drop_zone`, `dnd_set_drag_payload`, `dnd_release_payload`, `ui.close()`, `context_menu`) are egui 0.36's. If one has a different name in 0.36.2, find the current one with `grep -rn "pub fn dnd_" ~/.cargo/registry/src/*/egui-0.36.2/src/`. Keep the behaviour.
- **`AMBER` and `RED`** must be `pub(super)` in `columns.rs` (`AMBER` is already imported by `advanced.rs`).
- **Remove the `let _ = border_of;` line** if `border_of` stays used elsewhere.

- [ ] **Step 6: Wire it in `crates/gui/src/ui/mod.rs`.** Add `pub mod sets_column;` and a `drops: sets_column::DropState` field on `State`. Then make the Sets `column_panel` closure call `sets_column::draw(ui, model, &mut state.thumbs, &mut state.drops)`. **Two closures can't both borrow `state` mutably.** Pass `&mut state.thumbs` and `&mut state.drops` through local bindings:

```rust
let (thumbs, drops) = (&mut state.thumbs, &mut state.drops);
```

Do this before the Sets call, as the borrow checker requires.

- [ ] **Step 7: Run the tests.**

Run: `cargo test -p sdc-gui --test set_editor_ui && cargo test -p sdc-gui`
Expected: all pass. Update the M3a `ui_smoke` assertions that named the old set pill only if they now fail because of the new Defaults card or badges. Keep what they check.

- [ ] **Step 8: Run the gates and commit.**

```bash
cargo fmt --all && cargo clippy --workspace --all-targets -- -D warnings && cargo test -p sdc-gui
git add crates/gui/src/ui/sets_column.rs crates/gui/src/ui/columns.rs crates/gui/src/ui/mod.rs crates/gui/src/model/mod.rs crates/gui/tests/set_editor_ui.rs
git commit -m "feat(gui): editable Sets column: drag and drop, new set, reorder, context menu, badges, Defaults card (M3b §3.1)

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

---

### Task 14: The set editor, left side: slot cards, Opaque, slot commands (spec §4.1–4.2)

**Files:**
- Create: `crates/gui/src/ui/set_editor.rs` and `crates/gui/src/ui/slot_commands.rs`.
- Delete: `crates/gui/src/ui/set_view.rs`. Its recipe and channel lines move into the editor, under the slot cards.
- Modify:
  - `crates/gui/src/ui/mod.rs`: `Selection::Set` → `set_editor::draw`; `State.slot_cmds: slot_commands::SlotCmdState`;
  - `crates/gui/src/model/mod.rs`: `set_format(item, Format, ColorTag)` and `set_opaque(item, bool)`.
- Test: `crates/gui/tests/set_editor_ui.rs` (append)

**Interfaces:**
- Consumes:
  - `sets::{set_views, SlotKind, SlotFill, SetView, SlotCommand}`;
  - `fields::{format_label, format_options, field_lock}`;
  - `AppModel::{set_slot_command, expand, select_item}`.
- Produces:
  - `AppModel::set_format(&mut self, item: &str, f: Format, tag: ColorTag)`: one undo step that writes `format` and `color.tag`, then re-plans;
  - `AppModel::set_opaque(&mut self, item: &str, on: bool)`: `alpha.policy = "remove"` when on, and clears that field when off;
  - `AppModel::is_opaque(&self, item: &str) -> bool`: the item's own `[files]` `alpha.policy == "remove"`;
  - `set_editor::draw(ui, model, key, thumbs, state)`.

**Layout A** (approved). Two columns inside the centre: slot cards on the left, PGPatcher settings on the right (Task 15).
- **A header line:** the set id, then `match_diffuse` (with "(derived)" while it isn't set), then the prefix or the prefix error in red.
- **A grid of 8 cards**, in `SlotKind::ALL` order (four per row). Each card shows:
  - the label;
  - the thumbnail, or "empty";
  - the source file name;
  - "borrowed from `<set>`" on a borrowed slot, which is read-only;
  - a **preset dropdown** whose options are `format_label(format, tag)` for:
    - every `format_options(target)` format;
    - for BC1/BC3/BC7, both `ColorTag::Srgb` and `ColorTag::Unorm`.

    When `field_lock(target, profile, "format")` gives a reason, the dropdown is disabled with that reason as hover text (the reason is shown, not hidden).
  - On the **Diffuse** card only: the **Opaque** checkbox (`is_opaque` / `set_opaque`), with the hover text "Strips alpha: the diffuse is fully opaque".
  - Clicking a card selects that texture, which shows its M3a settings. Add a "← `<set>`" link at the top of `settings_panel` when the selected item belongs to a set: `model.select_set(set_key)`. Double-clicking expands it.
- **Under the grid:**
  - the **[Slot commands…]** button;
  - the set's warnings: each member's warnings, as in M3a;
  - the recipe lines, from the old `set_view.rs`.
- **The slot-commands dialog** (`slot_commands.rs`, a modal). For each slot 1–8, the label from the schema (`Slot 1` … `Slot 8`) plus what the set gives there. Then a combo: **From this set** / **Path** / **Clear**.
  - Path shows a text field; Enter or Apply calls `set_slot_command(set, n, SlotCommand::Path(text))`.
  - An `Err(reason)` stays shown under the field.

- [ ] **Step 1: Write the failing tests.** Append to `crates/gui/tests/set_editor_ui.rs`:

```rust
#[test]
fn the_set_editor_shows_slot_cards_opaque_and_slot_commands() {
    let d = tempfile::tempdir().unwrap();
    let mut m = pbr_opened(d.path());
    let key = m.sets.iter().find(|s| s.key.ends_with("landscape/rock01")).unwrap().key.clone();
    m.select_set(&key);
    let mut h = harness(m);
    h.run();
    for label in ["Diffuse", "Normal _n", "RMAOS", "Height _p", "Emissive _g", "Subsurface _s", "Coat _cnr", "Fuzz _f"] {
        h.get_by_label(label);
    }
    h.get_by_label("Opaque");
    h.get_by_label_contains("BC7 · sRGB");
    h.get_by_label("Slot commands…").click();
    h.run();
    h.get_by_label("From this set");
}

#[test]
fn opaque_and_format_edit_the_diffuse_in_one_step_each() {
    let d = tempfile::tempdir().unwrap();
    let mut m = pbr_opened(d.path());
    let diffuse = m.items.iter().find(|i| i.name == "rock01.png").unwrap().key.clone();
    m.set_opaque(&diffuse, true);
    assert!(m.is_opaque(&diffuse));
    m.undo();
    assert!(!m.is_opaque(&diffuse));
    m.set_format(&diffuse, engine::config::Format::Bc1, engine::config::ColorTag::Srgb);
    let p = m.project.as_ref().unwrap();
    assert_eq!(p.file_value(&diffuse, "format"), Some(serde_json::json!("bc1")));
    assert_eq!(p.file_value(&diffuse, "color.tag"), Some(serde_json::json!("srgb")));
    m.undo();
    let p = m.project.as_ref().unwrap();
    assert_eq!(p.file_value(&diffuse, "format"), None, "both fields undo together");
    assert_eq!(p.file_value(&diffuse, "color.tag"), None);
}

#[test]
fn a_bad_slot_path_is_refused_with_the_reason() {
    let d = tempfile::tempdir().unwrap();
    let mut m = pbr_opened(d.path());
    assert!(m.set_slot_command("landscape/rock01", 3, sdc_gui::model::sets::SlotCommand::Path(r"..\x.dds".into())).is_err());
    assert!(m.set_slot_command("landscape/rock01", 3, sdc_gui::model::sets::SlotCommand::Path(r"effects\white.png".into())).is_err());
    m.set_slot_command("landscape/rock01", 3, sdc_gui::model::sets::SlotCommand::Path(r"effects\white.dds".into())).unwrap();
    m.set_slot_command("landscape/rock01", 5, sdc_gui::model::sets::SlotCommand::Clear).unwrap();
    let e = m.sets_doc.as_ref().unwrap().entries().into_iter().find(|e| e.id == "landscape/rock01").unwrap();
    assert_eq!(e.settings["slot3"], serde_json::json!(r"effects\white.dds"));
    assert_eq!(e.settings["slot5"], serde_json::json!(""));
}
```

- [ ] **Step 2: Run the tests and confirm they fail.**

Run: `cargo test -p sdc-gui --test set_editor_ui`
Expected: FAIL (no slot cards, no `set_format`/`set_opaque`).

- [ ] **Step 3: Add the model methods** to `crates/gui/src/model/mod.rs`:

```rust
    /// The slot card's preset dropdown: format and tag together, one undo step (M3b §4.1).
    pub fn set_format(&mut self, item: &str, f: engine::config::Format, tag: engine::config::ColorTag) {
        let k = item.to_string();
        self.edit("Format", move |p| {
            p.set_file_value(&k, "format", &json!(f));
            p.set_file_value(&k, "color.tag", &json!(tag));
        });
    }

    /// The Diffuse card's Opaque checkbox: alpha `remove` while ticked; unticked returns to the resolved default.
    pub fn set_opaque(&mut self, item: &str, on: bool) {
        let k = item.to_string();
        self.edit("Opaque", move |p| {
            if on {
                p.set_file_value(&k, "alpha.policy", &json!(AlphaPolicy::Remove));
            } else {
                p.clear_file_value(&k, "alpha.policy");
            }
        });
    }

    pub fn is_opaque(&self, item: &str) -> bool {
        self.project
            .as_ref()
            .and_then(|p| p.file_value(item, "alpha.policy"))
            .is_some_and(|v| v == json!(AlphaPolicy::Remove))
    }
```

- [ ] **Step 4: Create `crates/gui/src/ui/slot_commands.rs`:**

```rust
//! The Slot commands dialog (M3b spec §4.2): per slot 1–8, from this set / a path / clear.
use crate::model::sets::SlotCommand;
use crate::model::AppModel;
use eframe::egui;
use serde_json::Value;

#[derive(Debug, Default)]
pub struct SlotCmdState {
    /// The set whose dialog is open.
    pub open: Option<String>,
    /// Per slot: the text being typed for a Path, and the last refusal.
    pub text: [String; 8],
    pub error: [Option<String>; 8],
}

const NAMES: [&str; 8] = ["Diffuse", "Normal", "Emissive", "Height", "Cubemap", "RMAOS", "Coat normal / fuzz", "Subsurface"];

pub fn draw(ctx: &egui::Context, model: &mut AppModel, st: &mut SlotCmdState) {
    let Some(set) = st.open.clone() else { return };
    let settings = model
        .sets_doc
        .as_ref()
        .and_then(|d| d.entries().into_iter().find(|e| e.id.eq_ignore_ascii_case(&set)))
        .map(|e| e.settings)
        .unwrap_or_default();
    let mut close = false;
    let mut apply: Vec<(u8, SlotCommand)> = vec![];
    egui::Modal::new(egui::Id::new("slot-commands")).show(ctx, |ui| {
        ui.set_max_width(560.0);
        ui.heading(format!("Slot commands: {set}"));
        ui.label("A path is relative to Data\\ and ends in .dds; textures\\ is added when missing (except for the cubemap). Base-game files in a BSA work too.");
        egui::Grid::new("slot-cmd-grid").num_columns(3).show(ui, |ui| {
            for n in 1..=8u8 {
                let i = usize::from(n - 1);
                ui.label(format!("Slot {n} ({})", NAMES[i]));
                let current = match settings.get(&format!("slot{n}")) {
                    None => 0,
                    Some(Value::String(s)) if s.is_empty() => 2,
                    Some(_) => 1,
                };
                if st.text[i].is_empty() {
                    if let Some(Value::String(s)) = settings.get(&format!("slot{n}")) {
                        st.text[i] = s.clone();
                    }
                }
                let mut choice = current;
                egui::ComboBox::from_id_salt(("slot-cmd", n))
                    .selected_text(["From this set", "Path", "Clear"][choice])
                    .show_ui(ui, |ui| {
                        ui.selectable_value(&mut choice, 0, "From this set");
                        ui.selectable_value(&mut choice, 1, "Path");
                        ui.selectable_value(&mut choice, 2, "Clear");
                    });
                ui.vertical(|ui| {
                    if choice == 1 {
                        let r = ui.text_edit_singleline(&mut st.text[i]);
                        if (r.lost_focus() && ui.input(|k| k.key_pressed(egui::Key::Enter))) || ui.small_button("Apply").clicked() {
                            apply.push((n, SlotCommand::Path(st.text[i].clone())));
                        }
                    }
                    if let Some(e) = &st.error[i] {
                        ui.colored_label(ui.visuals().error_fg_color, e);
                    }
                });
                if choice != current && choice != 1 {
                    apply.push((n, if choice == 0 { SlotCommand::FromSet } else { SlotCommand::Clear }));
                }
                ui.end_row();
            }
        });
        close = ui.button("Close").clicked();
    });
    for (n, cmd) in apply {
        st.error[usize::from(n - 1)] = model.set_slot_command(&set, n, cmd).err();
    }
    if close {
        *st = SlotCmdState::default();
    }
}
```

With "Path" chosen and nothing typed yet, nothing is written until Apply.

- [ ] **Step 5: Create `crates/gui/src/ui/set_editor.rs`** (left side; Task 15 adds the right side through `pgp_panel(ui, model, set_id)`; until then the right column holds `ui.label("PGPatcher settings")`):

```rust
//! The set editor, layout A (M3b spec §4): slot cards on the left, PGPatcher settings on the right (Task 15).
use super::columns::{export_prefix, RED};
use super::slot_commands::SlotCmdState;
use super::thumbs::ThumbCache;
use crate::model::fields::{field_lock, format_label, format_options};
use crate::model::sets::{set_views, SetView, SlotFill, SlotKind};
use crate::model::AppModel;
use eframe::egui;
use engine::config::{ColorTag, Format};

const CARD: f32 = 150.0;
const THUMB: f32 = 72.0;

enum Act {
    Select(String),
    Expand(String),
    Format(String, Format, ColorTag),
    Opaque(String, bool),
}

fn tag_options(f: Format) -> Vec<ColorTag> {
    match f {
        Format::Bc1 | Format::Bc3 | Format::Bc7 => vec![ColorTag::Srgb, ColorTag::Unorm],
        _ => vec![ColorTag::Unorm],
    }
}

pub fn draw(ui: &mut egui::Ui, model: &mut AppModel, key: &str, thumbs: &mut ThumbCache, cmds: &mut SlotCmdState) {
    let Some(v) = set_views(model).into_iter().find(|v| v.key == key) else {
        ui.weak("This set is no longer in the plan.");
        return;
    };
    let _ = export_prefix;
    ui.horizontal_wrapped(|ui| {
        ui.heading(&v.label);
        let m = v.settings.get("match_diffuse").and_then(|x| x.as_str()).map(str::to_string);
        match (m, &v.derived_match) {
            (Some(m), _) => ui.label(format!("match_diffuse: {m}")),
            (None, Some(d)) => ui.label(format!("match_diffuse: {d} (derived)")),
            (None, None) => ui.weak("no diffuse to derive a match from"),
        };
        match (&v.prefix, &v.prefix_error) {
            (Some(p), _) => ui.weak(format!("→ {p}")),
            (_, Some(e)) => ui.colored_label(RED, e),
            _ => ui.weak(""),
        };
    });
    ui.separator();
    let mut acts = vec![];
    ui.columns(2, |cols| {
        cards(&mut cols[0], model, &v, thumbs, &mut acts);
        if cols[0].button("Slot commands…").clicked() {
            cmds.open = Some(v.id.clone());
        }
        super::pgp_fields::panel(&mut cols[1], model, Some(&v.id));
    });
    super::slot_commands::draw(ui.ctx(), model, cmds);
    for a in acts {
        match a {
            Act::Select(k) => model.select_item(&k),
            Act::Expand(k) => model.expand(&k),
            Act::Format(k, f, t) => model.set_format(&k, f, t),
            Act::Opaque(k, on) => model.set_opaque(&k, on),
        }
    }
}

fn cards(ui: &mut egui::Ui, model: &AppModel, v: &SetView, thumbs: &mut ThumbCache, acts: &mut Vec<Act>) {
    egui::Grid::new(("slot-cards", &v.id)).num_columns(4).spacing([8.0, 8.0]).show(ui, |ui| {
        for (i, slot) in SlotKind::ALL.iter().enumerate() {
            card(ui, model, v, *slot, thumbs, acts);
            if i % 4 == 3 {
                ui.end_row();
            }
        }
    });
}

fn card(ui: &mut egui::Ui, model: &AppModel, v: &SetView, slot: SlotKind, thumbs: &mut ThumbCache, acts: &mut Vec<Act>) {
    let fill = v.slots.get(&slot).cloned().unwrap_or(SlotFill::Empty);
    egui::Frame::group(ui.style()).show(ui, |ui| {
        ui.set_width(CARD);
        ui.label(egui::RichText::new(slot.label()).strong());
        let (item_key, borrowed) = match &fill {
            SlotFill::Own { item } => (Some(item.clone()), None),
            SlotFill::Borrowed { from, item } => (Some(item.clone()), Some(from.clone())),
            SlotFill::Empty => (None, None),
        };
        let item = item_key.as_deref().and_then(|k| model.item(k));
        let (rect, resp) = ui.allocate_exact_size(egui::vec2(THUMB, THUMB), egui::Sense::click());
        match item.and_then(|i| thumbs.get(ui.ctx(), i)) {
            Some(t) => egui::Image::new(&t).fit_to_exact_size(rect.size()).paint_at(ui, rect),
            None => {
                ui.painter().rect_filled(rect, 2.0, ui.visuals().widgets.noninteractive.weak_bg_fill);
            }
        }
        match item {
            None => {
                ui.weak("empty");
            }
            Some(i) => {
                ui.add(egui::Label::new(egui::RichText::new(&i.name).small()).truncate());
                if let Some(from) = &borrowed {
                    ui.colored_label(super::columns::AMBER, format!("borrowed from {from}"));
                } else if let Some(job) = &i.job {
                    let (cur_f, cur_t) = (job.profile.format, job.profile.color.tag);
                    let lock = field_lock(model.target, &job.profile, "format");
                    let mut choice = (cur_f, cur_t);
                    ui.add_enabled_ui(lock.is_none(), |ui| {
                        egui::ComboBox::from_id_salt(("slot-format", &v.id, slot.label()))
                            .selected_text(format_label(cur_f, cur_t))
                            .show_ui(ui, |ui| {
                                for f in format_options(model.target) {
                                    for t in tag_options(f) {
                                        ui.selectable_value(&mut choice, (f, t), format_label(f, t));
                                    }
                                }
                            });
                    })
                    .response
                    .on_disabled_hover_text(lock.unwrap_or(""));
                    if choice != (cur_f, cur_t) {
                        acts.push(Act::Format(i.key.clone(), choice.0, choice.1));
                    }
                    if slot == SlotKind::Diffuse {
                        let mut on = model.is_opaque(&i.key);
                        if ui.checkbox(&mut on, "Opaque").on_hover_text("Strips alpha: the diffuse is fully opaque").changed() {
                            acts.push(Act::Opaque(i.key.clone(), on));
                        }
                    }
                }
                if resp.double_clicked() {
                    acts.push(Act::Expand(i.key.clone()));
                } else if resp.clicked() && borrowed.is_none() {
                    acts.push(Act::Select(i.key.clone()));
                }
            }
        }
    });
}
```

Notes:
- **`ColorTag::Unorm`:** M3a's `format_label` treats any non-sRGB tag as "linear". Check the `ColorTag` variant names in `engine::config` (`Srgb`, `Unorm`, `NotApplicable`) and use those.
- **The Opaque checkbox** only exists on an item with a job. The test expects it on rock01's diffuse, which has one.
- **The right column** calls `super::pgp_fields::panel`. Create `crates/gui/src/ui/pgp_fields.rs` now with a stub:

  ```rust
  pub fn panel(ui: &mut egui::Ui, _m: &mut AppModel, _set: Option<&str>) { ui.label("PGPatcher settings"); }
  ```

  Task 15 fills it.
- **`ui.columns` gives `&mut [Ui]`,** and `cards` needs `&AppModel` while `panel` needs `&mut AppModel`. Draw `cards` first, collecting `acts`. Then call `panel`. Then apply `acts`. The borrows don't overlap, because `cards` returns before `panel` starts.

- [ ] **Step 6: Wire it.** In `ui/mod.rs`:
  - `Selection::Set(key)` → `set_editor::draw(ui, model, &key, &mut state.thumbs, &mut state.slot_cmds)`;
  - remove `pub mod set_view;` and delete `set_view.rs` (`git rm`);
  - add `pub mod set_editor; pub mod slot_commands; pub mod pgp_fields;`;
  - add `slot_cmds: slot_commands::SlotCmdState` to `State`.

  Also add the "← set" link at the top of `settings_panel::draw` when the item has a `set_key`: `if ui.link(format!("← {}", set_label(...))).clicked() { model.select_set(&set_key) }`.

- [ ] **Step 7: Run the tests.**

Run: `cargo test -p sdc-gui --test set_editor_ui && cargo test -p sdc-gui`
Expected: all pass. The M3a tests that exercised `set_view` should still find their labels: member names, recipe lines and warnings are still drawn. Adjust a test only where it named a removed widget, and keep its intent.

- [ ] **Step 8: Run the gates and commit.**

```bash
cargo fmt --all && cargo clippy --workspace --all-targets -- -D warnings && cargo test -p sdc-gui
git rm crates/gui/src/ui/set_view.rs
git add crates/gui/src/ui/set_editor.rs crates/gui/src/ui/slot_commands.rs crates/gui/src/ui/pgp_fields.rs crates/gui/src/ui/mod.rs crates/gui/src/ui/settings_panel.rs crates/gui/src/model/mod.rs crates/gui/tests/set_editor_ui.rs
git commit -m "feat(gui): set editor layout A: slot cards, per-slot preset, Opaque on the diffuse, slot commands (M3b §4.1–4.2)

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

---

### Task 15: PGPatcher settings, the Advanced drawer, the Defaults card and the JSON name (spec §4.3–4.5, §6.1)

**Files:**
- Modify:
  - `crates/gui/src/ui/pgp_fields.rs` (the full panel);
  - `crates/gui/src/ui/advanced.rs` (a "PGPatcher (advanced)" section for the selected set or Defaults);
  - `crates/gui/src/ui/mod.rs` (`Selection::Defaults` → the Defaults editor);
  - `crates/gui/src/ui/header.rs` (a JSON name field on CS);
  - `crates/gui/src/model/mod.rs` (`set_json_name`).
- Test: `crates/gui/tests/set_editor_ui.rs` (append)

**Interfaces:**
- Consumes:
  - `pgp::schema::{KEYS, KeyKind, Group, spec}`;
  - `pgp::settings::{Feature, FEATURE_KEYS, feature_key, blocked_by, effective, feature_on, GLINT_DEFAULTS}`;
  - `AppModel::set_json_value` (Task 10);
  - `pbr_export::json_name`.
- Produces:
  - `pgp_fields::panel(ui, model, set: Option<&str>)`, where `None` means the Defaults card;
  - `pgp_fields::advanced(ui, model, set: Option<&str>)`;
  - `AppModel::set_json_name(&mut self, name: &str)`: an undo step, no re-plan.

**Behaviour:**
- **Groups in the main view, in order:** Match (only for a set, not Defaults), Material, Features, then each enabled feature's group (Emissive, Parallax, Subsurface, Multilayer, Glint, Fuzz). Only keys with `advanced == false` appear; `Slot` keys never do (the dialog owns them).
- **Controls per kind:**

  | Kind | Control | Writes |
  |---|---|---|
  | `AlwaysBool`, `OnlyTrue` | Checkbox | `true`, or `false`/remove |
  | `Object` | Checkbox | `true`, or remove |
  | `Tri` | Three buttons "not set / on / off" | `None` / `true` / `false` |
  | `Number` | `DragValue` | The number |
  | `Text` | Text field, committed on Enter or focus loss | The text, or remove when empty |
  | `Rgb`, `Rgba` | Three `DragValue`s 0–255, labelled R G B | `[r, g, b]` |

  Details:
  - `AlwaysBool` unticked writes `false`; `OnlyTrue` and `Object` unticked remove the key.
  - A `Text` field shows "(derived)" with the derived value as hint text for `match_diffuse`.
  - Colours have no 0–1 toggle (owner).
  - Turning glint on writes only `glint = true`; the writer fills the defaults. The four fields show `GLINT_DEFAULTS` as values until edited.
- **Greying:** a feature checkbox is disabled when `blocked_by(effective, f)` is `Some(b)`, with the hover text `off: <b> is on`. Its dependent group isn't shown while it's off.
- **Inherited values:** in a set, a key without its own value but present in the defaults shows the default value **greyed**, with "from defaults" and a **reset** button that removes the set's own value (shown only when the set has its own value).
- **Sliders and `DragValue`s** send one `set_json_value` per change. The model merges consecutive changes of the same key into one undo step (Task 10's merge key).
- **The Advanced drawer** gains a "PGPatcher (advanced)" collapsing section when a set or Defaults is selected. It lists every `advanced == true` key, grouped by Match / Uv / Vertex / Locks / Multilayer, with the same controls.
  - Choosing another match mode (setting `match_normal`, `path_contains` or `matchN`) is just setting that key. `match_mode` then omits `match_diffuse`.
  - The Match group shows which mode is active ("Matching by: match2").
- **The header** shows, on CS, "JSON:" and a text field with `json_name(model)` as hint text. Editing it calls `model.set_json_name(text)`, and an empty field restores the default.

- [ ] **Step 1: Write the failing tests.** Append to `crates/gui/tests/set_editor_ui.rs`:

```rust
#[test]
fn feature_toggles_grey_their_exclusions_and_reveal_their_groups() {
    let d = tempfile::tempdir().unwrap();
    let mut m = pbr_opened(d.path());
    let key = m.sets.iter().find(|s| s.key.ends_with("landscape/rock01")).unwrap().key.clone();
    m.select_set(&key);
    let mut h = harness(m);
    h.run();
    h.get_by_label("Specular level");
    assert!(h.query_by_label("Displacement scale").is_none(), "parallax group hidden while off");
    h.get_by_label("Parallax").click();
    h.run();
    h.get_by_label("Displacement scale");
    h.get_by_label("Hair").click();
    h.run();
    let e = h.state().sets_doc.as_ref().unwrap().entries().into_iter().find(|e| e.id == "landscape/rock01").unwrap();
    assert_eq!(e.settings["parallax"], serde_json::json!(true));
    assert!(!e.settings.contains_key("hair"), "hair is greyed while parallax is on");
}

#[test]
fn the_defaults_card_edits_the_default_block() {
    let d = tempfile::tempdir().unwrap();
    let mut m = pbr_opened(d.path());
    m.selection = Selection::Defaults;
    m.set_json_value(None, "specular_level", Some(serde_json::json!(0.04)));
    let mut h = harness(m);
    h.run();
    h.get_by_label("Specular level");
    assert!(h.query_by_label("Match diffuse").is_none(), "match fields are per set only");
    assert_eq!(h.state().sets_doc.as_ref().unwrap().defaults()["specular_level"], serde_json::json!(0.04));
}

#[test]
fn the_json_name_is_editable_and_undoable() {
    let d = tempfile::tempdir().unwrap();
    let mut m = pbr_opened(d.path());
    m.set_json_name("rock-pack");
    assert_eq!(sdc_gui::model::pbr_export::json_name(&m), "rock-pack");
    m.undo();
    assert_eq!(sdc_gui::model::pbr_export::json_name(&m), "textures");
}
```

`query_by_label` is kittest's optional query. If the 0.36.2 `Queryable` names it differently, use the equivalent that returns an `Option`.

- [ ] **Step 2: Run the tests and confirm they fail.**

Run: `cargo test -p sdc-gui --test set_editor_ui`
Expected: FAIL (the stub panel).

- [ ] **Step 3: Add `set_json_name`** to `model/mod.rs`:

```rust
    /// The PGPatcher JSON file name (header field); empty restores the default (the folder's name). No re-plan (P7).
    pub fn set_json_name(&mut self, name: &str) {
        let n = name.trim().to_string();
        self.edit_docs("JSON name", Some("json_name".into()), false, move |_, s| {
            s.set_json_name(if n.is_empty() { None } else { Some(&n) })
        });
    }
```

- [ ] **Step 4: Write `crates/gui/src/ui/pgp_fields.rs`:**

```rust
//! PGPatcher settings controls (M3b spec §4.3–4.5): the main groups for a set or the Defaults card, and the advanced
//! keys for the Advanced drawer. Drawing only; edits go through `AppModel::set_json_value` after drawing.
use crate::model::AppModel;
use crate::pgp::prefix::{match_mode, MatchMode};
use crate::pgp::schema::{Group, KeyKind, KeySpec, KEYS};
use crate::pgp::settings::{blocked_by, effective, feature_key, feature_on, Feature, Settings, FEATURE_KEYS, GLINT_DEFAULTS};
use eframe::egui;
use serde_json::{json, Value};

type Edit = (String, Option<Value>);

fn feature_of(key: &str) -> Option<Feature> {
    FEATURE_KEYS.iter().find(|(_, k)| *k == key).map(|(f, _)| *f)
}

fn group_open(g: Group, eff: &Settings) -> bool {
    let f = match g {
        Group::Emissive => Feature::Emissive,
        Group::Parallax => Feature::Parallax,
        Group::Subsurface => Feature::Subsurface,
        Group::Multilayer => Feature::Multilayer,
        Group::Glint => Feature::Glint,
        Group::Fuzz => Feature::Fuzz,
        _ => return true,
    };
    feature_on(eff, f) && blocked_by(eff, f).is_none()
}

fn group_title(g: Group) -> &'static str {
    match g {
        Group::Match => "Match",
        Group::Material => "Material",
        Group::Features => "Features",
        Group::Emissive => "Emissive settings",
        Group::Parallax => "Parallax settings",
        Group::Subsurface => "Subsurface settings",
        Group::Multilayer => "Coat settings",
        Group::Glint => "Glint settings",
        Group::Fuzz => "Fuzz settings",
        Group::Vertex => "Vertex colours and depth",
        Group::Locks => "Texture locks",
        Group::Uv => "UV and cubemap",
        Group::Slots => "Slots",
    }
}

const MAIN: [Group; 9] = [
    Group::Match,
    Group::Material,
    Group::Features,
    Group::Emissive,
    Group::Parallax,
    Group::Subsurface,
    Group::Multilayer,
    Group::Glint,
    Group::Fuzz,
];
const ADVANCED: [Group; 5] = [Group::Match, Group::Uv, Group::Vertex, Group::Locks, Group::Multilayer];

struct Ctx<'a> {
    own: Settings,
    eff: Settings,
    defaults: Settings,
    derived: Option<String>,
    set: Option<&'a str>,
}

fn context<'a>(model: &AppModel, set: Option<&'a str>) -> Ctx<'a> {
    let defaults = model.sets_doc.as_ref().map(|d| d.defaults()).unwrap_or_default();
    let (own, derived) = match set {
        None => (defaults.clone(), None),
        Some(id) => {
            let v = crate::model::sets::set_views(model).into_iter().find(|v| v.id.eq_ignore_ascii_case(id));
            (v.as_ref().map(|v| v.settings.clone()).unwrap_or_default(), v.and_then(|v| v.derived_match))
        }
    };
    let eff = if set.is_some() { effective(&defaults, &own) } else { defaults.clone() };
    Ctx { own, eff, defaults, derived, set }
}

pub fn panel(ui: &mut egui::Ui, model: &mut AppModel, set: Option<&str>) {
    let c = context(model, set);
    let editable = model.sets_doc.as_ref().is_some_and(|d| !d.read_only());
    let mut edits: Vec<Edit> = vec![];
    ui.add_enabled_ui(editable, |ui| {
        for g in MAIN {
            if (g == Group::Match && set.is_none()) || !group_open(g, &c.eff) {
                continue;
            }
            let keys: Vec<&KeySpec> = KEYS.iter().filter(|k| k.group == g && !k.advanced && k.kind != KeyKind::Slot).collect();
            if keys.is_empty() {
                continue;
            }
            ui.label(egui::RichText::new(group_title(g)).strong());
            egui::Grid::new(("pgp", g as u8, set.unwrap_or("defaults"))).num_columns(3).show(ui, |ui| {
                for k in keys {
                    row(ui, k, &c, &mut edits);
                    ui.end_row();
                }
            });
            ui.add_space(6.0);
        }
    });
    for (k, v) in edits {
        model.set_json_value(set, &k, v);
    }
}

pub fn advanced(ui: &mut egui::Ui, model: &mut AppModel, set: Option<&str>) {
    let c = context(model, set);
    let editable = model.sets_doc.as_ref().is_some_and(|d| !d.read_only());
    let mut edits: Vec<Edit> = vec![];
    ui.add_enabled_ui(editable, |ui| {
        if set.is_some() {
            let mode = match match_mode(&c.eff) {
                MatchMode::Diffuse => "match_diffuse".to_string(),
                MatchMode::Normal => "match_normal".into(),
                MatchMode::PathContains => "path_contains".into(),
                MatchMode::Slot(n) => format!("match{n}"),
            };
            ui.label(format!("Matching by: {mode}"));
        }
        for g in ADVANCED {
            if (g == Group::Match && set.is_none()) || !group_open(g, &c.eff) {
                continue;
            }
            let keys: Vec<&KeySpec> = KEYS.iter().filter(|k| k.group == g && k.advanced).collect();
            if keys.is_empty() {
                continue;
            }
            ui.label(egui::RichText::new(group_title(g)).strong());
            egui::Grid::new(("pgp-adv", g as u8, set.unwrap_or("defaults"))).num_columns(3).show(ui, |ui| {
                for k in keys {
                    row(ui, k, &c, &mut edits);
                    ui.end_row();
                }
            });
        }
    });
    for (k, v) in edits {
        model.set_json_value(set, &k, v);
    }
}

/// One row: label, control, and "from defaults" / reset.
fn row(ui: &mut egui::Ui, k: &KeySpec, c: &Ctx, edits: &mut Vec<Edit>) {
    let own = c.own.get(k.key);
    let inherited = own.is_none() && c.set.is_some() && c.defaults.contains_key(k.key);
    let value = own.or_else(|| c.eff.get(k.key)).cloned();
    let block = feature_of(k.key).and_then(|f| blocked_by(&c.eff, f).filter(|_| !feature_on(&c.eff, f)));
    ui.label(k.label);
    let r = ui
        .scope(|ui| {
            if block.is_some() {
                ui.disable();
            }
            if inherited {
                ui.style_mut().visuals.override_text_color = Some(ui.visuals().weak_text_color());
            }
            control(ui, k, value, c, edits)
        })
        .inner;
    if let (Some(b), Some(r)) = (block, r) {
        r.on_disabled_hover_text(format!("off: {} is on", feature_key(b)));
    }
    if own.is_some() && c.set.is_some() {
        if ui.small_button("reset").on_hover_text("Use the default again").clicked() {
            edits.push((k.key.to_string(), None));
        }
    } else if inherited {
        ui.weak("from defaults");
    } else {
        ui.label("");
    }
}

fn control(ui: &mut egui::Ui, k: &KeySpec, value: Option<Value>, c: &Ctx, edits: &mut Vec<Edit>) -> Option<egui::Response> {
    let key = k.key.to_string();
    match k.kind {
        KeyKind::AlwaysBool | KeyKind::OnlyTrue | KeyKind::Object => {
            let mut on = value.and_then(|v| v.as_bool()).unwrap_or(false);
            let r = ui.checkbox(&mut on, "");
            if r.changed() {
                let v = match (k.kind, on) {
                    (_, true) => Some(json!(true)),
                    (KeyKind::AlwaysBool, false) => Some(json!(false)),
                    _ => None,
                };
                edits.push((key, v));
            }
            Some(r)
        }
        KeyKind::Tri => {
            let cur = value.and_then(|v| v.as_bool());
            let mut out = None;
            let r = ui
                .horizontal(|ui| {
                    for (label, v) in [("not set", None), ("on", Some(true)), ("off", Some(false))] {
                        if ui.selectable_label(cur == v, label).clicked() && cur != v {
                            out = Some(v.map(Value::Bool));
                        }
                    }
                })
                .response;
            if let Some(v) = out {
                edits.push((key, v));
            }
            Some(r)
        }
        KeyKind::Number => {
            let default = GLINT_DEFAULTS.iter().find(|(d, _)| *d == k.key).map(|(_, x)| *x);
            let mut x = value.and_then(|v| v.as_f64()).or(default).unwrap_or(0.0);
            let r = ui.add(egui::DragValue::new(&mut x).speed(0.01));
            if r.changed() {
                edits.push((key, Some(json!(x))));
            }
            Some(r)
        }
        KeyKind::Text => {
            let id = egui::Id::new(("pgp-text", k.key, c.set.unwrap_or("defaults")));
            let mut text = ui.data(|d| d.get_temp::<String>(id)).unwrap_or_else(|| value.and_then(|v| v.as_str().map(str::to_string)).unwrap_or_default());
            let hint = if k.key == "match_diffuse" { c.derived.clone().map(|d| format!("{d} (derived)")).unwrap_or_default() } else { String::new() };
            let r = ui.add(egui::TextEdit::singleline(&mut text).hint_text(hint).desired_width(220.0));
            if r.changed() {
                ui.data_mut(|d| d.insert_temp(id, text.clone()));
            }
            if r.lost_focus() {
                ui.data_mut(|d| d.remove::<String>(id));
                let v = if text.trim().is_empty() { None } else { Some(json!(text.trim())) };
                edits.push((key, v));
            }
            Some(r)
        }
        KeyKind::Rgb | KeyKind::Rgba => {
            let mut rgb: [i64; 3] = value
                .and_then(|v| v.as_array().cloned())
                .map(|a| [0, 1, 2].map(|i| a.get(i).and_then(Value::as_i64).unwrap_or(0)))
                .unwrap_or([0, 0, 0]);
            let mut changed = false;
            let r = ui
                .horizontal(|ui| {
                    for (i, l) in ["R", "G", "B"].iter().enumerate() {
                        ui.label(*l);
                        changed |= ui.add(egui::DragValue::new(&mut rgb[i]).range(0..=255)).changed();
                    }
                })
                .response;
            if changed {
                edits.push((key, Some(json!(rgb))));
            }
            Some(r)
        }
        KeyKind::Slot => None,
    }
}
```

**Checkbox labels:** kittest finds controls by label. The row's first column is the label, and the checkbox carries no text, so the test's `get_by_label("Parallax").click()` clicks the **label**. Make the label the clickable part: draw `AlwaysBool`/`OnlyTrue`/`Object` rows as `ui.checkbox(&mut on, k.label)`, with an empty first column, instead of a separate label plus an empty checkbox. Adapt `row()` for those kinds.

- [ ] **Step 5: Wire the Defaults card, the Advanced section and the header.**
  - In `ui/mod.rs` `centre`: `Selection::Defaults` → a heading "Defaults" with the subtitle "Values every set's JSON entry inherits (the PGPatcher default block)", then `pgp_fields::panel(ui, model, None)` inside a scroll area.
  - In `ui/advanced.rs`, after the inspector section, when the selection is a set (find its id with `set_views`) or `Defaults`:

    ```rust
    egui::CollapsingHeader::new("PGPatcher (advanced)")
        .default_open(true)
        .show(ui, |ui| super::pgp_fields::advanced(ui, model, set_id.as_deref()));
    ```

    `advanced::draw` holds `&mut AppModel`. Collect `set_id` before the panel closure.
  - In `ui/header.rs`, on CS and with a folder open, add this before the export folder:

    ```rust
    ui.label("JSON:");
    let id = egui::Id::new("json-name");
    let mut t = ui.data(|d| d.get_temp::<String>(id))
        .unwrap_or_else(|| model.sets_doc.as_ref().and_then(|d| d.json_name()).unwrap_or_default());
    let r = ui.add(egui::TextEdit::singleline(&mut t)
        .hint_text(crate::model::pbr_export::json_name(model))
        .desired_width(120.0));
    if r.changed() { ui.data_mut(|d| d.insert_temp(id, t.clone())); }
    if r.lost_focus() { ui.data_mut(|d| d.remove::<String>(id)); model.set_json_name(&t); }
    ```

    The hover text is "The PGPatcher JSON is written to <export>\\PBRNIFPatcher\\<name>.json".

- [ ] **Step 6: Run the tests.**

Run: `cargo test -p sdc-gui --test set_editor_ui && cargo test -p sdc-gui`
Expected: all pass.

- [ ] **Step 7: Run the gates and commit.**

```bash
cargo fmt --all && cargo clippy --workspace --all-targets -- -D warnings && cargo test -p sdc-gui
git add crates/gui/src/ui/pgp_fields.rs crates/gui/src/ui/advanced.rs crates/gui/src/ui/mod.rs crates/gui/src/ui/header.rs crates/gui/src/model/mod.rs crates/gui/tests/set_editor_ui.rs
git commit -m "feat(gui): PGPatcher settings with greying and inheritance, Advanced keys, Defaults card, JSON name (M3b §4.3–4.5)

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

---
### Task 16: Docs, decisions and the roadmap (spec §9; DECISIONS Note P)

**Files:**
- Modify:
  - `docs/DECISIONS.md` (new Note P);
  - `docs/gui.md` (a "Sets and the PGPatcher JSON" section);
  - `docs/superpowers/plans/2026-10-08-skyrim-dds-converter-phase0-2.md` (M3 section: "Consumed by M3b"; later-round list; mark the M3a "fix first" item done);
  - `docs/superpowers/specs/2026-10-10-sdc-m3b-set-editor-design.md` (Status line; a pointer to Note P beside §2.1's `SetOutput`, §5.4's "slot 6 (TX06)", and §7.1's `key =`).
- Test: none (docs). Run `cargo test --workspace` once to confirm nothing else moved.

**Interfaces:** none.

- [ ] **Step 1: DECISIONS Note P.** Append after Note O, in the existing note style:

```markdown
## Note P — M3b set editor and PGPatcher JSON decisions (owner and M3b plan, 2026-10-10)

The owner approved the M3b design section by section (spec `docs/superpowers/specs/2026-10-10-sdc-m3b-set-editor-design.md`); the plan refined it as follows.

- **P1 Output paths, not prefixes** (plan): the engine takes `output_paths` (explicit paths per source file or per packed recipe) instead of a per-set prefix. The ownership rule (§3.3) can place a shared output at another set's prefix; explicit paths say that, and the engine stays free of PGPatcher concepts.
- **P2 Delete and order are GUI-only** (plan): the engine's `SetEdit` has Create, Rename, Move, Remove. Deleting a created set drops its edits (its files regroup automatically); order lives in `sdc-project.toml`.
- **P3 Only created sets can be deleted** (plan): an automatic set would regroup at once.
- **P4 Vanilla/CPM borrowed slots are file copies** (plan): copied after the run, listed in the report, not tracked by the manifest.
- **P5 A PBR set has a `pbr_albedo` diffuse** (plan): the `set_resolved.pbr` flag is path-based and misses user-built sets.
- **P6 Slots are PGPatcher's 1-based `slotN`** (plan): `_cnr`/`_f` share slot 7 (TX06), `_s` is slot 8.
- **P7 JSON-only edits do not re-plan** (plan): match, rename, set and `[files]` edits do.
- **P8 A bad `sdc-project.toml` makes sets and JSON read-only** (plan); `sdc.toml` editing continues.
- **P9 Undo keeps document snapshots** (plan), 200 steps; Duplicate source records the copy and its SHA-256.
- **P10 Colours are stored 0–255** (owner: edited as 0–255) and written as 0–1 with 4 decimals; `emissive_color` gets alpha 1.0 (owner).
- **P11 Borrowed slot paths are written with `textures\`** (plan).
- **P12 In-tree schema validation** (plan): no new dependency; a test fails if the vendored schema uses a keyword the validator lacks.
- **P13 Invalid path characters block export** (plan) rather than being rewritten: a changed path is not where PGPatcher looks.
- **P14 GUI = CLI holds for non-PBR sets only** (owner: the JSON is GUI-only): a PBR set on CS is written under `textures\pbr\` by the GUI alone.
- **P15 "Not in a set" is a clickable count** in the Textures header (plan), not an automatically switched filter.
- **Owner, in the design conversation:** `emissive`, `parallax`, `subsurface` are binary and always written; `hair` and `multilayer` binary, written only when on; `lock_*`, `vertex_colors`, `zbuffer_write` three-state; mutual exclusion greys (hair excludes multilayer, glint, fuzz, emissive, parallax, subsurface; multilayer excludes subsurface; multilayer/glint/fuzz exclude each other); a set cannot hold both `_cnr` and `_f`; a feature on without its texture blocks export (no lock quick fix); `delete` and hex colour entry are later rounds; Opaque sits on the Diffuse card.
```

- [ ] **Step 2: `docs/gui.md`.** Add a section (keep the file's tone) covering:
  - the editable Sets column: drag a texture onto a set, onto empty space, Share / Duplicate, reorder by ≡, the right-click menu, badges;
  - the set editor: slot cards, the preset dropdown, Opaque, Slot commands;
  - the PGPatcher settings: greyed features and why, defaults and reset;
  - the JSON: `<export>\PBRNIFPatcher\<name>.json`, written after a complete export, only sets that fully exported, the overwrite prompt;
  - `sdc-project.toml` next to `sdc.toml`;
  - undo/redo (Ctrl+Z, Ctrl+Y).

- [ ] **Step 3: The phase plan's M3 section.**
  - Add **"Consumed by M3b"**: the stale `after_export` fix (Task 1).
  - Move the M3b later-round list (spec §9) into a new **"Later GUI rounds (M3b-2 and after)"** list:
    - import and merge of existing PBR mods;
    - landscape EDID JSON;
    - rule-table and suffix-alias editors, and the source/mod `_m` switch;
    - NIF reading;
    - the `delete` key;
    - hex colour paste and a colour picker;
    - refining the derived `match_diffuse`.
  - Add to M4's carried items:
    - "Borrowed roughness slots are marked shared (`sdc-project.toml` `borrow.slot6`); M4 must not bake normal variance into a shared roughness and must renormalise only (v1 instruction; M3b spec §3.3)."

- [ ] **Step 4: The M3b spec.**
  - Change the Status line to "Owner-approved 2026-10-10. Built on `feat/m3b-set-editor` (plan `docs/superpowers/plans/2026-10-10-sdc-m3b-set-editor.md`); refinements in DECISIONS Note P."
  - Add one-line pointers:
    - §2.1: "→ P1/P2: `output_paths`, no Delete/Order in the engine";
    - §5.4: "→ P6: slot 7 (TX06)";
    - §7.1: "→ the file uses `id =` and an explicit `[project] order` (Task 4)".

- [ ] **Step 5: Run every gate and commit.**

```bash
cargo fmt --all && cargo clippy --workspace --all-targets -- -D warnings && cargo test --workspace
git add docs/DECISIONS.md docs/gui.md docs/superpowers/plans/2026-10-08-skyrim-dds-converter-phase0-2.md docs/superpowers/specs/2026-10-10-sdc-m3b-set-editor-design.md
git commit -m "docs: DECISIONS Note P, gui.md sets and PGPatcher JSON, M3b roadmap updates

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

---

### Task 17: Full flow (spec §10)

**Files:**
- Create: `crates/gui/tests/m3b_flow.rs`
- Modify: none, unless the flow finds a defect. Fix it in the owning module and say so in the report.

**Interfaces:**
- Consumes: everything above, through the model API only.
- Produces: the end-to-end regression test.

**The flow:**
1. Open the M3a fixture and decide rock01 as PBR (`pbr_opened`).
2. Create a second PBR set from `odd.png`. Decide it as `pbr_albedo` and acknowledge `DIM-NPOT`, as `gui_repro` does.
3. Make it borrow rock01's `_rmaos`, then reorder so rock01 stays first (rock01 owns).
4. Set a slot command `slot3 = effects\white.dds` on rock01.
5. Turn parallax on for the new set. With no `_p`, export is blocked. Turn it off again: unblocked.
6. Export.
7. Assert:
   - the `.dds` files under `textures\pbr\…`, and the borrowed `_rmaos` written once;
   - the JSON validates, has 2 entries in set order, has rock01's `slot3`, and points the second entry's `slot6` at rock01's `_rmaos`;
   - the hash is recorded.
8. Export again: no file changes (every row up to date; the JSON hash is unchanged; no overwrite prompt).

- [ ] **Step 1: Write the test.** Create `crates/gui/tests/m3b_flow.rs`:

```rust
mod support;
use engine::codes::Code;
use engine::config::Subtype;
use sdc_gui::model::pbr_export::{export_blocks, json_name};
use sdc_gui::model::sets::{set_views, DropChoice, DropOutcome, SlotCommand};
use sdc_gui::model::{Answer, Dialog};
use serde_json::{json, Value};
use std::time::Duration;
use support::*;

fn idle(m: &mut sdc_gui::model::AppModel) {
    assert!(m.wait_idle(Duration::from_secs(120)));
}

fn export(m: &mut sdc_gui::model::AppModel) {
    m.export_clicked();
    if matches!(m.dialog, Some(Dialog::ProceedAnyway(_))) {
        m.proceed_anyway();
    }
    idle(m);
}

#[test]
fn build_sets_export_and_reexport_unchanged() {
    let d = tempfile::tempdir().unwrap();
    let mut m = pbr_opened(d.path());
    let odd = m.items.iter().find(|i| i.name == "odd.png").unwrap().key.clone();
    m.decide(&odd, Answer::Subtype(Subtype::PbrAlbedo));
    m.decide(&odd, Answer::Acknowledge(Code::DimNpot));
    idle(&mut m);
    let second = m.drop_new_set(&odd).unwrap();
    idle(&mut m);
    let rmaos = m.items.iter().find(|i| i.name.ends_with("rmaos")).unwrap().key.clone();
    assert_eq!(m.drop_on_set(&rmaos, &second), DropOutcome::AskChoice { from: "landscape/rock01".into() });
    assert_eq!(m.drop_choice(&rmaos, &second, DropChoice::Share), DropOutcome::Joined);
    m.reorder_set("landscape/rock01", 0);
    m.set_slot_command("landscape/rock01", 3, SlotCommand::Path(r"effects\white.dds".into())).unwrap();
    idle(&mut m);

    m.set_json_value(Some(&second), "parallax", Some(json!(true)));
    assert!(export_blocks(&m).iter().any(|i| i.text.contains("parallax")));
    m.set_json_value(Some(&second), "parallax", Some(json!(false)));
    assert_eq!(export_blocks(&m), vec![]);

    export(&mut m);
    let out = m.export_root();
    assert!(out.join("textures/pbr/landscape/rock01.dds").exists());
    assert!(out.join("textures/pbr/landscape/rock01_rmaos.dds").exists());
    let views = set_views(&m);
    let second_prefix = views.iter().find(|v| v.id == second).unwrap().prefix.clone().unwrap();
    let second_rmaos = format!("{}_rmaos.dds", second_prefix.replace('\\', "/"));
    assert!(!out.join(&second_rmaos).exists(), "the borrowed _rmaos is written once, at the owner's path");

    let p = out.join("PBRNIFPatcher").join(format!("{}.json", json_name(&m)));
    let text = std::fs::read_to_string(&p).unwrap_or_else(|e| panic!("{e}; notes {:?}", m.export.notes));
    let v: Value = serde_json::from_str(&text).unwrap();
    sdc_gui::pgp::validate::validate(&v).unwrap();
    let entries = v["entries"].as_array().unwrap();
    assert_eq!(entries.len(), 2, "{text}");
    assert_eq!(entries[0]["match_diffuse"], json!(r"landscape\rock01"));
    assert_eq!(entries[0]["slot3"], json!(r"effects\white.dds"));
    assert_eq!(entries[1]["slot6"], json!(r"textures\pbr\landscape\rock01_rmaos.dds"));
    assert_eq!(entries[1]["parallax"], json!(false));
    let sha = m.sets_doc.as_ref().unwrap().last_json_sha256();

    // Export again: nothing changes.
    let before: Vec<(String, std::time::SystemTime)> = walk(&out);
    export(&mut m);
    assert_eq!(m.dialog, None, "no overwrite prompt for our own JSON");
    assert_eq!(m.sets_doc.as_ref().unwrap().last_json_sha256(), sha);
    let after = walk(&out);
    let changed: Vec<_> = after
        .iter()
        .filter(|(f, t)| !f.ends_with(".json") && !f.contains("_sdc") && before.iter().any(|(g, u)| g == f && u != t))
        .collect();
    assert!(changed.is_empty(), "re-export rewrote {changed:?}");
}

fn walk(dir: &std::path::Path) -> Vec<(String, std::time::SystemTime)> {
    let mut v = vec![];
    for e in walkdir::WalkDir::new(dir).into_iter().flatten().filter(|e| e.file_type().is_file()) {
        v.push((e.path().to_string_lossy().replace('\\', "/"), e.metadata().unwrap().modified().unwrap()));
    }
    v.sort();
    v
}
```

`walkdir` is a workspace dependency. Add `walkdir.workspace = true` to the GUI's `[dev-dependencies]`; it's already in `Cargo.lock`, so nothing is downloaded. The JSON is rewritten with identical bytes on the second export (its hash is unchanged), so the `.json` file is excluded from the mtime comparison.

- [ ] **Step 2: Run the test.**

Run: `cargo test -p sdc-gui --test m3b_flow -- --nocapture`
Expected: PASS. If an assertion fails, the message names the stage. Find the defect in the owning module (sets model, export, writer), fix it there, and add a focused test next to that module's tests. Don't loosen this test.

- [ ] **Step 3: Run the whole suite and the release build.**

```bash
cargo fmt --all && cargo clippy --workspace --all-targets -- -D warnings && cargo test --workspace
cargo build --release -p sdc -p sdc-gui
```

Expected: all green. Report the test totals (passed / failed / ignored) in the task report.

- [ ] **Step 4: Commit.**

```bash
git add crates/gui/tests/m3b_flow.rs crates/gui/Cargo.toml Cargo.lock
git commit -m "test(gui): M3b full flow: build sets, block, export DDS + PGPatcher JSON, re-export unchanged (M3b §10)

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

Add any module you fixed to the commit by explicit path.

---

## Owner exit check (not a task; after the final review)

1. Use a small CS test mod: 3–5 PBR sets, one shared `_rmaos`, one set using a slot command.
2. Run PGPatcher on the exported mod and confirm in game that the meshes are patched as expected.
   - Check `match_normal` especially, if used: its prefix rule is an assessment (Task 7).
3. Do a usability pass on the Sets column (dragging in a real window) and the set editor.
