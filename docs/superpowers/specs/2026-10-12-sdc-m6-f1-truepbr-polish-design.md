# sdc M6 Track F1 — TruePBR panel polish: design

Owner-approved in chat on 2026-10-12. Base: `main` 735cb79. Track F was split into F1 (this), F2 (RNM) and F3 (bulk editing).

## 1. Intent

**The owner asked for:**
- PBR settings must not go negative, whether dragged or typed.
- Partial-path matching is not encouraged, so drop it. Use exact slot matching (match1–match8) and write the extension on JSON export.
- Put toggles on the diffuse and normal slot cards. The matching logic is OR, so it goes in its own "Matching" header in the TruePBR box, separate from the visual settings.
- `path_contains` stays available under advanced.
- Whatever is left of TruePBR "Advanced" moves into a collapsed "Advanced TruePBR settings" header inside the TruePBR box.
- The set card shows only the slots its shader can use, ordered 1–8.
- Old projects convert match_diffuse/match_normal to match1/match2.

**Success:**
- No PBR number can be set below its floor.
- A new set exports `match1` (the exact diffuse path with `.dds`) by default.
- The Matching header explains that any row hit applies the entry.
- Opening an old project rewrites partial matches to exact ones and says so.
- The set card shows only usable slots, in slot order.

**Out of scope:**
- RNM (F2);
- bulk editing (F3);
- changing what PGPatcher does.

## 2. PBR value limits

- **No documented CS or PGPatcher limits exist.** The CS True PBR wiki only advises specular 0.04, or 0.08 with a specular map. The owner's 0.04 roughness floor is unconfirmed, so it is not used.
- Floors and ceilings are per key, in `crates/gui/src/pgp/schema.rs` (a new optional range on `K::Number`):
  - **Roughness values** (`coat_roughness`, `glint.microfacet_roughness`, any key whose name contains `roughness` and that is not a scale): min 0, no max.
  - **Multipliers and levels** (`roughness_scale`, `specular_level`, `coat_specular_level`, `displacement_scale`, `subsurface`-type scales): min 0, no max.
  - **Strengths, weights, opacities, ratios** (`coat_strength`, `fuzz.weight`, `subsurface_opacity`, other 0–1 factors): 0–1.
  - The plan lists every Number key with its class. A key that fits none of these classes gets min 0, and the plan says so.
- **GUI** (`crates/gui/src/ui/pgp_fields.rs` Number arm): `DragValue::range(min..=max)`. If a typed value falls outside the range, it is clamped, and a small note shows for that frame: "clamped to ≥ 0", or "to 0–1".
- **JSON writer** (`crates/gui/src/pgp/writer.rs`): clamps every ranged key the same way, so a hand-edited `sdc-project.toml` value cannot export out of range. `pgpsim::check` gains the same ranges as warnings.

## 3. Matching

### 3.1 Model

- **Supported for new edits:** `match1`–`match8`, the exact slot paths. On JSON export each value gets a `.dds` extension when it has none. Values are stored without the extension.
- **No longer offered:** `match_diffuse` and `match_normal`, the partial ends-with matches. The writer stops emitting them.
- **`path_contains`** (substring) stays as an advanced tool (§4).
- **`nif_filter`** stays a separate filter. It is applied on top of whichever match hit (AND).
- **PGPatcher semantics** (pgpsim `matching.rs:22-55`): an entry applies when ANY of its match fields hits. The first hit, in PGPatcher's order, supplies the matched path.

### 3.2 GUI

- **Slot card toggles:**
  - The **diffuse** card (slot 1) and the **normal** card (slot 2) each get a small "Match on this" toggle on the card.
  - Turning one on writes `match1` / `match2` with that slot's exact output-relative path, derived the way today's match value is derived but without stripping suffixes.
  - The diffuse toggle is on by default for a new set. The normal toggle is off by default.
- **A "Matching" header inside the TruePBR box,** separate from the visual-settings headers:
  - one row per active match: the slot name, the exact path (editable), and × to remove;
  - a **+** button with a dropdown of slots 3–8 that have a texture in the set. It adds that slot's exact path as `matchN`;
  - one line of help: "The entry applies when any of these matches."
  - The slot 1 and 2 rows mirror the card toggles: removing the row turns the toggle off, and the other way round.
- **No match:** a set with no match active shows a warning, "this entry matches nothing". The export still runs.

### 3.3 Old projects

- On open, a set entry with `match_diffuse` becomes `match1` and one with `match_normal` becomes `match2`. Each takes the set's exact slot path, taken from the set's own members. The partial value is not reused.
- A one-line note appears in the set's Matching header: "converted from match_diffuse (partial) to an exact match". The note stays until the next save. The project file is rewritten on the next save, not silently at open.
- **Read-only projects:** the conversion is shown, but nothing is written.
- `rename`, which depended on the matched part, is re-checked against the exact path. If it can no longer apply, the set shows a warning and keeps the value.

## 4. TruePBR box layout

- The TruePBR box has three headers:
  1. the visual settings, as today;
  2. **Matching** (§3.2);
  3. **Advanced TruePBR settings**, collapsed by default.
- Every key that is in TruePBR's "Advanced" group today, and that is not one of the match fields, moves under **Advanced TruePBR settings**. That includes `path_contains`, `nif_filter`, locks and the other advanced keys. The old "Advanced" group stops showing TruePBR keys.

## 5. Set card slot row

- The thumbnail row shows only the slots that the set's shader can use. The shader is the resolved pick from Track A (`ShaderPick` → `ShaderSpec` roles → slots), and the row is ordered by PGPatcher slot number 1–8.
- **Empty usable slot:** shown as an empty box, as today.
- **Filled slot the shader cannot use:** stays visible at its slot position with a warning outline, and its hover text names the problem. Nothing disappears silently.
- **No shader pick yet:** the row shows the slots that are filled, in order.

## 6. Tests

- Each Number key class clamps on drag and on typed entry. The writer clamps a hand-edited out-of-range value. `pgpsim::check` warns.
- **New set:** `match1` is exported with `.dds`, and neither `match_diffuse` nor `match_normal` is written.
- **Toggles:** the diffuse and normal toggles add and remove `match1`/`match2`. The + dropdown adds `match3`–`match8`, but only for filled slots.
- **Old project:** `match_diffuse` → `match1` and `match_normal` → `match2`, with the note shown. A read-only project shows the conversion but writes nothing. A conflicting `rename` raises a warning.
- **Layout:** `path_contains` and `nif_filter` render under "Advanced TruePBR settings", collapsed by default.
- **Slot row:** a PBR set shows only PBR slots in 1–8 order. A filled unusable slot shows its warning outline.

## 7. Review focus

1. A set whose diffuse output path differs from its mesh's slot path, for example after a set rename, must still match. The exact path must be the path PGPatcher sees in the mesh after patching, i.e. the `textures\pbr\` mapping (pgpsim `strip_pbr`). The plan must pin which path `match1` holds, and test it against pgpsim.
2. The `.dds` extension must be added exactly once, including for values a user pastes with the extension already on.
3. An old project with both `match_diffuse` and `match1` must not get a duplicate `match1`.
4. A clamped field must not mark the project dirty when the value did not actually change.
5. The slot row must not hide a filled slot on a set whose shader fell back (`SHADER-UNAVAILABLE`).
