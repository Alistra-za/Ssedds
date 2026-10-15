# GUI suggestions, Tessalith rename and colour pickers: implementation plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development. Each task is one implementer dispatch plus a task review. Steps are TDD: failing test, RED output, fix, GREEN, commit.

**Goal:** Ship every item of `docs/superpowers/specs/2026-10-14-sdc-gui-suggestions.md` (items 1-24), rename the product to Tessalith (the CLI stays `sdc`), and give every colour input a hex field plus a colour-square picker.

**Architecture:** GUI-only changes in `crates/gui`, plus the `[[bin]]` names in `crates/cli` and `crates/gui`. No engine behaviour change, and the engine version stays 0.7.0.

**Tech stack:** Rust, eframe/egui 0.36.2, rfd, kittest for UI tests.

**Spec:** `docs/superpowers/specs/2026-10-14-sdc-gui-suggestions.md` (the "suggestions doc"). Each item there is the spec for its task. The owner's additions (2026-10-14) are the rename and the colour pickers. Owner instruction: "go ahead with 2026-10-14-sdc-gui-suggestions".

Branch: `feat/gui-suggestions`, cut from `fix/audit-2026-10-13b` once its review passes. Worktree: `.worktrees/gui`.

## Global constraints
- Owner rulings that must not be contradicted. Each brief pastes them:
  - **N1:** Live encode stays off by default.
  - **N3:** Proceed anyway / Cancel never decides for the user.
  - **P19:** rename stays a dialog, never inline.
  - **Z8:** no bulk recipes, presets, regex, hidden modifier keys, or bulk editing of Advanced or set-level settings.
  - **P13/P26:** never auto-fix invalid paths.
  - **AB4:** the 2D | 3D default stays 2D.
- Out of scope: the performance items #7-#9 in `docs/AUDIT-2026-10-14.md`.
- Gates before every commit (unpiped, `--offline`):
  - `cargo fmt --all --check`
  - `cargo clippy --workspace --all-targets -- -D warnings`
  - `cargo clippy --workspace -- -D warnings`
  - `cargo test --workspace`
- Golden tests unchanged. No new crates, except `windows` features that are already in the lock if item 6 uses them.
- Update `docs/gui.md` for every user-visible change.
- DECISIONS: one new note, **Note AG — GUI suggestions, Tessalith (2026-10-14)**. AE and AF are taken by the audit round; AD is reserved by the PBR landscape plan. Every ruling below becomes an AG item.
- Commits: explicit paths, with the trailer `Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>`.
- Never run `python -` heredocs or bare `git stash`. Owner data and G:/ are read-only. Tests never touch the real `%LOCALAPPDATA%\sdc`.

## Controller rulings (become Note AG items)
- **AG1 Rename scope:** rename what the user sees, keep what is on disk.
  - Renamed:
    - the GUI binary `tessalith.exe` (`[[bin]] name = "tessalith"`);
    - the CLI binary stays `sdc.exe` (owner, 2026-10-14);
    - the window title "Tessalith";
    - the About, help and `--help` text and clap's `name`;
    - docs titles and product mentions;
    - README;
    - the GUI's "Copy CLI command" output (stays `sdc …`);
    - log and crash-report headers.
  - Kept: `sdc.toml`, `sdc-project.toml`, `%LOCALAPPDATA%\sdc\` (scan.db, gui.toml, logs), the `SDC_*` env vars, package and lib names, the `_sdc/` folder, and code IDs.
  - Why: renaming those orphans every existing project and the scan DB. Doing it properly is a migration item, which needs the owner's word.
- **AG2 Colour inputs:** every colour input gets two things.
  - A swatch that opens egui's colour-square popup (`egui::color_picker::color_picker_color32`, Alpha::Opaque).
  - A hex text field: `#RRGGBB`, `#` optional, case-insensitive. An invalid string turns the field red and does not commit; the value commits on Enter or focus loss.
  - Stored values keep their current type and range. TruePBR fields stay integers 0-255 with the existing clamping and reset, so the JSON is unchanged.
  - This overrides item 16's "no hex entry (Note P)" on the owner's word of 2026-10-14.
- **AG3 Item 3 persistence:** save in `gui.toml` via `settings.rs`, not eframe `persistence`. One settings file the user can read and delete. Column widths move out of egui temp memory into gui.toml.
- **AG4 Item 8 sort:** Name (default), Source size, Status. Status order: errors, warnings, undecided, ok. Ties break by name. Saved in gui.toml.
- **AG5 Item 2 Ctrl+Z in Meshes:** enable it unless `app.rs:169` documents a reason. If it does, keep it off and quote the reason in the report.
- **AG6 Item 6 taskbar progress:** skip it unless `windows` crate `ITaskbarList3` is already in the lock with the right features. Say which.
- **AG7 Item 13 streaming thumbnails:** only if the plan worker can send per-file results without reordering the grid. Otherwise do the progress bar only and record why.
- **AG8 Item 10 pin:** one pin, held in memory. Dropped on Close, on a texture change and on folder open.

## Tasks
Each task's brief is: the suggestions-doc items listed, verbatim, plus the rulings above and the global constraints. Tests go in `crates/gui/tests/` in the style of `ui_smoke.rs` / `set_editor_ui.rs`.

| Task | Content | Model |
|---|---|---|
| 0 | Rename (AG1): one mechanical commit, no logic. Grep `sdc`, `sdc-gui` and "sdc " in user strings, docs, README and the CLI. Also the window title, and scripts that call `sdc-gui.exe` (update the paths). The CLI binary, clap `name` and Copy CLI command stay `sdc`; CLI help text may say "sdc, the Tessalith command line". Report every kept `sdc` category. | sonnet |
| 1 | Colour inputs (AG2). Enumerate every colour input in the model and UI: TruePBR fields (pgp_fields.rs:436), 3D Look background / sun / ambient, RNM, any tint/emissive/specular. List each site in the report. One shared widget `ui::color::color_field`. Tests: hex parse (valid, `#`-less, invalid stays uncommitted), u8 round trip, clamping unchanged. | sonnet |
| 2 | Items 2, 4, 23, 24: undo buttons and labels, dialog Esc/Enter and the Delete set confirm, rename menu hover text, hover text on unlabelled controls. | sonnet |
| 3 | Item 3: persistence (AG3), with off-screen clamping. | sonnet |
| 4 | Items 5 + 19: keyboard navigation, the F1/? shortcut sheet, the Ctrl+K command palette, all from ONE action list (palette, sheet and menus share it). | opus |
| 5 | Items 6 + 14: open export folder, Show in Explorer, rate and ETA, virtualised Files list, filters, warning text (AG6). | sonnet |
| 6 | Item 7: async file pickers with no re-entry. | sonnet |
| 7 | Item 8: filters, sort (AG4), tile size, empty-filter message, hover text. | sonnet |
| 8 | Items 1 + 15: pixel readout and magnifier, exposure, 3×3 tiling, heatmap legend with full scale, heatmap off the UI thread. | opus |
| 9 | Item 10: pin and compare (AG8). | sonnet |
| 10 | Item 12: 3D pan, light rotation, debug views, magenta missing slots, Copy image. Note render3d is NIF-owned: debug views via existing render3d API only. If a shader change is needed, stop and report. | opus |
| 11 | Item 13: plan progress, cancel, streaming thumbnails (AG7), scan row bar and ETA. | opus |
| 12 | Items 9 + 17 + 18: Used by meshes, Open in Textures, drop files onto a slot, Fill from names (reuse the engine suffix table). | opus |
| 13 | Item 11: JSON editor errors, Next error, search, cached text. | sonnet |
| 14 | Items 20 + 21 + 22: toasts plus the Messages log, the RNM blended thumbnail, output size and VRAM estimate. | sonnet |
| 15 | Docs: Note AG (AG1-AG8 plus each task's rulings) and the gui.md sweep. Then the final Opus whole-branch review. | sonnet / opus |

## Review focus
- Text-field focus: no new shortcut fires while a TextEdit has focus (items 4, 5, 19).
- Off-screen and multi-monitor window restore (item 3).
- Huge folders (10k textures): the virtualised lists, the filters and the palette stay responsive.
- Async picker and planner results arriving after the folder changed are dropped, keyed by generation.
- Colour hex entry typed mid-edit never commits a partial value.
