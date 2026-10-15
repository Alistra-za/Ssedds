# PGPatcher Complex Material Card Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** A per-set "PGPatcher complex material" card that, when its export box is ticked, writes `<texture>.json` (only the keys the user set, plus `$schema`) next to every complex `_m` of the set after a clean export, on CS and Vanilla. Under Vanilla, a varying-alpha `_m` becomes a complex `_m` too (D16).

**Architecture:** Under Vanilla the engine stops falling back from `complex_material` to `envmask`. That takes two deletions, in `policy.rs` and `suggest.rs`, and changes no format. On the GUI side, a new `[sets.cm]` sub-table in `sdc-project.toml` is parsed into `SetEntry.cm` and kept out of the TruePBR settings through `RESERVED`. A new `model/cm_json.rs` decides which members get a JSON, builds and validates the text, and writes it from the export hooks with a hand-edit guard and a hash record (`[project] cm_json_sha256`). This copies the CS PBR export (`model/cs_pbr_export.rs`). A new `ui/cm_card.rs` draws the card in the set editor.

**Tech Stack:** Rust 2021 (workspace 0.7.0, rust 1.88), egui/eframe + egui_kittest 0.36.2, toml_edit, serde_json (no `preserve_order`), the existing draft-07 subset validator in `crates/gui/src/pgp/validate.rs`.

**Spec:** `F:\ai\dds\nifparseref\plan\complex-material\2026-10-14-complex-material-design.md` (APPROVED 2026-10-14, including the golden updates). Evidence: `01-research.md`, `02-test-jsons.md`, `03-nif-check.md` in the same folder. The vendored schema to copy is `F:\ai\dds\nifparseref\plan\complex-material\complex_material.schema.json`: upstream PGPatcher `0176567:schema/complex_material.schema.json`, LF, sha256 `001f12add94f103fa6bb1fe5512b22af08587f8d9f633e2b2900507d9a553451`.

## Global Constraints

- Branch `nif/l-cm` from `nif/main` in a new worktree `F:\ai\dds\converter-nif-cm`. The spec names `bd495d2`. The tip is now `f5e60dc`, which is `bd495d2` plus one docs-only commit, so branch from the tip after the check in Task 1 Step 1.
- `CARGO_TARGET_DIR=F:\ai\dds\converter-nif-p1\target` for every cargo command. In Git Bash: `export CARGO_TARGET_DIR=/f/ai/dds/converter-nif-p1/target`.
- Never push. Never merge to `main`. Never touch `F:\ai\dds\converter`.
- No new crates: `Cargo.lock` stays unchanged.
- Owner-approved edits: `crates/gui/src/sets_doc.rs`, `crates/gui/src/pgp/validate.rs`, `crates/engine/src/policy.rs`, `crates/engine/src/suggest.rs`, the `presets/vanilla-quality.toml` description, regeneration of `schema/`, the two golden tests in spec §8, and one new Note **AT** in `docs/DECISIONS.md` (AS is the last note).
- Still off-limits:
  - `crates/cli` source and `crates/encoders-sys`.
  - The `gui_repro` tests, and every golden test except the two named.
  - `crates/gui/src/model/pbr_export.rs` and `crates/gui/src/model/sets.rs`. Import from them; do not edit them.
- Shared files are add-only, in their own commits: root `Cargo.toml`, `Cargo.lock`, `crates/gui/Cargo.toml`, `ui/mod.rs`, `model/mod.rs`, `app.rs`, `settings.rs`, `worker.rs`.
- Tests never read owner data, except `#[ignore]` tests with `SDC_ACHERUS`. Never read the real `scan.db`.
- Commits:
  - Name explicit paths. Never `git add -A` or `git add .`.
  - End every message with `Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>`.
- Never run `python -` with stdin.
- `render.rs` GPU tests may hang under contention. The gate uses `cargo test --workspace -- --skip render_` plus a separate `cargo test -p sdc-gui --test render` run.
- Copy rules:
  - User-facing text says "Tessalith" where the current tree does. For example, `cs_pbr_export::finish` says "kept as edited outside Tessalith".
  - File names (`sdc-project.toml`) and the CLI (`sdc`) stay `sdc`.
- The JSON file name is `<the _m's output>.json`, so `.dds` becomes `.json` (D9). It is never `.dds.json`.

## Review Focus

1. **A non-bool `export` in `[sets.cm]`** (`export = "yes"`) reads as off and is listed as a bad key. It never panics and never exports. Test: Task 3 `a_wrong_typed_export_reads_as_off_and_is_listed`.
2. **TOML integers** (`glossiness = 80`, `specular_color = [1, 1, 1]`) count as numbers and are written as floats (`80.0`, `1.0`). Tests: Task 3 `a_cm_block_reads_back_…` (ints parse) and Task 4 `to_text_keeps_only_well_typed_keys_as_floats`.
3. **A `[sets.cm]` block with export on and no complex `_m`** (the shader changed, the `_m` moved, the set has no `_m`) gets a "no complex _m" export note and no file, and the card still shows. Tests: Task 4 `a_stored_block_keeps_the_card` and Task 5 `a_set_without_a_complex_m_gets_a_note`.
4. **An unparsable `sdc-project.toml`.** The cm setters change nothing and the card is greyed out. The export records nothing. Test: Task 3 `a_read_only_document_takes_no_cm_edit`.
5. **A fractional smallest-mip alpha.** An averaged non-square mip can give 250.6. It is rounded, so 250.6 shows as 251 and lands in the 251–254 band. Test: Task 6 `the_alpha_lines_follow_the_latest_export_row`.

---

## File Structure

| File | Status | Responsibility |
|---|---|---|
| `crates/engine/src/policy.rs` | modify (approved) | Delete the Vanilla `complex_material` → `envmask` arm (`:1103`) |
| `crates/engine/src/suggest.rs` | modify (approved) | Delete the "_m under vanilla" branch (`:109-110`); update its unit test (`:350-360`) |
| `crates/engine/tests/policy_checks.rs` | modify | `availability_table`, `allow_cfg_unavailable_accepts_fallbacks_only` |
| `crates/engine/tests/complex_m_alpha.rs` | modify | New Vanilla `_m` pipeline tests |
| `crates/engine/tests/presets.rs` | modify | `vanilla-compact` halves a complex `_m` |
| `crates/engine/tests/golden.rs` | modify (approved, 2 tests only) | `vanilla_fallbacks_are_written_with_allow_cfg_unavailable`, `vanilla_g_is_glow_and_m_with_alpha_is_envmask_with_loss` |
| `presets/vanilla-quality.toml` | modify (approved) | Description text only |
| `crates/gui/assets/complex_material.schema.json` | create | The vendored upstream schema, byte for byte |
| `crates/gui/src/pgp/validate.rs` | modify (approved) | `CM_SCHEMA_JSON`, `validate_cm` |
| `crates/gui/tests/pgp.rs` | modify | The keyword guard walks the CM schema; `validate_cm` tests |
| `crates/gui/src/model/cm_json.rs` | create | Key table, `CmBlock` and its parser, model setters, writer, member predicate, targets, export (`prepare`/`finish`/`finish_cm_json`), card warnings. No egui |
| `crates/gui/src/sets_doc.rs` | modify (approved) | `RESERVED` += `cm`, `SetEntry.cm`, `set_cm_value`/`set_cm_export`, `cm_json_shas`/`set_cm_json_shas`, and undo keeps the record |
| `crates/gui/src/model/mod.rs` | shared, add-only | `pub mod cm_json;` and a `pending_cm` field with its 4 hook lines |
| `crates/gui/src/ui/cm_card.rs` | create | The card (drawing only) |
| `crates/gui/src/ui/mod.rs` | shared, add-only | `pub mod cm_card;` |
| `crates/gui/src/ui/set_editor.rs` | modify | Two `cm_card::section` calls (2D right column, 3D next to CS) |
| `crates/gui/tests/cm_json.rs` | create | All GUI tests for the feature |
| `docs/DECISIONS.md` | modify (approved) | Note AT |
| `docs/superpowers/specs/2026-10-08-skyrim-dds-converter-design.md` | modify | §4.2 Vanilla `complex_material` cell (`:252`) |
| `docs/wiki/Presets-and-Targets.md`, `docs/wiki/Texture-Types.md` | modify | Vanilla rows that say "no complex `_m`" |

---

### Task 1: Engine: a varying-alpha `_m` is `complex_material` under Vanilla (D16)

**Files:**
- Modify: `crates/engine/src/policy.rs:1103`
- Modify: `crates/engine/src/suggest.rs:108-118` and the unit test at `:350-360`
- Modify: `crates/engine/tests/policy_checks.rs:289-310` (`availability_table`) and `:1094-1125` (inside `allow_cfg_unavailable_accepts_fallbacks_only`)
- Modify: `crates/engine/tests/complex_m_alpha.rs` (append)
- Modify: `crates/engine/tests/presets.rs` (append)
- Modify: `crates/engine/tests/golden.rs:490-581` and `:653-705` (the two approved tests only)
- Modify: `presets/vanilla-quality.toml:6`

**Interfaces:**
- Consumes: nothing new.
- Produces: `engine::policy::availability(Target::Vanilla, Subtype::ComplexMaterial) == Availability::Available`. Under Vanilla, a varying-alpha `_m` resolves to `complex_material` (BC7, `alpha.policy = data`, `alpha.parallax = true`). The GUI tasks rely on this (Task 4 predicate, Task 5 Vanilla export).

- [ ] **Step 1: Create the worktree**

```bash
cd /f/ai/dds/converter-nif-p1
git diff --stat bd495d2 nif/main    # expect only docs/superpowers/plans/2026-10-14-plan-pbr-grass.md and docs/superpowers/specs/2026-10-14-pbr-grass-design.md
git worktree add -b nif/l-cm /f/ai/dds/converter-nif-cm nif/main
cd /f/ai/dds/converter-nif-cm && git log --oneline -1
```

If the diff shows any code file, stop and ask the coordinator.

- [ ] **Step 2: Change the tests to the new behaviour**

In `crates/engine/src/suggest.rs`, in the `mod tests` assert at `:350-360`, change the expected sub-type and add the plain case after it:

```rust
        assert_eq!(
            top(&ctx(
                Target::Vanilla,
                Some("m"),
                "textures/a/x_m.png",
                &none,
                AlphaContent::Varying
            ))
            .subtype,
            Subtype::ComplexMaterial
        );
        assert_eq!(
            top(&ctx(
                Target::Vanilla,
                Some("m"),
                "textures/a/x_m.png",
                &none,
                AlphaContent::Constant255
            ))
            .subtype,
            Subtype::Envmask
        );
```

In `crates/engine/tests/policy_checks.rs::availability_table`, replace the Vanilla `ComplexMaterial` assert:

```rust
    assert_eq!(
        availability(Target::Vanilla, Subtype::ComplexMaterial),
        Availability::Available
    );
```

In `allow_cfg_unavailable_accepts_fallbacks_only`, replace the comment line `// complex_material under vanilla falls back to envmask, …` and the `let j = resolve_in(…); assert_eq!(…Envmask… LossAlphaData…);` block. Keep the `m`, `reg`, `overrides`, `allow_set` and `ctx` lines as they are:

```rust
    // complex_material is available under vanilla (Note AT, D16): no fallback, and its varying alpha is kept
```

```rust
    let j = resolve_in(&ctx, "a/x_m.png", Some("m"), &m);
    assert_eq!(
        (
            j.profile.subtype,
            j.profile.format,
            j.profile.alpha.policy,
            j.status.clone()
        ),
        (
            Subtype::ComplexMaterial,
            Format::Bc7,
            AlphaPolicy::Data,
            JobStatus::Ready
        ),
        "{:?}",
        j.warnings
    );
    assert!(!codes(&j).contains(&Code::CfgUnavailable), "{:?}", j.warnings);
```

Append to `crates/engine/tests/complex_m_alpha.rs`:

```rust
/// Note AT (D16): under vanilla a varying-alpha `_m` is a complex `_m` too: BC7 with its alpha as height, parallax on.
#[test]
fn vanilla_keeps_a_varying_alpha_m_as_complex_material() {
    let d = tempfile::tempdir().unwrap();
    put_rgba(d.path(), height);
    let o = convert(d.path(), Target::Vanilla, &[]);
    assert_eq!(o.summary.converted, 1, "{:?}", warnings(&o));
    let j = planned(&o, "iron_m.png");
    assert_eq!(j["profile"]["subtype"], "complex_material");
    assert_eq!(j["profile"]["format"], "bc7");
    assert_eq!(j["profile"]["alpha"]["policy"], "data");
    assert_eq!(j["profile"]["alpha"]["parallax"], true);
    let w = warnings(&o);
    assert!(
        !w.contains(&Code::LossAlphaData) && !w.contains(&Code::CfgUnavailable),
        "{w:?}"
    );
    let a = alphas(d.path());
    for (i, &v) in a[0].iter().enumerate() {
        let (x, y) = (i as u32 % W, i as u32 / W);
        assert!(
            (i32::from(v) - i32::from(height(x, y))).abs() <= 8,
            "{x},{y}: {v}"
        );
    }
}

/// Note AT: a plain `_m` (solid 255 or no alpha) stays a BC4 env mask under vanilla.
#[test]
fn vanilla_m_with_solid_or_no_alpha_stays_an_envmask() {
    let cases: [fn(&Path); 2] = [|d| put_rgba(d, |_, _| 255), put_rgb];
    for put in cases {
        let d = tempfile::tempdir().unwrap();
        put(d.path());
        let o = convert(d.path(), Target::Vanilla, &[]);
        assert_eq!(o.summary.converted, 1, "{:?}", warnings(&o));
        let j = planned(&o, "iron_m.png");
        assert_eq!(
            (
                j["profile"]["subtype"].as_str(),
                j["profile"]["format"].as_str()
            ),
            (Some("envmask"), Some("bc4"))
        );
    }
}

/// Note AT + AH1: an explicit `complex_material` without source alpha under vanilla writes alpha 0, not a fallback.
#[test]
fn vanilla_explicit_complex_material_without_alpha_writes_0() {
    let d = tempfile::tempdir().unwrap();
    put_rgb(d.path());
    let o = convert(d.path(), Target::Vanilla, &[SUB]);
    assert_eq!(o.summary.converted, 1, "{:?}", warnings(&o));
    assert!(!warnings(&o).contains(&Code::CfgUnavailable));
    assert_all_zero(d.path());
}
```

Append to `crates/engine/tests/presets.rs`:

```rust
/// Note AT: vanilla-compact halves a complex `_m` (BC7 with alpha), as cs-compact does.
#[test]
fn vanilla_compact_halves_a_complex_m() {
    let r = PresetRegistry::builtin().unwrap();
    let p = r
        .profile("vanilla-compact", Target::Vanilla, Subtype::ComplexMaterial)
        .unwrap();
    assert_eq!(
        (p.format, p.resize, p.alpha.policy),
        (Format::Bc7, ResizeMode::Half, AlphaPolicy::Data)
    );
}
```

In `crates/engine/tests/golden.rs`, replace `vanilla_fallbacks_are_written_with_allow_cfg_unavailable`, its doc comment included, with the version below. `iron_m` is no longer a fallback, so it leaves this test. The `LossAlphaData` entry stays in `allow` to keep the diff minimal.

```rust
/// Spec §4.2: with `--allow CFG-UNAVAILABLE` the offered fallback is resolved and written, with its own checks; the
/// acknowledged CFG-UNAVAILABLE stays as a warning. `complex_material` is available under vanilla since Note AT, so it
/// is no longer one of the fallbacks.
#[test]
fn vanilla_fallbacks_are_written_with_allow_cfg_unavailable() {
    let d = tempfile::tempdir().unwrap();
    put(d.path(), "textures/pbr/iron.png", rgb(64));
    put(
        d.path(),
        "textures/a/iron_n.png",
        rgba(64, |x, _| [128, 128, 255, (x * 4) as u8]),
    );
    put(d.path(), "textures/b/lamp_g.png", rgb(64));
    let sets = [
        "file:**/iron.png.subtype=pbr_albedo",
        "file:**/iron_n.png.subtype=pbr_normal",
        "file:**/lamp_g.png.subtype=pbr_emissive",
    ];
    // Without --allow every one of them is blocked and nothing is written.
    let o = convert(d.path(), Target::Vanilla, &sets, &[]);
    assert_eq!(
        (
            o.summary.converted,
            o.summary.skipped,
            o.summary.exit_code()
        ),
        (0, 3, 1),
        "{:?}",
        warnings(&o)
    );
    assert_eq!(warnings(&o), vec![Code::CfgUnavailable; 3]);
    assert!(!d.path().join("out/textures").exists());

    let allow = [Code::CfgUnavailable, Code::LossAlphaData];
    let o = convert(d.path(), Target::Vanilla, &sets, &allow);
    assert_eq!(
        (o.summary.converted, o.summary.exit_code()),
        (3, 0),
        "{:?}",
        warnings(&o)
    );
    for (src, from, to) in [
        ("iron.png", "pbr_albedo", "diffuse"),
        ("iron_n.png", "pbr_normal", "normal"),
        ("lamp_g.png", "pbr_emissive", "glow"),
    ] {
        let w = warning_of(&o, src, Code::CfgUnavailable);
        assert_eq!(w.severity, Severity::Warning, "{src}: {}", w.message);
        assert!(
            w.message.contains(from) && w.message.contains(to),
            "{src}: {}",
            w.message
        );
        assert_eq!(
            planned(&o, src)["provenance"]["subtype"],
            "fallback",
            "{src}"
        );
    }
    assert_profile(&o, "iron.png", "diffuse", "remove", false, "bc7");
    assert_profile(&o, "iron_n.png", "normal", "data", false, "bc7");
    assert_profile(&o, "lamp_g.png", "glow", "remove", false, "bc7");
    for rel in [
        "textures/pbr/iron.dds",
        "textures/a/iron_n.dds",
        "textures/b/lamp_g.dds",
    ] {
        assert_dds(
            &dds(d.path(), rel),
            DdsPixelFormat::Bc7 { srgb: false },
            HeaderKind::Dx10,
            7,
        );
    }
    // The normal fallback keeps the specular alpha that pbr_normal would have removed.
    assert_alpha_kept(
        &dds(d.path(), "textures/a/iron_n.dds"),
        |x, _| (x * 4) as u8,
        8,
    );
}
```

In `vanilla_g_is_glow_and_m_with_alpha_is_envmask_with_loss`, keep the name, because the owner approved the change under this name. Replace the doc comment, and replace everything from the second `let d = tempfile::tempdir().unwrap();` to the end of the function:

```rust
/// Spec §4.2/§4.3 item 3: under vanilla `_g` is `glow` (BC7 UNORM, even in a `pbr/` path). Since Note AT (D16) a
/// varying-alpha `_m` is `complex_material` under vanilla too: BC7 with its alpha, no LOSS-ALPHA-DATA. (The name
/// predates AT.)
```

```rust
    let d = tempfile::tempdir().unwrap();
    put(
        d.path(),
        "textures/a/iron_m.png",
        rgba(64, |x, _| [90, 90, 90, (x * 4) as u8]),
    );
    let o = convert(d.path(), Target::Vanilla, &[], &[]);
    assert_eq!(
        (
            o.summary.converted,
            o.summary.skipped,
            o.summary.exit_code()
        ),
        (1, 0, 0),
        "{:?}",
        warnings(&o)
    );
    assert_profile(&o, "iron_m.png", "complex_material", "data", true, "bc7");
    assert_eq!(
        planned(&o, "iron_m.png")["profile"]["alpha"]["parallax"],
        true
    );
    // Grey RGB (90, 90, 90): VAL-M-GRAYSCALE is not target-gated (Note AH). Nothing else is raised.
    assert_eq!(warnings(&o), vec![Code::ValMGrayscale]);
    assert_eq!(skip_code(&o, "iron_m.png"), None);
    assert_dds(
        &dds(d.path(), "textures/a/iron_m.dds"),
        DdsPixelFormat::Bc7 { srgb: false },
        HeaderKind::Dx10,
        7,
    );
}
```

If the warnings list is not exactly `[ValMGrayscale]`, stop and report the list to the coordinator. The owner approved the golden change only as described in §8. Do not adopt a different list on your own.

- [ ] **Step 3: Run the tests to verify they fail**

```bash
cd /f/ai/dds/converter-nif-cm && export CARGO_TARGET_DIR=/f/ai/dds/converter-nif-p1/target
cargo test -p engine --lib suggest
cargo test -p engine --test policy_checks -- availability_table allow_cfg_unavailable
cargo test -p engine --test complex_m_alpha vanilla_
cargo test -p engine --test golden vanilla_
cargo test -p engine --test presets vanilla_compact_halves_a_complex_m
```

Expected:
- The `suggest` tests and `availability_table` FAIL (Envmask / Unavailable).
- `allow_cfg_unavailable…` FAILS.
- `vanilla_keeps_a_varying_alpha_m_as_complex_material` and `vanilla_explicit_complex_material_without_alpha_writes_0` FAIL.
- Golden `vanilla_g_is_glow_and_m_with_alpha_is_envmask_with_loss` FAILS.
- These PASS already:
  - Golden `vanilla_fallbacks_are_written_with_allow_cfg_unavailable`. Without `iron_m` it no longer depends on D16; the edit only removes the row that the engine change would otherwise break.
  - `vanilla_m_with_solid_or_no_alpha_stays_an_envmask` and `vanilla_compact_halves_a_complex_m`. They pin the unchanged behaviour.

- [ ] **Step 4: The two deletions and the description**

In `crates/engine/src/policy.rs`, delete this line in `availability`:

```rust
        (Target::Vanilla, ComplexMaterial) => un(Some(Envmask)),
```

In `crates/engine/src/suggest.rs`, the `_m` arm becomes:

```rust
        Some("m") | Some("em") | Some("envmask") => {
            if c.alpha == AlphaContent::Varying {
                vec![
                    s(ComplexMaterial, High, "_m with non-white alpha"),
                    s(Envmask, Low, "_m"),
                ]
            } else {
                vec![s(Envmask, High, "_m with no or all-255 alpha")]
            }
        }
```

`Target` stays imported, because `:44` and the tests use it.

In `presets/vanilla-quality.toml:6`:

```toml
description = "Vanilla – Quality: Bethesda shaders; no _SRGB, no BC6H."
```

- [ ] **Step 5: Run the engine tests to verify they pass**

```bash
cargo test -p engine --lib suggest
cargo test -p engine --test policy_checks --test complex_m_alpha --test golden --test presets --test allowed_encodings --test shaders --test suffix_vocabulary --test packing
```

Expected: PASS.

- [ ] **Step 6: Regenerate `schema/` and check for leftover Vanilla `_m` assumptions**

```bash
cargo run -p sdc -- presets schema all --out schema
git status --short schema/        # expect nothing
cargo test -p engine --test schemas
grep -rn "complex_material\|ComplexMaterial" crates/gui/tests crates/cli/tests | grep -i vanilla    # expect nothing
cargo test -p engine
cargo test -p sdc
cargo test -p sdc-gui -- --skip render_
cargo clippy -p engine --all-targets -- -D warnings
cargo fmt --all -- --check
```

Expected:
- `schema/` has no diff.
- All tests PASS. The CLI `validate` tests (`cli_inspect_validate.rs:767`, `:798-810`) are unaffected, because their inputs are near-opaque or CS.

If any other test pins the old Vanilla `_m` result, stop and report it. Only the two named golden tests are approved.

- [ ] **Step 7: Commit**

```bash
git add crates/engine/src/policy.rs crates/engine/src/suggest.rs crates/engine/tests/policy_checks.rs crates/engine/tests/complex_m_alpha.rs crates/engine/tests/presets.rs crates/engine/tests/golden.rs presets/vanilla-quality.toml
git commit -m "$(cat <<'EOF'
feat(engine): a varying-alpha _m is complex_material under vanilla (D16)

Drop the vanilla complex_material -> envmask fallback and the "_m under
vanilla" suggestion: vanilla now uses the CS rule (Note L varying alpha
-> complex_material BC7 with alpha, else envmask BC4). Golden tests
vanilla_fallbacks_are_written_with_allow_cfg_unavailable and
vanilla_g_is_glow_and_m_with_alpha_is_envmask_with_loss updated as
approved (spec section 8).

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
EOF
)"
```

---

### Task 2: Vendored complex material schema and `validate_cm`

**Files:**
- Create: `crates/gui/assets/complex_material.schema.json` (a copy, byte for byte)
- Modify: `crates/gui/src/pgp/validate.rs:1-6, 189-198`
- Modify: `crates/gui/tests/pgp.rs:917-938` (keyword guard) and a new test after it

**Interfaces:**
- Consumes: the private `check(root, s, v, at, errs)` in `validate.rs`.
- Produces:
  - `pub const sdc_gui::pgp::validate::CM_SCHEMA_JSON: &str`
  - `pub fn sdc_gui::pgp::validate::validate_cm(doc: &serde_json::Value) -> Result<(), Vec<String>>`

- [ ] **Step 1: Verify that `pbr.schema.json` is already 2.2.0 (D13). Verify only, vendor nothing.**

```bash
cd /f/ai/dds/converter-nif-cm
tr -d '\r' < crates/gui/assets/pbr.schema.json | sha256sum     # expect 4bdc79204d0fb526197b18d3e6b47432ba884e338b9573c509c4739a12fa956a
grep -c '"alphaprop_' crates/gui/assets/pbr.schema.json         # expect 4 or more
```

- [ ] **Step 2: Vendor the CM schema**

```bash
cp /f/ai/dds/nifparseref/plan/complex-material/complex_material.schema.json crates/gui/assets/complex_material.schema.json
sha256sum crates/gui/assets/complex_material.schema.json        # expect 001f12add94f103fa6bb1fe5512b22af08587f8d9f633e2b2900507d9a553451
grep -c $'\r' crates/gui/assets/complex_material.schema.json     # expect 0 (LF; .gitattributes pins crates/gui/assets/*.json to eol=lf)
```

- [ ] **Step 3: Write the failing tests**

In `crates/gui/tests/pgp.rs`, replace `the_validator_covers_every_keyword_the_schema_uses` with:

```rust
#[test]
fn the_validator_covers_every_keyword_the_schema_uses() {
    for (name, text) in [
        ("pbr.schema.json", sdc_gui::pgp::schema::SCHEMA_JSON),
        ("complex_material.schema.json", CM_SCHEMA_JSON),
    ] {
        let schema: Value = serde_json::from_str(text).unwrap();
        let mut used = std::collections::BTreeSet::new();
        keywords_in(&schema, &mut used, false);
        let annotations = [
            "$schema",
            "$id",
            "title",
            "description",
            "examples",
            "default",
            "definitions",
            "properties",
        ];
        for k in used {
            assert!(
                KEYWORDS.contains(&k.as_str()) || annotations.contains(&k.as_str()),
                "{name} uses '{k}', which validate.rs does not implement (DECISIONS P12)"
            );
        }
    }
}

/// Complex material design §7: both schema examples pass; an unknown key, a short colour and a non-number fail.
#[test]
fn validate_cm_accepts_the_schema_examples_and_rejects_bad_documents() {
    let schema: Value = serde_json::from_str(CM_SCHEMA_JSON).unwrap();
    let examples = schema["examples"].as_array().unwrap();
    assert_eq!(examples.len(), 2);
    for ex in examples {
        assert_eq!(validate_cm(ex), Ok(()), "{ex}");
    }
    for (bad, why) in [
        (json!({"specular": 1.0}), "unknown key 'specular'"),
        (json!({"specular_color": [1.0, 1.0]}), "needs at least 3 items"),
        (json!({"specular_color": [1.0, "1", 1.0]}), "specular_color[1]: expected"),
        (json!({"glossiness": "80"}), "glossiness: expected"),
    ] {
        let e = validate_cm(&bad).unwrap_err();
        assert!(e.iter().any(|x| x.contains(why)), "{bad}: {e:?}");
    }
}
```

- [ ] **Step 4: Run the tests to verify they fail**

```bash
export CARGO_TARGET_DIR=/f/ai/dds/converter-nif-p1/target
cargo test -p sdc-gui --test pgp -- validator validate_cm
```

Expected: a compile error, "cannot find value `CM_SCHEMA_JSON`" / "`validate_cm`".

- [ ] **Step 5: Implement**

In `crates/gui/src/pgp/validate.rs`, replace the module doc and the imports at `:1-6`:

```rust
//! A draft-07 subset validator for the vendored PGPatcher schemas (DECISIONS P12): exactly the keywords those schemas
//! use, `pbr.schema.json` and `complex_material.schema.json`. A test fails if either starts using another one.
//! Validation is a safety net that should never fire: the writers build documents from the same key tables.
use super::schema::SCHEMA_JSON;
use serde_json::Value;
use std::sync::OnceLock;

/// PGPatcher's complex material schema (upstream `0176567`, identical at 2.2.0), vendored byte for byte.
pub const CM_SCHEMA_JSON: &str = include_str!("../../assets/complex_material.schema.json");
```

Replace `pub fn validate` at `:189-198` with:

```rust
fn cm_schema() -> &'static Value {
    static S: OnceLock<Value> = OnceLock::new();
    S.get_or_init(|| {
        serde_json::from_str(CM_SCHEMA_JSON).expect("the vendored complex material schema parses")
    })
}

fn run(root: &Value, doc: &Value) -> Result<(), Vec<String>> {
    let mut errs = vec![];
    check(root, root, doc, "$", &mut errs);
    if errs.is_empty() {
        Ok(())
    } else {
        Err(errs)
    }
}

pub fn validate(doc: &Value) -> Result<(), Vec<String>> {
    run(schema(), doc)
}

/// A PGPatcher complex material JSON (`<texture>_m.json`) against its vendored schema.
pub fn validate_cm(doc: &Value) -> Result<(), Vec<String>> {
    run(cm_schema(), doc)
}
```

- [ ] **Step 6: Run the tests to verify they pass**

```bash
cargo test -p sdc-gui --test pgp
cargo clippy -p sdc-gui --all-targets -- -D warnings
```

Expected: PASS. Every existing `validate` test passes unchanged.

- [ ] **Step 7: Commit**

```bash
git add crates/gui/assets/complex_material.schema.json crates/gui/src/pgp/validate.rs crates/gui/tests/pgp.rs
git commit -m "$(cat <<'EOF'
feat(gui): vendor PGPatcher's complex material schema and validate_cm

Byte-for-byte copy of upstream 0176567 (sha256 001f12ad..., same at
2.2.0). The keyword guard now walks both schemas. pbr.schema.json was
checked: already 2.2.0 (AG27).

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
EOF
)"
```

---

### Task 3: `[sets.cm]` in `sdc-project.toml`: block, setters, hash record, undo

**Files:**
- Create: `crates/gui/src/model/cm_json.rs` (the key table, `CmBlock`, the parser, the `AppModel` setters)
- Modify: `crates/gui/src/sets_doc.rs:4, 18-34, 52-57, 172-176, 246-268, 329-349`, and new setters after `set_cs_value` (`:822`)
- Modify (shared, own commit): `crates/gui/src/model/mod.rs:4` (one line `pub mod cm_json;`)
- Create: `crates/gui/tests/cm_json.rs`

**Interfaces:**
- Consumes:
  - `crate::model::cs_pbr::{valid_value, FieldKind}`, `crate::project::{sub_table, toml_to_json}`, `SetsDoc::{table_mut, entries}`, `write_path`
  - `AppModel::{edit_docs(label, merge, replan, f), sets_editable()}`
- Produces:
  - `pub enum CmKind { Bool, Num, Rgb }`; `pub struct CmKey { pub name: &'static str, pub kind: CmKind, pub hint: &'static str }`; `pub const KEYS: [CmKey; 6]`
  - `pub fn key(name: &str) -> Option<&'static CmKey>`; `pub fn valid(kind: CmKind, v: &Json) -> bool`; `pub fn first_value(k: &CmKey) -> Json`
  - `pub struct CmBlock { pub export: bool, pub values: Map<String, Json>, pub bad: Vec<String> }` (Debug, Clone, PartialEq, Default); `pub(crate) fn block_of(t: &dyn TableLike) -> CmBlock`
  - `SetEntry.cm: Option<CmBlock>`
  - `SetsDoc::{set_cm_value(&mut self, id: &str, key: &str, v: Option<Json>), set_cm_export(&mut self, id: &str, on: bool), cm_json_shas(&self) -> BTreeMap<String, String>, set_cm_json_shas(&mut self, &BTreeMap<String, String>)}`
  - `AppModel::{cm_block(&self, set: &str) -> Option<CmBlock>, set_cm_value(&mut self, set: &str, key: &str, v: Option<Json>), set_cm_export(&mut self, set: &str, on: bool)}`

- [ ] **Step 1: Write the failing tests**

Create `crates/gui/tests/cm_json.rs`:

```rust
//! PGPatcher complex material JSON (complex material design 2026-10-14): the `[sets.cm]` block, the writer, the card
//! predicate, the export and the card's warnings.
mod support;
use sdc_gui::sets_doc::SetsDoc;
use serde_json::json;
use std::collections::BTreeMap;

fn doc(text: &str) -> (tempfile::TempDir, SetsDoc) {
    let d = tempfile::tempdir().unwrap();
    std::fs::write(d.path().join("sdc-project.toml"), text).unwrap();
    let s = SetsDoc::open(d.path());
    (d, s)
}

const DOC: &str = "[[sets]]\nid = \"a/one\"\nhair = true\n\n[sets.cm]\n# mine\nexport = true\nglossiness = 80\n\
specular_color = [1, 0.85, 0.6]\nspecular_strength = \"high\"\nmyNote = \"keep\"\n\n[[sets]]\nid = \"b/two\"\nhair = true\n";

#[test]
fn a_cm_block_reads_back_with_unknown_and_wrong_typed_keys_and_never_reaches_the_settings() {
    let (_d, s) = doc(DOC);
    let e = s.entries();
    let b = e[0].cm.as_ref().expect("the block");
    assert!(b.export);
    assert_eq!(b.values["glossiness"], json!(80));
    assert_eq!(b.values["specular_color"], json!([1, 0.85, 0.6]));
    assert_eq!(b.bad, ["specular_strength"]);
    assert!(!b.values.contains_key("myNote"));
    // RESERVED: nothing of the block is a PGPatcher setting.
    assert_eq!(e[0].settings.keys().collect::<Vec<_>>(), ["hair"]);
    assert!(e[1].cm.is_none());
}

/// Review focus 1.
#[test]
fn a_wrong_typed_export_reads_as_off_and_is_listed() {
    let (_d, s) =
        doc("[[sets]]\nid = \"a/one\"\n\n[sets.cm]\nexport = \"yes\"\nspecular_color = [1, 1]\n");
    let b = s.entries()[0].cm.clone().unwrap();
    assert!(!b.export);
    assert_eq!(b.bad, ["export", "specular_color"]);
}

/// Edits keep comments and unknown keys, and a new `[sets.cm]` is printed under its own set.
#[test]
fn cm_edits_keep_the_file_and_land_in_their_own_set() {
    let (d, mut s) = doc(DOC);
    s.set_cm_value("b/two", "dynamic_cubemap", Some(json!(false)));
    s.set_cm_value("a/one", "glossiness", None);
    s.set_cm_export("a/one", false);
    s.save(false).unwrap();
    let text = std::fs::read_to_string(d.path().join("sdc-project.toml")).unwrap();
    for keep in ["# mine", "myNote = \"keep\"", "specular_strength = \"high\""] {
        assert!(text.contains(keep), "{keep} in {text}");
    }
    let s = SetsDoc::open(d.path());
    let e = s.entries();
    let a = e[0].cm.clone().unwrap();
    assert!(!a.export, "unticked");
    assert!(a.values.contains_key("specular_color"), "values are kept");
    assert!(!a.values.contains_key("glossiness"));
    assert_eq!(e[1].cm.clone().unwrap().values["dynamic_cubemap"], json!(false));
}

#[test]
fn removing_a_key_from_a_set_without_a_block_changes_nothing() {
    let (_d, mut s) = doc("[[sets]]\nid = \"b/two\"\nhair = true\n");
    s.set_cm_value("b/two", "glossiness", None);
    assert!(!s.is_dirty());
    assert!(s.entries()[0].cm.is_none());
}

/// Review focus 4.
#[test]
fn a_read_only_document_takes_no_cm_edit() {
    let (_d, mut s) = doc("[[sets]\nbroken");
    assert!(s.read_only());
    s.set_cm_value("a/one", "glossiness", Some(json!(1.0)));
    s.set_cm_export("a/one", true);
    s.set_cm_json_shas(&BTreeMap::from([("x.json".to_string(), "00".to_string())]));
    assert!(!s.is_dirty());
    assert!(s.cm_json_shas().is_empty());
}

/// Design §3: the hash record reads back, and an undo (restore) keeps the live record.
#[test]
fn the_hash_record_round_trips_and_survives_undo() {
    let (_d, mut s) = doc(DOC);
    let before = s.text();
    s.set_cm_value("a/one", "glossiness", Some(json!(50.0)));
    let shas = BTreeMap::from([("m/iron_m.json".to_string(), "ab12".to_string())]);
    s.set_cm_json_shas(&shas);
    assert_eq!(s.cm_json_shas(), shas);
    assert!(s.text().contains("cm_json_sha256"));
    s.restore_keeping_json_sha(&before);
    assert_eq!(s.cm_json_shas(), shas, "undo keeps the record");
    assert_eq!(
        s.entries()[0].cm.clone().unwrap().values["glossiness"],
        json!(80),
        "undo took the value back"
    );
    s.set_cm_json_shas(&BTreeMap::new());
    assert!(!s.text().contains("cm_json_sha256"));
}

/// Design §3 RESERVED leak: through the model, `cm` keys never reach the TruePBR JSON or `shows_json_panel`.
#[test]
fn model_edits_are_undoable_and_never_reach_the_pgpatcher_json() {
    use sdc_gui::model::pbr_export::json_entries;
    use sdc_gui::model::sets::{set_views, shows_json_panel};
    use sdc_gui::pgp::writer::{document, to_text};
    let d = tempfile::tempdir().unwrap();
    let mut m = support::pbr_opened(d.path());
    for id in ["landscape/rock01", "landscape/leaf"] {
        m.set_cm_export(id, true);
        m.set_cm_value(id, "glossiness", Some(json!(80.0)));
    }
    let entries: Vec<_> = json_entries(&m).into_iter().map(|(_, e)| e).collect();
    let text = to_text(&document(&entries));
    for leak in ["glossiness", "\"cm", "export"] {
        assert!(!text.contains(leak), "{leak} in {text}");
    }
    let leaf = set_views(&m)
        .into_iter()
        .find(|v| v.id == "landscape/leaf")
        .unwrap();
    assert!(leaf.settings.is_empty());
    assert!(!shows_json_panel(&leaf, m.target));
    assert_eq!(
        m.cm_block("landscape/leaf").unwrap().values["glossiness"],
        json!(80.0)
    );
    m.undo();
    assert!(m.wait_idle(std::time::Duration::from_secs(60)));
    assert!(
        !m.cm_block("landscape/leaf")
            .unwrap()
            .values
            .contains_key("glossiness"),
        "one step undone"
    );
}
```

- [ ] **Step 2: Run the tests to verify they fail**

```bash
cd /f/ai/dds/converter-nif-cm && export CARGO_TARGET_DIR=/f/ai/dds/converter-nif-p1/target
cargo test -p sdc-gui --test cm_json
```

Expected: a compile error (no field `cm`, no method `set_cm_value`).

- [ ] **Step 3: Create `crates/gui/src/model/cm_json.rs` (first part)**

```rust
//! PGPatcher complex material JSON (complex material design 2026-10-14): the `[sets.cm]` block, which members get
//! `<texture>.json`, its text, the export hooks and the card's warnings. No egui types.
use super::cs_pbr::{valid_value, FieldKind};
use serde_json::{json, Map, Value as Json};
use toml_edit::TableLike;

/// The type PGPatcher reads for a key (`complex_material.schema.json`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CmKind {
    Bool,
    Num,
    Rgb,
}

/// One schema key: its type and the card's hint while it is unset (design §5).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CmKey {
    pub name: &'static str,
    pub kind: CmKind,
    pub hint: &'static str,
}

const fn k(name: &'static str, kind: CmKind, hint: &'static str) -> CmKey {
    CmKey { name, kind, hint }
}

/// The schema's keys in schema order. `$schema` is written by the writer, never stored.
pub const KEYS: [CmKey; 6] = [
    k(
        "specular_enabled",
        CmKind::Bool,
        "PGPatcher: on if the _m has gloss (G), else the mesh's",
    ),
    k("specular_color", CmKind::Rgb, "PGPatcher: 1, 1, 1"),
    k("specular_strength", CmKind::Num, "PGPatcher: 1"),
    k(
        "glossiness",
        CmKind::Num,
        "PGPatcher: keeps the mesh's value",
    ),
    k("environment_map_scale", CmKind::Num, "PGPatcher: 1"),
    k(
        "dynamic_cubemap",
        CmKind::Bool,
        "PGPatcher: on (dynamic cubemap)",
    ),
];

pub fn key(name: &str) -> Option<&'static CmKey> {
    KEYS.iter().find(|k| k.name == name)
}

/// Whether `v` has the type PGPatcher reads. A colour is exactly three numbers: 1.2.0 crashes on anything else.
pub fn valid(kind: CmKind, v: &Json) -> bool {
    match kind {
        CmKind::Bool => v.is_boolean(),
        CmKind::Num => valid_value(FieldKind::Num, v),
        CmKind::Rgb => valid_value(FieldKind::Rgb, v),
    }
}

/// The value a key takes when its box is ticked (design §5). Glossiness 80 is the schema example, never a mesh's (D10).
pub fn first_value(k: &CmKey) -> Json {
    match k.kind {
        CmKind::Bool => json!(true),
        CmKind::Rgb => json!([1.0, 1.0, 1.0]),
        CmKind::Num if k.name == "glossiness" => json!(80.0),
        CmKind::Num => json!(1.0),
    }
}

/// `[sets.cm]` as read (design §3).
#[derive(Debug, Clone, PartialEq, Default)]
pub struct CmBlock {
    /// Write the JSON on export. Absent, `false` or not a bool is off.
    pub export: bool,
    /// Set keys with a usable value, as read.
    pub values: Map<String, Json>,
    /// Known keys (`export` included) with the wrong type: kept in the file, warned about, not exported.
    pub bad: Vec<String>,
}

/// Reads a `[sets.cm]` table. Unknown keys stay in the file and are never exported (the schema forbids them).
pub(crate) fn block_of(t: &dyn TableLike) -> CmBlock {
    let mut b = CmBlock::default();
    for (name, item) in t.iter() {
        let v = crate::project::toml_to_json(item);
        if name == "export" {
            match v {
                Some(Json::Bool(on)) => b.export = on,
                _ => b.bad.push(name.to_string()),
            }
        } else if let Some(k) = key(name) {
            match v.filter(|v| valid(k.kind, v)) {
                Some(v) => {
                    b.values.insert(name.to_string(), v);
                }
                None => b.bad.push(name.to_string()),
            }
        }
    }
    b
}

use super::AppModel;
use crate::sets_doc::SetsDoc;
use engine::sets::same_id;

impl AppModel {
    /// Set `set`'s `[sets.cm]` block, as the document holds it.
    pub fn cm_block(&self, set: &str) -> Option<CmBlock> {
        self.sets_doc
            .as_ref()?
            .entries()
            .into_iter()
            .find(|e| same_id(&e.id, set))?
            .cm
    }

    /// One undoable edit of `[sets.cm]`. Nothing is planned: the block never changes a job.
    fn cm_edit(
        &mut self,
        set: &str,
        label: &str,
        merge: Option<String>,
        f: impl FnOnce(&mut SetsDoc, &str),
    ) {
        if !self.sets_editable() {
            return;
        }
        let s = set.to_string();
        self.edit_docs(label, merge, false, move |_, d| f(d, &s));
    }

    /// Set (or with `None` remove) key `key` of `[sets.cm]`. Drags of one key merge into one undo step.
    pub fn set_cm_value(&mut self, set: &str, key: &str, v: Option<Json>) {
        let merge = Some(format!("{set}:cm:{key}"));
        let k = key.to_string();
        self.cm_edit(set, &format!("Change {key}"), merge, move |d, s| {
            d.set_cm_value(s, &k, v)
        });
    }

    pub fn set_cm_export(&mut self, set: &str, on: bool) {
        let label = if on {
            "Export complex material JSON"
        } else {
            "Stop exporting complex material JSON"
        };
        self.cm_edit(set, label, None, move |d, s| d.set_cm_export(s, on));
    }
}
```

- [ ] **Step 4: `sets_doc.rs`**

Imports (`:4`), next to the CS import:

```rust
use crate::model::cm_json::{block_of as cm_block_of, CmBlock};
```

Constants (`:18-34`): add `CM_SHA` after `CS_SHA` and grow `RESERVED`:

```rust
/// Complex material design §3: `[project] cm_json_sha256`, path under the export root → hash of the
/// `<texture>.json` sdc last wrote there.
const CM_SHA: &str = "cm_json_sha256";
/// Keys of a `[[sets]]` table that are not JSON settings.
const RESERVED: [&str; 13] = [
    "id",
    "created",
    "renamed_from",
    "add",
    "remove",
    "borrow",
    "shader",
    "lock",
    "output_name",
    "cs_txst",
    "cs_mato",
    "skin",
    "cm",
];
```

In `SetEntry`, after `cs_mato`:

```rust
    /// Complex material design §3: the PGPatcher complex material JSON block (`[sets.cm]`).
    pub cm: Option<CmBlock>,
```

In `entry_of`'s `Some(SetEntry { … })`, after `cs_mato,`:

```rust
        cm: t.get("cm").and_then(Item::as_table_like).map(cm_block_of),
```

Replace `cs_pbr_shas` and `set_cs_pbr_shas` (`:329-349`) with a shared pair plus the four public accessors:

```rust
    fn shas(&self, key: &str) -> BTreeMap<String, String> {
        self.doc
            .get("project")
            .and_then(|p| p.get(key))
            .and_then(Item::as_table_like)
            .map(|t| {
                t.iter()
                    .filter_map(|(k, v)| Some((k.to_string(), v.as_str()?.to_string())))
                    .collect()
            })
            .unwrap_or_default()
    }

    fn set_shas(&mut self, key: &str, shas: &BTreeMap<String, String>) {
        self.rev = next_rev();
        let mut t = toml_edit::InlineTable::new();
        for (k, v) in shas {
            t.insert(k, Value::from(v.as_str()));
        }
        self.set_project(key, (!shas.is_empty()).then_some(Value::InlineTable(t)));
    }

    pub fn cs_pbr_shas(&self) -> BTreeMap<String, String> {
        self.shas(CS_SHA)
    }

    pub fn set_cs_pbr_shas(&mut self, shas: &BTreeMap<String, String>) {
        self.set_shas(CS_SHA, shas)
    }

    pub fn cm_json_shas(&self) -> BTreeMap<String, String> {
        self.shas(CM_SHA)
    }

    pub fn set_cm_json_shas(&mut self, shas: &BTreeMap<String, String>) {
        self.set_shas(CM_SHA, shas)
    }
```

In `restore_keeping_json_sha` (`:246-268`), update the doc comment to say "`last_json_sha256`, `cs_pbr_sha256` and `cm_json_sha256`". Then add the third record beside the CS one:

```rust
        let cs = self.cs_pbr_shas();
        let cm = self.cm_json_shas();
        let old = self.text();
        self.restore(text);
        if self.last_json_sha256() != sha {
            self.set_project("last_json_sha256", sha.map(Value::from));
        }
        if self.cs_pbr_shas() != cs {
            self.set_cs_pbr_shas(&cs);
        }
        if self.cm_json_shas() != cm {
            self.set_cm_json_shas(&cm);
        }
```

After `set_cs_value` (`:822`):

```rust
    /// Complex material design §3: set (or with `None` remove) key `key` of set `id`'s `[sets.cm]`, creating the block
    /// when missing. Keys are written as given; `cm_json::block_of` decides what is used.
    pub fn set_cm_value(&mut self, id: &str, key: &str, v: Option<Json>) {
        self.rev = next_rev();
        if self.read_only() {
            return;
        }
        // Removing from a set with no block changes nothing (no table, no dirty, no undo step).
        if v.is_none()
            && !self
                .entries()
                .iter()
                .any(|e| same_id(&e.id, id) && e.cm.is_some())
        {
            return;
        }
        let t = self.table_mut(id);
        let Some(b) = sub_table(t, "cm", || Item::Table(Table::new())) else {
            return;
        };
        if write_path(b, key, v) {
            self.dirty = true;
        }
    }

    /// Tick or untick the export (D3). Unticking writes `export = false` and keeps the values, so ticking again
    /// restores them.
    pub fn set_cm_export(&mut self, id: &str, on: bool) {
        self.set_cm_value(id, "export", Some(Json::Bool(on)));
    }
```

`set_value`'s TXST redirect touches only `cs_txst`, so it cannot reach `cm`. Leave it unchanged.

- [ ] **Step 5: Register the module**

In `crates/gui/src/model/mod.rs`, add one line after `pub mod bulk;`:

```rust
pub mod cm_json;
```

- [ ] **Step 6: Run the tests to verify they pass**

```bash
cargo test -p sdc-gui --test cm_json --test cs_pbr --test cs_pbr_export --test sets_doc --test history
cargo clippy -p sdc-gui --all-targets -- -D warnings
cargo fmt --all -- --check
```

Expected: PASS.

- [ ] **Step 7: Commit, in two commits (shared file alone)**

```bash
git add crates/gui/src/model/cm_json.rs crates/gui/src/sets_doc.rs crates/gui/tests/cm_json.rs
git commit -m "$(cat <<'EOF'
feat(gui): [sets.cm] complex material block, setters and hash record

RESERVED gains cm, so the block never reaches the TruePBR settings.
cm_json_sha256 shares the CS hash-map code and survives undo.

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
EOF
)"
git add crates/gui/src/model/mod.rs
git commit -m "$(cat <<'EOF'
chore(gui): register model::cm_json

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
EOF
)"
```

Note: the first commit does not build on its own, because the module is registered in the second. That is acceptable under the shared-file rule. If the reviewer wants every commit to build, swap the order: commit `mod.rs` together with an empty `cm_json.rs` first.

---

### Task 4: Writer, member predicate and targets

**Files:**
- Modify: `crates/gui/src/model/cm_json.rs` (append)
- Modify: `crates/gui/tests/cm_json.rs` (append)

**Interfaces:**
- Consumes:
  - Task 3's `KEYS`, `key`, `valid`, `first_value`, `CmBlock`, `AppModel::cm_block`
  - `crate::model::cs_pbr::floats`, `crate::model::sets::{views, SetView}`, `crate::pgp::validate::validate_cm` (Task 2)
  - Task 1's Vanilla engine behaviour
- Produces:
  - `pub const SCHEMA_URL: &str`
  - `pub fn to_text(values: &Map<String, Json>) -> String`
  - `pub fn cm_members(m: &AppModel, v: &SetView) -> Vec<String>` (item keys)
  - `pub fn shows_card(m: &AppModel, v: &SetView) -> bool`
  - `pub struct CmTarget { pub set: String, pub key: String, pub name: String, pub rel: Result<String, String> }`
  - `pub fn targets(m: &AppModel) -> Vec<CmTarget>`
  - `pub fn clashes<'a>(rels: impl Iterator<Item = &'a str>) -> BTreeSet<String>`

- [ ] **Step 1: Write the failing tests**

Append to `crates/gui/tests/cm_json.rs`:

```rust
use engine::config::{Subtype, Target};
use engine::shader::Shader;
use image::{ImageBuffer, Luma, Rgb, Rgba};
use sdc_gui::model::cm_json::*;
use sdc_gui::model::sets::{set_views, SetView};
use sdc_gui::model::AppModel;
use sdc_gui::pgp::validate::validate_cm;
use sdc_gui::settings::GuiSettings;
use serde_json::{Map, Value as Json};
use std::path::Path;
use std::time::Duration;

const IRON_M: &str = "m/iron_m.png";
const BOLT_M: &str = "m/bolt_m.png";

fn obj(v: Json) -> Map<String, Json> {
    v.as_object().unwrap().clone()
}

fn lines(l: &[&str]) -> String {
    l.join("\n")
}

fn idle(m: &mut AppModel) {
    assert!(
        m.wait_idle(Duration::from_secs(60)),
        "planning did not finish"
    );
}

/// Opens `<dir>/src` (exports under `<dir>/export`), High suggestions accepted, under `target`.
fn open(dir: &Path, target: Target) -> AppModel {
    let settings = GuiSettings {
        export_folder: Some(dir.join("export")),
        ..GuiSettings::default()
    };
    let mut m = AppModel::new(settings, None, support::session());
    m.accept = engine::policy::AcceptSuggestions::High;
    m.open_folder(dir.join("src"));
    idle(&mut m);
    if target != Target::Cs {
        m.set_target(target);
        idle(&mut m);
    }
    m
}

/// `iron`: diffuse + `_m` with a varying (height) alpha. `bolt`: diffuse + RGB-only `_m`. `axe`: diffuse + `_em` with
/// a varying alpha. Colourful RGB, so no VAL-M-GRAYSCALE.
fn fixture(dir: &Path, target: Target) -> AppModel {
    let r = dir.join("src/m");
    std::fs::create_dir_all(&r).unwrap();
    let rgb = |x: u32, y: u32| [(x * 4) as u8, (y * 4) as u8, 90u8];
    for base in ["iron", "bolt", "axe"] {
        ImageBuffer::from_fn(64, 64, |x, y| Rgb(rgb(x, y)))
            .save(r.join(format!("{base}.png")))
            .unwrap();
    }
    for name in ["iron_m.png", "axe_em.png"] {
        ImageBuffer::from_fn(64, 64, |x, y| {
            let [a, b, c] = rgb(x, y);
            Rgba([a, b, c, (40 + x * 2) as u8])
        })
        .save(r.join(name))
        .unwrap();
    }
    ImageBuffer::from_fn(64, 64, |x, y| Rgb(rgb(x, y)))
        .save(r.join("bolt_m.png"))
        .unwrap();
    open(dir, target)
}

fn view(m: &AppModel, key_end: &str) -> SetView {
    set_views(m)
        .into_iter()
        .find(|v| v.key.ends_with(key_end))
        .unwrap_or_else(|| panic!("no set {key_end}"))
}

fn set_id(m: &AppModel, key_end: &str) -> String {
    view(m, key_end).id
}

fn ok(name: &str, rel: &str) -> (String, Result<String, String>) {
    (name.to_string(), Ok(rel.to_string()))
}

fn rels(m: &AppModel) -> Vec<(String, Result<String, String>)> {
    targets(m).into_iter().map(|t| (t.name, t.rel)).collect()
}

/// Design §9: the golden text of both schema examples: `$schema` first, floats, 2-space indent, final newline.
#[test]
fn the_schema_examples_are_written_with_schema_first_floats_and_a_newline() {
    let schema = format!("  \"$schema\": \"{SCHEMA_URL}\",");
    let ex2 = obj(json!({"specular_enabled": true, "specular_color": [1, 0.85, 0.6],
                         "specular_strength": 1.5, "glossiness": 80}));
    assert_eq!(
        to_text(&ex2),
        lines(&[
            "{",
            &schema,
            "  \"glossiness\": 80.0,",
            "  \"specular_color\": [",
            "    1.0,",
            "    0.85,",
            "    0.6",
            "  ],",
            "  \"specular_enabled\": true,",
            "  \"specular_strength\": 1.5",
            "}",
            "",
        ])
    );
    let ex1 = obj(json!({"environment_map_scale": 0.5, "dynamic_cubemap": false}));
    assert_eq!(
        to_text(&ex1),
        lines(&[
            "{",
            &schema,
            "  \"dynamic_cubemap\": false,",
            "  \"environment_map_scale\": 0.5",
            "}",
            "",
        ])
    );
    // Zero keys: the file still exists, `$schema` only (it restates PGPatcher's defaults).
    assert_eq!(
        to_text(&Map::new()),
        format!("{{\n  \"$schema\": \"{SCHEMA_URL}\"\n}}\n")
    );
}

/// Review focus 2: only set, well-typed, known keys; ints become floats; colours above 1 stay.
#[test]
fn to_text_keeps_only_well_typed_keys_as_floats() {
    let v = obj(json!({"glossiness": "high", "specular_color": [2, 3.5, 1], "foo": 1, "specular_strength": 1}));
    assert_eq!(
        to_text(&v),
        lines(&[
            "{",
            &format!("  \"$schema\": \"{SCHEMA_URL}\","),
            "  \"specular_color\": [",
            "    2.0,",
            "    3.5,",
            "    1.0",
            "  ],",
            "  \"specular_strength\": 1.0",
            "}",
            "",
        ])
    );
}

#[test]
fn every_written_text_passes_the_schema() {
    let mut all = Map::new();
    for k in &KEYS {
        let one = Map::from_iter([(k.name.to_string(), first_value(k))]);
        let t = to_text(&one);
        assert_eq!(
            validate_cm(&serde_json::from_str(&t).unwrap()),
            Ok(()),
            "{t}"
        );
        all.insert(k.name.to_string(), first_value(k));
    }
    let t = to_text(&all);
    assert_eq!(validate_cm(&serde_json::from_str(&t).unwrap()), Ok(()), "{t}");
}

#[test]
fn two_members_writing_one_file_clash_whatever_the_case() {
    let c = clashes(["m/a_m.json", "M/A_m.JSON", "m/b_m.json"].into_iter());
    assert_eq!(c.into_iter().collect::<Vec<_>>(), ["m/a_m.json"]);
}

/// Design §4 predicate, both targets (D16).
#[test]
fn the_card_shows_for_complex_m_sets_on_both_targets_and_hides_otherwise() {
    let d = tempfile::tempdir().unwrap();
    let mut m = fixture(d.path(), Target::Cs);
    let iron = view(&m, "m/iron");
    assert_eq!(cm_members(&m, &iron), [IRON_M]);
    assert!(shows_card(&m, &iron));
    assert!(
        !shows_card(&m, &view(&m, "m/bolt")),
        "a plain env mask under the Envmap shader"
    );
    let bolt = set_id(&m, "m/bolt");
    m.set_shader(&bolt, Some(Shader::ComplexMaterial));
    idle(&mut m);
    assert_eq!(cm_members(&m, &view(&m, "m/bolt")), [BOLT_M]);
    m.set_shader(&bolt, None);
    idle(&mut m);
    m.set_target(Target::Vanilla);
    idle(&mut m);
    assert_eq!(
        m.item(IRON_M).unwrap().job.as_ref().unwrap().profile.subtype,
        Subtype::ComplexMaterial
    );
    assert!(shows_card(&m, &view(&m, "m/iron")));
    assert!(!shows_card(&m, &view(&m, "m/bolt")));
}

/// Review focus 3: a stored block keeps the card visible; a PBR set without one is hidden.
#[test]
fn a_stored_block_keeps_the_card() {
    let d = tempfile::tempdir().unwrap();
    let mut m = fixture(d.path(), Target::Cs);
    let bolt = set_id(&m, "m/bolt");
    m.set_cm_value(&bolt, "glossiness", Some(json!(80.0)));
    assert!(shows_card(&m, &view(&m, "m/bolt")));
    let d = tempfile::tempdir().unwrap();
    let m = support::pbr_opened(d.path());
    assert!(!shows_card(&m, &view(&m, "landscape/rock01")));
}

/// Design §6 step 2: the JSON follows the output as resolved (`_em`, an `output_name` rename).
#[test]
fn targets_follow_the_resolved_output_name() {
    let d = tempfile::tempdir().unwrap();
    let mut m = fixture(d.path(), Target::Cs);
    let r = rels(&m);
    assert!(r.contains(&ok("iron_m", "m/iron_m.json")), "{r:?}");
    assert!(r.contains(&ok("axe_em", "m/axe_em.json")), "{r:?}");
    let iron = set_id(&m, "m/iron");
    m.set_output_name(&iron, Some("steel")).unwrap();
    idle(&mut m);
    let r = rels(&m);
    assert!(r.contains(&ok("steel_m", "m/steel_m.json")), "{r:?}");
}

/// Design §6 step 2: a packed cpm `_m` gets its JSON next to the packed output.
#[test]
fn a_packed_cpm_m_gets_its_json_next_to_the_packed_output() {
    let d = tempfile::tempdir().unwrap();
    let p = d.path().join("src/p");
    std::fs::create_dir_all(&p).unwrap();
    for n in ["steel_metallic.png", "steel_glossiness.png"] {
        ImageBuffer::from_fn(64, 64, |x, _| Luma([(x * 4) as u8]))
            .save(p.join(n))
            .unwrap();
    }
    let m = open(d.path(), Target::Cpm);
    let r = rels(&m);
    assert!(r.contains(&ok("steel_m", "p/steel_m.json")), "{r:?}");
}
```

- [ ] **Step 2: Run the tests to verify they fail**

```bash
cargo test -p sdc-gui --test cm_json
```

Expected: a compile error (`to_text`, `SCHEMA_URL`, `cm_members`, `shows_card`, `targets`, `clashes` not found).

- [ ] **Step 3: Implement (append to `crates/gui/src/model/cm_json.rs`)**

```rust
use super::cs_pbr::floats;
use super::sets::{views, SetView};
use engine::config::Subtype;
use engine::shader::Shader;
use std::collections::{BTreeMap, BTreeSet};

/// The schema's `$id`, written as `$schema` (D6). PGPatcher ignores it; editors use it.
pub const SCHEMA_URL: &str =
    "https://raw.githubusercontent.com/hakasapl/PGPatcher/main/schema/complex_material.schema.json";

/// The file's text (design §6): `$schema`, then the block's well-typed known keys, every number a float. serde_json
/// pretty (2-space indent, keys sorted, so `$schema` comes first) and a final newline.
pub fn to_text(values: &Map<String, Json>) -> String {
    let mut obj = Map::new();
    obj.insert("$schema".into(), json!(SCHEMA_URL));
    for (k, v) in values {
        if key(k).is_some_and(|f| valid(f.kind, v)) {
            obj.insert(k.clone(), floats(v));
        }
    }
    let mut s = serde_json::to_string_pretty(&Json::Object(obj)).expect("JSON values serialise");
    s.push('\n');
    s
}

/// The set's complex `_m` members, as item keys (design §4): resolved `complex_material`, or `envmask` in a set whose
/// shader (the choice, else the engine's pick) is Complex Material.
pub fn cm_members(m: &AppModel, v: &SetView) -> Vec<String> {
    let shader = v.shader_choice.or(v.shader.as_ref().map(|p| p.shader));
    v.members
        .iter()
        .filter(|k| {
            m.item(k)
                .and_then(|i| i.job.as_ref())
                .is_some_and(|j| match j.profile.subtype {
                    Subtype::ComplexMaterial => true,
                    Subtype::Envmask => shader == Some(Shader::ComplexMaterial),
                    _ => false,
                })
        })
        .cloned()
        .collect()
}

/// Whether the set editor draws the card: a complex `_m` member, or a stored block (never hide the user's values).
pub fn shows_card(m: &AppModel, v: &SetView) -> bool {
    !cm_members(m, v).is_empty() || m.cm_block(&v.id).is_some()
}

/// One member's JSON (design §6).
#[derive(Debug, Clone, PartialEq)]
pub struct CmTarget {
    pub set: String,
    /// The member's item key.
    pub key: String,
    /// The output's file name without extension (`iron_m`).
    pub name: String,
    /// The JSON's path under the export root, `/`-separated (the hash record's key), or why it is not written.
    pub rel: Result<String, String>,
}

/// The paths, lower-cased, that more than one exporting member writes. Windows file names ignore case.
pub fn clashes<'a>(rels: impl Iterator<Item = &'a str>) -> BTreeSet<String> {
    let mut n: BTreeMap<String, usize> = BTreeMap::new();
    for r in rels {
        *n.entry(r.to_lowercase()).or_default() += 1;
    }
    n.into_iter().filter(|(_, c)| *c > 1).map(|(r, _)| r).collect()
}

/// Every card set's members with their JSON path: the job's output with `.json` (so `_em`, `_envmask`, renames
/// and packed cpm `_m` follow). The output is not `item.job_key`, which is case-folded. Members of exporting sets
/// that share a path are refused.
pub fn targets(m: &AppModel) -> Vec<CmTarget> {
    let root = m.export_root();
    let mut out = vec![];
    for v in views(m).iter() {
        for key in cm_members(m, v) {
            let Some(job) = m.item(&key).and_then(|i| i.job.as_ref()) else {
                continue;
            };
            let json = job.output.with_extension("json");
            let rel = json
                .strip_prefix(&root)
                .map(|r| r.to_string_lossy().replace('\\', "/"))
                .map_err(|_| {
                    format!(
                        "{} is outside the export folder {}",
                        json.display(),
                        root.display()
                    )
                });
            out.push(CmTarget {
                set: v.id.clone(),
                name: job
                    .output
                    .file_stem()
                    .unwrap_or_default()
                    .to_string_lossy()
                    .into_owned(),
                key,
                rel,
            });
        }
    }
    let exporting = |set: &str| m.cm_block(set).is_some_and(|b| b.export);
    let twice = clashes(
        out.iter()
            .filter(|t| exporting(&t.set))
            .filter_map(|t| t.rel.as_deref().ok()),
    );
    for t in out.iter_mut() {
        let clash = exporting(&t.set)
            && t.rel
                .as_ref()
                .is_ok_and(|r| twice.contains(&r.to_lowercase()));
        if clash {
            t.rel = Err(format!(
                "{}.json is written by two complex _m (names ignore case)",
                t.name
            ));
        }
    }
    out
}
```

If `targets_follow_the_resolved_output_name` fails because every `rel` is `Err("… outside the export folder …")`, the logic is not at fault. The cause is path normalisation between `export_root()` and `job.output`. Fix it by comparing `std::path::absolute(&root)` with `std::path::absolute(&json)` before `strip_prefix`. Do not rewrite the function.

- [ ] **Step 4: Run the tests to verify they pass**

```bash
cargo test -p sdc-gui --test cm_json
cargo clippy -p sdc-gui --all-targets -- -D warnings
```

Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add crates/gui/src/model/cm_json.rs crates/gui/tests/cm_json.rs
git commit -m "$(cat <<'EOF'
feat(gui): complex material JSON writer, member predicate and targets

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
EOF
)"
```

---

### Task 5: Export: `prepare`, `finish`, hooks, hand-edit guard, untick note

**Files:**
- Modify: `crates/gui/src/model/cm_json.rs` (append)
- Modify (shared, own commit): `crates/gui/src/model/mod.rs`:
  - field after `pending_cs` (`:367`)
  - init (`:485`)
  - `open_folder` (`:566`)
  - `ExportDone` (`:974-975`)
  - `start_export` (`:1341`)
- Modify: `crates/gui/tests/cm_json.rs` (append)

**Interfaces:**
- Consumes: Task 4's `targets`, `to_text`; Task 2's `validate_cm`; `pbr_export::{hand_edited, write_atomic}` (pub(crate)); `history::sha256_of`; `SetsDoc::{cm_json_shas, set_cm_json_shas, save}`.
- Produces:
  - `pub struct PendingCm { pub root: PathBuf, pub files: Vec<(String, String)>, pub notes: Vec<String> }` (Debug, Clone, Default, PartialEq)
  - `pub fn prepare(m: &AppModel) -> Option<PendingCm>`
  - `pub fn finish(p: &PendingCm, shas: &BTreeMap<String, String>, notes: &mut Vec<String>) -> BTreeMap<String, String>`
  - `#[doc(hidden)] pub fn AppModel::finish_cm_json(&mut self, p: Option<PendingCm>)`

- [ ] **Step 1: Write the failing tests**

Append to `crates/gui/tests/cm_json.rs`:

```rust
use sdc_gui::model::Dialog;
use std::path::PathBuf;

fn export_and_wait(m: &mut AppModel) {
    m.export_clicked();
    if matches!(m.dialog, Some(Dialog::ProceedAnyway(_))) {
        m.proceed_anyway();
    }
    assert!(
        m.wait_idle(Duration::from_secs(120)),
        "export did not finish"
    );
}

fn out(m: &AppModel, rel: &str) -> PathBuf {
    m.export_root().join(rel)
}

fn notes(m: &AppModel) -> String {
    m.export.notes.join("\n")
}

#[test]
fn nothing_is_written_without_a_ticked_export() {
    let d = tempfile::tempdir().unwrap();
    let mut m = fixture(d.path(), Target::Cs);
    let iron = set_id(&m, "m/iron");
    m.set_cm_value(&iron, "glossiness", Some(json!(80.0)));
    export_and_wait(&mut m);
    assert!(out(&m, "m/iron_m.dds").is_file(), "{}", notes(&m));
    assert!(!out(&m, "m/iron_m.json").exists());
    assert!(!out(&m, "m/axe_em.json").exists());
    assert!(m.sets_doc.as_ref().unwrap().cm_json_shas().is_empty());
}

/// Design §6/§9: the file sits next to the `_m`, holds only the set keys, and is recorded. A `--sidecar` record
/// (`<name>.dds.json`) is another file and stays. The GUI exports with `sidecar = false`, and the engine reads or
/// removes `<name>.dds.json` only under `--sidecar` (`api.rs:1798`, `:1934`), so the stub is never read.
#[test]
fn export_writes_the_json_next_to_the_m_and_records_its_hash() {
    let d = tempfile::tempdir().unwrap();
    let mut m = fixture(d.path(), Target::Cs);
    let side = out(&m, "m/iron_m.dds.json");
    std::fs::create_dir_all(side.parent().unwrap()).unwrap();
    std::fs::write(&side, "{\"sidecar\": true}").unwrap();
    let iron = set_id(&m, "m/iron");
    m.set_cm_export(&iron, true);
    m.set_cm_value(&iron, "glossiness", Some(json!(80)));
    export_and_wait(&mut m);
    assert!(out(&m, "m/iron_m.dds").is_file(), "{}", notes(&m));
    assert_eq!(
        std::fs::read_to_string(out(&m, "m/iron_m.json")).unwrap(),
        format!("{{\n  \"$schema\": \"{SCHEMA_URL}\",\n  \"glossiness\": 80.0\n}}\n")
    );
    assert_eq!(
        std::fs::read_to_string(&side).unwrap(),
        "{\"sidecar\": true}"
    );
    assert!(!out(&m, "m/axe_em.json").exists(), "axe's set exports none");
    let shas = m.sets_doc.as_ref().unwrap().cm_json_shas();
    assert_eq!(shas.keys().collect::<Vec<_>>(), ["m/iron_m.json"]);
    assert!(
        notes(&m).contains("Complex material JSON written: 1 file"),
        "{}",
        notes(&m)
    );
}

#[test]
fn a_hand_edited_file_is_kept_and_reported() {
    let d = tempfile::tempdir().unwrap();
    let mut m = fixture(d.path(), Target::Cs);
    let iron = set_id(&m, "m/iron");
    m.set_cm_export(&iron, true);
    export_and_wait(&mut m);
    let f = out(&m, "m/iron_m.json");
    std::fs::write(&f, "{}\n").unwrap();
    m.set_cm_value(&iron, "glossiness", Some(json!(50.0)));
    export_and_wait(&mut m);
    assert_eq!(std::fs::read_to_string(&f).unwrap(), "{}\n");
    assert!(
        notes(&m).contains("kept as edited outside Tessalith"),
        "{}",
        notes(&m)
    );
}

/// D7: unticking leaves the file and its record, with a note; sdc never deletes it.
#[test]
fn unticking_leaves_the_file_in_place_with_a_note() {
    let d = tempfile::tempdir().unwrap();
    let mut m = fixture(d.path(), Target::Cs);
    let iron = set_id(&m, "m/iron");
    m.set_cm_export(&iron, true);
    export_and_wait(&mut m);
    m.set_cm_export(&iron, false);
    export_and_wait(&mut m);
    assert!(out(&m, "m/iron_m.json").is_file());
    assert!(
        notes(&m).contains("complex material export is off for its set"),
        "{}",
        notes(&m)
    );
    assert!(m
        .sets_doc
        .as_ref()
        .unwrap()
        .cm_json_shas()
        .contains_key("m/iron_m.json"));
}

/// Review focus 3.
#[test]
fn a_set_without_a_complex_m_gets_a_note() {
    let d = tempfile::tempdir().unwrap();
    let mut m = fixture(d.path(), Target::Cs);
    let bolt = set_id(&m, "m/bolt");
    m.set_cm_export(&bolt, true);
    export_and_wait(&mut m);
    assert!(!out(&m, "m/bolt_m.json").exists());
    assert!(
        notes(&m).contains(&format!(
            "set {bolt}: no complex _m; complex material JSON not written"
        )),
        "{}",
        notes(&m)
    );
}

#[test]
fn wrong_typed_keys_are_left_out_with_a_note() {
    let d = tempfile::tempdir().unwrap();
    let mut m = fixture(d.path(), Target::Cs);
    let iron = set_id(&m, "m/iron");
    m.set_cm_export(&iron, true);
    m.set_cm_value(&iron, "specular_color", Some(json!([1.0, 2.0])));
    let p = prepare(&m).unwrap();
    assert!(
        p.notes
            .contains(&format!("{iron}: cm.specular_color has the wrong type and was left out")),
        "{:?}",
        p.notes
    );
    assert!(!p.files[0].1.contains("specular_color"));
}

#[test]
fn an_unfinished_export_writes_nothing() {
    let d = tempfile::tempdir().unwrap();
    let mut m = fixture(d.path(), Target::Cs);
    let iron = set_id(&m, "m/iron");
    m.set_cm_export(&iron, true);
    let p = prepare(&m).unwrap();
    assert_eq!(p.files.len(), 1);
    m.export.error = Some("boom".into());
    m.finish_cm_json(Some(p));
    assert!(!out(&m, "m/iron_m.json").exists());
    assert!(notes(&m).contains("Complex material JSON not written: the export did not finish"));
}

#[test]
fn a_member_whose_dds_is_missing_gets_a_note_and_no_json() {
    let d = tempfile::tempdir().unwrap();
    let p = PendingCm {
        root: d.path().to_path_buf(),
        files: vec![("textures/a/x_m.json".into(), to_text(&Map::new()))],
        notes: vec![],
    };
    let mut n = vec![];
    let shas = finish(&p, &BTreeMap::new(), &mut n);
    assert!(shas.is_empty());
    assert!(!d.path().join("textures/a/x_m.json").exists());
    assert!(
        n.iter()
            .any(|x| x.contains("x_m.dds was not exported; JSON not written")),
        "{n:?}"
    );
}

/// D16: a Vanilla complex `_m` gets its JSON too.
#[test]
fn a_vanilla_complex_m_writes_its_json() {
    let d = tempfile::tempdir().unwrap();
    let mut m = fixture(d.path(), Target::Vanilla);
    let iron = set_id(&m, "m/iron");
    m.set_cm_export(&iron, true);
    export_and_wait(&mut m);
    assert!(out(&m, "m/iron_m.json").is_file(), "{}", notes(&m));
}
```

- [ ] **Step 2: Run the tests to verify they fail**

```bash
cargo test -p sdc-gui --test cm_json
```

Expected: a compile error (`prepare`, `finish`, `PendingCm`, `finish_cm_json` not found).

- [ ] **Step 3: Implement (append to `crates/gui/src/model/cm_json.rs`)**

```rust
use super::history::sha256_of;
use super::pbr_export::{hand_edited, write_atomic};
use super::Dialog;
use std::path::PathBuf;

/// What an export about to start writes once it has ended (computed when it starts, like the CS PBR files).
#[derive(Debug, Clone, Default, PartialEq)]
pub struct PendingCm {
    pub root: PathBuf,
    /// (path under the root, `/`-separated; text).
    pub files: Vec<(String, String)>,
    /// Refused files and left-out keys, for the export report.
    pub notes: Vec<String>,
}

/// Design §6: one file per complex `_m` of every set whose export is ticked, on every target (D2). `Some` even when
/// nothing is ticked: the untick/orphan report needs it.
pub fn prepare(m: &AppModel) -> Option<PendingCm> {
    let entries = m.sets_doc.as_ref()?.entries();
    let ts = targets(m);
    let mut p = PendingCm {
        root: m.export_root(),
        ..Default::default()
    };
    for e in &entries {
        let Some(b) = e.cm.as_ref().filter(|b| b.export) else {
            continue;
        };
        for k in &b.bad {
            p.notes.push(format!(
                "{}: cm.{k} has the wrong type and was left out",
                e.id
            ));
        }
        let mine: Vec<&CmTarget> = ts.iter().filter(|t| same_id(&t.set, &e.id)).collect();
        if mine.is_empty() {
            p.notes.push(format!(
                "set {}: no complex _m; complex material JSON not written",
                e.id
            ));
            continue;
        }
        let text = to_text(&b.values);
        let doc: Json = serde_json::from_str(&text).expect("to_text writes JSON");
        // A safety net that should never fire: `to_text` writes only schema keys with their schema types.
        if let Err(errs) = crate::pgp::validate::validate_cm(&doc) {
            p.notes.push(format!(
                "set {}: complex material JSON not written: {}",
                e.id,
                errs.join("; ")
            ));
            continue;
        }
        for t in mine {
            match &t.rel {
                Ok(r) => p.files.push((r.clone(), text.clone())),
                Err(why) => p.notes.push(format!(
                    "{}: complex material JSON not written: {why}",
                    t.name
                )),
            }
        }
    }
    Some(p)
}

/// After a clean export (design §6): write each file whose `_m` is on disk, unless it was edited outside Tessalith,
/// then report the recorded files this run did not write (D7: never deleted). `shas` is the record (path → hash of
/// what was last written there); returns the new record.
pub fn finish(
    p: &PendingCm,
    shas: &BTreeMap<String, String>,
    notes: &mut Vec<String>,
) -> BTreeMap<String, String> {
    notes.extend(p.notes.iter().cloned());
    let mut out = BTreeMap::new();
    let mut written = 0;
    for (rel, text) in &p.files {
        let path = p.root.join(rel);
        let last = shas.get(rel).map(String::as_str);
        let dds = path.with_extension("dds");
        let skip = if !dds.is_file() {
            Some(format!(
                "{}: {} was not exported; JSON not written",
                path.display(),
                dds.file_name().unwrap_or_default().to_string_lossy()
            ))
        } else if hand_edited(&path, text, last) {
            Some(format!(
                "{} kept as edited outside Tessalith (delete it to let Tessalith write it again)",
                path.display()
            ))
        } else {
            match write_atomic(&path, text) {
                Ok(()) => {
                    written += 1;
                    if let Some(s) = sha256_of(&path) {
                        out.insert(rel.clone(), s);
                    }
                    None
                }
                Err(e) => Some(format!("{} not written: {e}", path.display())),
            }
        };
        if let Some(n) = skip {
            notes.push(n);
            if let Some(s) = last {
                out.insert(rel.clone(), s.to_string());
            }
        }
    }
    if written > 0 {
        notes.push(format!(
            "Complex material JSON written: {written} file{} under {}",
            if written == 1 { "" } else { "s" },
            p.root.display()
        ));
    }
    for (rel, sha) in shas {
        if p.files.iter().any(|(r, _)| r.eq_ignore_ascii_case(rel)) {
            continue;
        }
        let path = p.root.join(rel);
        if path.is_file() {
            notes.push(format!(
                "{}: complex material export is off for its set (or the _m left the set); the file was left in \
                 place. Delete it to drop the overrides.",
                path.display()
            ));
            out.insert(rel.clone(), sha.clone());
        }
    }
    out
}

impl AppModel {
    /// The export ended. After a clean export, write the files of `p` (taken from `pending_cm` by the caller) and
    /// record their hashes (saved, like the CS PBR record). Otherwise only add a note.
    #[doc(hidden)]
    pub fn finish_cm_json(&mut self, p: Option<PendingCm>) {
        let Some(p) = p else {
            return;
        };
        let ok = self.export.error.is_none()
            && self
                .export
                .summary
                .as_ref()
                .is_some_and(|s| s.cancelled.is_empty());
        if !ok {
            if !p.files.is_empty() {
                self.export
                    .notes
                    .push("Complex material JSON not written: the export did not finish".into());
            }
            return;
        }
        let recorded = match self.sets_doc.as_ref() {
            Some(d) => d.cm_json_shas(),
            None => return,
        };
        let shas = finish(&p, &recorded, &mut self.export.notes);
        if shas == recorded {
            return;
        }
        let Some(d) = self.sets_doc.as_mut() else {
            return;
        };
        if d.read_only() {
            self.export.notes.push(
                "the complex material JSON hashes were not recorded: sdc-project.toml is read-only".into(),
            );
            return;
        }
        d.set_cm_json_shas(&shas);
        if let Err(e) = d.save(false) {
            let e = format!("sdc-project.toml: {e}");
            // An open question (the PGPatcher JSON overwrite) must stay.
            match self.dialog {
                Some(_) => self.export.notes.push(e),
                None => self.dialog = Some(Dialog::Error(e)),
            }
        }
    }
}
```

- [ ] **Step 4: Hook it into `model/mod.rs` (add-only)**

After `pending_cs` (`:367`):

```rust
    /// Complex material design §6: the `<texture>.json` files the running export writes afterwards.
    pending_cm: Option<cm_json::PendingCm>,
```

In `placeholder`, after `pending_cs: None,` (`:485`):

```rust
            pending_cm: None,
```

In `open_folder`, after `self.pending_cs = None;` (`:566`):

```rust
        self.pending_cm = None;
```

In `ExportDone`, after `self.finish_cs_pbr(cs);` (`:975`):

```rust
                let cm = self.pending_cm.take();
                self.finish_cm_json(cm);
```

In `start_export`, after `self.pending_cs = cs_pbr_export::prepare(self);` (`:1341`):

```rust
        self.pending_cm = cm_json::prepare(self);
```

- [ ] **Step 5: Run the tests to verify they pass**

```bash
cargo test -p sdc-gui --test cm_json --test cs_pbr_export --test export --test pbr_export
cargo clippy -p sdc-gui --all-targets -- -D warnings
```

Expected: PASS.

- [ ] **Step 6: Commit, in two commits**

```bash
git add crates/gui/src/model/cm_json.rs crates/gui/tests/cm_json.rs
git commit -m "$(cat <<'EOF'
feat(gui): write <texture>_m.json after a clean export

Hand-edit guard and hash record as the CS PBR export; an unticked or
orphaned file is reported and never deleted (D7).

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
EOF
)"
git add crates/gui/src/model/mod.rs
git commit -m "$(cat <<'EOF'
chore(gui): run the complex material JSON export from the export hooks

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
EOF
)"
```

---

### Task 6: The card: warnings and UI in the set editor

**Files:**
- Modify: `crates/gui/src/model/cm_json.rs` (append: `smallest_alpha`, `card_warnings`)
- Create: `crates/gui/src/ui/cm_card.rs`
- Modify (shared, own commit): `crates/gui/src/ui/mod.rs` (`pub mod cm_card;` after `pub mod bulk_panel;`)
- Modify: `crates/gui/src/ui/set_editor.rs:160-165` (3D) and `:189-191` (2D)
- Modify: `crates/gui/tests/cm_json.rs` (append)

**Interfaces:**
- Consumes: Tasks 3–5 (`KEYS`, `first_value`, `CmKind`, `CmKey`, `shows_card`, `targets`, `AppModel::{cm_block, set_cm_value, set_cm_export}`); `model::export::Row`; `engine::codes::{Code, Warning}`.
- Produces:
  - `pub fn smallest_alpha(m: &AppModel, key: &str) -> Option<u8>`
  - `pub fn card_warnings(m: &AppModel, v: &SetView) -> Vec<String>`
  - `pub fn sdc_gui::ui::cm_card::section(ui: &mut egui::Ui, model: &mut AppModel, v: &SetView)`
  - `pub const SECTION`, `EXPORT`, `NOTE`

- [ ] **Step 1: Write the failing tests**

Append to `crates/gui/tests/cm_json.rs`:

```rust
use egui_kittest::kittest::Queryable;
use egui_kittest::Harness;
use engine::codes::{Code, Warning};
use sdc_gui::model::export::{Row, RowState};

fn margin_row(m: &AppModel, key: &str, alpha: f64) -> Row {
    Row {
        job_key: m.item(key).unwrap().job_key.clone(),
        path: String::new(),
        state: RowState::Done { passed: true },
        warnings: vec![Warning::new(Code::ValMComplexMargin, "margin")
            .with_data(json!({"smallest_alpha": alpha, "complex": alpha < 251.0}))],
    }
}

/// D12/D17 and Review focus 5: the 251–254 line and the 255 line come from the latest export row only; a fractional
/// alpha is rounded.
#[test]
fn the_alpha_lines_follow_the_latest_export_row() {
    let d = tempfile::tempdir().unwrap();
    let mut m = fixture(d.path(), Target::Cs);
    let iron = view(&m, "m/iron");
    assert!(
        card_warnings(&m, &iron).is_empty(),
        "before an export nothing shows"
    );
    for (a, want) in [
        (
            252.0,
            Some("iron_m: smallest-mip alpha 252. PGPatcher treats it as complex material and applies iron_m.json"),
        ),
        (250.6, Some("iron_m: smallest-mip alpha 251.")),
        (250.0, None),
        (
            255.0,
            Some("PGPatcher likely reads iron_m as an env mask; the JSON will not be applied."),
        ),
    ] {
        m.export.rows = vec![margin_row(&m, IRON_M, a)];
        let w = card_warnings(&m, &iron);
        match want {
            Some(s) => assert!(w.iter().any(|l| l.contains(s)), "{a}: {w:?}"),
            None => assert!(w.is_empty(), "{a}: {w:?}"),
        }
    }
}

/// Design §5: env-mask members warn; a Vanilla complex `_m` does not (D16); a wrong-typed key is listed.
#[test]
fn env_mask_members_and_bad_keys_warn_and_a_vanilla_complex_m_does_not() {
    let d = tempfile::tempdir().unwrap();
    let mut m = fixture(d.path(), Target::Cs);
    let bolt = set_id(&m, "m/bolt");
    m.set_shader(&bolt, Some(Shader::ComplexMaterial));
    idle(&mut m);
    let w = card_warnings(&m, &view(&m, "m/bolt"));
    assert!(
        w.iter()
            .any(|l| l.contains("bolt_m is written as an env mask without alpha")),
        "{w:?}"
    );
    m.set_field(IRON_M, "alpha.policy", json!("remove"));
    idle(&mut m);
    let w = card_warnings(&m, &view(&m, "m/iron"));
    assert!(
        w.iter()
            .any(|l| l.contains("iron_m is written as an env mask")),
        "{w:?}"
    );
    m.clear_field(IRON_M, "alpha.policy");
    idle(&mut m);
    m.set_target(Target::Vanilla);
    idle(&mut m);
    assert_eq!(card_warnings(&m, &view(&m, "m/iron")), Vec::<String>::new());
    let iron = set_id(&m, "m/iron");
    m.set_cm_value(&iron, "glossiness", Some(json!("high")));
    assert_eq!(
        card_warnings(&m, &view(&m, "m/iron")),
        ["glossiness: wrong type in sdc-project.toml; not exported"]
    );
}

/// The card in the set editor: the note, the target path, and one undo step per tick.
#[test]
fn the_card_ticks_export_and_a_key_in_one_undo_step_each() {
    use sdc_gui::ui::cm_card::{EXPORT, NOTE, SECTION};
    let d = tempfile::tempdir().unwrap();
    let mut m = fixture(d.path(), Target::Cs);
    let v = view(&m, "m/iron");
    m.select_set(&v.key);
    let mut h = Harness::builder()
        .with_size(eframe::egui::vec2(1400.0, 2400.0))
        .build_ui_state(|ui, model: &mut AppModel| sdc_gui::ui::draw(ui, model), m);
    sdc_gui::fonts::install_system_fonts(&h.ctx).expect("system fonts");
    h.run();
    h.get_by_label(SECTION).click();
    h.run();
    assert!(h.query_all_by_label_contains(NOTE).next().is_some());
    assert!(h
        .query_all_by_label_contains("m\\iron_m.json")
        .next()
        .is_some());
    h.get_by_label(EXPORT).click();
    h.run();
    h.get_by_label("glossiness").click();
    h.run();
    let m = h.state_mut();
    let b = m.cm_block(&v.id).unwrap();
    assert!(b.export);
    assert_eq!(b.values["glossiness"], json!(80.0));
    m.undo();
    idle(m);
    let b = m.cm_block(&v.id).unwrap();
    assert!(b.export && !b.values.contains_key("glossiness"), "{b:?}");
}
```

- [ ] **Step 2: Run the tests to verify they fail**

```bash
cargo test -p sdc-gui --test cm_json
```

Expected: a compile error (`card_warnings`, `sdc_gui::ui::cm_card` not found).

- [ ] **Step 3: Model warnings (append to `crates/gui/src/model/cm_json.rs`)**

```rust
use engine::codes::Code;
use engine::config::AlphaPolicy;

/// The smallest-mip alpha of `key`'s latest export, from its `VAL-M-COMPLEX-MARGIN` data, rounded (an averaged
/// non-square smallest mip can be fractional). `None` before an export, or when the engine raised no margin warning
/// (alpha below `validation.complex_m_warn_alpha`, 247 by default) (D17).
pub fn smallest_alpha(m: &AppModel, key: &str) -> Option<u8> {
    let jk = &m.item(key)?.job_key;
    let a = m
        .export
        .rows
        .iter()
        .find(|r| r.job_key.eq_ignore_ascii_case(jk))?
        .warnings
        .iter()
        .find(|w| w.code == Code::ValMComplexMargin)?
        .data
        .get("smallest_alpha")?
        .as_f64()?;
    Some(a.round().clamp(0.0, 255.0) as u8)
}

/// Written without alpha (an `envmask`, or an explicit `alpha.policy = remove`): PGPatcher reads it as an env mask.
fn env_written(m: &AppModel, key: &str) -> bool {
    m.item(key)
        .and_then(|i| i.job.as_ref())
        .is_some_and(|j| j.profile.alpha.policy == AlphaPolicy::Remove)
}

/// The card's amber lines (design §5), in order: wrong-typed keys, then per member its env-mask or alpha case and
/// its refusal.
pub fn card_warnings(m: &AppModel, v: &SetView) -> Vec<String> {
    let mut out: Vec<String> = m
        .cm_block(&v.id)
        .map(|b| {
            b.bad
                .iter()
                .map(|k| format!("{k}: wrong type in sdc-project.toml; not exported"))
                .collect()
        })
        .unwrap_or_default();
    for t in targets(m).iter().filter(|t| same_id(&t.set, &v.id)) {
        let n = &t.name;
        if env_written(m, &t.key) {
            out.push(format!(
                "{n} is written as an env mask without alpha, so PGPatcher ignores the JSON. Set its sub-type to \
                 complex material."
            ));
        } else {
            match smallest_alpha(m, &t.key) {
                Some(a @ 251..=254) => out.push(format!(
                    "{n}: smallest-mip alpha {a}. PGPatcher treats it as complex material and applies {n}.json, but \
                     Community Shaders renders it as a plain env mask (alpha ≥ 251)."
                )),
                Some(255) => out.push(format!(
                    "PGPatcher likely reads {n} as an env mask; the JSON will not be applied."
                )),
                _ => {}
            }
        }
        if let Err(why) = &t.rel {
            out.push(format!("{n}: complex material JSON not written: {why}"));
        }
    }
    out
}
```

- [ ] **Step 4: Create `crates/gui/src/ui/cm_card.rs`**

```rust
//! The set editor's "PGPatcher complex material" card (complex material design 2026-10-14 §5). Drawing only: every
//! change is a model call made after drawing (one `sets_doc` edit, one undo step).
use super::columns::AMBER;
use crate::model::cm_json::{card_warnings, first_value, shows_card, targets, CmKey, CmKind, KEYS};
use crate::model::sets::SetView;
use crate::model::AppModel;
use eframe::egui;
use engine::sets::same_id;
use serde_json::{json, Value as Json};

pub const SECTION: &str = "PGPatcher complex material";
pub const EXPORT: &str = "Export complex material JSON";
const EXPORT_HOVER: &str = "Writes a .json with the _m's name next to each complex _m when you export. PGPatcher \
                            applies it to every shape it patches as complex material with that _m.";
/// D11.
pub const NOTE: &str = "PGPatcher sets specular colour, specular strength and environment map scale to 1 on every \
                        complex-material shape unless this file sets them. Glossiness keeps the mesh's value. Only \
                        the keys you tick are written.";
const CUBEMAP_HOVER: &str =
    "Off keeps the shape's own cubemap. Under CS an active complex material still uses the dynamic path.";

enum Act {
    Export(bool),
    Value(String, Option<Json>),
}

pub fn section(ui: &mut egui::Ui, model: &mut AppModel, v: &SetView) {
    if !shows_card(model, v) {
        return;
    }
    let mut acts = vec![];
    let m: &AppModel = model;
    let b = m.cm_block(&v.id).unwrap_or_default();
    let editable = m.sets_doc.as_ref().is_some_and(|d| !d.read_only());
    let why = m.banner.clone().unwrap_or_default();
    egui::CollapsingHeader::new(SECTION)
        .id_salt(("cm-card", v.id.as_str()))
        .show(ui, |ui| {
            ui.add_enabled_ui(editable, |ui| {
                let mut on = b.export;
                if ui
                    .checkbox(&mut on, EXPORT)
                    .on_hover_text(EXPORT_HOVER)
                    .changed()
                {
                    acts.push(Act::Export(on));
                }
                for t in targets(m).iter().filter(|t| same_id(&t.set, &v.id)) {
                    ui.weak(match t.rel.as_deref() {
                        Ok(r) => r.replace('/', "\\"),
                        Err(_) => format!("{}.json", t.name),
                    });
                }
                ui.weak(NOTE);
                for k in &KEYS {
                    key_row(ui, k, b.values.get(k.name), &mut acts);
                }
                for w in card_warnings(m, v) {
                    ui.colored_label(AMBER, w);
                }
            })
            .response
            .on_disabled_hover_text(&why);
        });
    for a in acts {
        match a {
            Act::Export(on) => model.set_cm_export(&v.id, on),
            Act::Value(k, x) => model.set_cm_value(&v.id, &k, x),
        }
    }
}

/// `[ ] key` then its value widget once ticked; unticking removes the key (the per-key clear).
fn key_row(ui: &mut egui::Ui, k: &CmKey, cur: Option<&Json>, acts: &mut Vec<Act>) {
    ui.horizontal_wrapped(|ui| {
        let mut set = cur.is_some();
        let mut r = ui.checkbox(&mut set, k.name);
        if k.name == "dynamic_cubemap" {
            r = r.on_hover_text(CUBEMAP_HOVER);
        }
        if r.changed() {
            acts.push(Act::Value(k.name.into(), set.then(|| first_value(k))));
        }
        let Some(cur) = cur else {
            ui.weak(k.hint);
            return;
        };
        match k.kind {
            CmKind::Bool => {
                let mut on = cur.as_bool().unwrap_or(true);
                if ui.checkbox(&mut on, "on").changed() {
                    acts.push(Act::Value(k.name.into(), Some(json!(on))));
                }
            }
            CmKind::Num => {
                let mut x = cur.as_f64().unwrap_or(1.0);
                if drag(ui, &mut x) {
                    acts.push(Act::Value(k.name.into(), Some(json!(x))));
                }
            }
            CmKind::Rgb => {
                let mut c: Vec<f64> = cur
                    .as_array()
                    .map(|a| a.iter().map(|x| x.as_f64().unwrap_or(1.0)).collect())
                    .unwrap_or_default();
                let mut changed = false;
                for (x, l) in c.iter_mut().zip(["R", "G", "B"]) {
                    ui.label(l);
                    changed |= drag(ui, x);
                }
                if changed {
                    acts.push(Act::Value(k.name.into(), Some(json!(c))));
                }
            }
        }
    });
}

/// ≥ 0 with no upper bound (D5, D8).
fn drag(ui: &mut egui::Ui, x: &mut f64) -> bool {
    ui.add(
        egui::DragValue::new(x)
            .range(0.0..=f64::INFINITY)
            .speed(0.01),
    )
    .changed()
}
```

- [ ] **Step 5: Place it in the set editor**

`crates/gui/src/ui/set_editor.rs`, 3D (`:160-165`). The CS scroll area now holds both sections:

```rust
        // The CS PBR texture-set/material-object section and the complex material card fold here too.
        egui::ScrollArea::vertical()
            .id_salt("set-3d-cs-scroll")
            .max_height(tall)
            .show(ui, |ui| {
                super::cs_pbr::section(ui, model, &v);
                super::cm_card::section(ui, model, &v);
            });
        return;
```

2D. In the right column after the skin card, at `:189-191`, after the `if v.skin { … }` block. `cols[0]`'s last use is the `cs_pbr` call above, so `cols[1]` is free here:

```rust
                if v.skin {
                    super::skin_card::card(&mut cols[1], model, &v);
                }
                super::cm_card::section(&mut cols[1], model, &v);
```

`crates/gui/src/ui/mod.rs`, after `pub mod bulk_panel;`:

```rust
pub mod cm_card;
```

- [ ] **Step 6: Run the tests to verify they pass**

```bash
cargo test -p sdc-gui --test cm_json --test set_editor_ui --test cs_pbr_ui --test skin_card --test parallax
cargo clippy -p sdc-gui --all-targets -- -D warnings
cargo fmt --all -- --check
```

Expected: PASS.

- [ ] **Step 7: Commit, in two commits**

```bash
git add crates/gui/src/model/cm_json.rs crates/gui/src/ui/cm_card.rs crates/gui/src/ui/set_editor.rs crates/gui/tests/cm_json.rs
git commit -m "$(cat <<'EOF'
feat(gui): PGPatcher complex material card in the set editor

Export box, target paths, the D11 note, six unset-by-default keys and
amber warnings (bad types, post-export 251-254 / 255 alpha, env mask,
refusals). 2D right column, 3D next to the CS section.

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
EOF
)"
git add crates/gui/src/ui/mod.rs
git commit -m "$(cat <<'EOF'
chore(gui): register ui::cm_card

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
EOF
)"
```

---

### Task 7: Docs (Note AT, spec cell, wiki) and the full gate

**Files:**
- Modify: `docs/DECISIONS.md` (append after Note AS, at the end of the file)
- Modify: `docs/superpowers/specs/2026-10-08-skyrim-dds-converter-design.md:252`
- Modify: `docs/wiki/Presets-and-Targets.md:11, 29, 86`
- Modify: `docs/wiki/Texture-Types.md:41, 113`

**Interfaces:** none.

- [ ] **Step 1: Append Note AT to `docs/DECISIONS.md`**

```markdown
## Note AT — PGPatcher complex material JSON and the Vanilla complex `_m` (plan rulings, 2026-10-15)

Spec `F:\ai\dds\nifparseref\plan\complex-material\2026-10-14-complex-material-design.md` (owner-approved 2026-10-14, including edits to `sets_doc.rs`, `pgp/validate.rs`, engine `policy.rs`/`suggest.rs`, the `vanilla-quality` description, `schema/` and two golden tests).

- **AT1 Vanilla keeps a complex `_m` (D16).** The `(Vanilla, complex_material) → envmask` fallback and the "_m under vanilla" suggestion are gone.
  - Vanilla uses the CS rule: a Note L varying alpha gives `complex_material` (High, BC7 UNORM with alpha, BC3 option, Note AH unchanged), and anything else gives `envmask` (BC4, no alpha).
  - Side effects: a Vanilla set with a complex `_m` auto-picks the Complex Material shader. A varying-alpha `_m` that used to be skipped (`LOSS-ALPHA-DATA`) or written as BC4 is now BC7, twice the size, and its `profile_sha256` changes once, so `if-changed` re-encodes it. The engine version stays 0.7.0.
  - The vanilla shader ignores the alpha without ENB. The owner accepted this.
  - Golden `vanilla_fallbacks_are_written_with_allow_cfg_unavailable` drops `iron_m`: three fallbacks.
  - Golden `vanilla_g_is_glow_and_m_with_alpha_is_envmask_with_loss` now expects BC7 `complex_material` plus `VAL-M-GRAYSCALE` (its RGB is grey). The test name predates AT.
- **AT2 `[sets.cm]`.** Per set: `export` plus the schema keys under their schema names. A key that is present is set, and an absent key is unset.
  - `RESERVED` has 13 entries with `cm`.
  - Keys with the wrong type, including a non-bool `export`, stay in the file, are listed and are not exported.
  - Unticking writes `export = false` and keeps the values.
- **AT3 File and text.** The file is `<job output>.json` under the export root, so `_em`, renames and packed cpm `_m` follow.
  - The text is `$schema` (the schema `$id`) plus the set, well-typed keys, every number a float, serde_json pretty with a final newline.
  - It is validated against the vendored `complex_material.schema.json` (upstream `0176567`, identical at 2.2.0) by `pgp::validate::validate_cm`.
  - Written after a clean export only, on every target. The guards are the hand-edit guard and `[project] cm_json_sha256`, which undo keeps. A missing `.dds`, a path clash or an output outside the export root gets a note.
  - Untick or orphan: the file is left in place, with a note. It is never deleted.
- **AT4 Card.** It shows when the set has a complex `_m`: resolved `complex_material`, or `envmask` under the Complex Material shader. It also shows when a `[sets.cm]` block exists.
  - It holds the export box, the target paths, the D11 note and six key rows, which start unset. A ticked key starts at 1, (1, 1, 1), `true` or glossiness 80. It is never prefilled from a mesh.
  - Amber lines: bad types; written as an env mask (`alpha.policy = remove`); and, after an export only, the `VAL-M-COMPLEX-MARGIN` smallest alpha. That alpha is rounded: 251–254 means CS env mask but PGPatcher complex, and 255 means PGPatcher likely env mask.
  - Placement: in 2D the right column, after the TruePBR panel and the skin card. In 3D, next to the CS PBR section.
- **AT5 Plan rulings.**
  - Engine Vanilla tests live in `complex_m_alpha.rs`, not `policy_checks.rs`, because its helpers take a target.
  - `_em`, rename and packed cpm paths are tested at the `targets` level. The write itself is tested once, on `iron_m`, through the same code.
  - Notes say "Tessalith", as the CS export does.
```

- [ ] **Step 2: Update the spec cell and the wiki**

`docs/superpowers/specs/2026-10-08-skyrim-dds-converter-design.md:252`:

```markdown
| `complex_material` | BC7_UNORM (BC3 option) | BC7_UNORM (BC3 option) | BC7_UNORM (BC3 option) |
```

`docs/wiki/Presets-and-Targets.md`:
- At `:11`, delete `no complex _m, `. The rest of the line stays.
- Delete the `complex_material` row at `:29`.
- At `:86`, the description becomes `Bethesda shaders. No `_SRGB`, no BC6H.`

`docs/wiki/Texture-Types.md`:
- At `:41`, the row becomes:

```markdown
| `_m`, `_em`, `_envmask` | `envmask`; `complex_material` when the alpha varies | High for both |
```

- At `:113`, `Complex _m. Not under vanilla.` becomes `Complex _m. Under every target (Note AT).`

- [ ] **Step 3: The full gate**

```bash
cd /f/ai/dds/converter-nif-cm && export CARGO_TARGET_DIR=/f/ai/dds/converter-nif-p1/target
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace -- --skip render_
cargo test -p sdc-gui --test render
git diff --stat nif/main -- Cargo.lock Cargo.toml crates/gui/Cargo.toml crates/cli crates/encoders-sys    # expect nothing
git diff nif/main --stat -- crates/engine/tests/golden.rs    # only the two approved tests
git diff nif/main -- crates/gui/src/model/pbr_export.rs crates/gui/src/model/sets.rs    # expect nothing
```

Expected:
- All PASS.
- `Cargo.lock` is unchanged.
- No `gui_repro` test changed.
- `schema/` is unchanged.

- [ ] **Step 4: Commit**

```bash
git add docs/DECISIONS.md docs/superpowers/specs/2026-10-08-skyrim-dds-converter-design.md docs/wiki/Presets-and-Targets.md docs/wiki/Texture-Types.md
git commit -m "$(cat <<'EOF'
docs: DECISIONS Note AT (complex material JSON, vanilla complex _m)

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
EOF
)"
```

- [ ] **Step 5: Owner check in game (report only; not done by the executor)**

Hand the owner these steps:
1. Export a test set with the box ticked, once on CS and once on Vanilla.
2. Rerun PGPatcher 2.2 with trace.
3. Expect `Failed to parse JSON` = 0, and the Vanilla `_m` logged as `Type: complex material`.
4. Then do the NIF check from `03-nif-check.md` on one key.

---

## Self-review

**1. Spec coverage**

| Spec item | Task |
|---|---|
| §1 goal | Tasks 3–6 |
| D1 / §4 card only on complex sets | Task 4 (`shows_card`), Task 6 (`section` returns early) |
| D2 both targets | Task 5 (`prepare` not target-gated; Vanilla export test) |
| D3 export box off by default | Task 3, Task 6 |
| D4 one block per set | Task 3 |
| D5 three drags, no upper bound | Task 6 `drag` |
| D6 `$schema` | Task 4 |
| D7 untick leaves the file | Task 5 |
| D8 no ranges or preview | Task 6 |
| D9 file name | Task 4 `targets` |
| D10 unset hints, first values, no mesh prefill | Tasks 3 and 6 |
| D11 note | Task 6 `NOTE` |
| D12 / D17 251–254 post-export | Task 6 |
| D13 `pbr.schema.json` verify | Task 2 Step 1 |
| D14 hand-edit guard | Task 5 |
| D15 `sets_doc` / `validate.rs` | Tasks 2–3 |
| D16 engine, goldens, preset text, schema regen | Task 1 |
| §3 parse, bad, unknown, `RESERVED`, setters, record, undo | Task 3 |
| §5 placement 2D/3D, read-only greying, rows, warnings, refusals | Task 6 |
| §6 hooks, `prepare` steps 1–5, `finish`, no `.dds.json` clash | Task 5 (clash logic in Task 4) |
| §7 vendored schema, `validate_cm`, keyword guard | Task 2 |
| §8 engine tests, golden rows, spec §4.2 cell, schema | Tasks 1 and 7 |
| §9 tests | spread as listed in each task |
| §10 files | File Structure |

The spec also lists four items with no task of their own:
- **Packed cpm `_m` export.** Tested at the `targets` level (Task 4), as a ruling.
- **Read-only greying.** Done by `add_enabled_ui`. No UI test.
- **3D placement.** Compile-checked only.
- **Owner in-game check.** Task 7 Step 5.

**2. Placeholder scan.** There are no TBD or "similar to" steps. Every code step has its code.

**3. Type consistency.**
- `set_cm_value(set, key, Option<Json>)` and `set_cm_export(set, bool)` are the same on `SetsDoc` and `AppModel`.
- `CmTarget.rel: Result<String, String>` is used the same way in `prepare`, `card_warnings` and the card.
- `smallest_alpha -> Option<u8>` is matched with `251..=254` and `255`.
- `PendingCm` fields match `finish`.
- `CM_SCHEMA_JSON` and `validate_cm` are as named in Task 2.

**4. Review Focus.** Each of the 5 items has its test in Task 3, 4, 5 or 6, named in the Review Focus section.
