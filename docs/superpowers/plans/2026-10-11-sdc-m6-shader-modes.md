# sdc M6 Track A — two output modes and a per-set shader Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Every texture set gets a shader that is correct for its maps without the user touching it. The shader is chosen explicitly (GUI `sdc-project.toml`, CLI `--set set:<id>.shader=<id>`), or auto-picked, or falls back to the mode's default. It sets a few per-map defaults and reports extra and missing maps as warnings.

**Architecture:**
- **Engine data.** A new module `crates/engine/src/shader.rs` holds the shader table as data (`Shader`, `MapRole`, `ShaderSpec`), the auto-pick (`pick`), the per-member defaults (`member_defaults`, `member_subtype`) and the map check (`check_maps`). The CLI and GUI only read it.
- **Planning order (the chicken-and-egg).** Auto-pick rule 3 ("an `_m` whose sub-type is `complex_material`") needs the `_m`'s alpha analysis, so the shader cannot be picked before Load/Analyse. But the shader layer must apply inside `layered_profile_with`. So `Engine::plan_cached` changes as follows:
  - Pass 1 is unchanged, except that `resolve_one` also returns the `Analysis` and `SourceInfo` it computed.
  - A sequential step then picks each set's shader from the pass-1 jobs.
  - Members of sets whose shader has member defaults are resolved again (pure CPU, no reload), under the same panic isolation.
  - Pass 2 (`plan_packing`/`finish_recipe`) and everything after see the shader through `ResolveCtx::shaders`.
- **Precedence.** The shader layer is applied in `layered_profile_with` after the preset chain, the configs, the rules, the analysis-alpha step and `skin_wet_form`. It writes only fields that no explicit layer set (`field_explicit` is false: `builtin` and built-in presets). It is applied before the vocabulary green step and `ranked_overrides`. The two sub-type effects (eye `_n` → `msn`, landscape diffuse → `landscape_diffuse`) sit in `decide_subtype`, after `--set subtype` and rules and before the suggestions.
- **Where the warnings go.** The four `SHADER-*` codes travel on the `set_resolved` event (`shader`, `warnings`), not as separate `warning` events. So `plan`/`inspect` print them under the set line, the GUI shows them on the set card, and the existing golden `warnings(o)` assertions stay valid.
- **Mesh-reported shader first (owner, 2026-10-11; spec §12; S-shader-10).** When the scanned meshes report a shader for a set's diffuse, auto-pick uses it before the suffix rules 3–12:
  - **Evidence.** `BatchRequest::mesh_shaders: BTreeMap<String, Vec<Shader>>`, keyed by `shader::mesh_key` (the diffuse's game path without extension, e.g. `textures/a/wall`); one `Shader` per lighting shape that uses `<key>.dds` in slot 0. The engine never depends on `nif`/`modscan`: the GUI fills the map from the scan database (raw SQL on `Store::conn()`, as `meshes.rs` does) and maps `BSLightingShaderType` with the engine's `Shader::from_lighting_type`. The CLI sends nothing (no flag).
  - **Order in `pick`:** explicit → rule 1 (advanced skin) → rule 2 (PBR) → **mesh majority** (`auto:mesh`) → rules 3–12. A PBR or skin member always wins over the meshes: a mod's shipped mesh for a PBR set says type 0 until PGPatcher patches it, and the NIF's TruePBR bit (`SLSF2` bit 23) is set only on PGPatcher's output copies, which do not vote.
  - **Majority:** strictly more than half of the mapped votes; a tie or no votes falls through to the suffix rules. Lighting type 1 is both Environment Map and Complex Material, so a mesh `envmap` majority with a `complex_material` `_m` in the set picks `complex_material`.

**Tech Stack:** Rust 2021 workspace (`engine`, `cli` = `sdc`, `gui` = `sdc_gui`); egui 0.36.2; serde/schemars; toml_edit.

**Spec:** `docs/superpowers/specs/2026-10-11-sdc-m6-shader-modes-design.md` (owner-approved, binding). Background:
- parent spec `docs/superpowers/specs/2026-10-08-skyrim-dds-converter-design.md`: §3 config/precedence, §4 sub-types, §6 grouping, §10 codes;
- `docs/DECISIONS.md`: Note P (set model, PGPatcher JSON) and Note R (R-skin-1 Advanced skin detection, `set_is_advanced_skin`).

## Global Constraints

- **Execution order (rebased 2026-10-11, pre-flight `.superpowers/sdd/2026-10-11-sdc-m6-shader-modes/preflight.md`).** `main` 8159bdf already holds Track B (inputs/formats), Track C (mip filters/resize, DECISIONS S-mips-0..13) and the NIF team's merge (`nif`, `modscan`, `vfs`, `render3d`, the GUI Meshes view and live set preview). This plan never edits those plans.
  - The code catalogue count on `main` is **69** (incl. `DIM-RESIZED`, `CFG-FILTER-IGNORED`); after Task 3 it is **73**.
  - The workspace version on `main` is **0.3.0** (Track C did not bump, S-mips-9); Task 9 bumps it to **0.4.0**.
  - Line numbers in this plan were re-checked against 8159bdf; if one has drifted again, find the code by the quoted text, not the number.
- **Branch.** Execution branch `feat/m6-shader-modes` (exists, from `main` 8159bdf). Do not create branches.
- **Gates.** Run unpiped before every commit:
  - `cargo fmt --all --check`
  - `cargo clippy --workspace --all-targets -- -D warnings`
  - `cargo test --workspace`
- **Golden and `gui_repro`.** Both stay green. Any changed expectation is named in that task's report.
- **Schema.** A task that changes a serialised type (`event.schema.json`, `manifest.schema.json`, `config.schema.json`) regenerates the schemas with `cargo run -p sdc -- presets schema all --out schema` and commits `schema/`.
- **Commits.**
  - Commit only explicit paths (`git add <path> …`, never `-A`/`.`).
  - End every message with `Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>`.
  - Do not push.
- **Shell.** Never run `python -` heredocs.
- **Off-limits.**
  - Never touch `F:/ai/dds/bench`, `F:/ai/dds/converter-nif`, `nif/*` branches, `crates/nif`, `crates/vfs`, `crates/modscan`, `crates/render3d`, `crates/pgpsim` or DECISIONS Note Q (NIF team). The mesh votes read the scan database through the existing `Store::open_read_only` and `Store::conn()`; no new `Store` method.
  - In `crates/gui`, the NIF merge changed `app.rs`, `ui/header.rs`, `worker.rs`, `settings.rs` and `model/mod.rs`. Only Task 7 touches `app.rs` and `model/mod.rs` (small additive hunks, named there); `header.rs`, `worker.rs` and `settings.rs` are not touched.
  - `G:/` is read-only.
- **Decisions.** `## Note S` exists (Track B's S1–S12, then `### M6 Track C` with S-mips-0..13). Task 9 appends a `### M6 Track A: shader modes (plan rulings, 2026-10-11)` subsection after Track C's, with sub-ids `S-shader-1` … `S-shader-11`.
- **Versioning (spec §10).** Output-affecting changes need the workspace version bump, and it is the last task (Task 9). The changes here are the hair default, eye `_n` → `msn`, multilayer `data`, the `_i` suffix, the `_coat` alias and the mesh-reported shader (which changes member defaults wherever it changes the pick).
- **Mode = target (spec §2).** `cs` = Community Shaders, `vanilla` = Vanilla/ENB, `cpm` keeps its behaviour and uses the Vanilla/ENB list. There is no new axis.

## Review Focus

1. **A set that is not PBR on CS.** A lone `wall.png` or `wall.png` + `wall_n.png` under `cs` auto-picks `default` (rule 12, owner plan review), not `pbr`, and raises no missing-map warning. Only an explicit `pbr` choice on such a set warns. Task 8 pins it with a golden, `cs_plain_set_is_default_not_pbr`.
2. **Explicit choices never become JSON settings.** `shader` in a `[[sets]]` entry must not leak into the PGPatcher JSON (`SetEntry.settings`). Task 7 pins it: `shader_is_reserved_not_a_json_setting`.
3. **Fallback after a mode switch.** An explicit `pbr` on a set, then the GUI switched to vanilla, shows `default` with "(fallback)" and `SHADER-UNAVAILABLE`, and the `shader = "pbr"` entry stays in the file. Switching back restores `pbr`. Task 7 pins it: `switching_mode_falls_back_and_back`.
4. **A `--set set:` id that names no set** is reported (`SET-EDIT-STALE`), never silently ignored and never fatal. A bare base name with no `/` matches every set with that base. Task 4 pins both.
5. **Shader defaults never beat an explicit layer.** A `--set` format, a project `[subtypes]` table, a rule and a `[files]` entry each win over the shader layer. A source with no alpha keeps the analysis `remove` under `hair`. Task 5 pins each.
6. **Meshes never override a PBR member or an explicit choice.** A Default-typed mesh on a set with `_rmaos` still auto-picks `pbr`; `--set set:` and the GUI choice beat any mesh majority; a tie falls back to the suffix rules. Tasks 2 and 4 pin these: `mesh_majority_comes_after_pbr_and_before_the_suffix_rules`, `meshes_pick_the_shader_unless_a_pbr_member_or_a_choice_says_otherwise`.
7. **A hair diffuse and a non-box filter (S-mips-0, S-mips-13).** The shader's `cutout` bars a non-box `mips.filter`: unscoped it falls back to box with `CFG-FILTER-IGNORED` (Info), scoped it is `CFG-INVALID`. Task 5 pins both.

---

## File map

| File | Responsibility | Tasks |
|---|---|---|
| `crates/engine/src/shader.rs` (new) | Shader table as data, `from_lighting_type`, `mesh_key`, auto-pick (mesh votes first), member defaults, map check, notes | 1, 2, 3, 4 |
| `crates/engine/src/lib.rs` | `pub mod shader;` | 1 |
| `crates/engine/src/codes.rs` | 4 new codes, catalogue count | 3 |
| `crates/engine/src/group.rs` | `i` added to `SUFFIXES` | 2 |
| `crates/engine/src/suggest.rs` | `_i` suggestion; `has_component` made `pub(crate)` | 2 |
| `crates/engine/src/overrides.rs` | `Scope::Set`, `set:<id>.shader=` | 4 |
| `crates/engine/src/policy.rs` | `ResolveCtx::shaders`, shader layer, sub-type effect, `ResolvedJob::shader` | 4, 5 |
| `crates/engine/src/presets.rs` | `Provenance::Shader` | 5 |
| `crates/engine/src/api.rs` | `BatchRequest::set_shaders`, planning order, `Plan::shaders`, set warnings | 4 |
| `crates/engine/src/events.rs` | `SetResolved { shader, warnings }` | 4 |
| `crates/engine/src/manifest.rs`, `pipeline.rs` | `ManifestRecord::shader` | 6 |
| `crates/cli/src/verbs.rs`, `args.rs` | `set` line in plan output, `--set` help | 6 |
| `crates/gui/src/sets_doc.rs` | `shader` reserved key, `SetEntry::shader`, `set_shader` | 7 |
| `crates/gui/src/model/{items,sets,mod}.rs` | `SetItem`/`SetView` shader + warnings, `set_shader`, `set_shaders` input | 7 |
| `crates/gui/src/ui/sets_column.rs` | Shader dropdown, set warnings in the badge | 7 |
| `crates/gui/src/model/meshes.rs` | `mesh_votes` (scan DB → shaders per `mesh_key`) | 7 |
| `crates/gui/src/app.rs`, `crates/gui/src/main.rs` | refresh the votes when a scan ends; `AppModel::scan_db` set by `main` | 7 |
| `crates/engine/tests/shaders.rs` (new) | Plan-level shader tests | 4, 5 |
| `crates/engine/tests/golden.rs` | One golden per shader | 8 |
| `docs/DECISIONS.md`, `docs/gui.md`, `Cargo.toml`, `Cargo.lock` | Note S, GUI doc, version | 9 |

---

### Task 1: Engine — the shader table (`shader.rs`)

**Spec:** §3 (table, map identity, Environment Map `_m` optional, table location), §4.4 provenance strings.

**Files:**
- Create: `crates/engine/src/shader.rs`
- Modify: `crates/engine/src/lib.rs` (add `pub mod shader;` in alphabetical order)
- Test: unit tests at the bottom of `crates/engine/src/shader.rs`

**Interfaces:**
- Consumes: `crate::config::{Subtype, Target}`.
- Produces (later tasks rely on these exact names):
  - `pub enum Shader { Pbr, AdvancedSkin, Default, Envmap, ComplexMaterial, Glow, Parallax, Skin, Hair, Eye, Multilayer, Landscape }`
    - derives `Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize, JsonSchema`, with `#[serde(rename_all = "snake_case")]`
  - `impl Shader`:
    - `pub const ALL: [Shader; 12]`
    - `pub fn id(self) -> &'static str`
    - `pub fn label(self) -> &'static str`
    - `pub fn available(t: Target) -> &'static [Shader]`
    - `pub fn is_available(self, t: Target) -> bool`
    - `pub fn default_for(t: Target) -> Shader`
    - `pub fn spec(self) -> &'static ShaderSpec`
    - `pub fn has_member_defaults(self) -> bool`
    - `pub fn from_lighting_type(t: u32) -> Option<Shader>` (S-shader-10): the shader a NIF `BSLightingShaderProperty` of `BSLightingShaderType` `t` reports, or `None` (no vote)
  - `pub fn mesh_key(game_path: &str, stem: &str) -> String` (S-shader-10): the folder of `game_path` + `/` + `stem`, lower case (`("textures/a/wall_n.png", "wall")` → `textures/a/wall`; no folder → just `stem`). The engine calls it with the set's base, the GUI with a job's output stem; the scan database is asked for `<key>.dds`.
  - `pub struct MapRole { pub label: &'static str, pub subtypes: &'static [Subtype], pub suffixes: &'static [&'static str] }` and `impl MapRole { pub fn matches(&self, suffix: Option<&str>, subtype: Option<Subtype>) -> bool }`
  - `pub struct ShaderSpec { pub required: &'static [MapRole], pub optional: &'static [MapRole] }`
  - `pub enum AutoRule { AdvancedSkin, Pbr, Mesh, ComplexM, EnvM, P, G, I, Sk, HairPath, EyesPath, Landscape, Otherwise }` with `pub fn id(self) -> &'static str` (`Mesh`: S-shader-10)
    - derives `Debug, Clone, Copy, PartialEq, Eq`
  - `pub enum ShaderOrigin { Set, Auto(AutoRule), Fallback }`
    - derives `Debug, Clone, PartialEq, Eq`
    - serialises as a string: `set`, `auto:<rule id>`, `fallback`
    - `Display` and `FromStr`; `#[schemars(with = "String")]`
  - `pub struct ShaderPick { pub shader: Shader, pub provenance: ShaderOrigin, #[serde(default, skip_serializing_if = "Option::is_none")] pub wanted: Option<Shader> }`
    - derives `Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema`

**The table.** Write it exactly, as `static` data. `D` is `&[Subtype::Diffuse]`. `N` is `_n` = `&[Subtype::Normal, Subtype::LandscapeNormal]`.

| Shader | label | required roles (label: subtypes / suffixes) | optional roles |
|---|---|---|---|
| `pbr` | "PBR" | `albedo`: [PbrAlbedo]; `_n`: [PbrNormal]; `_rmaos`: [Rmaos] | `emissive _g`: [PbrEmissive]; `subsurface/coat colour _s`: [PbrSubsurface]; `coat _cnr`: [CoatNormal]; `fuzz _f`: [PbrFuzz]; `_p`: [Height] |
| `advanced_skin` | "Advanced Skin" | `skin_albedo`: [SkinAlbedo]; `_n/_msn`: [PbrNormal, Normal, Msn]; `skin_rfaos`: [SkinRfaos] | `skin_sk`: [SkinSk]; `skin_wet`: [SkinWet] |
| `default` | "Default" | `diffuse`: D; `_n`: N | `backlight _b/_bl`: [BacklightTint] / ["b","bl"] |
| `envmap` | "Environment Map" | `diffuse`: D; `_n`: N | `_m env mask`: [Envmask]; `_e`: [] / ["e"] |
| `complex_material` | "Complex Material" | `diffuse`: D; `_n`: N; `_m complex`: [ComplexMaterial] | `_e`: [] / ["e"] |
| `glow` | "Glow" | `diffuse`: D; `_n`: N; `_g`: [Glow] | — |
| `parallax` | "Parallax" | `diffuse`: D; `_n`: N; `_p`: [Height] | — |
| `skin` | "Skin" | `diffuse`: D; `_n/_msn`: [Normal, Msn] | `_sk`: [] / ["sk"]; `_s (with _msn)`: [MsnSpecular] |
| `hair` | "Hair" | `diffuse`: D; `_n`: N | — |
| `eye` | "Eye" | `diffuse`: D; `_n (model-space)`: [Msn] | `_sk`: [] / ["sk"]; `_m`: [Envmask]; `_e`: [] / ["e"] |
| `multilayer` | "Multilayer Parallax" | `diffuse`: D; `_n`: N; `_i`: [] / ["i"] | `_m`: [Envmask]; `_e`: [] / ["e"]; `_b`: [BacklightTint] / ["b","bl"] |
| `landscape` | "Landscape" | `diffuse`: [LandscapeDiffuse, Diffuse]; `_n`: [LandscapeNormal, Normal] | — |

- **`MapRole::matches`:** true when (`subtypes` is empty or `subtype` is in it) and (`suffixes` is empty or `suffix` is in it), and at least one of the two lists is non-empty.
- **`available`:**
  - `Cs`: all 12, `pbr` first, in the table order.
  - `Vanilla` and `Cpm`: the same list without `pbr` and `advanced_skin`.
- **`default_for`:** `Cs` → `Pbr`; otherwise → `Default`.
- **`has_member_defaults`:** true for `Hair`, `Eye`, `Landscape`, `Multilayer`, `Envmap` and `Parallax`.
- **`AutoRule::id`:** `advanced_skin`, `pbr`, `mesh`, `complex_m`, `env_m`, `p`, `g`, `i`, `sk`, `hair_path`, `eyes_path`, `landscape`, `default`.
- **`from_lighting_type`** (nif.xml `BSLightingShaderType`; names as `gui/src/model/meshes.rs` `SHADER_TYPES`): `0` Default → `Default`, `1` Environment Map → `Envmap`, `2` Glow → `Glow`, `3` Parallax → `Parallax`, `5` Skin Tint → `Skin`, `6` Hair Tint → `Hair`, `8` Multitexture Landscape → `Landscape`, `11` MultiLayer Parallax → `Multilayer`, `16` Eye Envmap → `Eye`. Every other value → `None`: Face Tint (4), Parallax Occ (7), the LOD, Snow, Tree Anim, Cloud and FO4 types, and unknown numbers. No lighting type yields `Pbr` or `AdvancedSkin`: PBR is not a lighting type (the TruePBR flag is PGPatcher's output marker, S-shader-10).

- [ ] **Step 1: Write the failing tests** (bottom of `shader.rs`)

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{Subtype, Target};

    #[test]
    fn ids_round_trip_and_are_snake_case() {
        for s in Shader::ALL {
            let j = serde_json::to_string(&s).unwrap();
            assert_eq!(j, format!("\"{}\"", s.id()));
            assert_eq!(serde_json::from_str::<Shader>(&j).unwrap(), s);
        }
        assert_eq!(Shader::AdvancedSkin.id(), "advanced_skin");
        assert_eq!(Shader::ComplexMaterial.id(), "complex_material");
        assert_eq!(Shader::Multilayer.id(), "multilayer");
    }

    #[test]
    fn lists_and_defaults_follow_the_mode() {
        assert_eq!(Shader::available(Target::Cs).len(), 12);
        assert_eq!(Shader::available(Target::Cs)[0], Shader::Pbr);
        for t in [Target::Vanilla, Target::Cpm] {
            let l = Shader::available(t);
            assert_eq!(l.len(), 10, "{t:?}");
            assert!(!l.contains(&Shader::Pbr) && !l.contains(&Shader::AdvancedSkin));
            assert_eq!(Shader::default_for(t), Shader::Default);
        }
        assert_eq!(Shader::default_for(Target::Cs), Shader::Pbr);
        assert!(!Shader::Pbr.is_available(Target::Vanilla));
        assert!(Shader::ComplexMaterial.is_available(Target::Vanilla));
    }

    #[test]
    fn envmap_does_not_require_m() {
        let s = Shader::Envmap.spec();
        assert!(!s.required.iter().any(|r| r.matches(Some("m"), Some(Subtype::Envmask))));
        assert!(s.optional.iter().any(|r| r.matches(Some("m"), Some(Subtype::Envmask))));
    }

    #[test]
    fn roles_match_by_subtype_and_suffix() {
        let skin = Shader::Skin.spec();
        // `_s` counts only as the msn specular, not as a subsurface tint.
        assert!(skin.optional.iter().any(|r| r.matches(Some("s"), Some(Subtype::MsnSpecular))));
        assert!(!skin.optional.iter().any(|r| r.matches(Some("s"), Some(Subtype::SubsurfaceTint))));
        // `_e` is identified by suffix: a skipped DDS cubemap has no sub-type.
        assert!(Shader::Envmap.spec().optional.iter().any(|r| r.matches(Some("e"), None)));
        // An empty role never matches.
        let empty = MapRole { label: "x", subtypes: &[], suffixes: &[] };
        assert!(!empty.matches(None, Some(Subtype::Diffuse)));
    }

    #[test]
    fn origin_serialises_as_a_string() {
        let p = ShaderPick { shader: Shader::Hair, provenance: ShaderOrigin::Auto(AutoRule::HairPath), wanted: None };
        let j = serde_json::to_value(&p).unwrap();
        assert_eq!(j, serde_json::json!({"shader": "hair", "provenance": "auto:hair_path"}));
        assert_eq!(serde_json::from_value::<ShaderPick>(j).unwrap(), p);
        assert_eq!("fallback".parse::<ShaderOrigin>().unwrap(), ShaderOrigin::Fallback);
        assert_eq!("set".parse::<ShaderOrigin>().unwrap(), ShaderOrigin::Set);
        assert_eq!("auto:mesh".parse::<ShaderOrigin>().unwrap(), ShaderOrigin::Auto(AutoRule::Mesh));
        assert!("auto:nope".parse::<ShaderOrigin>().is_err());
    }

    #[test]
    fn lighting_types_map_to_the_twelve_or_nothing() {
        let want = [(0, Shader::Default), (1, Shader::Envmap), (2, Shader::Glow), (3, Shader::Parallax), (5, Shader::Skin),
            (6, Shader::Hair), (8, Shader::Landscape), (11, Shader::Multilayer), (16, Shader::Eye)];
        for (t, s) in want {
            assert_eq!(Shader::from_lighting_type(t), Some(s), "{t}");
        }
        for t in [4, 7, 9, 10, 12, 13, 14, 15, 17, 18, 19, 20, 99, u32::MAX] {
            assert_eq!(Shader::from_lighting_type(t), None, "{t}");
        }
    }

    #[test]
    fn mesh_key_is_the_folder_and_the_stem() {
        assert_eq!(mesh_key("textures/a/wall_n.png", "wall"), "textures/a/wall");
        assert_eq!(mesh_key("textures/a/Wall.png", "Wall"), "textures/a/wall");
        assert_eq!(mesh_key("wall.png", "wall"), "wall");
    }

    #[test]
    fn member_default_shaders() {
        let with: Vec<Shader> = Shader::ALL.into_iter().filter(|s| s.has_member_defaults()).collect();
        assert_eq!(with, vec![Shader::Envmap, Shader::Parallax, Shader::Hair, Shader::Eye, Shader::Multilayer, Shader::Landscape]);
    }
}
```

- [ ] **Step 2: Run them and check that they fail**

Run: `cargo test -p engine --lib shader::`
Expected: a compile error (`shader` module or `Shader` not found).

- [ ] **Step 3: Implement `shader.rs`**
  - Module doc: `//! M6 Track A (spec 2026-10-11-sdc-m6-shader-modes-design.md, DECISIONS Note S): the per-set shader table as data, auto-pick, member defaults and the extra/missing map check. The CLI and GUI read it; neither duplicates it.`
  - Write the table with `static` `MapRole` arrays per the table above, and `ShaderSpec` statics returned by `spec()`.
  - `ShaderOrigin` gets a hand-written `Serialize`/`Deserialize` through `Display`/`FromStr`, as `Provenance` does in `presets.rs` (`impl fmt::Display for Provenance`, `impl FromStr for Provenance`).
  - `FromStr` maps `auto:<id>` through a match on `AutoRule::id` over all 13 rules, and errors on anything else.
  - `from_lighting_type` and `mesh_key` as specified; `from_lighting_type`'s doc names nif.xml `BSLightingShaderType` and S-shader-10.

- [ ] **Step 4: Run the tests and check that they pass**

Run: `cargo test -p engine --lib shader::`
Expected: PASS (8 tests).

- [ ] **Step 5: Run the gates, then commit**

```bash
git add crates/engine/src/shader.rs crates/engine/src/lib.rs
git commit -m "feat(engine): shader table as data (M6 Track A spec §3)

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

---

### Task 2: Engine — auto-pick, explicit choice and fallback (`shader::pick`); `_i` becomes a suffix; `_coat` names the coat colour

**Spec:** §4.1–4.3 (explicit wins, auto-pick rules 1–12 in order, unavailable → fallback), §12 (plan-review amendments).

**`_i` (S-shader-6).** Rule 7 (`_i`) needs `_i` to group into the set, but it is **not** on the §6.1 suffix list today (`group::SUFFIXES` has 19 entries, no `i`). Owner answer, 2026-10-11: yes, `_i` joins sets and suggests `diffuse`. So add `i` to `SUFFIXES` and suggest `diffuse` (Medium, "_i multilayer inner layer (colour)") for it. No vocabulary term ends in `i`, so the vocabulary invariant ("none ends in `_<§6.1 suffix>`") holds.

**`_coat` (S-shader-9; owner: "_coat (for the coat diffuse)", spec §12).**
- **What carries the coat colour today.** It is the `_s` member of a PBR set: sub-type `pbr_subsurface`, suggested by `suggest.rs` as "_s in a PBR set (subsurface / coat colour)". In the GUI it fills `SlotKind::Subsurface` (PGPatcher `slot8`). It is written `<base>_s.dds` (under the set prefix in the GUI).
- **The mechanism: a built-in vocabulary term, not a preset alias.** Preset aliases (§3.7) are user-declared in alias presets. The built-in "stands for a canonical suffix and renames the output" mechanism is the §4.4 vocabulary, as `iron_normalgl.png` → `iron_n.dds`. So add `term("coat", CoatColour)`, with a new `VocabRole::CoatColour` whose `canonical()` is `Some("s")`.
- **What `x_coat.png` then does:**
  - it splits to base `x`, suffix `s`, and joins set `x`;
  - its output is renamed `x_s.dds` (`discover::canonicalise_outputs`: `CoatColour => suffixed()`);
  - `apply_vocabulary` prefixes the reasons "exporter suffix _coat (coat colour)" and puts `pbr_subsurface` at the front with **High** confidence, whatever the set. The owner states it is the coat colour, so it is not the low `subsurface_tint` reading a bare `_s` gets outside a PBR set.
- **Consequences:**
  - same sub-type, slot and output name as the `_s` coat case;
  - `x_coat.png` next to `x_s.png` is `IN-DUPLICATE`, as with any vocabulary or canonical collision;
  - under `vanilla`/`cpm`, `pbr_subsurface` is unavailable with no fallback, so `CFG-UNAVAILABLE` blocks it, like any PBR-only map;
  - a user preset alias named `coat` is now rejected, because aliases may not be vocabulary terms (§3.7). Record this in S-shader-9.

**Files:**
- Modify: `crates/engine/src/shader.rs` (add `PickInput`, `pick`)
- Modify: `crates/engine/src/group.rs`:
  - `:8-11`: `SUFFIXES: [&str; 20]`, insert `"i"` after `"e"`. One-letter suffixes are order-independent.
  - `VocabRole` gets a `CoatColour` variant with doc "`_coat`: the PBR coat colour, the `_s` member (`pbr_subsurface`; DECISIONS Note S-shader-9)", and `Vocab::canonical` maps it to `Some("s".into())`.
  - `VOCABULARY: [Vocab; 32]`: insert `term("coat", CoatColour)` among the 4-letter terms, right after `term("fuzz", PackingInput(Fuzz))`, to keep it longest-first.
- Modify: `crates/engine/src/discover.rs:157` (was `:155`): `VocabRole::Normal(_) | VocabRole::Emissive | VocabRole::CoatColour => suffixed(),`.
- Modify: `crates/engine/src/suggest.rs`:
  - make `fn has_component` `pub(crate)`;
  - add the arm `Some("i") => vec![s(Diffuse, Medium, "_i multilayer inner layer (colour)")],` before `Some("b") | Some("bl")`;
  - in `apply_vocabulary`, add the role label `VocabRole::CoatColour => "coat colour"`. Then, for `CoatColour`, remove any `pbr_subsurface` entry and insert `Suggestion { subtype: PbrSubsurface, confidence: High, reason: "exporter suffix _coat (coat colour); the PBR coat colour (_s)".into() }` at index 0.
- Check other `match` arms on `VocabRole` (`pack/plan.rs:45`, `policy.rs` `vocab_for` users, `gui/src/model/sets.rs:218`): each either has a `_` arm or compares with `==`. Add an arm only where the compiler asks.
- Test:
  - unit tests in `shader.rs`;
  - grouping tests in `crates/engine/tests/discover_group.rs`;
  - `crates/engine/tests/suffix_vocabulary.rs`: change `SUFFIXES.len()` 19 → 20 and `VOCABULARY.len()` 31 → 32, and add the `_coat` test below.

**Interfaces:**
- Produces:

```rust
pub struct PickInput<'a> {
    pub target: Target,
    /// The explicit choice for this set (GUI `sdc-project.toml` or `--set set:`), if any.
    pub explicit: Option<Shader>,
    /// `pack::plan::set_is_advanced_skin` for the set.
    pub advanced_skin: bool,
    /// `suggest::is_pbr_set` evaluated as under `cs`, whatever the target: rule 2 is target-independent, so a PBR set
    /// under vanilla auto-picks `pbr` and falls back with SHADER-UNAVAILABLE.
    pub pbr: bool,
    pub suffixes: &'a BTreeSet<String>,
    /// Pass-1 sub-types of the set's members (packing inputs excluded).
    pub subtypes: &'a [Subtype],
    pub game_paths: &'a [&'a str],
    /// S-shader-10: what the scanned meshes report for the set's diffuse, one entry per lighting shape
    /// (`BatchRequest::mesh_shaders`); empty = no evidence.
    pub mesh: &'a [Shader],
}
pub fn pick(i: &PickInput) -> ShaderPick;
```

**Rules of `pick`.** First match wins:
1. `advanced_skin`, or any of `SkinAlbedo | SkinRfaos | SkinSk | SkinWet` in `subtypes` → `AdvancedSkin`, `AutoRule::AdvancedSkin`.
2. `pbr`, or any of `PbrAlbedo | PbrEmissive | PbrSubsurface | PbrFuzz | PbrNormal | CoatNormal | Rmaos` in `subtypes` → `Pbr`.
- **Mesh (S-shader-10; owner: mesh-reported shader first).** `mesh_majority(i)` → that shader, `AutoRule::Mesh`. It sits after rules 1–2, so a PBR or skin member always wins (a shipped mesh of a PBR set says Default until PGPatcher patches it), and before rules 3–12, which are the fallback:

```rust
/// S-shader-10: the strict majority of the mesh votes (more than half), else `None` (a tie or no votes: the suffix
/// rules decide). Lighting type 1 is both Environment Map and Complex Material, so an `envmap` majority in a set
/// with a complex `_m` is `complex_material`.
fn mesh_majority(i: &PickInput) -> Option<Shader> {
    let mut n: BTreeMap<Shader, usize> = BTreeMap::new();
    for s in i.mesh {
        *n.entry(*s).or_default() += 1;
    }
    let (top, c) = n.into_iter().max_by_key(|(_, c)| *c)?;
    if c * 2 <= i.mesh.len() {
        return None;
    }
    Some(if top == Shader::Envmap && i.subtypes.contains(&Subtype::ComplexMaterial) { Shader::ComplexMaterial } else { top })
}
```
3. `ComplexMaterial` in `subtypes` → `ComplexMaterial`, `ComplexM`.
4. `Envmask` in `subtypes`, or any of `m`, `em`, `envmask` in `suffixes` → `Envmap`, `EnvM`.
5. `p` in suffixes → `Parallax`.
6. `g` → `Glow`.
7. `i` → `Multilayer`.
8. `sk` → `Skin`.
9. any game path with a component equal (ASCII case-insensitive) to `hair` → `Hair`, `HairPath`.
10. `eyes` → `Eye`, `EyesPath`.
11. `LandscapeDiffuse` in `subtypes`, or a game path that starts with `textures/landscape/` or has a `landscape` component (the `suggest.rs:63` rule) → `Landscape`.
12. otherwise `Shader::Default`, `Otherwise` (provenance `auto:default`). Owner, 2026-10-11 plan review (spec §4.2 rule 12, §12): auto-pick never reaches `pbr` without a PBR member, even on CS. `default_for` (`pbr` on CS) is used only for the dropdown's first entry and as the fallback shader.

Then:
- `explicit = Some(s)` replaces the result with `s`, origin `Set`.
- If the result is not `is_available(target)`, the pick becomes `{ shader: default_for(target), provenance: Fallback, wanted: Some(result) }`.

- [ ] **Step 1: Write the failing tests** (in `shader.rs` `mod tests`)

```rust
fn input<'a>(t: Target, suffixes: &'a BTreeSet<String>, subtypes: &'a [Subtype], paths: &'a [&'a str]) -> PickInput<'a> {
    PickInput { target: t, explicit: None, advanced_skin: false, pbr: false, suffixes, subtypes, game_paths: paths, mesh: &[] }
}
fn sx(s: &[&str]) -> BTreeSet<String> { s.iter().map(|x| x.to_string()).collect() }
fn auto(p: &ShaderPick) -> (Shader, ShaderOrigin) { (p.shader, p.provenance.clone()) }

#[test]
fn one_test_per_auto_rule() {
    use Subtype::*;
    let none = sx(&[]);
    let cases: Vec<(Target, BTreeSet<String>, Vec<Subtype>, Vec<&str>, bool, bool, Shader, AutoRule)> = vec![
        (Target::Cs, sx(&["sk"]), vec![SkinAlbedo, SkinSk], vec!["textures/a/b.png"], true, false, Shader::AdvancedSkin, AutoRule::AdvancedSkin),
        (Target::Cs, sx(&["n", "rmaos"]), vec![PbrAlbedo, PbrNormal, Rmaos], vec!["textures/a/b.png"], false, true, Shader::Pbr, AutoRule::Pbr),
        (Target::Cs, sx(&["n", "m"]), vec![Diffuse, Normal, ComplexMaterial], vec!["textures/a/b.png"], false, false, Shader::ComplexMaterial, AutoRule::ComplexM),
        (Target::Vanilla, sx(&["n", "m"]), vec![Diffuse, Normal, Envmask], vec!["textures/a/b.png"], false, false, Shader::Envmap, AutoRule::EnvM),
        (Target::Cs, sx(&["n", "p"]), vec![Diffuse, Normal, Height], vec!["textures/a/b.png"], false, false, Shader::Parallax, AutoRule::P),
        (Target::Vanilla, sx(&["n", "g"]), vec![Diffuse, Normal, Glow], vec!["textures/a/b.png"], false, false, Shader::Glow, AutoRule::G),
        (Target::Vanilla, sx(&["n", "i"]), vec![Diffuse, Normal, Diffuse], vec!["textures/a/b.png"], false, false, Shader::Multilayer, AutoRule::I),
        (Target::Vanilla, sx(&["n", "sk"]), vec![Diffuse, Normal, BacklightTint], vec!["textures/a/b.png"], false, false, Shader::Skin, AutoRule::Sk),
        (Target::Vanilla, sx(&["n"]), vec![Diffuse, Normal], vec!["textures/actors/character/Hair/braid.png"], false, false, Shader::Hair, AutoRule::HairPath),
        (Target::Vanilla, sx(&["n"]), vec![Diffuse, Normal], vec!["textures/actors/character/eyes/eye.png"], false, false, Shader::Eye, AutoRule::EyesPath),
        (Target::Vanilla, sx(&["n"]), vec![LandscapeDiffuse, LandscapeNormal], vec!["textures/landscape/dirt.png"], false, false, Shader::Landscape, AutoRule::Landscape),
        (Target::Vanilla, none.clone(), vec![Diffuse], vec!["textures/a/b.png"], false, false, Shader::Default, AutoRule::Otherwise),
        // Owner (plan review): a plain set on CS auto-picks `default`, never `pbr`.
        (Target::Cs, sx(&["n"]), vec![Diffuse, Normal], vec!["textures/a/b.png"], false, false, Shader::Default, AutoRule::Otherwise),
    ];
    for (t, s, st, paths, skin, pbr, want, rule) in cases {
        let mut i = input(t, &s, &st, &paths);
        i.advanced_skin = skin;
        i.pbr = pbr;
        assert_eq!(auto(&pick(&i)), (want, ShaderOrigin::Auto(rule)), "{want:?}");
    }
}

#[test]
fn rule_order() {
    use Subtype::*;
    let paths = ["textures/a/b.png"];
    // `_m` complex beats `_p`.
    let s = sx(&["m", "p"]);
    let st = [Diffuse, ComplexMaterial, Height];
    assert_eq!(pick(&input(Target::Cs, &s, &st, &paths)).shader, Shader::ComplexMaterial);
    // PBR beats `_p`.
    let st = [PbrAlbedo, Height, Rmaos];
    let mut i = input(Target::Cs, &s, &st, &paths);
    i.pbr = true;
    assert_eq!(pick(&i).shader, Shader::Pbr);
    // Advanced skin beats PBR.
    i.advanced_skin = true;
    assert_eq!(pick(&i).shader, Shader::AdvancedSkin);
    // A suffix rule beats a path rule: `_g` under hair/ is glow.
    let s = sx(&["g"]);
    let st = [Diffuse, Glow];
    assert_eq!(pick(&input(Target::Vanilla, &s, &st, &["textures/hair/x.png"])).shader, Shader::Glow);
}

#[test]
fn explicit_wins_and_unavailable_falls_back() {
    let s = sx(&["n"]);
    let st = [Subtype::Diffuse, Subtype::Normal];
    let paths = ["textures/a/b.png"];
    let mut i = input(Target::Vanilla, &s, &st, &paths);
    i.explicit = Some(Shader::Hair);
    assert_eq!(auto(&pick(&i)), (Shader::Hair, ShaderOrigin::Set));
    i.explicit = Some(Shader::Pbr);
    assert_eq!(pick(&i), ShaderPick { shader: Shader::Default, provenance: ShaderOrigin::Fallback, wanted: Some(Shader::Pbr) });
    // Auto `pbr` under vanilla falls back too.
    i.explicit = None;
    i.pbr = true;
    assert_eq!(pick(&i), ShaderPick { shader: Shader::Default, provenance: ShaderOrigin::Fallback, wanted: Some(Shader::Pbr) });
    // cpm uses the vanilla list.
    i.target = Target::Cpm;
    assert_eq!(pick(&i).shader, Shader::Default);
}

#[test]
fn mesh_majority_comes_after_pbr_and_before_the_suffix_rules() {
    use Subtype::*;
    let paths = ["textures/a/b.png"];
    let s = sx(&["n", "p"]);
    let st = [Diffuse, Normal, Height];
    let mut i = input(Target::Vanilla, &s, &st, &paths);
    // Two hair shapes against one default: hair, before the `_p` rule.
    let votes = [Shader::Hair, Shader::Default, Shader::Hair];
    i.mesh = &votes;
    assert_eq!(auto(&pick(&i)), (Shader::Hair, ShaderOrigin::Auto(AutoRule::Mesh)));
    // A tie is no evidence: the suffix rules decide.
    let tie = [Shader::Hair, Shader::Default];
    i.mesh = &tie;
    assert_eq!(auto(&pick(&i)), (Shader::Parallax, ShaderOrigin::Auto(AutoRule::P)));
    // An explicit choice beats the meshes.
    i.mesh = &votes;
    i.explicit = Some(Shader::Glow);
    assert_eq!(auto(&pick(&i)), (Shader::Glow, ShaderOrigin::Set));
    // A PBR member beats a Default mesh, and an advanced skin set beats it too.
    i.explicit = None;
    i.target = Target::Cs;
    i.pbr = true;
    let def = [Shader::Default];
    i.mesh = &def;
    assert_eq!(auto(&pick(&i)), (Shader::Pbr, ShaderOrigin::Auto(AutoRule::Pbr)));
    i.advanced_skin = true;
    assert_eq!(auto(&pick(&i)), (Shader::AdvancedSkin, ShaderOrigin::Auto(AutoRule::AdvancedSkin)));
    // Lighting type 1 is envmap or complex material: a complex `_m` refines an envmap majority.
    let s = sx(&["n", "m"]);
    let st = [Diffuse, Normal, ComplexMaterial];
    let env = [Shader::Envmap];
    let mut i = input(Target::Vanilla, &s, &st, &paths);
    i.mesh = &env;
    assert_eq!(auto(&pick(&i)), (Shader::ComplexMaterial, ShaderOrigin::Auto(AutoRule::Mesh)));
    // A Default mesh beats the complex `_m` suffix rule: the game renders the shape as Default.
    i.mesh = &def;
    assert_eq!(auto(&pick(&i)), (Shader::Default, ShaderOrigin::Auto(AutoRule::Mesh)));
}
```

In `crates/engine/tests/discover_group.rs`, add:

```rust
#[test]
fn an_i_member_joins_its_set() {
    let (base, suffix) = engine::group::split_suffix("glass_i");
    assert_eq!((base.as_str(), suffix.as_deref()), ("glass", Some("i")));
    assert!(!engine::group::is_vocabulary("i"));
}
```

In `crates/engine/tests/suffix_vocabulary.rs`, add a plan-level test. The file has no plan helper (checked on 8159bdf: only `resolve_img`), so add `fn plan_cs(dir: &Path) -> Plan` with `Engine::plan` on `BatchRequest::new(vec![dir.join("in")], dir.join("out"), Target::Cs)`, `recursive = true` and `accept = AcceptSuggestions::High` (a `VecSink` reporter and `CancelToken::default()`, as `tests/shaders.rs` `plan` does):

```rust
#[test]
fn coat_is_the_s_coat_colour_member() {
    use engine::group::{split_suffix_with, vocab_of, VocabRole};
    let s = split_suffix_with("rock_coat", &Default::default());
    assert_eq!((s.base.as_str(), s.suffix.as_deref()), ("rock", Some("s")));
    assert_eq!(vocab_of("rock_coat").unwrap().role, VocabRole::CoatColour);
    // Plan: rock_coat.png joins set `rock`, resolves pbr_subsurface (High, decided) and is written rock_s.dds,
    // exactly as rock_s.png would be in the same PBR set.
    let d = tempfile::tempdir().unwrap();
    for (name, px) in [("rock.png", [120u8, 90, 60]), ("rock_n.png", [128, 128, 255]), ("rock_rmaos.png", [128, 0, 255]), ("rock_coat.png", [200, 180, 160])] {
        let p = d.path().join("in/textures/a").join(name);
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        image::ImageBuffer::from_fn(16, 16, |_, _| image::Rgb(px)).save(p).unwrap();
    }
    let p = plan_cs(d.path()); // the file's plan helper, as described above
    let j = p.jobs.iter().find(|j| j.input.ends_with("rock_coat.png")).unwrap();
    assert_eq!(j.profile.subtype.id(), "pbr_subsurface");
    assert!(j.output.ends_with("textures/a/rock_s.dds"), "{}", j.output.display());
    assert!(j.set_key.ends_with("textures/a/rock"));
    assert!(!matches!(j.status, engine::policy::JobStatus::Unresolved { .. }), "{:?}", j.status);
}
```

- [ ] **Step 2: Run them and check that they fail**

Run: `cargo test -p engine --lib shader:: && cargo test -p engine --test discover_group an_i_member && cargo test -p engine --test suffix_vocabulary coat`
Expected:
- compile errors (`pick`, `PickInput`, `VocabRole::CoatColour` missing);
- then the `_i` test fails (`suffix` is `None`);
- then the `_coat` test fails (`rock_coat` keeps its own set and name).

- [ ] **Step 3: Implement**
  - `pick` as specified.
  - Use `crate::suggest::has_component` for the path rules, with a lower-cased copy of each path.
  - Change `SUFFIXES`.
  - Add the `_i` suggestion arm.
  - Update the `SUFFIXES` doc comment: "`_i` (multilayer inner layer) added in M6 (DECISIONS Note S-shader-6)".
  - Add `VocabRole::CoatColour`, the `coat` term, the `discover.rs` arm and the `apply_vocabulary` branch.
  - Spec §4.4 lists the vocabulary: Task 9 adds `_coat` there.

- [ ] **Step 4: Run the tests and check that they pass**

Run: `cargo test --workspace`
Expected: PASS. `suffix_vocabulary.rs` changes its counts to 20 and 32: name them in the report.

- [ ] **Step 5: Run the gates, then commit**

```bash
git add crates/engine/src/shader.rs crates/engine/src/group.rs crates/engine/src/suggest.rs crates/engine/src/discover.rs crates/engine/tests/discover_group.rs crates/engine/tests/suffix_vocabulary.rs
git commit -m "feat(engine): shader auto-pick (mesh majority after PBR, plain sets -> default), explicit choice and fallback; _i is a set suffix; _coat names the coat colour _s (M6 spec §4, §12)

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

---

### Task 3: Engine — the four `SHADER-*` codes and the map check

**Spec:** §3 map identity, §5 (extra maps still export, missing writes nothing), §8 codes.

**Files:**
- Modify: `crates/engine/src/codes.rs`:
  - add after `SkinWetForm`:
    ```
    ShaderAuto = "SHADER-AUTO" => Info,
    ShaderUnavailable = "SHADER-UNAVAILABLE" => Warning,
    ShaderExtraMap = "SHADER-EXTRA-MAP" => Warning,
    ShaderMissingMap = "SHADER-MISSING-MAP" => Warning,
    ```
  - extend the module doc with "the four `SHADER-*` codes (M6 Track A)";
  - count test (`codes.rs:176`): `assert_eq!(Code::ALL.len(), 69);` → `73` (69 on `main` 8159bdf, incl. Track C's `DIM-RESIZED` and `CFG-FILTER-IGNORED`; S-mips-0).
- Modify: `crates/engine/src/shader.rs`: add `MemberMap`, `assign`, `check_maps`, `pick_notes`.

**Interfaces:**
- Produces:

```rust
/// One member of a set as the map check sees it. A skipped DDS member has no sub-type.
pub struct MemberMap { pub name: String, pub suffix: Option<String>, pub subtype: Option<Subtype> }
/// The role each member fills, in `members` order. Roles keyed by suffix are tried first, so an `_i` that resolves
/// `diffuse` fills `_i`, not `diffuse`. `None` = the shader does not use the member.
pub fn assign<'s>(spec: &'s ShaderSpec, members: &[MemberMap]) -> Vec<Option<&'s MapRole>>;
/// SHADER-EXTRA-MAP per unused member (data `{"member", "shader"}`) and SHADER-MISSING-MAP per required role no
/// member fills (data `{"map", "shader"}`), members first, then roles in table order.
pub fn check_maps(pick: &ShaderPick, members: &[MemberMap]) -> Vec<Warning>;
/// SHADER-AUTO for an `auto:` pick (data `{"shader", "rule"}`); SHADER-UNAVAILABLE for a fallback (data
/// `{"wanted", "shader"}`); nothing for `set`.
pub fn pick_notes(pick: &ShaderPick, target: Target) -> Vec<Warning>;
```

**Messages.** Use these exact templates, because the goldens and GUI read them:
- AUTO: `shader {id} picked automatically ({rule id})`
- UNAVAILABLE: `shader {wanted} is not available under {target:?}; using {shader}`
- EXTRA: `{name} is not used by the {id} shader; it is still converted`
- MISSING: `the {id} shader needs {role label}; none is in the set (nothing is generated)`

- [ ] **Step 1: Write the failing tests** (in `shader.rs`)

```rust
fn m(name: &str, suffix: Option<&str>, st: Option<Subtype>) -> MemberMap {
    MemberMap { name: name.into(), suffix: suffix.map(str::to_string), subtype: st }
}
fn codes(w: &[crate::codes::Warning]) -> Vec<crate::codes::Code> { w.iter().map(|w| w.code).collect() }

#[test]
fn extra_and_missing_maps() {
    use crate::codes::Code::*;
    let glow = ShaderPick { shader: Shader::Glow, provenance: ShaderOrigin::Set, wanted: None };
    let ms = [m("lamp.dds", None, Some(Subtype::Diffuse)), m("lamp_p.dds", Some("p"), Some(Subtype::Height))];
    let w = check_maps(&glow, &ms);
    assert_eq!(codes(&w), vec![ShaderExtraMap, ShaderMissingMap, ShaderMissingMap]);
    assert_eq!(w[0].data["member"], "lamp_p.dds");
    assert_eq!(w[1].data["map"], "_n");
    assert_eq!(w[2].data["map"], "_g");
}

#[test]
fn envmap_without_m_is_complete() {
    let p = ShaderPick { shader: Shader::Envmap, provenance: ShaderOrigin::Set, wanted: None };
    let ms = [m("a.dds", None, Some(Subtype::Diffuse)), m("a_n.dds", Some("n"), Some(Subtype::Normal))];
    assert!(check_maps(&p, &ms).is_empty());
}

#[test]
fn an_i_that_resolves_diffuse_fills_i_not_diffuse() {
    let p = ShaderPick { shader: Shader::Multilayer, provenance: ShaderOrigin::Set, wanted: None };
    let ms = [m("g_i.dds", Some("i"), Some(Subtype::Diffuse)), m("g_n.dds", Some("n"), Some(Subtype::Normal))];
    let w = check_maps(&p, &ms);
    assert_eq!(codes(&w), vec![crate::codes::Code::ShaderMissingMap]);
    assert_eq!(w[0].data["map"], "diffuse");
}

#[test]
fn notes_for_auto_and_fallback() {
    use crate::codes::Code::*;
    let a = ShaderPick { shader: Shader::Hair, provenance: ShaderOrigin::Auto(AutoRule::HairPath), wanted: None };
    assert_eq!(codes(&pick_notes(&a, Target::Cs)), vec![ShaderAuto]);
    let f = ShaderPick { shader: Shader::Default, provenance: ShaderOrigin::Fallback, wanted: Some(Shader::Pbr) };
    let w = pick_notes(&f, Target::Vanilla);
    assert_eq!(codes(&w), vec![ShaderUnavailable]);
    assert_eq!(w[0].message, "shader pbr is not available under Vanilla; using default");
    let s = ShaderPick { shader: Shader::Hair, provenance: ShaderOrigin::Set, wanted: None };
    assert!(pick_notes(&s, Target::Cs).is_empty());
}
```

In `codes.rs` `severities_follow_spec_section_10`, add:

```rust
assert_eq!(Code::ShaderAuto.severity(), Severity::Info);
assert_eq!(Code::ShaderMissingMap.severity(), Severity::Warning);
assert_eq!(Code::parse("SHADER-EXTRA-MAP"), Some(Code::ShaderExtraMap));
```

- [ ] **Step 2: Run them and check that they fail**

Run: `cargo test -p engine --lib shader:: codes::`
Expected: compile errors (`ShaderAuto`, `check_maps` missing).

- [ ] **Step 3: Implement** the codes, then `assign`/`check_maps`/`pick_notes` as specified.

- [ ] **Step 4: Run the tests and check that they pass**

Run: `cargo test -p engine`
Expected: PASS, including `codes_serialise_as_their_strings_and_are_unique` with the new count.

- [ ] **Step 5: Run the gates, regenerate the schema** (`event.schema.json` lists codes), **then commit**

```bash
cargo run -p sdc -- presets schema all --out schema
git add crates/engine/src/codes.rs crates/engine/src/shader.rs schema
git commit -m "feat(engine): SHADER-AUTO/-UNAVAILABLE/-EXTRA-MAP/-MISSING-MAP and the per-shader map check (M6 spec §8)

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

---

### Task 4: Engine — explicit choices, the planning order, and the shader on events and jobs

**Spec:** §4.1 (explicit from `sdc-project.toml` or `--set set:<base>.shader=<id>`), §4.4 provenance, §7 (switching mode re-resolves), §5 extra and missing maps.

**`--set` ruling (S-shader-1).** `--set` today has three forms: `field=`, `subtype:<id>.field=` and `file:<glob>.field=` (`overrides.rs:150`). `Scope::Path` comes only from `[files]`, and **no set scope exists**. So:
- Add `Scope::Set(String)` with the prefix `set:`. The only field it accepts is `shader`: parse with `rest.strip_suffix(".shader")`, because set ids may contain dots.
- `shader` is not a profile field. It stays out of `FIELD_PATHS`, and it is rejected unscoped, under `file:` and under `subtype:`. Those already fail as "unknown field".
- The value must be a `Shader` id. `auto` is rejected: the CLI has no saved choice to clear.
- `<id>` is a set id as `sets::set_id(output_root, key)` prints it, compared with `sets::same_id`. An id without `/` also matches every set whose key ends in `/<id>`.
- An id that matches no set gives one `SET-EDIT-STALE` (Info) with data `{"set": id}`.

**`sdc-project.toml` ruling (S-shader-2).** The spec writes `[sets."<base>"] shader`. The real file is `[[sets]]` with `id = "…"` (`sets_doc.rs`), so the choice is `shader = "<id>"` inside that set's `[[sets]]` entry (Task 7). The engine receives it as `BatchRequest::set_shaders`. The command line beats the GUI map (as N2: the command line always wins).

**Files:**
- Modify: `crates/engine/src/overrides.rs`:
  - add `Scope::Set(String)`;
  - in `parse_set`, add a `set:` branch before the `file:` branch;
  - update the module doc to list `set:<id>.shader=<shader>`.
- Modify: `crates/engine/src/policy.rs`:
  - `ResolveCtx` gets `pub shaders: &'a BTreeMap<String, crate::shader::ShaderPick>` (set key → pick);
  - add `pub static NO_SHADERS: BTreeMap<String, crate::shader::ShaderPick> = BTreeMap::new();` next to the `ResolveCtx` definition;
  - `override_applies`: `Scope::Set(_) => false`;
  - `ranked_overrides` rank: `Scope::Set(_) => 4`;
  - `layered_profile_with` reach match: `Scope::Set(_) => unreachable!("set scopes never apply to a file")`. It is filtered out by `override_applies`, so place the match after that filter.
  - `ResolvedJob` gets `#[serde(default, skip_serializing_if = "Option::is_none")] pub shader: Option<crate::shader::ShaderPick>`.
  - Every `ResolvedJob { … }` literal gets `shader: None`: `policy.rs` (`packing_input_job`, both literals in `resolve_decided`), `pack/plan.rs:192`, `api.rs` tests (`:1931`, `:1982`; were near `:1823`), `tests/bake_run.rs:53`, `tests/events_manifest.rs:122`, `tests/pack_run.rs:35`.
  - Every `ResolveCtx { … }` literal gets `shaders: &NO_SHADERS`: `api.rs` (the one in `plan_cached`), `tests/policy_checks.rs` (10), `tests/policy_layers.rs` (7), `tests/suffix_aliases.rs` (2), `tests/suffix_vocabulary.rs` (1). Run `grep -rn "ResolveCtx {" crates` to confirm.
- Modify: `crates/engine/src/shader.rs`: add `pick_for_set` (glue).
- Modify: `crates/engine/src/events.rs`: `SetResolved` gets `#[serde(default, skip_serializing_if = "Option::is_none")] shader: Option<crate::shader::ShaderPick>` and `#[serde(default, skip_serializing_if = "Vec::is_empty")] warnings: Vec<crate::codes::Warning>`. `EVENT_SCHEMA` stays 2: both fields are additive and default.
- Modify: `crates/engine/src/api.rs`:
  - `BatchRequest::set_shaders: BTreeMap<String, Shader>` (doc: "GUI (M6, DECISIONS Note S-shader-2): set id → explicit shader. Empty for the CLI; `--set set:<id>.shader` beats it"), defaulted in `new`;
  - `BatchRequest::mesh_shaders: BTreeMap<String, Vec<Shader>>` (doc: "GUI (M6, DECISIONS Note S-shader-10): `shader::mesh_key` → the shader each scanned lighting shape that uses `<key>.dds` as its diffuse reports. Empty for the CLI and when no scan exists: the suffix rules decide"), defaulted in `new`. `BatchRequest` derives only `Debug, Clone`, so a plain map fits; every other constructor goes through `new`;
  - `Plan::shaders: BTreeMap<String, ShaderPick>` (set key → pick);
  - the planning order below;
  - `SetResolved` construction at `:1232` (was `:1170`) gets the two fields;
  - fix the `SetResolved` literal in `tests/events_manifest.rs:115`.
- Test: `crates/engine/tests/shaders.rs` (new).

**Interfaces:**
- Consumes: `shader::{pick, PickInput, check_maps, pick_notes, MemberMap, ShaderPick, Shader}` (Tasks 1–3); `pack::plan::set_is_advanced_skin`; `suggest::{is_pbr_set, SuggestContext}`.
- Produces:
  - `pub fn pick_for_set(ctx: &ResolveCtx, set: &TextureSet, files: &[InputFile], jobs: &[ResolvedJob], explicit: Option<Shader>, mesh: &BTreeMap<String, Vec<Shader>>) -> ShaderPick` in `shader.rs`. It builds `PickInput`:
    - `mesh` = `mesh.get(&mesh_key(&files[set.members[0]].game_path, base))`, or `&[]`, where `base` is the last `/` component of `set.key` (the colour member's output stem: set members share one folder). Shortcut comment: `shortcut: a set whose first member was added from another folder (set edits) looks up that folder; key by the colour member if that bites`;
    - `subtypes` = `profile.subtype` of `jobs` with `set_key == set.key` whose status is not `PackingInput`;
    - `game_paths` from `set.members`;
    - `advanced_skin` = `set_is_advanced_skin(ctx, set, &files[first member])`;
    - `pbr` = `is_pbr_set(&SuggestContext { target: Target::Cs, suffix: None, game_path: <first path or "">, set_suffixes: &set.suffixes, alpha: AlphaContent::None, is_float: false, advanced_skin: false })`.
  - `ResolveCtx::shaders`, `ResolvedJob::shader`, `Plan::shaders`, `BatchRequest::{set_shaders, mesh_shaders}`, `SetResolved { shader, warnings }`, `Scope::Set`.

**Planning order** in `Engine::plan_cached`. Add steps 6a–6c to the load-bearing comment list at `api.rs` "M3b planning sequence":
1. `resolve_one` (on 8159bdf a closure `|&(set, f), admitted: Admitted<'_>, stage: &Cell<Stage>| -> Result<ResolvedJob, LoadError>`, `api.rs:861`; Track C added the admission and stage arguments) returns `Result<(ResolvedJob, Analysis, SourceInfo), LoadError>`. Keep its admission, stage and cache handling and the job resolve exactly as today, with `ctx.shaders = &NO_SHADERS`. Its slot type changes with it. The collection loop pushes `(job, work index, analysis, info)` into `pass1: Vec<_>`, not `jobs`.
2. **6a — explicit choices.** A new private `fn explicit_shaders(req: &BatchRequest, sets: &[TextureSet], rep: &Reporter) -> BTreeMap<String, Shader>` (set key → shader):
   - first `req.set_shaders`, then the `Scope::Set` entries of `req.overrides` in command-line order, so later wins;
   - matching as in S-shader-1;
   - one `SET-EDIT-STALE` per id that matches nothing: message `shader choice skipped: no set '{id}'`.
3. **6b — picks.** `let picks: BTreeMap<String, ShaderPick> = sets.iter().map(|s| (s.key.clone(), shader::pick_for_set(&ctx, s, &files, &pass1_jobs, explicit.get(&s.key).copied(), &req.mesh_shaders))).collect();`. Then shadow `let ctx = ResolveCtx { shaders: &picks, ..ctx };`. Every `ResolveCtx` field is a reference or `Copy`, so the struct update compiles.
4. **6c — re-resolve.** For each `(job, i, a, info)` in `pass1`:
   - if `picks[&job.set_key].shader.has_member_defaults()`, rebuild `FileFacts` for `work[i]` exactly as `resolve_one` does (same `member_split`), and call `resolve_file(&ctx, &facts)` inside `catch_unwind`;
   - on success, copy `source` from the old job and insert `provenance["target"] = target_src`;
   - on a panic, push `FailedFile { stage: Stage::Resolve, error: format!("internal error: {}", panic_message(&*p)) }`;
   - then push the job into `jobs`.
   - Extract the split into a helper `split_for(i) -> SuffixSplit` (stem of `work[i].1.rel`, then `member_split`), shared with `resolve_one`. Each call site builds `FileFacts` from its own local split, because `FileFacts` borrows `suffix`/`alias` from it and a helper cannot return both.
5. Pass 2 and the bake run unchanged, with the shadowed `ctx`.
6. **After `plan_bakes`:** `for j in &mut jobs { j.shader = picks.get(&j.set_key).cloned(); }`.
7. **`SetResolved` loop.** For each emitted set:
   - `warnings = pick_notes(&pick, target)` followed by `check_maps(&pick, &members)`;
   - `members` is built by a private `fn map_members(set: &TextureSet, files: &[InputFile], jobs: &[ResolvedJob], skipped: &[SkippedFile], aliases: &BTreeMap<String, String>) -> Vec<MemberMap>`:
     - each file index `i` in `set.members` with `pack::plan::raw_role(&files[i]).is_none()`. Raw maps are excluded by role, not by status, because `plan_packing` re-tags an unused raw input (`PACK-UNUSED-INPUT`):
       - with a non-packed job whose `input == files[i].path`: `name` = the job's output file name, `suffix` = `member_split(stem, set, aliases).suffix`, `subtype` = `Some(job.profile.subtype)`;
       - otherwise, only when `skipped` holds it with `code == Some(Code::InDdsSkipped)`: suffix from its stem, `subtype: None`. Duplicates and failed files are left out.
     - then every packed job of the set (`job.pack.is_some()`): `suffix: None`, `subtype: Some(profile.subtype)`.
8. `Plan { …, shaders: picks }`.

- [ ] **Step 1: Write the failing tests** (`crates/engine/tests/shaders.rs`)

```rust
//! M6 Track A (DECISIONS Note S): shader choice, fallback and set warnings at plan level.
use engine::api::*;
use engine::codes::Code;
use engine::config::Target;
use engine::events::*;
use engine::overrides::{parse_set, Scope};
use engine::policy::AcceptSuggestions;
use engine::shader::{Shader, ShaderOrigin};
use image::{ImageBuffer, Rgb};
use std::path::Path;
use std::sync::Arc;

fn put_rgb(dir: &Path, rel: &str, px: [u8; 3]) {
    let p = dir.join("in").join(rel);
    std::fs::create_dir_all(p.parent().unwrap()).unwrap();
    ImageBuffer::from_fn(16, 16, |_, _| Rgb(px)).save(p).unwrap();
}

fn plan(dir: &Path, t: Target, sets: &[&str], gui: &[(&str, Shader)]) -> (Plan, Vec<EngineEvent>) {
    plan_with(dir, t, sets, gui, &[])
}

/// `plan` with mesh votes (S-shader-10): `shader::mesh_key` → the shaders the meshes report.
fn plan_with(dir: &Path, t: Target, sets: &[&str], gui: &[(&str, Shader)], mesh: &[(&str, Vec<Shader>)]) -> (Plan, Vec<EngineEvent>) {
    let mut req = BatchRequest::new(vec![dir.join("in")], dir.join("out"), t);
    req.recursive = true;
    req.accept = AcceptSuggestions::High;
    req.overrides = sets.iter().map(|s| parse_set(s).unwrap()).collect();
    req.set_shaders = gui.iter().map(|(k, s)| (k.to_string(), *s)).collect();
    req.mesh_shaders = mesh.iter().map(|(k, v)| (k.to_string(), v.clone())).collect();
    let sink = Arc::new(VecSink::default());
    let rep = Reporter::new(sink.clone(), "shaders");
    let p = Engine::new().unwrap().plan(req, &rep, &CancelToken::default()).unwrap();
    (p, sink.take())
}

fn wall(dir: &Path) {
    put_rgb(dir, "textures/a/wall.png", [120, 90, 60]);
    put_rgb(dir, "textures/a/wall_n.png", [128, 128, 255]);
}

fn set_event<'a>(ev: &'a [EngineEvent], tail: &str) -> &'a EventKind {
    &ev.iter().find(|e| matches!(e.kind, EventKind::SetResolved { .. }) && e.set.as_deref().is_some_and(|s| s.ends_with(tail))).unwrap().kind
}

#[test]
fn set_scope_parses_and_rejects_misuse() {
    let o = parse_set("set:textures/a/wall.v2.shader=hair").unwrap();
    assert_eq!(o.scope, Scope::Set("textures/a/wall.v2".into()));
    assert_eq!(o.field, "shader");
    assert!(parse_set("set:wall.shader=nope").unwrap_err().contains("unknown shader"));
    assert!(parse_set("set:wall.shader=auto").is_err());
    assert!(parse_set("set:wall.format=bc7").unwrap_err().contains("only `shader`"));
    assert!(parse_set("shader=hair").is_err());
    assert!(parse_set("file:*.png.shader=hair").is_err());
}

#[test]
fn auto_pick_is_reported_on_the_set() {
    let d = tempfile::tempdir().unwrap();
    wall(d.path());
    let (p, ev) = plan(d.path(), Target::Vanilla, &[], &[]);
    let pick = p.shaders.values().next().unwrap();
    assert_eq!((pick.shader, pick.provenance.to_string().as_str()), (Shader::Default, "auto:default"));
    let EventKind::SetResolved { shader, warnings, .. } = set_event(&ev, "textures/a/wall") else { unreachable!() };
    assert_eq!(shader.as_ref(), Some(pick));
    assert_eq!(warnings.iter().map(|w| w.code).collect::<Vec<_>>(), vec![Code::ShaderAuto]);
    assert!(p.jobs.iter().all(|j| j.shader.as_ref() == Some(pick)));
}

#[test]
fn explicit_via_cli_and_gui_and_cli_wins() {
    let d = tempfile::tempdir().unwrap();
    wall(d.path());
    let (p, _) = plan(d.path(), Target::Vanilla, &[], &[("textures/a/wall", Shader::Hair)]);
    assert_eq!(p.shaders.values().next().unwrap().shader, Shader::Hair);
    assert_eq!(p.shaders.values().next().unwrap().provenance, ShaderOrigin::Set);
    let (p, _) = plan(d.path(), Target::Vanilla, &["set:wall.shader=glow"], &[("textures/a/wall", Shader::Hair)]);
    assert_eq!(p.shaders.values().next().unwrap().shader, Shader::Glow, "the command line wins; a bare base matches");
}

#[test]
fn unavailable_falls_back_with_a_warning() {
    let d = tempfile::tempdir().unwrap();
    wall(d.path());
    let (p, ev) = plan(d.path(), Target::Vanilla, &["set:textures/a/wall.shader=pbr"], &[]);
    let pick = p.shaders.values().next().unwrap();
    assert_eq!((pick.shader, &pick.provenance, pick.wanted), (Shader::Default, &ShaderOrigin::Fallback, Some(Shader::Pbr)));
    let EventKind::SetResolved { warnings, .. } = set_event(&ev, "textures/a/wall") else { unreachable!() };
    assert_eq!(warnings.iter().map(|w| w.code).collect::<Vec<_>>(), vec![Code::ShaderUnavailable]);
}

#[test]
fn extra_and_missing_maps_warn_and_everything_still_converts() {
    let d = tempfile::tempdir().unwrap();
    wall(d.path());
    put_rgb(d.path(), "textures/a/wall_g.png", [200, 200, 0]);
    let (p, ev) = plan(d.path(), Target::Vanilla, &["set:wall.shader=parallax"], &[]);
    let EventKind::SetResolved { warnings, .. } = set_event(&ev, "textures/a/wall") else { unreachable!() };
    let codes: Vec<Code> = warnings.iter().map(|w| w.code).collect();
    assert_eq!(codes, vec![Code::ShaderExtraMap, Code::ShaderMissingMap]);
    assert_eq!(warnings[0].data["member"], "wall_g.dds");
    assert_eq!(warnings[1].data["map"], "_p");
    assert!(p.jobs.iter().any(|j| j.output.ends_with("wall_g.dds")), "an extra map still converts");
    assert!(!p.jobs.iter().any(|j| j.output.ends_with("wall_p.dds")), "nothing is generated");
}

#[test]
fn an_unmatched_set_id_is_stale_not_fatal() {
    let d = tempfile::tempdir().unwrap();
    wall(d.path());
    let (_, ev) = plan(d.path(), Target::Vanilla, &["set:nothing/here.shader=hair"], &[]);
    let stale: Vec<_> = ev.iter().filter_map(|e| match &e.kind {
        EventKind::Warning(w) if w.code == Code::SetEditStale => Some(w.clone()),
        _ => None,
    }).collect();
    assert_eq!(stale.len(), 1);
    assert_eq!(stale[0].data["set"], "nothing/here");
}

#[test]
fn switching_mode_re_resolves() {
    let d = tempfile::tempdir().unwrap();
    wall(d.path());
    put_rgb(d.path(), "textures/a/wall_rmaos.png", [128, 0, 255]);
    let (cs, _) = plan(d.path(), Target::Cs, &[], &[]);
    assert_eq!(cs.shaders.values().next().unwrap().shader, Shader::Pbr);
    let (va, _) = plan(d.path(), Target::Vanilla, &[], &[]);
    assert_eq!(va.shaders.values().next().unwrap().provenance, ShaderOrigin::Fallback);
}

#[test]
fn meshes_pick_the_shader_unless_a_pbr_member_or_a_choice_says_otherwise() {
    let d = tempfile::tempdir().unwrap();
    wall(d.path());
    put_rgb(d.path(), "textures/a/wall_p.png", [128, 128, 128]);
    let hair = [("textures/a/wall", vec![Shader::Hair, Shader::Hair])];
    let (p, ev) = plan_with(d.path(), Target::Vanilla, &[], &[], &hair);
    let pick = p.shaders.values().next().unwrap();
    assert_eq!((pick.shader, pick.provenance.to_string().as_str()), (Shader::Hair, "auto:mesh"));
    let EventKind::SetResolved { warnings, .. } = set_event(&ev, "textures/a/wall") else { unreachable!() };
    assert_eq!(warnings[0].code, Code::ShaderAuto);
    assert_eq!(warnings[0].data["rule"], "mesh");
    // A key for another texture is no evidence: `_p` decides.
    let (p, _) = plan_with(d.path(), Target::Vanilla, &[], &[], &[("textures/a/other", vec![Shader::Hair])]);
    assert_eq!(p.shaders.values().next().unwrap().shader, Shader::Parallax);
    // The explicit choice still wins.
    let (p, _) = plan_with(d.path(), Target::Vanilla, &["set:wall.shader=glow"], &[], &hair);
    assert_eq!(p.shaders.values().next().unwrap().shader, Shader::Glow);
    // A PBR member wins over a Default mesh: the shipped mesh is not patched by PGPatcher yet.
    put_rgb(d.path(), "textures/a/wall_rmaos.png", [128, 0, 255]);
    let (p, _) = plan_with(d.path(), Target::Cs, &[], &[], &[("textures/a/wall", vec![Shader::Default])]);
    assert_eq!(p.shaders.values().next().unwrap().provenance.to_string(), "auto:pbr");
}
```

- [ ] **Step 2: Run them and check that they fail**

Run: `cargo test -p engine --test shaders`
Expected: compile errors (`set_shaders`, `Plan::shaders`, `Scope::Set` missing).

- [ ] **Step 3: Implement** everything listed under **Files** and **Planning order**. `parse_set` error strings:
  - `--set '{s}': expected set:<id>.shader=<shader>`
  - `--set '{s}': a set scope sets only `shader``
  - `--set '{s}': unknown shader '{raw}' (one of: pbr, advanced_skin, …)`: the list comes from `Shader::ALL` ids joined by `, `.

- [ ] **Step 4: Run the tests and check that they pass**

Run: `cargo test -p engine`
Expected: PASS. The existing golden, `pack_*`, `bake_*` and `set_edits` tests are unchanged, because no shader here has member defaults that touch their fixtures. If one changes, it is a planning-order regression: fix it, do not re-baseline.

- [ ] **Step 5: Run the gates, regenerate the schema** (`event.schema.json` gains `shader`/`warnings`) **and commit**

```bash
cargo run -p sdc -- presets schema all --out schema
git add crates/engine/src/overrides.rs crates/engine/src/policy.rs crates/engine/src/shader.rs crates/engine/src/events.rs crates/engine/src/api.rs crates/engine/src/pack/plan.rs crates/engine/tests/shaders.rs crates/engine/tests/policy_checks.rs crates/engine/tests/policy_layers.rs crates/engine/tests/suffix_aliases.rs crates/engine/tests/suffix_vocabulary.rs crates/engine/tests/bake_run.rs crates/engine/tests/events_manifest.rs crates/engine/tests/pack_run.rs schema
git commit -m "feat(engine): per-set shader picked after analysis; --set set:<id>.shader; shader and SHADER-* on set_resolved (M6 spec §4)

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

---

### Task 5: Engine — the shader default layer

**Spec:** §5 member defaults, §6 single-channel maps, §9 shader-default tests.

**Track C interaction (pre-flight, S-mips-0/-13).** On 8159bdf `layered_profile_with` returns `Layered = (Profile, provenance, Option<(MipFilter, &str)>)` and ends with `unscoped_filter_falls_back`, which reads the final alpha policy. The shader layer goes after the `skin_wet_form` block, so that check sees the shader's `cutout`: no extra code. The shader layer never writes `mips.filter`. Expected and pinned by `hair_cutout_and_a_non_box_filter`: unscoped kaiser → box + `CFG-FILTER-IGNORED`; scoped → `CFG-INVALID` (S-mips-0 already ruled both).

**Precedence ruling (S-shader-3).**
- `Provenance::Shader(String)` (shown `shader:<id>`) is a new layer. It is applied in `layered_profile_with` after:
  - the preset chain;
  - the user and project configs;
  - the rules;
  - the analysis-alpha step;
  - `skin_wet_form`.
- It writes a field only when `!field_explicit(ctx, &prov, field)`, that is when the field came from `builtin` or a built-in preset. It is applied before the vocabulary green step and `ranked_overrides`.
- So it beats built-in profiles and suggestions, and loses to user presets, configs, rules, analysis facts, suffix facts, `[files]` and `--set`.
- It counts as explicit itself (`field_explicit`'s `Some(_) => true`). So a shader alpha policy answers the alpha-meaning question.
- Consequence: `hair` on a source with **no** alpha keeps the analysis `remove` (Analysis already set it). On **varying** alpha it gets `cutout` and asks nothing.
- In the `Provenance` enum (`Ord`; `presets.rs:200`, unchanged by B and C apart from the `Fallback` doc), the variant sits between `AnalysisFact` and `Suffix`, so `float_source_is_linear`'s `>= Suffix` check still treats suffix facts and above as overruling.

**Sub-type ruling (S-shader-4).** Eye `_n` → `msn` and landscape colour member → `landscape_diffuse` are sub-type effects. They are applied in `decide_subtype` after `--set subtype` and rules, before `suggestions.first()`:
- **explicit shader** (`ShaderOrigin::Set`): `decided: true`, `provenance: Provenance::Shader(id)`;
- **auto or fallback**: a `High` suggestion inserted at the front, with reason `"{id} shader ({origin}): {why}"`. It is decided only as any High suggestion is (`--accept-suggestions high`, GUI accept).

**Member defaults** (`shader.rs`):

| Shader | Member sub-type | Fields |
|---|---|---|
| `hair` | `diffuse` | `alpha.policy = "cutout"`, `mips.coverage = "on"` |
| `multilayer` | `diffuse` | `alpha.policy = "data"` (owner, plan review: the alpha is the layer mask, kept exact; S-shader-5) |
| `envmap`, `eye` | `envmask` | `format = "bc4"` |
| `parallax` | `height` | `format = "bc4"` |

**Finding for S-shader-5.** `sdc-base.toml` already has `[subtypes.envmask] format = "bc4"` and `[subtypes.height] format = "bc4"` (parent spec §4.1 rows `envmask`/`height`). So the §6 "new" BC4 rows change only the provenance (`shader:envmap`), not the bytes. The layer still writes them, so a later built-in preset change cannot undo them. The shader does **not** re-type an `_m` (a varying-alpha `_m` under explicit `envmap` stays `complex_material` and is reported as an extra map).

**Sub-type effects** (`shader.rs`):

| Shader | Top suggestion | Becomes | why |
|---|---|---|---|
| `eye` | `normal`, `pbr_normal`, `landscape_normal` | `msn` | "model-space `_n`" |
| `landscape` | `diffuse`, `pbr_albedo` | `landscape_diffuse` | "landscape diffuse (alpha = snow mask)" |

**Files:**
- Modify: `crates/engine/src/presets.rs` (`Provenance::Shader`, `Display` `shader:{id}`, `FromStr` prefix `shader:`, and the doc list of string forms).
- Modify: `crates/engine/src/shader.rs`:
  - `pub fn member_defaults(shader: Shader, st: Subtype) -> Vec<(&'static str, serde_json::Value)>`
  - `pub fn member_subtype(shader: Shader, suggested: Subtype) -> Option<(Subtype, &'static str)>`
- Modify: `crates/engine/src/policy.rs`:
  - `fn shader_layer(ctx, shader, st, doc, prov, sources)`, called in `layered_profile_with` right after the `skin_wet_form` block, as `if let Some(p) = ctx.shaders.get(&f.set.key) { shader_layer(ctx, p.shader, st, &mut doc, &mut prov, &mut sources); }`. Each patch is applied with `Provenance::Shader(shader.id().into())` and reach `GENERIC`;
  - the `decide_subtype` block per S-shader-4.
- Test: `crates/engine/tests/shaders.rs` (extend).

**Interfaces:**
- Consumes: `ResolveCtx::shaders` (Task 4); `field_explicit`, `apply`, `to_patch` (existing, `policy.rs`).
- Produces: `Provenance::Shader(String)`, `shader::member_defaults`, `shader::member_subtype`.

- [ ] **Step 1: Write the failing tests** (append to `crates/engine/tests/shaders.rs`)

```rust
use image::Rgba;

fn put_rgba(dir: &Path, rel: &str, f: impl Fn(u32, u32) -> [u8; 4]) {
    let p = dir.join("in").join(rel);
    std::fs::create_dir_all(p.parent().unwrap()).unwrap();
    ImageBuffer::from_fn(16, 16, |x, y| Rgba(f(x, y))).save(p).unwrap();
}
fn job<'a>(p: &'a Plan, tail: &str) -> &'a engine::policy::ResolvedJob {
    p.jobs.iter().find(|j| j.output.to_string_lossy().replace('\\', "/").ends_with(tail)).unwrap_or_else(|| panic!("no job {tail}"))
}
fn prov(j: &engine::policy::ResolvedJob, f: &str) -> String { j.provenance[f].to_string() }

#[test]
fn hair_diffuse_is_cutout_with_coverage() {
    let d = tempfile::tempdir().unwrap();
    put_rgba(d.path(), "textures/hair/braid.png", |x, _| [90, 60, 30, if x < 8 { 255 } else { 0 }]);
    put_rgb(d.path(), "textures/hair/braid_n.png", [128, 128, 255]);
    let (p, _) = plan(d.path(), Target::Vanilla, &[], &[]);
    let j = job(&p, "braid.dds");
    assert_eq!(serde_json::to_value(j.profile.alpha.policy).unwrap(), "cutout");
    assert_eq!(serde_json::to_value(j.profile.mips.coverage).unwrap(), "on");
    assert_eq!(prov(j, "alpha.policy"), "shader:hair");
    // A bare stem is a Medium colour suggestion, so the job may still wait for its sub-type, but never for the
    // alpha meaning: the shader answered it.
    if let engine::policy::JobStatus::Unresolved { needs, .. } = &j.status {
        assert!(!needs.contains(&engine::policy::Need::AlphaMeaning), "{needs:?}");
    }
}

#[test]
fn hair_without_alpha_keeps_remove() {
    let d = tempfile::tempdir().unwrap();
    put_rgb(d.path(), "textures/hair/braid.png", [90, 60, 30]);
    let (p, _) = plan(d.path(), Target::Vanilla, &[], &[]);
    let j = job(&p, "braid.dds");
    assert_eq!(serde_json::to_value(j.profile.alpha.policy).unwrap(), "remove");
    assert_eq!(prov(j, "alpha.policy"), "analysis");
}

#[test]
fn eye_n_is_model_space() {
    let d = tempfile::tempdir().unwrap();
    put_rgb(d.path(), "textures/eyes/eye.png", [90, 60, 30]);
    put_rgb(d.path(), "textures/eyes/eye_n.png", [128, 128, 255]);
    let (p, _) = plan(d.path(), Target::Vanilla, &[], &[]);
    assert_eq!(job(&p, "eye_n.dds").profile.subtype.id(), "msn");
    // Explicit choice: decided with shader provenance.
    let d = tempfile::tempdir().unwrap();
    wall(d.path());
    let (p, _) = plan(d.path(), Target::Vanilla, &["set:wall.shader=eye"], &[]);
    let n = job(&p, "wall_n.dds");
    assert_eq!((n.profile.subtype.id().as_str(), prov(n, "subtype").as_str()), ("msn", "shader:eye"));
}

#[test]
fn explicit_landscape_makes_the_diffuse_landscape_diffuse() {
    let d = tempfile::tempdir().unwrap();
    wall(d.path());
    let (p, _) = plan(d.path(), Target::Vanilla, &["set:wall.shader=landscape"], &[]);
    assert_eq!(job(&p, "wall.dds").profile.subtype.id(), "landscape_diffuse");
}

#[test]
fn multilayer_diffuse_keeps_alpha() {
    let d = tempfile::tempdir().unwrap();
    put_rgba(d.path(), "textures/a/glass.png", |x, _| [90, 90, 90, (x * 16) as u8]);
    put_rgb(d.path(), "textures/a/glass_n.png", [128, 128, 255]);
    put_rgb(d.path(), "textures/a/glass_i.png", [40, 40, 90]);
    let (p, _) = plan(d.path(), Target::Vanilla, &[], &[]);
    let j = job(&p, "glass.dds");
    assert_eq!(serde_json::to_value(j.profile.alpha.policy).unwrap(), "data");
    assert_eq!(prov(j, "alpha.policy"), "shader:multilayer");
}

#[test]
fn env_m_and_parallax_p_are_bc4_by_shader() {
    let d = tempfile::tempdir().unwrap();
    wall(d.path());
    put_rgb(d.path(), "textures/a/wall_m.png", [200, 200, 200]);
    let (p, _) = plan(d.path(), Target::Vanilla, &[], &[]);
    let m = job(&p, "wall_m.dds");
    assert_eq!((serde_json::to_value(m.profile.format).unwrap(), prov(m, "format")), ("bc4".into(), "shader:envmap".to_string()));
    let d = tempfile::tempdir().unwrap();
    wall(d.path());
    put_rgb(d.path(), "textures/a/wall_p.png", [128, 128, 128]);
    let (p, _) = plan(d.path(), Target::Cs, &[], &[]);
    let h = job(&p, "wall_p.dds");
    assert_eq!((serde_json::to_value(h.profile.format).unwrap(), prov(h, "format")), ("bc4".into(), "shader:parallax".to_string()));
}

#[test]
fn explicit_layers_beat_the_shader_default() {
    let d = tempfile::tempdir().unwrap();
    wall(d.path());
    put_rgb(d.path(), "textures/a/wall_p.png", [128, 128, 128]);
    // --set format
    let (p, _) = plan(d.path(), Target::Cs, &["file:**/wall_p.png.format=bc7"], &[]);
    let h = job(&p, "wall_p.dds");
    assert_eq!((serde_json::to_value(h.profile.format).unwrap(), prov(h, "format")), ("bc7".into(), "user".to_string()));
    // a project config [subtypes.height] table
    std::fs::write(d.path().join("in/sdc.toml"), "[subtypes.height]\nformat = \"bc7\"\n").unwrap();
    let (p, _) = plan(d.path(), Target::Cs, &[], &[]);
    assert_eq!(prov(job(&p, "wall_p.dds"), "format"), "project_config");
    // a rule that names the sub-type beats the eye sub-type effect
    std::fs::write(d.path().join("in/sdc.toml"), "[[rules]]\nmatch = { glob = \"**/wall_n.png\" }\nsubtype = \"normal\"\n").unwrap();
    let (p, _) = plan(d.path(), Target::Cs, &["set:wall.shader=eye"], &[]);
    assert_eq!(job(&p, "wall_n.dds").profile.subtype.id(), "normal");
}

/// S-mips-0 / S-mips-13 (Track C): the hair `cutout` bars a non-box filter. Unscoped, the filter falls back to box
/// with CFG-FILTER-IGNORED (Info); file-scoped, it stays and is CFG-INVALID.
#[test]
fn hair_cutout_and_a_non_box_filter() {
    let soft = |x: u32, _| [90, 60, 30, (x * 16) as u8];
    let d = tempfile::tempdir().unwrap();
    put_rgba(d.path(), "textures/hair/braid.png", soft);
    let (p, _) = plan(d.path(), Target::Vanilla, &["mips.filter=kaiser"], &[]);
    let j = job(&p, "braid.dds");
    assert_eq!(prov(j, "alpha.policy"), "shader:hair");
    assert_eq!((serde_json::to_value(j.profile.mips.filter).unwrap(), prov(j, "mips.filter")), ("box".into(), "fallback".to_string()));
    assert!(j.warnings.iter().any(|w| w.code == Code::CfgFilterIgnored), "{:?}", j.warnings);
    let (p, _) = plan(d.path(), Target::Vanilla, &["file:**/braid.png.mips.filter=kaiser"], &[]);
    let j = job(&p, "braid.dds");
    assert!(j.warnings.iter().any(|w| w.code == Code::CfgInvalid), "{:?}", j.warnings);
}
```

Also add to `presets.rs` tests (or `policy_layers.rs`):

```rust
assert_eq!(Provenance::Shader("hair".into()).to_string(), "shader:hair");
assert_eq!("shader:hair".parse::<Provenance>().unwrap(), Provenance::Shader("hair".into()));
assert!(Provenance::Shader(String::new()) < Provenance::Suffix(String::new()));
assert!(Provenance::Shader(String::new()) > Provenance::AnalysisFact(String::new()));
```

- [ ] **Step 2: Run them and check that they fail**

Run: `cargo test -p engine --test shaders`
Expected: FAIL. `hair_diffuse_is_cutout_with_coverage` is unresolved (the alpha meaning is unconfirmed), and `eye_n_is_model_space` gets `normal`.

- [ ] **Step 3: Implement**
  - Add `Provenance::Shader`, `member_defaults`, `member_subtype`, `shader_layer` and the `decide_subtype` block.
  - Check the rule syntax in `explicit_layers_beat_the_shader_default` against the `Rule` struct in `presets.rs` (`match`, `subtype`, `overrides`), and adjust the TOML keys if the struct names them differently.

- [ ] **Step 4: Run the tests and check that they pass**

Run: `cargo test --workspace`
Expected: PASS. Existing tests whose fixtures live under `hair/`, `eyes/` or have an `_i` member, or that assert `provenance["format"]` of an `envmask`/`height`, change. List each changed expectation and the reason (shader layer) in the report. Do not change any other expectation.

- [ ] **Step 5: Run the gates, then commit**

```bash
git add crates/engine/src/presets.rs crates/engine/src/shader.rs crates/engine/src/policy.rs crates/engine/tests/shaders.rs
git commit -m "feat(engine): shader default layer above built-in profiles, below explicit layers (M6 spec §5–6)

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

(Add any other test file whose expectation changed, by explicit path.)

---

### Task 6: CLI and manifest — the shader in `plan`/`inspect` output, the manifest and `--set` help

**Spec:** §4.4 (provenance shown by `plan`, `inspect` and the manifest), §4.1 (`--set set:`).

**Files:**
- Modify: `crates/cli/src/verbs.rs` `print_event` (`:233`). Replace the `SetResolved { recipes, .. } if !recipes.is_empty()` arm with an arm for every `SetResolved`:

```rust
EventKind::SetResolved { recipes, shader, warnings, .. } => {
    let shown = shader.as_ref().map(|p| format!("  shader {} ({})", p.shader.id(), p.provenance)).unwrap_or_default();
    println!("set {}{shown}", ev.set.as_deref().unwrap_or(""));
    for w in warnings {
        println!("  {} {} {}", tag!(&w.severity), w.code.as_str(), w.message);
    }
    for r in recipes { /* the existing pack lines, unchanged */ }
}
```

- Modify: `crates/cli/src/args.rs` (the `--set` doc comment at `:117`, was `:116`): add "`set:<id>.shader=<shader>` picks a set's shader (ids as `inspect` prints them after `set`)".
- Modify: `crates/engine/src/manifest.rs` `ManifestRecord`: `#[serde(default, skip_serializing_if = "Option::is_none")] pub shader: Option<crate::shader::ShaderPick>`.
- Modify: `crates/engine/src/pipeline.rs` (the `ManifestRecord { … }` literal at `:423`; the plan had `:445`): `shader: job.shader.clone(),`.
- Test: `crates/cli/tests/cli_inspect_validate.rs`, `crates/cli/tests/cli_convert.rs`, `crates/engine/tests/events_manifest.rs`.

**Interfaces:**
- Consumes: `SetResolved { shader, warnings }`, `ResolvedJob::shader` (Task 4).
- Produces: the `set <key>  shader <id> (<provenance>)` human line and `ManifestRecord::shader`.

- [ ] **Step 1: Write the failing tests**

In `cli_inspect_validate.rs` (use the file's `sdc()`, `code`, `text` helpers):

```rust
#[test]
fn inspect_prints_the_set_shader_and_its_warnings() {
    let d = tempfile::tempdir().unwrap();
    let a = d.path().join("in/textures/a");
    std::fs::create_dir_all(&a).unwrap();
    ImageBuffer::from_fn(16, 16, |_, _| Rgb([120u8, 90, 60])).save(a.join("wall.png")).unwrap();
    ImageBuffer::from_fn(16, 16, |_, _| Rgb([128u8, 128, 255])).save(a.join("wall_n.png")).unwrap();
    let o = sdc()
        .args(["inspect", "--recursive", "--target", "vanilla", "--set", "set:wall.shader=glow"])
        .arg(d.path().join("in"))
        .output()
        .unwrap();
    assert_eq!(code(&o), 0, "{}", text(&o.stderr));
    let out = text(&o.stdout);
    assert!(out.contains("shader glow (set)"), "{out}");
    assert!(out.contains("SHADER-MISSING-MAP"), "{out}");
}

#[test]
fn a_bad_set_scope_is_a_usage_error() {
    let d = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(d.path().join("in")).unwrap();
    let o = sdc().args(["inspect", "--set", "set:wall.shader=nope"]).arg(d.path().join("in")).output().unwrap();
    assert_eq!(code(&o), 2);
    assert!(text(&o.stderr).contains("unknown shader"), "{}", text(&o.stderr));
}
```

In `cli_convert.rs`, add `the_manifest_records_the_shader`. Convert a `wall.png` + `wall_n.png` folder under `--target vanilla` with `--set set:wall.shader=hair`, read the manifest the way the file's existing manifest tests do, and assert that the `wall.dds` record has `"shader": {"shader": "hair", "provenance": "set"}`.

In `events_manifest.rs`, make the existing `ManifestRecord` round-trip (if any) include `shader: Some(ShaderPick { shader: Shader::Default, provenance: ShaderOrigin::Fallback, wanted: Some(Shader::Pbr) })`, and assert that it round-trips through JSON.

- [ ] **Step 2: Run them and check that they fail**

Run: `cargo test -p sdc --test cli_inspect_validate inspect_prints_the_set_shader && cargo test -p sdc --test cli_convert the_manifest_records_the_shader`
Expected: FAIL (no `shader glow` line; no `shader` key in the manifest).

- [ ] **Step 3: Implement** as listed.

- [ ] **Step 4: Run the tests and check that they pass**

Run: `cargo test --workspace`
Expected: PASS. A CLI test asserting the old "set line only with recipes" output changes: name it in the report.

- [ ] **Step 5: Run the gates, regenerate the schema** (`manifest.schema.json`) **and commit**

```bash
cargo run -p sdc -- presets schema all --out schema
git add crates/cli/src/verbs.rs crates/cli/src/args.rs crates/engine/src/manifest.rs crates/engine/src/pipeline.rs crates/cli/tests/cli_inspect_validate.rs crates/cli/tests/cli_convert.rs crates/engine/tests/events_manifest.rs schema
git commit -m "feat(cli): plan/inspect print each set's shader and SHADER-* notes; the manifest records the shader (M6 spec §4.4)

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

---

### Task 7: GUI — the Shader dropdown on the set card, `sdc-project.toml`, set warnings, and the mesh votes

**Spec:** §7 (dropdown with the mode's shaders, "(auto)", choosing writes `shader`, "Auto" removes it, warnings on the card, switching mode re-resolves), §5 PGPatcher JSON unchanged, §9 GUI model tests, §12 mesh-reported shader first (the votes come from the scan database; "(auto)" covers `auto:mesh` as every `auto:` provenance).

**Files:**
- Modify: `crates/gui/src/sets_doc.rs`:
  - `RESERVED: [&str; 7]` adds `"shader"`;
  - `SetEntry` gets `pub shader: Option<engine::shader::Shader>`, parsed in `entry_of` from `t.get("shader").and_then(Item::as_str)` via `serde_json::from_value(Json::String(..)).ok()`. An unknown id reads as `None`, and the raw value stays in the file;
  - `pub fn set_shader(&mut self, id: &str, shader: Option<engine::shader::Shader>)`: no-op when `read_only`; inserts `shader = "<id>"` or removes the key; sets `dirty`.
- Modify: `crates/gui/src/model/items.rs`: `SetItem` gets `pub shader: Option<ShaderPick>` and `pub warnings: Vec<Warning>`, filled from `SetResolved { shader, warnings, .. }`.
- Modify: `crates/gui/src/model/sets.rs`:
  - `SetView` gets `pub shader: Option<ShaderPick>` (the engine's pick, from `m.sets`), `pub shader_choice: Option<Shader>` (the saved choice, from `SetEntry`) and `pub set_warnings: Vec<Warning>`, filled in `set_views`;
  - `pub fn set_shaders(m: &AppModel) -> BTreeMap<String, Shader>` (entry id → saved choice);
  - `pub fn shader_options(target: Target) -> Vec<Shader>` = `Shader::available(target).to_vec()`;
  - `impl AppModel { pub fn set_shader(&mut self, set: &str, shader: Option<Shader>) }` via `self.edit_docs("Shader", None, true, move |_, d| d.set_shader(&id, shader))`, guarded by `sets_editable()`.
- Modify: `crates/gui/src/model/mod.rs`: `EngineInputs` gets `set_shaders`, filled in `of` and in `replan`'s `last_inputs`; `request()` sets `req.set_shaders = sets::set_shaders(self);`.
- Modify: `crates/gui/src/ui/sets_column.rs` `pill`. In the badge row, before the warnings badge, and only when `!v.key.is_empty()`:
  - show `egui::ComboBox::from_id_salt(("shader", &v.id))`. Its `selected_text` is the pick's label plus " (auto)" when the provenance is `auto:`, " (fallback)" when it is `fallback`, and nothing for `set`;
  - entries: "Auto", then `shader_options(model.target)` by `label()`. Picking pushes a new `Act::Shader(v.id.clone(), Option<Shader>)`, applied in `draw` with the other acts via `model.set_shader`;
  - disable it (`ui.add_enabled_ui(editing.on, …)`) when sets are not editable;
  - chain `v.set_warnings` into `rows` as `(v.label.as_str(), w)`, so `SHADER-EXTRA-MAP`/`-MISSING-MAP`/`-UNAVAILABLE` count in the ⚠ badge and its hover popup. `SHADER-AUTO` is Info and drops out through `counts_toward_badge`, as other Info codes do.
- **Mesh votes (S-shader-10; owner: mesh-reported shader first).**
  - Modify `crates/gui/src/model/meshes.rs` (next to the other store reads; `rusqlite` and `modscan` are already GUI dependencies; `crates/modscan` itself is not touched):

```rust
/// M6 Track A (DECISIONS S-shader-10): for each key (`engine::shader::mesh_key`), the shader of every lighting shape
/// that uses `<key>.dds` in slot 0, in non-derived copies of enabled origins. PGPatcher's output copies and TruePBR
/// shapes (SLSF2 bit 23) do not vote: sdc's own rules decide PBR. Types outside the 12 shaders do not vote
/// (`Shader::from_lighting_type`). Keys without a vote are left out.
pub fn mesh_votes(db: &Path, keys: &BTreeSet<String>) -> Result<BTreeMap<String, Vec<Shader>>, String> {
    let store = Store::open_read_only(db).map_err(db_err)?;
    // `DISTINCT` per NIF path and block: the same mesh in two mods votes once.
    let mut st = store
        .conn()
        .prepare(
            "SELECT DISTINCT c.path_id, s.block, s.shader_type FROM path tp
             JOIN tex_use tu ON tu.tex_path_id = tp.id AND tu.slot = 0
             JOIN shape s ON s.texset_id = tu.texset_id AND s.shader_kind = 1
                          AND (ifnull(s.slsf2, 0) & 8388608) = 0
             JOIN nif_copy c ON c.id = s.copy_id AND c.is_derived = 0
             JOIN origin o ON o.id = c.origin_id AND o.enabled = 1
             WHERE tp.p = ?1",
        )
        .map_err(db_err)?;
    let mut out = BTreeMap::new();
    for k in keys {
        let types = st
            .query_map([format!("{k}.dds")], |r| r.get::<_, Option<i64>>(2))
            .and_then(|rows| rows.collect::<Result<Vec<_>, _>>())
            .map_err(db_err)?;
        let v: Vec<Shader> = types
            .into_iter()
            .flatten()
            .filter_map(|t| u32::try_from(t).ok().and_then(Shader::from_lighting_type))
            .collect();
        if !v.is_empty() {
            out.insert(k.clone(), v);
        }
    }
    Ok(out)
}
```

  - Modify `crates/gui/src/model/mod.rs` (a NIF-changed file: additive hunks only):
    - `AppModel` gets `pub scan_db: Option<PathBuf>` (doc: "The scan database the mesh votes read (`meshes::default_store_path()`), set by `main`; `None` in tests so a developer's scan cannot leak in") and `pub mesh_votes: BTreeMap<String, Vec<Shader>>` (doc: "S-shader-10: `shader::mesh_key` → what the scanned meshes report; sent as `BatchRequest::mesh_shaders`"), both empty in `new`;
    - `pub fn refresh_mesh_votes(&mut self) -> bool`: keys = `mesh_key(&job.game_path, <job.output file stem>)` of every item job with `pack.is_none()`; votes = `meshes::mesh_votes(db, &keys)` when `scan_db` is a file, else empty. A read error also means no votes (the suffix rules decide; the Meshes view reports database errors). Stores the votes and returns whether they changed;
    - `EngineInputs` gets `mesh_votes: BTreeMap<String, Vec<Shader>>`: `of` copies `m.mesh_votes`, `replan`'s `last_inputs` copies `req.mesh_shaders`; `request()` sets `req.mesh_shaders = self.mesh_votes.clone();`;
    - in the plan-landed handler, call `self.refresh_mesh_votes();` right before `let now = EngineInputs::of(self);` (the bounded auto-replan then plans once more with the votes: the first plan of a folder has no game paths yet).
  - Modify `crates/gui/src/app.rs` (a NIF-changed file), where `self.meshes.poll();` is called in `update`:

```rust
        let scanning = self.meshes.scanning();
        self.meshes.poll();
        // S-shader-10: a finished scan may change what the meshes report.
        if scanning && !self.meshes.scanning() && self.model.refresh_mesh_votes() {
            self.model.replan();
        }
```

  - Modify `crates/gui/src/main.rs`: next to `m.user_config = user_config_path();`, add `m.scan_db = sdc_gui::model::meshes::default_store_path();`.
- Test: `crates/gui/tests/sets_doc.rs`, `crates/gui/tests/sets_model.rs` (also the mesh votes: it has the `pbr_opened` fixture), `crates/gui/tests/pbr_export.rs`.

**Interfaces:**
- Consumes: `engine::shader::{Shader, ShaderPick, ShaderOrigin, mesh_key}`, `Shader::from_lighting_type`, `BatchRequest::{set_shaders, mesh_shaders}`, `SetResolved { shader, warnings }`, `modscan::store::Store::{open_read_only, conn}`.
- Produces: `SetsDoc::set_shader`, `SetEntry::shader`, `AppModel::set_shader`, `sets::{set_shaders, shader_options}`, `SetView::{shader, shader_choice, set_warnings}`, `SetItem::{shader, warnings}`, `meshes::mesh_votes`, `AppModel::{scan_db, mesh_votes, refresh_mesh_votes}`.

- [ ] **Step 1: Write the failing tests**

`crates/gui/tests/sets_doc.rs`, using the same `SetsDoc::open(tempdir)` pattern as the file's existing tests:

```rust
#[test]
fn shader_is_reserved_not_a_json_setting() {
    use engine::shader::Shader;
    let d = tempfile::tempdir().unwrap();
    let mut s = sdc_gui::sets_doc::SetsDoc::open(d.path());
    s.set_shader("landscape/leaf", Some(Shader::Hair));
    let e = s.entries().into_iter().find(|e| e.id == "landscape/leaf").unwrap();
    assert_eq!(e.shader, Some(Shader::Hair));
    assert!(!e.settings.contains_key("shader"), "{:?}", e.settings);
    assert!(s.text().contains("shader = \"hair\""), "{}", s.text());
    s.set_shader("landscape/leaf", None);
    assert!(!s.text().contains("shader"), "{}", s.text());
}
```

`crates/gui/tests/sets_model.rs` (the fixture: `landscape/rock01` packs `_rmaos`; `landscape/leaf` is under `textures/landscape`):

```rust
#[test]
fn shader_dropdown_lists_match_the_mode() {
    use engine::{config::Target, shader::Shader};
    assert_eq!(shader_options(Target::Cs).len(), 12);
    assert!(!shader_options(Target::Vanilla).contains(&Shader::Pbr));
    assert_eq!(shader_options(Target::Cpm), shader_options(Target::Vanilla));
}

#[test]
fn choosing_and_clearing_a_shader_round_trips_in_sdc_project_toml() {
    use engine::shader::{Shader, ShaderOrigin};
    let d = tempfile::tempdir().unwrap();
    let mut m = pbr_opened(d.path());
    idle(&mut m);
    let v = set_views(&m);
    assert_eq!(view(&v, "landscape/leaf").shader.as_ref().unwrap().shader, Shader::Landscape);
    assert!(matches!(view(&v, "landscape/leaf").shader.as_ref().unwrap().provenance, ShaderOrigin::Auto(_)));
    m.set_shader("landscape/leaf", Some(Shader::Hair));
    idle(&mut m);
    let v = set_views(&m);
    let leaf = view(&v, "landscape/leaf");
    assert_eq!((leaf.shader_choice, leaf.shader.as_ref().unwrap().provenance.clone()), (Some(Shader::Hair), ShaderOrigin::Set));
    assert!(m.sets_doc.as_ref().unwrap().text().contains("shader = \"hair\""));
    assert_eq!(set_shaders(&m).get("landscape/leaf"), Some(&Shader::Hair));
    m.set_shader("landscape/leaf", None);
    idle(&mut m);
    assert!(!m.sets_doc.as_ref().unwrap().text().contains("shader"));
    assert_eq!(view(&set_views(&m), "landscape/leaf").shader_choice, None);
}

#[test]
fn switching_mode_falls_back_and_back() {
    use engine::{config::Target, codes::Code, shader::{Shader, ShaderOrigin}};
    let d = tempfile::tempdir().unwrap();
    let mut m = pbr_opened(d.path());
    idle(&mut m);
    m.set_shader("landscape/rock01", Some(Shader::Pbr));
    idle(&mut m);
    m.set_target(Target::Vanilla);
    idle(&mut m);
    let v = set_views(&m);
    let rock = view(&v, "landscape/rock01");
    let pick = rock.shader.as_ref().unwrap();
    assert_eq!((pick.shader, &pick.provenance), (Shader::Default, &ShaderOrigin::Fallback));
    assert!(rock.set_warnings.iter().any(|w| w.code == Code::ShaderUnavailable));
    assert_eq!(rock.shader_choice, Some(Shader::Pbr), "the saved choice stays");
    m.set_target(Target::Cs);
    idle(&mut m);
    assert_eq!(view(&set_views(&m), "landscape/rock01").shader.as_ref().unwrap().shader, Shader::Pbr);
}

/// A scan database where `tex` (a `textures/…` path with `.dds`) is slot 0 of these shapes:
/// (lighting shader type, SLSF2, shader kind, derived copy, enabled origin).
fn scan_db(db: &std::path::Path, tex: &str, shapes: &[(u32, u32, i32, bool, bool)]) {
    let s = modscan::store::Store::open(db).unwrap();
    let c = s.conn();
    c.execute("INSERT INTO path(id, p) VALUES (1, ?1)", [tex]).unwrap();
    c.execute_batch("INSERT INTO texset(id, n, t0) VALUES (1, 1, 1);
                     INSERT INTO tex_use(tex_path_id, texset_id, slot) VALUES (1, 1, 0);").unwrap();
    for (i, (ty, slsf2, kind, derived, enabled)) in shapes.iter().enumerate() {
        let id = i as i64 + 2;
        c.execute("INSERT INTO path(id, p) VALUES (?1, ?2)", rusqlite::params![id, format!("meshes/m{id}.nif")]).unwrap();
        c.execute("INSERT INTO origin(id, kind, name, root, priority, enabled) VALUES (?1, 1, ?2, ?2, ?1, ?3)",
            rusqlite::params![id, format!("mod{id}"), enabled]).unwrap();
        c.execute("INSERT INTO nif_copy(id, path_id, origin_id, parse_status, is_derived) VALUES (?1, ?1, ?1, 0, ?2)",
            rusqlite::params![id, derived]).unwrap();
        c.execute("INSERT INTO shape(copy_id, block, kind, shader_kind, shader_type, slsf2, texset_id) VALUES (?1, 0, 'BSTriShape', ?2, ?3, ?4, 1)",
            rusqlite::params![id, kind, ty, slsf2]).unwrap();
    }
}

#[test]
fn mesh_votes_count_enabled_source_lighting_shapes_only() {
    use engine::shader::Shader;
    let d = tempfile::tempdir().unwrap();
    let db = d.path().join("scan.db");
    scan_db(&db, "textures/a/wall.dds", &[
        (6, 0, 1, false, true),          // votes hair
        (6, 1 << 23, 1, false, true),    // TruePBR: no vote
        (0, 0, 2, false, true),          // an effect shader: no vote
        (0, 0, 1, true, true),           // PGPatcher's output copy: no vote
        (0, 0, 1, false, false),         // a disabled mod: no vote
        (4, 0, 1, false, true),          // Face Tint: not one of the 12, no vote
    ]);
    let keys = ["textures/a/wall".to_string(), "textures/a/none".to_string()].into_iter().collect();
    let v = sdc_gui::model::meshes::mesh_votes(&db, &keys).unwrap();
    assert_eq!(v.get("textures/a/wall"), Some(&vec![Shader::Hair]));
    assert!(!v.contains_key("textures/a/none"));
}

#[test]
fn a_scan_database_picks_the_set_shader() {
    use engine::shader::{AutoRule, Shader, ShaderOrigin};
    let d = tempfile::tempdir().unwrap();
    let mut m = pbr_opened(d.path());
    idle(&mut m);
    // The key the GUI asks for, taken from the plan: the leaf diffuse's game folder and output stem.
    let j = m.items.iter().filter_map(|i| i.job.as_ref())
        .find(|j| j.set_key.ends_with("landscape/leaf") && j.output.file_stem().is_some_and(|s| s == "leaf"))
        .expect("the leaf diffuse job");
    let key = engine::shader::mesh_key(&j.game_path, "leaf");
    let dbdir = tempfile::tempdir().unwrap();
    let db = dbdir.path().join("scan.db");
    scan_db(&db, &format!("{key}.dds"), &[(6, 0, 1, false, true)]);
    m.scan_db = Some(db);
    assert!(m.refresh_mesh_votes());
    m.replan();
    idle(&mut m);
    let leaf = view(&set_views(&m), "landscape/leaf").shader.clone().unwrap();
    assert_eq!((leaf.shader, leaf.provenance), (Shader::Hair, ShaderOrigin::Auto(AutoRule::Mesh)));
    // No database: the suffix rules again (the leaf is under textures/landscape).
    m.scan_db = None;
    assert!(m.refresh_mesh_votes());
    m.replan();
    idle(&mut m);
    assert_eq!(view(&set_views(&m), "landscape/leaf").shader.as_ref().unwrap().shader, Shader::Landscape);
}
```

If the fixture's leaf diffuse has another file name, adjust the `find` to it; the test must take the key from the plan, never hard-code it. `rusqlite` and `modscan` are normal dependencies of `sdc_gui`, so the integration test can use them.

`crates/gui/tests/pbr_export.rs`:

```rust
#[test]
fn an_explicit_shader_does_not_change_which_sets_get_json() {
    use engine::shader::Shader;
    let d = tempfile::tempdir().unwrap();
    let mut m = ready(d.path());
    let before: Vec<String> = json_entries(&m).into_iter().map(|(v, _)| v.id).collect();
    m.set_shader("landscape/rock01", Some(Shader::Glow));
    m.set_shader("landscape/leaf", Some(Shader::Pbr));
    idle(&mut m);
    let after: Vec<String> = json_entries(&m).into_iter().map(|(v, _)| v.id).collect();
    assert_eq!(before, after);
    assert!(after.contains(&"landscape/rock01".to_string()) && !after.contains(&"landscape/leaf".to_string()));
}
```

- [ ] **Step 2: Run them and check that they fail**

Run: `cargo test -p sdc_gui --test sets_doc --test sets_model --test pbr_export`
Expected: compile errors (`set_shader`, `shader_options`, `SetView::shader`, `mesh_votes`, `AppModel::scan_db` missing).

- [ ] **Step 3: Implement** as listed. Check that `crates/gui/tests/ui_smoke.rs` still renders the sets column (the `ComboBox` must not panic on an empty `shader`).

- [ ] **Step 4: Run the tests and check that they pass**

Run: `cargo test -p sdc_gui`
Expected: PASS, including `ui_smoke`, `gui_repro` (`cargo test -p sdc --test gui_repro`) and `set_editor_ui`.

- [ ] **Step 5: Run the gates, then commit**

```bash
git add crates/gui/src/sets_doc.rs crates/gui/src/model/items.rs crates/gui/src/model/sets.rs crates/gui/src/model/mod.rs crates/gui/src/ui/sets_column.rs crates/gui/src/model/meshes.rs crates/gui/src/app.rs crates/gui/src/main.rs crates/gui/tests/sets_doc.rs crates/gui/tests/sets_model.rs crates/gui/tests/pbr_export.rs
git commit -m "feat(gui): Shader dropdown on the set card, saved in sdc-project.toml; shader warnings on the card; mesh votes from the scan database (M6 spec §7, §12)

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

---

### Task 8: Goldens — one plan case per shader

**Spec:** §9 "One golden case per shader for `plan` output", plus the CS-default review focus.

**Files:**
- Modify: `crates/engine/tests/golden.rs` (new cases at the end; reuse `put`, `rgb`, `rgba`, `flat_normal`, `gray_ramp`, `planned`, `assert_profile`).

**Helper** (add once):

```rust
/// Plan only (no run): the `set_resolved` shader, provenance string and set warning codes of the set whose key ends
/// with `tail`.
fn plan_shader(dir: &Path, target: Target, sets: &[&str], tail: &str) -> (Out, String, String, Vec<Code>) {
    let sink = Arc::new(VecSink::default());
    let rep = Reporter::new(sink.clone(), "golden");
    Engine::new().unwrap().plan(request(dir, target, sets, &[]), &rep, &CancelToken::default()).unwrap();
    let events = sink.take();
    let (shader, prov, codes) = events.iter().find_map(|e| match &e.kind {
        EventKind::SetResolved { shader: Some(p), warnings, .. } if e.set.as_deref().is_some_and(|s| s.ends_with(tail)) =>
            Some((p.shader.id().to_string(), p.provenance.to_string(), warnings.iter().map(|w| w.code).collect())),
        _ => None,
    }).unwrap_or_else(|| panic!("no set_resolved for {tail}"));
    (Out { summary: BatchSummary::default(), events }, shader, prov, codes)
}
```

(If `BatchSummary` has no `Default`, change `Out` for these cases to hold only `events`, and add a small `PlanOut` struct.)

**Cases.** Each is its own `#[test]`, named `shader_<id>_…`. The `golden` `request` helper uses `AcceptSuggestions::High`.

| Test | Target | Files under `in/` | Expect shader, provenance | Set codes | Extra assertion |
|---|---|---|---|---|---|
| `shader_pbr_auto_from_rmaos` | cs | `textures/a/rock.png` rgb, `rock_n.png` flat_normal, `rock_rmaos.png` rgb | `pbr`, `auto:pbr` | `[ShaderAuto]` | `assert_profile(.., "rock.png", "pbr_albedo", ..)` |
| `shader_advanced_skin_auto_from_rfaos` | cs | `textures/b/body.png`, `body_n.png`, `body_rfaos.png` | `advanced_skin`, `auto:advanced_skin` | `[ShaderAuto]` | diffuse is `skin_albedo` |
| `shader_default_is_vanillas_default` | vanilla | `textures/a/wall.png`, `wall_n.png` | `default`, `auto:default` | `[ShaderAuto]` | — |
| `shader_envmap_auto_from_m` | vanilla | + `wall_m.png` gray_ramp | `envmap`, `auto:env_m` | `[ShaderAuto]` | `_m` format `bc4`, `provenance.format == "shader:envmap"` |
| `shader_complex_material_auto_from_m_alpha` | cs | `wall.png`, `wall_n.png`, `wall_m.png` rgba with varying alpha | `complex_material`, `auto:complex_m` | `[ShaderAuto]` | — |
| `shader_glow_auto_from_g` | vanilla | `lamp.png`, `lamp_n.png`, `lamp_g.png` | `glow`, `auto:g` | `[ShaderAuto]` | — |
| `shader_parallax_auto_from_p` | cs | `stone.png`, `stone_n.png`, `stone_p.png` gray_ramp | `parallax`, `auto:p` | `[ShaderAuto]` | `_p` format `bc4`, `shader:parallax` |
| `shader_skin_auto_from_sk` | vanilla | `face.png`, `face_n.png`, `face_sk.png` | `skin`, `auto:sk` | `[ShaderAuto]` | — |
| `shader_hair_auto_from_path` | vanilla | `textures/hair/braid.png` rgba near-binary alpha, `braid_n.png` | `hair`, `auto:hair_path` | `[ShaderAuto]` | diffuse alpha `cutout`, `mips.coverage` `on` |
| `shader_eye_auto_from_path` | vanilla | `textures/eyes/eye.png`, `eye_n.png` | `eye`, `auto:eyes_path` | `[ShaderAuto]` | `_n` sub-type `msn` |
| `shader_multilayer_auto_from_i` | vanilla | `glass.png` rgba soft alpha, `glass_n.png`, `glass_i.png` | `multilayer`, `auto:i` | `[ShaderAuto]` | diffuse alpha `data` |
| `shader_landscape_auto_from_path` | vanilla | `textures/landscape/dirt.png`, `dirt_n.png` | `landscape`, `auto:landscape` | `[ShaderAuto]` | diffuse `landscape_diffuse` |
| `shader_pbr_under_vanilla_falls_back` | vanilla | the `pbr` fixture | `default`, `fallback` | `[ShaderUnavailable, ShaderExtraMap]` | the extra map is `rock_rmaos.dds` (`rmaos` is not a `default` role) |
| `cs_plain_set_is_default_not_pbr` | cs | `wall.png`, `wall_n.png` | `default`, `auto:default` | `[ShaderAuto]`: no `SHADER-MISSING-MAP` and no `SHADER-EXTRA-MAP` | `warnings(&o)` (file-level) is `[]` |
| `shader_coat_alias_is_the_s_coat_colour` | cs | `rock.png`, `rock_n.png`, `rock_rmaos.png`, `rock_coat.png` | `pbr`, `auto:pbr` | `[ShaderAuto]` (`rock_coat` fills the `_s` coat-colour role) | `assert_profile(.., "rock_coat.png", "pbr_subsurface", ..)`; its job output ends with `rock_s.dds` |

Check the rows' exact code lists against the Task 3 table:
- a CS plain set: diffuse is `diffuse` and `_n` is `normal`, both extra under `pbr`; albedo, `_n` and `_rmaos` are missing;
- the `rfaos` fixture: `_n` is `pbr_normal`, which fills the `_n/_msn` role.

Write the asserted list from what the table implies. If the engine disagrees, the table or the fixture is wrong: fix the code, not the expectation, unless the table itself is wrong (then report it).

- [ ] **Step 1: Write the 14 failing tests** per the table, each built like:

```rust
#[test]
fn shader_glow_auto_from_g() {
    let d = tempfile::tempdir().unwrap();
    put(d.path(), "textures/a/lamp.png", rgb(16));
    put(d.path(), "textures/a/lamp_n.png", flat_normal(16));
    put(d.path(), "textures/a/lamp_g.png", rgb(16));
    let (_, shader, prov, codes) = plan_shader(d.path(), Target::Vanilla, &[], "textures/a/lamp");
    assert_eq!((shader.as_str(), prov.as_str()), ("glow", "auto:g"));
    assert_eq!(codes, vec![Code::ShaderAuto]);
}
```

- [ ] **Step 2: Run them.** Run: `cargo test -p engine --test golden shader_ cs_plain_set`. Expected: PASS, since Tasks 1–5 implement the behaviour; a failure here is a bug in Tasks 1–5. If a case fails, fix the engine code, and state the fix in the report.

- [ ] **Step 3: Run the whole golden suite.** Run: `cargo test -p engine --test golden`. Expected: PASS, with no pre-existing case changed. If one did change in Task 5 (e.g. a fixture under `hair/`), it was already reported there.

- [ ] **Step 4: Run the gates, then commit**

```bash
git add crates/engine/tests/golden.rs
git commit -m "test(golden): one plan case per shader, the vanilla fallback and the CS plain-set default (M6 spec §9)

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

---

### Task 9: Docs, DECISIONS Note S and the workspace version bump

**Spec:** §10 (bump the minor so `if-changed` re-encodes), decisions note.

**Files:**
- Modify: `docs/DECISIONS.md`. `## Note S` exists: append a `### M6 Track A: shader modes (plan rulings, 2026-10-11)` subsection after the Track C one (and after its Deferred bullet). One bullet each, in this form: `- **S-shader-N <title> (owner / plan, 2026-10-11).** <ruling>`:
  - **S-shader-1** `--set` set scope: there was none; `set:<id>.shader=<shader>`, `shader` only, the id matching rules, `SET-EDIT-STALE` for no match, the command line beats the GUI.
  - **S-shader-2** `sdc-project.toml`: `shader` goes in the `[[sets]]` entry (the spec's `[sets."<base>"]` adapted); it is a reserved key, never a JSON setting.
  - **S-shader-3** Precedence: the full order as in Task 5, plus the Provenance placement (between `analysis:<fact>` and `suffix:<term>`) and the hair no-alpha consequence.
  - **S-shader-4** Sub-type effects (eye `_n` → `msn`, landscape colour → `landscape_diffuse`): explicit → decided; auto → High suggestion.
  - **S-shader-5** Member defaults table; multilayer = `data` on varying alpha (owner: the alpha is the layer mask, kept exact). Finding: `envmask`/`height` were already BC4 in `sdc-base`, so the §6 "new" rows change provenance, not bytes. No re-typing of `_m`.
  - **S-shader-6** `_i` added to the §6.1 suffix list (grouping change) and suggests `diffuse` Medium (owner: yes). Rule 12 auto-picks `default`, never `pbr` without a PBR member (owner); `default_for` (`pbr` on CS) is only the dropdown's first entry and the fallback shader.
  - **S-shader-7** Planning order: pick after pass 1, re-resolve members of shaders with defaults, panic isolation kept. Warnings on `set_resolved`, not as `warning` events. `EVENT_SCHEMA` stays 2 (additive fields).
  - **S-shader-8** Rule 2 is target-independent, so a PBR set under vanilla/cpm auto-picks `pbr` and falls back with `SHADER-UNAVAILABLE`. PGPatcher JSON still follows `is_pbr_diffuse` (P5/C1), not the shader.
  - **S-shader-9** `_coat` (owner: "_coat (for the coat diffuse)"). It is a built-in §4.4 vocabulary term (`VocabRole::CoatColour`, canonical `s`), not a preset alias:
    - `x_coat.png` joins set `x` as its `_s` member, `pbr_subsurface` (High), slot 8, written `x_s.dds`;
    - it collides with `x_s.png`, and with a re-encoded `x_s.dds` (Track B S6), as `IN-DUPLICATE`;
    - it is blocked by `CFG-UNAVAILABLE` under vanilla/cpm;
    - a user preset alias `coat` is now refused, because aliases may not be vocabulary terms.
  - **S-shader-10** Mesh-reported shader first (owner, 2026-10-11): order explicit → advanced skin → PBR → mesh majority (`auto:mesh`) → suffix rules 3–12. Evidence = `BatchRequest::mesh_shaders` keyed by `shader::mesh_key` (diffuse game folder + stem), filled by the GUI from the scan database (lighting shapes, slot 0, non-derived copies of enabled origins, not TruePBR); `Shader::from_lighting_type` maps 0/1/2/3/5/6/8/11/16, any other type is no vote. Strict majority, else the suffix rules; an `envmap` majority with a complex `_m` is `complex_material`. PBR is never read from a NIF (the TruePBR bit is PGPatcher's output marker; a shipped mesh of a PBR set says Default), so a PBR or skin member always wins. The CLI sends no votes. `modscan` untouched: raw SQL on `Store::conn()`.
  - **S-shader-11** Track C interaction (S-mips-0/-13): the shader layer runs before `unscoped_filter_falls_back`, so a hair diffuse's `cutout` makes an unscoped non-box filter box + `CFG-FILTER-IGNORED` and a scoped one `CFG-INVALID`. Catalogue count 69 → 73; workspace version 0.3.0 → 0.4.0.
- Modify: `docs/gui.md`: in the set card section, add a short paragraph on the Shader dropdown ("(auto)", "(fallback)", "Auto" clears, warnings on the ⚠ badge, and that a scanned mod's meshes decide the auto pick unless a PBR member does).
- Modify: `docs/superpowers/specs/2026-10-08-skyrim-dds-converter-design.md` §6.1 suffix list: add `_i` with "(M6, Note S-shader-6)". In the §4.4 vocabulary table, add the `_coat` → `_s` (`pbr_subsurface`, coat colour) row "(M6, Note S-shader-9)". Make only these two edits.
- Modify: `Cargo.toml` `[workspace.package] version` `0.3.0` → **`0.4.0`** (main 8159bdf is 0.3.0: Track B bumped it, Track C did not, S-mips-9). Then `cargo build` updates `Cargo.lock`.
- Test: none new. `ENGINE_VERSION` follows `CARGO_PKG_VERSION`.

- [ ] **Step 1: Write the DECISIONS, gui.md and spec edits.**
- [ ] **Step 2: Bump the version.** Run: `cargo build --workspace`. Expected: `Cargo.lock` shows only the workspace crates' version lines changed (`git diff --stat Cargo.lock`).
- [ ] **Step 3: Run the gates (unpiped).** Expected: all green.
- [ ] **Step 4: Commit (two commits, as the M4 precedent did)**

```bash
git add docs/DECISIONS.md docs/gui.md docs/superpowers/specs/2026-10-08-skyrim-dds-converter-design.md
git commit -m "docs: DECISIONS Note S shader rulings (S-shader-1..11), GUI shader dropdown, _i suffix (M6 Track A)

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
git add Cargo.toml Cargo.lock
git commit -m "chore: workspace version 0.4.0 so if-changed re-encodes outputs the shader defaults changed (hair, eye _n, multilayer, _i, mesh-picked shaders)

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

---

## Owner answers (plan review, 2026-10-11; spec §12, commit 92919f9)

1. **Rule 12 → `default`.** No set without a PBR member auto-picks `pbr`, even on CS. Covered by Task 2 (rule 12, test row) and Task 8 (`cs_plain_set_is_default_not_pbr`).
2. **`_i`: yes.** It joins sets and suggests `diffuse` (Medium). Covered by Task 2 and S-shader-6.
3. **Multilayer diffuse alpha = `data`.** Covered by Task 5 and S-shader-5.
4. **New: `_coat`** names the PBR coat colour, i.e. the `_s` member (`pbr_subsurface`, slot 8, written `<base>_s.dds`). Covered by Task 2, the Task 8 golden and S-shader-9.
5. **Mesh-reported shader first** (owner, 2026-10-11, after the NIF merge; spec §12). The scanned meshes' majority shader is used after the PBR/skin rules and before suffix rules 3–12. Covered by Tasks 1, 2, 4 and 7 and S-shader-10.

## Spec coverage

| Spec | Task |
|---|---|
| §2 mode = target, cpm uses the vanilla list | 1 (`available`), 2 |
| §3 table as data in one module, map identity, `_m` optional for envmap | 1, 3 |
| §4.1 explicit (TOML, `--set`), ruling in Note S | 4, 7, 9 |
| §4.2 auto-pick rules 1–12 and order | 2, 8 |
| §4.3 unavailable → fallback + `SHADER-UNAVAILABLE` | 2, 3, 4 |
| §4.4 provenance in plan/inspect/manifest/GUI | 4, 6, 7 |
| §5 member defaults, precedence in Note S | 5, 9 |
| §5 extra/missing maps, nothing generated | 3, 4 |
| §5 PGPatcher JSON unchanged | 7 (`an_explicit_shader_does_not_change_which_sets_get_json`) |
| §6 BC4 single-channel rows | 5 (finding recorded in S-shader-5) |
| §7 GUI dropdown, "(auto)", Auto clears, warnings, mode switch | 7 |
| §8 four codes, count 69 + 4 = 73 | 3 |
| §9 tests | 2, 3, 4, 5, 7, 8 |
| §10 version bump | 9 |
| §11 Track D boundary | nothing to do (no preset verbs touched) |
| §12 amendments: rule 12 → `default`, multilayer `data`, `_i`, `_coat` | 2, 5, 8, 9 |
| §12 amendment: mesh-reported shader first | 1 (`from_lighting_type`, `mesh_key`), 2 (`pick` order), 4 (`BatchRequest::mesh_shaders`), 7 (scan DB votes), 9 (S-shader-10) |
