# Per-texture packing fills, the `_rfaos` choice and Advanced skin sub-types (owner, 2026-10-11) Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development. Steps use checkbox (`- [ ]`) syntax.

**Goal:** Packing fill values move off the global Advanced panel and onto each packed output's card. A set can be told to build `_rfaos` (the skin shader) even without a fuzz source, with the fuzz channel taken from the fuzz fill. Opacity fill goes away. Advanced skin gets its own sub-types: `skin_albedo` (UNORM), `skin_sk` and `skin_wet`. Picking RFAOS switches a set to them.

**Architecture:**
- **Engine:** an explicit per-output recipe choice, plus a `Fuzz` slot that can be filled or defaulted (`crates/engine/src/pack/`, `config.rs`).
- **GUI:** the fill rows are drawn on the set editor's packed-output card (`crates/gui/src/ui/set_editor.rs`). They are stored on that output's own `[files."<packed key>"]` entry; the packed key is the first source's name, per M3b final fix I1.

**Tech Stack:** Rust; egui 0.36.2.

**Spec:** the owner's chat decisions of 2026-10-11, reproduced verbatim under each task. Background: spec §6.4 (channel packing), DECISIONS Note M (M1–M13), and Note P (P6 slots).

## Global Constraints

- **Gates:** `cargo fmt --all && cargo clippy --workspace --all-targets -- -D warnings && cargo test --workspace`, run unpiped before every commit.
- **Golden and `gui_repro`:** both stay green. Any expectation that changes is named in the report.
- **Schema:** if a config field changes, regenerate with `cargo run -p sdc -- presets schema all --out schema`.
- **Commits:** commit only explicit paths, with a `Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>` trailer. Do not push. Never run `python -`.
- **Off-limits:** do not touch `crates/nif`, `crates/vfs`, `crates/modscan` or DECISIONS Note Q. They belong to the NIF team, in worktree `F:/ai/dds/converter-nif`.
- **Decisions:** record them in DECISIONS Note R (M4 and later, 2026-10-11).

## Review Focus

1. **Existing projects are unchanged.** A set with a fuzz source still builds `_rfaos`. A set without one still builds `_rmaos`. Old global `packing.fill.*` values still apply as a fallback.
2. **A fuzz fill never appears on an `_rmaos` card,** and choosing `_rmaos` never writes a fuzz fill.
3. **Per-output fills don't leak.** A fill on one set's packed output doesn't change another set's output.

---

### Task 1: Engine — an explicit `_rfaos` choice, and a fillable fuzz channel

**Owner, verbatim:** "no, allow building a rfaos without fuzz and using fuzz fill. That would just invite confusion if it was present on rmaos card. Rfaos = skin = unorm diffuse, very different shader". Also: "keep fuzz fill for rfaos".

**Rules:**
- **New field `packing.recipe`:** values `auto` (default), `rmaos` and `rfaos`. It only matters on CS.
  - `auto`: today's behaviour. `_rfaos` when a fuzz source exists, otherwise `_rmaos`.
  - `rfaos`: build `_rfaos` even with no fuzz source. Fill the G (fuzz) channel from `packing.fill.fuzz`, else default it to **0** (no fuzz). Report it like the other defaulted channels (`PACK-DEFAULTED`). The metallic source, if any, goes unused, with the existing "_rfaos has no metallic channel" reason.
  - `rmaos`: build `_rmaos` even when a fuzz source exists. The fuzz source goes unused, with the reason "the set is set to _rmaos (packing.recipe)".
- **Where it is set:** the field is resolved the way other packing fields are. A per-output `[files."<packed key>"] packing.recipe` overrides project, preset and default. Find how `PackingSpec` is resolved for a virtual packed input, and make the recipe choice read from that resolved spec.
  - If the choice must be known before the virtual key exists (a chicken-and-egg problem, because the key may depend on the recipe), use the **set's diffuse/first-source** entry instead, and document which in Note R.
- **Unchanged:** `PACK-DEFAULTED`, `fill_for`, Note M5 inversion, cpm and vanilla.
- **Opacity:** `RawMap::Opacity` stays a recognised raw map (still "no recipe packs opacity yet"). Only the GUI stops offering an opacity fill.

**Tests** (engine; follow `crates/engine/tests/packing.rs` and `pack_recipes.rs`):
- (a) `packing.recipe = "rfaos"` with roughness and AO only (no fuzz source): the output is `_rfaos`, and G is 0.
- (b) Same as (a), plus `packing.fill.fuzz = 128`: G is 128.
- (c) `packing.recipe = "rmaos"` with a fuzz source: the output is `_rmaos`, and the fuzz input is listed unused with the new reason.
- (d) `auto` matches today, both with and without a fuzz source.
- (e) Two sets in one folder, only one of which has `packing.recipe = "rfaos"` on its packed entry: only that set builds `_rfaos`.

**DECISIONS:**
- Note R gets **R-fills-1**: `packing.recipe` (`auto`/`rmaos`/`rfaos`); `_rfaos` without fuzz defaults G to 0; `_rfaos` is the skin shader.
- The skin sub-types are covered in Task 1b.

Commit: `feat(engine): packing.recipe lets a set build _rfaos without a fuzz source (fuzz from fill, default 0) or force _rmaos (owner)`.

### Task 1b: Engine — Advanced skin sub-types (`skin_albedo`, `skin_sk`, `skin_wet`)

**Owner, verbatim:**
- "Add a separate section to hold Advanced skin maps, rfaos, sk(grayscale vs vanilla), wet(normal w/binary mask or grayscale height)"
- "advanced skin uses unorm as it's albedo, that's why I said it should be it's own thing"
- "_wet is either a normal with a binary mask or a grayscale height map, advanced skin supports both map types with no distinction"
- Earlier: "_sk maps for advanced skin use only the red channel"

The design was approved in chat on 2026-10-11 ("go").

**Rules** (`crates/engine/src/config.rs` `Subtype`, `category()`, profiles/presets, `policy.rs` availability, `suggest.rs`, `group.rs` suffixes):
- **New sub-types, available on `cs` only.** On `cpm`/`vanilla` they are unavailable, with a reason, like the other PBR-only sub-types:
  - `skin_albedo`: Category Color. BC7, **UNORM** tag (never sRGB, so do not add it to `is_pbr_color`), alpha like `diffuse` (`alpha_from_mesh` = true).
  - `skin_sk`: Category Grayscale. Single channel, red only (luma is not used; take R). BC4 UNORM.
  - `skin_wet`: one sub-type with two content forms, picked by analysis:
    - **Grey content** (R≈G≈B, using the same "grey" test as `VAL-M-GRAYSCALE` / channel analysis): a height map, BC4 UNORM, plain box mips.
    - **Otherwise:** a tangent-space normal with a binary mask in alpha. BC7 UNORM, RGB renormalised like `normal`, alpha policy `data`.
    - The form is overridable through the existing `format` field (BC4 vs BC7).
    - Add an Info code `SKIN-WET-FORM` that states which form was chosen and why.
- **Suffixes.**
  - Add `wet` to the §6.1 suffix list. Keep longest-first ordering, and check that `_wet` doesn't collide with any vocabulary term.
  - `_sk` already exists.
- **Suggestions.** A set is **advanced skin** when it contains an `_rfaos` source or member, or a raw input with a `packing.recipe = "rfaos"` resolution. Task 1's field is resolved first, so read the resolved recipe.
  - In an advanced-skin set, suggest:
    - the diffuse as `skin_albedo` (High);
    - `_sk` as `skin_sk` (High);
    - `_wet` as `skin_wet` (High).
  - Outside one:
    - `_sk` keeps today's suggestion (`backlight_tint`, Low);
    - `_wet` suggests `skin_wet` (Low), because only skin uses it.
- **Slots and PGPatcher.** If the M3b set model's `SlotKind` mapping (`crates/gui/src/model/sets.rs`) maps sub-types to slots, add:
  - `skin_albedo` → Diffuse
  - `skin_sk` → slot 3 (glow/skin, P6 index 3)
  - `skin_wet` → none
  - `skin_wet` stays unslotted in the set editor and the PGPatcher JSON. Owner, 2026-10-11: "it's currently patched at runtime, I think it is forced into slot 5 or 7 using a non standard method". sdc only converts it. Record that in Note R.
- **DECISIONS Note R:** add R-skin-1 with the three sub-types, their formats, the detection rule and the open slot question.
- **Schema:** regenerate it, since `Subtype` changed. Update the codes count test.

**Tests:**
- (a) A CS set with `x.png`, `x_rfaos.png`, `x_sk.png` and `x_wet.png` suggests `skin_albedo`, `skin_sk` and `skin_wet`.
- (b) The same files without `_rfaos` suggest the vanilla `_sk`, and a low-confidence `skin_wet`.
- (c) A grey `_wet` encodes to BC4, a colour `_wet` encodes to BC7 with alpha kept, and `SKIN-WET-FORM` says which.
- (d) `skin_albedo` writes a UNORM DX10 format (`BC7_UNORM`, not `_SRGB`).
- (e) `skin_*` is unavailable on vanilla and cpm, with a reason.
- (f) Golden and `gui_repro` are unchanged.

Commit: `feat(engine): Advanced skin sub-types skin_albedo (UNORM), skin_sk (R only, BC4), skin_wet (normal+mask or height) (owner)`.

### Task 2: GUI — fills and the recipe choice on the packed card; Advanced fill panel removed

**Owner, verbatim:**
- "packing fill values needs to be per texture, not a global value … take them out of advanced and put them in rmaos / rfaos / m … cards"
- "i assume specular is for non pbr normals" (answered: it is the `_rmaos`/`_rfaos` A channel, the PBR specular level)
- "keep fuzz fill for rfaos"
- "Rfaos = skin = unorm diffuse"

**Rules** (set editor slot cards, `crates/gui/src/ui/set_editor.rs`):
- **Recipe selector.** The RMAOS-slot card (P6 slot 6) holds the packed output. On CS it gets a recipe selector: `RMAOS` / `RFAOS (skin)`. The selector writes `packing.recipe` on the packed output's entry (or wherever Task 1 put it). `auto` shows as whichever recipe the engine actually chose, with "(auto)" after it.
- **Fill rows.** They appear only for channels that have no source. Each is a 0–255 DragValue with a reset that removes the key, stored on that output's own entry:
  - `_rmaos`: roughness, metallic, AO, specular.
  - `_rfaos`: roughness, **fuzz**, AO, specular. **No metallic row.**
  - cpm `_m`: roughness (or glossiness, whichever the recipe uses), metallic, height.
  - vanilla `_m`: **no fill rows** (owner: "vanilla m doesn't need a fill. It is a one channel texture"). It is built only from a metallic or specular source, and has nothing to fill.
  - Show no fuzz row on `_rmaos`, and no opacity row anywhere.
  - Show the fill row for a channel with a source as read-only text: "from `<file>`".
- **RFAOS switches the set to Advanced skin** (owner-approved design, 2026-10-11; replaces the earlier "diffuse tag = unorm" idea). When the user picks `RFAOS (skin)`, the same undo step:
  - switches the set's diffuse to `skin_albedo`;
  - switches its `_sk` member, if any, to `skin_sk`.

  Only **inferred** members change, with the same rule as polish Task 2's PBR follow: the sub-type provenance is `Suggestion`. Track them in an `sdc-project.toml` list (e.g. `[project] auto_skin`), the same way as `auto_pbr`.
  - Switching back to RMAOS reverts only the listed members that still hold the auto value.
  - P8: if `sdc-project.toml` is read-only, change only the recipe.
  - Reuse `model/pbr_follow.rs`'s helpers where they fit; don't duplicate them.
- **Dropdown grouping** (`ui::settings_panel::subtype_groups`):
  - Add a group `Advanced skin (Community Shaders)` right after the PBR group. It holds `skin_albedo`, `rfaos` (moved out of the PBR group), `skin_sk` and `skin_wet`.
  - Move `ui_sky` out of "Colour maps" into a last group, `Other`.
  - Update the completeness test. Every available sub-type still appears exactly once.
- **Global fills.** The Advanced drawer's global packing fill panel (`crates/gui/src/ui/advanced.rs`) is removed.
  - Existing project-level `defaults.packing.fill.*` keys still load and still apply as a fallback.
  - When any such key exists, the Advanced drawer shows one read-only line: "Project-wide fills (legacy): …", plus a "Clear" button that removes them.
- **Undo and P8.** Every edit is one undo step. Fills and recipe edit `sdc.toml`, so they work while `sdc-project.toml` is read-only. They are disabled, with the reason on hover, when `sdc.toml` is read-only.

**Tests** (kittest and model):
- (a) An `_rmaos` card with no AO source shows an AO fill row. Setting it writes the per-output key, and the planned output's AO channel provenance becomes Fill.
- (b) Switching to RFAOS shows a fuzz row and hides metallic, switches an inferred diffuse to `skin_albedo` and an inferred `_sk` to `skin_sk`, and leaves user-set ones alone. One undo restores everything.
- (f) The dropdown shows the `Advanced skin (Community Shaders)` group right after PBR, holding `rfaos`, and `ui_sky` comes last, under `Other`.
- (c) An `_rmaos` card never shows a fuzz row.
- (d) A legacy project-wide fill is listed read-only in Advanced, still applies, and "Clear" removes it.
- (e) Two sets with different AO fills produce different AO channels.

Commit: `feat(gui): packing fills and the RMAOS/RFAOS choice on the packed card; skin implies UNORM diffuse; global fill panel retired (owner)`.

---

### Owner amendment (2026-10-11): every Advanced skin sub-type is prefixed `skin_`

**Owner, verbatim:** "ensure all advanced skin matierials are prefixed like pbr is, so it's not confusing".

This applies to Tasks 1b and 2, and overrides their text where they conflict.
- **Rename the sub-type `rfaos` to `skin_rfaos`.**
  - In config, keep `rfaos` as a deserialise alias, so existing `sdc.toml` files and presets still load.
  - Write the new id on save.
  - Mark the old id as deprecated in the schema.
  - Every user-facing label and the dropdown show `skin_rfaos`.
  - The packed output's file suffix stays `_rfaos`, because that is the PGPatcher/CS naming.
  - The `packing.recipe` value stays `rfaos`, because it names the output recipe, not a sub-type.
- **The Advanced skin group** therefore holds `skin_albedo`, `skin_rfaos`, `skin_sk` and `skin_wet`. All four are prefixed.
- **Display labels** follow the PBR style. Use whatever `subtype_label` does for `pbr_*`, e.g. "Skin albedo", "Skin RFAOS", "Skin _sk (red)", "Skin wet".
- **Tests:**
  - an `sdc.toml` with `subtype = "rfaos"` loads as `skin_rfaos`;
  - golden and `gui_repro` are unchanged. Packed output names keep `_rfaos`.
  - If any golden expectation prints the sub-type id, update only that expectation and name it in the report.
- **Docs:** list the rename in DECISIONS Note R.
