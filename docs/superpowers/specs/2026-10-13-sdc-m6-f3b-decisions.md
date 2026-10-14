# F3b design decisions (controller, 2026-10-13)

The owner said "continue work on f3b". No F3b plan existed, so each design question is decided here from the owner's scope, the owner's card answer below, the research doc (docs/research/bulk-editing.md, sections 3 and 4) and the code at `main` ddae78d. These decisions are the input to the spec. Anything the owner later disagrees with goes in DECISIONS as a correction.

Owner's scope:
- F3 split (owner, 2026-10-13): "F3b is input fixing (edit path, input rename), output rename and auto PGPatcher rename."
- Input rename (owner, 2026-10-13, decision card): **alias in sdc.toml**. Source files are never touched. The file on disk keeps its name; sdc reads it as if it had the corrected one.

What the code does today (facts the decisions rest on):
- An output mirrors its source: `discover::make` sets `output = <output_root>/<rel>.dds` (crates/engine/src/discover.rs:72), and `canonicalise_outputs` only swaps the suffix to the Skyrim one.
- Grouping and kind come from the file name (`group::split_suffix_with`, `group_sets_with`). The only remapping is a preset's global `suffix_aliases`, set edits (Move, Remove) and a `[files]` sub-type.
- A set's id never names an output. `SetEdit::Rename` changes no output file name.
- The only output-name control is the PGPatcher `rename` key on a CS PBR set (DECISIONS V3). It replaces the last component of the `textures\pbr\…` prefix, and the JSON carries the full prefix.
- The GUI sends explicit `output_paths` only for CS PBR sets (`engine_inputs`, crates/gui/src/model/sets.rs:599). The engine applies any `output_paths` it is given (`sets::apply_output_paths`).

## D1 Input alias (edit path and input rename are one thing)
- A source file can carry an **alias**: the path, relative to the folder holding `sdc.toml`, that sdc reads it as. A new name is an input rename; a new folder is a path edit; both at once is allowed.
- Stored in `sdc.toml` in a new table, so the CLI honours it too:
  ```toml
  [inputs."textures/armor/wall.png"]
  as = "textures/armor/wall_n.png"
  ```
  The key is the real file, matched like a `[files]` key (case-insensitive, with or without the extension).
- The alias replaces the file's `rel` at discovery, before canonicalising and grouping. Grouping, kind, suggestions, rule globs, `game_path` and the output path all see the alias. The decoder still reads the real `path`.
- **Identity stays the real file.** The file's `[files]` key, its `add`/`remove` entries and borrows in `sdc-project.toml`, and its GUI item key stay on the real path. So adding, changing or clearing an alias never orphans a setting and needs no re-keying. (`overrides::project_rel` builds keys from the folder on disk plus `rel`'s file name today; it takes the real name for an aliased file.)
- In code the feature is called "read as" (`InputFile::real_rel`, the config key `as`), because "alias" already names a preset's `suffix_aliases` (`FileFacts::alias`).
- The extension is the real file's. A typed alias extension equal to it is dropped; a different one blocks ("an alias keeps the file's own extension").
- An alias must stay under the project folder (no absolute path, no `..`), use `/` or `\` (stored as `/`), and must not equal another file's real or aliased path (case-insensitive). Breaking any of these is reported (`IN-ALIAS-INVALID`), with the reason (the spec makes it a batch Warning and ignores the alias, §9).
- An alias whose real file is gone is reported, not fatal (`IN-ALIAS-STALE`, Info, like `SET-EDIT-STALE`). The GUI shows it with a Remove button.
- An aliased file is marked in the GUI ("read as wall_n.png") so it never looks like the file was renamed on disk.

## D2 Output rename
- Renames a set's outputs: the user gives a new base name, each member keeps its suffix (`<new>_n.dds`, `<new>_rmaos.dds`), and the folder is kept. Folder edits of outputs are out of scope (D7).
- Plan-only. Nothing on disk is renamed. Outputs already written under the old names stay; the existing `OUT-UNKNOWN-EXISTING` and manifest rules apply to them as today.
- **On a CS PBR set it writes the PGPatcher `rename` key** (D3), because the prefix already drives that set's outputs and JSON.
- **On every other set** it writes `output_name = "<new base>"` in the set's `[[sets]]` table in `sdc-project.toml`, and `engine_inputs` sends explicit `output_paths` for that set's members on every target, not only CS. GUI-only, like every other set edit (the CLI reads no `sdc-project.toml`).
- A non-PBR output rename usually breaks the meshes that point at the old texture path. When the scan database knows a mesh or plugin that references a renamed member's old game path, the rename row warns "N meshes still point at <old path>". It does not block.
- Clearing the name restores the mirrored output (removes `output_name`, or the `rename` key).

## D3 Auto PGPatcher rename
- On a CS PBR set, an output rename writes the typed base name into the set's PGPatcher `rename` key, under the existing V3 rules (file name only, one `.dds` cut). The outputs then land on `textures\pbr\<folder>\<new>…` and the JSON's `rename` carries that full prefix, as V3 already does.
- An input alias on a PBR set's diffuse changes the derived match (it comes from the source's path). When that happens and the set has no `rename` of its own, the old output prefix would move with it. Decision: the alias moves the prefix too (the alias says "this is the file's real name"), and the preview shows the new prefix in its Output column so the move is visible before Apply.
- The existing "matches disagree and no rename" warning stays as it is.
- The open V3 point (does real PGPatcher accept the leading `\` it adds to `rename`?) is still unverified. F3b makes more sets use `rename`, so the spec repeats it as an owner check.

## D4 The rename dialog
- One dialog, opened from the right-click menu, with a mode at the top: **Inputs** (writes aliases, D1) or **Outputs** (writes output names, D2 and D3).
- Scope is the base name only (the part before the suffix sdc detected), so suffixes are never touched by accident. In Inputs mode there is also a per-row text cell to type a whole new name or path for one file (that is how a missing `_n` gets added).
- Operations, applied in this order: find and replace (literal by default, a Regex checkbox), prefix, suffix, case (keep, lower, upper). No counters or tokens (D7).
- Regex uses the `regex` crate (linear time, no look-around). `$1` and `${name}` captures. An invalid pattern shows its message in place and disables Apply; the dialog shows "N of M matched".
- Live preview table: Old | New | Kind (Inputs mode: old kind → new kind, so a newly detected `_n` is visible) | Output. Each row has a checkbox to exclude it. Changed rows are highlighted.
- Checks, run on the whole result before Apply:
  - Blocks: two rows, or a row and an unselected file or set, end on the same input or output path (case-insensitive; the engine would report `IN-DUPLICATE` anyway); an illegal Windows name (`<>:"|?*`, control chars, reserved names such as `CON`, a trailing dot or space); an output path over 260 characters; an empty name.
  - Warns: in Outputs mode, a mesh still pointing at the old path (D2); in Inputs mode, a row whose detected kind or set changes (it is often the point of the fix, so it never blocks).
- The preview and the commit share one pure function, `plan_rename(model, rows, recipe) -> Vec<RenameRow>`, testable without egui.
- Enter applies when valid, Esc cancels. The dialog keeps its recipe for the next open in this session (no saved presets, D7).

## D5 Menu entries (appended to the F3a lists)
- Texture tiles: "Rename input…" (F2), "Rename output…", and "Clear input alias" when the selection has one.
- Set pills: "Rename outputs…" (F2) and "Rename inputs…" (all members of the selected sets).
- One texture or set selected uses the same dialog with one row.
- F3a's `MenuAction` gets the new variants; `run_menu` opens the dialog.

## D6 Undo and re-plan
- Each Apply is one undo step and one re-plan, through F3a's `batch_edit`.
- An alias does not move a file's identity (D1), so nothing is re-keyed.
- A read-only `sdc.toml` makes Inputs mode a no-op with the existing notice; a read-only `sdc-project.toml` does the same for Outputs mode on non-PBR sets.
- Slot locks (F3a Task 8) are stored by slot number (`lock = ["slot2"]`), so they need no re-keying. An alias can change a file's kind or set, which would pull it out of a locked slot, so Inputs mode skips a file that fills a locked slot and the preview shows "skipped (locked)". Outputs mode changes no membership and ignores locks.

## D7 Out of scope (YAGNI, say so in the spec)
- Renaming or moving files on disk (owner chose alias).
- Output folder edits (fixed folder, subfolder, make relative). The owner scoped output *rename*.
- Counters, tokens (`{name}`, `{counter:3}`), trim, Title case.
- Saved rename recipes and "recent" lists.
- A CLI command to write aliases (the CLI reads them; editing is GUI or by hand).
- Aliases in presets or user configs (project `sdc.toml` only, like `[files]`).

## D8 Testing
- Engine: alias applied at discovery (rel, game_path, output, grouping, kind); key matching; invalid and stale aliases; the CLI path; a golden case only if a new one is added (existing goldens unchanged).
- GUI model tests (the existing style):
  - `plan_rename`: each operation, regex captures, invalid pattern, excluded rows, every block and warning;
  - output rename on a non-PBR set (`output_name` → `output_paths` on all targets) and on a CS PBR set (`rename` key);
  - one undo step and one re-plan for N=300;
  - `[files]` settings, members and borrows survive adding and clearing an alias; the locked-slot skip; undo.
