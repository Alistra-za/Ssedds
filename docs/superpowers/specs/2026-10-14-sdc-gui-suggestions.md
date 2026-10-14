# sdc GUI suggestions: implementation prompts

Source: a read-only review of `crates/gui` on main (2026-10-14, after PR #5 merged), plus research on features users praised in similar tools. The full write-up with sources is the "sdc GUI feature review" artifact.

Each item below is a self-contained prompt. Paste the **Shared context** block first, then one item.

---

## Shared context (paste with every item)

> Repo: https://github.com/Alistra-za/Ssedds. `sdc` is a Windows-only Rust Skyrim SE/AE DDS texture converter. The GUI is `crates/gui` (eframe/egui 0.36.2, wgpu renderer). UI code lives in `crates/gui/src/ui/*.rs`, state in `crates/gui/src/model/*`, app loop in `crates/gui/src/app.rs`. The user guide is `docs/gui.md`; update it for any user-visible change. Owner rulings are in `docs/DECISIONS.md` and `docs/RULINGS.md` and must not be contradicted.
>
> Do not do any of these (owner rulings):
> - Turn Live encode on by default (N1).
> - Make Proceed anyway / Cancel decide for the user (N3).
> - Inline rename; rename stays a dialog (P19).
> - Bulk recipes, presets, regex, hidden modifier keys, or bulk editing of Advanced or set-level settings (Z8).
> - Auto-fix invalid paths (P13, P26).
> - Change the 2D | 3D switch default (AB4).
>
> The performance items in `docs/AUDIT-2026-10-14.md` (#7 re-plan stalls, #8 settings panel clone per frame, #9 f32 decodes) are tracked separately; don't fold them in.
>
> Add tests in `crates/gui/tests/` in the style of the existing ones (`ui_smoke.rs`, `set_editor_ui.rs`). Keep the change to what the item asks.

---

## Tier 1: quick wins

### 1. Pixel readout and magnifier lens in the 2D preview
**Value high, effort S–M.**

In the expanded 2D preview (`ui/preview.rs`), show a readout of the pixel under the cursor:
- source and encoded RGBA, both as 0–255 and as float,
- for normal maps, the decoded normal vector and its length,
- the per-pixel error.

It should honour the current channel, mip and sRGB/UNORM settings.

Add a toggle for a magnifier circle that follows the cursor at 8x with nearest-neighbour sampling, in both viewports of the side-by-side and split layouts.

Today `hover_pos` is used only for zoom (`ui/preview.rs:410`). The existing readouts are in `model/view.rs:174-204`.

*Why:* Upscayl 2.15's Lens Viewer drew reactions from 625 people, the strongest praise found in the research. Tacent View lists a per-pixel readout as a core feature.

### 2. Make undo visible
**Value high, effort S.**

- Add Undo and Redo buttons to the header (`ui/header.rs`). Disable them using `can_undo`/`can_redo` (`model/mod.rs:1512`, currently unused by the UI).
- Put the history step's label in the hover text, for example "Undo: set sub-type on 12 textures".
- After an undo or redo, show "Undid: <label>" or "Redid: <label>" in the status line.
- Allow Ctrl+Z and Ctrl+Y in the Meshes view. They are disabled there at `app.rs:169`; enable them unless there is a documented reason, and if there is, say so in the PR.

### 3. Remember window size, position and layout
**Value high, effort S.**

The window always opens at 1400×900 (`main.rs:55-57`). Column widths are kept with `insert_persisted` (`ui/mod.rs:233-251`), but they are lost on restart because eframe's `persistence` feature is off (`crates/gui/Cargo.toml:26`).

Persist these, either by enabling the feature or by saving them in `gui.toml` through `settings.rs` (pick one and say why):
- window size and position,
- column widths,
- the Textures/Meshes view (`ui/header.rs:149` uses `insert_temp`),
- the Textures filter toggles,
- the preview channel and layout,
- the 3D Look (`model/viewport3d.rs:332` says it is not saved).

If a saved window position is off-screen, clamp it back on-screen.

### 4. Consistent dialog keys and a confirm on Delete set
**Value high, effort S.**

The shared `modal()` helper (`ui/dialogs.rs:211-216`) never checks `should_close()`, so Esc and click-outside do nothing on most dialogs. Make Esc mean that dialog's Cancel, and make Enter trigger its safe default button. The external-change dialog must not default to Overwrite.

Match the behaviour already in `rename_dialog.rs:129-131` and `slot_commands.rs:155`.

Delete set (`ui/sets_column.rs:633`) currently runs with no confirmation, unlike Reset all (`dialogs.rs:276-290`). Add a confirm dialog that names the set and its member count.

### 5. Keyboard navigation and a shortcut sheet
**Value high, effort S–M.**

Textures grid (`ui/columns.rs`):
- Arrow keys move the selection, in the displayed order.
- Enter or Space opens the expanded preview (today only a double-click does, `columns.rs:213`).

Expanded preview (`ui/preview.rs:155-161`, where Esc is the only key today):
- `[` and `]` go to the previous and next texture, in the filtered order.
- 1–6 pick the channel buttons.
- PageUp and PageDown step through the mips.

Don't fire any of these while a text field has focus.

Add a shortcut sheet opened by F1 (and `?` when no text field has focus) that lists every shortcut, including the existing ones in `ui/mod.rs:271-286`.

### 6. Open the export folder; better export progress
**Value high, effort S.**

In the export drawer (`ui/drawer.rs:66-97`):
- Add "Open export folder" (`explorer.exe`).
- Add elapsed time, files per second and an ETA to the progress line.

Add "Show in Explorer" (`explorer /select,`) to the texture tile context menu and to the drawer's Files list.

Optionally, show Windows taskbar progress if eframe or the windows crate makes it cheap; skip it otherwise and say so.

### 7. Stop file pickers freezing the window
**Value medium, effort S.**

Replace the blocking `rfd::FileDialog` calls with `rfd::AsyncFileDialog` driven off the UI thread, with the result handed back through the model. The calls are at:
- `ui/header.rs:39-41`,
- `ui/meshes.rs:55` and `:63`,
- `ui/viewport3d.rs:18-22`,
- the NVTT chooser,
- Save report.

While a picker is open, block re-entry (no second picker), but keep repainting.

### 8. Better Textures column filtering
**Value medium, effort S–M.**

In `ui/columns.rs` and `model/mod.rs:1395-1411`:
- Add filters for sub-type, output format, encoder and set membership, next to the existing four toggles.
- Add a sort order: name, source size, or status (needs decision first).
- Add a tile size slider. `TILE = 96.0` is fixed at `columns.rs:14`.
- When the filters hide everything, replace "No textures to show." (`:265`) with a message that a filter is active and a "Clear filters" button.
- Give each filter toggle hover text.

Bulk selection must keep the existing "hidden by filter" semantics.

---

## Tier 2: high value, more work

### 9. Link textures, meshes and mods both ways
**Value high, effort M.**

In the texture settings panel (`ui/settings_panel.rs`), when a scan database exists, add a collapsible "Used by meshes" list. Each row shows the mesh path, its mod and its MO2 priority, and the list marks which mesh decided the alpha meaning. Sort by MO2 priority (winner first).

In the Meshes view detail pane (`ui/meshes.rs:429-512`), add "Open in Textures" on a texture path that is in the open folder.

*Why:* PGPatcher's most-upvoted issue (#93) asked for sorting by MO2 order. #197 and #674 asked to see which meshes are involved.

### 10. Pin an encode and compare against the next one
**Value high, effort M.**

In the 2D preview, add "Pin this result". It keeps the current encoded image and the settings that made it.

While a result is pinned, the side-by-side and split layouts can compare the pinned result against the current encode, instead of source against encoded. Show each side's settings summary and PSNR.

Unpin on Close or when the texture changes. Keep at most one pin, to bound memory.

*Why:* this answers "is NVTT or a higher effort worth it on this texture". NVIDIA Texture Tools Exporter's preview-before-write and Upscayl's comparison slider are both strongly praised.

### 11. Show JSON errors inside the PGPatcher JSON editor
**Value high, effort S–M.**

Schema validation already runs each frame (`model/pbr_export.rs:345`, `export_blocks`). Its results currently show only as badges in the Sets column (`sets_column.rs:133`).

In `ui/json_editor.rs`:
- Show each error on the entry it belongs to.
- Show an error count in the header, with a "Next error" button that scrolls to it.
- Add a search box that filters entries by set name or path.

Don't regenerate the JSON text every frame while the text section is open (`:203`). Cache it, keyed on a model revision counter.

### 12. 3D preview: pan, rotate lighting, solo a map
**Value high, effort M.**

In `model/viewport3d.rs:318-327` and `ui/viewport3d.rs:88-150`:
- Add pan with middle-drag and with Shift+left-drag.
- Rotate the environment and sun with Shift+right-drag.
- Add a debug view selector with Lit (default), Albedo, Normals (world), Roughness, Metallic, AO and a UV checker.
- Render an unresolved or missing texture slot in solid magenta instead of a fallback.
- Add a "Copy image" button that puts the current frame on the clipboard.

*Why:* Material Maker 1.5 added Shift+right-drag environment rotation. Node Wrangler's "preview this node" is one of Blender's best-loved shortcuts. NifSkope's magenta missing-texture colour is how modders spot broken paths.

### 13. Real progress while a folder opens
**Value medium, effort M.**

Planning shows only "Analysing N files…" (`model/mod.rs:667`). Thumbnails arrive only when the whole plan lands (`model/items.rs:221`).

- Report done/total from the plan worker and show a progress bar with Cancel. Planning is already cancellable internally.
- Stream thumbnails into the grid as each file finishes, if the worker can send partial results without reordering issues. If it can't, explain why in the PR and do the bar only.
- Give the mod scan row (`ui/meshes.rs:41-88`) a bar and an ETA as well.

### 14. Export drawer that scales to big folders
**Value medium, effort S–M.**

In `ui/drawer.rs:154-186`:
- Virtualise the Files list with `show_rows`.
- Add "Failed only" and "Has warnings" filters.
- In the Warnings tab, show each warning's message text, not just its code and severity.

### 15. 2D preview: exposure, tiling and a heatmap legend
**Value medium, effort S–M.**

- Add an exposure slider for HDR textures (BC6H, RGBA16F), applied before the existing Reinhard tonemap.
- Add a 3×3 tiling toggle to check seams.
- Add a colour legend for the error heatmap, with an adjustable full-scale value. It is hard-coded to 0.1 at `model/view.rs:135`.
- Move the heatmap computation (`app.rs:131` → `model/view.rs:124-143`) off the UI thread, and show the previous heatmap until the new one is ready.

### 16. Colour swatch on TruePBR colour fields
**Value medium, effort S.**

In `ui/pgp_fields.rs:436-442`, draw a colour swatch next to the three 0–255 DragValues, and open egui's colour picker on click. Values must stay integers 0–255 and keep the existing clamping and reset behaviour. No hex entry (Note P).

### 17. Drop files from Explorer onto a set slot
**Value medium, effort M.**

Today a dropped file opens its folder (`app.rs:191-201`). When the drop lands on a slot card in the set editor and the file is inside the open folder, fill that slot instead, as one undo step.

Files outside the open folder keep today's behaviour, or show an error line explaining why. Don't copy files.

### 18. Fill a set from its folder by suffix
**Value medium, effort M.**

On a set, add "Fill from names…". It finds files in the set's folder that share the set's base name with a known suffix: `_n`, `_rmaos`, `_rfaos`, `_p`, `_cnr`, `_s`, and the suffixes the engine already recognises (reuse the engine's suffix table; don't duplicate it).

Show the proposed slot assignments in a confirm dialog with a tick per row, then apply them as one undo step.

---

## Tier 3: larger polish

### 19. Command palette
**Value medium, effort M.**

Ctrl+K opens a searchable list of actions (Open folder, Save, Export, Rename outputs, toggle views, open the JSON editor, …) and of textures and sets by name. Enter runs the action or selects the item. Arrow keys move, Esc closes.

### 20. Non-blocking notices with a log
**Value medium, effort M.**

Add short toasts for routine results (saved; N written, M skipped; scan finished) that fade out after a few seconds, and a "Messages" log panel to read them back.

Keep modal dialogs only for things that need a decision. Don't change which events are errors.

### 21. Preview of the blended RNM result
**Value medium, effort M.**

In `ui/rnm_section.rs`, show a small thumbnail of the blended normal map, computed off the UI thread at thumbnail resolution. Refresh it when a strength slider is released (sliders already commit on release) and when the layer order changes.

### 22. Output size and VRAM estimate
**Value medium, effort S.**

In the settings panel, show the expected DDS size for the selected texture: format, dimensions after any resize, and the full mip chain. In the bulk panel and the header, show the total for the selection and for the folder.

### 23. Tidy the three Rename menu items
**Value medium, effort S.**

In `ui/sets_column.rs:577-645`, "Rename…", "Rename outputs…" and "Rename inputs…" sit together. Either give each a one-line hover text (the set name only; the output files; the source files) or group them under a "Rename" submenu. Keep the F2 hint on Rename outputs.

### 24. Hover text on unlabelled controls
**Value medium, effort S.**

Add hover text to:
- the Mode combo, explaining cs, vanilla and cpm (`ui/header.rs:121-136`),
- the Textures filter toggles,
- the preview's channel, Fit, 1:1, Split and Live encode buttons (`ui/preview.rs:19-26`, `:202-230`).

Use the wording from `docs/gui.md` where it exists.

---

## Already strong (don't redo)

- Per-field provenance, ↺ reset, lock reasons, and "(auto)" with a why tooltip.
- In the 2D preview:
  - channel isolation, mip slider and sRGB/UNORM toggle,
  - side-by-side and split compare,
  - heatmap, PSNR and normal-length readouts.
- 200-step labelled undo. External-change detection on `sdc.toml`. Source files are never modified.
- Incremental, cancellable MO2 scan. Alpha meaning inferred from the meshes.
- Bulk editing:
  - mixed values and "applies to K of N",
  - Copy settings and Reset all,
  - the Proceed anyway decision dialog.
- The rename dialog with regex and live validation. The searchable path picker. Copy CLI command.
