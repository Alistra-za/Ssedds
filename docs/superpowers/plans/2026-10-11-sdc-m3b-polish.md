# M3b Polish (owner usability pass, 2026-10-11) Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development. Steps use checkbox (`- [ ]`) syntax.

**Goal:** Fix the owner's first-use findings in `sdc-gui` after M3b: alpha-warning noise, PBR set member types, the undecided-texture export dialog, and five UI affordances.

**Architecture:** These are small changes to existing modules.
- Engine: `policy.rs` (one validation rule).
- Model: `crates/gui/src/model/` (PBR propagation; the undecided-texture list for the dialog).
- UI: `crates/gui/src/ui/`: preview, dialogs, header, sets_column, settings_panel.

No new files are needed except tests.

**Tech Stack:** Rust; egui/eframe 0.36.2; egui_kittest 0.36.2.

**Spec:** The design was approved in chat on 2026-10-11 (owner said "go"). The items are reproduced verbatim under each task. Binding background is the M3b spec `docs/superpowers/specs/2026-10-10-sdc-m3b-set-editor-design.md` and DECISIONS Notes M, P.

## Global Constraints

- **Gates:** `cargo fmt --all && cargo clippy --workspace --all-targets -- -D warnings && cargo test --workspace`. Run clippy before every commit.
- **Golden and gui_repro:** `crates/engine/tests/golden.rs` and `crates/cli/tests/gui_repro.rs` must stay green.
  - If Task 1 changes a golden expectation, update only that expectation and name it in the report.
- **Commits:** commit only explicit paths. End each commit message with `Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>`. Do not push.
- **Python:** never run `python -` or `python3 -` with stdin.
- **Model/UI split:** the model holds no egui types. Every user edit goes through `AppModel::edit`/`edit_docs`, so it is undoable.
- **P8 read-only state:** when `sdc-project.toml` is read-only, set controls are disabled and their hover text shows the reason. `sdc.toml` controls are disabled only when `sdc.toml` itself is read-only.

## Review Focus

1. **The alpha rule must not hide real transparency loss.** A `diffuse` or `pbr_albedo` with varying alpha and an *inferred* `remove` policy still blocks.
2. **PBR propagation must never override a sub-type the user chose for a member.** Reverting the diffuse to vanilla reverts only the members that were auto-switched.
3. **"Accept all suggestions" never decides a texture that has no suggestion.** Proceed anyway still skips whatever is left undecided.
4. **Pill drag feedback must not break drag-to-reorder or DragItem drops.** This is the Task 13 fix: a DragSet payload must never be consumed by a DragItem zone.

---

### Task 1: The alpha warning is too broad (engine)

**Owner:** "the alpha warning is too broad, i think half of the textures in /glass displayed it."

**Finding:**
- In `F:/ai/dds/glass/m`, every `_n` normal has a real vanilla specular map in alpha (247 distinct values).
- Converted as `pbr_normal`, the alpha is removed, and `LOSS-ALPHA-DATA` (Blocking) fires at `crates/engine/src/policy.rs` ~line 930.
- The owner had to `allow` it on every normal.

**Approved rule:**
- `pbr_normal`: when the alpha is discarded, emit an **Info** code instead of `LOSS-ALPHA-DATA`, with the text `"vanilla specular in alpha dropped (PBR normals don't use alpha)"`.
  - Add a new Info code, e.g. `ALPHA-PBR-NORMAL-DROPPED`, to `codes.rs`.
  - Regenerate the schemas if the code list is in them: `cargo run -p sdc -- presets schema all --out schema`.
  - Update the codes count test.
- **Explicit remove:** when the user explicitly set `alpha.policy = "remove"` (provenance from `[files]`/`--set`, not a sub-type default), no `LOSS-ALPHA-DATA` fires; the user already decided.
  - Find how provenance is available to the policy check, e.g. `job.provenance["alpha.policy"]`, or the source of the resolved field.
  - If provenance isn't reachable there, report NEEDS_CONTEXT with what you found. Do not guess.
- **Everything else is unchanged.** In particular, a varying-alpha `diffuse`/`pbr_albedo` with an inferred `remove` still blocks.

**Files:**
- `crates/engine/src/policy.rs`, `crates/engine/src/codes.rs`;
- the schema output if it changes;
- docs: `docs/DECISIONS.md` (append a line to Note P as **P25**, owner 2026-10-11), and the spec's code table if codes are listed there.

**Tests** (engine; follow the existing policy/validate test style):
- (a) a varying-alpha `pbr_normal` with an inferred remove gives the new Info code and no `LOSS-ALPHA-DATA`;
- (b) a varying-alpha `diffuse` with an explicit `alpha.policy = "remove"` in `[files]` gives no `LOSS-ALPHA-DATA`;
- (c) a varying-alpha `diffuse` with an inferred remove still gives `LOSS-ALPHA-DATA`;
- (d) a varying-alpha vanilla `normal` with `alpha data` is unchanged.

Commit: `fix(engine): PBR normal alpha drop and explicit alpha removal no longer block (LOSS-ALPHA-DATA too broad, owner)`.

### Task 2: PBR set members follow the diffuse; PBR types grouped in the dropdown

**Owner:** "A PBR texture should set all of it's members to pbr values normal > pbr normal, they should also all be grouped together in the dropdown." Also: "height stays height."

**Rules:**
- **Diffuse becomes PBR.** When a set's Diffuse-slot member is decided or changed to `pbr_albedo` (through `decide` or the subtype field), the same undo step sets each other member of that set to its PBR counterpart:
  - `normal` / `msn` → `pbr_normal`;
  - `glow` → `pbr_emissive`;
  - every other sub-type, including `height`, is unchanged.
- **Only inferred members switch.** A member switches only if its current sub-type is *inferred*, meaning its own `[files]` entry has no `subtype` key, or the member is still undecided. A member whose sub-type the user set is left alone.
- **Marking auto-switched members.** Record which members were auto-switched, so revert can find them. Use one of these and say which in the report:
  - a GUI-only marker in `sdc.toml`, `[files."x"] subtype_auto = true`, if the engine ignores unknown keys; check `CfgInvalid` behaviour first;
  - or a list in `sdc-project.toml`.
- **Diffuse goes back to vanilla.** When the diffuse changes from `pbr_albedo` to a non-PBR sub-type, the auto-switched members revert: remove their `subtype` key, so they return to inferred.
- **One undo step.** Undo restores everything in a single step.
- **P8:** the member switch writes `sdc.toml` only, so it works while `sdc-project.toml` is read-only.
- **Dropdown order.** Every sub-type dropdown has two parts:
  - The PBR sub-types (`Subtype::*` whose id starts with `pbr_`, plus `rmaos`, `rfaos` and `coat_normal`) come first, under a non-selectable heading `PBR (Community Shaders)`.
  - The remaining types follow, grouped by `Category` with a heading per group.
  - Use the same order in both places that list sub-types: `settings_panel.rs` `options_of("subtype")` (~line 62) and the confirm-subtype combo (~line 449).
  - Keep the existing availability filtering.

**Files:**
- `crates/gui/src/model/mod.rs` (or `decide`'s module) for the propagation;
- `crates/gui/src/ui/settings_panel.rs` for the grouping;
- a tests file: `crates/gui/tests/` (new `pbr_members.rs`, or append to an existing model test).

**Tests:**
- (a) Deciding rock01 as `pbr_albedo` sets its `_n` to `pbr_normal` and its `_g` to `pbr_emissive`. `_p` stays `height`.
- (b) A member with a user-set sub-type is unchanged.
- (c) Reverting the diffuse to `diffuse` reverts only the auto-switched members.
- (d) One undo restores all of them.
- (e) A kittest checks the subtype combo lists the `PBR (Community Shaders)` heading before the first vanilla type.

Commit: `feat(gui): PBR diffuse switches inferred set members to PBR types; PBR types grouped first in the dropdown (owner)`.

### Task 3: Export resolves undecided textures in the dialog

**Owner:** "Warnings need a quick way to resolve all on export, that popup should bring up undecided textures with a chance to resolve them."

**Rules:**
- `Dialog::ProceedAnyway(usize)` becomes a list, either `Dialog::ProceedAnyway(Vec<String>)` (item keys) or a separate struct. The dialog shows:
  - **One row per undecided texture:**
    - its file name, which is clickable and selects it;
    - a sub-type dropdown preselected with the engine's suggestion, if any (use what `confirm-subtype` uses), and grouped as in Task 2;
    - a **✓** button that decides that row (`Answer::Subtype`).
  - **Accept all suggestions:** decides every row that has a suggestion. Rows without one stay listed. Disable the button when no row has a suggestion.
  - **Proceed anyway:** behaviour unchanged; it skips what is still undecided.
  - **Cancel.**
- **After deciding:**
  - When the last row is decided, the dialog continues to the export by itself, the same path as when there were no undecided textures. The plan must settle first: reuse the existing wait pattern, and do not export on a stale plan. The unsettled-plan guard exists.
  - Each decision is its own undo step. Accept-all is one undo step.
- **Long lists:** the list scrolls past about 12 rows.

**Files:** `crates/gui/src/model/mod.rs` (dialog data, `accept_all_suggestions()`), `crates/gui/src/ui/dialogs.rs`, and tests.

**Tests:**
- (a) Export with 2 undecided textures, both with suggestions → the dialog lists both. `accept_all_suggestions` decides both, and the export proceeds.
- (b) An item with no suggestion stays listed, and Accept all leaves it.
- (c) Proceed anyway still works.
- (d) One undo reverts an Accept all.

Commit: `feat(gui): export lists undecided textures with per-row decide and Accept all suggestions (owner)`.

### Task 4: UI affordances

**Owner:**
- "the first slider could use some markers to let you know where stops are, it's too smooth"
- "the 2d preview Is not intuitive to close"
- "Dragging the pills works but it's not obvious to know your holding one or that it was released or that it was reordered (maybe add a border)"
- "The warnings on the texture sets should have a small window popup with the issue on scroll over"
- "Export button should be bigger"

**Rules:**
1. **Mip slider** (`crates/gui/src/ui/preview.rs` ~line 227, `egui::Slider::new(&mut mip, first..=end-1)`):
   - Make it discrete: integer steps, `step_by(1.0)`.
   - Paint a small tick under the track for each mip, using the slider response rect, and highlight the current mip's tick.
   - Keep the `mip N (W×H)` label.
2. **Preview close:**
   - Add a `✕ Close` button at the right end of the preview toolbar. It calls the same collapse as Esc.
   - Its hover text is `Close (Esc)`.
3. **Pill drag feedback** (`crates/gui/src/ui/sets_column.rs`):
   - While a DragSet is in flight, the dragged pill gets a 2 px accent border and reduced opacity.
   - An accent insertion line is drawn at the gap where the pill would land, using the same target logic as `reorder_target`.
   - After a successful reorder, the moved pill's border flashes the accent colour for about 1 s. Store the reorder time in UI state, and request repaint until the flash ends.
   - Do not reintroduce a consuming DragItem zone while a DragSet is in flight (Review Focus 4).
4. **Warning badge hover:** hovering a pill's `⚠ n` badge shows a tooltip. It lists each warning as `<file>: <code> <message>`, at most 10 lines, then `… and K more`. Use the same data that builds the count.
5. **Export button** (`crates/gui/src/ui/header.rs` ~line 66):
   - Make it larger: `min_size` about 120×30, strong text, filled with the accent colour when enabled.
   - Keep the disabled state and its hover text.

**Files:** `ui/preview.rs`, `ui/sets_column.rs`, `ui/header.rs`, and kittest tests in `crates/gui/tests/set_editor_ui.rs` or `ui_smoke.rs`.

**Tests** (kittest, where observable):
- (a) The `✕ Close` button exists and clicking it collapses the preview.
- (b) The Export button's rect is at least 110 px wide.
- (c) A warning badge hover shows the warning text. Tooltips aren't accesskit tooltips here; follow Task 15's hover-test pattern, with `tooltip_delay = 0`.
- (d) The mip slider value is an integer after a drag, if kittest can drag. Otherwise unit-test a pure tick-position helper.

Note that drag feedback can't be tested by kittest. Describe it for the owner's hand check.

Commit: `feat(gui): mip slider stops, preview Close button, pill drag feedback, warning hover, bigger Export (owner)`.

### Task 5: Mipmap and coverage fields in the right place

**Owner (2026-10-11):** "mipmaps do we need a full chain or simply a yes / no, full chain in advanced maybe. Coverage should be under alpha settings no? mipmap filter should be in basic not advanced"

**Rules** (`crates/gui/src/model/fields.rs` `FIELDS`/`ADVANCED_FIELDS`/`group_of`, `crates/gui/src/ui/settings_panel.rs`, `crates/gui/src/ui/advanced.rs`):
1. **Mipmaps on/off (basic):** a checkbox labelled "Mipmaps".
   - Ticked writes `mips.levels = "full"`. Unticked writes `"none"`.
   - When the resolved value is a number N, the box shows ticked with the note "(N levels — see Advanced)".
   - Clicking it writes `full` or `none`.
2. **Level count (Advanced drawer):** the existing "Levels" dropdown (Full chain / None / 2…N levels) moves into the Advanced drawer, in a "Mipmaps" section.
3. **Coverage (Alpha group):** `mips.coverage` moves into the Alpha group, directly under Threshold/Comparison. It shows only for the `cutout` and `cutout_blend` policies, the same visibility rule as Threshold.
   - Add a `group_of` override for the key `mips.coverage`, so it reports Alpha, not Mipmaps.
4. **Mip filter (basic):** the engine's `MipFilter` has only `box` in v1 (`config.rs:27`). Show a disabled "Filter: Box" row in the Mipmaps group, with the hover text "More filters (Kaiser, Mitchell, Lanczos) arrive with M6". It becomes a live dropdown when M6 adds variants. M6 carries that item.
5. **Normal handling** stays in the Mipmaps group (normal maps only, as today).
6. **Nothing else moves.** The saved TOML keys don't change; this is only where fields are shown.
   - The `differs_from_base` / "only differences" logic in Advanced must still work for the moved Levels row.

**Tests (kittest + model):**
- (a) Unticking "Mipmaps" writes `mips.levels = "none"`, and ticking writes `"full"`.
- (b) "Levels" appears in the Advanced drawer and not in the main panel.
- (c) "Coverage" appears under the Alpha heading only for a cutout policy.
- (d) The "Filter: Box" row is present and disabled.

Commit: `feat(gui): Mipmaps on/off in basic, level count in Advanced, coverage under Alpha, filter row (owner)`.
