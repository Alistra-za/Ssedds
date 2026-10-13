# sdc M6 Track E — NIF-aware alpha and fewer confirmations Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** A file with no suffix is the set's colour member at High confidence. `_coat` is coat colour + strength. The GUI accepts High suggestions, and an accepted one reads "(auto)". A colour member with varying alpha takes its alpha policy from the scanned meshes that use it (provenance `mesh:alpha`), so the alpha question is not asked.

**Architecture:**
- **Suggestions.** In `suggest::suggest`, the bare-stem colour arms (`diffuse`, `pbr_albedo`, `ui_sky`) become High. Note H's vocabulary colour raise in `apply_vocabulary` becomes vacuous and is deleted. The `_coat` reason text changes.
- **Rule.** `shader::MeshAlpha` and `shader::mesh_alpha_policy` hold the owner's table as a pure function, beside `colour_mesh_key`.
- **Layer.**
  - `BatchRequest::mesh_alpha` reaches `ResolveCtx::mesh_alpha`, a field beside `shaders`, with a `NO_MESH_ALPHA` static.
  - `layered_profile_with` applies a new `Provenance::MeshAlpha` (`mesh:alpha`) after the analysis-alpha step and `skin_wet_form` and before the shader layer, only where `alpha.policy` is not yet explicit.
  - The layer counts as explicit itself, so `resolve_decided` does not ask the alpha question.
- **GUI evidence.** `meshes::mesh_alpha` reads `alpha_flags`/`alpha_threshold` with the same raw-SQL source rules as `mesh_votes`. `refresh_mesh_votes` fills both maps, and `EngineInputs` carries both, so the automatic re-plan settles the alpha.
- **GUI accept.**
  - `AppModel::accept` defaults to `High`, and `request()` sends it.
  - "(auto)" reads `provenance["subtype"] == suggestion` together with the top suggestion (High, its reason). No engine field is added.
  - The Subsurface card title reads "Coat colour + strength" for a `_coat` source.

**Tech Stack:** Rust 2021 workspace (`engine`, `cli` = `sdc`, `gui` = `sdc_gui`); serde/serde_json/schemars; rusqlite (GUI only, through `modscan::store::Store::conn()`); egui 0.36.2 + egui_kittest.

**Spec:** `docs/superpowers/specs/2026-10-12-sdc-m6-nif-alpha-confident-design.md`. It is owner-approved and binding.

Background:
- `docs/DECISIONS.md`:
  - Note H: the vocabulary;
  - Note S-shader-3: the shader layer;
  - Note S-shader-9: `_coat`;
  - Note S-shader-10: the mesh votes;
  - Note T2/T3: the compact BC1 rules;
  - Note P25: an explicit remove is not a loss.

## Global Constraints

- **Base.** `main` at 64d1646, which is the spec commits on top of 1b5f571. Line numbers below were read at 64d1646. If one has drifted, find the code by the quoted text, not by the number.
- **Branch.** The execution branch is `feat/m6-nif-alpha`, created by the controller from 64d1646. Implementers do not create, switch or delete branches.
- **Gates.** Run each one unpiped before every commit. All must pass:
  - `cargo fmt --all --check`
  - `cargo clippy --workspace --all-targets --offline -- -D warnings`
  - `cargo clippy --workspace --offline -- -D warnings`
  - `cargo test --workspace --offline`
- **Commits.**
  - Commit only explicit paths (`git add <path> …`, never `-A` or `.`).
  - End every message with `Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>`.
  - Never push.
- **Shell.** Never run `python -` or `python3 -` (stdin heredocs). This plan needs no Python.
- **Off-limits.**
  - Nothing is ever written under `F:/ai/dds/bench`.
  - The NIF team's crates are read-only: `crates/nif`, `crates/vfs`, `crates/modscan`, `crates/render3d`, `crates/pgpsim`. The GUI reads modscan's `shape.alpha_flags` / `shape.alpha_threshold` columns with raw SQL; `crates/modscan/src/store.rs` is not edited.
- **Engine dependencies.** The engine takes no `nif` or `modscan` dependency, and `crates/engine/Cargo.toml` is not edited. The NiAlphaProperty bit constants live in the GUI (`crates/gui/src/model/meshes.rs`).
- **CLI unchanged.** The CLI still needs `--accept-suggestions high`, and it sends an empty `mesh_alpha`. No CLI source file changes. Only two CLI tests change (Task 1), because their fixtures used a bare stem as "the Medium file".
- **Counts.**
  - `Code::ALL.len()` stays **75**: no new codes (spec §5).
  - The workspace version is **0.5.0** (`Cargo.toml:7`). Task 6 bumps it to **0.6.0**.
- **Schema.** Task 3 changes the `Provenance` doc comment, which is embedded in `schema/event.schema.json` and `schema/manifest.schema.json`, so it regenerates them with `cargo run -p sdc --offline -- presets schema all --out schema` and commits `schema/`. `crates/engine/tests/schemas.rs` fails until it does.
- **Golden and `gui_repro`.** Both stay green. The task's report names any changed expectation and its reason.
- **Decisions.** Task 6 appends `## Note U — M6 Track E: NIF-aware alpha and fewer confirmations (plan rulings, 2026-10-12)` at the end of `docs/DECISIONS.md`, after Note T's last bullet (T13). It has one bullet per ruling, `U1` … `U10`.

## Plan rulings

These settle the questions the spec left to the plan. Each one becomes a Note U bullet in Task 6.

- **U1 No suffix is the colour member, at High.**
  - `suggest.rs:66-67` drops `let colour = …`. The three arms that used it (`UiSky`, `PbrAlbedo`, `Diffuse`) take `High`.
  - `landscape_diffuse` and `skin_albedo` were already High. `hdr_sky` (`suggest.rs:127-129`) and `_i` (`:120`) stay Medium.
  - **Dead code removed.** The vocabulary colour raise in `apply_vocabulary` (`:170-179`) can no longer change anything, so it is deleted:
    - every colour member it raised is now High;
    - `hdr_sky` was already excluded;
    - a colour term canonicalises to no suffix, so the top suggestion is never the Medium `_i`.
- **U2 `_coat` = coat colour + strength.**
  - The `pbr_subsurface` alpha policy stays `data` (`sdc-base`). No engine logic changes; T3 already keeps `pbr_subsurface` with alpha out of BC1.
  - The role text becomes `coat colour + strength`, and the inserted reason becomes `exporter suffix _coat (coat colour + strength); the PBR coat map (_s)` (spec §3.1).
  - The GUI decides "came from `_coat`" by the source file name (`group::split_vocab` role `CoatColour`), not by the reason text, so an explicit sub-type on a `_coat` file keeps the title.
- **U3 GUI accepts High; how existing GUI tests keep working.**
  - `AppModel` gains `pub accept: AcceptSuggestions`, which `placeholder()` sets to `High`. `request()` sends `req.accept = self.accept`.
  - So every model the app builds (`main.rs` goes through `new`/`placeholder`) accepts High, as spec §2 says. No `main.rs` change is needed.
  - The M3-era GUI tests were written for "nothing accepted": their fixture `mod_folder` has High members (`rock01.png`, `leaf.png`, `odd.png` under `landscape/`) that the tests expect to stay undecided. Each existing test construction sets `accept = None` (Task 5 lists the 18 sites), so their meaning is unchanged.
  - The new Track E GUI tests use `support::opened_as_the_app`, which leaves the default `High`.
  - Explicit choices still win. `[files]` (provenance `project_file`), rules and `--set subtype` decide in `decide_subtype` before any suggestion (`policy.rs:308-334`).
- **U4 Where "(auto)" reads its data.**
  - There is no new engine field. `decide_subtype` writes `provenance["subtype"] = suggestion` whether or not the suggestion was accepted (`policy.rs:360-369`), and `decided` lives only on `SubtypeDecision`.
  - So `fields::auto_subtype_reason(job)` is `Some(top.reason)` exactly when all three hold:
    - `job.provenance["subtype"] == Provenance::Suggestion`;
    - `job.suggestions[0].confidence == High`;
    - the job does not need `Need::Subtype`.
  - The third check keeps "(auto)" off a High suggestion that was not accepted (the M3 fixtures, `accept = None`).
  - Display: the Sub-type row's provenance column (`settings_panel::field_row`) shows `(auto)` in place of "suggested", with the reason as hover text.
  - An acknowledged Medium suggestion (via `allow`) still reads "suggested".
- **U5 Mesh alpha evidence.**
  - **The field.** `BatchRequest::mesh_alpha: BTreeMap<String, Vec<MeshAlpha>>` is keyed by `shader::colour_mesh_key`. The engine looks a file up by `colour_mesh_key(&f.input.game_path, &f.input.output, Some(st))`. `f.input.output` is already canonicalised and output-path-adjusted, so `wall_albedo.png` is keyed `…/wall` (spec Review Focus 4).
  - **The GUI query.** `meshes::mesh_alpha` shares `mesh_votes`'s `FROM … WHERE` clause (a new `SLOT0_SOURCE_SHAPES` const):
    - lighting shapes (`shader_kind = 1`) using `<key>.dds` in slot 0;
    - TruePBR (SLSF2 bit 23) excluded;
    - non-derived copies of enabled origins;
    - `DISTINCT` per NIF path, block and alpha property.
  - **Unlike the votes,** every lighting shader type counts: a Face Tint shape that alpha-tests is alpha evidence even though it is no shader vote. The spec names only the four source rules.
  - **Bits.** NiAlphaProperty bit 0 is blend and bit 9 is test. A NULL `alpha_flags` (no property) is `{false, false}`, and a NULL threshold reads 0.
- **U6 The rule** (`shader::mesh_alpha_policy`), exactly spec §4.2:
  - no entries give `None`;
  - no shape with alpha gives `remove`;
  - blend only gives `blend`;
  - test only gives `cutout`;
  - any blend and any test (one shape or two) give `cutout_blend`.
  - The threshold is the largest among the testing shapes. **Spec gap:** a NIF threshold is 0..=255, but `check_profile` refuses `alpha.threshold` outside 5..=200 with `CFG-INVALID` (`policy.rs:1155`). The rule clamps each threshold to 5..=200.
- **U7 How a mesh-decided alpha is layered and made explicit.**
  - **Provenance.** `Provenance::MeshAlpha`, shown `mesh:alpha`, a unit variant. It is declared between `Shader` and `Suffix`; declaration order is precedence order. The order becomes `AnalysisFact < Shader < MeshAlpha < Suffix`, so the facts pinned by `shaders.rs:524-525` (`Shader > AnalysisFact`, `Shader < Suffix`) still hold.
  - **Where it is applied.** In `layered_profile_with`:
    - after the analysis-alpha step (`policy.rs:541-555`) and `skin_wet_form` (`:556-558`);
    - before `shader_layer` (`:559-561`);
    - only when `st.alpha_from_mesh()`, there is evidence for the colour key, and `!alpha_policy_explicit(ctx, &prov)`.
  - **What beats it.**
    - User presets, configs and rules come before it and are explicit.
    - `[files]` and `--set` come after it and overwrite it.
    - The analysis `remove` (a source with no alpha channel, or alpha constant 255) is explicit to `field_explicit` (`policy.rs:454-460`), so the mesh never overrides it. This is spec §4.3 "source without alpha", extended to constant 255, which settles spec Review Focus 2.
  - **What it beats.** It beats a shader's alpha default (hair `cutout`, multilayer `data`), because `shader_layer` writes only where `!field_explicit`.
  - **How it is "explicit".** `field_explicit` already returns `true` for every provenance except `builtin` and a built-in preset, so `MeshAlpha` is explicit with no change. The alpha question (`policy.rs:1722-1736`) is not asked, and `check_profile(…, alpha_explicit = true)` treats a mesh `remove` as explicit (P25).
  - **Threshold.** `alpha.threshold` is written with the same provenance when the rule gives one and the threshold is not already explicit.
- **U8 Compact (spec Review Focus 5).**
  - A mesh `remove` on a varying-alpha diffuse drops the alpha with no `LOSS-ALPHA-DATA` (explicit remove, P25). It stays **BC7** under `cs-compact`, because the BC1 rules match the source's alpha content (`varying`), not the policy (T2).
  - A mesh `cutout` never reaches a constant-255 source (U7), so an opaque RGBA diffuse stays BC1.
- **U9 Two-pass GUI.** The first plan of a folder asks the alpha question until the evidence is read after that plan. `EngineInputs` carries `mesh_alpha`, so the existing automatic re-plan (`mod.rs:1039-1040`) settles it. This is accepted as it is for the shader votes.
- **U10 Versions and manifest.**
  - There are no new codes (75).
  - The workspace goes 0.5.0 → 0.6.0.
  - **No manifest staleness change.** The up-to-date check (`api.rs:559-565`) compares `engine_version` (follows `CARGO_PKG_VERSION`) and `profile_sha256`, and a mesh-decided policy changes the profile hash too.
  - `schema/` is regenerated only because the `Provenance` doc text names `mesh:alpha`. The format does not change: `Provenance` is `schemars(with = "String")`, and `BatchRequest` is not serialised.

## Review Focus

1. **A NIF alpha-test threshold outside 5..=200** (0 is common in vanilla meshes) must give a usable cutout, never `CFG-INVALID` on the diffuse.
   - Task 2 pins it: `mesh_threshold_is_clamped_to_the_sdc_range`.
   - Task 3 pins it at plan level: `the_mesh_beats_the_shader_default_and_its_threshold_is_clamped`.
2. **An opaque RGBA export (alpha constant 255) used by an alpha-tested mesh** must stay `remove` (analysis) and BC1 under `cs-compact`, not become a BC7 cutout.
   - Task 3 pins it: `a_source_without_varying_alpha_keeps_the_analysis_remove`.
3. **A scan that finishes after the first plan, and adds only alpha evidence** (no shader vote changes) must still re-plan.
   - Task 4 pins it: `a_scan_database_decides_a_diffuse_alpha`. `refresh_mesh_votes()` returns `true` when only the alpha changed.
4. **A mesh whose shader type is not one of the 12** (Face Tint, type 4) but which alpha-tests must still count as alpha evidence.
   - Task 4 pins it in `mesh_alpha_reads_the_alpha_property_of_each_source_lighting_shape` and `a_scan_database_decides_a_diffuse_alpha`.
5. **An explicit hair shader on a set whose meshes blend** must follow the meshes (`blend`, `mesh:alpha`), not the shader's `cutout` default.
   - Task 3 pins it: `the_mesh_beats_the_shader_default_and_its_threshold_is_clamped`.

The spec's own Review Focus 1–5 are pinned as follows:
- RF1: Task 2, `mesh_alpha_policy_follows_the_owner_table`.
- RF2: Task 3 (above).
- RF3: Task 5, `an_explicit_subtype_beats_an_accepted_suggestion`.
- RF4: Task 3, `a_renamed_colour_member_is_keyed_by_its_output_stem`.
- RF5: Task 3, `a_mesh_remove_on_varying_alpha_stays_bc7_under_compact_without_a_loss_warning`.

---

## File map

| File | Responsibility | Tasks |
|---|---|---|
| `crates/engine/src/suggest.rs` | bare stem High, `_coat` text, dead colour raise removed, unit tests | 1 |
| `crates/engine/src/shader.rs` | `MeshAlpha`, `mesh_alpha_policy`, unit tests | 2 |
| `crates/engine/src/presets.rs` | `Provenance::MeshAlpha` (`mesh:alpha`) | 3 |
| `crates/engine/src/policy.rs` | `ResolveCtx::mesh_alpha`, `NO_MESH_ALPHA`, the layer in `layered_profile_with` | 3 |
| `crates/engine/src/api.rs` | `BatchRequest::mesh_alpha`, its default, the pass-1 `ResolveCtx` literal | 3 |
| `schema/event.schema.json`, `schema/manifest.schema.json` | regenerated (`Provenance` doc text) | 3 |
| `crates/gui/src/model/meshes.rs` | `SLOT0_SOURCE_SHAPES`, `mesh_alpha`, the alpha bits | 4 |
| `crates/gui/src/model/mod.rs` | `AppModel::mesh_alpha`, `EngineInputs::mesh_alpha`, `refresh_mesh_votes` fills both, `request()` sends it; `AppModel::accept` | 4, 5 |
| `crates/gui/src/model/fields.rs` | `auto_subtype_reason`, `mesh:alpha` prose | 5 |
| `crates/gui/src/ui/settings_panel.rs` | "(auto)" in the Sub-type row | 5 |
| `crates/gui/src/model/sets.rs`, `crates/gui/src/ui/set_editor.rs` | `slot_label` ("Coat colour + strength") | 5 |
| tests: `crates/engine/tests/{suffix_vocabulary,skin_subtypes,policy_checks,golden}.rs`, `crates/cli/tests/cli_convert.rs` | bare-stem expectation updates, new suggestion/coat tests | 1 |
| tests: `crates/engine/tests/mesh_alpha.rs` (new); the `ResolveCtx` literals in `crates/engine/tests/{policy_checks,policy_layers,suffix_aliases,suffix_vocabulary}.rs` | planning tests; `mesh_alpha: &NO_MESH_ALPHA` | 3 |
| tests: `crates/gui/tests/{support/mod.rs,sets_model.rs}` | `opened_as_the_app`, DB read and plan tests | 4 |
| tests: `crates/gui/tests/{support/mod.rs,model.rs,ui_smoke.rs,set_editor_ui.rs,fields.rs,edges.rs,history.rs,packing_fills.rs,pbr_members.rs,sets_model.rs}` | accept opt-out at 18 sites, Track E GUI tests | 5 |
| `docs/DECISIONS.md`, `docs/gui.md`, `Cargo.toml`, `Cargo.lock` | Note U, the GUI doc, 0.6.0 | 6 |

---

### Task 1: Engine — no suffix is the colour member at High; `_coat` is coat colour + strength

**Spec:** §3, §3.1, §6 "Suggestions". Rulings U1, U2.

**Files:**
- Modify: `crates/engine/src/suggest.rs`:
  - `:66-67`: the comment and `let colour`;
  - `:130`, `:138`, `:139`: the three arms;
  - `:144-180`: `apply_vocabulary`;
  - the tests `suffix_rules` (`:285-295`) and `diffuse_is_never_high_confidence` (`:532-550`), which is replaced.
- Modify (existing expectations that encode "a bare stem is Medium"):
  - `crates/engine/tests/skin_subtypes.rs:161`: `x_wet.png` is a bare stem outside Advanced skin.
  - `crates/engine/tests/suffix_vocabulary.rs:278-285`: "a bare stem stays medium".
  - `crates/engine/tests/policy_checks.rs:939`: `resolved_job_json_shape`, `"medium"`.
  - `crates/cli/tests/cli_convert.rs:415-493`: `accepted_suggestion_loses_to_rules_which_lose_to_set` uses `floor.png` as the Medium file.
  - `crates/cli/tests/cli_convert.rs:634-657`: `unresolved_files_are_skipped_with_exit_1` uses `wall.png`/`floor.png` as Medium files.
  - `crates/engine/tests/golden.rs:2693-2694` and `:3245`: comments only. They say a suffixless file trips `SUG-LOW-CONFIDENCE`; the assertions still pass.
- Test: `crates/engine/tests/suffix_vocabulary.rs` (a new test at the end), `crates/engine/tests/golden.rs` (a new test after `compact_cutout_diffuse_stays_bc7`, at the end of the file).

Every other engine and CLI test that meets a bare stem with `--accept-suggestions high` already pins its sub-type with `file:**/<name>.subtype=…` or a `_d` name (checked: `golden.rs`, `api.rs`, `preview.rs`, `uncompressed_interop.rs`, `shaders.rs`, `cli_*`). Tests without acceptance are unaffected, because nothing is decided without it. If another test fails *only* because a bare stem is now High, update its expectation (Medium → High, or unresolved → ready) and name it in the report. Do not change a fixture unless the test is about a Medium suggestion; in that case use an `_i` name, as below.

**Interfaces:**
- Consumes: nothing new.
- Produces: no new names. `suggest()` returns High for a bare stem's `Diffuse`, `PbrAlbedo` and `UiSky`. The `_coat` top suggestion's reason is exactly `exporter suffix _coat (coat colour + strength); the PBR coat map (_s)`.

- [ ] **Step 1: Write the failing tests**

`suggest.rs`, `mod tests`. Delete `diffuse_is_never_high_confidence` (`:532-550`) and put this in its place:

```rust
    /// M6 Track E (owner, 2026-10-12: "No suffix should equal diffuse by default"; DECISIONS Note U1): a bare stem is
    /// the set's colour member at High, like `_d`: diffuse, pbr_albedo in a PBR set (by `_rmaos`, `_cnr`, `_f`, a
    /// packing input or a `pbr` folder), ui_sky under interface/ or sky/. `hdr_sky` and `_i` stay Medium; Advanced
    /// skin (`_rfaos`) is its own thing.
    #[test]
    fn a_bare_stem_is_the_colour_member_at_high() {
        let none = BTreeSet::new();
        let top = |set: &BTreeSet<String>, path: &str, alpha: AlphaContent| {
            let s = suggest(&ctx(Target::Cs, None, path, set, alpha))[0].clone();
            (s.subtype, s.confidence)
        };
        for alpha in [
            AlphaContent::None,
            AlphaContent::Constant255,
            AlphaContent::Varying,
        ] {
            assert_eq!(
                top(&none, "textures/a/x.png", alpha),
                (Subtype::Diffuse, Confidence::High)
            );
            for marker in ["rmaos", "cnr", "f", "roughness"] {
                assert_eq!(
                    top(&set_of(&[marker]), "textures/a/x.png", alpha),
                    (Subtype::PbrAlbedo, Confidence::High),
                    "{marker}"
                );
            }
            assert_eq!(
                top(&none, "textures/pbr/x.png", alpha),
                (Subtype::PbrAlbedo, Confidence::High)
            );
            assert_eq!(
                top(&none, "textures/interface/x.png", alpha),
                (Subtype::UiSky, Confidence::High)
            );
            assert_eq!(
                top(&none, "textures/sky/x.png", alpha),
                (Subtype::UiSky, Confidence::High)
            );
        }
        let mut f = ctx(Target::Cs, None, "textures/sky/x.exr", &none, AlphaContent::None);
        f.is_float = true;
        assert_eq!(
            (suggest(&f)[0].subtype, suggest(&f)[0].confidence),
            (Subtype::HdrSky, Confidence::Medium)
        );
        let i = suggest(&ctx(
            Target::Cs,
            Some("i"),
            "textures/a/x_i.png",
            &none,
            AlphaContent::None,
        ))[0]
            .clone();
        assert_eq!((i.subtype, i.confidence), (Subtype::Diffuse, Confidence::Medium));
        let mut skin = ctx(
            Target::Cs,
            None,
            "textures/a/x.png",
            &none,
            AlphaContent::None,
        );
        skin.advanced_skin = true;
        let s = suggest(&skin)[0].clone();
        assert_eq!((s.subtype, s.confidence), (Subtype::SkinAlbedo, Confidence::High));
    }

    /// M6 Track E spec §3.1 (Note U2): `_coat` is the coat colour (RGB) and strength (A).
    #[test]
    fn coat_is_coat_colour_and_strength() {
        let none = BTreeSet::new();
        let mut sg = suggest(&ctx(
            Target::Cs,
            Some("s"),
            "textures/a/x_coat.png",
            &none,
            AlphaContent::Varying,
        ));
        apply_vocabulary(&mut sg, crate::group::vocab_of("x_coat").unwrap());
        assert_eq!(
            (sg[0].subtype, sg[0].confidence, sg[0].reason.as_str()),
            (
                Subtype::PbrSubsurface,
                Confidence::High,
                "exporter suffix _coat (coat colour + strength); the PBR coat map (_s)"
            )
        );
        assert!(sg[1..]
            .iter()
            .all(|s| s.reason.starts_with("exporter suffix _coat (coat colour + strength); ")));
    }
```

`suggest.rs` `suffix_rules`, `:292-295`. Change the expectation:

```rust
        assert_eq!(
            (d.subtype, d.confidence),
            (Subtype::Diffuse, Confidence::High)
        );
```

`crates/engine/tests/skin_subtypes.rs:161`:

```rust
    assert_eq!(top(&p, "x_wet.png"), (Subtype::Diffuse, Confidence::High));
```

`crates/engine/tests/suffix_vocabulary.rs:278-285`. Replace the "a bare stem stays medium" block with:

```rust
    // a bare stem is the colour member at High too (M6 Track E, DECISIONS Note U1)
    let j = resolve_img("iron.png", &[], &[], &[], &grey(), &RGB8);
    assert_eq!(
        (
            j.profile.subtype,
            j.suggestions[0].confidence,
            j.status.clone()
        ),
        (Subtype::Diffuse, Confidence::High, JobStatus::Ready)
    );
```

`crates/engine/tests/suffix_vocabulary.rs`, at the end:

```rust
/// M6 Track E (owner: "_rfaos _RFAOS _rmaos _RMaos should not need confirmation"; Note U1): with acceptance, every
/// member of a correctly named PBR or Advanced skin set decides itself, whatever the case of its suffix. The bare
/// stem is the colour member at High: `pbr_albedo` beside `_RMAOS`, `skin_albedo` beside `_RFAOS`.
#[test]
fn upper_case_suffixes_resolve_like_lower_case_and_nothing_asks_a_sub_type() {
    let d = tempfile::tempdir().unwrap();
    for (name, px) in [
        ("IRON.png", [120u8, 90, 60]),
        ("IRON_N.png", [128, 128, 255]),
        ("IRON_RMAOS.png", [128, 0, 255]),
        ("Body.png", [200, 160, 140]),
        ("Body_RFAOS.png", [128, 0, 255]),
    ] {
        let p = d.path().join("in/textures/a").join(name);
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        image::ImageBuffer::from_fn(16, 16, |_, _| image::Rgb(px))
            .save(p)
            .unwrap();
    }
    let p = plan_cs(d.path());
    for (input, want) in [
        ("IRON.png", Subtype::PbrAlbedo),
        ("IRON_N.png", Subtype::PbrNormal),
        ("IRON_RMAOS.png", Subtype::Rmaos),
        ("Body.png", Subtype::SkinAlbedo),
        ("Body_RFAOS.png", Subtype::SkinRfaos),
    ] {
        let j = p
            .jobs
            .iter()
            .find(|j| j.input.ends_with(input))
            .unwrap_or_else(|| panic!("no job for {input}"));
        assert_eq!(
            (j.profile.subtype, j.suggestions[0].confidence),
            (want, Confidence::High),
            "{input}"
        );
        assert!(
            !matches!(&j.status, JobStatus::Unresolved { needs, .. } if needs.contains(&Need::Subtype)),
            "{input}: {:?}",
            j.status
        );
    }
}
```

`crates/engine/tests/policy_checks.rs:939` (`resolved_job_json_shape`). The status stays `unresolved`, because the alpha varies:

```rust
    assert_eq!(v["suggestions"][0]["confidence"], "high");
```

`crates/engine/tests/golden.rs`, at the end of the file:

```rust
/// M6 Track E spec §3.1 (DECISIONS Note U2): `_coat` is the coat colour + strength. Its varying alpha is the strength:
/// policy `data`, kept, and BC7 under cs-compact (never BC1).
#[test]
fn compact_coat_keeps_its_strength_alpha_as_bc7() {
    let d = tempfile::tempdir().unwrap();
    put(d.path(), "textures/a/rock.png", rgb(64));
    put(d.path(), "textures/a/rock_n.png", flat_normal(64));
    put(d.path(), "textures/a/rock_rmaos.png", rgb(64));
    put(
        d.path(),
        "textures/a/rock_coat.png",
        rgba(64, |x, _| [200, 180, 160, (x * 4) as u8]),
    );
    let mut req = request(d.path(), Target::Cs, &[], &[]);
    req.preset = Some("cs-compact".into());
    let o = run(req);
    assert_profile(&o, "rock_coat.png", "pbr_subsurface", "data", false, "bc7");
    assert_eq!(
        planned(&o, "rock_coat.png")["status"]["state"],
        "ready",
        "{:?}",
        warnings(&o)
    );
    assert_eq!(o.summary.failed, 0, "{:?}", warnings(&o));
    let reason = planned(&o, "rock_coat.png")["suggestions"][0]["reason"].clone();
    assert!(
        reason
            .as_str()
            .unwrap()
            .starts_with("exporter suffix _coat (coat colour + strength)"),
        "{reason}"
    );
    assert_alpha_kept(&dds(d.path(), "textures/a/rock_s.dds"), |x, _| (x * 4) as u8, 8);
}
```

`crates/cli/tests/cli_convert.rs`, `accepted_suggestion_loses_to_rules_which_lose_to_set` (`:415-493`). Replace every `floor.png` in this function with `floor_i.png`. There are 10 occurrences, at `:419`, `:422`, `:424`, `:430`, `:446`, `:447`, `:450`, `:486`, `:491`, `:492`. `_i` (a multilayer inner layer) is the remaining Medium `diffuse` suggestion. The comment at `:422` then reads:

```rust
    // An accepted high-confidence suggestion decides `wall_n.png`; the medium one for `floor_i.png` stays open.
```

`crates/cli/tests/cli_convert.rs`, `unresolved_files_are_skipped_with_exit_1` (`:634-657`). Replace the function with:

```rust
#[test]
fn unresolved_files_are_skipped_with_exit_1() {
    let fx = Fx::new();
    fx.rgb("wall_i.png");
    fx.rgb("floor_i.png");
    // `_i` (a multilayer inner layer) is only a medium-confidence `diffuse` suggestion, so even
    // `--accept-suggestions high` leaves it open (a bare stem is High since M6 Track E).
    let o = fx.convert(&["--accept-suggestions", "high"]);
    assert_eq!(code(&o), 1);
    assert!(
        text(&o.stderr).contains("SUG-LOW-CONFIDENCE"),
        "{}",
        text(&o.stderr)
    );
    let s = skips(&o);
    assert_eq!(s.len(), 2);
    assert!(s
        .iter()
        .all(|(_, c)| c.as_deref() == Some("SUG-LOW-CONFIDENCE")));
    assert!(!fx.p("out/textures").exists());
    // `--allow SUG-LOW-CONFIDENCE` is the explicit acknowledgement.
    let o = fx.convert(&["--allow", "SUG-LOW-CONFIDENCE"]);
    assert_eq!(code(&o), 0, "{}", text(&o.stderr));
    assert!(fx.dds("textures/a/wall_i.dds").exists());
}
```

- [ ] **Step 2: Run them and check that they fail**

Run: `cargo test -p engine --offline --lib suggest::`, then `cargo test -p engine --offline --test suffix_vocabulary --test skin_subtypes --test policy_checks`, then `cargo test -p engine --offline --test golden compact_coat`.
Expected:
- `a_bare_stem_is_the_colour_member_at_high` and `suffix_rules` FAIL: Medium.
- `coat_is_coat_colour_and_strength` FAILS: the reason is `exporter suffix _coat (coat colour); …`.
- `upper_case_suffixes_resolve_like_lower_case_and_nothing_asks_a_sub_type` FAILS on `IRON.png` (Medium, needs a sub-type).
- `outside_advanced_skin_sk_is_the_vanilla_guess_and_wet_is_a_base_name`, `colour_terms_and_d_are_high_confidence_but_alpha_meaning_is_never_assumed` and `resolved_job_json_shape` FAIL: Medium.
- `compact_coat_keeps_its_strength_alpha_as_bc7` FAILS on the reason prefix.
- The two CLI tests already pass: `_i` is Medium today. They pin the CLI's behaviour through the change.

- [ ] **Step 3: Implement** (`suggest.rs`)

Replace `:66-67`:

```rust
    // M6 Track E (owner, 2026-10-12; DECISIONS Note U1, reversing Note H's "a bare stem is only a guess"): no suffix
    // is the colour member, at High like `_d`. `hdr_sky` and `_i` stay Medium.
```

The three colour arms, `:130`, `:138`, `:139`:

```rust
        Some("d") | None if ui_or_sky => vec![s(UiSky, High, "colour under interface/ or sky/")],
```

```rust
        Some("d") | None if pbr => vec![s(PbrAlbedo, High, "colour member of a PBR set")],
        Some("d") | None => vec![s(Diffuse, High, "colour member, non-PBR set")],
```

Replace `apply_vocabulary` (`:144-180`) with:

```rust
/// Spec §4.3 item 7, §4.4 (DECISIONS Notes H, U1, U2): an exporter vocabulary term declares the file's role. Every
/// reason is prefixed `exporter suffix _<term> (<role>)`, and `_coat` puts the coat map first at High. A colour term
/// needs no raise: its colour member is already High (Note U1), and `hdr_sky` stays Medium. Alpha meaning is
/// untouched.
pub fn apply_vocabulary(suggestions: &mut Vec<Suggestion>, v: &Vocab) {
    let role = match v.role {
        VocabRole::Colour => "colour",
        VocabRole::Normal(_) => "normal",
        VocabRole::Emissive => "emissive",
        VocabRole::PackingInput(_) => "packing input",
        VocabRole::CoatColour => "coat colour + strength",
    };
    for s in suggestions.iter_mut() {
        s.reason = format!("exporter suffix _{} ({role}); {}", v.term, s.reason);
    }
    if v.role == VocabRole::CoatColour {
        // The owner states it is the coat map (colour in RGB, strength in A): High, whatever the set (a bare `_s`
        // outside a PBR set is only a tint).
        suggestions.retain(|s| s.subtype != Subtype::PbrSubsurface);
        suggestions.insert(
            0,
            Suggestion {
                subtype: Subtype::PbrSubsurface,
                confidence: Confidence::High,
                reason: "exporter suffix _coat (coat colour + strength); the PBR coat map (_s)".into(),
            },
        );
    }
}
```

Fix the two stale `golden.rs` comments:
- `:2693-2694`: `// A file-scoped sub-type pins the diffuse whatever the suggestion; \`subtype:diffuse\` would only scope fields.`
- `:3245`: `// File-level: no SHADER-* code may appear.`

- [ ] **Step 4: Run the tests and check that they pass**

Run: `cargo test -p engine --offline --lib suggest::`, `cargo test -p engine --offline`, `cargo test -p sdc --offline --test cli_convert`, then `cargo test --workspace --offline`.
Expected: PASS. `colour_vocabulary_raises_the_colour_member_only` still passes: the colour member is High without the raise, and `hdr_sky` stays Medium. Name any further expectation you changed under the rule above in the report.

- [ ] **Step 5: Run the gates, then commit**

```bash
git add crates/engine/src/suggest.rs crates/engine/tests/suffix_vocabulary.rs crates/engine/tests/skin_subtypes.rs crates/engine/tests/policy_checks.rs crates/engine/tests/golden.rs crates/cli/tests/cli_convert.rs
git commit -m "feat(suggest): no suffix is the colour member at High; _coat is coat colour + strength (M6 Track E U1, U2)

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

---

### Task 2: Engine — `MeshAlpha` and the owner's mesh alpha rule

**Spec:** §4.1 (the struct), §4.2 (the table), §6 "`mesh_alpha_policy`", Review Focus 1. Ruling U6.

**Files:**
- Modify: `crates/engine/src/shader.rs`:
  - the import at `:11`;
  - new items after `colour_mesh_key` (`:317-333`);
  - tests at the end of `mod tests`.

**Interfaces:**
- Consumes: `crate::config::AlphaPolicy`.
- Produces (Tasks 3–5 rely on these exact names):
  - `#[derive(Debug, Clone, Copy, PartialEq, Eq)] pub struct MeshAlpha { pub blend: bool, pub test: bool, pub threshold: u8 }`
  - `pub fn mesh_alpha_policy(shapes: &[MeshAlpha]) -> Option<(AlphaPolicy, Option<u8>)>`

- [ ] **Step 1: Write the failing tests** (at the end of `mod tests` in `shader.rs`)

```rust
    const BLEND: MeshAlpha = MeshAlpha {
        blend: true,
        test: false,
        threshold: 0,
    };
    const NO_ALPHA: MeshAlpha = MeshAlpha {
        blend: false,
        test: false,
        threshold: 0,
    };
    const fn tested(threshold: u8) -> MeshAlpha {
        MeshAlpha {
            blend: false,
            test: true,
            threshold,
        }
    }

    /// M6 Track E spec §4.2: every row of the owner's table. Shapes without alpha never outvote one with alpha, and
    /// blend on one mesh with test on another is cutout_blend, not a majority (Review Focus 1).
    #[test]
    fn mesh_alpha_policy_follows_the_owner_table() {
        use AlphaPolicy::*;
        assert_eq!(mesh_alpha_policy(&[]), None);
        assert_eq!(mesh_alpha_policy(&[NO_ALPHA, NO_ALPHA]), Some((Remove, None)));
        assert_eq!(mesh_alpha_policy(&[BLEND, BLEND]), Some((Blend, None)));
        assert_eq!(
            mesh_alpha_policy(&[NO_ALPHA, BLEND, NO_ALPHA]),
            Some((Blend, None))
        );
        assert_eq!(
            mesh_alpha_policy(&[tested(90), tested(128), NO_ALPHA]),
            Some((Cutout, Some(128)))
        );
        assert_eq!(
            mesh_alpha_policy(&[tested(90), BLEND, BLEND, BLEND]),
            Some((CutoutBlend, Some(90)))
        );
        let both = MeshAlpha {
            blend: true,
            test: true,
            threshold: 100,
        };
        assert_eq!(
            mesh_alpha_policy(&[both, NO_ALPHA]),
            Some((CutoutBlend, Some(100)))
        );
    }

    /// Note U6: a NIF threshold outside sdc's 5..=200 is clamped into it (a NIF may say 0 or 255; `check_profile`
    /// refuses anything outside 5..=200).
    #[test]
    fn mesh_threshold_is_clamped_to_the_sdc_range() {
        assert_eq!(
            mesh_alpha_policy(&[tested(0)]),
            Some((AlphaPolicy::Cutout, Some(5)))
        );
        assert_eq!(
            mesh_alpha_policy(&[tested(255), tested(30)]),
            Some((AlphaPolicy::Cutout, Some(200)))
        );
    }
```

- [ ] **Step 2: Run them and check that they fail**

Run: `cargo test -p engine --offline --lib shader::`
Expected: FAIL to compile: `MeshAlpha` and `mesh_alpha_policy` are not found.

- [ ] **Step 3: Implement**

`shader.rs:11`:

```rust
use crate::config::{AlphaPolicy, Subtype, Target};
```

After `colour_mesh_key`:

```rust
/// The alpha property of one scanned lighting shape that uses a colour member in slot 0 (M6 Track E spec §4.1,
/// DECISIONS Note U5): the NiAlphaProperty blend and test enables and the test threshold. A shape without the
/// property is all false.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MeshAlpha {
    pub blend: bool,
    pub test: bool,
    pub threshold: u8,
}

/// The owner's table (spec §4.2, DECISIONS Note U6): the alpha policy the shapes give their colour member, and the
/// cutout threshold when a shape tests; `None` = no evidence. Any shape with alpha beats the shapes without (there is
/// no vote), and blend and test anywhere, on one shape or two, is `cutout_blend`. The threshold is the largest of the
/// testing shapes (the stricter cutoff), clamped to sdc's 5..=200.
pub fn mesh_alpha_policy(shapes: &[MeshAlpha]) -> Option<(AlphaPolicy, Option<u8>)> {
    if shapes.is_empty() {
        return None;
    }
    let blend = shapes.iter().any(|s| s.blend);
    let threshold = shapes
        .iter()
        .filter(|s| s.test)
        .map(|s| s.threshold.clamp(5, 200))
        .max();
    Some(match (blend, threshold) {
        (false, None) => (AlphaPolicy::Remove, None),
        (true, None) => (AlphaPolicy::Blend, None),
        (false, t) => (AlphaPolicy::Cutout, t),
        (true, t) => (AlphaPolicy::CutoutBlend, t),
    })
}
```

- [ ] **Step 4: Run the tests and check that they pass**

Run: `cargo test -p engine --offline --lib shader::`
Expected: PASS.

- [ ] **Step 5: Run the gates, then commit**

```bash
git add crates/engine/src/shader.rs
git commit -m "feat(shader): MeshAlpha and the owner's mesh alpha rule (M6 Track E U6)

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

---

### Task 3: Engine — the `mesh:alpha` layer

**Spec:** §4.1 (the request field), §4.3, §5 (no codes), §6 "Planning", Review Focus 2, 4 and 5. Rulings U5, U7, U8, U10.

**Files:**
- Modify: `crates/engine/src/presets.rs`:
  - the `Provenance` doc (`:232-236`);
  - the enum (`:239-275`): a new variant after `Shader`;
  - `Display` (`:277-299`);
  - `FromStr` (`:332-343`).
- Modify: `crates/engine/src/policy.rs`:
  - `ResolveCtx` (`:32-48`), the new field;
  - after `NO_SHADERS` (`:50-51`), the new static;
  - `layered_profile_with`, between `skin_wet_form` (`:556-558`) and the shader layer (`:559-561`).
- Modify: `crates/engine/src/api.rs`:
  - the import at `:26`;
  - `BatchRequest` after `mesh_shaders` (`:95-97`);
  - `BatchRequest::new` (`:131`);
  - the pass-1 `ResolveCtx` literal (`:782-793`).
- Modify: every test `ResolveCtx { … }` literal. After its `shaders: &NO_SHADERS,` line, add `mesh_alpha: &NO_MESH_ALPHA,`. They import `engine::policy::*`, so there are no import changes. The sites:
  - `crates/engine/tests/policy_checks.rs`: `:42`, `:310`, `:816`, `:1071`, `:1146`, `:1222`, `:1258`;
  - `crates/engine/tests/policy_layers.rs`: `:54`, `:117`, `:133`, `:160`, `:233`, `:289`;
  - `crates/engine/tests/suffix_aliases.rs`: `:105`, `:489`;
  - `crates/engine/tests/suffix_vocabulary.rs`: `:62`.
  - Literals written `ResolveCtx { …, ..c }` take the field from `c`, so leave them alone.
- Create: `crates/engine/tests/mesh_alpha.rs`.
- Regenerate: `schema/` (the `Provenance` doc text).

**Interfaces:**
- Consumes: `shader::{MeshAlpha, mesh_alpha_policy, colour_mesh_key}` (Task 2 and existing).
- Produces:
  - `BatchRequest::mesh_alpha: BTreeMap<String, Vec<MeshAlpha>>`, empty in `BatchRequest::new`.
  - `ResolveCtx::mesh_alpha: &'a BTreeMap<String, Vec<crate::shader::MeshAlpha>>`, and `pub static NO_MESH_ALPHA` in `engine::policy`.
  - `Provenance::MeshAlpha`, which displays and parses as `mesh:alpha`.

- [ ] **Step 1: Write the failing tests** (new file `crates/engine/tests/mesh_alpha.rs`)

```rust
//! M6 Track E (DECISIONS Note U): a colour member's alpha policy from the meshes that use it (`BatchRequest::mesh_alpha`,
//! provenance `mesh:alpha`).
use engine::api::*;
use engine::codes::Code;
use engine::config::{AlphaPolicy, Format, Target};
use engine::events::*;
use engine::overrides::parse_set;
use engine::policy::{AcceptSuggestions, JobStatus, Need, Provenance, ResolvedJob};
use engine::shader::MeshAlpha;
use image::{ImageBuffer, Rgb, Rgba};
use std::path::Path;
use std::sync::Arc;

const WALL: &str = "textures/a/wall";
const BLEND: MeshAlpha = MeshAlpha {
    blend: true,
    test: false,
    threshold: 0,
};
const NO_ALPHA: MeshAlpha = MeshAlpha {
    blend: false,
    test: false,
    threshold: 0,
};
const fn tested(threshold: u8) -> MeshAlpha {
    MeshAlpha {
        blend: false,
        test: true,
        threshold,
    }
}

fn put_rgba(dir: &Path, rel: &str, f: impl Fn(u32, u32) -> [u8; 4]) {
    let p = dir.join("in").join(rel);
    std::fs::create_dir_all(p.parent().unwrap()).unwrap();
    ImageBuffer::from_fn(16, 16, |x, y| Rgba(f(x, y)))
        .save(p)
        .unwrap();
}

fn put_rgb(dir: &Path, rel: &str) {
    let p = dir.join("in").join(rel);
    std::fs::create_dir_all(p.parent().unwrap()).unwrap();
    ImageBuffer::from_fn(16, 16, |x, y| Rgb([(x * 16) as u8, (y * 16) as u8, 90]))
        .save(p)
        .unwrap();
}

/// A colour map whose alpha varies (a ramp).
fn varying(dir: &Path, rel: &str) {
    put_rgba(dir, rel, |x, _| [90, 60, 30, (x * 16) as u8]);
}

fn plan(dir: &Path, preset: Option<&str>, sets: &[&str], mesh: &[(&str, Vec<MeshAlpha>)]) -> Plan {
    let mut req = BatchRequest::new(vec![dir.join("in")], dir.join("out"), Target::Cs);
    req.recursive = true;
    req.accept = AcceptSuggestions::High;
    req.preset = preset.map(str::to_string);
    req.overrides = sets.iter().map(|s| parse_set(s).unwrap()).collect();
    req.mesh_alpha = mesh
        .iter()
        .map(|(k, v)| (k.to_string(), v.clone()))
        .collect();
    let rep = Reporter::new(Arc::new(NullSink), "mesh-alpha");
    Engine::new()
        .unwrap()
        .plan(req, &rep, &CancelToken::default())
        .unwrap()
}

fn job<'a>(p: &'a Plan, tail: &str) -> &'a ResolvedJob {
    p.jobs
        .iter()
        .find(|j| {
            j.output
                .to_string_lossy()
                .replace('\\', "/")
                .ends_with(tail)
        })
        .unwrap_or_else(|| panic!("no job {tail}"))
}

fn prov(j: &ResolvedJob, f: &str) -> String {
    j.provenance[f].to_string()
}

fn codes(j: &ResolvedJob) -> Vec<Code> {
    j.warnings.iter().map(|w| w.code).collect()
}

fn asks_alpha(j: &ResolvedJob) -> bool {
    matches!(&j.status, JobStatus::Unresolved { needs, .. } if needs.contains(&Need::AlphaMeaning))
}

#[test]
fn mesh_alpha_provenance_round_trips_and_sits_above_the_shader() {
    assert_eq!(Provenance::MeshAlpha.to_string(), "mesh:alpha");
    assert_eq!(
        "mesh:alpha".parse::<Provenance>().unwrap(),
        Provenance::MeshAlpha
    );
    assert!(Provenance::Shader(String::new()) < Provenance::MeshAlpha);
    assert!(Provenance::MeshAlpha < Provenance::Suffix(String::new()));
}

/// Spec §4.3, §6: evidence decides a varying-alpha diffuse (provenance `mesh:alpha`, the threshold too) and the alpha
/// question is not asked; without evidence, or with evidence for another texture, it is asked as before.
#[test]
fn mesh_evidence_decides_the_alpha_and_no_evidence_asks_as_before() {
    let d = tempfile::tempdir().unwrap();
    varying(d.path(), "textures/a/wall.png");
    let p = plan(
        d.path(),
        None,
        &[],
        &[(WALL, vec![tested(100), tested(140), NO_ALPHA])],
    );
    let j = job(&p, "wall.dds");
    assert_eq!(
        (j.profile.alpha.policy, j.profile.alpha.threshold),
        (AlphaPolicy::Cutout, 140)
    );
    assert_eq!(prov(j, "alpha.policy"), "mesh:alpha");
    assert_eq!(prov(j, "alpha.threshold"), "mesh:alpha");
    assert!(!asks_alpha(j), "{:?}", j.status);
    assert!(!codes(j).contains(&Code::SugLowConfidence), "{:?}", j.warnings);
    assert!(asks_alpha(job(&plan(d.path(), None, &[], &[]), "wall.dds")));
    let other = plan(d.path(), None, &[], &[("textures/a/other", vec![BLEND])]);
    assert!(asks_alpha(job(&other, "wall.dds")));
}

/// Spec §4.3, §6: an explicit layer beats the mesh: `--set`/`[files]` (applied after it) and a project config
/// (applied before it).
#[test]
fn an_explicit_alpha_policy_beats_the_mesh() {
    let d = tempfile::tempdir().unwrap();
    varying(d.path(), "textures/a/wall.png");
    let mesh = [(WALL, vec![BLEND])];
    let p = plan(
        d.path(),
        None,
        &["file:**/wall.png.alpha.policy=cutout"],
        &mesh,
    );
    let j = job(&p, "wall.dds");
    assert_eq!(
        (j.profile.alpha.policy, prov(j, "alpha.policy")),
        (AlphaPolicy::Cutout, "user".to_string())
    );
    std::fs::write(
        d.path().join("in/sdc.toml"),
        "[subtypes.diffuse]\nalpha = { policy = \"data\" }\n",
    )
    .unwrap();
    let p = plan(d.path(), None, &[], &mesh);
    let j = job(&p, "wall.dds");
    assert_eq!(
        (j.profile.alpha.policy, prov(j, "alpha.policy")),
        (AlphaPolicy::Data, "project_config".to_string())
    );
}

/// Spec §4.3 "source without alpha", Review Focus 2 (Note U7): a source with no alpha channel, or alpha constant
/// 255, keeps the analysis `remove` whatever the meshes say, so an opaque RGBA colour map stays BC1 under cs-compact.
#[test]
fn a_source_without_varying_alpha_keeps_the_analysis_remove() {
    let d = tempfile::tempdir().unwrap();
    put_rgb(d.path(), "textures/a/wall.png");
    put_rgba(d.path(), "textures/a/glass.png", |x, y| {
        [(x * 16) as u8, (y * 16) as u8, 90, 255]
    });
    let mesh = [
        (WALL, vec![tested(128)]),
        ("textures/a/glass", vec![tested(128)]),
    ];
    let p = plan(d.path(), Some("cs-compact"), &[], &mesh);
    for tail in ["wall.dds", "glass.dds"] {
        let j = job(&p, tail);
        assert_eq!(
            (j.profile.alpha.policy, prov(j, "alpha.policy")),
            (AlphaPolicy::Remove, "analysis".to_string()),
            "{tail}"
        );
        assert_eq!(j.profile.format, Format::Bc1, "{tail}");
    }
}

/// Spec Review Focus 5 (Note U8): a mesh `remove` on a varying-alpha diffuse drops the alpha with no
/// LOSS-ALPHA-DATA (the meshes never read it), and stays BC7 under cs-compact: the BC1 rules match the source's alpha
/// content.
#[test]
fn a_mesh_remove_on_varying_alpha_stays_bc7_under_compact_without_a_loss_warning() {
    let d = tempfile::tempdir().unwrap();
    varying(d.path(), "textures/a/wall.png");
    let p = plan(
        d.path(),
        Some("cs-compact"),
        &[],
        &[(WALL, vec![NO_ALPHA, NO_ALPHA])],
    );
    let j = job(&p, "wall.dds");
    assert_eq!(
        (j.profile.alpha.policy, prov(j, "alpha.policy")),
        (AlphaPolicy::Remove, "mesh:alpha".to_string())
    );
    assert_eq!(j.profile.format, Format::Bc7);
    assert!(!codes(j).contains(&Code::LossAlphaData), "{:?}", j.warnings);
    assert_eq!(j.status, JobStatus::Ready, "{:?}", j.warnings);
}

/// Spec Review Focus 4: a colour member renamed by the vocabulary (`wall_albedo.png` → `wall.dds`) is keyed by its
/// output stem, the name the meshes reference.
#[test]
fn a_renamed_colour_member_is_keyed_by_its_output_stem() {
    let d = tempfile::tempdir().unwrap();
    varying(d.path(), "textures/a/wall_albedo.png");
    let p = plan(d.path(), None, &[], &[(WALL, vec![BLEND])]);
    let j = job(&p, "wall.dds");
    assert_eq!(
        (j.profile.alpha.policy, prov(j, "alpha.policy")),
        (AlphaPolicy::Blend, "mesh:alpha".to_string())
    );
}

/// Note U7, Review Focus 1 and 5 of the plan: the mesh beats a shader's alpha default (hair: cutout), and a mesh
/// threshold outside 5..=200 is clamped, never CFG-INVALID.
#[test]
fn the_mesh_beats_the_shader_default_and_its_threshold_is_clamped() {
    let d = tempfile::tempdir().unwrap();
    varying(d.path(), "textures/a/wall.png");
    let p = plan(
        d.path(),
        None,
        &["set:textures/a/wall.shader=hair"],
        &[(WALL, vec![BLEND])],
    );
    let j = job(&p, "wall.dds");
    assert_eq!(
        (j.profile.alpha.policy, prov(j, "alpha.policy")),
        (AlphaPolicy::Blend, "mesh:alpha".to_string())
    );
    let p = plan(d.path(), None, &[], &[(WALL, vec![tested(0)])]);
    let j = job(&p, "wall.dds");
    assert_eq!(
        (j.profile.alpha.policy, j.profile.alpha.threshold),
        (AlphaPolicy::Cutout, 5)
    );
    assert!(!codes(j).contains(&Code::CfgInvalid), "{:?}", j.warnings);
}
```

- [ ] **Step 2: Run them and check that they fail**

Run: `cargo test -p engine --offline --test mesh_alpha`
Expected: FAIL to compile: `Provenance::MeshAlpha` and `BatchRequest::mesh_alpha` do not exist.

- [ ] **Step 3: Implement**

`presets.rs`, the `Provenance` doc (`:234-236`). Name `mesh:alpha` in the list:

```rust
/// Serialises as a string: `builtin`, `preset:<id>`, `rule:<source>#<n>` (the `Rule` payload is the
/// `<source>#<n>` part), `analysis:<fact>`, `shader:<id>`, `mesh:alpha`, `suffix:<term>`, `source:<file>`,
/// `default:<value>`, `fill:<code>`, `recipe:<kind>`, and the snake_case variant name for the rest.
```

The new variant, directly after `Shader(String),`:

```rust
    /// The alpha policy (and threshold) the meshes that use the colour member give it (M6 Track E, DECISIONS Note
    /// U7): above shader defaults, below every explicit layer and the analysis `remove`. Shown as `mesh:alpha`.
    MeshAlpha,
```

`Display`, after the `Shader` arm:

```rust
            Provenance::MeshAlpha => f.write_str("mesh:alpha"),
```

`FromStr`, in the final `match s`, after `"analysis" => Provenance::Analysis,`:

```rust
            "mesh:alpha" => Provenance::MeshAlpha,
```

`policy.rs`, `ResolveCtx`, after `shaders`:

```rust
    /// M6 Track E (DECISIONS Note U5): `shader::colour_mesh_key` → the alpha property of each scanned shape that uses
    /// the colour member (`BatchRequest::mesh_alpha`). Empty (`NO_MESH_ALPHA`) = no evidence.
    pub mesh_alpha: &'a BTreeMap<String, Vec<crate::shader::MeshAlpha>>,
```

After `NO_SHADERS`:

```rust
/// For `ResolveCtx` literals with no mesh evidence (tests).
pub static NO_MESH_ALPHA: BTreeMap<String, Vec<crate::shader::MeshAlpha>> = BTreeMap::new();
```

`layered_profile_with`, between the `skin_wet_form` block and `if let Some(p) = ctx.shaders.get(&f.set.key)`:

```rust
    // M6 Track E (spec §4.3, DECISIONS Note U7): what the meshes that use this colour member say about its alpha.
    // Only where no explicit layer and no analysis fact (a source without varying alpha) chose the policy; it is
    // explicit itself, so the shader default below never overwrites it and the alpha question is not asked.
    if st.alpha_from_mesh() && !alpha_policy_explicit(ctx, &prov) {
        if let Some((policy, threshold)) =
            crate::shader::colour_mesh_key(&f.input.game_path, &f.input.output, Some(st))
                .and_then(|k| ctx.mesh_alpha.get(&k))
                .and_then(|shapes| crate::shader::mesh_alpha_policy(shapes))
        {
            let value = serde_json::to_value(policy).expect("AlphaPolicy serialises");
            apply(
                &mut doc,
                &mut prov,
                &mut sources,
                &to_patch("alpha.policy", &value),
                Provenance::MeshAlpha,
                GENERIC,
            );
            if let Some(t) = threshold.filter(|_| !field_explicit(ctx, &prov, "alpha.threshold")) {
                apply(
                    &mut doc,
                    &mut prov,
                    &mut sources,
                    &to_patch("alpha.threshold", &json!(t)),
                    Provenance::MeshAlpha,
                    GENERIC,
                );
            }
        }
    }
```

`api.rs:26`:

```rust
use crate::shader::{MeshAlpha, Shader, ShaderPick};
```

`BatchRequest`, after `mesh_shaders`:

```rust
    /// GUI (M6 Track E, DECISIONS Note U5): `shader::colour_mesh_key` → the NiAlphaProperty of each scanned lighting
    /// shape that uses `<key>.dds` in slot 0. The GUI fills it from the scan database; the CLI sends none (empty: no
    /// evidence, and the alpha question is asked as before).
    pub mesh_alpha: BTreeMap<String, Vec<MeshAlpha>>,
```

`BatchRequest::new`, after `mesh_shaders: BTreeMap::new(),`:

```rust
            mesh_alpha: BTreeMap::new(),
```

The pass-1 `ResolveCtx` literal (`:782-793`), after `shaders: &NO_SHADERS,`:

```rust
            mesh_alpha: &req.mesh_alpha,
```

The 6c re-resolve `ResolveCtx { shaders: &picks, ..ctx }` (`:1064-1067`) inherits it. Then add `mesh_alpha: &NO_MESH_ALPHA,` to the 16 test literals listed under **Files**.

- [ ] **Step 4: Regenerate the schemas**

Run: `cargo run -p sdc --offline -- presets schema all --out schema`
Expected: `git status --short schema` shows `schema/event.schema.json` and `schema/manifest.schema.json` modified, with only the `Provenance` description changed (`git diff schema`).

- [ ] **Step 5: Run the tests and check that they pass**

Run: `cargo test -p engine --offline --test mesh_alpha`, then `cargo test --workspace --offline`.
Expected: PASS, including `schemas.rs` and `shaders.rs:524-525` (the Provenance order).

- [ ] **Step 6: Run the gates, then commit**

```bash
git add crates/engine/src/presets.rs crates/engine/src/policy.rs crates/engine/src/api.rs crates/engine/tests/mesh_alpha.rs crates/engine/tests/policy_checks.rs crates/engine/tests/policy_layers.rs crates/engine/tests/suffix_aliases.rs crates/engine/tests/suffix_vocabulary.rs schema/event.schema.json schema/manifest.schema.json
git commit -m "feat(policy): mesh:alpha layer from BatchRequest::mesh_alpha; no alpha question with mesh evidence (M6 Track E U5, U7, U8)

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

---

### Task 4: GUI — read the mesh alpha from the scan database and send it

**Spec:** §4.1 (GUI), §6 "GUI DB read". Rulings U5, U9. Review Focus 3 and 4.

**Files:**
- Modify: `crates/gui/src/model/meshes.rs`:
  - the import at `:9`;
  - `mesh_votes` (`:235-275`): the shared clause;
  - new items after it.
- Modify: `crates/gui/src/model/mod.rs`:
  - `EngineInputs` (`:117-141`);
  - `AppModel` after `mesh_votes` (`:161-162`);
  - `placeholder()` (`:242`);
  - `request()` (`:402`);
  - `refresh_mesh_votes` (`:406-427`);
  - the `EngineInputs` literal in `replan()` (`:443-450`).
- Modify: `crates/gui/tests/support/mod.rs`: a new helper `opened_as_the_app`.
- Test: `crates/gui/tests/sets_model.rs`: a helper `scan_db_alpha` and two tests after `a_scan_database_picks_the_set_shader` (`:1137-1182`).

**Interfaces:**
- Consumes: `engine::shader::MeshAlpha`, `BatchRequest::mesh_alpha` (Tasks 2, 3).
- Produces:
  - `pub fn meshes::mesh_alpha(db: &Path, keys: &BTreeSet<String>) -> Result<BTreeMap<String, Vec<MeshAlpha>>, String>`.
  - `AppModel::mesh_alpha: BTreeMap<String, Vec<engine::shader::MeshAlpha>>`.
  - `refresh_mesh_votes() -> bool` is true when the votes **or** the alpha changed.
  - `support::opened_as_the_app(root: PathBuf, export: PathBuf) -> AppModel`: a model built as `main` builds it and opened on `root`. Task 5 relies on it.

- [ ] **Step 1: Write the failing tests**

`crates/gui/tests/support/mod.rs`, at the end:

```rust
/// A model built as the app builds it (no test overrides), opened on `root` with exports under `export`.
pub fn opened_as_the_app(root: PathBuf, export: PathBuf) -> AppModel {
    let settings = GuiSettings {
        export_folder: Some(export),
        ..GuiSettings::default()
    };
    let mut m = AppModel::new(settings, None, session());
    m.open_folder(root);
    assert!(
        m.wait_idle(Duration::from_secs(60)),
        "planning did not finish"
    );
    m
}
```

`crates/gui/tests/sets_model.rs`, after `a_scan_database_picks_the_set_shader`:

```rust
/// A scan database where `tex` is slot 0 of these lighting shapes:
/// (shader type, `alpha_flags`, `alpha_threshold`, SLSF2, derived copy). Every origin is enabled.
fn scan_db_alpha(
    db: &std::path::Path,
    tex: &str,
    shapes: &[(u32, Option<i64>, Option<i64>, u32, bool)],
) {
    let s = modscan::store::Store::open(db).unwrap();
    let c = s.conn();
    c.execute("INSERT INTO path(id, p) VALUES (1, ?1)", [tex])
        .unwrap();
    c.execute_batch(
        "INSERT INTO texset(id, n, t0) VALUES (1, 1, 1);
         INSERT INTO tex_use(tex_path_id, texset_id, slot) VALUES (1, 1, 0);",
    )
    .unwrap();
    for (i, (ty, flags, threshold, slsf2, derived)) in shapes.iter().enumerate() {
        let id = i as i64 + 2;
        c.execute(
            "INSERT INTO path(id, p) VALUES (?1, ?2)",
            rusqlite::params![id, format!("meshes/m{id}.nif")],
        )
        .unwrap();
        c.execute(
            "INSERT INTO origin(id, kind, name, root, priority, enabled) VALUES (?1, 1, ?2, ?2, ?1, 1)",
            rusqlite::params![id, format!("mod{id}")],
        )
        .unwrap();
        c.execute(
            "INSERT INTO nif_copy(id, path_id, origin_id, parse_status, is_derived) VALUES (?1, ?1, ?1, 0, ?2)",
            rusqlite::params![id, derived],
        )
        .unwrap();
        c.execute(
            "INSERT INTO shape(copy_id, block, kind, shader_kind, shader_type, slsf2, texset_id, alpha_flags, alpha_threshold) VALUES (?1, 0, 'BSTriShape', 1, ?2, ?3, 1, ?4, ?5)",
            rusqlite::params![id, ty, slsf2, flags, threshold],
        )
        .unwrap();
    }
}

/// M6 Track E spec §4.1, §6 (Note U5): one entry per source lighting shape, from NiAlphaProperty bit 0 (blend) and bit
/// 9 (test). No property is no alpha. TruePBR shapes and PGPatcher's output copies are left out; a shader type that
/// is no vote (Face Tint, 4) still counts.
#[test]
fn mesh_alpha_reads_the_alpha_property_of_each_source_lighting_shape() {
    use engine::shader::MeshAlpha;
    let d = tempfile::tempdir().unwrap();
    let db = d.path().join("scan.db");
    scan_db_alpha(
        &db,
        "textures/a/wall.dds",
        &[
            (0, Some(1), Some(0), 0, false),          // blend
            (0, Some(1 << 9), Some(128), 0, false),   // test at 128
            (4, Some(1 | 1 << 9), Some(90), 0, false), // both, on a Face Tint shape
            (0, None, None, 0, false),                // no NiAlphaProperty: no alpha
            (0, Some(1 << 9), Some(200), 1 << 23, false), // TruePBR: left out
            (0, Some(1), Some(0), 0, true),           // PGPatcher's output copy: left out
        ],
    );
    let keys = ["textures/a/wall".to_string(), "textures/a/none".to_string()]
        .into_iter()
        .collect();
    let v = sdc_gui::model::meshes::mesh_alpha(&db, &keys).unwrap();
    let mut got = v["textures/a/wall"].clone();
    got.sort_by_key(|a| (a.blend, a.test, a.threshold));
    let ma = |blend, test, threshold| MeshAlpha {
        blend,
        test,
        threshold,
    };
    assert_eq!(
        got,
        vec![
            ma(false, false, 0),
            ma(false, true, 128),
            ma(true, false, 0),
            ma(true, true, 90),
        ]
    );
    assert!(!v.contains_key("textures/a/none"));
}

/// M6 Track E spec §1 success, plan Review Focus 3 and 4: the scan's alpha evidence reaches the plan. A refresh that
/// changes only the alpha (a Face Tint shape is no shader vote) still reports a change, and a varying-alpha diffuse
/// is then no longer asked its alpha meaning.
#[test]
fn a_scan_database_decides_a_diffuse_alpha() {
    use engine::config::AlphaPolicy;
    use sdc_gui::model::items::Decision;
    let d = tempfile::tempdir().unwrap();
    let t = d.path().join("textures/a");
    std::fs::create_dir_all(&t).unwrap();
    image::ImageBuffer::from_fn(16, 16, |x, _| image::Rgba([90u8, 60, 30, (x * 16) as u8]))
        .save(t.join("glass.png"))
        .unwrap();
    let mut m = opened_as_the_app(d.path().join("textures"), d.path().join("export"));
    let asked = |m: &sdc_gui::model::AppModel| {
        m.item("a/glass.png")
            .unwrap()
            .decisions
            .contains(&Decision::AlphaMeaning)
    };
    assert!(asked(&m));
    let j = m.item("a/glass.png").unwrap().job.clone().unwrap();
    let key = engine::shader::mesh_key(&j.game_path, "glass");
    let dbdir = tempfile::tempdir().unwrap();
    let db = dbdir.path().join("scan.db");
    scan_db_alpha(&db, &format!("{key}.dds"), &[(4, Some(1), Some(0), 0, false)]);
    m.scan_db = Some(db);
    assert!(m.refresh_mesh_votes(), "only the alpha evidence changed");
    assert!(m.mesh_votes.is_empty(), "Face Tint is no shader vote");
    m.replan();
    assert!(m.wait_idle(Duration::from_secs(60)));
    let j = m.item("a/glass.png").unwrap().job.clone().unwrap();
    assert_eq!(
        (j.profile.alpha.policy, j.provenance["alpha.policy"].to_string()),
        (AlphaPolicy::Blend, "mesh:alpha".to_string())
    );
    assert!(!asked(&m));
}
```

- [ ] **Step 2: Run them and check that they fail**

Run: `cargo test -p sdc_gui --offline --test sets_model mesh_alpha`, then `cargo test -p sdc_gui --offline --test sets_model a_scan_database_decides`.
Expected: FAIL to compile: `meshes::mesh_alpha` does not exist.

- [ ] **Step 3: Implement**

`meshes.rs:9`:

```rust
use engine::shader::{MeshAlpha, Shader};
```

Replace `mesh_votes` (`:235-275`) with the shared clause, `mesh_votes` on it, and `mesh_alpha`:

```rust
/// The slot-0 source lighting shapes of `<key>.dds` (DECISIONS S-shader-10, Note U5): non-derived copies of enabled
/// origins, TruePBR shapes (SLSF2 bit 23) left out. `?1` is the texture path.
const SLOT0_SOURCE_SHAPES: &str = "FROM path tp
     JOIN tex_use tu ON tu.tex_path_id = tp.id AND tu.slot = 0
     JOIN shape s ON s.texset_id = tu.texset_id AND s.shader_kind = 1
                  AND (ifnull(s.slsf2, 0) & 8388608) = 0
     JOIN nif_copy c ON c.id = s.copy_id AND c.is_derived = 0
     JOIN origin o ON o.id = c.origin_id AND o.enabled = 1
     WHERE tp.p = ?1";

/// M6 Track A (DECISIONS S-shader-10): for each key (`engine::shader::mesh_key`), the shader of every lighting shape
/// that uses `<key>.dds` in slot 0, in non-derived copies of enabled origins. PGPatcher's output copies and TruePBR
/// shapes (SLSF2 bit 23) do not vote: sdc's own rules decide PBR. Types outside the 12 shaders do not vote
/// (`Shader::from_lighting_type`). Keys without a vote are left out.
pub fn mesh_votes(
    db: &Path,
    keys: &BTreeSet<String>,
) -> Result<BTreeMap<String, Vec<Shader>>, String> {
    let store = Store::open_read_only(db).map_err(db_err)?;
    // shortcut: raw SQL on `Store::conn()` against modscan's schema (breaks silently if `SCHEMA_VERSION` changes
    // these tables), one query per key; move it into `modscan::store` as a typed query when modscan is next edited.
    // `DISTINCT` per NIF path and block: the same mesh in two mods votes once.
    let mut st = store
        .conn()
        .prepare(&format!(
            "SELECT DISTINCT c.path_id, s.block, s.shader_type {SLOT0_SOURCE_SHAPES}"
        ))
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

/// NiAlphaProperty flag bits (nif.xml): alpha blending enable, alpha testing enable.
const ALPHA_BLEND: i64 = 1;
const ALPHA_TEST: i64 = 1 << 9;

/// M6 Track E (spec §4.1, DECISIONS Note U5): for each key, the NiAlphaProperty of every source lighting shape that
/// uses `<key>.dds` in slot 0, by `mesh_votes`'s rules but whatever the shader type. A shape without the property
/// is no alpha. Keys without a shape are left out.
pub fn mesh_alpha(
    db: &Path,
    keys: &BTreeSet<String>,
) -> Result<BTreeMap<String, Vec<MeshAlpha>>, String> {
    let store = Store::open_read_only(db).map_err(db_err)?;
    // shortcut: raw SQL on `Store::conn()`, as `mesh_votes`; move both into `modscan::store` when modscan is next
    // edited. `DISTINCT` per NIF path, block and alpha property: the same mesh in two mods counts once.
    let mut st = store
        .conn()
        .prepare(&format!(
            "SELECT DISTINCT c.path_id, s.block, s.alpha_flags, s.alpha_threshold {SLOT0_SOURCE_SHAPES}"
        ))
        .map_err(db_err)?;
    let mut out = BTreeMap::new();
    for k in keys {
        let rows = st
            .query_map([format!("{k}.dds")], |r| {
                Ok((r.get::<_, Option<i64>>(2)?, r.get::<_, Option<i64>>(3)?))
            })
            .and_then(|rows| rows.collect::<Result<Vec<_>, _>>())
            .map_err(db_err)?;
        let v: Vec<MeshAlpha> = rows
            .into_iter()
            .map(|(flags, threshold)| {
                let f = flags.unwrap_or(0);
                MeshAlpha {
                    blend: f & ALPHA_BLEND != 0,
                    test: f & ALPHA_TEST != 0,
                    threshold: threshold.unwrap_or(0).clamp(0, 255) as u8,
                }
            })
            .collect();
        if !v.is_empty() {
            out.insert(k.clone(), v);
        }
    }
    Ok(out)
}
```

`mod.rs`, `EngineInputs`. Add the field after `mesh_votes`, and in `of()`:

```rust
    mesh_alpha: std::collections::BTreeMap<String, Vec<engine::shader::MeshAlpha>>,
```

```rust
            mesh_alpha: m.mesh_alpha.clone(),
```

`AppModel`, after `mesh_votes`:

```rust
    /// M6 Track E (DECISIONS Note U5): `shader::colour_mesh_key` → the alpha property of each scanned shape that uses
    /// the colour member; sent as `BatchRequest::mesh_alpha`.
    pub mesh_alpha: std::collections::BTreeMap<String, Vec<engine::shader::MeshAlpha>>,
```

`placeholder()`, after `mesh_votes: Default::default(),`:

```rust
            mesh_alpha: Default::default(),
```

`request()`, after `req.mesh_shaders = self.mesh_votes.clone();`:

```rust
        req.mesh_alpha = self.mesh_alpha.clone();
```

`refresh_mesh_votes`. Replace its doc comment and the `votes`…`changed` lines:

```rust
    /// S-shader-10, M6 Track E: read the mesh votes and the mesh alpha for the plan's sets (keys from the colour-member
    /// jobs only, by `shader::colour_mesh_key`, the engine's rule). No database, or a read error, means no evidence:
    /// the suffix rules decide and the alpha is asked, and the Meshes view reports database errors. True when either
    /// changed.
```

```rust
        let (votes, alpha) = match &self.scan_db {
            Some(db) if db.is_file() => (
                meshes::mesh_votes(db, &keys).unwrap_or_default(),
                meshes::mesh_alpha(db, &keys).unwrap_or_default(),
            ),
            _ => Default::default(),
        };
        let changed = votes != self.mesh_votes || alpha != self.mesh_alpha;
        self.mesh_votes = votes;
        self.mesh_alpha = alpha;
        changed
```

`replan()`, the `EngineInputs` literal, after `mesh_votes: req.mesh_shaders.clone(),`:

```rust
            mesh_alpha: req.mesh_alpha.clone(),
```

- [ ] **Step 4: Run the tests and check that they pass**

Run: `cargo test -p sdc_gui --offline --test sets_model`, then `cargo test --workspace --offline`.
Expected: PASS. `mesh_votes_count_enabled_source_lighting_shapes_only` and `a_scan_database_picks_the_set_shader` are unchanged, which pins that the shared clause keeps the vote rules.

- [ ] **Step 5: Run the gates, then commit**

```bash
git add crates/gui/src/model/meshes.rs crates/gui/src/model/mod.rs crates/gui/tests/support/mod.rs crates/gui/tests/sets_model.rs
git commit -m "feat(gui): read mesh alpha from the scan database and send it as BatchRequest::mesh_alpha (M6 Track E U5, U9)

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

---

### Task 5: GUI — accept High suggestions, "(auto)", `mesh:alpha` prose, the coat card title

**Spec:** §2, §3.1 (the slot label), §6 "Model", "(auto)", Review Focus 3. Rulings U2, U3, U4.

**Files:**
- Modify: `crates/gui/src/model/mod.rs`:
  - `AppModel` (a new field after `mesh_alpha`);
  - `placeholder()`;
  - `request()` (`:379-386`).
- Modify: `crates/gui/src/model/fields.rs`:
  - the imports at `:2-4`;
  - `provenance_text` (`:232-253`);
  - a new `auto_subtype_reason` after it.
- Modify: `crates/gui/src/ui/settings_panel.rs`:
  - the `fields` import at `:4-8`;
  - `field_row` (`:380-390`).
- Modify: `crates/gui/src/model/sets.rs`: a new `slot_label` after `SlotKind`'s `impl` (`:85`).
- Modify: `crates/gui/src/ui/set_editor.rs`:
  - `card` (`:187-192`);
  - the import of `sets` items. Add `slot_label` to the existing `use crate::model::sets::{…}` line.
- Modify: `crates/gui/tests/support/mod.rs`, `opened()` (`:53`).
- Modify: the existing GUI test constructions that plan, which keep "nothing accepted" (ruling U3). Directly after each `let mut <var> = …AppModel::new(…);` statement, add `<var>.accept = engine::policy::AcceptSuggestions::None;`. The sites:
  - `crates/gui/tests/edges.rs`: `:22`, `:176`, `:196`, `:372`, `:413`;
  - `crates/gui/tests/history.rs`: `:153-154`;
  - `crates/gui/tests/model.rs`: `:148`;
  - `crates/gui/tests/packing_fills.rs`: `:50`, `:570`, `:669`;
  - `crates/gui/tests/pbr_members.rs`: `:43`;
  - `crates/gui/tests/sets_model.rs`: `:821`;
  - `crates/gui/tests/ui_smoke.rs`: `:621` (`model`), `:640` (`model`), `:923`, `:1037`, `:1070`, `:1105`.

  That makes 17 here, plus `support::opened`, for 18. Do **not** add it to `support::opened_as_the_app` or to the tests this plan adds.
- Test: `crates/gui/tests/model.rs`, `crates/gui/tests/ui_smoke.rs`, `crates/gui/tests/set_editor_ui.rs`, `crates/gui/tests/fields.rs`.

**Interfaces:**
- Consumes: `support::opened_as_the_app` (Task 4), `Provenance::MeshAlpha` (Task 3).
- Produces:
  - `AppModel::accept: engine::policy::AcceptSuggestions` (default `High`).
  - `pub fn fields::auto_subtype_reason(job: &ResolvedJob) -> Option<&str>`.
  - `pub fn sets::slot_label(slot: SlotKind, key: &str) -> &'static str`.

- [ ] **Step 1: Write the failing tests**

`crates/gui/tests/model.rs`, at the end:

```rust
/// M6 Track E spec §2, §6: the GUI accepts High suggestions. A PBR set's colour, `_n` and `_rmaos` members decide
/// themselves (no sub-type question; "(auto)" with the reason), and a Medium `_i` still asks.
#[test]
fn the_gui_accepts_high_suggestions_and_still_asks_for_medium_ones() {
    use engine::policy::{AcceptSuggestions, Provenance};
    use sdc_gui::model::fields::auto_subtype_reason;
    let d = tempfile::tempdir().unwrap();
    let t = d.path().join("textures/a");
    std::fs::create_dir_all(&t).unwrap();
    for (name, px) in [
        ("iron.png", [120u8, 90, 60]),
        ("iron_n.png", [128, 128, 255]),
        ("iron_rmaos.png", [128, 0, 255]),
        ("wall_i.png", [90, 90, 90]),
    ] {
        image::ImageBuffer::from_fn(16, 16, |_, _| image::Rgb(px))
            .save(t.join(name))
            .unwrap();
    }
    let m = opened_as_the_app(d.path().join("textures"), d.path().join("export"));
    assert_eq!(m.request().unwrap().accept, AcceptSuggestions::High);
    let asks_subtype = |key: &str| {
        m.item(key)
            .unwrap()
            .decisions
            .iter()
            .any(|x| matches!(x, Decision::Subtype { .. }))
    };
    for (key, want) in [
        ("a/iron.png", Subtype::PbrAlbedo),
        ("a/iron_n.png", Subtype::PbrNormal),
        ("a/iron_rmaos.png", Subtype::Rmaos),
    ] {
        let job = m.item(key).unwrap().job.clone().unwrap();
        assert_eq!(
            (job.profile.subtype, job.provenance["subtype"].clone()),
            (want, Provenance::Suggestion),
            "{key}"
        );
        assert!(!asks_subtype(key), "{key}");
        assert_eq!(
            auto_subtype_reason(&job),
            Some(job.suggestions[0].reason.as_str()),
            "{key}"
        );
    }
    assert!(asks_subtype("a/wall_i.png"));
    let wall = m.item("a/wall_i.png").unwrap().job.clone().unwrap();
    assert_eq!(auto_subtype_reason(&wall), None);
}

/// Spec Review Focus 3: a sub-type the user chose (`sdc.toml` `[files]`) beats the accepted suggestion and is not
/// "(auto)".
#[test]
fn an_explicit_subtype_beats_an_accepted_suggestion() {
    use sdc_gui::model::fields::auto_subtype_reason;
    let d = tempfile::tempdir().unwrap();
    let mut m = opened_as_the_app(mod_folder(d.path()), d.path().join("export"));
    let job = m.item("landscape/rock01.png").unwrap().job.clone().unwrap();
    assert!(auto_subtype_reason(&job).is_some(), "{:?}", job.status);
    m.decide("landscape/rock01.png", Answer::Subtype(Subtype::Diffuse));
    assert!(m.wait_idle(Duration::from_secs(60)));
    let job = m.item("landscape/rock01.png").unwrap().job.clone().unwrap();
    assert_eq!(
        (job.profile.subtype, job.provenance["subtype"].to_string()),
        (Subtype::Diffuse, "project_file".to_string())
    );
    assert_eq!(auto_subtype_reason(&job), None);
}
```

`crates/gui/tests/ui_smoke.rs`, at the end:

```rust
/// M6 Track E spec §2, §6: a sub-type decided by an accepted High suggestion reads "(auto)" next to it, with the
/// suggestion's reason as hover text.
#[test]
fn an_accepted_suggestion_reads_auto_with_its_reason_on_hover() {
    let d = tempfile::tempdir().unwrap();
    let mut model = opened_as_the_app(mod_folder(d.path()), d.path().join("export"));
    model.select_item("landscape/rock01.png");
    let mut h = harness(model);
    h.ctx.global_style_mut(|s| {
        s.interaction.tooltip_delay = 0.0;
        s.interaction.show_tooltips_only_when_still = false;
    });
    h.run();
    assert!(
        h.query_by_label_contains("colour under textures/landscape")
            .is_none(),
        "the reason shows only on hover"
    );
    h.get_by_label("(auto)").hover();
    h.run();
    h.run();
    h.get_by_label_contains("colour under textures/landscape");
}
```

`crates/gui/tests/set_editor_ui.rs`, at the end:

```rust
/// M6 Track E spec §3.1 (Note U2): a `_coat` source in the Subsurface slot titles its card "Coat colour + strength".
#[test]
fn a_coat_source_titles_the_subsurface_card_coat_colour_and_strength() {
    use sdc_gui::model::sets::{slot_label, SlotKind};
    assert_eq!(
        slot_label(SlotKind::Subsurface, "landscape/rock01_s.png"),
        "Subsurface _s"
    );
    assert_eq!(
        slot_label(SlotKind::Subsurface, "landscape/rock01_Coat.png"),
        "Coat colour + strength"
    );
    assert_eq!(slot_label(SlotKind::Diffuse, "landscape/rock01.png"), "Diffuse");
    let d = tempfile::tempdir().unwrap();
    let r = d.path().join("textures/landscape");
    std::fs::create_dir_all(&r).unwrap();
    image::ImageBuffer::from_fn(64, 64, |x, _| image::Rgba([200u8, 180, 160, (x * 4) as u8]))
        .save(r.join("rock01_coat.png"))
        .unwrap();
    let mut m = pbr_opened(d.path());
    rock_selected(&mut m);
    let mut h = Harness::builder()
        .with_size(eframe::egui::vec2(1400.0, 1200.0))
        .build_ui_state(|ui, model: &mut AppModel| sdc_gui::ui::draw(ui, model), m);
    sdc_gui::fonts::install_system_fonts(&h.ctx).expect("system fonts");
    h.run();
    h.get_by_label("Coat colour + strength");
    assert!(h.query_by_label("Subsurface _s").is_none());
}
```

`crates/gui/tests/fields.rs`, `provenance_reads_as_prose`. Add before the closing brace:

```rust
    assert_eq!(
        provenance_text("mesh:alpha"),
        "from the meshes that use this texture"
    );
```

- [ ] **Step 2: Run them and check that they fail**

Run: `cargo test -p sdc_gui --offline --test model --test ui_smoke --test set_editor_ui --test fields`
Expected: FAIL to compile: `auto_subtype_reason`, `slot_label` and `AppModel::accept` do not exist.

- [ ] **Step 3: Implement**

`mod.rs`, `AppModel`, after `mesh_alpha`:

```rust
    /// What the plan accepts (M6 Track E spec §2, DECISIONS Note U3): High suggestions, as the app runs. The M3-era
    /// test fixtures set `None` to keep their High members undecided, as the CLI does without the flag.
    pub accept: AcceptSuggestions,
```

`placeholder()`, after `mesh_alpha: Default::default(),`:

```rust
            accept: AcceptSuggestions::High,
```

`request()`. The doc comment becomes `/// What the next plan uses: the folder, recursive, High suggestions accepted (Note U3), the in-memory \`sdc.toml\` (DECISIONS N4).`, and replace `req.accept = AcceptSuggestions::None;` with:

```rust
        req.accept = self.accept;
```

`fields.rs`, the imports:

```rust
use engine::config::*;
use engine::policy::{JobStatus, Need, Provenance, ResolvedJob};
use engine::suggest::Confidence;
use serde_json::Value;
```

`provenance_text`, a new arm after `"set" => …`:

```rust
        "mesh:alpha" => "from the meshes that use this texture".into(),
```

After `provenance_text`:

```rust
/// M6 Track E spec §2 (DECISIONS Note U4): the reason of the High suggestion that decided `job`'s sub-type, shown as
/// "(auto)" with this hover text. `None` when the user, a rule or a shader chose it, or it still asks.
pub fn auto_subtype_reason(job: &ResolvedJob) -> Option<&str> {
    let top = job.suggestions.first()?;
    let asks = matches!(&job.status, JobStatus::Unresolved { needs, .. } if needs.contains(&Need::Subtype));
    (job.provenance.get("subtype") == Some(&Provenance::Suggestion)
        && top.confidence == Confidence::High
        && !asks)
        .then_some(top.reason.as_str())
}
```

`settings_panel.rs`. Add `auto_subtype_reason` to the `crate::model::fields::{…}` import. In `field_row`, put a new branch between the `f.lock` branch and the final `else`:

```rust
    } else if let Some(why) = (f.field == "subtype")
        .then(|| auto_subtype_reason(cx.job))
        .flatten()
    {
        // M6 Track E (Note U4): a sub-type an accepted High suggestion decided; hover says why.
        ui.label(egui::RichText::new("(auto)").small().weak())
            .on_hover_text(why);
```

`sets.rs`, after `impl SlotKind { … }`:

```rust
/// The card title of `slot` holding item `key`: a `_coat` source in the Subsurface slot is the coat colour + strength
/// (M6 Track E spec §3.1, DECISIONS Note U2), decided by the file name.
pub fn slot_label(slot: SlotKind, key: &str) -> &'static str {
    let stem = Path::new(key).file_stem().unwrap_or_default().to_string_lossy();
    match split_vocab(&stem) {
        Some((_, v)) if slot == SlotKind::Subsurface && v.role == VocabRole::CoatColour => {
            "Coat colour + strength"
        }
        _ => slot.label(),
    }
}
```

`set_editor.rs`, `card`. Replace the title line `ui.label(egui::RichText::new(slot.label()).strong());` with:

```rust
            let title = match &fill {
                SlotFill::Own { item } | SlotFill::Borrowed { item, .. } => slot_label(slot, item),
                SlotFill::Empty => slot.label(),
            };
            ui.label(egui::RichText::new(title).strong());
```

The format combo's `id_salt` keeps `slot.label()`.

`crates/gui/tests/support/mod.rs`, `opened()`. Directly after `let mut m = AppModel::new(settings, None, session());`:

```rust
    // M3-era fixture: High members stay undecided, as before M6 Track E (DECISIONS Note U3).
    m.accept = engine::policy::AcceptSuggestions::None;
```

Then add the one-line `accept = None` at the 17 other sites listed under **Files**.

- [ ] **Step 4: Run the tests and check that they pass**

Run: `cargo test -p sdc_gui --offline`, then `cargo test --workspace --offline`.
Expected: PASS. The M3-era GUI tests are unchanged in meaning because their models still send `None`. In `an_accepted_suggestion_reads_auto_with_its_reason_on_hover`, if the pre-hover `is_none()` fails because another widget shows the same reason, drop that one assertion (keep the hover check) and name the widget in the report. If one fails, it constructs an `AppModel` at a site this task missed. Add the `accept = None` line there and name the site in the report.

- [ ] **Step 5: Run the gates, then commit**

```bash
git add crates/gui/src/model/mod.rs crates/gui/src/model/fields.rs crates/gui/src/ui/settings_panel.rs crates/gui/src/model/sets.rs crates/gui/src/ui/set_editor.rs crates/gui/tests/support/mod.rs crates/gui/tests/model.rs crates/gui/tests/ui_smoke.rs crates/gui/tests/set_editor_ui.rs crates/gui/tests/fields.rs crates/gui/tests/edges.rs crates/gui/tests/history.rs crates/gui/tests/packing_fills.rs crates/gui/tests/pbr_members.rs crates/gui/tests/sets_model.rs
git commit -m "feat(gui): accept High suggestions, (auto) with its reason, mesh:alpha prose, coat card title (M6 Track E U2-U4)

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

---

### Task 6: Docs — DECISIONS Note U, `docs/gui.md`, workspace version 0.6.0

**Spec:** §5 (version, Note U, `docs/gui.md`). Rulings U1–U10.

**Files:**
- Modify: `docs/DECISIONS.md`. Append the note below at the end of the file, after T13's last sub-bullet ("Known quirk: …").
- Modify: `docs/gui.md`:
  - the "Deciding" section (`:20-22`);
  - the set editor paragraph (`:77`).
- Modify: `Cargo.toml:7`, `version = "0.5.0"` → `"0.6.0"`. Then `cargo build --workspace --offline` updates `Cargo.lock`.

The text of Note U (each bullet's form follows Note T):

```markdown
## Note U — M6 Track E: NIF-aware alpha and fewer confirmations (plan rulings, 2026-10-12)

Rulings of `docs/superpowers/plans/2026-10-12-sdc-m6-nif-alpha-confident.md` on spec `docs/superpowers/specs/2026-10-12-sdc-m6-nif-alpha-confident-design.md` (owner-approved 2026-10-12).

- **U1 No suffix is the colour member, at High (owner, 2026-10-12: "No suffix should equal diffuse by default").**
  - This reverses Note H's "a bare stem is only a guess". A bare stem suggests `diffuse`, `pbr_albedo` (a PBR set) or `ui_sky` (`interface/`, `sky/`) at High, like `_d`.
  - `landscape_diffuse` and `skin_albedo` were already High. `hdr_sky` (a float source under `sky/` may be an HDRI) and `_i` stay Medium.
  - Names that break the conventions (a normal map without `_n`) now resolve as colour maps; fixing them is the mod author's job (owner).
  - Note H's colour-term raise was removed: every colour member it raised is now High.
- **U2 `_coat` is coat colour + strength (owner, 2026-10-12).**
  - `pbr_subsurface` keeps alpha policy `data` (`sdc-base`), so a varying alpha is kept as the strength and stays BC7 under compact (T3).
  - The reason reads "exporter suffix _coat (coat colour + strength); the PBR coat map (_s)".
  - The slot card reads "Coat colour + strength" when its source file is a `_coat` (by file name).
- **U3 The GUI accepts High suggestions (owner, 2026-10-12).**
  - `AppModel::accept` is `High`, and `request()` sends it. Medium and Low suggestions still ask.
  - Explicit choices win: `[files]`, rules and `--set subtype` decide before any suggestion.
  - The CLI is unchanged (`--accept-suggestions high`).
  - The M3-era GUI test fixtures set `accept = None` to keep their undecided members.
- **U4 "(auto)" (plan, 2026-10-12).** There is no new engine field. A job shows "(auto)" in the Sub-type row's provenance column when all three hold, with the top suggestion's reason on hover:
  - its `provenance.subtype` is `suggestion`;
  - its top suggestion is High;
  - it does not need a sub-type.
- **U5 Mesh alpha evidence (plan, 2026-10-12).**
  - `BatchRequest::mesh_alpha` is keyed by `shader::colour_mesh_key`, the output stem, so a renamed `x_albedo.png` is keyed `x`. The CLI sends none.
  - The GUI reads it with raw SQL (`meshes::mesh_alpha`, sharing `mesh_votes`'s `FROM` clause):
    - lighting shapes using `<key>.dds` in slot 0;
    - non-derived copies of enabled origins;
    - TruePBR excluded;
    - one entry per NIF path, block and alpha property.
  - Unlike the votes, every lighting shader type counts.
  - NiAlphaProperty bit 0 is blend and bit 9 is test. A shape with no property is no alpha.
- **U6 The rule (owner, 2026-10-12).**
  - Any shape with alpha beats shapes without; there is no vote.
  - Blend only gives `blend`; test only gives `cutout`; blend and test anywhere, on one shape or two, give `cutout_blend`; no alpha gives `remove`.
  - The threshold is the largest of the testing shapes (the stricter cutoff), clamped to 5..=200. Spec gap: a NIF may say 0..255, and sdc refuses a threshold outside 5..=200.
- **U7 The `mesh:alpha` layer (plan, 2026-10-12).**
  - **Provenance.** `Provenance::MeshAlpha` (`mesh:alpha`) sits between `shader:<id>` and `suffix:<term>`.
  - **Where.** It is applied after the analysis-alpha step and `skin_wet_form`, before the shader layer, and only for `alpha_from_mesh` sub-types where `alpha.policy` is not explicit.
  - **What beats it.** User presets, configs and rules (before it), and `[files]` and `--set` (after it).
  - **The analysis `remove` keeps priority** (no alpha channel, or alpha constant 255). This extends spec §4.3's "source without alpha" to constant 255.
  - **What it beats.** A shader's alpha default (hair `cutout`).
  - **Explicit.** It counts as explicit, so the alpha question is not asked. `alpha.threshold` gets the same provenance unless that field is explicit.
- **U8 Compact (plan, 2026-10-12).**
  - A mesh `remove` on a varying-alpha diffuse is an explicit remove (P25): there is no `LOSS-ALPHA-DATA`. It stays BC7 under compact, because the BC1 rules match the source's alpha content, not the policy (T2).
  - A mesh `cutout` never reaches a constant-255 source (U7), so an opaque RGBA diffuse stays BC1.
- **U9 Two-pass GUI (plan, 2026-10-12).** The first plan of a folder asks the alpha question until the scan's evidence is read. `EngineInputs` carries `mesh_alpha`, so the automatic re-plan settles it.
- **U10 Versions (plan, 2026-10-12).**
  - There are no new codes (75).
  - The workspace goes 0.5.0 → 0.6.0, so `if-changed` re-encodes.
  - There is no manifest change: the up-to-date check compares `engine_version` and `profile_sha256`, and both change.
  - `schema/` was regenerated only for the `Provenance` doc text.
```

`docs/gui.md`, the "Deciding" section. Replace the paragraph under `## Deciding` (`:22`) with:

```markdown
A texture whose name follows the conventions decides itself. A High-confidence suggestion is accepted, and its sub-type reads **(auto)**; hover it to see why. High suggestions include:

- a known suffix such as `_n`, `_rmaos` or `_rfaos`;
- an exporter term such as `_albedo`;
- a file with no suffix, as its set's colour map;
- `_n` in a PBR set.

Choose another sub-type to override it: your choice is written to `sdc.toml` and always wins. Naming that breaks the conventions (a normal map saved without `_n`) is read as a colour map, so rename the file or choose its sub-type. A texture that still needs an answer has an **amber border**: a Medium or Low sub-type suggestion, the meaning of its alpha, the green-channel convention of a normal map, or a blocking warning you must acknowledge. Select it and use the confirm controls in the centre column (for example the sub-type buttons, the *Remove / Cutout / Blend / Cutout + Blend / Data* alpha buttons, *DirectX / OpenGL* for green, or an acknowledge button for a blocking warning). Each answer is written explicitly, as a field value or as an `allow` code, so it stays in your project file.

**Alpha from the meshes.** When a scan database exists (the Meshes view), a colour map whose alpha varies takes its alpha meaning from the meshes that use it, and the alpha question is not asked:

- any mesh with alpha blending gives *Blend*;
- alpha testing gives *Cutout*, at the strictest mesh threshold;
- both give *Cutout + Blend*;
- no mesh with alpha gives *Remove*.

The Alpha row then reads *from the meshes that use this texture*. A texture with no alpha, or a fully opaque one, keeps *Remove*. Your own choice still wins. Right after a folder opens, the question may show until the scan has been read.
```

`docs/gui.md:77`, at the end of the set editor paragraph ("**Opaque** sits on the Diffuse card."), add:

```markdown
A `_coat` map fills the Subsurface card, which then reads **Coat colour + strength**: its alpha is the coat strength and is kept.
```

- [ ] **Step 1: Write the DECISIONS and gui.md edits**, as given above.
- [ ] **Step 2: Bump the version.** Set `Cargo.toml:7` to `version = "0.6.0"`, then run `cargo build --workspace --offline`. Expected: `git diff --stat Cargo.lock` shows only the workspace crates' version lines changed.
- [ ] **Step 3: Run the gates (unpiped).** Expected: all green. `ENGINE_VERSION` follows `CARGO_PKG_VERSION`, and no test pins `0.5.0` (checked at 64d1646). If one does, update it and name it in the report.
- [ ] **Step 4: Commit (two commits, as Tracks A and D did)**

```bash
git add docs/DECISIONS.md docs/gui.md
git commit -m "docs: DECISIONS Note U (U1-U10), gui.md (auto) and mesh-decided alpha (M6 Track E)

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
git add Cargo.toml Cargo.lock
git commit -m "chore: workspace version 0.6.0 (bare stems High, mesh-decided alpha; if-changed re-encodes) (M6 Track E)

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

---

## Spec coverage

| Spec | Task |
|---|---|
| §1 success: High members of a correctly named set ask nothing in the GUI; `_rmaos`, `_n`, bare colour resolve without a click | 1 (engine), 5 (`the_gui_accepts_high_suggestions_and_still_asks_for_medium_ones`) |
| §1 success: Medium and Low still ask | 1 (CLI `_i` tests), 5 (`wall_i.png`) |
| §1 success: a varying-alpha diffuse with scanned meshes gets its policy and is not asked | 3, 4 (`a_scan_database_decides_a_diffuse_alpha`) |
| §1 success: the CLI is unchanged; no evidence behaves as today | 1 (CLI tests), 3 (`mesh_evidence_decides_the_alpha_and_no_evidence_asks_as_before`); ruling U3 |
| §2 `request()` accepts High; explicit choices win; "(auto)" with reason; no new engine field | 5; rulings U3, U4 |
| §3 bare stem High (diffuse, pbr_albedo via every PBR marker, ui_sky); `hdr_sky` and `_i` Medium; `_rfaos` advanced skin unchanged; `is_pbr_set` unchanged | 1; ruling U1 |
| §3.1 `_coat` = colour + strength: `data`, reason text, slot label, never BC1 under compact | 1 (`coat_is_coat_colour_and_strength`, `compact_coat_keeps_its_strength_alpha_as_bc7`), 5 (`slot_label`); ruling U2 |
| §4.1 `BatchRequest::mesh_alpha`, `MeshAlpha`, `meshes::mesh_alpha` source rules, bits, no-property = no alpha, raw SQL `shortcut:`, both maps in one refresh | 2, 3, 4; ruling U5 |
| §4.2 the table, any alpha beats none, largest threshold | 2; ruling U6 |
| §4.3 alpha_from_mesh colour member only; `mesh:alpha` placement; explicit; threshold; source without alpha; no evidence | 3; ruling U7 |
| §5 no new codes; 0.6.0; Note U; gui.md | 6; ruling U10 |
| §6 tests: model, "(auto)", suggestions, `mesh_alpha_policy`, planning, GUI DB read | 5, 5, 1, 2, 3, 4 |
| §7 Review Focus 1–5 | 2, 3, 5, 3, 3 (see Review Focus above) |

## Spec gaps found while planning

1. **NIF thresholds are 0..=255, but sdc refuses `alpha.threshold` outside 5..=200** with `CFG-INVALID`. The rule clamps (U6).
2. **"Ignored when the source has no alpha channel" also holds for alpha constant 255.** The analysis `remove` is an explicit fact for both, and the mesh layer yields to it. This is what spec Review Focus 2 wants (U7).
3. **Mesh alpha beats a shader's alpha default.** The spec places the layer "as the shader-default layer does" but does not order the two. The mesh is the more specific evidence (U7).
4. **Alpha evidence and shader votes differ on the shader-type filter.** The spec names only the four source rules, so every lighting type counts for alpha (U5).
5. **Note H's vocabulary colour raise becomes dead code** once bare stems are High. It is removed (U1).
6. **The M3-era GUI fixtures depend on "nothing accepted"** (High members under `landscape/` that tests expect undecided). `AppModel::accept` keeps `request()` at High for the app, and lets those 18 test sites keep `None` instead of rewriting their scenarios (U3).
7. **A mesh `remove` on varying alpha raises no `LOSS-ALPHA-DATA`** (explicit remove, P25) and stays BC7 under compact (U8). The spec asked for this to be recorded.
8. **The `Provenance` doc text is part of the event and manifest schemas,** so `schema/` is regenerated in Task 3. The format does not change (U10).
9. **The GUI's first plan may ask the alpha question** until the scan is read and the automatic re-plan lands (U9).
