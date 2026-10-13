# F3a design decisions (controller, 2026-10-13)

The owner said "continue with F3a and keep going until NVTT is done". Each design question is decided here from the owner's stated scope and the research doc (docs/research/bulk-editing.md, section 3). These decisions are the input to the spec. Anything the owner later disagrees with goes in DECISIONS as a correction.

Owner's scope (2026-10-12, verbatim intent):
- "Multi-select sets; right-click menu opening popups: [...] per-map settings applied to all selected sets."
- "Fill gaps + research how quality software does it."
- F3 split (owner, 2026-10-13): F3a is multi-select, bulk per-map settings and right-click. F3b is input fixing (edit path, input rename), output rename and auto PGPatcher rename.

## D1 Selection model
- `AppModel.selection` gains a multi-selection: an ordered set of keys plus an "active" key. The active key is the last plain-clicked or Ctrl-clicked one. Items and sets stay two separate kinds; selecting a set and selecting textures do not mix.
- Textures column (tiles):
  - plain click selects one;
  - Ctrl+click toggles;
  - Shift+click selects the range in the displayed order (filters applied);
  - Ctrl+A selects all visible;
  - Esc clears.
- Sets column (pills): the same, over sets.
- Selection is keyed by key, so it survives a re-plan, scrolling and filtering. Keys that disappear after a re-plan are dropped.
- The header shows the count, with how many are hidden by the current filter: "12 selected (4 hidden by filter)".
- One selected entry behaves exactly as today. The settings panel and the set editor are unchanged for a single selection.

## D2 Bulk settings panel (2+ textures selected)
- The right panel shows the same field rows as the single-texture settings panel, over all selected textures:
  - **Equal values** show normally.
  - **Mixed values** show "— mixed" (every control type needs a mixed state).
  - **Locked fields:** a field locked for some items (e.g. the grass Sub-type set by the set's shader) shows "locked on K of N", and a bulk write skips those items. The panel reports the skip.
- Editing a field writes it to every selected texture except the locked ones. Fields not touched are never written.
- Reset (clear) works per field for all selected textures.
- Sub-type in bulk goes through the existing single-item path (`edit_subtype`, so the PBR follow-ons stay right), per item, inside one undo step.
- The title reads "N textures selected". A field that does not apply to some items (e.g. alpha on an `_n`) applies only where it makes sense and says "applies to K of N". It never silently skips.

## D3 Sets selected, then per-map settings
- With 2+ sets selected, the right panel shows a map-kind chip row: All, Diffuse, `_n`, `_rmaos`, `_p`, and so on, built from the slot kinds present. Each chip shows its count.
- Picking a chip opens the D2 bulk panel over the members of that kind in the selected sets. "All" means every member.
- The panel says "applies to K of N sets" when some sets lack that kind.

## D4 Right-click menu
- On a tile or a set pill:
  - Right-clicking an unselected entry selects it alone first.
  - Right-clicking a selected entry keeps the selection.
  - The menu title gives the count.
- Items in the menu:
  - "Edit settings for N selected": focuses the bulk panel.
  - "Copy settings from <active> to N selected…": opens a dialog with a per-field checklist of the active item's explicitly set fields (`[files]` values), each ticked. Apply writes the ticked fields to the others.
  - "Reset all settings for N selected": clears `[files]` entries, after a confirm.
  - "Select members of kind ▸" (sets only): selects the textures of that kind in the selected sets.
- F3b adds its rename and path entries to this same menu later. Keep the menu built from a list, so F3b only appends.

## D5 Undo, re-plan, safety
- Every bulk apply is ONE undo step: one snapshot, one Command, ONE re-plan. Reuse the accept-all pattern from audit Task 7 (one snapshot before, `replan = false` per write, one push, one `replan()`).
- Read-only `sdc.toml` makes the bulk edit a no-op with the existing notice.
- After apply, the selection is kept.
- The preview and the commit share one pure function: `plan_bulk_edit(model, selection, field, value) -> Vec<(key, Change)>`. It is unit-testable without egui. The panel's "applies to K of N" counts come from it.

## D6 Out of scope for F3a (YAGNI, say so in the spec)
- Recipes/presets of bulk edits.
- Auto-sync / live mirroring.
- Alt-click hidden modifiers.
- Regex anything (that's F3b).
- Path, rename and output edits (F3b).
- Select-by-filter beyond the existing filters/search. The existing search plus Ctrl+A covers it.

## D7 Testing
- Model-level tests (the existing gui test style) for:
  - selection: ctrl, shift range in the displayed order, filter-hidden count, survival across re-plan;
  - `plan_bulk_edit`: mixed, locked, not-applicable, counts;
  - one undo step plus one re-plan for N=300;
  - copy-from-active with a checklist;
  - map-kind chips over sets.
- Egui drawing is covered through model helpers, as elsewhere.
