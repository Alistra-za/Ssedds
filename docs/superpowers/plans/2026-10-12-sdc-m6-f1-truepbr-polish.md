# sdc M6 Track F1 — TruePBR panel polish Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** PBR numbers cannot go below their floor, or above 1 for the 0–1 factors. A set matches on exact slot paths (`match1`–`match8`, `match1` on by default). The diffuse and normal cards carry "Match on this" toggles, and a "Matching" header lists the matches. The remaining TruePBR advanced keys move under a collapsed "Advanced TruePBR settings" header. The slot cards show only the slots the set's shader can use, in slot order 1–8.

**Architecture:**
- **Ranges.** `KeyKind::Number` becomes `KeyKind::Number(NumRange)`. `NumRange` is `Min0` (0 or more) or `Unit` (0–1).
  - The writer clamps every number it writes.
  - The Number control gets `DragValue::range`, shows a note when a typed value is clamped, and never edits the project when the value did not change.
  - `pgpsim::range_warnings` repeats the ranges as warnings. A GUI test checks that the two tables agree.
- **Matching.** `prefix.rs` drops `MatchMode`/`match_mode`/`check_match_value`. It gains:
  - `active_matches`: `match1` is the derived diffuse path while the set has no `match1` key, and `match1 = ""` means off;
  - `match_value`: no leading `textures\`, and `.dds` exactly once;
  - `stored_match`, `match_toggle`, `slot_match_path`, `matches_nothing`.
  - The writer emits only `matchN`, never `match_diffuse`/`match_normal`.
  - `prefix_of` follows the lowest active `matchN`, as PGPatcher does.
  - A set that matches nothing asks before export and is left out of the JSON.
- **GUI.**
  - `pgp_fields::panel` draws the visual groups, then **Matching**, then a collapsed **Advanced TruePBR settings** header.
  - The Advanced drawer loses its "PGPatcher (advanced)" section.
  - The set editor's diffuse and normal cards get "Match on this". Its cards come from `sets::slot_row`: the shader's usable slots, plus filled slots it cannot use, which get an amber outline.

**Tech Stack:** Rust 2021 workspace. The packages touched are `sdc-gui` (lib `sdc_gui`) and `pgpsim`. Also used: serde_json, egui 0.36.2 and egui_kittest 0.36.2.

**Spec:** `docs/superpowers/specs/2026-10-12-sdc-m6-f1-truepbr-polish-design.md`. It is owner-approved and binding.

Background:
- `docs/DECISIONS.md` Note P (M3b PGPatcher JSON: P6 slot numbers, P7 match edits re-plan, P13/P26 path checks) and Note S (shaders).
- PGPatcher's match rules, as pgpsim simulates them: `crates/pgpsim/src/matching.rs:22-55`.
- The vendored schema: `crates/gui/assets/pbr.schema.json`. Its `matchSlot` and `hasMatcher` definitions matter here.

## Global Constraints

- **Base.** `main` at 3489466. Line numbers below were read at 3489466. If one has drifted, find the code by the quoted text, not by the number.
- **Branch.** The execution branch is `feat/m6-truepbr-polish`, created by the controller from 3489466. Implementers do not create, switch or delete branches.
- **Gates.** Run each one unpiped before every commit. All must pass:
  - `cargo fmt --all --check`
  - `cargo clippy --workspace --all-targets --offline -- -D warnings`
  - `cargo clippy --workspace --offline -- -D warnings`
  - `cargo test --workspace --offline`
- **Commits.**
  - Commit only explicit paths (`git add <path> …`, never `-A` or `.`).
  - End every message with `Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>`.
  - Never push. This is local-only work.
- **Shell.** Never run `python -` or `python3 -` (stdin heredocs). This plan needs no Python.
- **Off-limits.**
  - Nothing is ever written under `F:/ai/dds/bench`.
  - `crates/nif`, `crates/vfs`, `crates/modscan` and `crates/render3d` are not edited.
  - **`crates/pgpsim` is edited, unlike Track E**, because spec §2 says "`pgpsim::check` gains the same ranges as warnings". Only `crates/pgpsim/src/check.rs` (a new function and its unit test) and `crates/pgpsim/src/lib.rs` (one `pub use`) change. No simulation behaviour changes.
- **Engine and CLI unchanged.** No file under `crates/engine`, `crates/cli`, `schema/` or `crates/encoders-sys` changes.
- **Counts.** `Code::ALL.len()` stays **75**. There are no new codes.
- **Version: no bump (ruling V9).**
  - The workspace stays **0.6.0** (`Cargo.toml:7`).
  - The engine does not change, and no texture byte changes.
  - The manifest up-to-date check (`crates/engine/src/api.rs:559-565`) compares `engine_version` (`CARGO_PKG_VERSION`) and `profile_sha256`. A bump would therefore re-encode every texture of every user for a GUI/JSON-only change.
  - The JSON change is visible in the file itself and is recorded in DECISIONS Note V and `docs/gui.md`.
- **Old projects.** There is no migration (spec §3.3). A stored `match_diffuse`/`match_normal` is ignored and never written.
- **Decisions.** Task 7 appends `## Note V — M6 Track F1: TruePBR panel polish (plan rulings, 2026-10-12)` at the end of `docs/DECISIONS.md`, after Note U's last bullet (U10). It has one bullet per ruling, `V1` … `V9`.

## Plan rulings

These settle the questions the spec left to the plan. Each one becomes a Note V bullet in Task 7.

- **V1 What `match1`/`match2` hold (spec Review Focus 1).**
  - PGPatcher compares `matchN` **exactly** with the mesh's raw slot string. It lower-cases both and prepends `textures\` to the value unless the value starts with exactly `textures\` (`pgpsim/src/matching.rs:40-52`).
  - `strip_pbr` applies only to `match_diffuse`/`match_normal`/`path_contains`, never to `matchN`.
  - PGPatcher patches the *unpatched* mesh (modscan's `MatchRow::load_copy_id` reads a derived winner's source). So the value is the path the unpatched mesh carries.
  - The value is the set's **derived diffuse path**, `derive_match_diffuse(dir_of(source), mirrored_stem(source))`. That is the source's folder without `textures/` and `pbr/`, plus its stem with no colour term. The path is relative to `textures\` and is stored without `.dds`.
  - `match1` = derived, with any `_d` or `mask` suffix kept. `d` is not an exporter vocabulary term (`engine/src/group.rs`), so `mirrored_stem` keeps `_d`.
  - Slot N > 1 = derived with a trailing `_d` cut (only `_d` pairs with `_n` in vanilla naming; `facemask` goes with `facemask_n`), then `_` and the slot's canonical suffix (`SlotKind::suffix()`), e.g. `match2` = `landscape\rock01_n`.
  - On a `facemask` mesh, PGPatcher's prefix differs by which match hits: `…\face` from match1, `…\facemask` from match2. sdc stays consistent because `prefix_of` follows the lowest active match, which is also PGPatcher's first hit.
  - It comes from the source, never from the set id or the output, so a set rename does not change it.
  - PGPatcher's prefix for a hit is `add_pbr(tex_base_slot(slot, N))` = `textures\pbr\` + the value with its extension and slot suffix cut. sdc's `prefix_of` computes the same, so the files land where PGPatcher looks.
  - Proved in Task 3: `slot_matches_hit_the_unpatched_mesh_and_land_on_the_sdc_prefix` (gui `tests/pgp.rs`, through `pgpsim::prepare`/`shape_matches`/`matched_path`) and `a_renamed_set_still_matches_its_source_path`.
  - A mesh that already points at `textures\pbr\…` is **not** hit, and the test pins that.
- **V2 How matches are stored and written.**
  - `match1` absent: the derived path (on by default; an old project gets this too).
  - `match1 = ""`: the diffuse toggle is off.
  - `match2`–`match8`: present means on.
  - Stored values have no `.dds`: `stored_match` cuts one trailing `.dds`, case-insensitively.
  - The writer writes `match_value(v)`:
    - a leading `textures\` is cut, case-insensitively, because PGPatcher's prepend check is case-sensitive and `Textures\…` would never match;
    - `.dds` is added unless the value already ends in `.dds` (any case). So `rock.v2` gets `rock.v2.dds` (spec Review Focus 2).
  - `match_diffuse` and `match_normal` move from `KEYS` to `NOT_EXPOSED`. The writer never emits them.
- **V3 The output prefix.**
  - `prefix_of` uses the lowest active `matchN`: `textures\pbr\` + `match_value` without `.dds`, with that slot's first suffix cut. This is PGPatcher's first hit for a mesh that carries all the paths.
  - Every active value is checked with `check_json_path`.
  - With no slot match (`path_contains` only, or nothing), the prefix is `textures\pbr\` + the derived path, as for today's `path_contains`.
  - With a slot match, `rename` is the **whole** prefix, as today's `matchN` rule says. The Rename row's hint says so (spec gap 3).
- **V4 No match.** A set with no active `matchN` and no `path_contains`:
  - shows "this entry matches nothing" in the Matching header;
  - asks before export ("this entry matches nothing, so it is left out of the PGPatcher JSON");
  - is left out of `json_entries`.

  The vendored schema's `hasMatcher` refuses a matcherless entry, and `export_blocks` would otherwise block. The textures still export (spec gap 1).
- **V5 Number ranges** (spec §2). There are 19 `Number` keys:

  | Class | Range | Keys |
  |---|---|---|
  | Roughness values | `Min0` (0 or more) | `coat_roughness`, `glint.microfacet_roughness` |
  | Multipliers and levels | `Min0` | `roughness_scale`, `specular_level`, `coat_specular_level`, `displacement_scale`, `emissive_scale`, `uv_scale`, `inner_uv_scale`, `vertex_color_lum_mult`, `vertex_color_sat_mult`, `glint.screen_space_scale`, `glint.density_randomization` |
  | 0–1 factors | `Unit` (0–1) | `coat_strength`, `subsurface_opacity`, `fuzz.weight` |
  | **Fits no class: min 0, as the spec says** | `Min0` | `smooth_angle` (degrees), `auto_uv` (a divisor), `glint.log_microfacet_density` (a logarithm, so a negative value is meaningful: owner to confirm, spec gap 4) |

- **V6 How the Number control clamps.**
  - `DragValue::range(lo..=hi)`, so a drag and a typed value are clamped by egui.
  - `.clamp_existing_to_range(false)`: a hand-edited `-3` shows as it is, with the note, and is not rewritten just by being drawn.
  - A `custom_parser` sees the raw typed number. When it is outside the range, the note shows: "clamped to ≥ 0" (`Min0`) or "clamped to 0–1" (`Unit`).
  - An edit is pushed only when the value differs from the stored one, because `SetsDoc::set_value` marks the project dirty even for an equal value (spec Review Focus 3).
  - The writer clamps on export.
  - `pgpsim::range_warnings(entry)` lists out-of-range numbers. It is library API for hand-written JSON. The GUI does not call it, because sdc's writer never produces one (spec gap 5).
- **V7 Layout** (spec §4).
  - The main view is the visual groups, then **Matching**, then **Advanced TruePBR settings** (collapsed). The Match group leaves the main view.
  - `path_contains`, `nif_filter` and `rename` become `advanced: true`. `match1`–`match8` become `advanced: false`, so no group draws them: the Matching header owns them.
  - The Advanced drawer's "PGPatcher (advanced)" section and `pgp_fields::advanced` are removed.
- **V8 Roles → slots, and where the slot row is** (spec §5).
  - `sets::slot_kind(Subtype)` is the table `slot_of` already used.
  - A shader's usable slots are every `role.subtypes` of `spec.required ∪ spec.optional`, mapped through it. Roles matched only by suffix (`_e`, `_i`, `_sk` on Skin/Eye) and sub-types without a card (`envmask`, `complex_material`, `msn_specular`, `backlight_tint`, `skin_wet`) map to none. PGPatcher slot 5 has no card.
  - The row is ordered by `slot_number()`, then `SlotKind` order (slot 3: `_g` before `_sk`; slot 7: `_cnr` before `_f`).

    | Shader | Usable slot cards |
    |---|---|
    | `pbr` | 1 Diffuse, 2 Normal, 3 Emissive `_g`, 4 Height `_p`, 6 RMAOS, 7 Coat `_cnr`, 7 Fuzz `_f`, 8 Subsurface `_s` |
    | `advanced_skin` | 1, 2, 3 Skin `_sk`, 6 |
    | `glow` | 1, 2, 3 Emissive |
    | `parallax` | 1, 2, 4 Height |
    | every other shader | 1, 2 |

  - The pick is `SetView::shader`, so a `SHADER-UNAVAILABLE` fallback (`provenance: Fallback`) uses the fallback shader's slots. Its filled unusable slots stay visible with the amber outline (spec Review Focus 4).
  - With no pick, only the filled slots show.
  - **Where:** the 9-thumbnail row with empty boxes is the set editor's slot cards (`set_editor::cards`, `SlotKind::ALL`). The Sets-column pill draws one thumbnail per *member* and has no empty boxes (`sets_column.rs:402-414`). The slot row is therefore the set editor's cards, and the pill does not change.
- **V9 Versions.** There is no bump, for the reason in Global Constraints. There are no new codes and no `schema/` change.

## Review Focus

1. **A pasted `Textures\…` match value** (capital T, as Explorer copies it) must still match. PGPatcher prepends `textures\` unless the value starts with exactly `textures\`, which gives `textures\Textures\…`, and that never matches.
   - Task 3 pins it: `match_values_are_written_without_textures_and_with_one_dds`.
2. **A dotted stem (`rock.v2`)** must export `rock.v2.dds`, and its prefix must be `textures\pbr\…\rock.v2`, not `…\rock`.
   - Task 3 pins it: `match_values_are_written_without_textures_and_with_one_dds` and `the_default_match1_prefixes_and_renames`.
3. **A `_d`-suffixed source** (`rock01_d.png`, the vanilla naming) must keep `_d` in `match1` (that is the mesh's path), while its outputs go under `…\rock01`. Turning the normal on must write `rock01_n`, not `rock01_d_n`.
   - Task 3 pins it: `slot_matches_hit_the_unpatched_mesh_and_land_on_the_sdc_prefix` (case "a _d source") and `slot_match_paths_swap_the_diffuse_suffix`.
4. **Clicking into a Matching row and leaving it unchanged** must write nothing. Above all it must not turn the default `match1` into an explicit key, or mark the project dirty.
   - Task 4 pins it: `leaving_a_matching_row_unchanged_writes_nothing`.
5. **Typing into a number that has no default** ("not set", e.g. `coat_strength`) must clamp the same way.
   - Task 2 pins it: `typed_values_clamp_and_say_so`, the `coat_strength` half.

The spec's own Review Focus 1–4 are pinned as follows:
- RF1: Task 3, `slot_matches_hit_the_unpatched_mesh_and_land_on_the_sdc_prefix` and `a_renamed_set_still_matches_its_source_path`.
- RF2: Task 3, `match_values_are_written_without_textures_and_with_one_dds`.
- RF3: Task 2, `a_hand_edited_negative_number_shows_as_it_is_and_does_not_dirty_the_project`.
- RF4: Task 6, `a_fallback_shader_keeps_filled_slots_it_cannot_use`.

---

## File map

| File | Responsibility | Tasks |
|---|---|---|
| `crates/gui/src/pgp/schema.rs` | `NumRange`, `KeyKind::Number(NumRange)`, the 19 ranges; match keys out of `KEYS`; advanced flags | 1, 3, 5 |
| `crates/gui/src/pgp/writer.rs` | clamped numbers; `matchN` emission | 1, 3 |
| `crates/pgpsim/src/check.rs`, `crates/pgpsim/src/lib.rs` | `range_warnings` | 1 |
| `crates/gui/src/ui/pgp_fields.rs` | Number pattern (1); the ranged control (2); Matching header (4); layout (5) | 1–5 |
| `crates/gui/src/pgp/prefix.rs` | `active_matches`, `match_value`, `stored_match`, `match_toggle`, `slot_match_path`, `matches_nothing`, `NO_MATCH`, new `prefix_of`/`pitfalls` | 3 |
| `crates/gui/src/model/pbr_export.rs` | no-match sets out of the JSON; `pitfalls` call | 3 |
| `crates/gui/src/model/sets.rs` | `SlotMatch`, `slot_matches` (4); `slot_kind`, `usable_slots`, `SlotCell`, `slot_row` (6) | 4, 6 |
| `crates/gui/src/ui/set_editor.rs` | header line; "Match on this" (4); cards from `slot_row` (6) | 4, 6 |
| `crates/gui/src/ui/advanced.rs` | "PGPatcher (advanced)" section removed | 5 |
| tests: `crates/gui/tests/pgp.rs` | ranges, writer, prefix, RF1 proof | 1, 3 |
| tests: `crates/gui/tests/set_editor_ui.rs` | ranged control, Matching UI, layout, slot cards | 2–6 |
| tests: `crates/gui/tests/{sets_model,pbr_export,m3b_flow,set_preview_model}.rs` | `match_diffuse` → `match1` rewrites, new model tests | 3, 4, 6 |
| `docs/DECISIONS.md`, `docs/gui.md` | Note V, the GUI guide | 7 |

---

### Task 1: Number ranges — schema, writer clamp, pgpsim warnings

**Spec:** §2, §6 first bullet. Rulings V5, V6 (writer and pgpsim parts).

**Files:**
- Modify: `crates/gui/src/pgp/schema.rs:12-28` (`KeyKind`), `:73-74` (the `use` aliases), every `K::Number` in `KEYS` (`:102-293`).
- Modify: `crates/gui/src/pgp/writer.rs:1-6` (imports), `:150-154` (Number arm of `object`), `:194-202` (Number arm of `sub_object`).
- Modify: `crates/gui/src/ui/pgp_fields.rs:222` and `:267`, patterns only.
- Modify: `crates/pgpsim/src/check.rs` (new `range_warnings` + `mod tests`), `crates/pgpsim/src/lib.rs:12` (re-export).
- Test: `crates/gui/tests/pgp.rs` (three new tests after `kinds_match_the_owner_rules`).

**Interfaces:**
- Consumes: nothing new.
- Produces:
  - `pub enum NumRange { Min0, Unit }` (`Debug, Clone, Copy, PartialEq, Eq`) in `sdc_gui::pgp::schema`, with:
    - `pub fn bounds(self) -> (f64, f64)` (`(0.0, f64::INFINITY)` / `(0.0, 1.0)`);
    - `pub fn clamp(self, x: f64) -> f64`;
    - `pub fn note(self) -> &'static str` (`"clamped to ≥ 0"` / `"clamped to 0–1"`).
  - `KeyKind::Number(NumRange)`.
  - `pub fn pgpsim::range_warnings(entry: &serde_json::Value) -> Vec<String>`.

- [ ] **Step 1: Write the failing tests**

In `crates/gui/tests/pgp.rs`, after `kinds_match_the_owner_rules` (ends `:91`), add:

```rust
/// F1 spec §2, DECISIONS V5: every number key and its range, in KEYS order. Keys that fit no class (smooth_angle,
/// auto_uv, glint.log_microfacet_density) get min 0.
#[test]
fn number_keys_and_their_ranges() {
    use NumRange::{Min0, Unit};
    let got: Vec<(&str, NumRange)> = KEYS
        .iter()
        .filter_map(|k| match k.kind {
            KeyKind::Number(r) => Some((k.key, r)),
            _ => None,
        })
        .collect();
    assert_eq!(
        got,
        vec![
            ("smooth_angle", Min0),
            ("auto_uv", Min0),
            ("uv_scale", Min0),
            ("vertex_color_lum_mult", Min0),
            ("vertex_color_sat_mult", Min0),
            ("emissive_scale", Min0),
            ("displacement_scale", Min0),
            ("specular_level", Min0),
            ("roughness_scale", Min0),
            ("subsurface_opacity", Unit),
            ("coat_specular_level", Min0),
            ("coat_roughness", Min0),
            ("coat_strength", Unit),
            ("inner_uv_scale", Min0),
            ("glint.screen_space_scale", Min0),
            ("glint.log_microfacet_density", Min0),
            ("glint.microfacet_roughness", Min0),
            ("glint.density_randomization", Min0),
            ("fuzz.weight", Unit),
        ]
    );
    assert_eq!(Min0.clamp(-2.0), 0.0);
    assert_eq!(Min0.clamp(7.5), 7.5);
    assert_eq!(Unit.clamp(1.5), 1.0);
    assert_eq!(Min0.note(), "clamped to ≥ 0");
    assert_eq!(Unit.note(), "clamped to 0–1");
}

/// pgpsim keeps its own copy of the ranges (it cannot see the GUI); this pins the two together key for key.
#[test]
fn pgpsim_warns_on_exactly_the_gui_ranges() {
    for k in KEYS {
        let KeyKind::Number(range) = k.kind else {
            continue;
        };
        // Indexing a Null `Value` by a string makes it an object.
        let entry = |x: f64| {
            let mut v = Value::Null;
            match k.key.split_once('.') {
                Some((obj, sub)) => v[obj][sub] = json!(x),
                None => v[k.key] = json!(x),
            }
            v
        };
        assert_eq!(pgpsim::range_warnings(&entry(-0.5)).len(), 1, "{} below 0", k.key);
        assert!(pgpsim::range_warnings(&entry(0.5)).is_empty(), "{} inside", k.key);
        assert_eq!(
            pgpsim::range_warnings(&entry(2.0)).len(),
            usize::from(range == NumRange::Unit),
            "{} above 1",
            k.key
        );
    }
}
```

After `fixed_numbers_are_omitted_when_unset_or_equal_and_written_otherwise` (ends `:373`), add:

```rust
/// F1 spec §2: a hand-edited out-of-range value cannot reach the JSON; an in-range value is written as it was typed.
#[test]
fn the_writer_clamps_a_hand_edited_number_into_its_range() {
    let v = written(&entry(
        &[
            ("roughness_scale", json!(-2.0)),
            ("specular_level", json!(-0.1)),
            ("multilayer", json!(true)),
            ("coat_strength", json!(3)),
            ("coat_roughness", json!(-1)),
            ("coat_specular_level", json!(2)),
        ],
        "x",
    ));
    assert_eq!(v["roughness_scale"], json!(0.0));
    assert_eq!(v["specular_level"], json!(0.0));
    assert_eq!(v["coat_strength"], json!(1.0));
    assert_eq!(v["coat_roughness"], json!(0.0));
    assert_eq!(v["coat_specular_level"], json!(2), "in range: as typed");
    assert!(pgpsim::range_warnings(&v).is_empty(), "{v}");
    let g = written(&entry(
        &[
            ("glint_on", json!(true)),
            ("glint.microfacet_roughness", json!(-0.5)),
        ],
        "x",
    ));
    assert_eq!(g["glint"]["microfacet_roughness"], json!(0.0));
    let f = written(&entry(
        &[("fuzz_on", json!(true)), ("fuzz.weight", json!(1.5))],
        "x",
    ));
    assert_eq!(f["fuzz"]["weight"], json!(1.0));
    assert!(pgpsim::range_warnings(&g).is_empty() && pgpsim::range_warnings(&f).is_empty());
}
```

- [ ] **Step 2: Run them to verify they fail**

Run: `cargo test -p sdc-gui --offline --test pgp number_keys_and_their_ranges`
Expected: a compile FAIL. `NumRange` is not found, and `KeyKind::Number` takes no field.

- [ ] **Step 3: Implement `NumRange` and the table**

In `crates/gui/src/pgp/schema.rs`, change the `Number` variant of `KeyKind` (`:18`) to:

```rust
    /// A number, kept inside its range (F1 spec §2, DECISIONS V5).
    Number(NumRange),
```

After the `KeyKind` enum (after `:28`), add:

```rust
/// The values sdc lets a number key take (F1 spec §2, DECISIONS V5). Neither the CS True PBR wiki nor PGPatcher
/// documents limits; these keep values from going negative, and the 0–1 factors from going above 1.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NumRange {
    /// 0 or more: roughness values, multipliers and levels, and every key that fits no class.
    Min0,
    /// 0 to 1: strengths, weights, opacities.
    Unit,
}

impl NumRange {
    pub fn bounds(self) -> (f64, f64) {
        match self {
            NumRange::Min0 => (0.0, f64::INFINITY),
            NumRange::Unit => (0.0, 1.0),
        }
    }

    pub fn clamp(self, x: f64) -> f64 {
        let (lo, hi) = self.bounds();
        x.clamp(lo, hi)
    }

    /// The note a typed value outside the range shows next to its box.
    pub fn note(self) -> &'static str {
        match self {
            NumRange::Min0 => "clamped to ≥ 0",
            NumRange::Unit => "clamped to 0–1",
        }
    }
}
```

After `use KeyKind as K;` (`:74`), add:

```rust
const MIN0: KeyKind = K::Number(NumRange::Min0);
const UNIT: KeyKind = K::Number(NumRange::Unit);
```

In `KEYS`:
- Replace `K::Number` with `UNIT` in exactly three entries: `subsurface_opacity`, `coat_strength`, `fuzz.weight`.
- Replace `K::Number` with `MIN0` in the other 16 entries: `smooth_angle`, `auto_uv`, `uv_scale`, `vertex_color_lum_mult`, `vertex_color_sat_mult`, `emissive_scale`, `displacement_scale`, `specular_level`, `roughness_scale`, `coat_specular_level`, `coat_roughness`, `inner_uv_scale` and the four `glint.*`.
- Run `cargo fmt --all` afterwards. Some entries will fit on one line.

- [ ] **Step 4: Clamp in the writer**

In `crates/gui/src/pgp/writer.rs`, change the schema import (`:4`) to:

```rust
use super::schema::{Group, KeyKind, NumRange, KEYS, SCHEMA_URL};
```

After `fn colour` (ends `:63`), add:

```rust
/// `v` inside `range` (F1 spec §2): a hand-edited value outside it is written clamped. A value inside it, or one that
/// is not a number (the validator reports that), is written as it is.
fn clamped(v: &Value, range: NumRange) -> Value {
    match v.as_f64() {
        Some(x) if range.clamp(x) != x => json!(range.clamp(x)),
        _ => v.clone(),
    }
}
```

Replace the Number arm of `object` (`:150-154`):

```rust
            KeyKind::Number(range) => {
                let fixed = FIXED_NUMBERS.iter().find(|(n, _)| *n == k.key);
                v.map(|v| clamped(v, range))
                    .filter(|v| fixed.is_none_or(|(_, x)| v.as_f64() != Some(*x)))
                    .map(Node::Scalar)
            }
```

Replace the Number arm of `sub_object` (`:194-202`):

```rust
            KeyKind::Number(range) => v
                .map(|v| clamped(v, range))
                .or_else(|| {
                    GLINT_DEFAULTS
                        .iter()
                        .find(|(d, _)| *d == k.key)
                        .map(|(_, x)| json!(x))
                })
                .map(Node::Scalar),
```

- [ ] **Step 5: Keep `pgp_fields.rs` compiling**

In `crates/gui/src/ui/pgp_fields.rs`:
- At `:222`, change `matches!(k.kind, KeyKind::Number | KeyKind::Rgb | KeyKind::Rgba)` to `matches!(k.kind, KeyKind::Number(_) | KeyKind::Rgb | KeyKind::Rgba)`.
- At `:267`, change `KeyKind::Number => {` to `KeyKind::Number(_) => {`. Task 2 replaces this arm.

- [ ] **Step 6: Add `pgpsim::range_warnings`**

At the end of `crates/pgpsim/src/check.rs`, add:

```rust
/// The 0–1 factors among the number keys (sdc M6 F1 spec §2). Mirrors `KeyKind::Number(NumRange::Unit)` in sdc-gui's
/// `pgp/schema.rs`; the GUI test `pgpsim_warns_on_exactly_the_gui_ranges` keeps the two in step.
const UNIT_KEYS: [&str; 3] = ["subsurface_opacity", "coat_strength", "fuzz.weight"];

/// sdc's value ranges as warnings (sdc M6 F1 spec §2): every number key below 0, and a 0–1 factor above 1. PGPatcher
/// itself has no limits, so this is never a `Skip`. Values of the wrong type are `check_types`' business.
pub fn range_warnings(entry: &Value) -> Vec<String> {
    let Some(e) = entry.as_object() else {
        return vec![];
    };
    let mut nums: Vec<(String, f64)> = NUM_KEYS
        .iter()
        .filter_map(|k| Some((k.to_string(), e.get(*k)?.as_f64()?)))
        .collect();
    for (obj, keys) in [("glint", &GLINT_KEYS[..]), ("fuzz", &["weight"][..])] {
        if let Some(o) = e.get(obj).and_then(Value::as_object) {
            nums.extend(
                keys.iter()
                    .filter_map(|k| Some((format!("{obj}.{k}"), o.get(*k)?.as_f64()?))),
            );
        }
    }
    nums.into_iter()
        .filter_map(|(k, x)| {
            let unit = UNIT_KEYS.contains(&k.as_str());
            let hi = if unit { 1.0 } else { f64::INFINITY };
            (!(0.0..=hi).contains(&x)).then(|| {
                let range = if unit { "0–1" } else { "≥ 0" };
                format!("{k} is {x}, outside {range}")
            })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::range_warnings;
    use serde_json::json;

    #[test]
    fn ranges_warn_below_zero_and_above_one_for_the_factors() {
        assert!(range_warnings(
            &json!({"roughness_scale": 0.0, "coat_strength": 1.0, "fuzz": {"weight": 0.5}})
        )
        .is_empty());
        let w = range_warnings(&json!({
            "roughness_scale": -1, "coat_strength": 1.5, "specular_level": 9.0,
            "glint": {"microfacet_roughness": -0.1}, "fuzz": {"weight": 2}
        }));
        assert_eq!(w.len(), 4, "{w:?}");
        assert!(w.iter().any(|m| m.contains("coat_strength") && m.contains("0–1")));
        assert!(w
            .iter()
            .any(|m| m.contains("glint.microfacet_roughness") && m.contains("≥ 0")));
        assert!(
            range_warnings(&json!({"roughness_scale": "x"})).is_empty(),
            "a wrong type is check_types' business"
        );
        assert!(range_warnings(&json!(3)).is_empty());
    }
}
```

In `crates/pgpsim/src/lib.rs`, after `pub use matching::{matched_path, shape_matches};` (`:12`), add:

```rust
pub use check::range_warnings;
```

- [ ] **Step 7: Run the tests to verify they pass**

Run: `cargo test -p sdc-gui --offline --test pgp`
Expected: PASS, including the three new tests.
Run: `cargo test -p pgpsim --offline --lib ranges_warn`
Expected: PASS.

- [ ] **Step 8: Run all gates, then commit**

```bash
git add crates/gui/src/pgp/schema.rs crates/gui/src/pgp/writer.rs crates/gui/src/ui/pgp_fields.rs crates/pgpsim/src/check.rs crates/pgpsim/src/lib.rs crates/gui/tests/pgp.rs
git commit -m "feat(gui): PGPatcher number ranges; the writer clamps, pgpsim warns (M6 Track F1)

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

---

### Task 2: The ranged number control

**Spec:** §2 "GUI", §6 first bullet, Review Focus 3. Ruling V6.

**Files:**
- Modify: `crates/gui/src/ui/pgp_fields.rs`:
  - imports (`:3-13`);
  - the `KeyKind::Number(_)` arm of `control` (`:267-288`);
  - a new `parse_number` after `control`.
- Test: `crates/gui/tests/set_editor_ui.rs`: a `panel_harness` helper and three tests, after `an_unset_number_shows_the_fixed_default_and_an_edit_is_the_sets_own`.

**Interfaces:**
- Consumes: `NumRange::{bounds, clamp, note}` (Task 1).
- Produces, for later tasks' tests: in `crates/gui/tests/set_editor_ui.rs`, `fn panel_harness(m: AppModel) -> Harness<'static, AppModel>`. It draws only `pgp_fields::panel(ui, m, "landscape/rock01")` at 700×900.

- [ ] **Step 1: Write the failing tests**

In `crates/gui/tests/set_editor_ui.rs`, after `an_unset_number_shows_the_fixed_default_and_an_edit_is_the_sets_own`, add:

```rust
/// The PGPatcher panel of `landscape/rock01` alone.
fn panel_harness(m: AppModel) -> Harness<'static, AppModel> {
    let h = Harness::builder()
        .with_size(eframe::egui::vec2(700.0, 900.0))
        .build_ui_state(
            |ui, m: &mut AppModel| sdc_gui::ui::pgp_fields::panel(ui, m, "landscape/rock01"),
            m,
        );
    sdc_gui::fonts::install_system_fonts(&h.ctx).expect("system fonts");
    h
}

/// Focus the number box showing `shown` and select its text (egui_kittest's DragValue pattern). If the accesskit
/// `focus()` does not put the DragValue into text editing, replace it with `.click()`: a plain click on a DragValue
/// requests focus.
fn focus_spin(h: &mut Harness<'static, AppModel>, shown: &str) {
    h.query_all_by_role(eframe::egui::accesskit::Role::SpinButton)
        .find(|n| n.value().as_deref() == Some(shown))
        .unwrap_or_else(|| panic!("no number box shows {shown}: {:?}", spin_values(h)))
        .focus();
    h.run();
    h.key_press_modifiers(eframe::egui::Modifiers::COMMAND, eframe::egui::Key::A);
    h.run();
}

/// Type one character into the focused widget. Text events go to the focused widget, whichever node sends them.
fn type_char(h: &Harness<'static, AppModel>, c: char) {
    h.query_all_by_role(eframe::egui::accesskit::Role::SpinButton)
        .next()
        .expect("a number box")
        .type_text(&c.to_string());
}

/// F1 spec §2: egui clamps a drag to the range the box reports, so the range is the drag test.
#[test]
fn every_number_box_reports_its_range() {
    let d = tempfile::tempdir().unwrap();
    let mut m = pbr_opened(d.path());
    m.set_json_value(
        "landscape/rock01",
        "multilayer",
        Some(serde_json::json!(true)),
    );
    m.set_json_value(
        "landscape/rock01",
        "coat_strength",
        Some(serde_json::json!(0.5)),
    );
    let mut h = panel_harness(m);
    h.run();
    let spins: Vec<_> = h
        .query_all_by_role(eframe::egui::accesskit::Role::SpinButton)
        .map(|n| {
            let a = n.accesskit_node();
            (n.value(), a.min_numeric_value(), a.max_numeric_value())
        })
        .collect();
    assert!(spins.len() >= 5, "{spins:?}");
    for (v, min, max) in &spins {
        assert_eq!(*min, Some(0.0), "{v:?}");
        let unit = v.as_deref() == Some("0.5");
        assert_eq!(*max, unit.then_some(1.0), "{v:?}");
    }
}

/// F1 spec §2: a typed value outside the range is clamped, and the box says so for that frame.
#[test]
fn typed_values_clamp_and_say_so() {
    let d = tempfile::tempdir().unwrap();
    let mut m = pbr_opened(d.path());
    m.set_json_value(
        "landscape/rock01",
        "multilayer",
        Some(serde_json::json!(true)),
    );
    let mut h = panel_harness(m);
    h.run();
    // roughness_scale shows its fixed default 1.0.
    focus_spin(&mut h, "1.0");
    type_char(&h, '-');
    h.run();
    type_char(&h, '5');
    h.step();
    h.get_by_label("clamped to ≥ 0");
    h.key_press(eframe::egui::Key::Enter);
    h.run();
    assert_eq!(
        rock_settings(&h)["roughness_scale"],
        serde_json::json!(0.0)
    );
    // coat_strength has no default ("not set"): a 2 becomes 1.
    h.state_mut().set_json_value(
        "landscape/rock01",
        "coat_strength",
        Some(serde_json::json!(0.5)),
    );
    h.run();
    focus_spin(&mut h, "0.5");
    type_char(&h, '2');
    h.step();
    h.get_by_label("clamped to 0–1");
    h.key_press(eframe::egui::Key::Enter);
    h.run();
    assert_eq!(rock_settings(&h)["coat_strength"], serde_json::json!(1.0));
}

/// F1 spec Review Focus 3: a hand-edited value outside the range shows as it is with the note. Drawing it is no
/// edit, so the project stays clean (the writer clamps it on export).
#[test]
fn a_hand_edited_negative_number_shows_as_it_is_and_does_not_dirty_the_project() {
    let d = tempfile::tempdir().unwrap();
    let mut m = pbr_opened(d.path());
    m.set_json_value(
        "landscape/rock01",
        "roughness_scale",
        Some(serde_json::json!(-3.0)),
    );
    m.save().unwrap();
    assert!(!m.sets_doc.as_ref().unwrap().is_dirty());
    let mut h = panel_harness(m);
    h.run();
    h.run();
    assert!(spin_values(&h).contains(&"-3.0".to_string()), "{:?}", spin_values(&h));
    h.get_by_label("clamped to ≥ 0");
    assert!(!h.state().sets_doc.as_ref().unwrap().is_dirty());
    assert_eq!(
        rock_settings(&h)["roughness_scale"],
        serde_json::json!(-3.0)
    );
}
```

- [ ] **Step 2: Run them to verify they fail**

Run: `cargo test -p sdc-gui --offline --test set_editor_ui -- every_number_box typed_values a_hand_edited`
Expected: FAIL, for three reasons:
- `min_numeric_value` is `None`, because the boxes have no range;
- there is no "clamped" label;
- the `-3.0` is redrawn as it is, but the note is missing.

- [ ] **Step 3: Implement the ranged control**

In `crates/gui/src/ui/pgp_fields.rs`, add to the imports:

```rust
use super::columns::AMBER;
```

Replace the whole `KeyKind::Number(_) => { … }` arm of `control` with:

```rust
        KeyKind::Number(range) => {
            let default = GLINT_DEFAULTS
                .iter()
                .chain(FIXED_NUMBERS.iter())
                .find(|(d, _)| *d == k.key)
                .map(|(_, x)| *x);
            let unset_unknown = value.is_none() && default.is_none();
            let stored = value.as_ref().and_then(Value::as_f64);
            let mut x = stored.or(default).unwrap_or(0.0);
            let (lo, hi) = range.bounds();
            // The parser is the only place a typed number is seen before DragValue clamps it.
            let typed_outside = std::cell::Cell::new(false);
            let mut drag = egui::DragValue::new(&mut x)
                .range(lo..=hi)
                // A hand-edited value outside the range shows as it is: clamping it here would edit the project just
                // by drawing it (F1 Review Focus 3). The writer clamps it on export.
                .clamp_existing_to_range(false)
                .custom_parser(|s| {
                    let n = parse_number(s)?;
                    typed_outside.set(!(lo..=hi).contains(&n));
                    Some(n)
                })
                .speed(0.01)
                .max_decimals(4)
                .custom_formatter(|n, _| format!("{:?}", (n * 1e4).round() / 1e4));
            if unset_unknown {
                // No default to show: say so rather than a 0.0 that looks like one.
                drag = drag.custom_formatter(|_, _| "not set".to_string());
            }
            let r = ui
                .horizontal(|ui| {
                    let r = ui.add(drag);
                    let stored_outside = stored.is_some_and(|s| range.clamp(s) != s);
                    if typed_outside.get() || stored_outside {
                        ui.label(egui::RichText::new(range.note()).small().color(AMBER));
                    }
                    r
                })
                .inner;
            // An equal value is no edit: `SetsDoc::set_value` marks the project dirty even then.
            if r.changed() && stored != Some(x) {
                edits.push((key, Some(json!(x))));
            }
        }
```

After `fn control` (at the end of the file), add:

```rust
/// egui's own number parsing (whitespace ignored, U+2212 read as a minus), so the box accepts what it always did.
fn parse_number(s: &str) -> Option<f64> {
    s.chars()
        .filter(|c| !c.is_whitespace())
        .map(|c| if c == '\u{2212}' { '-' } else { c })
        .collect::<String>()
        .parse()
        .ok()
}
```

- [ ] **Step 4: Run the tests to verify they pass**

Run: `cargo test -p sdc-gui --offline --test set_editor_ui`
Expected: PASS. `an_unset_number_shows_the_fixed_default_and_an_edit_is_the_sets_own` still passes, because the values show as before.

- [ ] **Step 5: Run all gates, then commit**

```bash
git add crates/gui/src/ui/pgp_fields.rs crates/gui/tests/set_editor_ui.rs
git commit -m "feat(gui): number boxes keep their range and say when a typed value is clamped (M6 Track F1)

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

---

### Task 3: Exact slot matching — model, writer, export, and the pgpsim proof

**Spec:** §3.1, §3.3, §6 "New set", §7 Review Focus 1–2. Rulings V1–V4.

**Files:**
- Modify: `crates/gui/src/pgp/prefix.rs`:
  - delete `MatchMode` and `match_mode` (`:10-34`);
  - delete `check_match_value` (`:108-123`);
  - replace `prefix_of` (`:166-213`) and `pitfalls` (`:215-238`);
  - add the new functions;
  - `use serde_json::{json, Value};`.
- Modify: `crates/gui/src/pgp/schema.rs:9-10` (`NOT_EXPOSED`), `:77-78` (remove the `match_diffuse` and `match_normal` entries).
- Modify: `crates/gui/src/pgp/writer.rs:3` (import), `:132` (`let mode`), `:156-170` (Text arm).
- Modify: `crates/gui/src/model/pbr_export.rs:8` (import), `json_entries` (`:170-188`), `export_asks` (`:396-409`).
- Modify: `crates/gui/src/ui/pgp_fields.rs`:
  - imports (`:5`);
  - `advanced()` drops the "Matching by" lines (`:148-154`);
  - the Text hint (`:295-299`).
- Test (rewrites and new): `crates/gui/tests/pgp.rs`, `crates/gui/tests/sets_model.rs`, `crates/gui/tests/pbr_export.rs`, `crates/gui/tests/m3b_flow.rs`, `crates/gui/tests/set_preview_model.rs`, `crates/gui/tests/set_editor_ui.rs`.

**Interfaces:**
- Consumes: nothing from Tasks 1–2.
- Produces, in `sdc_gui::pgp::prefix`:
  - `pub fn active_matches(s: &Settings, derived: &str) -> Vec<(u8, String)>`: stored form (no `.dds`), N ascending.
  - `pub fn match_value(v: &str) -> String`: JSON form.
  - `pub fn stored_match(n: u8, typed: &str) -> Option<Value>`.
  - `pub fn match_toggle(n: u8, on: bool, path: &str) -> (String, Option<Value>)`.
  - `pub fn slot_match_path(derived: &str, suffix: &str) -> String`.
  - `pub fn matches_nothing(s: &Settings, derived: &str) -> bool`.
  - `pub const NO_MATCH: &str = "this entry matches nothing"`.
  - `pub fn pitfalls(s: &Settings, derived: &str) -> Vec<String>`: this signature changes.
  - `prefix_of(&Settings, &str)` keeps its signature.
  - `MatchMode` and `match_mode` no longer exist.

- [ ] **Step 1: Write the failing tests and rewrite the ones that encode `match_diffuse`**

`crates/gui/tests/pgp.rs`:

(a) Replace `a_plain_entry_writes_only_the_match` (`:309-313`):

```rust
/// F1 spec §6 "New set": match1 is the derived diffuse path with .dds; match_diffuse and match_normal are never written.
#[test]
fn a_plain_entry_writes_only_match1() {
    let v = written(&entry(&[], r"landscape\rock01"));
    assert_eq!(v, json!({ "match1": r"landscape\rock01.dds" }));
    let old = written(&entry(
        &[
            ("match_diffuse", json!(r"a\b")),
            ("match_normal", json!(r"c\d")),
        ],
        r"landscape\rock01",
    ));
    assert_eq!(old, json!({ "match1": r"landscape\rock01.dds" }), "old keys are ignored");
}
```

(b) Replace `diffuse_mode_prefixes_and_renames` (`:143-175`):

```rust
#[test]
fn the_default_match1_prefixes_and_renames() {
    let p = |pairs: &[(&str, Value)], derived: &str| prefix_of(&s(pairs), derived).unwrap();
    assert_eq!(p(&[], r"landscape\rock01"), r"textures\pbr\landscape\rock01");
    // PGPatcher cuts the slot-1 suffix from a match1 hit, so a _d source's outputs go under ...\rock01.
    assert_eq!(p(&[], r"landscape\rock01_d"), r"textures\pbr\landscape\rock01");
    assert_eq!(p(&[], r"landscape\rock.v2"), r"textures\pbr\landscape\rock.v2");
    assert_eq!(
        p(&[("match1", json!(r"landscape\rock02.dds"))], "x"),
        r"textures\pbr\landscape\rock02"
    );
    assert_eq!(
        p(&[("match1", json!(r"pbr\landscape\rock02"))], "x"),
        r"textures\pbr\landscape\rock02",
        "never doubled"
    );
    assert_eq!(
        p(&[("match1", json!(r"Textures\landscape\rock02"))], "x"),
        r"textures\pbr\landscape\rock02"
    );
    // With a slot match, rename is the whole prefix.
    assert_eq!(
        p(
            &[("rename", json!(r"textures\pbr\landscape\boulder"))],
            r"landscape\rock01"
        ),
        r"textures\pbr\landscape\boulder"
    );
    // The lowest active match decides; match2 alone gives the same prefix.
    assert_eq!(
        p(
            &[("match1", json!("")), ("match2", json!(r"landscape\rock01_n"))],
            "x"
        ),
        r"textures\pbr\landscape\rock01"
    );
    // path_contains alone, or nothing at all: the derived path.
    assert_eq!(
        p(
            &[("match1", json!("")), ("path_contains", json!("rock"))],
            r"landscape\rock01"
        ),
        r"textures\pbr\landscape\rock01"
    );
    assert_eq!(
        p(&[("match1", json!(""))], r"landscape\rock01"),
        r"textures\pbr\landscape\rock01"
    );
    // Old keys are ignored (F1 spec §3.3).
    assert_eq!(
        p(
            &[("match_diffuse", json!(r"a\b")), ("match_normal", json!(r"c\d"))],
            r"landscape\rock01"
        ),
        r"textures\pbr\landscape\rock01"
    );
}
```

(c) In `slot_modes_strip_the_first_suffix_of_their_own_list` (`:176-211`):
- Change the helper `p` so the default `match1` is off:

  ```rust
      let p = |key: &str, v: &str| {
          prefix_of(&s(&[("match1", json!("")), (key, json!(v))]), "unused").unwrap()
      };
  ```

- Change the rename case's settings to:

  ```rust
          &s(&[
              ("match1", json!("")),
              ("match2", json!(r"a\b_n.dds")),
              ("rename", json!(r"textures\pbr\c\d")),
          ]),
  ```

(d) In `bad_paths_block` (`:212-230`):
- Change `("match_diffuse", json!(bad))` to `("match1", json!(bad))`.
- Replace the comment `// An empty match_diffuse falls back to the derived match; an empty derived match blocks.` with `// No match1 key and no derived path: no prefix.`

(e) Replace `match_values_starting_with_textures_block` (`:231-248`):

```rust
/// F1 spec Review Focus 2, DECISIONS V2: .dds exactly once, and no leading textures\ (PGPatcher's prepend check is
/// case-sensitive, so a pasted Textures\ would become textures\Textures\ and never match).
#[test]
fn match_values_are_written_without_textures_and_with_one_dds() {
    for (stored, written) in [
        (r"landscape\rock01", r"landscape\rock01.dds"),
        (r"landscape\rock01.dds", r"landscape\rock01.dds"),
        (r"landscape\rock01.DDS", r"landscape\rock01.DDS"),
        (r"Textures\landscape\rock01.dds", r"landscape\rock01.dds"),
        (r"textures\pbr\x\y", r"pbr\x\y.dds"),
        (r"x\rock.v2", r"x\rock.v2.dds"),
        ("  x\\y  ", r"x\y.dds"),
    ] {
        assert_eq!(match_value(stored), written, "{stored:?}");
    }
    // What a typed value stores: no .dds; empty is off (match1 keeps "" so the derived default stays off).
    assert_eq!(stored_match(1, r"x\y.dds"), Some(json!(r"x\y")));
    assert_eq!(stored_match(3, r" x\y.DDS "), Some(json!(r"x\y")));
    assert_eq!(stored_match(2, r"a\b_n"), Some(json!(r"a\b_n")));
    assert_eq!(stored_match(1, "  "), Some(json!("")));
    assert_eq!(stored_match(4, ""), None);
    // A pasted value with its extension is written once.
    let v = written(&entry(&[("match1", json!(r"landscape\rock01.dds"))], "x"));
    assert_eq!(v["match1"], json!(r"landscape\rock01.dds"));
}

#[test]
fn toggles_and_slot_match_paths() {
    assert_eq!(match_toggle(1, true, "ignored"), ("match1".to_string(), None));
    assert_eq!(match_toggle(1, false, "ignored"), ("match1".to_string(), Some(json!(""))));
    assert_eq!(
        match_toggle(2, true, r"a\b_n"),
        ("match2".to_string(), Some(json!(r"a\b_n")))
    );
    assert_eq!(match_toggle(6, false, r"a\b_rmaos"), ("match6".to_string(), None));
    let on = |pairs: &[(&str, Value)]| -> Vec<u8> {
        active_matches(&s(pairs), r"landscape\rock01").iter().map(|(n, _)| *n).collect()
    };
    assert_eq!(on(&[]), vec![1]);
    assert_eq!(on(&[("match1", json!(""))]), Vec::<u8>::new());
    assert_eq!(on(&[("match2", json!(r"x\y_n")), ("match6", json!(" "))]), vec![1, 2]);
    assert!(active_matches(&s(&[]), "").is_empty(), "no diffuse, no default");
    assert!(matches_nothing(&s(&[("match1", json!(""))]), r"landscape\rock01"));
    assert!(!matches_nothing(
        &s(&[("match1", json!("")), ("path_contains", json!("rock"))]),
        r"landscape\rock01"
    ));
}

/// Review Focus 3: the slot-1 suffix of the source gives way to the slot's own.
#[test]
fn slot_match_paths_swap_the_diffuse_suffix() {
    assert_eq!(slot_match_path(r"landscape\rock01", ""), r"landscape\rock01");
    assert_eq!(slot_match_path(r"landscape\rock01_d", ""), r"landscape\rock01_d");
    assert_eq!(slot_match_path(r"landscape\rock01_d", "n"), r"landscape\rock01_n");
    assert_eq!(slot_match_path(r"landscape\Rock01_D", "n"), r"landscape\Rock01_n");
    assert_eq!(slot_match_path(r"actors\facemask", "n"), r"actors\facemask_n");
    assert_eq!(slot_match_path("d", "n"), "d_n");
    assert_eq!(slot_match_path(r"landscape\rock01", "rmaos"), r"landscape\rock01_rmaos");
}
```

(f) In `forward_slashes_in_json_paths_block` (`:250-268`), change `for key in ["match_diffuse", "match_normal", "match2", "rename"]` to `for key in ["match1", "match2", "rename"]`.

(g) Replace `pitfalls_are_reported_not_fixed` (`:270-280`):

```rust
#[test]
fn pitfalls_are_reported_not_fixed() {
    let w = pitfalls(&s(&[("match8", json!(r"x\leaf_s.dds"))]), "x");
    assert!(w.iter().any(|m| m.contains("match8")), "{w:?}");
    let w = pitfalls(
        &s(&[("match2", json!(r"a\b_n.dds")), ("rename", json!(r"c\d"))]),
        "x",
    );
    assert!(w.iter().any(|m| m.contains("full path")), "{w:?}");
    assert!(pitfalls(&s(&[]), r"landscape\rock01").is_empty());
    let w = pitfalls(&s(&[("match1", json!(""))]), "x");
    assert!(
        w.iter().any(|m| m.starts_with(NO_MATCH) && m.contains("left out")),
        "{w:?}"
    );
}
```

(h) Replace `other_match_modes_omit_match_diffuse_and_tri_state_writes_only_when_set` (`:469-483`):

```rust
#[test]
fn match_slots_are_written_with_dds_and_tri_state_writes_only_when_set() {
    let v = written(&entry(
        &[
            ("match_diffuse", json!("old")),
            ("match2", json!(r"a\b_n")),
            ("lock_rmaos", json!(false)),
        ],
        "x",
    ));
    assert!(v.get("match_diffuse").is_none() && v.get("match_normal").is_none());
    assert_eq!(v["match1"], json!("x.dds"));
    assert_eq!(v["match2"], json!(r"a\b_n.dds"));
    assert_eq!(v["lock_rmaos"], json!(false));
    assert!(v.get("lock_diffuse").is_none());
}
```

(i) In `slots_and_key_order` (`:484-510`), change `pos("match_diffuse")` to `pos("match1")`.

(j) In `slot_suffixes_compare_ascii_only` (`:680-691`), add `("match1", json!("")),` as the first pair of both `s(&[…])` calls.

(k) At the end of `crates/gui/tests/pgp.rs`, add the RF1 proof:

```rust
/// F1 spec Review Focus 1, DECISIONS V1: matchN holds the path the unpatched mesh carries. PGPatcher hits it, and
/// its matched path is the prefix sdc writes the outputs under. `derive_match_diffuse` is called with the stem that
/// `set_views` gives it: `mirrored_stem` cuts only colour vocabulary terms, and `d` is not one, so `rock01_d` stays.
#[test]
fn slot_matches_hit_the_unpatched_mesh_and_land_on_the_sdc_prefix() {
    let default = document(&[]).to_value()["default"].clone();
    let mesh = |d: &str, n: &str| {
        let mut m = pgpsim::MaterialState::builtin_default();
        m.set_slot_raw(0, d);
        m.set_slot_raw(1, n);
        m
    };
    let vanilla = mesh(
        r"textures\landscape\rock01.dds",
        r"textures\landscape\rock01_n.dds",
    );
    let d_named = mesh(
        r"Textures\Landscape\Rock01_D.dds",
        r"textures\landscape\rock01_n.dds",
    );
    let nif = r"meshes\landscape\rock01.nif";
    let cases: Vec<(&str, &str, &str, Vec<(&str, Value)>, &pgpsim::MaterialState)> = vec![
        ("match1, the default", "textures/landscape", "rock01", vec![], &vanilla),
        (
            "a pbr source folder still names the vanilla path",
            "textures/pbr/landscape",
            "rock01",
            vec![],
            &vanilla,
        ),
        (
            "match2 alone",
            "textures/landscape",
            "rock01",
            vec![
                ("match1", json!("")),
                ("match2", json!(slot_match_path(r"landscape\rock01", "n"))),
            ],
            &vanilla,
        ),
        (
            "match1 and match2",
            "textures/landscape",
            "rock01",
            vec![("match2", json!(r"landscape\rock01_n"))],
            &vanilla,
        ),
        ("a _d source keeps _d in match1", "textures/landscape", "rock01_d", vec![], &d_named),
    ];
    for (label, dir, stem, pairs, shape) in cases {
        let derived = derive_match_diffuse(dir, stem);
        let e = entry(&pairs, &derived);
        let prepared = pgpsim::prepare(&written(&e), &default);
        assert!(pgpsim::shape_matches(&prepared, nif, &shape.slots), "{label}");
        assert_eq!(
            pgpsim::matched_path(&prepared, &shape.slots),
            Some(prefix_of(&e.settings, &derived).unwrap().to_ascii_lowercase()),
            "{label}"
        );
    }
    // matchN is exact (no strip_pbr): a mesh already pointing at textures\pbr\ is not hit. PGPatcher patches the
    // unpatched copy, which carries the vanilla path.
    let patched = mesh(
        r"textures\pbr\landscape\rock01.dds",
        r"textures\pbr\landscape\rock01_n.dds",
    );
    let prepared = pgpsim::prepare(&written(&entry(&[], r"landscape\rock01")), &default);
    assert!(!pgpsim::shape_matches(&prepared, nif, &patched.slots));
}
```

`crates/gui/tests/sets_model.rs`:
- In `json_only_edits_do_not_replan_but_match_edits_do` (`:310-327`), `clearing_a_match_returns_to_the_derived_prefix` (`:329-360`), `a_packed_format_applies_after_a_match_edit` (`:720-727`) and `a_packed_entry_stays_matched_after_a_match_edit` (`:762-769`), change every `"match_diffuse"` key to `"match1"`. The values and expectations stay; for example, `match1 = landscape\boulder` gives the prefix `textures\pbr\landscape\boulder`.
- Add at the end of the file:

```rust
/// F1 spec Review Focus 1: match1 comes from the source, so a set rename does not move it off the mesh's path.
#[test]
fn a_renamed_set_still_matches_its_source_path() {
    use sdc_gui::model::pbr_export::json_entries;
    use sdc_gui::pgp::writer::entry_node;
    let d = tempfile::tempdir().unwrap();
    let mut m = pbr_opened(d.path());
    idle(&mut m);
    m.rename_set("landscape/rock01", "landscape/boulder").unwrap();
    idle(&mut m);
    let (_, e) = json_entries(&m)
        .into_iter()
        .find(|(v, _)| v.id == "landscape/boulder")
        .expect("the renamed set is a PBR set");
    assert_eq!(
        entry_node(&e).to_value()["match1"],
        json!(r"landscape\rock01.dds")
    );
}
```

`crates/gui/tests/pbr_export.rs`:
- In the collision test at `:96-101`, change `"match_diffuse"` to `"match1"`.
- In `export_writes_a_valid_json_and_records_its_hash` (`:504`), change `.find(|e| e["match_diffuse"] == json!(r"landscape\rock01"))` to `.find(|e| e["match1"] == json!(r"landscape\rock01.dds"))`.
- Add at the end of the file:

```rust
/// F1 spec §3.2 "No match", DECISIONS V4: the export still runs, asks first, and the entry stays out of the JSON
/// (the schema refuses an entry without a matcher).
#[test]
fn a_set_that_matches_nothing_asks_and_is_left_out_of_the_json() {
    let d = tempfile::tempdir().unwrap();
    let mut m = ready(d.path());
    m.set_json_value("landscape/rock01", "match1", Some(json!("")));
    idle(&mut m);
    assert_eq!(export_blocks(&m), vec![]);
    assert!(
        export_asks(&m)
            .iter()
            .any(|i| i.set_id == "landscape/rock01" && i.text.contains("this entry matches nothing")),
        "{:?}",
        export_asks(&m)
    );
    assert!(json_entries(&m).iter().all(|(v, _)| v.id != "landscape/rock01"));
}
```

`crates/gui/tests/m3b_flow.rs:128`: replace the line with

```rust
    assert_eq!(entries[0]["match1"], json!(r"landscape\rock01.dds"));
    assert!(entries[0].get("match_diffuse").is_none());
```

`crates/gui/tests/set_preview_model.rs`:
- At `:428`, change `("match_diffuse", json!(r"y\rock"))` to `("match1", json!(r"y\rock"))`.
- Replace `the_query_key_follows_the_match_mode` (`:484-529`):

```rust
#[test]
fn the_query_key_follows_the_match_mode() {
    let d = default_block();
    let key = |pairs: &[(&str, Value)]| tex_key(&value(pairs), &d).unwrap();
    let default_match1 = TexKey::Exact {
        slot: 0,
        path: r"x\rock.dds".into(),
    };
    assert_eq!(key(&[]), (default_match1.clone(), None));
    assert_eq!(
        key(&[
            ("match_diffuse", json!(r"y\z")),
            ("match_normal", json!(r"y\z"))
        ])
        .0,
        default_match1,
        "old keys are never written"
    );
    assert_eq!(
        key(&[("path_contains", json!("rock"))]).0,
        TexKey::Contains {
            slot: 0,
            frag: "rock".into()
        }
    );
    assert_eq!(
        key(&[("match1", json!("")), ("match3", json!(r"x\rock_g.dds"))]).0,
        TexKey::Exact {
            slot: 2,
            path: r"x\rock_g.dds".into()
        }
    );
    assert_eq!(
        key(&[("nif_filter", json!("armor"))]).1.as_deref(),
        Some("armor")
    );
}
```

- In `a_built_in_shape_stands_in_for_every_match_mode` (`:567-573`), replace the case `&[("match_normal", json!(r"x\rock"))],` with `&[("match1", json!("")), ("match2", json!(r"x\rock_n"))],`.

`crates/gui/tests/set_editor_ui.rs`, `the_header_has_the_json_name_and_the_drawer_has_the_advanced_keys` (`:619-634`): replace the two lines

```rust
    h.get_by_label_contains("Matching by: match_diffuse");
    h.get_by_label("Match normal");
```

with

```rust
    h.get_by_label("Match slot 1 (diffuse)");
    assert!(h.query_by_label("Match normal").is_none());
```

Task 5 rewrites this test.

- [ ] **Step 2: Run them to verify they fail**

Run: `cargo test -p sdc-gui --offline --test pgp`
Expected: a compile FAIL. `match_value`, `stored_match`, `match_toggle`, `active_matches`, `slot_match_path`, `matches_nothing` and `NO_MATCH` are not found, and `pitfalls` takes 1 argument.

- [ ] **Step 3: Implement the matching model in `prefix.rs`**

In `crates/gui/src/pgp/prefix.rs`:
- Change the module doc's first line to: `//! Slot matches and the output prefix PGPatcher derives (M3b spec §5.5; M6 F1 spec §3, DECISIONS V1–V4).`
- Change `use serde_json::Value;` to `use serde_json::{json, Value};`.
- Delete `MatchMode` and `match_mode` (`:10-34`), and `check_match_value` (`:108-123`).
- Replace `prefix_of` and `pitfalls` (`:164-238`) with:

```rust
/// `matchN` as the JSON writes it (DECISIONS V2): no leading `textures\` (PGPatcher prepends one only when the value
/// does not start with exactly `textures\`), and `.dds` exactly once (F1 Review Focus 2).
pub fn match_value(v: &str) -> String {
    let v = strip_ci(v.trim(), "textures\\");
    if has_dds(v) {
        v.to_string()
    } else {
        format!("{v}.dds")
    }
}

fn has_dds(v: &str) -> bool {
    v.len() >= 4
        && v.get(v.len() - 4..)
            .is_some_and(|e| e.eq_ignore_ascii_case(".dds"))
}

/// The active slot matches of a set, stored form (no `.dds`), N ascending (F1 spec §3.1, DECISIONS V2). `match1` is
/// the derived diffuse path while the set has no `match1` key; `match1 = ""` is the diffuse toggle turned off.
/// `match_diffuse` and `match_normal` are ignored (spec §3.3).
pub fn active_matches(s: &Settings, derived: &str) -> Vec<(u8, String)> {
    (1..=8u8)
        .filter_map(|n| {
            let v = match s.get(&format!("match{n}")) {
                None if n == 1 => derived,
                Some(Value::String(v)) => v.as_str(),
                _ => return None,
            };
            let v = v.trim();
            (!v.is_empty()).then(|| (n, v.to_string()))
        })
        .collect()
}

/// What a typed `matchN` value stores: trimmed, one trailing `.dds` cut. Empty turns the match off: `match1 = ""`
/// (removing the key would bring the derived default back), any other slot loses its key.
pub fn stored_match(n: u8, typed: &str) -> Option<Value> {
    let t = typed.trim();
    let t = if has_dds(t) { &t[..t.len() - 4] } else { t };
    match (n, t.is_empty()) {
        (1, true) => Some(json!("")),
        (_, true) => None,
        _ => Some(json!(t)),
    }
}

/// The edit that turns slot match `n` on or off, as (key, value for `set_json_value`) (F1 spec §3.2). Slot 1 on
/// removes the key (the derived default applies) and off stores `""`; other slots store `path` when on and lose the
/// key when off.
pub fn match_toggle(n: u8, on: bool, path: &str) -> (String, Option<Value>) {
    let v = match (n == 1, on) {
        (true, true) | (false, false) => None,
        (true, false) => Some(json!("")),
        (false, true) => Some(json!(path)),
    };
    (format!("match{n}"), v)
}

/// The path a match on the slot with canonical `suffix` starts from (DECISIONS V1): the derived diffuse path for the
/// diffuse (`""`), else that path with a trailing `_d` cut and `_<suffix>` added. Only `_d` pairs with `_n` in vanilla
/// naming: `facemask.dds` goes with `facemask_n.dds`, so `mask` stays (unlike PGPatcher's prefix rule).
pub fn slot_match_path(derived: &str, suffix: &str) -> String {
    if suffix.is_empty() {
        return derived.to_string();
    }
    let cut = derived.len().saturating_sub(2);
    let base = match derived.get(cut..) {
        Some(d) if d.eq_ignore_ascii_case("_d") => &derived[..cut],
        _ => derived,
    };
    format!("{base}_{suffix}")
}

/// The warning of a set whose entry can never apply (F1 spec §3.2).
pub const NO_MATCH: &str = "this entry matches nothing";

/// No active `matchN` and no `path_contains`: PGPatcher would never apply the entry (DECISIONS V4).
pub fn matches_nothing(s: &Settings, derived: &str) -> bool {
    active_matches(s, derived).is_empty() && text(s, "path_contains").is_none()
}

/// The output prefix (backslashes, no extension) of a set with settings `s` and derived diffuse path `derived`
/// (DECISIONS V3): the lowest active `matchN`, as PGPatcher's first hit gives it, else the derived path.
pub fn prefix_of(s: &Settings, derived: &str) -> Result<String, PrefixError> {
    let rename = text(s, "rename").map(str::trim);
    if let Some(r) = &rename {
        check_json_path(r, "rename")?;
    }
    let matches = active_matches(s, derived);
    for (n, v) in &matches {
        check_json_path(v, &format!("match{n}"))?;
    }
    match matches.first() {
        Some((n, v)) => {
            if let Some(r) = rename {
                return Ok(r.to_string());
            }
            let full = match_value(v);
            let stem = &full[..full.len() - 4];
            Ok(under_pbr(&strip_slot_suffix(stem, *n)))
        }
        None => {
            let d = derived.trim();
            check_json_path(d, "diffuse path")?;
            Ok(under_pbr(rename.unwrap_or(d)))
        }
    }
}

/// Warnings that are never "corrected" (owner, 2026-10-09; F1 DECISIONS V4).
pub fn pitfalls(s: &Settings, derived: &str) -> Vec<String> {
    let mut out = vec![];
    if matches_nothing(s, derived) {
        out.push(format!(
            "{NO_MATCH}, so it is left out of the PGPatcher JSON"
        ));
    }
    if let Some(v) = text(s, "match8") {
        if strip_ext(&v.to_lowercase()).ends_with("_s") {
            out.push(
                "match8 does not strip _s (its list is the old backlight one); use match7 for a subsurface map"
                    .into(),
            );
        }
    }
    if !active_matches(s, derived).is_empty() {
        if let Some(r) = text(s, "rename") {
            let starts_with_textures = backslash(r).to_lowercase().starts_with("textures\\");
            if !starts_with_textures {
                out.push(
                    "rename on a matchN entry replaces the whole prefix, so it must be a full path such as textures\\pbr\\…"
                        .into(),
                );
            }
        }
    }
    out
}
```

`strip_ext` is still used by `pitfalls`. `check_rel_path`, `check_json_path`, `derive_match_diffuse`, `under_pbr`, `text`, `strip_slot_suffix` and `output_rel` are unchanged.

- [ ] **Step 4: Take the old keys out of the schema table**

In `crates/gui/src/pgp/schema.rs`, replace `:9-10` with:

```rust
/// Never written: `delete` is a later round (spec §9); `texture`, `match_diffuse` and `match_normal` are the partial
/// ends-with matches, no longer offered (M6 F1 spec §3.1, DECISIONS V2).
pub const NOT_EXPOSED: &[&str] = &["texture", "delete", "match_diffuse", "match_normal"];
```

Delete the two `KEYS` entries `k("match_diffuse", …)` and `k("match_normal", …)` (`:77-78`).

- [ ] **Step 5: Write `matchN` in the writer**

In `crates/gui/src/pgp/writer.rs`:
- Replace `use super::prefix::{match_mode, MatchMode};` with `use super::prefix::{active_matches, match_value};`.
- In `object`, replace `let mode = match_mode(own);` with `let matches = active_matches(own, derived);`.
- Replace the whole `KeyKind::Text => { … }` arm with:

```rust
            KeyKind::Text => match k.key.strip_prefix("match").and_then(|n| n.parse::<u8>().ok()) {
                // matchN: the active slot matches only, in the JSON form (DECISIONS V2).
                Some(n) => matches
                    .iter()
                    .find(|(m, _)| *m == n)
                    .map(|(_, v)| Node::Scalar(json!(match_value(v)))),
                None => v
                    .and_then(Value::as_str)
                    .filter(|s| !s.is_empty())
                    .map(|s| Node::Scalar(json!(s))),
            },
```

Update the doc comment of `EntryInput::derived_match` to `/// The derived diffuse path: \`match1\` while the set has no \`match1\` key (DECISIONS V2).`

- [ ] **Step 6: Keep no-match sets out of the JSON, and ask**

In `crates/gui/src/model/pbr_export.rs`:
- Change the prefix import (`:8`) to `use crate::pgp::prefix::{check_json_path, check_rel_path, matches_nothing, output_rel, pitfalls};`.
- In `json_entries`, replace `.filter(|v| v.pbr)` with:

```rust
        // An entry with no match could never apply, and the schema refuses one (F1 spec §3.2, DECISIONS V4).
        .filter(|v| v.pbr && !matches_nothing(&v.settings, v.derived_match.as_deref().unwrap_or("")))
```

- In `export_asks`, replace `pitfalls(&v.settings)` with `pitfalls(&v.settings, v.derived_match.as_deref().unwrap_or(""))`.

- [ ] **Step 7: Keep `pgp_fields.rs` compiling**

In `crates/gui/src/ui/pgp_fields.rs`:
- Delete `use crate::pgp::prefix::{match_mode, MatchMode};`.
- In `advanced`, delete the `let mode = match match_mode(&c.own) { … };` block and `ui.label(format!("Matching by: {mode}"));`.
- In the Text arm, change the two hint arms to `("match1", Some(d)) => format!("{d} (derived)"),` and `("match1", None) => "(derived)".to_string(),`.

- [ ] **Step 8: Run the tests to verify they pass**

Run each:
- `cargo test -p sdc-gui --offline --test pgp`
- `cargo test -p sdc-gui --offline --test sets_model`
- `cargo test -p sdc-gui --offline --test pbr_export`
- `cargo test -p sdc-gui --offline --test m3b_flow`
- `cargo test -p sdc-gui --offline --test set_preview_model`
- `cargo test -p sdc-gui --offline --test set_editor_ui`

Expected: PASS.

If another test fails *only* because `match_diffuse` is no longer written or read, change that key to `match1` (value + `.dds` where it reads the JSON) and name the test in the report. Do not change fixtures.

- [ ] **Step 9: Run all gates, then commit**

```bash
git add crates/gui/src/pgp/prefix.rs crates/gui/src/pgp/schema.rs crates/gui/src/pgp/writer.rs crates/gui/src/model/pbr_export.rs crates/gui/src/ui/pgp_fields.rs crates/gui/tests/pgp.rs crates/gui/tests/sets_model.rs crates/gui/tests/pbr_export.rs crates/gui/tests/m3b_flow.rs crates/gui/tests/set_preview_model.rs crates/gui/tests/set_editor_ui.rs
git commit -m "feat(gui): exact slot matching; match1 is the derived diffuse path with .dds (M6 Track F1)

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

---

### Task 4: The Matching header and the card toggles

**Spec:** §3.2, §6 "Toggles". Rulings V1, V2, V4.

**Files:**
- Modify: `crates/gui/src/model/sets.rs`:
  - the import (`:6`);
  - add `SlotMatch` and `slot_matches` after `slot_label` (`:100`).
- Modify: `crates/gui/src/ui/pgp_fields.rs`:
  - imports;
  - `Ctx` (`:97-106`);
  - `context` (`:108-127`);
  - `panel` (`:130-145`);
  - a new `matching` fn.
- Modify: `crates/gui/src/ui/set_editor.rs`:
  - imports;
  - `Act` (`:25-32`);
  - the header (`:52-69`);
  - the action loop (`:95-106`);
  - `card` (`:179-290`).
- Test: `crates/gui/tests/sets_model.rs` (one test), `crates/gui/tests/set_editor_ui.rs` (four tests).

**Interfaces:**
- Consumes:
  - `active_matches`, `match_toggle`, `stored_match`, `slot_match_path`, `matches_nothing`, `NO_MATCH` (Task 3);
  - `panel_harness`, `rock_settings`, `rock_selected`, `harness` (test helpers in `set_editor_ui.rs`).
- Produces, in `sdc_gui::model::sets`:
  - `#[derive(Debug, Clone, PartialEq, Eq)] pub struct SlotMatch { pub n: u8, pub slot: String, pub path: String }`;
  - `pub fn slot_matches(v: &SetView) -> (Vec<SlotMatch>, Vec<SlotMatch>)`: (active rows, + menu).

- [ ] **Step 1: Write the failing tests**

At the end of `crates/gui/tests/sets_model.rs`:

```rust
/// F1 spec §3.2: the Matching rows are the active matches; the + menu offers filled slots 3–8 not matched yet.
#[test]
fn the_matching_rows_and_the_plus_menu_follow_the_filled_slots() {
    let d = tempfile::tempdir().unwrap();
    let mut m = pbr_opened(d.path());
    idle(&mut m);
    let (active, add) = slot_matches(view(&set_views(&m), "landscape/rock01"));
    assert_eq!(
        active,
        vec![SlotMatch {
            n: 1,
            slot: "Diffuse".into(),
            path: r"landscape\rock01".into()
        }]
    );
    assert_eq!(
        add,
        vec![SlotMatch {
            n: 6,
            slot: "RMAOS".into(),
            path: r"landscape\rock01_rmaos".into()
        }]
    );
    m.set_json_value("landscape/rock01", "match6", Some(json!(r"landscape\rock01_rmaos")));
    m.set_json_value("landscape/rock01", "match1", Some(json!("")));
    idle(&mut m);
    let (active, add) = slot_matches(view(&set_views(&m), "landscape/rock01"));
    assert_eq!(active.iter().map(|a| a.n).collect::<Vec<_>>(), vec![6]);
    assert!(add.is_empty());
}
```

In `crates/gui/tests/set_editor_ui.rs`, after `a_hand_edited_negative_number_shows_as_it_is_and_does_not_dirty_the_project`:

```rust
fn text_inputs(h: &Harness<'static, AppModel>) -> Vec<String> {
    h.query_all_by_role(eframe::egui::accesskit::Role::TextInput)
        .filter_map(|n| n.value())
        .collect()
}

/// F1 spec §3.2: the Matching header lists the default match1 and says how the rows combine.
#[test]
fn the_matching_header_lists_the_default_match1() {
    let d = tempfile::tempdir().unwrap();
    let mut h = panel_harness(pbr_opened(d.path()));
    h.run();
    h.get_by_label("Matching");
    h.get_by_label("The entry applies when any of these matches.");
    assert!(
        text_inputs(&h).contains(&r"landscape\rock01".to_string()),
        "{:?}",
        text_inputs(&h)
    );
    assert!(h.query_by_label_contains("matches nothing").is_none());
}

fn match_toggles(h: &Harness<'static, AppModel>) -> Vec<bool> {
    use eframe::egui::accesskit::Toggled;
    h.query_all_by_label("Match on this")
        .map(|n| n.accesskit_node().toggled() == Some(Toggled::True))
        .collect()
}

/// F1 spec §3.2, §6 "Toggles": the diffuse card is on by default and the normal card off. Removing the match1 row
/// turns the diffuse toggle off and warns; the normal toggle adds match2.
#[test]
fn the_card_toggles_mirror_the_matching_rows() {
    let d = tempfile::tempdir().unwrap();
    let mut m = pbr_opened(d.path());
    rock_selected(&mut m);
    let mut h = harness(m);
    h.run();
    assert_eq!(match_toggles(&h), vec![true, false], "diffuse, normal");
    // The Matching header sits below the visual groups: bring it into view before a pointer click.
    h.get_by_label("×").scroll_to_me();
    h.run();
    h.get_by_label("×").click();
    h.run();
    assert_eq!(rock_settings(&h)["match1"], serde_json::json!(""));
    assert_eq!(match_toggles(&h), vec![false, false]);
    h.get_by_label_contains("this entry matches nothing");
    h.query_all_by_label("Match on this").nth(1).unwrap().scroll_to_me();
    h.run();
    h.query_all_by_label("Match on this").nth(1).unwrap().click();
    h.run();
    assert_eq!(
        rock_settings(&h)["match2"],
        serde_json::json!(r"landscape\rock01_n")
    );
    assert!(text_inputs(&h).contains(&r"landscape\rock01_n".to_string()));
    h.query_all_by_label("Match on this").next().unwrap().scroll_to_me();
    h.run();
    h.query_all_by_label("Match on this").next().unwrap().click();
    h.run();
    assert!(!rock_settings(&h).contains_key("match1"), "back to the derived default");
    assert_eq!(match_toggles(&h), vec![true, true]);
}

/// F1 spec §6 "Toggles": + offers slots 3–8 only when the set has a texture there.
#[test]
fn the_plus_menu_offers_only_filled_slots_3_to_8() {
    let d = tempfile::tempdir().unwrap();
    let mut h = panel_harness(pbr_opened(d.path()));
    h.run();
    h.get_by_label("+").click();
    h.run();
    assert!(h.query_by_label_contains("Emissive").is_none(), "slot 3 is empty");
    h.get_by_label("6 · RMAOS").click();
    h.run();
    assert_eq!(
        rock_settings(&h)["match6"],
        serde_json::json!(r"landscape\rock01_rmaos")
    );
}

/// F1 Review Focus 4 (plan): leaving a row unchanged writes nothing: the default match1 does not become a key, and
/// the project stays clean.
#[test]
fn leaving_a_matching_row_unchanged_writes_nothing() {
    let d = tempfile::tempdir().unwrap();
    let mut m = pbr_opened(d.path());
    m.save().unwrap();
    let mut h = panel_harness(m);
    h.run();
    h.query_all_by_role(eframe::egui::accesskit::Role::TextInput)
        .find(|n| n.value().as_deref() == Some(r"landscape\rock01"))
        .expect("the match1 row")
        .focus();
    h.run();
    h.key_press(eframe::egui::Key::Enter);
    h.run();
    assert!(!rock_settings(&h).contains_key("match1"));
    assert!(!h.state().sets_doc.as_ref().unwrap().is_dirty());
}
```

- [ ] **Step 2: Run them to verify they fail**

Run: `cargo test -p sdc-gui --offline --test sets_model the_matching_rows`
Expected: a compile FAIL. `slot_matches` and `SlotMatch` are not found.
Run: `cargo test -p sdc-gui --offline --test set_editor_ui -- matching card_toggles plus_menu leaving`
Expected: FAIL. There is no "Matching" label and no "Match on this".

- [ ] **Step 3: Add `slot_matches` to the model**

In `crates/gui/src/model/sets.rs`, change the import at `:6` to:

```rust
use crate::pgp::prefix::{
    active_matches, check_rel_path, derive_match_diffuse, output_rel, prefix_of, slot_match_path,
};
```

After `slot_label` (`:100`), add:

```rust
/// One row of the Matching header, or one choice of its + menu (F1 spec §3.2): slot number, slot name, path (stored
/// form, no `.dds`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SlotMatch {
    pub n: u8,
    pub slot: String,
    pub path: String,
}

/// The Matching header of `v`: its active slot matches, and the + menu, which offers the filled slots 3–8 not matched
/// yet, starting from `slot_match_path` (DECISIONS V1).
pub fn slot_matches(v: &SetView) -> (Vec<SlotMatch>, Vec<SlotMatch>) {
    let derived = v.derived_match.as_deref().unwrap_or("");
    let filled: Vec<SlotKind> = v
        .slots
        .iter()
        .filter(|(_, f)| !matches!(f, SlotFill::Empty))
        .map(|(k, _)| *k)
        .collect();
    let name = |n: u8| match filled.iter().find(|k| k.slot_number() == n) {
        Some(k) => k.label().to_string(),
        None if n == 1 => SlotKind::Diffuse.label().to_string(),
        None if n == 2 => SlotKind::Normal.label().to_string(),
        None => format!("Slot {n}"),
    };
    let active: Vec<SlotMatch> = active_matches(&v.settings, derived)
        .into_iter()
        .map(|(n, path)| SlotMatch {
            n,
            slot: name(n),
            path,
        })
        .collect();
    let add = filled
        .iter()
        .filter(|k| (3..=8).contains(&k.slot_number()))
        .filter(|k| !active.iter().any(|a| a.n == k.slot_number()))
        .map(|k| SlotMatch {
            n: k.slot_number(),
            slot: k.label().to_string(),
            path: slot_match_path(derived, k.suffix()),
        })
        .collect();
    (active, add)
}
```

- [ ] **Step 4: Draw the Matching header**

In `crates/gui/src/ui/pgp_fields.rs`:
- Change the sets import to `use crate::model::sets::{set_views, slot_matches, SetView, SlotFill, SlotKind, SlotMatch};`.
- Add `use crate::pgp::prefix::{match_toggle, matches_nothing, stored_match, NO_MATCH};`.

Replace `Ctx` and `context` with:

```rust
struct Ctx {
    /// The set's own settings (there are no inherited ones: the `default` block is fixed).
    own: Settings,
    derived: Option<String>,
    set: String,
    /// Why every control is disabled, when the sets document is read-only.
    read_only: Option<String>,
    coat_filled: bool,
    fuzz_filled: bool,
    /// The Matching header (F1 spec §3.2): the active slot matches, the + menu, and whether nothing matches.
    matches: Vec<SlotMatch>,
    addable: Vec<SlotMatch>,
    no_match: bool,
}

fn context(model: &AppModel, set: &str) -> Ctx {
    let v = set_views(model).into_iter().find(|v| same_id(&v.id, set));
    let filled =
        |v: &SetView, s: SlotKind| !matches!(v.slots.get(&s), None | Some(SlotFill::Empty));
    let editable = model.sets_doc.as_ref().is_some_and(|d| !d.read_only());
    let own = v.as_ref().map(|v| v.settings.clone()).unwrap_or_default();
    let derived = v.as_ref().and_then(|v| v.derived_match.clone());
    let (matches, addable) = v.as_ref().map(slot_matches).unwrap_or_default();
    Ctx {
        no_match: matches_nothing(&own, derived.as_deref().unwrap_or("")),
        coat_filled: v.as_ref().is_some_and(|v| filled(v, SlotKind::Coat)),
        fuzz_filled: v.as_ref().is_some_and(|v| filled(v, SlotKind::Fuzz)),
        own,
        derived,
        matches,
        addable,
        set: set.to_string(),
        read_only: (!editable).then(|| {
            model
                .sets_doc
                .as_ref()
                .and_then(|d| d.error())
                .map_or_else(|| READ_ONLY.into(), str::to_string)
        }),
    }
}
```

In `panel`, after the `for g in MAIN { … }` loop and before `for (k, v) in edits`, add:

```rust
    matching(ui, &c, &mut edits);
```

After `fn group` (`:163-183`), add:

```rust
/// The Matching header (F1 spec §3.2): one row per slot match (slot, exact path, ×), a + menu for filled slots 3–8,
/// and the warning when nothing matches. The slot 1 and 2 rows are the card toggles' state.
fn matching(ui: &mut egui::Ui, c: &Ctx, edits: &mut Vec<Edit>) {
    ui.label(egui::RichText::new("Matching").strong());
    ui.label(
        egui::RichText::new("The entry applies when any of these matches.")
            .small()
            .weak(),
    );
    let r = ui.add_enabled_ui(c.read_only.is_none(), |ui| {
        egui::Grid::new(("pgp-matching", &c.set))
            .num_columns(3)
            .show(ui, |ui| {
                for m in &c.matches {
                    ui.label(&m.slot);
                    let id = egui::Id::new(("pgp-match", m.n, &c.set));
                    let mut text = ui
                        .data(|d| d.get_temp::<String>(id))
                        .unwrap_or_else(|| m.path.clone());
                    let r = ui.add(egui::TextEdit::singleline(&mut text).desired_width(220.0));
                    if r.changed() {
                        ui.data_mut(|d| d.insert_temp(id, text.clone()));
                    }
                    // Committed on Enter or focus loss; an unchanged path is no edit (F1 Review Focus 4).
                    if r.lost_focus() {
                        ui.data_mut(|d| d.remove::<String>(id));
                        if text.trim() != m.path {
                            edits.push((format!("match{}", m.n), stored_match(m.n, &text)));
                        }
                    }
                    if ui
                        .small_button("×")
                        .on_hover_text("Stop matching on this slot")
                        .clicked()
                    {
                        edits.push(match_toggle(m.n, false, &m.path));
                    }
                    ui.end_row();
                }
            });
        if !c.addable.is_empty() {
            ui.menu_button("+", |ui| {
                for a in &c.addable {
                    if ui.button(format!("{} · {}", a.n, a.slot)).clicked() {
                        edits.push(match_toggle(a.n, true, &a.path));
                        ui.close();
                    }
                }
            });
        }
    });
    if let Some(why) = &c.read_only {
        r.response.on_disabled_hover_text(why);
    }
    if c.no_match {
        ui.colored_label(AMBER, NO_MATCH);
    }
    ui.add_space(6.0);
}
```

- [ ] **Step 5: Add the card toggles, and drop the stale header line**

In `crates/gui/src/ui/set_editor.rs`:
- Add the imports `use crate::pgp::prefix::{active_matches, match_toggle, slot_match_path};` and `use serde_json::Value;`.
- Add a variant to `Act`:

```rust
    /// A PGPatcher setting of a set: (set id, key, value; `None` removes it).
    Json(String, String, Option<Value>),
```

- In `draw`'s action loop, add the arm `Act::Json(set, k, v) => model.set_json_value(&set, &k, v),`.
- Replace the header block `:52-69` with:

```rust
    ui.horizontal_wrapped(|ui| {
        ui.heading(&v.label);
        match (&v.prefix, &v.prefix_error) {
            (Some(p), _) => ui.weak(format!("→ {p}")),
            (_, Some(e)) => ui.colored_label(RED, e),
            _ => ui.weak(""),
        };
    });
```

- In `card`, directly after `ui.add(egui::Label::new(egui::RichText::new(&i.name).small()).truncate());` (`:231`) and before `if let Some(from) = &borrowed`, add:

```rust
            if let Some(n) = match slot {
                SlotKind::Diffuse => Some(1),
                SlotKind::Normal => Some(2),
                _ => None,
            } {
                // F1 spec §3.2: the card's switch is the Matching header's slot 1/2 row.
                let derived = v.derived_match.as_deref().unwrap_or("");
                let on = active_matches(&v.settings, derived)
                    .iter()
                    .any(|(m, _)| *m == n);
                let mut want = on;
                let editable = model.sets_doc.as_ref().is_some_and(|d| !d.read_only());
                if ui
                    .add_enabled(editable, egui::Checkbox::new(&mut want, "Match on this"))
                    .on_hover_text("PGPatcher applies the entry to meshes with this exact texture in this slot")
                    .changed()
                {
                    let (k, val) = match_toggle(n, want, &slot_match_path(derived, slot.suffix()));
                    acts.push(Act::Json(v.id.clone(), k, val));
                }
            }
```

- [ ] **Step 6: Run the tests to verify they pass**

Run: `cargo test -p sdc-gui --offline --test sets_model --test set_editor_ui`
Expected: PASS.

- [ ] **Step 7: Run all gates, then commit**

```bash
git add crates/gui/src/model/sets.rs crates/gui/src/ui/pgp_fields.rs crates/gui/src/ui/set_editor.rs crates/gui/tests/sets_model.rs crates/gui/tests/set_editor_ui.rs
git commit -m "feat(gui): Matching header and Match on this toggles on the diffuse and normal cards (M6 Track F1)

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

---

### Task 5: TruePBR box layout — Advanced TruePBR settings

**Spec:** §4, §6 "Layout". Ruling V7.

**Files:**
- Modify: `crates/gui/src/pgp/schema.rs`, the `advanced` flag of:
  - `path_contains`, `nif_filter` and `rename` → `true`;
  - `match1`–`match8` → `false`.
- Modify: `crates/gui/src/ui/pgp_fields.rs`:
  - `MAIN` (`:75-85`);
  - `Ctx`/`context` (drop `derived`);
  - `panel`;
  - delete `pub fn advanced` (`:146-161`);
  - the Text hint.
- Modify: `crates/gui/src/ui/advanced.rs`: delete `let set_id = selected_set_id(model);` (`:51`), the `if let Some(id) = &set_id { … }` block (`:64-71`) and `fn selected_set_id` (`:98-107`).
- Test: `crates/gui/tests/set_editor_ui.rs`:
  - rewrite `the_header_has_the_json_name_and_the_drawer_has_the_advanced_keys`;
  - fix `the_tri_buttons_write_true_false_and_remove`;
  - one new test.

**Interfaces:**
- Consumes: `matching` and `Ctx` (Task 4).
- Produces:
  - `pgp_fields::advanced` no longer exists.
  - `pgp_fields::panel` draws three parts: the visual groups, then "Matching", then a collapsed "Advanced TruePBR settings".

- [ ] **Step 1: Write the failing tests**

In `crates/gui/tests/set_editor_ui.rs`, replace `the_header_has_the_json_name_and_the_drawer_has_the_advanced_keys` (`:619-634`) with:

```rust
/// F1 spec §4: the drawer no longer shows PGPatcher keys; they are in the TruePBR box.
#[test]
fn the_header_has_the_json_name_and_the_drawer_has_no_pgpatcher_keys() {
    let d = tempfile::tempdir().unwrap();
    let mut m = pbr_opened(d.path());
    rock_selected(&mut m);
    m.settings.advanced_open = true;
    let mut h = harness(m);
    h.run();
    h.get_by_label("JSON:");
    assert!(h.query_by_label_contains("PGPatcher (advanced)").is_none());
    h.get_by_label("Advanced TruePBR settings");
}

/// F1 spec §4, §6 "Layout": path_contains, nif_filter, rename and the locks are under "Advanced TruePBR settings",
/// collapsed by default; match1–match8 are only in the Matching header.
#[test]
fn advanced_truepbr_settings_are_collapsed_and_hold_the_advanced_keys() {
    let d = tempfile::tempdir().unwrap();
    let mut h = panel_harness(pbr_opened(d.path()));
    h.run();
    for l in ["Path contains", "Mesh path contains", "Rename", "Lock diffuse"] {
        assert!(h.query_by_label(l).is_none(), "{l} is hidden while collapsed");
    }
    h.get_by_label("Advanced TruePBR settings").click();
    h.run();
    for l in ["Path contains", "Mesh path contains", "Rename", "Lock diffuse"] {
        h.get_by_label(l);
    }
    assert!(h.query_by_label_contains("Match slot").is_none());
}
```

In `the_tri_buttons_write_true_false_and_remove` (`:413-433`):
- Replace `h.state_mut().settings.advanced_open = true;` with:

  ```rust
      // The header and its rows are low in the set editor's scroll area: bring each into view before clicking.
      h.get_by_label("Advanced TruePBR settings").scroll_to_me();
      h.run();
      h.get_by_label("Advanced TruePBR settings").click();
      h.run();
      h.query_all_by_label("on").next().unwrap().scroll_to_me();
  ```

- Change the comment `// The first Tri row of the Advanced drawer is vertex_colors.` to `// The first Tri row under Advanced TruePBR settings is vertex_colors.`

- [ ] **Step 2: Run them to verify they fail**

Run: `cargo test -p sdc-gui --offline --test set_editor_ui -- drawer_has_no advanced_truepbr tri_buttons`
Expected: FAIL. There is no "Advanced TruePBR settings", and "PGPatcher (advanced)" is still in the drawer.

- [ ] **Step 3: Move the keys and build the header**

In `crates/gui/src/pgp/schema.rs`:
- Set `advanced` to `true` for `path_contains`, `nif_filter` and `rename`.
- Set `advanced` to `false` for `match1`–`match8`.
- Add above `match1`: `// match1–match8 are drawn by the Matching header only (F1 spec §3.2), so no group shows them.`

In `crates/gui/src/ui/pgp_fields.rs`, replace `MAIN` with:

```rust
/// The visual settings (F1 spec §4 header 1). Matching and the advanced keys have their own headers.
const MAIN: [Group; 8] = [
    Group::Material,
    Group::Features,
    Group::Emissive,
    Group::Parallax,
    Group::Subsurface,
    Group::Multilayer,
    Group::Glint,
    Group::Fuzz,
];
```

Keep `ADVANCED` as it is. Delete `pub fn advanced` (`:146-161`).

In `panel`, after `matching(ui, &c, &mut edits);`, add:

```rust
    egui::CollapsingHeader::new(egui::RichText::new("Advanced TruePBR settings").strong())
        .id_salt(("pgp-advanced", &c.set))
        .default_open(false)
        .show(ui, |ui| {
            for g in ADVANCED {
                if !group_open(g, &c.own) {
                    continue;
                }
                let keys: Vec<&KeySpec> =
                    KEYS.iter().filter(|k| k.group == g && k.advanced).collect();
                group(ui, &c, "pgp-adv", g, &keys, &mut edits);
            }
        });
```

Change the doc comment of `panel` to `/// The TruePBR box of set \`set\` (F1 spec §4): the visual settings, Matching, and Advanced TruePBR settings.`

Remove `derived` from `Ctx`: delete the field and the `derived,` line in `context`. Keep the local `let derived` used by `no_match`.

Replace the Text arm's hint with:

```rust
            // With a slot match, rename replaces the whole prefix (DECISIONS V3).
            let hint = if k.key == "rename" {
                r"textures\pbr\… (the whole prefix)"
            } else {
                ""
            };
```

The `.hint_text(hint)` call stays.

In `crates/gui/src/ui/advanced.rs`:
- Delete `let set_id = selected_set_id(model);`.
- Delete the block

  ```rust
                    if let Some(id) = &set_id {
                        egui::CollapsingHeader::new(
                            egui::RichText::new("PGPatcher (advanced)").strong(),
                        )
                        .default_open(true)
                        .show(ui, |ui| super::pgp_fields::advanced(ui, model, id));
                    }
  ```

- Delete `fn selected_set_id`. `Selection` is still used by `selected_job`.

- [ ] **Step 4: Run the tests to verify they pass**

Run: `cargo test -p sdc-gui --offline --test set_editor_ui --test advanced --test packing_fills`
Expected: PASS.

- [ ] **Step 5: Run all gates, then commit**

```bash
git add crates/gui/src/pgp/schema.rs crates/gui/src/ui/pgp_fields.rs crates/gui/src/ui/advanced.rs crates/gui/tests/set_editor_ui.rs
git commit -m "feat(gui): Advanced TruePBR settings header in the TruePBR box; the drawer drops PGPatcher keys (M6 Track F1)

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

---

### Task 6: Slot cards — usable slots in slot order

**Spec:** §5, §6 "Slot row", Review Focus 4. Ruling V8.

**Files:**
- Modify: `crates/gui/src/model/sets.rs`:
  - `slot_of` (`:178-200`) becomes `slot_kind` + `slot_of`;
  - add `usable_slots`, `SlotCell`, `slot_row`.
- Modify: `crates/gui/src/ui/set_editor.rs`: imports, `cards` (`:157-177`), and `card`'s signature, frame and title.
- Test: `crates/gui/tests/sets_model.rs` (two tests), `crates/gui/tests/set_editor_ui.rs` (one rewrite, one new test).

**Interfaces:**
- Consumes: `SetView::shader: Option<ShaderPick>` (Track A).
- Produces, in `sdc_gui::model::sets`:
  - `pub fn slot_kind(st: Subtype) -> Option<SlotKind>`;
  - `pub fn usable_slots(s: Shader) -> BTreeSet<SlotKind>`;
  - `#[derive(Debug, Clone, PartialEq, Eq)] pub struct SlotCell { pub kind: SlotKind, pub fill: SlotFill, pub usable: bool }`;
  - `pub fn slot_row(pick: Option<&ShaderPick>, slots: &BTreeMap<SlotKind, SlotFill>) -> Vec<SlotCell>`.

- [ ] **Step 1: Write the failing tests**

At the end of `crates/gui/tests/sets_model.rs`:

```rust
/// F1 spec §5, DECISIONS V8: a PBR set's cards are the PBR slots in PGPatcher order 1–8, empty or not.
#[test]
fn a_pbr_set_shows_its_pbr_slots_in_slot_order() {
    use engine::shader::Shader;
    let d = tempfile::tempdir().unwrap();
    let mut m = pbr_opened(d.path());
    idle(&mut m);
    let v = view(&set_views(&m), "landscape/rock01").clone();
    assert_eq!(v.shader.as_ref().map(|p| p.shader), Some(Shader::Pbr));
    let row = slot_row(v.shader.as_ref(), &v.slots);
    assert_eq!(
        row.iter().map(|c| c.kind).collect::<Vec<_>>(),
        vec![
            SlotKind::Diffuse,
            SlotKind::Normal,
            SlotKind::Emissive,
            SlotKind::Height,
            SlotKind::Rmaos,
            SlotKind::Coat,
            SlotKind::Fuzz,
            SlotKind::Subsurface,
        ]
    );
    assert!(row.iter().all(|c| c.usable));
    let set = |k: &[SlotKind]| k.iter().copied().collect::<std::collections::BTreeSet<_>>();
    assert_eq!(
        usable_slots(Shader::AdvancedSkin),
        set(&[SlotKind::Diffuse, SlotKind::Normal, SlotKind::SkinSk, SlotKind::Rmaos])
    );
    assert_eq!(
        usable_slots(Shader::Glow),
        set(&[SlotKind::Diffuse, SlotKind::Normal, SlotKind::Emissive])
    );
    assert_eq!(
        usable_slots(Shader::Eye),
        set(&[SlotKind::Diffuse, SlotKind::Normal])
    );
}

/// F1 spec §5, Review Focus 4: a SHADER-UNAVAILABLE fallback (default in place of pbr) keeps the filled RMAOS
/// visible, in slot order and marked; with no pick, only the filled slots show.
#[test]
fn a_fallback_shader_keeps_filled_slots_it_cannot_use() {
    use engine::shader::{Shader, ShaderOrigin, ShaderPick};
    use std::collections::BTreeMap;
    let own = |i: &str| SlotFill::Own { item: i.into() };
    let slots: BTreeMap<SlotKind, SlotFill> = [
        (SlotKind::Rmaos, own("r")),
        (SlotKind::Diffuse, own("d")),
    ]
    .into_iter()
    .collect();
    let fallback = ShaderPick {
        shader: Shader::Default,
        provenance: ShaderOrigin::Fallback,
        wanted: Some(Shader::Pbr),
    };
    let row = slot_row(Some(&fallback), &slots);
    assert_eq!(
        row,
        vec![
            SlotCell { kind: SlotKind::Diffuse, fill: own("d"), usable: true },
            SlotCell { kind: SlotKind::Normal, fill: SlotFill::Empty, usable: true },
            SlotCell { kind: SlotKind::Rmaos, fill: own("r"), usable: false },
        ]
    );
    let none = slot_row(None, &slots);
    assert_eq!(
        none.iter().map(|c| (c.kind, c.usable)).collect::<Vec<_>>(),
        vec![(SlotKind::Diffuse, true), (SlotKind::Rmaos, true)]
    );
}
```

In `crates/gui/tests/set_editor_ui.rs`, `the_set_editor_shows_slot_cards_opaque_and_slot_commands` (`:94-134`):
- Remove `"Skin _sk",` from the label list.
- After the `for label in [...] { h.get_by_label(label); }` loop, add:

```rust
    assert!(
        h.query_by_label("Skin _sk").is_none(),
        "the PBR shader has no _sk slot (F1 spec §5)"
    );
```

At the end of `crates/gui/tests/set_editor_ui.rs`, add:

```rust
/// F1 spec §5: a filled slot the set's shader cannot use stays, and says why on hover.
#[test]
fn a_filled_slot_the_shader_cannot_use_says_why() {
    let d = tempfile::tempdir().unwrap();
    let mut m = pbr_opened(d.path());
    m.set_shader("landscape/rock01", Some(engine::shader::Shader::Default));
    assert!(m.wait_idle(std::time::Duration::from_secs(60)));
    let v = sdc_gui::model::sets::set_views(&m)
        .into_iter()
        .find(|v| v.id == "landscape/rock01")
        .unwrap();
    assert_eq!(v.shader.as_ref().map(|p| p.shader), Some(engine::shader::Shader::Default));
    assert!(
        v.slots.contains_key(&sdc_gui::model::sets::SlotKind::Rmaos),
        "the fixture still packs _rmaos under the Default shader: {:?}",
        v.slots
    );
    rock_selected(&mut m);
    let mut h = harness(m);
    instant_tooltips(&h);
    h.run();
    assert!(h.query_by_label("Height _p").is_none(), "Default has no height slot");
    let rmaos = h.query_all_by_label("RMAOS").next().expect("the RMAOS card");
    rmaos.hover();
    h.run();
    h.run();
    h.get_by_label_contains("the Default shader does not use slot 6");
}
```

- [ ] **Step 2: Run them to verify they fail**

Run: `cargo test -p sdc-gui --offline --test sets_model -- pbr_set_shows fallback_shader`
Expected: a compile FAIL. `slot_row`, `SlotCell` and `usable_slots` are not found.
Run: `cargo test -p sdc-gui --offline --test set_editor_ui -- slot_cards_opaque filled_slot_the_shader`
Expected: FAIL. "Skin _sk" is still shown, and there is no hover text.

- [ ] **Step 3: Implement the slot row in the model**

In `crates/gui/src/model/sets.rs`, change the doc comment of `SlotKind::ALL` (`:34`) to `/// Every slot kind, in the slot commands' order. The set editor orders its cards by \`slot_row\` (F1 spec §5).`

Then replace `slot_of` (`:178-200`) with:

```rust
/// The slot card a resolved sub-type fills; `None` for sub-types without a card (env mask, complex `_m`,
/// `msn_specular`, backlight, `skin_wet`, …).
pub fn slot_kind(st: Subtype) -> Option<SlotKind> {
    Some(match st {
        Subtype::PbrAlbedo | Subtype::Diffuse | Subtype::LandscapeDiffuse | Subtype::SkinAlbedo => {
            SlotKind::Diffuse
        }
        Subtype::PbrNormal | Subtype::Normal | Subtype::LandscapeNormal | Subtype::Msn => {
            SlotKind::Normal
        }
        Subtype::PbrEmissive | Subtype::Glow => SlotKind::Emissive,
        Subtype::SkinSk => SlotKind::SkinSk,
        Subtype::Height => SlotKind::Height,
        Subtype::Rmaos | Subtype::SkinRfaos => SlotKind::Rmaos,
        Subtype::CoatNormal => SlotKind::Coat,
        Subtype::PbrFuzz => SlotKind::Fuzz,
        Subtype::PbrSubsurface => SlotKind::Subsurface,
        _ => return None,
    })
}

/// The slot a texture fills, by its resolved sub-type. Raw packing sources and items without a job have none.
pub fn slot_of(item: &TextureItem) -> Option<SlotKind> {
    if matches!(item.status, ItemStatus::PackedInto(_)) {
        return None;
    }
    slot_kind(item.job.as_ref()?.profile.subtype)
}

/// The slot cards shader `s` can use (F1 spec §5, DECISIONS V8): each role's sub-types mapped through `slot_kind`.
/// Roles matched by suffix only (`_e`, `_i`, `_sk` on Skin/Eye) map to none.
pub fn usable_slots(s: Shader) -> BTreeSet<SlotKind> {
    let spec = s.spec();
    spec.required
        .iter()
        .chain(spec.optional)
        .flat_map(|r| r.subtypes.iter().copied())
        .filter_map(slot_kind)
        .collect()
}

/// One slot card of the set editor (F1 spec §5).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SlotCell {
    pub kind: SlotKind,
    pub fill: SlotFill,
    /// False for a filled slot the set's shader does not use: it stays, outlined, and says why.
    pub usable: bool,
}

/// The set's slot cards in PGPatcher slot order 1–8 (slots 3 and 7 hold two kinds each, in `SlotKind` order). These
/// are every slot the picked shader can use, filled or empty, plus every filled slot it cannot use. With no pick yet,
/// only the filled slots show (F1 spec §5).
pub fn slot_row(pick: Option<&ShaderPick>, slots: &BTreeMap<SlotKind, SlotFill>) -> Vec<SlotCell> {
    let usable = pick.map(|p| usable_slots(p.shader));
    let filled = |k: &SlotKind| !matches!(slots.get(k), None | Some(SlotFill::Empty));
    let mut kinds: Vec<SlotKind> = SlotKind::ALL
        .into_iter()
        .filter(|k| filled(k) || usable.as_ref().is_some_and(|u| u.contains(k)))
        .collect();
    kinds.sort_by_key(|k| (k.slot_number(), *k));
    kinds
        .into_iter()
        .map(|kind| SlotCell {
            kind,
            fill: slots.get(&kind).cloned().unwrap_or(SlotFill::Empty),
            usable: usable.as_ref().is_none_or(|u| u.contains(&kind)),
        })
        .collect()
}
```

- [ ] **Step 4: Draw the cards from `slot_row`**

In `crates/gui/src/ui/set_editor.rs`, change the sets import to:

```rust
use crate::model::sets::{set_views, slot_label, slot_of, slot_row, SetView, SlotCell, SlotFill, SlotKind};
```

In `cards`, replace the loop `for (i, slot) in SlotKind::ALL.iter().enumerate() { card(ui, model, v, *slot, thumbs, acts); … }` with:

```rust
            for (i, cell) in slot_row(v.shader.as_ref(), &v.slots).iter().enumerate() {
                card(ui, model, v, cell, thumbs, acts);
                if i % per_row == per_row - 1 {
                    ui.end_row();
                }
            }
```

In `card`:
- Change the parameter `slot: SlotKind` to `cell: &SlotCell`.
- Replace the first lines up to `egui::Frame::group(ui.style()).show(ui, |ui| {` with:

```rust
    let slot = cell.kind;
    let fill = cell.fill.clone();
    let frame = egui::Frame::group(ui.style());
    // A filled slot the shader cannot use stays at its place, outlined, and says why on hover (F1 spec §5).
    let frame = if cell.usable {
        frame
    } else {
        frame.stroke(egui::Stroke::new(2.0, AMBER))
    };
    frame.show(ui, |ui| {
```

- Replace `ui.label(egui::RichText::new(title).strong());` with:

```rust
            let t = ui.label(egui::RichText::new(title).strong());
            if let (false, Some(p)) = (cell.usable, &v.shader) {
                t.on_hover_text(format!(
                    "{title}: the {} shader does not use slot {}",
                    p.shader.label(),
                    slot.slot_number()
                ));
            }
```

The rest of `card` already uses `slot` and `fill`, so it is unchanged. That includes Task 4's toggle.

- [ ] **Step 5: Run the tests to verify they pass**

Run: `cargo test -p sdc-gui --offline --test sets_model --test set_editor_ui --test set_preview_layout --test ui_smoke`
Expected: PASS.

- [ ] **Step 6: Run all gates, then commit**

```bash
git add crates/gui/src/model/sets.rs crates/gui/src/ui/set_editor.rs crates/gui/tests/sets_model.rs crates/gui/tests/set_editor_ui.rs
git commit -m "feat(gui): slot cards show the shader's slots in slot order; unusable filled slots are marked (M6 Track F1)

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

---

### Task 7: Docs — DECISIONS Note V and the GUI guide

**Spec:** all. Rulings V1–V9.

**Files:**
- Modify: `docs/DECISIONS.md`: append Note V after Note U's last bullet (U10, the end of the file).
- Modify: `docs/gui.md:93` (the set editor paragraph) and `:97-102` (PGPatcher settings).

**Interfaces:**
- Consumes: the behaviour of Tasks 1–6.
- Produces: no code.

- [ ] **Step 1: Append Note V**

Append to `docs/DECISIONS.md`:

```markdown

## Note V — M6 Track F1: TruePBR panel polish (plan rulings, 2026-10-12)

- **V1 What matchN holds (plan, 2026-10-12).**
  - PGPatcher compares `matchN` exactly with the unpatched mesh's raw slot path (no `strip_pbr`). So the value is the set's derived diffuse path, which comes from the source, not from the set id or the output.
  - `match1` is that path, with its `_d`/`mask` kept.
  - Slot N > 1 is that path with a trailing `_d` cut, plus `_<canonical suffix>` (`match2` = `…\rock01_n`; `facemask` → `facemask_n`).
  - Proven against pgpsim (`slot_matches_hit_the_unpatched_mesh_and_land_on_the_sdc_prefix`).
- **V2 Storage and writing.**
  - `match1` absent means the derived path (on by default; old projects get it too). `match1 = ""` means off. `match2`–`match8` are on when present.
  - Stored values have no `.dds`.
  - The JSON gets the value without a leading `textures\` (any case) and with `.dds` exactly once.
  - `match_diffuse`/`match_normal` are never written (`NOT_EXPOSED`).
- **V3 Prefix.**
  - The prefix comes from the lowest active `matchN`, with that slot's suffix cut, under `textures\pbr\`.
  - With no slot match, it is the derived path.
  - With a slot match, `rename` is the whole prefix.
- **V4 No match.** A set with no `matchN` and no `path_contains` shows "this entry matches nothing", asks before export, and is left out of the JSON. The schema's `hasMatcher` refuses it. Its textures still export.
- **V5 Number ranges.**
  - 0–1: `coat_strength`, `subsurface_opacity`, `fuzz.weight`.
  - Min 0: every other number key (16). That includes `smooth_angle`, `auto_uv` and `glint.log_microfacet_density`, which fit no class.
  - No CS or PGPatcher limits are documented.
- **V6 Clamping.**
  - The number box has `DragValue::range`, and a typed value outside the range shows "clamped to ≥ 0" / "clamped to 0–1".
  - A stored out-of-range value is shown as it is (`clamp_existing_to_range(false)`). An equal value is no edit, so drawing never dirties the project.
  - The writer clamps.
  - `pgpsim::range_warnings` mirrors the ranges, and a GUI test pins the two together.
- **V7 Layout.**
  - The TruePBR box shows the visual settings, then **Matching**, then **Advanced TruePBR settings** (collapsed: `path_contains`, `nif_filter`, `rename`, UV, vertex, locks, inner UV scale, coat colour).
  - The Advanced drawer's "PGPatcher (advanced)" section is gone.
- **V8 Slot cards.**
  - The set editor's cards (the 9-slot row; the Sets-column pill shows members, not slots) are the picked shader's usable slots, filled or empty, plus filled slots it cannot use (amber outline, hover reason).
  - They are ordered by PGPatcher slot 1–8.
  - Usable slots are the shader roles' sub-types mapped to slot cards.
  - A fallback pick uses the fallback shader. With no pick, only the filled slots show.
- **V9 Versions.** No bump (0.6.0): no engine change, and a bump would re-encode every texture through `engine_version`. No new codes (75). No `schema/` change.
```

- [ ] **Step 2: Update the GUI guide**

In `docs/gui.md`, replace the paragraph at `:93` ("Select a set to edit it. …") with:

```markdown
Select a set to edit it. Each texture slot is a card, in PGPatcher slot order: Diffuse, Normal, Emissive, Height, RMAOS and so on. Only the slots the set's shader can use are shown. A filled slot the shader cannot use stays, with an amber outline, and its title says why on hover. A card has a **preset dropdown** (it fills the PGPatcher settings for that kind of material), and **Slot commands** to move, share, duplicate or clear the slot. **Opaque** sits on the Diffuse card. The Diffuse and Normal cards have **Match on this** (see Matching below). A `_coat` map fills the Subsurface card, which then reads **Coat colour + strength**: its alpha is the coat strength and is kept.
```

Append these bullets to the `### PGPatcher settings` list (after the `default` block bullet):

```markdown
- Numbers cannot go below 0, and strengths, weights and opacities stay within 0–1. A typed value outside the range is clamped, and the box says "clamped to ≥ 0" or "clamped to 0–1". A value hand-edited in `sdc-project.toml` shows as it is, and the JSON gets it clamped.
- **Matching** lists the exact texture paths the entry matches (PGPatcher's `match1`–`match8`). The entry applies when any of them matches. The diffuse path is on by default. The **+** button adds another slot that holds a texture, and **×** removes a row. A set that matches nothing is left out of the JSON, and Export asks first.
- **Advanced TruePBR settings** (collapsed) holds path contains, mesh path contains, rename (with a slot match it replaces the whole prefix, so it must start with `textures\pbr\`), UV, vertex colour and lock keys.
```

- [ ] **Step 3: Run all gates (docs only, so they should be unchanged), then commit**

```bash
git add docs/DECISIONS.md docs/gui.md
git commit -m "docs: DECISIONS Note V (V1-V9) and the GUI guide for TruePBR panel polish (M6 Track F1)

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

---

## Self-review

**1. Spec coverage.**
- §1 intent: Tasks 1–6.
- §2 limits:
  - the per-key ranges: Task 1;
  - the GUI `DragValue::range` and the note: Task 2;
  - the writer clamp and the `pgpsim::check` warnings: Task 1.
- §3.1:
  - `matchN` with `.dds`, the old keys not written: Task 3;
  - `path_contains` kept: Task 5;
  - `nif_filter` unchanged: Task 3 (the writer writes it as before).
- §3.2:
  - the toggles, the Matching header, + and ×: Task 4;
  - the help line: Task 4;
  - the "matches nothing" warning: Task 4 (UI) and Task 3 (export).
- §3.3 no migration: Task 3. The old keys are ignored, and the default `match1` applies.
- §4 layout: Task 5.
- §5 slot row: Task 6.
- §6 tests: every bullet maps to a named test in Tasks 1–6.
- §7 Review Focus 1–4: pinned (see Review Focus).

**2. Placeholder scan.**
- No TBD or "similar to".
- Every code step has code.

**3. Type consistency.**
- `NumRange` (T1) is used in T2.
- `active_matches`/`match_toggle`/`stored_match`/`slot_match_path`/`matches_nothing`/`NO_MATCH` (T3) are used in T4.
- `SlotMatch`/`slot_matches` (T4) are used by `pgp_fields`.
- `Ctx.derived` is added in T4 and removed in T5.
- `SlotCell`/`slot_row(Option<&ShaderPick>, …)` (T6) are used by `set_editor::cards`.
- `panel_harness` (T2) is used in T4 and T5.
- `pitfalls(s, derived)` has its two callers, `export_asks` and the tests.

**4. Review Focus.** Five uncovered inputs are listed, each with its owning task's test.
