# sdc M3b: set editor and PGPatcher JSON, design specification

| Field | Value |
|---|---|
| Date | 2026-10-10 |
| Status | Design approved by the owner in conversation, section by section (2026-10-10). Written spec awaiting owner review |
| Milestone | M3b, the second of three GUI parts (M3a → M3b → M3c, owner 2026-10-10); the extras deferred from it form a later M3b-2 (§9) |
| Builds on | M3a, spec `2026-10-10-sdc-m3a-converter-gui-design.md` (merged into `main` at ddf0a3c) |
| Inputs | `F:/ai/dds/gui reference/v1 instruction.txt`, `Rough.jpg`, `v1-gap-review.md` (with its owner decisions of 2026-10-09), `ref/pbr.schema.json` (PGPatcher `main`, fetched 2026-10-09); plan `2026-10-08-skyrim-dds-converter-phase0-2.md`, "Carried from M3a" |

**Tags.** As in the main spec and the M3a spec:
- **[Requirement]**: decided by the owner (in the 2026-10-10 M3b design conversation unless cited otherwise);
- **[Finding]**: backed by the code, the PGPatcher schema or source;
- **[Proposal]**: a design choice made here;
- **[Open]**: undecided, with the shipped default stated.

---

## 1. Purpose and scope

**[Requirement]** M3b turns the read-only Sets column of M3a into an editor and adds a writer for PGPatcher's TruePBR JSON. When M3b is finished, you can:
1. open a folder;
2. arrange its textures into sets;
3. set each set's material settings;
4. export the DDS files and a PGPatcher JSON;
5. run PGPatcher, which patches the meshes as expected.

**In scope:**
1. Fix first: the M3a stale `after_export` defect (§8).
2. Set editing: drag and drop, new and deleted sets, reordering, shared slots, duplicating a source (§3).
3. The material editor (layout A) with per-slot presets and slot commands (§4).
4. The PGPatcher JSON model and writer: schema keys, write rules, mutual exclusion, match modes and prefixes, validation (§5).
5. Export of PBR sets to `textures\pbr\` plus the JSON, with export checks (§6).
6. The project file `sdc-project.toml` and undo/redo (§7).
7. Two additive engine fields, so your sets drive packing and, later, the M4 bake (§2).

**Out of scope.** These are listed for a later round in §9:
- importing and merging existing PBR mods;
- landscape EDID JSON;
- rule and suffix-alias editors;
- NIF reading;
- the `delete` key;
- hex colour paste and a colour picker.

**[Requirement]** The PGPatcher JSON is **GUI-only**. `sdc convert` never reads `sdc-project.toml` and never writes JSON; CLI users write their JSON by hand.

**Change to the M3a split.** "Duplicating a shared source" moves from M3c to M3b (§3.4). M3c keeps texture edits and the bake protection that needs M4.

---

## 2. Architecture

### 2.1 Engine: two additive `BatchRequest` fields

**[Requirement]** The GUI's set edits reach the engine, so packing (spec §6.4) and the M4 bake follow your sets, not the automatic grouping (owner: approach 1). **[Proposal]** The CLI never sets these fields, so its behaviour is unchanged.

1. **`set_edits: Vec<SetEdit>`**, applied in `group.rs` after `group_sets_with`. The name avoids `SetOverride`, which already means `--set` field overrides.

   | Variant | Effect |
   |---|---|
   | `Create { key }` | A new, empty set |
   | `Rename { from, to }` | Changes the set's identity; members are kept |
   | `Delete { key }` | Its members go back to the automatic grouping |
   | `Move { file, to }` | The file leaves its current set and joins `to` |
   | `Remove { file }` | The file becomes a set of its own |
   | `Order { keys }` | The order of sets, used for shared-slot ownership (§3.3) |

   - Files are named by their path relative to the project, matched case-insensitively (the M3a `[files]` key rule).
   - An edit naming a file or set that doesn't exist is skipped and reported, never fatal. It produces a new info code `SET-EDIT-STALE` with the edit's text.
   - Edits apply in list order.
   - A file is a member of exactly one set (§3.3).
2. **`set_outputs: BTreeMap<String, SetOutput>`**, keyed by set key.
   - A `SetOutput { prefix: PathBuf }` gives a set's members the output path `<prefix><canonical suffix>.dds` under the output root, instead of mirroring the source path.
   - The GUI fills it only for PBR sets on the CS target (§6.1).
   - Packed recipe outputs (`_rmaos`, `_rfaos`) follow the same prefix.
   - The duplicate-output check (`IN-DUPLICATE`) runs after prefixes are applied, so two sets that write the same file are caught (§6.3).

**[Finding]** Both fields default to empty. With them empty, the existing golden cases and `crates/cli/tests/gui_repro.rs` must stay byte-identical (§10).

### 2.2 GUI modules

| Module | Responsibility |
|---|---|
| `project.rs` (extended) | Also loads and saves `sdc-project.toml` (§7.1), with the same safety rules as `sdc.toml` |
| `model/sets.rs` | The set-edit list, the derived set view (members per slot, owner of each shared slot, derived match), and drop rules (§3) |
| `pgp/schema.rs` | The vendored `pbr.schema.json` (`crates/gui/assets/pbr.schema.json`, recorded in THIRD_PARTY_NOTICES), the key table, write rules and the mutual-exclusion table (§5) |
| `pgp/prefix.rs` | Match modes and output prefixes, including PGPatcher's per-slot suffix table (§5.5) |
| `pgp/writer.rs` | Builds the `{ "$schema", "default", "entries" }` document and validates it (§5.6) |
| `model/history.rs` | The undo/redo command history (§7.2) |
| `ui/set_editor.rs`, `ui/slot_commands.rs`, `ui/sets_column.rs` | Layout A (§4), the slot-commands dialog, and an editable Sets column |

**[Proposal]** The JSON model and writer depend on no egui types, so they get plain unit tests, as M3a's `AppModel` does.

---

## 3. Editing sets

### 3.1 Sets column

- **[Requirement]** Set pills can be dragged to reorder them. The order is the order of JSON entries, and it decides which set owns a shared slot.
- **[Requirement]** Dragging a texture from the Textures column onto a pill makes it a member.
  - Its slot comes from its suffix or resolved sub-type.
  - If that slot is already taken, the GUI asks "Replace `<file>` / Cancel".
- **[Requirement]** Dropping a texture on the empty area of the column starts a new set, named after the texture's base name.
- **[Requirement]** Right-clicking a pill offers:
  - Rename;
  - Edit match… (this opens the set editor on the Match group);
  - Remove member;
  - Delete set (its members go back to automatic grouping);
  - Move up / Move down.
- **[Requirement]** Pill badges:
  - "PBR → JSON" or "vanilla set (no JSON)";
  - the warning count, with a red badge when export is blocked (§6.3);
  - "owns `<slot>`" or "borrowed from `<set>`".
- **[Proposal]** The M3a "Not in a set" filter (a file whose stem groups with no other file, after your edits) is the Textures column's default view while any such texture exists.

### 3.2 Automatic grouping plus overrides

**[Requirement]** Your edits are overrides on top of the engine's automatic grouping. `sdc-project.toml` stores only what you changed. New files added to the folder later still group themselves automatically, and a set you never touched follows the engine if its grouping rules improve.

### 3.3 Sharing a slot

- **[Requirement]** Dropping a texture that already belongs to another set offers four choices:
  1. **Move it here**: a `Move` edit.
  2. **Share**: this set borrows the other set's finished output.
  3. **Duplicate the source for this set** (§3.4).
  4. **Cancel**.
- **[Requirement] Ownership.** The first set in your order owns a shared output. It is encoded once, at the owner's path.
  - On CS, every other set's JSON entry gets a slot command pointing at that file. Example: `slot6: "pbr\landscape\rock01_rmaos.dds"`; the schema adds `textures\` in front.
  - The set editor shows "shared with rock02, owned by rock01".
  - If the owner is deleted or moved below a borrower, ownership passes to the next set in order.
- **[Proposal] Sharing works per finished output, not per raw map.**
  - A borrowed slot is a finished DDS of another set.
  - Raw maps that feed packing belong to exactly one set. Two sets with different normals but one roughness map would otherwise need two different packed outputs from one input.
  - To share a raw map, duplicate it (§3.4).
- **[Requirement]** On the vanilla and CPM targets, a borrowed slot becomes a copy, encoded at the borrowing set's path (v1 instruction).
- **[Requirement]** For M4: a borrowed roughness slot is marked shared. M4 bakes no normal variance into it, and the normal is only renormalised (v1 instruction). M3b records the flag; M4 enforces it.

### 3.4 Duplicating a source

**[Requirement]** "Duplicate the source for this set" copies the file into the same source folder, named for the target set (e.g. `rock02_rough.png`), so it groups there by itself.
- **[Proposal]** If that name already exists, the GUI appends `-2`, `-3`, … to the base name and adds a `Move` edit.
- **[Requirement]** Undo deletes the copy only if its SHA-256 is still the one recorded when it was created. Otherwise undo of that step is refused, with the reason.

---

## 4. Material editor (layout A)

**[Requirement]** Selecting a set shows its material editor in the centre: slot cards on the left, PGPatcher settings on the right, and less-used settings in the Advanced drawer.

### 4.1 Slot cards

- **[Requirement]** One card per slot: Diffuse, Normal, RMAOS, Height, Emissive, Subsurface, Coat (`_cnr`), Fuzz (`_f`). Each card shows:
  - the thumbnail and the source file (or "empty");
  - a preset dropdown labelled with the BC format and sRGB/linear (e.g. "BC7 · sRGB"). A locked option shows its reason; it isn't hidden.
- **[Requirement]** The **Opaque** checkbox sits on the Diffuse card.
  - Ticked, it sets the diffuse texture's alpha policy to `remove`.
  - Unticked, it returns to the resolved default.
- **[Proposal]** The preset dropdown writes the texture's format through M3a's `[files]` entry, so the GUI and the encoding settings show the same value.
- **[Requirement]** Clicking a card opens that texture's M3a encoding settings, with a "← `<set>`" link back. Double-clicking expands the 2D preview (M3a behaviour).
- **[Requirement]** A borrowed slot shows "borrowed from `<set>`" and is read-only here.
- **[Requirement]** A set can't hold both `_f` and `_cnr`. Dropping the second one is refused, and the card shows the reason.

### 4.2 Slot commands

**[Requirement]** A **[Slot commands…]** button opens a dialog for slots 1–8. Each slot is one of:
- **from this set**: the default; the key is not written;
- **a path**: written as `slotN`. It can be a base-game texture such as a vanilla white `.dds` in a BSA;
- **clear**: written as `slotN: ""`.

**[Finding]** The schema's path rules apply:
- `textures\` is added when missing, except for `cubemap`;
- an empty string clears the slot.

**[Proposal]** A path must be relative, use no `..`, and end in `.dds`.

### 4.3 Main settings (right side)

| Group | Keys |
|---|---|
| Match | `match_diffuse` (derived, editable; §5.5), `rename` |
| Material | `specular_level`, `roughness_scale` |
| Features | `emissive`, `parallax`, `subsurface`, `hair`, `multilayer`, glint, fuzz |
| Emissive (when on) | `emissive_scale`, `emissive_color` |
| Parallax (when on) | `displacement_scale` |
| Subsurface (when on) | `subsurface_foliage`, `subsurface_color`, `subsurface_opacity` |
| Multilayer (when on) | `coat_specular_level`, `coat_roughness`, `coat_strength`, `coat_diffuse`, `coat_normal`, `coat_parallax` |
| Glint (when on) | `screen_space_scale`, `log_microfacet_density`, `microfacet_roughness`, `density_randomization` |
| Fuzz (when on) | `texture`, `color`, `weight` |

- **[Requirement]** A dependent group is shown only while its feature is on.
- **[Requirement]** Colours are edited as 0–255 RGB and written as decimals in 0–1 (§5.3).
- **[Proposal]** Sliders use TruePBR-Manager's ranges as a starting point. Typed values outside them are accepted, because the schema sets no minimum or maximum.
- **[Requirement]** Glint starts from real defaults when turned on: 1.5 / 40 / 0.015 / 2.0. **[Finding]** Fuzz colour defaults to the schema's `[0,0,0]`.

### 4.4 Advanced drawer additions

**[Requirement]** For a selected set, the Advanced drawer adds:
- `smooth_angle`;
- `vertex_colors`, `vertex_color_lum_mult`, `vertex_color_sat_mult`;
- `zbuffer_write`;
- every `lock_*` key;
- the other match modes: `match_normal`, `path_contains`, `match1`–`match8`, `nif_filter`. Choosing one turns off `match_diffuse` and leaves it out of the JSON;
- `auto_uv`, `uv_scale`, `inner_uv_scale`;
- `cubemap`, `lock_cubemap`;
- `coat_color`.

### 4.5 Project defaults

- **[Proposal]** A **Defaults** card pinned at the top of the Sets column opens the same editor. Its values form the JSON `default` block.
- Inside a set, an inherited value shows greyed as "from defaults", with a reset button.
- Match fields, `rename` and slot commands are per set only and don't appear on the Defaults card.

---

## 5. PGPatcher JSON model

### 5.1 Key coverage

- **[Requirement]** Every key of the vendored schema maps to a control or a fixed rule. A test fails if the schema contains a key the table doesn't handle (§10).
- **[Finding]** The removed keys (`pbr`, `env_mapping`, `env_map_scale`, `env_map_scale_mult`) are never written. `texture` (the old name for `match_diffuse`) is never written.
- **[Requirement]** `delete` is not exposed in M3b (§9).

### 5.2 Write rules

| Keys | Control | Written |
|---|---|---|
| `emissive`, `parallax`, `subsurface` | On/off | **Always**, `true` or `false` (owner: real-world JSON disables unused features explicitly) |
| `hair`, `multilayer` | On/off | Only when on |
| glint, fuzz | On/off | Their object is written only when on |
| `lock_*`, `vertex_colors`, `zbuffer_write` | Three-state: not set / on / off | Only when set (gap review point 4, owner 2026-10-09) |
| Numbers, colours, strings | Value, or unset | Only when set, in a set or in `default` |

### 5.3 Colours

- **[Requirement]** Each component is edited as 0–255 and written as value / 255, rounded to 4 decimals.
- **[Requirement]** `emissive_color` is always written as RGBA with alpha `1.0`. The schema requires 4 components, and PGPatcher doesn't patch it without the alpha.
- **[Requirement]** Hex paste and a colour picker are a later round (§9).

### 5.4 Mutual exclusion

**[Requirement]** Mutually exclusive options are greyed out, never silently overruled. Each greyed control shows why, e.g. "off: hair is on".

| When on | Greys out |
|---|---|
| `hair` | `multilayer`, glint, fuzz, `emissive`, `parallax`, `subsurface` |
| `multilayer` | `subsurface`, glint, fuzz |
| glint | `multilayer`, fuzz |
| fuzz | `multilayer`, glint |
| `subsurface` | `multilayer` |

- **[Requirement]** A set can't hold both `_f` and `_cnr` (§4.1). They share slot 6 (TX06).
- **[Finding]** Subsurface `_s` and coat colour share slot 7. The `subsurface` × `multilayer` exclusion covers that clash.

### 5.5 Match modes and output prefixes

- **[Requirement]** `match_diffuse` is the default match mode.
  - It is derived from the source layout: the diffuse's path relative to the opened folder, with any leading `textures\` dropped and the extension removed. For example, `<source>\landscape\rock01.png` or `<source>\textures\landscape\rock01.png` gives `landscape\rock01`.
  - The field is marked "derived" until you edit it. **[Open]** You plan to refine the derivation later; this rule is the shipped default.
- **[Finding]** PGPatcher's prefix rules (gap review, read from PGPatcher `main` on 2026-10-09). The GUI applies them exactly:
  - **`match_diffuse` / `match_normal`:** `textures\pbr\` plus the matched path. `rename` replaces only the matched part.
  - **`path_contains`:** the whole diffuse path.
  - **`match1`–`match8`:** the slot path without its extension, minus the first suffix from that slot's list that the path ends with, then `textures\` → `textures\pbr\`. `rename` replaces the whole prefix.

    | Key | Suffixes stripped, in order |
    |---|---|
    | `match1` | `_d`, `mask` |
    | `match2` | `_msn`, `_n` |
    | `match3` | `_g`, `_sk` |
    | `match4` | `_p` |
    | `match5` | `_e` |
    | `match6` | `_em`, `_envmask`, `_m`, `_rmaos` |
    | `match7` | `_cnr`, `_f`, `_i`, `_s` |
    | `match8` | `_b`, `_bl`, `_flow` |

- **[Requirement]** These pitfalls are warnings, never corrected automatically (owner 2026-10-09):
  - `match8` doesn't strip `_s`;
  - a `rename` on a `matchN` must be a full path.
- **[Proposal]** `textures\pbr` is never doubled. Each path component is sanitised to the characters Windows allows, and the prefix is lower-cased only for comparisons, never for writing.
- **[Requirement]** On export, files always get sdc's canonical suffixes under the prefix (`.dds`, `_n`, `_rmaos`, `_p`, `_g`, `_s`, `_cnr`, `_f`).

### 5.6 Document shape and validation

- **[Requirement]** The object form: `{ "$schema": "<PGPatcher schema URL>", "default": {…}, "entries": [ … ] }`.
  - Entries are in set order; keys are in schema order; indentation is 2 spaces; UTF-8 without a BOM.
  - Paths use single backslashes, escaped as JSON requires.
- **[Proposal]** Before writing, the document is validated against the vendored schema with the `jsonschema` crate.
  - That crate is a new dependency. Downloading it needs the owner's approval at plan time.
  - A validation failure blocks export. It is a safety net that should never fire.

---

## 6. Export

### 6.1 CS target

**[Requirement]** On CS, Export:
1. **Saves** `sdc.toml` and `sdc-project.toml` (the M3a rule, now covering both files).
2. **Plans** with `set_edits` and `set_outputs`. A **PBR set** is a set whose diffuse resolves to a PBR sub-type (`pbr_albedo`). Its members are written to `<export>\textures\pbr\<prefix><suffix>.dds`. Non-PBR sets export as in M3a.
3. **Checks** (§6.3), then **encodes**. A shared output is written once, by its owner (§3.3).
4. **Writes** `<export>\PBRNIFPatcher\<name>.json` atomically.
   - `<name>` defaults to the source folder's name and can be edited in the header.
   - One file per project.
   - Every PBR set is one entry, in set order.
   - **[Requirement]** A set that didn't fully export (failed, skipped as undecided, cancelled) is left out of the JSON. The report lists it as "not in JSON: `<reason>`", so PGPatcher never points at missing files.

### 6.2 Vanilla and CPM

**[Requirement]** No JSON is written. Outputs mirror the source folder as in M3a. A borrowed slot becomes a copy at the borrowing set's path (§3.3).

### 6.3 Checks

**Blocks export.** Each listed item is clickable and selects its set.
1. A feature is on but its texture is missing:

   | Feature | Missing texture |
   |---|---|
   | `emissive` | `_g` |
   | `parallax` | `_p` |
   | `subsurface` | `_s` |
   | `coat_normal` | `_cnr` |
   | fuzz `texture` | `_f` |

   **[Requirement]** There is no lock quick fix: a lock on a mesh without that texture fails to patch, or looks wrong in game.
2. Slot clashes (§5.4).
3. Two sets writing the same file.
4. An empty or invalid prefix or slot path.
5. JSON schema validation failure.

**Asks "Proceed anyway / Cancel"** (M3a N3):
- undecided textures;
- the `matchN` pitfalls.

### 6.4 Overwriting the JSON

- **[Proposal]** `sdc-project.toml` records the SHA-256 of the JSON sdc last wrote.
- If the file on disk differs, Export asks "Overwrite edited JSON / Cancel".
- Keeping hand edits is the import and merge work (§9).

---

## 7. Project file and undo

### 7.1 `sdc-project.toml`

- **[Requirement]** It lives next to `sdc.toml` in the opened folder and is loaded automatically. One folder is one project, and paths are relative.
- **[Proposal]** Shape:

  ```toml
  [project]
  json_name = "rock-pack"
  last_json_sha256 = "…"

  [defaults]            # the JSON default block (only keys you set)
  specular_level = 0.04

  [[sets]]              # in your order; an untouched automatic set has no entry
  key = "landscape/rock02"
  add = ["stone/rock02_rough.png"]
  remove = []
  borrow = { slot6 = "landscape/rock01" }
  match_diffuse = "landscape\\rock02"
  parallax = true
  displacement_scale = 0.4
  ```

- **[Requirement]** The M3a `sdc.toml` rules apply:
  - written only on Save (Ctrl+S) or Export;
  - one ● unsaved marker covers both files;
  - format-preserving through `toml_edit`;
  - atomic saves;
  - detection of edits made outside the GUI;
  - read-only mode, with a reason, when the file can't be parsed.
- **[Proposal]** A `[[sets]]` entry whose files no longer exist shows as a red "missing" set. It is never silently dropped.

### 7.2 Undo and redo

- **[Requirement]** Ctrl+Z / Ctrl+Y over a single history of every edit:
  - set edits;
  - JSON fields;
  - per-texture `[files]` settings from M3a;
  - Mode.
- **[Requirement]** The history is cleared when you open another folder. Export and Save are not steps.
- **[Proposal]** Each command stores the values before and after the change. A slider drag, or a run of typing in one field, is merged into one step.
- **[Requirement]** Undoing a duplicate follows §3.4.

---

## 8. Fix first: the stale `after_export`

**[Finding]** In M3a (`crates/gui/src/model/mod.rs`: `run_after_export`, `resolve_export_running`, `request_close`, `request_open`), a second close or open request, made while a cancelled export is stopping, can leave a stale pending action. A later export can then close the window or switch folders unasked.

**[Requirement]** The fix:
- the pending action is cleared in `open_folder` and on "Keep running";
- a new close or open request replaces the pending one; it never stacks.

The regression test: request close, cancel, request open, let the export finish. Exactly one action runs, the latest one, and a later export triggers nothing.

---

## 9. Later rounds (owner, 2026-10-10)

- **M3b-2:** importing existing PBR mods (`PBRNIFPatcher/*.json` plus `textures\pbr\**`). The owner may not need it; reopening your own work is covered by the project file.
- **M3b-2:** merging into an existing JSON while keeping hand edits.
- **M3b-2:** landscape EDID JSON (`PBRTextureSets/<edid>.json`, camelCase keys).
- **M3b-2:** rule-table and suffix-alias editors; a switch between source `_m` (metallic) and mod `_m` (env mask).
- **Later:** NIF reading (shared or standalone diffuse resolved from the mod's own meshes).
- **Later:** the `delete` key. It removes tri-shapes from meshes and usually needs a match on a different alias than the set's own, so it doesn't fit a per-set editor.
- **Later:** hex colour paste and a colour-picker square.
- **Later:** refining how `match_diffuse` is derived (§5.5).

---

## 10. Testing and exit

- **Engine:**
  - golden cases where moving a raw roughness map between sets changes which set packs it;
  - a new set made from loose files; a removed member; set order;
  - a CS PBR set's prefix outputs, including packed recipe outputs;
  - a stale edit giving `SET-EDIT-STALE`;
  - two sets writing one file giving `IN-DUPLICATE`.
  - With `set_edits` and `set_outputs` empty, every existing golden case and `gui_repro` stay byte-identical.
- **JSON (unit tests):**
  - the key-coverage test (§5.1);
  - every write rule in §5.2;
  - colour conversion and emissive alpha;
  - every row of the mutual-exclusion table;
  - every `matchN` suffix row, including `facemask` → `face` and `match8` keeping `_s`;
  - `textures\pbr` never doubled.
  - Every written document validates against the vendored schema.
  - Golden JSON files for: a plain set, a borrowed slot, defaults inheritance, each match mode.
- **GUI (kittest):**
  - drag and drop: join, replace, move, share, duplicate, new set, reorder;
  - greying from mutual exclusion;
  - each export block, and the overwrite prompt;
  - a project-file round trip, including surviving an outside edit;
  - undo and redo for each command kind, including the duplicate undo rule;
  - the §8 regression test.
- **Full flow:** open a fixture folder, build sets, export. The `.dds` files and the `.json` match the expected output, and a second export changes nothing.
- **Owner exit check:**
  1. Use a small CS test mod: 3–5 PBR sets, one shared `_rmaos`, one set using a slot command.
  2. Run PGPatcher on it and confirm in game that the meshes are patched as expected.
  3. Do a usability pass on the set editor.
