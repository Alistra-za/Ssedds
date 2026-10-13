# sdc M6 Track F3a: multi-select and bulk per-texture settings (design)

Base: `main` ee303e6, branch `feat/m6-f3a-bulk`. Inputs: the controller's decisions `2026-10-13-sdc-m6-f3a-decisions.md` (D1-D7, made under the owner's delegation) and the research `docs/research/bulk-editing.md` §3. Where this spec adjusts a decision to fit the code, it says so in §9.

## 1. Intent

**The owner said (2026-10-12/13):**
- Multi-select sets; a right-click menu opens popups; per-map settings are applied to all selected sets.
- Fill the gaps, and follow how quality software does it.
- F3a is multi-select, bulk per-map settings and the right-click menu. F3b is input fixing, output rename and the PGPatcher rename.

**Success:**
- Ctrl+click three textures, pick *BC1* in the Format row of the panel that appears: all three get `format = "bc1"` in `sdc.toml`, one Ctrl+Z takes all three back, and the folder is planned once.
- Select two sets, pick the `Normal _n` chip, set *Normal handling*: only the normal maps of those sets change.
- A field that cannot change on some selected textures says "applies to K of N", and the panel says which ones were skipped and why. Nothing is skipped silently.
- With one texture or one set selected, the GUI looks and behaves exactly as at ee303e6.

**Out of scope (D6, YAGNI):**
- Saved recipes or presets of bulk edits.
- Auto-sync (live mirroring of the active texture's edits).
- Alt-click or other hidden modifiers.
- Regex anything, path, rename and output edits (F3b).
- Select-by-filter beyond the existing filters and search (search plus Ctrl+A covers it).
- Bulk editing of the Advanced drawer's fields (encoder radius, rgbcx level, header, resize). The drawer keeps editing one texture. Copy-from-active does copy them (§5.2).
- Bulk editing of set-level settings (shader, PGPatcher JSON, slot commands). Only per-texture `[files]` fields are bulk-edited.
- Copy-from-active between sets (§5.2, §9).
- Modifier clicks on the small member thumbnails inside a set pill: they keep selecting one texture.

## 2. Selection (D1)

### 2.1 Model

- `AppModel::selection: Selection` (`model/mod.rs:47-52`, unchanged) stays the **active** entry. About 110 sites read it, tests included. Only the three highlight reads in the columns (`columns.rs:113`, `sets_column.rs:314`, `:404`) move to `AppModel::is_selected`; the rest are unchanged.
- New field `AppModel::multi: bulk::MultiSelection { keys: Vec<String>, sets: bool }`. It holds every selected key of one kind, the active one included, in the order they were added. `sets` says whether they are set keys (`SetItem::key`) or item keys (`TextureItem::key`).
- **In effect** only while `keys.len() >= 2` and the active entry is of that kind and is one of the keys. Otherwise the selection is the single active entry, exactly as today. So code that writes `selection` directly (tests, `apply_plan`'s set fallback) can never produce a half-valid multi-selection.
- `select_item` and `select_set` (`model/mod.rs:1314-1320`) clear `multi` and the bulk panel state. Every existing single-selection path (drop dialogs, export-issue links, the "← set" back link, Show packed output, `expand`) keeps its behaviour.
- `open_folder` clears both (`model/mod.rs:425`).
- Keys, never row indices: the selection survives scrolling, filtering and a re-plan.

### 2.2 Clicks and keys

| Input | Textures column (tiles) | Sets column (pills) |
|---|---|---|
| Click | selects that texture alone | selects that set alone |
| Ctrl+click | toggles it; an added key becomes active; removing the active one makes the last remaining key active; removing the last one leaves nothing selected | same over sets |
| Shift+click | selects the range from the active texture to the clicked one in `visible_items()` order (search and filters applied); the active one stays active; the range replaces the previous selection | same, in the Sets column's display order (`sets::views`) |
| Ctrl+A | every visible texture | every set, when a set is the active entry |
| Esc | clears the selection | same |

- Shift+click with no active entry of that kind, or with the active one hidden by the filter, is a plain click.
- A modifier click on the other kind (Ctrl+click a tile while sets are selected) is a plain click: textures and sets never mix.
- Ctrl+A keeps the active entry when it is among the selected; otherwise the first becomes active.
- Ctrl+A and Esc act only when no widget has keyboard focus (so Ctrl+A in the search box still selects text), no dialog or popup (context menu, combo) is open, and no texture is expanded (the preview owns Esc, `ui/preview.rs:158`).

### 2.3 Count

- While two or more textures are selected, the Textures heading row shows "12 selected", or "12 selected (4 hidden by filter)" when some of them are not in `visible_items()`.
- The Sets heading shows "N selected". Sets have no filter, so nothing is ever hidden there.
- Every selected texture is highlighted, as a tile and as a member thumbnail inside its set pill.
- Hidden selected textures are still edited by a bulk apply. The count is the warning (research §4, "Hidden selection").

### 2.4 After a plan

`apply_plan` (`model/mod.rs:1167-1175`) first prunes the multi-selection: keys whose item or set no longer exists leave it. If the active key is gone, the last remaining key becomes active. With fewer than two keys left, the selection is single again. Then its existing single-selection check runs unchanged. The bulk panel state (chip, report) survives a plan, so the report of an apply stays visible after its re-plan.

## 3. Bulk settings panel (D2)

### 3.1 When it shows

The centre (`ui/mod.rs:98-123`) shows the bulk panel while `AppModel::in_bulk()`: two or more textures, or two or more sets, are selected and no texture is expanded. Otherwise it is unchanged.

### 3.2 Rows

- The panel lists the single panel's rows, in its groups and order (`settings_panel.rs:32` `GROUPS`): Mipmaps (Levels, Filter, Edge), then `fields::FIELDS` (`fields.rs:8-22`). This list is `bulk::panel_fields()`.
- Levels is a combo (`options_of("mips.levels")`: Full chain, None, 2-13 levels), not the single panel's tick box, so a mixed level count has a value to show.
- The title reads "N textures selected".
- Options for a combo come from the active texture's job, as the single panel's do (`settings_panel.rs:121-200`). With sets selected, they come from the first target that has a job.

### 3.3 Each row's state (`bulk::BulkField`)

For every target texture, one rule decides whether a write of the field reaches it (`bulk::skip_of`, §5.1):
- **Applies**: the write reaches it.
- **Skipped, with a reason**:
  - the texture has no job: a raw map packed into another output, a skipped DDS, a failed file. The reason is "has no settings of its own (packed, skipped or failed)";
  - the field is hidden for its profile (`fields::field_hidden`: Coverage outside the cutout policies). The reason is "used only by cutout and cutout + blend";
  - the field's `FieldView::lock` is set (`fields.rs:106-143`, e.g. "normal maps only", "BC1 only"). The lock's text is the reason;
  - for a write of `mips.filter`, the picked filter is barred for that texture (`fields::filter_option_lock`).

What the row shows:
- **Equal values** across the textures it applies to: shown as in the single panel.
- **Mixed values**: "— mixed". A combo shows "— mixed" as its text. A tick box shows egui's indeterminate mark (`Checkbox::indeterminate`); a click ticks it on every texture. A slider (Threshold) sits at its default position, 128, and the note column says "— mixed".
- **Applies to fewer than all**: the note column says "applies to K of N". Hovering it lists "locked on M of N: <reason>", one line per reason.
- **Applies to none**: the row is disabled and still listed.
- **↺** shows when at least one texture the field applies to has its own `[files]` value for it (`FieldView::edited`). It clears that field on every texture it applies to.
- While `sdc.toml` is read-only, every row is disabled with the existing reason (`settings_panel::toml_read_only_reason`).

### 3.4 Writes

- Picking a value writes it to every texture the field applies to. Nothing else is written: a row that is not touched is never written. A mixed slider writes nothing until it is dragged and released (`commit_slider`), and a mixed combo writes nothing until a value is picked.
- Every write goes through the single-texture path, `AppModel::set_field` / `clear_field` (`model/mod.rs:1477-1497`). So a Sub-type write runs `edit_subtype` per texture, with its PBR follow-on (`pbr_follow.rs:42`). The texture's own explicit write wins over a follow-on switch from its diffuse, in either order (a follow-on only switches members with no `[files]` sub-type; a member's own write after the switch removes it from `auto_pbr`).
- After the apply, the panel shows a report line, e.g. "Normal handling: written to 9 of 12; locked on 3 of 12: normal maps only".
- An Esc or a cancel never leaves a half-written field: a combo writes only on a pick, and a slider only on release (the Unity bug in research §2 cannot happen).

## 4. Sets selected: map-kind chips (D3)

- With two or more sets selected, the bulk panel starts with a chip row. **All** comes first, then one chip per `sets::SlotKind` present among the members, in `SlotKind::ALL` order. Each chip is labelled with `SlotKind::label()` ("Normal _n", "RMAOS", "Height _p", ...) and its member count.
- A member's kind is `sets::slot_of(item)`, its resolved sub-type's slot. Members with no slot (raw packing sources, env masks, sub-types without a card) appear under All only.
- The panel's targets are the members of the picked kind across the selected sets, in set order and then member order. All means every member.
- When some selected sets have no member of the picked kind, the chip row says "applies to K of N sets".
- The picked chip is model state (`AppModel::bulk.kind`). A new selection resets it to All.

## 5. Model API

All of it is in a new module `crates/gui/src/model/bulk.rs`, with no egui types, plus small additions elsewhere.

### 5.1 Plan and rows (pure)

```rust
pub enum BulkOp { Set(serde_json::Value), Clear }
pub enum Change { Write, Skip(String) }
pub fn plan_bulk_edit(m: &AppModel, keys: &[String], field: &str, op: &BulkOp) -> Vec<(String, Change)>;
pub struct BulkField { pub field, pub label, pub value: Option<Value>, pub applies: usize,
                       pub edited: usize, pub skipped: Vec<(String, usize)>, pub total: usize }
pub fn bulk_fields(m: &AppModel, keys: &[String]) -> Vec<BulkField>;
pub fn panel_fields() -> Vec<(&'static str, &'static str)>;
```

- `plan_bulk_edit` is what the apply writes. `bulk_fields` is what the panel shows. Both call the same per-texture rule `skip_of`, so the preview counts and the commit cannot drift (research §1.10). `bulk_fields` calls it with `BulkOp::Clear`, so its counts are the value-independent ones. The filter lock, which depends on the value, shows only in the report.
- `value` is `None` when the textures the field applies to resolve to different values (mixed), or when it applies to none.
- `bulk_fields` serialises each texture's profile once for all rows (`fields::views_of`), not once per row.
- `fields.rs` gains `all_fields()`: every field the settings panel and the Advanced drawer edit, each once, in display order. It also gains `field_view(target, job, field) -> Option<FieldView>`, a lookup in that list, and `views_of` becomes `pub(crate)`.

### 5.2 Commands

```rust
impl AppModel {
    pub fn apply_bulk_edit(&mut self, keys: &[String], field: &str, op: BulkOp);
    pub fn open_copy_settings(&mut self);
    pub fn apply_copy_settings(&mut self, from: &str, to: &[String], fields: &[String]);
    pub fn ask_reset_all(&mut self);
    pub fn reset_all_settings(&mut self, keys: &[String]);
    pub fn generation(&self) -> u64; // tests: the plan generation, bumped once per `replan`
}
```

- **One undo step, one plan (D5).** Every command runs inside one private helper, `AppModel::batch_edit(label, f)`, which is the audit's accept-all pattern made reusable:
  1. one snapshot before;
  2. `batch = true`, so `edit_docs` changes the documents but records no step and does not re-plan;
  3. the per-texture writes;
  4. `batch = false`, one snapshot after;
  5. if the text changed: one `history.push`, `auto_replans = 0`, one `replan()`.
- `accept_all_suggestions` (`model/mod.rs:821-865`) is rewritten on top of `batch_edit`, with no change in behaviour.
- `edit_docs` (`model/mod.rs:1348-1378`) skips its `before` snapshot while batching. Today it serialises both documents on every write even though it then returns, which makes a batch of N writes O(N²) in the document size.
- **Read-only (D5).** While `sdc.toml` is read-only, the three write commands (`apply_bulk_edit`, `apply_copy_settings`, `reset_all_settings`) return at once: no step, no plan, no report. The panel shows the read-only reason, and the menu entries that would lead to a write are disabled with it.
- **The selection is kept** after every command.
- **Copy from active (D4).**
  - `open_copy_settings` opens `Dialog::CopySettings { from, to, fields }`. `from` is the active texture, `to` is the other selected textures, and `fields` lists `(field, ticked)` for every field of `fields::all_fields()` that `from`'s `[files]` entry sets, all ticked, in `all_fields` order.
  - The dialog has Check all / Check none and Apply (disabled with nothing ticked).
  - Apply writes each ticked field's value from `from` to every texture in `to` it applies to (the §3.3 rule per field), as one step.
  - `allow` (acknowledged codes), `rnm.*`, `pack.*` and `channels.*` entries are never copied: they belong to one file.
- **Reset all (D4).**
  - `ask_reset_all` opens `Dialog::ConfirmResetAll(keys)`. With textures selected, `keys` is the selection. With sets selected, it is every member of those sets.
  - Confirming removes each texture's whole `[files]` entry, acknowledgements included, as one step. A texture with a `[files]` sub-type has it cleared first through `clear_field`, so PBR follow-on members revert as they do for a single reset.
  - Locks do not apply: clearing an entry never fights a lock.
  - Set-level settings in `sdc-project.toml` (shader, JSON) are not touched.
- **Undo step labels:** "Change <Label> on N textures", "Reset <Label> on N textures", "Copy settings from <name>", "Reset all settings on N textures".

### 5.3 Selection and menus

```rust
pub struct MultiSelection { pub keys: Vec<String>, pub sets: bool }
pub struct BulkState { pub kind: Option<SlotKind>, pub report: Option<String> }
pub struct KindChip { pub kind: Option<SlotKind>, pub label: String, pub members: Vec<String>, pub sets_with: usize }
pub enum MenuAction { EditSettings, CopyFromActive, ResetAll, SelectMembersOfKind(Option<SlotKind>) }
pub struct MenuEntry { pub action: MenuAction, pub label: String, pub disabled: Option<String> }
impl AppModel {
    pub fn selected_items(&self) -> Vec<String>;  pub fn selected_sets(&self) -> Vec<String>;
    pub fn is_selected(&self, key: &str, sets: bool) -> bool;  pub fn in_bulk(&self) -> bool;
    pub fn click_item(&mut self, key: &str, ctrl: bool, shift: bool);
    pub fn click_set(&mut self, key: &str, ctrl: bool, shift: bool);
    pub fn select_all_visible(&mut self);  pub fn clear_selection(&mut self);
    pub fn selection_summary(&self, sets: bool) -> Option<String>;
    pub fn bulk_targets(&self) -> Vec<String>;
    pub fn select_members_of_kind(&mut self, kind: Option<SlotKind>);
    pub fn right_click_item(&mut self, key: &str);  pub fn right_click_set(&mut self, key: &str);
    pub fn item_menu(&self) -> Vec<MenuEntry>;  pub fn set_menu(&self) -> Vec<MenuEntry>;
    pub fn menu_title(&self, sets: bool) -> String;  pub fn run_menu(&mut self, a: MenuAction);
}
pub fn kind_chips(m: &AppModel, set_keys: &[String]) -> Vec<KindChip>;
```

## 6. Right-click menu (D4)

- **Right-clicking** an entry that is not selected selects it alone first. Right-clicking a selected one keeps the selection. The menu's first line is its title: "N textures selected" or "N sets selected".
- **Tile menu** (`MenuAction` list from `item_menu`, shown for any tile):
  1. "Edit settings for N selected": closes an expanded preview, so the centre shows the panel.
  2. "Copy settings from <active name> to N selected…", where N counts the other selected textures. Disabled, with the reason on hover, when fewer than two textures are selected, when the active one has no `[files]` value among `all_fields()`, or when `sdc.toml` is read-only.
  3. "Reset all settings for N selected…": opens the confirm. Disabled while `sdc.toml` is read-only.
- **Pill menu:**
  - With two or more sets selected and the pill among them, the pill shows `set_menu` instead of its existing menu:
    1. "Edit settings for N selected": sets the chip to All and closes the preview.
    2. "Reset all settings for N selected…": its confirm lists every member.
    3. "Select members of kind ▸", a submenu with one entry per chip ("All (12)", "Normal _n (3)", ...). It replaces the set selection with a texture selection of those members. The first becomes active, so the panel shows them as textures and Copy-from-active becomes available.
  - Otherwise the existing pill menu (Rename, Edit match, Remove member, Delete set, Move up/down; `sets_column.rs:503-560`) is unchanged.
- The menus are built from `Vec<MenuEntry>`, drawn by one helper. F3b appends its entries to these lists.

## 7. Error cases

| Case | Behaviour |
|---|---|
| `sdc.toml` read-only | Rows disabled with the reason; commands return without a step or plan; Copy and Reset entries disabled with the reason |
| `sdc-project.toml` read-only | Bulk `[files]` edits work; a Sub-type write changes the diffuse but not its members (P8, as `edit_subtype` does) |
| A target texture vanished before the apply (plan landed in between) | It has no job: skipped with the "no settings" reason |
| Every target skipped | No step, no plan; the report says "written to 0 of N" with the reasons |
| A chip whose kind has left after a plan | The targets fall back to All |
| The active texture deleted on disk | §2.4: the last remaining key becomes active |
| Hidden-by-filter textures selected | Edited too; the count says how many are hidden |
| A plan in flight during an apply | The apply reads the plan on screen (as every single edit does); its own re-plan supersedes the one in flight |

## 8. Testing (D7)

Model tests, in the existing style (`crates/gui/tests/*.rs` with `support::opened`, `pbr_opened`, `opened_as_the_app`, real plans on a temp folder):
- `bulk_selection.rs`: Ctrl toggle and the active key; Shift range in displayed order with a filter on; Ctrl+A and the "hidden by filter" count; survival across a re-plan and the drop of a deleted key; sets and textures not mixing; a plain `select_item` ending the multi-selection.
- `bulk_plan.rs`: mixed and equal values; a locked field ("normal maps only") with "applies to 1 of 2" and the skip text; a texture with no job; `plan_bulk_edit` returning exactly `Write`/`Skip` per key; the filter lock on a write.
- `bulk_apply.rs`:
  - 300 textures, one apply: one history step, `generation()` up by exactly 1, all 300 written, the selection kept, one undo restores all;
  - a Sub-type in bulk switching each set's inferred `_n` through the PBR follow-on, one undo;
  - read-only `sdc.toml` as a no-op;
  - copy-from-active with one field unticked;
  - reset all;
  - the skip report.
- `bulk_sets.rs`: chips with counts and "K of N sets"; `bulk_targets` per chip; Select members of kind; right-click keeping or replacing the selection; the menu lists; ResetAll's confirm over set members.
- egui drawing through kittest smoke tests (`bulk_ui.rs`): Ctrl+click on tile labels, the count in the heading, the panel's title and "— mixed", Ctrl+A and Esc, and the copy dialog's checklist. Drawing logic stays in model helpers, as elsewhere.
- Unchanged: every existing test, `crates/engine/tests/golden.rs`, the schemas (nothing serialised changes), `Cargo.lock`.
- **No version bump.** The workspace version is the engine's `ENGINE_VERSION` (`manifest.rs:15`, `env!("CARGO_PKG_VERSION")`, every crate `version.workspace = true`), and `is_up_to_date` requires a record's `engine_version` to match it (`api.rs:603`). A GUI-only 0.8.0 would make `--on-existing if-changed` re-encode every output. The version stays 0.7.0, and `docs/presets-changelog.md` gets no entry (its entries exist to announce re-encodes).

## 9. Adjustments to the decisions

- **D1 "`AppModel.selection` gains a multi-selection".** The multi-selection is a sibling field, `AppModel::multi`, and `selection` stays the active entry with its type unchanged. Changing the enum would touch about 110 sites, tests included, for the same behaviour.
- **D2's example lock (complex_grass Sub-type) is not at the base.** The commit that adds it (fbaceb4, "the set shader drives complex_grass; ... locked row") is only on `fix/gras-query`, not on `main` ee303e6: `fields.rs:226` reads `lock: field_lock(target, &job.profile, field)` with no shader lock.
  - The bulk rule reads `FieldView::lock` generically (through `fields::field_view`, which calls `views_of`), so the grass lock applies with no F3a change once that branch merges.
  - The F3a tests use a lock that exists at the base ("normal maps only").
  - `views_of` is a merge-conflict site between the two branches: keep both changes.
- **D2 "locked on K of N" and "applies to K of N".** At the base, every reason a write can skip a texture is either a lock or a not-applicable case, and the code does not tell them apart (both are `FieldView::lock`). So there is one rule: the note says "applies to K of N", and its hover lists "locked on M of N: <reason>" per reason.
- **D4 copy-from-active on sets.** Copy is offered for textures only. For sets, "Select members of kind ▸" turns the selection into textures of one kind, and Copy then works on them. Copying "set A's members to set B's same-kind members" needs a per-kind source rule, and nothing in D1-D7 defines one.
- **D4 "the active item's explicitly set fields".** These are read as the fields of `all_fields()` that the entry sets. `allow`, `rnm.*`, `pack.*` and `channels.*` belong to one file and are not offered.
- **D2 "Reset (clear) works per field for all selected textures".** A per-field reset reaches the textures the field applies to, the same set a write reaches, so the "applies to K of N" counts do not depend on the operation. A stale `[files]` value on a texture where the field is locked is cleared from that texture's single panel, or by Reset all.
- **D5 `plan_bulk_edit(model, selection, field, value)`.** The signature takes the target keys (the panel resolves them, chips included) and a `BulkOp` (set or clear).
