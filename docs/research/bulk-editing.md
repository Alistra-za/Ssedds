# Bulk editing and batch rename: patterns for sdc

Research date 2026-10-12. Sources are mostly vendor docs plus tutorials; where a source was thin it is flagged. Repo note: suffix vocabulary lives in `crates/engine/src/group.rs (SUFFIXES, VOCABULARY)` (tests: `suffix_vocabulary.rs`, `suffix_aliases.rs`).

## 1. Summary of best patterns

1. Live before/after preview table for every rename or path edit; each row can be excluded (Advanced Renamer).
2. Choose which fields to apply: checkbox per field, with "Check none" / "Check all" (Lightroom Sync dialog). Untouched fields are never written.
3. Mixed values shown as a dash; typing into a dashed field writes to all (Unity). Right-click a label to pull the value from one item.
4. Common-properties-only: show only what all selected items share (Unreal Property Matrix, Unity components).
5. "Copy from the active item to the rest" is a distinct, simple mode (Lightroom Sync with the first-selected as source, Blender Copy to Selected).
6. Whole batch is one undo step; Advanced Renamer keeps a per-batch undo list.
7. Save the recipe as a named preset (Bridge Batch Rename presets, Painter export presets).
8. Selection: Ctrl toggles, Shift ranges, a visible count, selection persists across search/filter, and "select all" states its scope (visible vs filtered).
9. Counter tokens need padding (001) and a stable sort order, or results surprise people (Bridge forum bugs).
10. Preview can disagree with the real result; compute the preview and the commit from the same function.

## 2. Per product

### Adobe Lightroom Classic: Sync Settings and Auto Sync
Source: https://lightroomkillertips.com/how-to-edit-multiple-photos-at-once-in-lightroom/ ; https://jkost.com/blog/2024/09/quickly-applying-settings-to-multiple-images-in-lightroom-classic.html (third-party; not Adobe docs)
- The first-selected photo is the source; the others are targets (shown with a brighter frame). Source is explicit, not implied.
- Sync opens a checklist of setting groups, all ticked by default, with Check None so you can tick only a few.
- Auto Sync is a toggle: every edit to the active item is mirrored live to the rest. No dialog, powerful, easy to forget it is on.
- Alt-click Sync reuses the last choice without a dialog; fast but relies on memory of what was chosen.
- The Sync button only appears when more than one item is selected.

### Adobe Bridge: Batch Rename
Source: https://photoshopessentials.com/?p=5223 ; Adobe community threads at https://community.adobe.com/t5/bridge-discussions/batch-rename-incorrect-sequence-numbers/m-p/14314480
- The new name is built from rows (text, sequence number, date, etc.), each row a token; rows can be added and reordered.
- Live preview of the first file's resulting name sits in the dialog.
- Sequence number has start value and digit count; users hit ordering bugs when the counter is at the end of the name or unpadded.
- Whole row layouts save as named presets; users suggest per-folder presets to manage counters.
- A reported case where preview and actual result differed shows why preview and commit should share code.

### Bulk Rename Utility
Source: https://www.bulkrenameutility.co.uk/forum/viewtopic.php?p=15775 ; https://www.softpedia.com/reviews/windows/Bulk-Rename-Utility--82549.shtml
- A "New Name" column sits beside the old name for all files, so every result is visible before committing.
- Many numbered rule panels (regex, replace, case, etc.) are applied in a fixed left-to-right order, so rules compose.
- Rules apply only to highlighted rows; a "nothing changes" confusion arises when nothing is selected or the pattern does not match. Show "0 of N matched" explicitly.
- Collision handling is not documented in what I found (flagged as unverified); do not copy it, design our own.

### Advanced Renamer
Source: https://www.advancedrenamer.com/user_guide/using_aren (index only; collision and undo pages not read, details from search summaries)
- A methods list: stack several operations (add, replace, case, regex, counter), with a preview list.
- Each preview entry can be unchecked to exclude it; F2 overrides the new name of a single row.
- Has an explicit "Name collision rules" feature and an "Undo batch" list that picks a past batch by date/time. Exact options unverified.
- Batch conditions let a method apply only to matching items (good model for "only the _n maps").
- F9 starts the batch; keyboard-first.

### Unreal Engine: Property Matrix
Source: https://dev.epicgames.com/documentation/unreal-engine/property-matrix-in-unreal-engine
- Opened from the selection (Asset Actions > Edit Selection in Property Matrix); selection becomes a table: rows are items, columns are chosen properties, sortable.
- You pick which columns (properties) to show by pinning; the matrix suggests useful ones from the types selected.
- Editing one cell is per item; selecting a column range edits many at once. The table doubles as a compare view.
- Only properties common to all selected objects appear.
- Known limitation for some asset types (materials) in older versions: bulk edit can be disabled where it is unsafe. Precedent for locking.

### Unity: multi-object editing
Source: https://docs.unity3d.com/420/Documentation/Manual/Multi-ObjectEditing.html ; https://docs.unity.cn/cn/2023.2/ScriptReference/EditorGUI.MixedValueScope.html
- Differing values show a dash; equal values show normally. Typing replaces all.
- Right-click a property label to inherit the value from one chosen object.
- Components not on every selected object are not editable, with a note saying so.
- Forum bug: Esc after typing into a mixed field set everything to one value instead of restoring. Lesson: cancel must restore each item's original.
- Some widgets (dropdowns) failed to show mixed state; every control type needs a mixed state.

### Blender: Alt-edit and Copy to Selected
Source: https://devtalk.blender.org/t/how-to-add-functionality-to-copy-to-selected-alt-apply-to-selection-on-greyed-out-menus/6657 ; https://developer.blender.org/T54862
- Hold Alt while changing a value to apply it to all selected; right-click > Copy to Selected pushes the active item's value to the rest.
- Criticized as not discoverable and clashing with Emulate-3-Button-Mouse; hidden modifiers need a visible alternative.
- Active vs selected distinction: one item is the source of truth.
- Not supported on every property, and unsupported ones silently do nothing (bad); disable and say why.

### Substance 3D Painter (texture-set export)
Source: https://helpx.adobe.com/substance-3d-painter/getting-started/export/export-window/export-settings.html
- Export window has "Global settings" for all texture sets plus per-set override; checkboxes on the left choose which sets export.
- Named export presets are saved and reused; presets also drive scripted batch export.
- No direct copy-settings-between-sets command was found in the docs. The model is global default plus override, which is a useful alternative to bulk copy.

### Mod Organizer 2
Source: https://www.nexusmods.com/site/mods/2160 (third-party plugin page)
- Stock MO2 reportedly lacks batch install; a plugin adds "Install selected downloads" only when 2 or more items are selected, running them sequentially through the normal pipeline. Little documented about stock multi-select actions (gap). Takeaway: batch operations reuse the single-item path rather than a parallel code path.

### Multi-selection conventions
Sources: https://learn.microsoft.com/ga-ie/windows/apps/develop/ui/controls/selection-modes ; https://soul.emplifi.io/latest/patterns/patterns/selection-G3EThl0p ; https://www.ordio.com/docs/guides/BULK_ACTIONS_PAGINATION.md
- Extended selection: Ctrl toggles one item, Shift selects a range, plain click resets.
- Show a count of selected items, and offer a way to clear.
- Keep selection when search or filter changes; "select all filtered" adds to the existing selection. State whether Select All means visible or all.
- Bulk actions disabled when nothing is selected, except Select All.
- Store selection by stable ID (a Set), not by row index or widget state, so scrolling and re-sorting keep it.

## 3. Recommended for sdc

### Selection
- Ctrl/Cmd-click toggle, Shift-click range (in the displayed order), plain click resets, Ctrl+A selects all in the current filter, Esc clears.
- Selection keyed by set ID (base name + folder), not row index; survives scroll, sort, filter. Sets hidden by a filter stay selected but the toolbar says "12 selected (4 hidden by filter)".
- Persistent count in the toolbar; "Select filtered", "Invert", "Clear".
- Select by filter/search: text box (substring, optional regex), plus quick filters: has map kind X, missing map X, has warnings, format = Y, output path contains Z.
- Active set (last-clicked) is the "source" for Copy-from-active and is marked.

### Context menu and popups
Right-click on a non-selected row selects it alone first; on a selected row it keeps the selection. Menu is disabled if zero selected. Title each popup with "N sets selected".

### (a) Output path edit
- Modes: Set to fixed folder, Prefix/suffix subfolder, Find/replace (literal by default, regex checkbox), Make relative to a root.
- Live preview table: Set | Old output | New output, with changed rows highlighted; rows are individually excludable.
- Regex tester: pattern + replacement field with `$1`/`${name}` captures, a sample line from the first selected set, invalid-pattern message in place, "N of M matched".
- Checks (block Apply on errors, warn on warnings): two sets resolve to the same output file; target already exists on disk (warn, show overwrite policy); path illegal on Windows (invalid chars, reserved names, too long); directory cannot be created.

### (b) Rename files
- Two scopes with a toggle: rename the set (base name, all maps follow) vs. rename individual map files. Default: base name only, so suffixes are never touched.
- Operations stacked in order: find/replace (literal or regex), prefix/suffix, case convert (lower, upper, Title), trim, tokens `{name}` `{counter:3}` `{folder}` `{old}`. Counter ordered by displayed order, padded.
- Live preview table before/after per set and per map file; exclude rows; override a single row (F2 style).
- Collision checks: duplicates between sets in the batch, clashes with sets not in the selection, clashes with files on disk, case-only differences on Windows (case-insensitive FS).
- Saved presets by name (rename recipe and path recipe), plus "recent".
- Suffix safety (see Risks): validate by re-running sdc's own suffix detection on each new name.

### (c) Per-map settings for all selected
- Show the same editor as the single-set one, but fields display "mixed" (a dash) when values differ across selection, and an apply-checkbox per field; only ticked or edited fields are written. Editing a field auto-ticks it.
- Target by map kind: a row of chips (all, `_n`, `_rmaos`, `_p`, ...) so "format = BC5 for all _n" does not touch others. Show how many maps each chip matches.
- Maps absent in some sets: say "applies to 9 of 12 sets", never silently skip.
- Offer Copy-from-active ("make selected match this set") as a one-click alternative, with the same field checklist as Lightroom Sync.
- Fields that make no sense for a kind (alpha on `_n`) are disabled with a reason, not hidden.
- Per-field right-click: "Take value from set X".

### Cross-cutting
- One undo step per Apply (single command holding old values of every touched set). Toast "Renamed 12 sets - Undo". Ctrl+Z / Ctrl+Y.
- Dry run is the preview itself; Apply does not touch the disk. Since sdc settings are likely metadata until conversion, say whether a rename changes only the planned output name or also renames existing output files; recommend plan-only unless the owner wants file moves.
- Locked/read-only sets (user lock or output pinned): shown with a lock icon, skipped by bulk ops and reported in the preview as "skipped (locked)".
- Sets already converted or currently converting: skip and report, never edit mid-run.
- Preview and commit share one pure function (`plan_edit(selection, recipe) -> Vec<Change>`), so preview cannot drift (Bridge lesson) and it is unit-testable without egui.
- After Apply, keep the selection and flash changed rows; keep the popup's settings for repeated use.
- Keyboard: Ctrl+A, Esc, Enter = Apply when valid, Ctrl+Z, F2 rename, and a shortcut hint in menu items.
- Missed-by-owner extras: presets; row exclusion in preview; match-count for regex; reset-to-default per field in bulk; "changed from default" markers; log of the last batch (copyable) for support.

## 4. Risks

- Regex rename breaks suffix grouping: e.g. `(.*)` replaced greedily or a pattern that eats `_n`, so the map becomes a stray set or gets the wrong kind. Mitigation: rename operates on the base name only by default; for per-file scope, re-run suffix detection on the result and block if the detected kind or grouping differs from before.
- Rename collapses two sets into one name (e.g. removing a prefix), so sets merge on regroup or overwrite each other. Mitigation: duplicate check inside the batch and against unselected sets, hard block.
- Output-path collisions: two sets writing the same file; case-insensitive duplicates on Windows; path length limits (260 chars); invalid characters from regex captures. Mitigation: checks above, before Apply.
- Overwriting existing DDS files on disk with no warning; decide and display the overwrite policy in the preview.
- Counter instability: renumbering depends on sort order; sort changes between preview and Apply. Mitigation: freeze the order in the plan.
- Regex hazards: invalid pattern while typing, catastrophic backtracking on pathological patterns (use the Rust `regex` crate, which is linear-time, and no look-around), replacement syntax confusion (`$1` vs `\1`), Unicode and backslash path separators.
- Mixed-value UX bugs: cancel or Esc must restore each item's own value (Unity bug); a mixed field must not be written unless touched.
- Silent partial application: maps or fields missing on some sets get skipped without telling the user. Always report counts.
- Hidden selection: filtered-out sets remain selected and get edited unexpectedly. Show "hidden by filter" in the count, and make the scope of Apply explicit in the popup title.
- Undo spanning both settings and any disk renames: if files are moved, undo may fail partway. Prefer plan-only edits; if moving, make undo best-effort and report failures.
- Auto-sync style live mirroring (Lightroom) is easy to leave on and hard to audit; recommend explicit Apply only.
- Evidence gaps: Bulk Rename Utility and Advanced Renamer collision/undo behavior and MO2 stock multi-select were not verified from primary docs.
