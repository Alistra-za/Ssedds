# M2.5 Channel Packing Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Raw single-channel maps (`_roughness`, `_metallic`, `_ao`, `_specular`, `_glossiness`, `_height`, `_fuzz`, …) in one texture set are packed automatically per target into Skyrim's packed outputs (`cs` `_rmaos`/`_rfaos` + `_p`, `cpm` complex `_m`, `vanilla` env-mask `_m`), with neutral defaults or user fill values for missing channels.

**Architecture:** A new `engine::pack` module plans recipes per set (pure), assembles the packed float image from its sources (I/O), and wires both into `Engine::plan`. A recipe becomes an ordinary `ResolvedJob` of the packed sub-type that carries `pack: Some(PackJob)`; from Preprocess on it runs the normal pipeline. Packing settings (`packing.enabled`, `packing.roughness_default`, `packing.prefer`, `packing.fill.<role>`) are profile fields resolved like any other.

**Tech Stack:** Rust (stable, `x86_64-pc-windows-msvc`), crates `engine` and `cli` of the `F:/ai/dds/converter` workspace; `image` for loading, `serde`/`schemars` for events and schemas.

**Spec:** `docs/superpowers/specs/2026-10-08-skyrim-dds-converter-design.md` (§3.2, §4.4, §6.4, §10, §14 #16–#22), `docs/DECISIONS.md` Note H and Spike S8, and the M2.5 section (scope, exit, carried items) of `docs/superpowers/plans/2026-10-08-skyrim-dds-converter-phase0-2.md`.

## Global Constraints

Every task's requirements implicitly include these lines.

- Licence **GPL-3.0-or-later**. No TruePBR-Manager (CC BY-NC 4.0) or AlphaBloom code is copied.
- Platform **Windows x64 only**; Rust stable on **`x86_64-pc-windows-msvc`**; `+crt-static`; **no ISPC at build time**. In Git Bash run `export PATH="$HOME/.cargo/bin:$PATH"` before `cargo`.
- Mips: float32 throughout; full chain to 1×1 unless `none`. No ≤128 px BC7 fallback (DECISIONS #7, confirmed by Spike S2).
- Alpha: `remove` drops the channel before any mip or encode step. `alpha.exact_opaque` is on by default only for `envmask` and `complex_material`. `ALPHA-EXACT-OFF-M` is **blocking** (Spike S6).
- Colour: `_SRGB` only under `cs` and only for PBR colour; BC6H only under `cs`. Packed outputs are data (`unorm`).
- CS complex-material rule (Spike S8): a `_m` is complex in CS when its smallest-mip RGB is not grey (some pair of channels ≥ 4/255 apart) or is black (all < 4/255 with 4/255 < alpha < 251/255), **and** smallest-mip alpha < 251/255.
- CLI: human output on **stderr** (`inspect` prints its plan on stdout); with `--json`, stdout carries **only JSON Lines**. Exit codes, highest first: **3** fatal > **2** usage/config > **1** partial > **4** validation failed > **0**.
- Never auto-decide: a file converts only when every field is decided. A packing recipe is a decided sub-type (provenance `recipe:<kind>`).
- Every published schema in `schema/` is regenerated in the same commit as the type change: `cargo run -q -p sdc -- presets schema all --out schema`; `crates/engine/tests/schemas.rs` must stay green.
- Process rules (owner): never use `python3 -`/`python -` heredoc probes (they hang on this machine; write a script file or use the Edit tool); commit only the paths you changed, by name; `G:/` is read-only; never touch `F:/ai/dds/bench` (`results.csv` and `results2.csv` are frozen).
- Before each commit run `cargo fmt --all`, `cargo clippy --workspace --all-targets -- -D warnings` and `cargo test --workspace`; all must pass.

## Review Focus

Inputs the spec implies but no exit bullet exercises. Each has a test in the task named.

1. **A raw source that fails to decode in a set that packs** (`iron_ao.png` is a corrupt file next to `iron_roughness.png`): the recipe is reported `failed`, it is not packed with AO silently defaulted, and the batch exits 1 (through the source's own failure). Test: Task 6 `a_source_that_fails_to_load_fails_its_recipe`.
2. **Two raw maps of one role in a set** (`iron_rough.png` + `iron_roughness.png`): the first by path is used, the other is `PACK-UNUSED-INPUT` naming the one used; nothing is packed twice. Test: Task 3 `a_second_source_of_one_role_is_unused`.
3. **A source edited between `plan` and `run`** (the user saves `iron_ao.png` again while sdc scans): the packed job fails at Load with "changed since planning" naming that source, and nothing is written. Test: Task 5 `a_source_changed_after_planning_fails_the_packed_job`.
4. **Event identity** (final review I5): no two `file_planned` events of one batch share a `path`, even when a set packs both `_rmaos` and `_p` and a pre-packed file is replaced. Test: Task 6 `file_planned_paths_stay_unique`.
5. **A raw map the user converts on its own** (`--set 'file:**/iron_height.png.subtype=height'`): it leaves every recipe, is written as `iron_height.dds`, and the set still packs `_rmaos` from the rest. Test: Task 6 `a_raw_map_given_a_subtype_leaves_the_recipes`.

## Decisions taken in this plan

Open items §14 #18–#22 have shipped defaults; this plan fixes them as below. The owner confirms them at plan review; Task 1 records them in `docs/DECISIONS.md` as **Note M**, and code comments cite "DECISIONS Note M".

- **M1 (#18) Env mask:** a packed `cpm`/`vanilla` `_m` R is the metallic source, else the specular source. Under `cpm` with neither it is `packing.fill.metallic`, else `packing.fill.specular`, else 0. The `cpm` `_m` height default is 0.5 (128 < 251, Spike S8).
- **M2 (#19) Raw maps without a recipe** (`_opacity`, specular-only and fuzz-only `cs` sets, height under `vanilla`, AO and fuzz under `cpm`, …) raise `PACK-UNUSED-INPUT` and are not written.
- **M3 (#20) Specular** is copied unchanged into `_rmaos`/`_rfaos` A.
- **M4 (#21) Exit code:** raw inputs reported `IN-PACKING-INPUT` or `PACK-UNUSED-INPUT` do not make the batch exit 1.
- **M5 (#22) Fill values:** field `packing.fill.<role>` (one per §4.4 raw-map role: `roughness`, `metallic`, `ao`, `specular`, `glossiness`, `height`, `opacity`, `fuzz`), an 8-bit code 0–255. CLI `--fill <role>=<0..255>` is an unscoped `--set packing.fill.<role>=<v>` added after every `--set`, so it beats an unscoped `--set` of the same field and loses to sub-type- and file-scoped ones. A roughness fill is used for a glossiness channel as `255 − v` and back. A filled channel has provenance `fill:<code written>` and raises **no** `PACK-DEFAULTED` (the user chose it). The GUI slider lands with M3's settings panel.
- **M6 Two-pass resolve:** `packing.*` of a packed output is read from its profile resolved with a placeholder analysis (the packed image does not exist yet); the real packed image is then assembled, analysed and resolved again for everything else. A rule that matches on `alpha` therefore cannot change `packing.*`. The same holds for `channels.grayscale_source`: the value of that first profile is stored in `PackJob::gray` and used by the plan-time assembly, the provenance, the report and the pipeline's assembly, so the written image is the analysed one even when the final profile's value differs.
- **M7 Set membership:** the packed suffix (`rmaos`, `rfaos`, `p`, `m`) is added to the set's suffixes before any member resolves, predicted from the raw roles present, so `set_has` rules and PBR detection see it. It is predicted even when `packing.enabled = false` turns that recipe off.
- **M8 Job identity:** a packed job's `input` is its first source in R, G, B, A order; the raw jobs a recipe consumes leave `Plan::jobs`. A raw map feeds at most one recipe per target, so no two jobs share an `input`.
- **M9 Pre-packed conflict** is keyed on an image member whose canonical suffix equals the recipe's suffix (`rmaos`, `rfaos`, `p` under `cs`; `m` under `cpm`/`vanilla`). DDS members are never written, so they never conflict. With `packing.prefer = raw` the replaced file is skipped with `PACK-CONFLICT` and counts as an intentional skip (Note I). Exception (never write neither): when the recipe's sources differ in size, the sizes are checked first, the pre-packed file stays a job (no skip, no `PACK-CONFLICT`) and the recipe fails with `PACK-SIZE-MISMATCH`.
- **M10 Profile hash:** `packing` joins every `Profile`, so every output's `profile_sha256` changes once and `--on-existing if-changed` re-encodes every existing output on the first M2.5 run.
- **M11 Failed source:** a recipe with a source that failed to load is `failed` in `set_resolved` and has no job; the batch already exits 1 through that source's `file_failed`. A recipe with a source whose Load was cancelled is `failed` too, reason "cancelled" (the batch exits 1 through `cancelled`).
- **M12 Provenance:** the recipe's sub-type is `recipe:<kind>`; each packed channel is recorded under `pack.r`, `pack.g`, `pack.b`, `pack.a` as `source:<file>:R` (`source:<file>:luma` with `channels.grayscale_source = luma`, `source:1-<file>:R` when inverted), `default:<value>` or `fill:<code>`.
- **M13 Duplicate roles:** with two raw maps of one role in a set, the first by path is used and the others are `PACK-UNUSED-INPUT`.

## File Structure

| File | Responsibility |
|---|---|
| `crates/engine/src/config.rs` (modify) | `PackPrefer`, `FillSpec`, `PackingSpec`; `Profile.packing` |
| `presets/sdc-base.toml` (modify) | `packing` defaults |
| `crates/engine/src/overrides.rs` (modify) | `packing.*` in `FIELD_PATHS` |
| `crates/engine/src/codes.rs` (modify) | `PACK-DEFAULTED`, `PACK-SIZE-MISMATCH`, `PACK-CONFLICT`, `PACK-UNUSED-INPUT` |
| `crates/engine/src/presets.rs` (modify) | `Provenance::{Source, Default, Fill, Recipe}` |
| `crates/engine/src/pack/mod.rs` (create) | Pure recipe planning: `RecipeKind`, `Recipe`, `recipes_for`, `predicted_suffixes`, `resolve_channels`, `ChannelSource`, `PackJob` |
| `crates/engine/src/pack/assemble.rs` (create) | `check_sizes`, `assemble`: the packed float image from its sources, one source at a time |
| `crates/engine/src/pack/plan.rs` (create) | Plan-time wiring: `raw_role`, `plan_packing` (pass 1), `finish_recipe` (pass 2) |
| `crates/engine/src/policy.rs` (modify) | `resolve_decided` split out of `resolve_file`; `ResolvedJob.pack`; `roughness_default` range check |
| `crates/engine/src/pipeline.rs` (modify) | Packed jobs: per-source stamp check, SHA and assembly; manifest `sources` |
| `crates/engine/src/manifest.rs` (modify) | `SourceRef::of` |
| `crates/engine/src/api.rs` (modify) | Predicted suffixes, packing passes in `plan`, `set_resolved.recipes`, run arms, `is_up_to_date` over all sources |
| `crates/engine/src/events.rs` (modify) | `RecipeReport`, `RecipeStatus`, `SetResolved.recipes` |
| `crates/cli/src/args.rs`, `crates/cli/src/verbs.rs` (modify) | `--fill`; `inspect` prints recipes |
| `crates/engine/tests/pack_recipes.rs`, `pack_assemble.rs`, `pack_run.rs`, `pack_plan.rs`, `packing.rs` (create) | Tests per task |
| `schema/*.schema.json` (regenerate) | Profile, manifest, event, dds-validation |
| `docs/DECISIONS.md`, spec, phase 0–2 plan (modify) | Note M (Task 1), spec sync (Task 9) |

Tasks run in order; each ends green and committed.

---
### Task 1: Packing settings in the profile

**Files:**
- Modify: `crates/engine/src/config.rs` (after `RawMap`; `Profile`)
- Modify: `presets/sdc-base.toml` (`[defaults]`)
- Modify: `crates/engine/src/overrides.rs` (`FIELD_PATHS`)
- Modify: `crates/engine/src/policy.rs` (`check_profile`, near the `alpha.threshold` check)
- Modify: `docs/DECISIONS.md` (new `## Note M` after Note L)
- Regenerate: `schema/profile.schema.json`, `schema/manifest.schema.json`
- Test: `crates/engine/tests/presets.rs`, `crates/engine/tests/overrides.rs`, `crates/engine/tests/policy_checks.rs`
- Add to the commit: `docs/superpowers/plans/2026-10-09-sdc-m2-5-channel-packing.md` (this plan, untracked until now; committed once, with Task 1)

**Interfaces:**
- Produces: `config::PackPrefer { Prepacked, Raw }`; `config::FillSpec` (8 `Option<u8>` fields named after the `RawMap` ids) with `fn get(&self, RawMap) -> Option<u8>`; `config::PackingSpec { enabled: bool, roughness_default: f64, prefer: PackPrefer, fill: FillSpec }`; `Profile.packing: PackingSpec`. Field paths `packing.enabled`, `packing.roughness_default`, `packing.prefer`, `packing.fill.<role>`.

- [ ] **Step 1: Write the failing tests**

Append to `crates/engine/tests/presets.rs` (it already imports `engine::presets::*` and `engine::config::*`; add `use engine::overrides::to_patch;` at the top if missing):

```rust
/// M2.5 (spec §6.4): packing settings are profile fields with the neutral defaults of the base preset.
#[test]
fn packing_fields_default_from_the_base_preset() {
    let reg = PresetRegistry::builtin().unwrap();
    let p = reg.profile("cs-quality", Target::Cs, Subtype::Rmaos).unwrap();
    assert_eq!(
        p.packing,
        PackingSpec {
            enabled: true,
            roughness_default: 0.5,
            prefer: PackPrefer::Prepacked,
            fill: FillSpec::default(),
        }
    );
    // Unset fills are left out of the JSON, so `profile_sha256` moves only when a fill is set.
    let v = serde_json::to_value(&p).unwrap();
    assert_eq!(v["packing"]["fill"], serde_json::json!({}));
}

/// DECISIONS Note M5: a fill is an 8-bit code per raw-map role.
#[test]
fn a_fill_is_an_8_bit_code_per_raw_map_role() {
    let reg = PresetRegistry::builtin().unwrap();
    let mut doc =
        serde_json::to_value(reg.profile("cs-quality", Target::Cs, Subtype::Rmaos).unwrap()).unwrap();
    merge_json(&mut doc, &to_patch("packing.fill.metallic", &serde_json::json!(255)));
    let p = finish_profile(doc.clone(), Target::Cs, Subtype::Rmaos).unwrap();
    assert_eq!(p.packing.fill.get(RawMap::Metallic), Some(255));
    assert_eq!(p.packing.fill.get(RawMap::Ao), None);
    for bad in [serde_json::json!(256), serde_json::json!(-1), serde_json::json!(0.5)] {
        let mut d = doc.clone();
        merge_json(&mut d, &to_patch("packing.fill.ao", &bad));
        assert!(finish_profile(d, Target::Cs, Subtype::Rmaos).is_err(), "{bad}");
    }
}
```

Append to `crates/engine/tests/overrides.rs`:

```rust
/// M2.5: every packing field is settable, unscoped, per sub-type and per (packed) file name.
#[test]
fn packing_fields_are_settable() {
    for s in [
        "packing.enabled=false",
        "packing.roughness_default=0.6",
        "packing.prefer=raw",
        "packing.fill.metallic=255",
        "subtype:rmaos.packing.fill.ao=200",
        "file:**/iron_rmaos.png.packing.fill.specular=128",
    ] {
        assert!(parse_set(s).is_ok(), "{s}");
    }
    let o = parse_set("file:**/iron_rmaos.png.packing.fill.specular=128").unwrap();
    assert_eq!(
        (o.scope, o.field.as_str(), o.value),
        (
            Scope::File("**/iron_rmaos.png".into()),
            "packing.fill.specular",
            json!(128)
        )
    );
    // Role ids only, not exporter terms.
    assert!(parse_set("packing.fill.metal=1").is_err());
}
```

Append to `crates/engine/tests/policy_checks.rs` (uses its `run` helper and `codes`):

```rust
/// M2.5: `packing.roughness_default` must lie in 0..=1 on the sub-types whose recipe reads it.
#[test]
fn roughness_default_outside_0_1_is_cfg_invalid() {
    let img = Rgba32F::new(64, 64, [0.5, 0.5, 0.5, 1.0]);
    let rmaos = |v: &str| {
        let set = format!("packing.roughness_default={v}");
        run(Target::Cs, "textures/a/x_rmaos.png", Some("rmaos"), &["rmaos"], img.clone(), &[set.as_str()], &[])
    };
    assert!(codes(&rmaos("1.5")).contains(&Code::CfgInvalid));
    assert!(codes(&rmaos("-0.1")).contains(&Code::CfgInvalid));
    assert!(!codes(&rmaos("1.0")).contains(&Code::CfgInvalid));
    // A diffuse never reads it.
    let j = run(
        Target::Cs,
        "textures/a/x.png",
        None,
        &[],
        img,
        &["packing.roughness_default=1.5", "subtype=diffuse"],
        &[],
    );
    assert!(!codes(&j).contains(&Code::CfgInvalid), "{:?}", j.warnings);
}
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo test -p engine --test presets --test overrides --test policy_checks 2>&1 | tail -20`
Expected: compile errors (`PackingSpec`, `FillSpec`, `PackPrefer` not found; `packing` unknown field).

- [ ] **Step 3: Implement the types**

In `crates/engine/src/config.rs`, after the `impl RawMap { … }` block:

```rust
cfg_enum!(PackPrefer, "lowercase" { Prepacked, Raw });

/// Global fill values for packed channels without a source map (spec §6.4, DECISIONS Notes H and M5): an 8-bit
/// code per §4.4 raw-map role, `None` = the neutral default. Unset fills are not serialised.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct FillSpec {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub roughness: Option<u8>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub metallic: Option<u8>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ao: Option<u8>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub specular: Option<u8>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub glossiness: Option<u8>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub height: Option<u8>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub opacity: Option<u8>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fuzz: Option<u8>,
}

impl FillSpec {
    pub fn get(&self, role: RawMap) -> Option<u8> {
        match role {
            RawMap::Roughness => self.roughness,
            RawMap::Metallic => self.metallic,
            RawMap::Ao => self.ao,
            RawMap::Specular => self.specular,
            RawMap::Glossiness => self.glossiness,
            RawMap::Height => self.height,
            RawMap::Opacity => self.opacity,
            RawMap::Fuzz => self.fuzz,
        }
    }
}

/// Channel-packing settings (spec §3.2, §6.4). Fields of the packed output's profile, resolved like any other.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct PackingSpec {
    /// Pack this output's recipe; `false` leaves its raw maps as packing inputs (`IN-PACKING-INPUT`).
    pub enabled: bool,
    /// Neutral roughness (0..=1) for a channel without a roughness or glossiness source.
    pub roughness_default: f64,
    /// Which side wins when the set already holds the packed output as a file (`PACK-CONFLICT`).
    pub prefer: PackPrefer,
    #[serde(default)]
    pub fill: FillSpec,
}
```

Add the field to `Profile`, after `validation`:

```rust
    pub validation: ValidationSpec,
    pub packing: PackingSpec,
```

In `presets/sdc-base.toml`, add to `[defaults]` after the `validation = …` line:

```toml
packing = { enabled = true, roughness_default = 0.5, prefer = "prepacked" }
```

In `crates/engine/src/overrides.rs`, append to `FIELD_PATHS` after `"validation.pgpatcher_non_white_share",`:

```rust
    "packing.enabled",
    "packing.roughness_default",
    "packing.prefer",
    "packing.fill.roughness",
    "packing.fill.metallic",
    "packing.fill.ao",
    "packing.fill.specular",
    "packing.fill.glossiness",
    "packing.fill.height",
    "packing.fill.opacity",
    "packing.fill.fuzz",
```

In `crates/engine/src/policy.rs` `check_profile`, right after the `alpha.threshold` range check:

```rust
    // M2.5 (spec §6.4): the neutral roughness (0..=1) of the sub-types whose recipe reads it: `_rmaos`/`_rfaos` and
    // `cpm` `_m`. The check also fires for a pre-packed file of that sub-type, which never reads the value; that is
    // harmless (the value is a profile-wide setting, so a bad one is worth flagging wherever it shows up).
    if (st.is_roughness_member() || st == Subtype::ComplexMaterial)
        && !(0.0..=1.0).contains(&p.packing.roughness_default)
    {
        add(
            CfgInvalid,
            format!(
                "packing.roughness_default {} outside 0..=1",
                p.packing.roughness_default
            ),
        );
    }
```

- [ ] **Step 4: Record DECISIONS Note M**

In `docs/DECISIONS.md`, insert after the `## Note L — …` section (before `## Phase 2 exit`) a section `## Note M — channel packing decisions (M2.5 plan, 2026-10-09)` that copies the thirteen bullets M1–M13 of this plan's "Decisions taken in this plan" verbatim, preceded by one line: "Defaults for spec §14 #18–#22 and the M2.5 design choices; confirmed by the owner at plan review."

- [ ] **Step 5: Regenerate the schemas and run the tests**

Run: `cargo run -q -p sdc -- presets schema all --out schema && cargo test -p engine --test presets --test overrides --test policy_checks --test schemas`
Expected: PASS. `git diff --stat schema/` shows `profile.schema.json` and `manifest.schema.json` changed.

- [ ] **Step 6: Full check and commit**

Run: `cargo fmt --all && cargo clippy --workspace --all-targets -- -D warnings && cargo test --workspace 2>&1 | grep -E "test result|FAILED"`
Expected: no `FAILED`.

```bash
git add crates/engine/src/config.rs presets/sdc-base.toml crates/engine/src/overrides.rs crates/engine/src/policy.rs docs/DECISIONS.md schema/profile.schema.json schema/manifest.schema.json crates/engine/tests/presets.rs crates/engine/tests/overrides.rs crates/engine/tests/policy_checks.rs docs/superpowers/plans/2026-10-09-sdc-m2-5-channel-packing.md
git commit -m "config: packing settings in the profile (M2.5 Task 1); DECISIONS Note M

Every profile_sha256 changes, so if-changed re-encodes existing outputs once (Note M10)."
```

---

### Task 2: PACK codes and packing provenance

**Files:**
- Modify: `crates/engine/src/codes.rs` (catalogue; its unit test `Code::ALL.len()`)
- Modify: `crates/engine/src/presets.rs` (`Provenance`, `Display`, `FromStr`)
- Regenerate: `schema/event.schema.json`, `schema/dds-validation.schema.json`
- Test: `crates/engine/tests/presets.rs`

**Interfaces:**
- Produces: `Code::{PackDefaulted (Warning), PackSizeMismatch (Error), PackConflict (Warning), PackUnusedInput (Warning)}`; `Provenance::{Source(String), Default(String), Fill(String), Recipe(String)}` shown as `source:<…>`, `default:<…>`, `fill:<…>`, `recipe:<…>`, ranked above `Fallback`.

- [ ] **Step 1: Write the failing tests**

Append to `crates/engine/tests/presets.rs` (add `use engine::codes::{Code, Severity};` at the top if missing):

```rust
/// DECISIONS Note M12: packing provenance round-trips and ranks above every existing source, so
/// `float_source_is_linear`'s `>= Suffix` comparison never sees it below a suffix fact.
#[test]
fn packing_provenance_round_trips_and_ranks_last() {
    for (p, s) in [
        (Provenance::Source("iron_roughness.png:R".into()), "source:iron_roughness.png:R"),
        (Provenance::Source("1-iron_gloss.png:R".into()), "source:1-iron_gloss.png:R"),
        (Provenance::Default("0.5".into()), "default:0.5"),
        (Provenance::Fill("255".into()), "fill:255"),
        (Provenance::Recipe("rmaos".into()), "recipe:rmaos"),
    ] {
        assert_eq!(p.to_string(), s);
        assert_eq!(s.parse::<Provenance>().unwrap(), p);
        assert_eq!(serde_json::to_value(&p).unwrap(), serde_json::json!(s));
        assert!(p > Provenance::Fallback && p > Provenance::Suffix(String::new()));
    }
}

#[test]
fn pack_codes_are_in_the_catalogue() {
    for (s, sev) in [
        ("PACK-DEFAULTED", Severity::Warning),
        ("PACK-SIZE-MISMATCH", Severity::Error),
        ("PACK-CONFLICT", Severity::Warning),
        ("PACK-UNUSED-INPUT", Severity::Warning),
    ] {
        let c = Code::parse(s).unwrap();
        assert_eq!((c.as_str(), c.severity()), (s, sev));
    }
}
```

In `crates/engine/src/codes.rs`'s own test, change `assert_eq!(Code::ALL.len(), 52);` to `assert_eq!(Code::ALL.len(), 56);`.

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo test -p engine --test presets pack 2>&1 | tail -5` (one name filter: it matches both new tests)
Expected: compile errors (`Provenance::Source` not found).

- [ ] **Step 3: Implement**

`crates/engine/src/codes.rs`: append to the `codes!` list after `OutUnknownExisting = …,`:

```rust
    PackDefaulted = "PACK-DEFAULTED" => Warning,
    PackSizeMismatch = "PACK-SIZE-MISMATCH" => Error,
    PackConflict = "PACK-CONFLICT" => Warning,
    PackUnusedInput = "PACK-UNUSED-INPUT" => Warning,
```

and extend the module doc comment's list of codes added beyond the spec's original catalogue with: "the four `PACK-*` codes of channel packing (spec §6.4, plan M2.5)".

`crates/engine/src/presets.rs`: append four variants to `Provenance` after `Fallback` (declaration order is precedence order):

```rust
    /// A packed channel read from a raw source (spec §6.4, DECISIONS Note M12): `source:<file>:R`,
    /// `source:<file>:luma`, `source:1-<file>:R` when stored inverted.
    Source(String),
    /// A packed channel without a source, at its neutral default: `default:<value>`.
    Default(String),
    /// A packed channel without a source, at the user's fill value: `fill:<code written>`.
    Fill(String),
    /// The sub-type of a packed output, set by its recipe: `recipe:<kind>`.
    Recipe(String),
```

In `Display`, add before the closing brace of the `match`:

```rust
            Provenance::Source(s) => write!(f, "source:{s}"),
            Provenance::Default(v) => write!(f, "default:{v}"),
            Provenance::Fill(v) => write!(f, "fill:{v}"),
            Provenance::Recipe(k) => write!(f, "recipe:{k}"),
```

In `FromStr`, after the `suffix:` prefix check:

```rust
        if let Some(s) = s.strip_prefix("source:") {
            return Ok(Provenance::Source(s.to_string()));
        }
        if let Some(v) = s.strip_prefix("default:") {
            return Ok(Provenance::Default(v.to_string()));
        }
        if let Some(v) = s.strip_prefix("fill:") {
            return Ok(Provenance::Fill(v.to_string()));
        }
        if let Some(k) = s.strip_prefix("recipe:") {
            return Ok(Provenance::Recipe(k.to_string()));
        }
```

- [ ] **Step 4: Regenerate the schemas and run the tests**

Run: `cargo run -q -p sdc -- presets schema all --out schema && cargo test -p engine --test presets --test schemas && cargo test -p engine --lib codes`
Expected: PASS; `event.schema.json` and `dds-validation.schema.json` list the four codes.

- [ ] **Step 5: Full check and commit**

Run the full check of Task 1 Step 6.

```bash
git add crates/engine/src/codes.rs crates/engine/src/presets.rs schema/event.schema.json schema/dds-validation.schema.json crates/engine/tests/presets.rs
git commit -m "codes: PACK-* codes; provenance source/default/fill/recipe (M2.5 Task 2)"
```

---

### Task 3: Packing recipes (pure)

**Files:**
- Create: `crates/engine/src/pack/mod.rs`
- Modify: `crates/engine/src/lib.rs` (add `pub mod pack;` in alphabetical order, after `pub mod overrides;`)
- Test: `crates/engine/tests/pack_recipes.rs`

**Interfaces:**
- Consumes: `config::{PackingSpec, RawMap, Subtype, Target, GraySource}` (Task 1), `presets::Provenance` (Task 2), `policy::SourceStamp`.
- Produces (all `pub` in `engine::pack`):
  - `enum RecipeKind { Rmaos, Rfaos, Parallax, CpmM, VanillaM }` with `fn subtype(self) -> Subtype`, `fn suffix(self) -> &'static str` (`"rmaos"`, `"rfaos"`, `"p"`, `"m"`, `"m"`), `fn id(self) -> &'static str` (`"rmaos"`, `"rfaos"`, `"parallax"`, `"cpm_m"`, `"vanilla_m"`).
  - `enum Slot { Roughness, Glossiness, Metallic, Ao, Specular, EnvMask, Height }` with `fn id(self) -> &'static str`.
  - `struct RawInput { path: PathBuf, role: RawMap }`.
  - `enum ChannelNeed { Source { path: PathBuf, role: RawMap, invert: bool }, Missing(Slot), MirrorR, Opaque }`.
  - `struct Recipe { kind: RecipeKind, channels: [ChannelNeed; 4] }` with `fn sources(&self) -> Vec<(PathBuf, RawMap)>` (channel order, each path once).
  - `struct Unused { path: PathBuf, role: RawMap, reason: String }`.
  - `fn recipes_for(target: Target, raw: &[RawInput]) -> (Vec<Recipe>, Vec<Unused>)`.
  - `fn predicted_suffixes(target: Target, roles: &BTreeSet<RawMap>) -> Vec<&'static str>`.
  - `enum ChannelSource { Source { path, role, invert }, Fill { role: RawMap, code: u8 }, Default { value: f32 }, MirrorR, Opaque }` (serde tag `from`, snake_case) with `fn provenance(&self, gray: GraySource) -> Option<Provenance>`.
  - `fn resolve_channels(r: &Recipe, p: &PackingSpec) -> ([ChannelSource; 4], Vec<(usize, Slot)>)`.
  - `struct PackSource { path: PathBuf, role: RawMap, stamp: Option<SourceStamp> }`; `struct PackJob { kind: RecipeKind, channels: [ChannelSource; 4], gray: GraySource, sources: Vec<PackSource> }` (both `Serialize + JsonSchema`, used by Tasks 5 and 6). `gray` is the `channels.grayscale_source` the recipe was planned with; the pipeline assembles with it, not with the final profile's value (Task 6 sets it, DECISIONS Note M6).
  - This task declares only `pub mod pack;`. Tasks 4 and 6 each add their own `pub mod assemble;` / `pub mod plan;` line to `pack/mod.rs` when they create the file; do not add either line now (the file would not exist).

- [ ] **Step 1: Write the failing tests**

Create `crates/engine/tests/pack_recipes.rs`:

```rust
//! M2.5 Task 3: which recipes a set's raw maps make per target, and what each channel holds (spec §6.4).
use engine::config::*;
use engine::pack::*;
use engine::presets::Provenance;
use std::collections::BTreeSet;
use std::path::PathBuf;
use ChannelNeed::*;
use RawMap::*;

fn raw(files: &[(&str, RawMap)]) -> Vec<RawInput> {
    files
        .iter()
        .map(|(f, r)| RawInput { path: PathBuf::from(f), role: *r })
        .collect()
}

fn src(f: &str, role: RawMap, invert: bool) -> ChannelNeed {
    Source { path: PathBuf::from(f), role, invert }
}

fn kinds(r: &[Recipe]) -> Vec<RecipeKind> {
    r.iter().map(|r| r.kind).collect()
}

fn unused_names(u: &[Unused]) -> Vec<String> {
    u.iter().map(|u| u.path.to_string_lossy().into_owned()).collect()
}

#[test]
fn cs_packs_rmaos_and_p_from_a_full_set() {
    let (r, u) = recipes_for(
        Target::Cs,
        &raw(&[
            ("iron_roughness.png", Roughness),
            ("iron_metallic.png", Metallic),
            ("iron_ao.png", Ao),
            ("iron_specular.png", Specular),
            ("iron_height.png", Height),
        ]),
    );
    assert_eq!(kinds(&r), [RecipeKind::Rmaos, RecipeKind::Parallax]);
    assert_eq!(
        r[0].channels,
        [
            src("iron_roughness.png", Roughness, false),
            src("iron_metallic.png", Metallic, false),
            src("iron_ao.png", Ao, false),
            src("iron_specular.png", Specular, false),
        ]
    );
    assert_eq!(r[1].channels, [src("iron_height.png", Height, false), MirrorR, MirrorR, Opaque]);
    assert!(u.is_empty(), "{u:?}");
    assert_eq!(r[0].sources().len(), 4);
}

/// Spec §6.4 (owner): missing roughness does not block when an AO or metallic source exists.
#[test]
fn cs_missing_roughness_does_not_block() {
    for only in [("iron_ao.png", Ao), ("iron_metallic.png", Metallic)] {
        let (r, _) = recipes_for(Target::Cs, &raw(&[only]));
        assert_eq!(kinds(&r), [RecipeKind::Rmaos], "{only:?}");
        assert_eq!(r[0].channels[0], Missing(Slot::Roughness));
        assert_eq!(r[0].channels[3], Missing(Slot::Specular));
    }
}

#[test]
fn cs_glossiness_is_inverted_and_roughness_wins_over_it() {
    let (r, u) = recipes_for(Target::Cs, &raw(&[("iron_gloss.png", Glossiness)]));
    assert_eq!(r[0].channels[0], src("iron_gloss.png", Glossiness, true));
    assert!(u.is_empty());
    let (r, u) = recipes_for(
        Target::Cs,
        &raw(&[("iron_gloss.png", Glossiness), ("iron_rough.png", Roughness)]),
    );
    assert_eq!(r[0].channels[0], src("iron_rough.png", Roughness, false));
    assert_eq!(unused_names(&u), ["iron_gloss.png"]);
    assert!(u[0].reason.contains("roughness is used"), "{}", u[0].reason);
}

#[test]
fn cs_fuzz_makes_rfaos_and_metallic_is_unused() {
    let (r, u) = recipes_for(
        Target::Cs,
        &raw(&[
            ("iron_rough.png", Roughness),
            ("iron_fuzz.png", Fuzz),
            ("iron_metal.png", Metallic),
        ]),
    );
    assert_eq!(kinds(&r), [RecipeKind::Rfaos]);
    assert_eq!(r[0].channels[1], src("iron_fuzz.png", Fuzz, false));
    assert_eq!(unused_names(&u), ["iron_metal.png"]);
    assert!(u[0].reason.contains("no metallic channel"), "{}", u[0].reason);
}

/// DECISIONS Note M2: specular-only and fuzz-only cs sets, and opacity anywhere, pack nothing.
#[test]
fn sets_without_a_trigger_pack_nothing() {
    for files in [
        vec![("iron_spec.png", Specular)],
        vec![("iron_fuzz.png", Fuzz)],
        vec![("iron_opacity.png", Opacity)],
    ] {
        let (r, u) = recipes_for(Target::Cs, &raw(&files));
        assert!(r.is_empty(), "{files:?}");
        assert_eq!(u.len(), 1, "{files:?}");
    }
    let (_, u) = recipes_for(
        Target::Cs,
        &raw(&[("iron_ao.png", Ao), ("iron_opacity.png", Opacity)]),
    );
    assert_eq!(unused_names(&u), ["iron_opacity.png"]);
    assert!(u[0].reason.contains("opacity"), "{}", u[0].reason);
}

#[test]
fn cpm_packs_a_complex_m() {
    let (r, u) = recipes_for(
        Target::Cpm,
        &raw(&[
            ("iron_rough.png", Roughness),
            ("iron_ao.png", Ao),
            ("iron_fuzz.png", Fuzz),
        ]),
    );
    assert_eq!(kinds(&r), [RecipeKind::CpmM]);
    assert_eq!(
        r[0].channels,
        [
            Missing(Slot::EnvMask),
            src("iron_rough.png", Roughness, true),
            Missing(Slot::Metallic),
            Missing(Slot::Height),
        ]
    );
    assert_eq!(unused_names(&u), ["iron_ao.png", "iron_fuzz.png"]);
    // Note M1: env mask from metallic (also B), else specular.
    let (r, u) = recipes_for(
        Target::Cpm,
        &raw(&[("iron_metal.png", Metallic), ("iron_spec.png", Specular)]),
    );
    assert_eq!(r[0].channels[0], src("iron_metal.png", Metallic, false));
    assert_eq!(r[0].channels[2], src("iron_metal.png", Metallic, false));
    assert_eq!(r[0].sources().len(), 1);
    assert_eq!(unused_names(&u), ["iron_spec.png"]);
    let (r, _) = recipes_for(
        Target::Cpm,
        &raw(&[("iron_spec.png", Specular), ("iron_height.png", Height)]),
    );
    assert_eq!(r[0].channels[0], src("iron_spec.png", Specular, false));
}

#[test]
fn vanilla_packs_an_env_mask_only_from_a_source() {
    let (r, u) = recipes_for(Target::Vanilla, &raw(&[("iron_metal.png", Metallic)]));
    assert_eq!(kinds(&r), [RecipeKind::VanillaM]);
    assert_eq!(r[0].channels, [src("iron_metal.png", Metallic, false), MirrorR, MirrorR, Opaque]);
    assert!(u.is_empty());
    let (r, u) = recipes_for(Target::Vanilla, &raw(&[("iron_height.png", Height)]));
    assert!(r.is_empty());
    assert_eq!(unused_names(&u), ["iron_height.png"]);
}

/// Review Focus 2, DECISIONS Note M13.
#[test]
fn a_second_source_of_one_role_is_unused() {
    let (r, u) = recipes_for(
        Target::Cs,
        &raw(&[("iron_roughness.png", Roughness), ("iron_rough.png", Roughness)]),
    );
    assert_eq!(r[0].channels[0], src("iron_rough.png", Roughness, false));
    assert_eq!(unused_names(&u), ["iron_roughness.png"]);
    assert!(u[0].reason.contains("iron_rough.png"), "{}", u[0].reason);
}

/// DECISIONS Note M7.
#[test]
fn predicted_suffixes_follow_the_recipes() {
    let set = |r: &[RawMap]| r.iter().copied().collect::<BTreeSet<_>>();
    assert_eq!(predicted_suffixes(Target::Cs, &set(&[Ao, Height])), ["rmaos", "p"]);
    assert_eq!(predicted_suffixes(Target::Cs, &set(&[Fuzz, Ao])), ["rfaos"]);
    assert_eq!(predicted_suffixes(Target::Cpm, &set(&[Height])), ["m"]);
    assert!(predicted_suffixes(Target::Vanilla, &set(&[Height])).is_empty());
}

fn packing() -> PackingSpec {
    PackingSpec {
        enabled: true,
        roughness_default: 0.5,
        prefer: PackPrefer::Prepacked,
        fill: FillSpec::default(),
    }
}

#[test]
fn missing_channels_take_neutral_defaults_or_fills() {
    let (r, _) = recipes_for(Target::Cs, &raw(&[("iron_ao.png", Ao)]));
    let (ch, defaulted) = resolve_channels(&r[0], &packing());
    assert_eq!(ch[0], ChannelSource::Default { value: 0.5 });
    assert_eq!(ch[1], ChannelSource::Default { value: 0.0 });
    assert_eq!(ch[3], ChannelSource::Default { value: 1.0 });
    assert_eq!(defaulted, [(0, Slot::Roughness), (1, Slot::Metallic), (3, Slot::Specular)]);
    // Note M5: a fill replaces the default and is not reported as defaulted.
    let mut p = packing();
    p.fill.metallic = Some(255);
    p.roughness_default = 0.7;
    let (ch, defaulted) = resolve_channels(&r[0], &p);
    assert_eq!(ch[1], ChannelSource::Fill { role: Metallic, code: 255 });
    assert_eq!(ch[0], ChannelSource::Default { value: 0.7 });
    assert_eq!(defaulted, [(0, Slot::Roughness), (3, Slot::Specular)]);
    // A glossiness fill feeds a roughness channel inverted.
    let mut p = packing();
    p.fill.glossiness = Some(51);
    let (ch, _) = resolve_channels(&r[0], &p);
    assert_eq!(ch[0], ChannelSource::Fill { role: Glossiness, code: 204 });
    // cpm: glossiness default is 1 - roughness_default; env mask and height defaults (Note M1).
    let (r, _) = recipes_for(Target::Cpm, &raw(&[("iron_rough.png", Roughness)]));
    let (ch, _) = resolve_channels(&r[0], &packing());
    assert_eq!(ch[0], ChannelSource::Default { value: 0.0 });
    assert_eq!(ch[3], ChannelSource::Default { value: 0.5 });
    let mut p = packing();
    p.fill.metallic = Some(40);
    let (ch, _) = resolve_channels(&r[0], &p);
    assert_eq!(ch[0], ChannelSource::Fill { role: Metallic, code: 40 });
}

/// DECISIONS Note M12.
#[test]
fn channel_provenance_strings() {
    let s = |path: &str, invert| ChannelSource::Source {
        path: PathBuf::from(format!("C:/in/{path}")),
        role: Roughness,
        invert,
    };
    assert_eq!(
        s("iron_rough.png", false).provenance(GraySource::R),
        Some(Provenance::Source("iron_rough.png:R".into()))
    );
    assert_eq!(
        s("iron_gloss.png", true).provenance(GraySource::R),
        Some(Provenance::Source("1-iron_gloss.png:R".into()))
    );
    assert_eq!(
        s("iron_rough.png", false).provenance(GraySource::Luma),
        Some(Provenance::Source("iron_rough.png:luma".into()))
    );
    assert_eq!(
        ChannelSource::Default { value: 0.5 }.provenance(GraySource::R),
        Some(Provenance::Default("0.5".into()))
    );
    assert_eq!(
        ChannelSource::Fill { role: Metallic, code: 255 }.provenance(GraySource::R),
        Some(Provenance::Fill("255".into()))
    );
    assert_eq!(ChannelSource::MirrorR.provenance(GraySource::R), None);
    assert_eq!(ChannelSource::Opaque.provenance(GraySource::R), None);
}
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo test -p engine --test pack_recipes 2>&1 | tail -5`
Expected: compile error (`engine::pack` not found).

- [ ] **Step 3: Implement `crates/engine/src/pack/mod.rs`**

```rust
//! Channel packing (spec §6.4, DECISIONS Notes H and M, plan M2.5): the raw single-channel maps of one texture
//! set become packed outputs per target. This file is pure (which recipes run, what each channel holds);
//! `assemble` builds the image and `plan` wires both into `Engine::plan`.
use crate::config::{GraySource, PackingSpec, RawMap, Subtype, Target};
use crate::policy::SourceStamp;
use crate::presets::Provenance;
use schemars::JsonSchema;
use serde::Serialize;
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum RecipeKind {
    /// `cs` `<base>_rmaos.dds`: roughness, metallic, AO, specular.
    Rmaos,
    /// `cs` `<base>_rfaos.dds`: roughness, fuzz, AO, specular.
    Rfaos,
    /// `cs` `<base>_p.dds`: height (BC4).
    Parallax,
    /// `cpm` complex `<base>_m.dds`: env mask, glossiness, metallic, height.
    CpmM,
    /// `vanilla` env-mask `<base>_m.dds` (BC4).
    VanillaM,
}

impl RecipeKind {
    pub fn subtype(self) -> Subtype {
        match self {
            RecipeKind::Rmaos => Subtype::Rmaos,
            RecipeKind::Rfaos => Subtype::Rfaos,
            RecipeKind::Parallax => Subtype::Height,
            RecipeKind::CpmM => Subtype::ComplexMaterial,
            RecipeKind::VanillaM => Subtype::Envmask,
        }
    }

    /// The §6.1 suffix of the output, `<base>_<suffix>.dds`.
    pub fn suffix(self) -> &'static str {
        match self {
            RecipeKind::Rmaos => "rmaos",
            RecipeKind::Rfaos => "rfaos",
            RecipeKind::Parallax => "p",
            RecipeKind::CpmM | RecipeKind::VanillaM => "m",
        }
    }

    pub fn id(self) -> &'static str {
        match self {
            RecipeKind::Rmaos => "rmaos",
            RecipeKind::Rfaos => "rfaos",
            RecipeKind::Parallax => "parallax",
            RecipeKind::CpmM => "cpm_m",
            RecipeKind::VanillaM => "vanilla_m",
        }
    }
}

/// The quantity a channel holds when no source map provides it (spec §6.4 defaults table).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Slot {
    Roughness,
    Glossiness,
    Metallic,
    Ao,
    Specular,
    EnvMask,
    Height,
}

impl Slot {
    pub fn id(self) -> &'static str {
        match self {
            Slot::Roughness => "roughness",
            Slot::Glossiness => "glossiness",
            Slot::Metallic => "metallic",
            Slot::Ao => "ao",
            Slot::Specular => "specular",
            Slot::EnvMask => "env_mask",
            Slot::Height => "height",
        }
    }
}

/// A raw map of one set waiting for a recipe.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RawInput {
    pub path: PathBuf,
    pub role: RawMap,
}

/// What a channel needs, before fills and defaults are known.
#[derive(Debug, Clone, PartialEq)]
pub enum ChannelNeed {
    /// R (or luma) of a source; `invert` stores 1 − x (glossiness ↔ roughness).
    Source { path: PathBuf, role: RawMap, invert: bool },
    /// No source: a fill value or the neutral default of the slot.
    Missing(Slot),
    /// A copy of R: the unused G and B of a single-channel output, so the image stays grey.
    MirrorR,
    /// Opaque alpha of an output whose alpha carries nothing.
    Opaque,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Recipe {
    pub kind: RecipeKind,
    /// R, G, B, A.
    pub channels: [ChannelNeed; 4],
}

impl Recipe {
    /// Every source, in channel order, each path once. A recipe always has at least one (its trigger).
    pub fn sources(&self) -> Vec<(PathBuf, RawMap)> {
        let mut v: Vec<(PathBuf, RawMap)> = vec![];
        for c in &self.channels {
            if let ChannelNeed::Source { path, role, .. } = c {
                if !v.iter().any(|(p, _)| p == path) {
                    v.push((path.clone(), *role));
                }
            }
        }
        v
    }
}

/// A raw map that no recipe of the target uses (`PACK-UNUSED-INPUT`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Unused {
    pub path: PathBuf,
    pub role: RawMap,
    pub reason: String,
}

fn file_name(p: &Path) -> String {
    p.file_name().unwrap_or_default().to_string_lossy().into_owned()
}

/// Why a raw map of `role` takes part in no recipe of `target` (DECISIONS Note M2).
fn unused_reason(target: Target, role: RawMap, have: &BTreeMap<RawMap, &RawInput>) -> String {
    let has = |r: RawMap| have.contains_key(&r);
    match (target, role) {
        (_, RawMap::Opacity) => "no recipe packs opacity yet (spec §14 #19)".into(),
        (Target::Cs, RawMap::Glossiness) if has(RawMap::Roughness) => {
            "roughness is used: the _rmaos/_rfaos R channel is roughness".into()
        }
        (Target::Cs, RawMap::Metallic) if has(RawMap::Fuzz) => "_rfaos has no metallic channel".into(),
        (Target::Cs, _) => {
            "nothing to pack: _rmaos/_rfaos need a roughness, glossiness, AO or metallic source".into()
        }
        (Target::Cpm, RawMap::Roughness) if has(RawMap::Glossiness) => {
            "glossiness is used: the cpm _m G channel is glossiness".into()
        }
        (Target::Cpm, RawMap::Specular) if has(RawMap::Metallic) => {
            "the env mask comes from the metallic source".into()
        }
        (Target::Cpm, RawMap::Ao | RawMap::Fuzz) => {
            format!("the cpm _m has no {} channel", role.id())
        }
        (Target::Cpm, _) => {
            "nothing to pack: the cpm _m needs a roughness, glossiness, metallic or height source".into()
        }
        (Target::Vanilla, RawMap::Specular) if has(RawMap::Metallic) => {
            "the env mask comes from the metallic source".into()
        }
        (Target::Vanilla, _) => {
            "vanilla packs only an env-mask _m, from a metallic or specular source".into()
        }
    }
}

/// The recipes of one set under `target` (spec §6.4 recipe table) and the raw maps none of them uses.
pub fn recipes_for(target: Target, raw: &[RawInput]) -> (Vec<Recipe>, Vec<Unused>) {
    use ChannelNeed::{Missing, MirrorR, Opaque};
    use RawMap::*;
    let mut sorted: Vec<&RawInput> = raw.iter().collect();
    sorted.sort_by(|a, b| a.path.cmp(&b.path));
    // DECISIONS Note M13: one source per role, the first by path.
    let mut by_role: BTreeMap<RawMap, &RawInput> = BTreeMap::new();
    let mut unused = vec![];
    for r in sorted {
        match by_role.get(&r.role) {
            Some(first) => unused.push(Unused {
                path: r.path.clone(),
                role: r.role,
                reason: format!(
                    "another {} source is used: {}",
                    r.role.id(),
                    file_name(&first.path)
                ),
            }),
            None => {
                by_role.insert(r.role, r);
            }
        }
    }
    let src = |role: RawMap, invert: bool| {
        by_role.get(&role).map(|r| ChannelNeed::Source {
            path: r.path.clone(),
            role,
            invert,
        })
    };
    let has = |role: RawMap| by_role.contains_key(&role);
    let mut recipes = vec![];
    match target {
        Target::Cs => {
            if [Roughness, Glossiness, Ao, Metallic].into_iter().any(has) {
                let rough = src(Roughness, false)
                    .or_else(|| src(Glossiness, true))
                    .unwrap_or(Missing(Slot::Roughness));
                let ao = src(Ao, false).unwrap_or(Missing(Slot::Ao));
                let spec = src(Specular, false).unwrap_or(Missing(Slot::Specular));
                match src(Fuzz, false) {
                    Some(fuzz) => recipes.push(Recipe {
                        kind: RecipeKind::Rfaos,
                        channels: [rough, fuzz, ao, spec],
                    }),
                    None => recipes.push(Recipe {
                        kind: RecipeKind::Rmaos,
                        channels: [
                            rough,
                            src(Metallic, false).unwrap_or(Missing(Slot::Metallic)),
                            ao,
                            spec,
                        ],
                    }),
                }
            }
            if let Some(h) = src(Height, false) {
                recipes.push(Recipe {
                    kind: RecipeKind::Parallax,
                    channels: [h, MirrorR, MirrorR, Opaque],
                });
            }
        }
        Target::Cpm => {
            if [Roughness, Glossiness, Metallic, Height].into_iter().any(has) {
                recipes.push(Recipe {
                    kind: RecipeKind::CpmM,
                    channels: [
                        src(Metallic, false)
                            .or_else(|| src(Specular, false))
                            .unwrap_or(Missing(Slot::EnvMask)),
                        src(Glossiness, false)
                            .or_else(|| src(Roughness, true))
                            .unwrap_or(Missing(Slot::Glossiness)),
                        src(Metallic, false).unwrap_or(Missing(Slot::Metallic)),
                        src(Height, false).unwrap_or(Missing(Slot::Height)),
                    ],
                });
            }
        }
        Target::Vanilla => {
            if let Some(env) = src(Metallic, false).or_else(|| src(Specular, false)) {
                recipes.push(Recipe {
                    kind: RecipeKind::VanillaM,
                    channels: [env, MirrorR, MirrorR, Opaque],
                });
            }
        }
    }
    let used: BTreeSet<PathBuf> = recipes
        .iter()
        .flat_map(|r| r.sources())
        .map(|(p, _)| p)
        .collect();
    for r in by_role.values() {
        if !used.contains(&r.path) {
            unused.push(Unused {
                path: r.path.clone(),
                role: r.role,
                reason: unused_reason(target, r.role, &by_role),
            });
        }
    }
    unused.sort_by(|a, b| a.path.cmp(&b.path));
    (recipes, unused)
}

/// The suffixes a set's recipes would add (spec §6.4 "Set membership", DECISIONS Note M7), from the raw roles
/// present, before anything is resolved.
pub fn predicted_suffixes(target: Target, roles: &BTreeSet<RawMap>) -> Vec<&'static str> {
    let raw: Vec<RawInput> = roles
        .iter()
        .map(|r| RawInput {
            path: PathBuf::from(r.id()),
            role: *r,
        })
        .collect();
    recipes_for(target, &raw)
        .0
        .iter()
        .map(|r| r.kind.suffix())
        .collect()
}

/// What a packed channel holds; serialised in `file_planned` (`job.pack.channels`) and the manifest.
#[derive(Debug, Clone, PartialEq, Serialize, JsonSchema)]
#[serde(tag = "from", rename_all = "snake_case")]
pub enum ChannelSource {
    /// R (or luma) of a raw source; `invert` stores 1 − x.
    Source { path: PathBuf, role: RawMap, invert: bool },
    /// A user fill (`packing.fill.<role>`); `code` is the 8-bit value written (255 − fill when inverted).
    Fill { role: RawMap, code: u8 },
    /// The neutral default of spec §6.4, 0..=1.
    Default { value: f32 },
    /// A copy of R.
    MirrorR,
    /// 1.0.
    Opaque,
}

impl ChannelSource {
    /// The channel's provenance (DECISIONS Note M12); `None` for a mirrored or opaque filler channel.
    pub fn provenance(&self, gray: GraySource) -> Option<Provenance> {
        match self {
            ChannelSource::Source { path, invert, .. } => Some(Provenance::Source(format!(
                "{}{}:{}",
                if *invert { "1-" } else { "" },
                file_name(path),
                match gray {
                    GraySource::R => "R",
                    GraySource::Luma => "luma",
                }
            ))),
            ChannelSource::Fill { code, .. } => Some(Provenance::Fill(code.to_string())),
            ChannelSource::Default { value } => Some(Provenance::Default(value.to_string())),
            ChannelSource::MirrorR | ChannelSource::Opaque => None,
        }
    }
}

/// A user fill for a missing slot (DECISIONS Note M5): the slot's own role, else the inverse quantity's role
/// inverted (roughness ↔ glossiness); the env mask takes the metallic, else the specular fill (Note M1).
fn fill_for(slot: Slot, p: &PackingSpec) -> Option<ChannelSource> {
    let direct = |role: RawMap| p.fill.get(role).map(|code| ChannelSource::Fill { role, code });
    let inverted = |role: RawMap| {
        p.fill.get(role).map(|code| ChannelSource::Fill {
            role,
            code: 255 - code,
        })
    };
    match slot {
        Slot::Roughness => direct(RawMap::Roughness).or_else(|| inverted(RawMap::Glossiness)),
        Slot::Glossiness => direct(RawMap::Glossiness).or_else(|| inverted(RawMap::Roughness)),
        Slot::Metallic => direct(RawMap::Metallic),
        Slot::Ao => direct(RawMap::Ao),
        Slot::Specular => direct(RawMap::Specular),
        Slot::EnvMask => direct(RawMap::Metallic).or_else(|| direct(RawMap::Specular)),
        Slot::Height => direct(RawMap::Height),
    }
}

/// The neutral default of a slot (spec §6.4 defaults table, DECISIONS Note M1).
fn default_for(slot: Slot, p: &PackingSpec) -> f32 {
    match slot {
        Slot::Roughness => p.roughness_default as f32,
        Slot::Glossiness => 1.0 - p.roughness_default as f32,
        Slot::Metallic | Slot::EnvMask => 0.0,
        Slot::Ao | Slot::Specular => 1.0,
        Slot::Height => 0.5,
    }
}

/// The channels of a recipe with fills and defaults applied, and the channels that took a neutral default
/// (index, slot): each raises `PACK-DEFAULTED`; a filled channel does not (DECISIONS Note M5).
pub fn resolve_channels(r: &Recipe, p: &PackingSpec) -> ([ChannelSource; 4], Vec<(usize, Slot)>) {
    let mut defaulted = vec![];
    let channels = std::array::from_fn(|i| match &r.channels[i] {
        ChannelNeed::Source { path, role, invert } => ChannelSource::Source {
            path: path.clone(),
            role: *role,
            invert: *invert,
        },
        ChannelNeed::MirrorR => ChannelSource::MirrorR,
        ChannelNeed::Opaque => ChannelSource::Opaque,
        ChannelNeed::Missing(slot) => fill_for(*slot, p).unwrap_or_else(|| {
            defaulted.push((i, *slot));
            ChannelSource::Default {
                value: default_for(*slot, p),
            }
        }),
    });
    (channels, defaulted)
}

/// One source of a packed job and its size and modification time when planning read it (final review I7).
#[derive(Debug, Clone, PartialEq, Serialize, JsonSchema)]
pub struct PackSource {
    pub path: PathBuf,
    pub role: RawMap,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stamp: Option<SourceStamp>,
}

/// How a packed output is built (`ResolvedJob::pack`). `sources[0].path` is the job's `input` (Note M8).
#[derive(Debug, Clone, PartialEq, Serialize, JsonSchema)]
pub struct PackJob {
    pub kind: RecipeKind,
    /// R, G, B, A.
    pub channels: [ChannelSource; 4],
    /// The `channels.grayscale_source` this recipe was planned with (provenance, analysis and report all use it).
    /// The pipeline assembles with this value, not with the final profile's, so the written image is the one that
    /// was analysed (DECISIONS Note M6). Serialised, so it shows in `file_planned` and the manifest.
    pub gray: GraySource,
    pub sources: Vec<PackSource>,
}
```

Add `pub mod pack;` to `crates/engine/src/lib.rs` after `pub mod overrides;`.

- [ ] **Step 4: Run the tests to verify they pass**

Run: `cargo test -p engine --test pack_recipes`
Expected: PASS (11 tests).

- [ ] **Step 5: Full check and commit**

Run the full check of Task 1 Step 6.

```bash
git add crates/engine/src/pack/mod.rs crates/engine/src/lib.rs crates/engine/tests/pack_recipes.rs
git commit -m "pack: recipes per target, fills and neutral defaults (M2.5 Task 3)"
```

---
### Task 4: Assembling the packed image

**Files:**
- Create: `crates/engine/src/pack/assemble.rs`
- Modify: `crates/engine/src/pack/mod.rs` (add `pub mod assemble;` under the module doc comment)
- Test: `crates/engine/tests/pack_assemble.rs`

**Interfaces:**
- Consumes: `pack::{PackJob, PackSource, ChannelSource, RecipeKind}` (Task 3), `load::{load_image, LoadError}`, `image::{Rgba32F, SourceInfo}`.
- Produces:
  - `enum PackError { Load(LoadError), SizeMismatch(Vec<(PathBuf, u32, u32)>) }` (`thiserror`, `Display` lists every source and size).
  - `fn check_sizes(sources: &[PathBuf]) -> Result<(u32, u32), PackError>` (headers only).
  - `struct Assembled { image: Rgba32F, info: SourceInfo, non_gray: Vec<PathBuf> }`.
  - `fn assemble(job: &PackJob, gray: GraySource) -> Result<Assembled, PackError>`. Callers pass `job.gray` (Task 3); the argument stays explicit so the tests can try both settings on one job.
- Property (stated in the module doc, relied on by `api::estimated_bytes`): sources are decoded one at a time, so the peak is the output plus one decoded source.

- [ ] **Step 1: Write the failing tests**

Create `crates/engine/tests/pack_assemble.rs`:

```rust
//! M2.5 Task 4: the packed float image, built from raw sources one at a time (spec §6.4 "Sources", "Precision").
use engine::config::*;
use engine::pack::assemble::*;
use engine::pack::*;
use image::{ImageBuffer, Luma, Rgb};
use std::path::{Path, PathBuf};

fn gray8(dir: &Path, name: &str, w: u32, f: impl Fn(u32, u32) -> u8) -> PathBuf {
    let p = dir.join(name);
    ImageBuffer::from_fn(w, w, |x, y| Luma([f(x, y)])).save(&p).unwrap();
    p
}

fn job(kind: RecipeKind, channels: [ChannelSource; 4], sources: &[(&PathBuf, RawMap)]) -> PackJob {
    PackJob {
        kind,
        channels,
        gray: GraySource::R,
        sources: sources
            .iter()
            .map(|(p, role)| PackSource { path: (*p).clone(), role: *role, stamp: None })
            .collect(),
    }
}

fn src(p: &Path, role: RawMap, invert: bool) -> ChannelSource {
    ChannelSource::Source { path: p.to_path_buf(), role, invert }
}

/// The loader's u8/u16 → f32 conversion is a division; allow for its last-bit rounding.
fn close(a: f32, b: f32) {
    assert!((a - b).abs() < 1e-6, "{a} vs {b}");
}

#[test]
fn channels_come_from_sources_and_constants() {
    let d = tempfile::tempdir().unwrap();
    let rough = gray8(d.path(), "iron_rough.png", 8, |x, _| x as u8 * 30);
    let ao = gray8(d.path(), "iron_ao.png", 8, |_, y| 255 - y as u8 * 20);
    let j = job(
        RecipeKind::Rmaos,
        [
            src(&rough, RawMap::Roughness, false),
            ChannelSource::Fill { role: RawMap::Metallic, code: 51 },
            src(&ao, RawMap::Ao, false),
            ChannelSource::Default { value: 1.0 },
        ],
        &[(&rough, RawMap::Roughness), (&ao, RawMap::Ao)],
    );
    let a = assemble(&j, GraySource::R).unwrap();
    assert_eq!((a.image.width, a.image.height), (8, 8));
    for y in 0..8 {
        for x in 0..8 {
            let p = a.image.at(x, y);
            // Exact before encode: the stored source value, no quantisation.
            close(p[0], f32::from(x as u8 * 30) / 255.0);
            close(p[1], 51.0 / 255.0);
            close(p[2], f32::from(255 - y as u8 * 20) / 255.0);
            close(p[3], 1.0);
        }
    }
    assert_eq!(a.info.bit_depth, 8);
    assert!(a.info.has_alpha && !a.info.is_float);
    assert!(a.non_gray.is_empty());
}

#[test]
fn inverted_mirrored_opaque_and_shared_sources() {
    let d = tempfile::tempdir().unwrap();
    let gloss = gray8(d.path(), "iron_gloss.png", 4, |x, _| x as u8 * 60);
    let metal = gray8(d.path(), "iron_metal.png", 4, |_, y| y as u8 * 60);
    // cpm: metallic feeds R (env mask) and B; glossiness is G.
    let j = job(
        RecipeKind::CpmM,
        [
            src(&metal, RawMap::Metallic, false),
            src(&gloss, RawMap::Glossiness, false),
            src(&metal, RawMap::Metallic, false),
            ChannelSource::Default { value: 0.5 },
        ],
        &[(&metal, RawMap::Metallic), (&gloss, RawMap::Glossiness)],
    );
    let a = assemble(&j, GraySource::R).unwrap();
    let p = a.image.at(2, 3);
    close(p[0], 180.0 / 255.0);
    close(p[1], 120.0 / 255.0);
    close(p[2], 180.0 / 255.0);
    close(p[3], 0.5);
    // cs `_rmaos` R from glossiness, inverted; `_p` mirrors R and is opaque.
    let j = job(
        RecipeKind::Parallax,
        [src(&gloss, RawMap::Glossiness, true), ChannelSource::MirrorR, ChannelSource::MirrorR, ChannelSource::Opaque],
        &[(&gloss, RawMap::Glossiness)],
    );
    let a = assemble(&j, GraySource::R).unwrap();
    let p = a.image.at(1, 0);
    close(p[0], 1.0 - 60.0 / 255.0);
    assert_eq!((p[1], p[2], p[3]), (p[0], p[0], 1.0));
    assert!(!a.info.has_alpha);
}

/// Spec §6.4 "Precision": a 16-bit source keeps its precision until the encoder.
#[test]
fn sixteen_bit_sources_keep_their_precision() {
    let d = tempfile::tempdir().unwrap();
    let p = d.path().join("iron_height.png");
    ImageBuffer::from_fn(4, 4, |x, _| Luma([1000u16 + x as u16])).save(&p).unwrap();
    let j = job(
        RecipeKind::Parallax,
        [src(&p, RawMap::Height, false), ChannelSource::MirrorR, ChannelSource::MirrorR, ChannelSource::Opaque],
        &[(&p, RawMap::Height)],
    );
    let a = assemble(&j, GraySource::R).unwrap();
    // 1001/65535 lies between two 8-bit codes: the value is not quantised to 8 bits.
    close(a.image.at(1, 0)[0], 1001.0 / 65535.0);
    assert!((a.image.at(1, 0)[0] * 255.0).fract().abs() > 0.01);
    assert_eq!(a.info.bit_depth, 16);
}

#[test]
fn non_grey_sources_are_named_and_luma_is_honoured() {
    let d = tempfile::tempdir().unwrap();
    let p = d.path().join("iron_ao.png");
    ImageBuffer::from_fn(4, 4, |_, _| Rgb([200u8, 100, 0])).save(&p).unwrap();
    let mk = || {
        job(
            RecipeKind::Rmaos,
            [ChannelSource::Default { value: 0.5 }, ChannelSource::Default { value: 0.0 }, src(&p, RawMap::Ao, false), ChannelSource::Default { value: 1.0 }],
            &[(&p, RawMap::Ao)],
        )
    };
    let a = assemble(&mk(), GraySource::R).unwrap();
    assert_eq!(a.non_gray, [p.clone()]);
    close(a.image.at(0, 0)[2], 200.0 / 255.0);
    let a = assemble(&mk(), GraySource::Luma).unwrap();
    let luma = 0.2126 * (200.0 / 255.0) + 0.7152 * (100.0 / 255.0);
    assert!((a.image.at(0, 0)[2] - luma).abs() < 1e-6);
}

#[test]
fn sources_of_different_sizes_are_refused_with_every_size() {
    let d = tempfile::tempdir().unwrap();
    let a = gray8(d.path(), "iron_rough.png", 64, |_, _| 10);
    let b = gray8(d.path(), "iron_ao.png", 32, |_, _| 10);
    let err = check_sizes(&[a.clone(), b.clone()]).unwrap_err();
    match &err {
        PackError::SizeMismatch(s) => assert_eq!(s, &vec![(a.clone(), 64, 64), (b.clone(), 32, 32)]),
        e => panic!("{e}"),
    }
    let text = err.to_string();
    assert!(text.contains("iron_rough.png 64x64") && text.contains("iron_ao.png 32x32"), "{text}");
    assert_eq!(check_sizes(&[a.clone()]).unwrap(), (64, 64));
    let j = job(
        RecipeKind::Rmaos,
        [src(&a, RawMap::Roughness, false), ChannelSource::Default { value: 0.0 }, src(&b, RawMap::Ao, false), ChannelSource::Default { value: 1.0 }],
        &[(&a, RawMap::Roughness), (&b, RawMap::Ao)],
    );
    assert!(matches!(assemble(&j, GraySource::R), Err(PackError::SizeMismatch(_))));
}

#[test]
fn an_unreadable_source_is_a_load_error() {
    let d = tempfile::tempdir().unwrap();
    let p = d.path().join("iron_ao.png");
    std::fs::write(&p, b"not a png").unwrap();
    let j = job(
        RecipeKind::Rmaos,
        [ChannelSource::Default { value: 0.5 }, ChannelSource::Default { value: 0.0 }, src(&p, RawMap::Ao, false), ChannelSource::Default { value: 1.0 }],
        &[(&p, RawMap::Ao)],
    );
    assert!(matches!(assemble(&j, GraySource::R), Err(PackError::Load(_))));
}
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo test -p engine --test pack_assemble 2>&1 | tail -5`
Expected: compile error (`engine::pack::assemble` not found).

- [ ] **Step 3: Implement `crates/engine/src/pack/assemble.rs`**

```rust
//! The packed float image of a recipe (spec §6.4 "Sources", "Dimensions", "Precision"). Each source contributes
//! its R channel (luma with `channels.grayscale_source = luma`); its alpha is ignored. Values stay as decoded
//! floats, so 16-bit sources keep their precision until Quantise. Sources are decoded **one at a time**: the peak
//! is the output plus one decoded source, inside the 3x per-file estimate of `api::estimated_bytes`, which reads
//! the first source (every source has its size).
use super::{ChannelSource, PackJob};
use crate::analyse::code8;
use crate::config::GraySource;
use crate::image::{Rgba32F, SourceInfo};
use crate::load::{load_image, LoadError};
use std::path::PathBuf;

#[derive(Debug, thiserror::Error)]
pub enum PackError {
    #[error("{0}")]
    Load(#[from] LoadError),
    #[error("sources differ in size: {}", sizes(.0))]
    SizeMismatch(Vec<(PathBuf, u32, u32)>),
}

/// `"<file> <w>x<h>, …"`; `pub(crate)` so `pack::plan` words its size-mismatch reason the same way.
pub(crate) fn sizes(s: &[(PathBuf, u32, u32)]) -> String {
    s.iter()
        .map(|(p, w, h)| {
            format!(
                "{} {w}x{h}",
                p.file_name().unwrap_or_default().to_string_lossy()
            )
        })
        .collect::<Vec<_>>()
        .join(", ")
}

pub struct Assembled {
    pub image: Rgba32F,
    /// Largest bit depth, any float source, 4 channels; `has_alpha` unless the alpha channel is `Opaque`.
    pub info: SourceInfo,
    /// Sources whose R, G and B differ by more than 4/255 somewhere (`CHAN-NOT-GRAY`).
    pub non_gray: Vec<PathBuf>,
}

/// The width and height every source shares, from the file headers alone.
pub fn check_sizes(sources: &[PathBuf]) -> Result<(u32, u32), PackError> {
    let mut found = vec![];
    for p in sources {
        let (w, h) = image::image_dimensions(p).map_err(|e| LoadError::Decode {
            path: p.clone(),
            msg: e.to_string(),
        })?;
        found.push((p.clone(), w, h));
    }
    let (w, h) = found.first().map_or((0, 0), |f| (f.1, f.2));
    if found.iter().any(|f| (f.1, f.2) != (w, h)) {
        return Err(PackError::SizeMismatch(found));
    }
    Ok((w, h))
}

/// The analysis' grey test (`analyse.rs`): the whole R/G/B spread within 4 codes (same `code8` rounding).
fn grey(q: &[f32; 4]) -> bool {
    let c = |v: f32| i32::from(code8(v));
    let (r, g, b) = (c(q[0]), c(q[1]), c(q[2]));
    r.max(g).max(b) - r.min(g).min(b) <= 4
}

/// Builds the packed image of `job`.
pub fn assemble(job: &PackJob, gray: GraySource) -> Result<Assembled, PackError> {
    let paths: Vec<PathBuf> = job.sources.iter().map(|s| s.path.clone()).collect();
    let (w, h) = check_sizes(&paths)?;
    let mut image = Rgba32F::new(w, h, [0.0, 0.0, 0.0, 1.0]);
    let mut info = SourceInfo {
        bit_depth: 8,
        channels: 4,
        has_alpha: !matches!(job.channels[3], ChannelSource::Opaque),
        is_float: false,
    };
    for (c, ch) in job.channels.iter().enumerate() {
        let value = match ch {
            ChannelSource::Default { value } => Some(*value),
            ChannelSource::Fill { code, .. } => Some(f32::from(*code) / 255.0),
            ChannelSource::Opaque => Some(1.0),
            ChannelSource::Source { .. } | ChannelSource::MirrorR => None,
        };
        if let Some(v) = value {
            for p in &mut image.px {
                p[c] = v;
            }
        }
    }
    let mut non_gray = vec![];
    for s in &job.sources {
        let targets: Vec<(usize, bool)> = job
            .channels
            .iter()
            .enumerate()
            .filter_map(|(c, ch)| match ch {
                ChannelSource::Source { path, invert, .. } if *path == s.path => Some((c, *invert)),
                _ => None,
            })
            .collect();
        let (src, si) = load_image(&s.path)?;
        // `check_sizes` read the headers; a file replaced since then is caught here.
        if (src.width, src.height) != (w, h) {
            return Err(PackError::SizeMismatch(vec![
                (paths[0].clone(), w, h),
                (s.path.clone(), src.width, src.height),
            ]));
        }
        info.bit_depth = info.bit_depth.max(si.bit_depth);
        info.is_float |= si.is_float;
        let mut all_grey = true;
        for (o, q) in image.px.iter_mut().zip(&src.px) {
            all_grey &= grey(q);
            let v = match gray {
                GraySource::R => q[0],
                GraySource::Luma => 0.2126 * q[0] + 0.7152 * q[1] + 0.0722 * q[2],
            };
            for &(c, invert) in &targets {
                o[c] = if invert { 1.0 - v } else { v };
            }
        }
        if !all_grey {
            non_gray.push(s.path.clone());
        }
    }
    let mirrored: Vec<usize> = (0..4)
        .filter(|&c| matches!(job.channels[c], ChannelSource::MirrorR))
        .collect();
    for p in &mut image.px {
        for &c in &mirrored {
            p[c] = p[0];
        }
    }
    Ok(Assembled {
        image,
        info,
        non_gray,
    })
}
```

In `crates/engine/src/pack/mod.rs`, add `pub mod assemble;` directly under the module doc comment.

- [ ] **Step 4: Run the tests to verify they pass**

Run: `cargo test -p engine --test pack_assemble`
Expected: PASS (6 tests).

- [ ] **Step 5: Full check and commit**

Run the full check of Task 1 Step 6.

```bash
git add crates/engine/src/pack/assemble.rs crates/engine/src/pack/mod.rs crates/engine/tests/pack_assemble.rs
git commit -m "pack: assemble the packed image one source at a time (M2.5 Task 4)"
```

---

### Task 5: Packed jobs at run time

**Files:**
- Modify: `crates/engine/src/policy.rs` (`ResolvedJob.pack`; the three `ResolvedJob { … }` literals add `pack: None`)
- Modify: `crates/engine/src/api.rs` (`is_up_to_date`; the `ResolvedJob` literal in its unit test adds `pack: None`)
- Modify: `crates/engine/src/manifest.rs` (`SourceRef::of`)
- Modify: `crates/engine/src/pipeline.rs` (`process_file`)
- Test: `crates/engine/tests/pack_run.rs`

**Interfaces:**
- Consumes: `pack::{PackJob, PackSource, ChannelSource, RecipeKind}` (Task 3), `pack::assemble::assemble` (Task 4).
- Produces: `ResolvedJob.pack: Option<PackJob>` (serialised only when `Some`); `manifest::SourceRef::of(&Path) -> io::Result<SourceRef>`; a packed job's manifest record has `source` = its first source and `sources` = every source with its SHA-256; `is_up_to_date` compares the record's `sources` hashes (in order, not paths) with the job's current ones. The job's `pack.gray` (Task 3) is what the pipeline assembles with; `ResolvedJob` embeds `PackJob`, so the `gray` field flows into `file_planned.job` (a `Value`, no schema) and not into any published schema (the manifest records `sources`, not the job): no schema regeneration in this task.

- [ ] **Step 1: Write the failing tests**

Create `crates/engine/tests/pack_run.rs`:

```rust
//! M2.5 Task 5: a packed job converts from all its sources, lists them in the manifest, is re-encoded when any
//! of them changes and fails when one changed between planning and conversion.
use engine::api::*;
use engine::config::*;
use engine::dds::read_dds;
use engine::decode::{decode_level, Decoded};
use engine::events::*;
use engine::manifest::{sha256_file, Manifest};
use engine::pack::*;
use engine::policy::*;
use engine::presets::PresetRegistry;
use image::{ImageBuffer, Luma};
use std::path::{Path, PathBuf};
use std::sync::Arc;

fn gray(dir: &Path, name: &str, w: u32, f: impl Fn(u32, u32) -> u8) -> PathBuf {
    let p = dir.join("in").join(name);
    std::fs::create_dir_all(p.parent().unwrap()).unwrap();
    ImageBuffer::from_fn(w, w, |x, y| Luma([f(x, y)])).save(&p).unwrap();
    p
}

/// One `_rmaos` job packed from `rough` (R) and `ao` (B), as `Engine::plan` builds it (stamps taken now).
fn plan(dir: &Path, rough: &Path, ao: &Path) -> Plan {
    let registry = PresetRegistry::builtin().unwrap();
    let source = |path: &Path, role| PackSource {
        path: path.to_path_buf(),
        role,
        stamp: SourceStamp::of(path),
    };
    let mut request = BatchRequest::new(vec![dir.join("in")], dir.join("out"), Target::Cs);
    request.on_existing = OnExisting::IfChanged;
    let job = ResolvedJob {
        input: rough.to_path_buf(),
        output: dir.join("out/iron_rmaos.dds"),
        game_path: "iron_rmaos.png".into(),
        set_key: "out/iron".into(),
        profile: registry.profile("cs-quality", Target::Cs, Subtype::Rmaos).unwrap(),
        provenance: Default::default(),
        suggestions: vec![],
        warnings: vec![],
        status: JobStatus::Ready,
        suffix_alias: None,
        source: None,
        pack: Some(PackJob {
            kind: RecipeKind::Rmaos,
            channels: [
                ChannelSource::Source { path: rough.to_path_buf(), role: RawMap::Roughness, invert: false },
                ChannelSource::Default { value: 0.0 },
                ChannelSource::Source { path: ao.to_path_buf(), role: RawMap::Ao, invert: false },
                ChannelSource::Default { value: 1.0 },
            ],
            gray: GraySource::R,
            sources: vec![source(rough, RawMap::Roughness), source(ao, RawMap::Ao)],
        }),
    };
    Plan {
        request,
        registry,
        preset_id: "cs-quality".into(),
        target: Target::Cs,
        jobs: vec![job],
        skipped: vec![],
        failed: vec![],
        peak_admitted_bytes: 0,
        cancelled: vec![],
    }
}

fn run(plan: Plan) -> (BatchSummary, Vec<EngineEvent>) {
    let sink = Arc::new(VecSink::default());
    let rep = Reporter::new(sink.clone(), "pack-run");
    let s = Engine::new().unwrap().run(plan, &rep, &CancelToken::default()).unwrap();
    (s, sink.take())
}

#[test]
fn converts_from_every_source_and_records_them() {
    let d = tempfile::tempdir().unwrap();
    let rough = gray(d.path(), "iron_rough.png", 32, |x, _| x as u8 * 8);
    let ao = gray(d.path(), "iron_ao.png", 32, |_, y| 255 - y as u8 * 8);
    let (s, ev) = run(plan(d.path(), &rough, &ao));
    assert_eq!(s.converted, 1, "{ev:#?}");
    let f = read_dds(&std::fs::read(d.path().join("out/iron_rmaos.dds")).unwrap()).unwrap();
    let (w, h, r) = f.levels[0].clone();
    let Decoded::Ldr(px) = decode_level(f.format, w, h, &f.data[r], None).unwrap() else {
        panic!("LDR expected")
    };
    for (i, p) in px.iter().enumerate() {
        let (x, y) = (i as i32 % 32, i as i32 / 32);
        assert!((i32::from(p[0]) - x * 8).abs() <= 6, "R at {x},{y}: {p:?}");
        assert!(i32::from(p[1]) <= 3, "G at {x},{y}: {p:?}");
        assert!((i32::from(p[2]) - (255 - y * 8)).abs() <= 6, "B at {x},{y}: {p:?}");
    }
    let m = Manifest::at(&d.path().join("out")).load().unwrap();
    let e = m.values().next().unwrap();
    assert_eq!(e.sources.len(), 2);
    assert!(e.sources[0].path.ends_with("iron_rough.png") && e.sources[1].path.ends_with("iron_ao.png"));
    assert_eq!(e.sources[1].sha256, sha256_file(&ao).unwrap());
    assert_eq!(e.source_sha256, sha256_file(&rough).unwrap());
}

/// Spec §6.4 "Provenance": `if-changed` re-encodes when any source changes.
#[test]
fn up_to_date_until_any_source_changes() {
    let d = tempfile::tempdir().unwrap();
    let rough = gray(d.path(), "iron_rough.png", 32, |x, _| x as u8 * 8);
    let ao = gray(d.path(), "iron_ao.png", 32, |_, _| 200);
    assert_eq!(run(plan(d.path(), &rough, &ao)).0.converted, 1);
    let (s, _) = run(plan(d.path(), &rough, &ao));
    assert_eq!((s.converted, s.up_to_date), (0, 1));
    gray(d.path(), "iron_ao.png", 32, |_, _| 100);
    let (s, _) = run(plan(d.path(), &rough, &ao));
    assert_eq!((s.converted, s.up_to_date), (1, 0));
}

/// `if-changed` compares the sources' bytes, in order, like an ordinary file (hash only): re-running the batch
/// with the same files spelled another way (`in/../in/x.png`, absolute, relative) re-encodes nothing.
#[test]
fn a_different_spelling_of_the_same_sources_is_up_to_date() {
    let d = tempfile::tempdir().unwrap();
    let rough = gray(d.path(), "iron_rough.png", 32, |x, _| x as u8 * 8);
    let ao = gray(d.path(), "iron_ao.png", 32, |_, _| 200);
    assert_eq!(run(plan(d.path(), &rough, &ao)).0.converted, 1);
    let respell = |p: &Path| d.path().join("in").join("..").join("in").join(p.file_name().unwrap());
    let (s, _) = run(plan(d.path(), &respell(&rough), &respell(&ao)));
    assert_eq!((s.converted, s.up_to_date), (0, 1));
}

/// Review Focus 3.
#[test]
fn a_source_changed_after_planning_fails_the_packed_job() {
    let d = tempfile::tempdir().unwrap();
    let rough = gray(d.path(), "iron_rough.png", 32, |x, _| x as u8 * 8);
    let ao = gray(d.path(), "iron_ao.png", 32, |_, _| 200);
    let p = plan(d.path(), &rough, &ao);
    gray(d.path(), "iron_ao.png", 64, |_, _| 100);
    let (s, ev) = run(p);
    assert_eq!((s.converted, s.failed), (0, 1));
    let err = ev
        .iter()
        .find_map(|e| match &e.kind {
            EventKind::FileFailed { stage: Stage::Load, error } => Some(error.clone()),
            _ => None,
        })
        .unwrap();
    assert!(err.contains("iron_ao.png") && err.contains("changed since planning"), "{err}");
    assert!(!d.path().join("out/iron_rmaos.dds").exists());
}
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo test -p engine --test pack_run 2>&1 | tail -5`
Expected: compile error (`ResolvedJob` has no field `pack`).

- [ ] **Step 3: Add `ResolvedJob.pack`**

In `crates/engine/src/policy.rs`, add to `ResolvedJob` after `source`:

```rust
    /// How a packed output is built from its raw sources (spec §6.4); `None` for an ordinary file. For a packed
    /// job `input` is `pack.sources[0].path` (DECISIONS Note M8).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pack: Option<crate::pack::PackJob>,
```

Add `pack: None,` to the three `ResolvedJob { … }` literals in `policy.rs` (`packing_input_job`, the error path and the end of `resolve_file`) and to the one in `api.rs`'s unit test `up_to_date_needs_a_comparable_matching_passing_record`.

- [ ] **Step 4: `SourceRef::of` and the pipeline**

In `crates/engine/src/manifest.rs`, below `struct SourceRef`:

```rust
impl SourceRef {
    /// `path` ('/'-separated, as the record's `source`) and its SHA-256.
    pub fn of(path: &Path) -> io::Result<SourceRef> {
        Ok(SourceRef {
            path: path.to_string_lossy().replace('\\', "/"),
            sha256: sha256_file(path)?,
        })
    }
}
```

In `crates/engine/src/pipeline.rs`:
- imports: `use crate::manifest::SourceRef;` (add to the existing `manifest` import list), `use crate::pack::assemble::assemble;`, `use std::path::Path;`.
- Replace the block from `// Final review I7: …` through `let (img, _) = load_image(…)?;` with:

```rust
    // Final review I7: the plan's decisions hold only for the files it analysed; a packed job checks every source.
    let stamps: Vec<(&Path, Option<SourceStamp>)> = match &job.pack {
        Some(pk) => pk
            .sources
            .iter()
            .map(|s| (s.path.as_path(), s.stamp))
            .collect(),
        None => vec![(job.input.as_path(), job.source)],
    };
    for (src, planned) in stamps {
        if let Some(planned) = planned {
            if SourceStamp::of(src) != Some(planned) {
                let what = if job.pack.is_some() {
                    format!("{}: ", src.display())
                } else {
                    String::new()
                };
                return Err(fail(Stage::Load)(format!(
                    "{what}source changed since planning; re-run"
                )));
            }
        }
    }
    let source_sha256 = sha256_file(&job.input).map_err(|e| fail(Stage::Load)(e.to_string()))?;
    // Spec §6.4 "Provenance": a packed output records every source (its first source is `job.input`, Note M8).
    let sources: Vec<SourceRef> = match &job.pack {
        Some(pk) => pk
            .sources
            .iter()
            .map(|s| SourceRef::of(&s.path))
            .collect::<std::io::Result<_>>()
            .map_err(|e| fail(Stage::Load)(e.to_string()))?,
        None => vec![],
    };
    let img = match &job.pack {
        Some(pk) => {
            // The gray source fixed at plan time (Note M6), not `p.channels.grayscale_source` of the final profile.
            assemble(pk, pk.gray)
                .map_err(|e| fail(Stage::Load)(e.to_string()))?
                .image
        }
        None => {
            load_image(&job.input)
                .map_err(|e| fail(Stage::Load)(e.to_string()))?
                .0
        }
    };
```

- In the `ManifestRecord { … }` literal replace `sources: vec![],` with `sources,`.

- [ ] **Step 5: `is_up_to_date` over every source**

In `crates/engine/src/api.rs`, add above `is_up_to_date`:

```rust
/// The SHA-256 of every pack source now, in source order, or none for an ordinary file. `None` when a source
/// cannot be hashed (then the output is not up to date).
fn current_source_hashes(job: &ResolvedJob) -> Option<Vec<String>> {
    match &job.pack {
        None => Some(vec![]),
        Some(pk) => pk.sources.iter().map(|s| sha256_file(&s.path).ok()).collect(),
    }
}
```

In `is_up_to_date`, replace `&& e.sources.is_empty()` with `&& current_source_hashes(job).is_some_and(|h| h.iter().eq(e.sources.iter().map(|r| &r.sha256)))`. Only the hashes are compared, in order, never the recorded paths: like an ordinary file (whose stored `source` string is not compared either), a packed output stays up to date when its sources are spelled another way, and a renamed source with equal bytes is not re-encoded (accepted). `sha256_file` is already imported in `api.rs`; no `SourceRef` import is needed. Update the doc comment: "same source (the bytes of every source, for a packed output), profile, …".

- [ ] **Step 6: Run the tests to verify they pass**

Run: `cargo test -p engine --test pack_run && cargo test -p engine --lib api`
Expected: PASS (the existing `"several sources"` case of `up_to_date_needs_a_comparable_matching_passing_record` still holds: an ordinary job with a record listing sources is not up to date).

- [ ] **Step 7: Full check and commit**

Run the full check of Task 1 Step 6.

```bash
git add crates/engine/src/policy.rs crates/engine/src/api.rs crates/engine/src/manifest.rs crates/engine/src/pipeline.rs crates/engine/tests/pack_run.rs
git commit -m "pipeline: packed jobs load, stamp and hash every source; if-changed over all (M2.5 Task 5)"
```

---
### Task 6: Packing in `Engine::plan`

**Files:**
- Modify: `crates/engine/src/policy.rs` (split `resolve_decided` out of `resolve_file`)
- Modify: `crates/engine/src/events.rs` (`RecipeReport`, `RecipeStatus`, `SetResolved.recipes`)
- Create: `crates/engine/src/pack/plan.rs`; modify `crates/engine/src/pack/mod.rs` (add `pub mod plan;`)
- Modify: `crates/engine/src/api.rs` (`plan`: predicted suffixes, packing passes, `set_resolved.recipes`; `run`: packing-input arm, intentional `PACK-CONFLICT` skips; `PlanFailure::Pack`)
- Regenerate: `schema/event.schema.json`
- Modify tests whose expected behaviour changes (spec §6.4 row 1: a lone raw roughness map now packs): `crates/engine/tests/api.rs` (`packing_inputs_are_reported_but_do_not_count_as_skipped`), `crates/cli/tests/cli_convert.rs` (`raw_packing_inputs_are_reported_but_not_a_failure`), `crates/engine/tests/events_manifest.rs` (the `SetResolved` literal gains `recipes: vec![]`). Rewrite them as given below; do not special-case lone raw maps to keep the old expectation. `crates/engine/tests/suffix_vocabulary.rs` `raw_maps_are_packing_inputs_not_errors` tests `resolve_file` alone and stays unchanged.
- Test: `crates/engine/tests/pack_plan.rs`

**Interfaces:**
- Consumes: Tasks 1–5.
- Produces:
  - `policy::resolve_decided(ctx: &ResolveCtx, f: &FileFacts, d: SubtypeDecision) -> ResolvedJob` (everything `resolve_file` did after the sub-type decision).
  - `events::RecipeReport { output: String, subtype: Subtype, channels: Vec<String>, status: RecipeStatus, reason: Option<String> }`; `events::RecipeStatus { Run, Conflict, Failed, Disabled }`; `EventKind::SetResolved.recipes: Vec<RecipeReport>` (omitted when empty; no `EVENT_SCHEMA` bump, M3 owns the next one).
  - `pack::plan::{raw_role, virtual_input, plan_packing, finish_recipe, Candidate, PackPlan}`. `plan_packing(ctx, sets, files, jobs, failed, cancelled, skipped) -> PackPlan` takes the failed and the cancelled paths of pass 1. The `PackJob` it builds carries the `gray` (`channels.grayscale_source`) of the pass-1 profile; `finish_recipe`, the plan-time assembly and the pipeline (Task 5) all use `PackJob::gray`, so the written image is the analysed one (Note M6). `pack::assemble::sizes` is `pub(crate)` for the size-mismatch text (Task 4).

- [ ] **Step 1: Write the failing tests**

Create `crates/engine/tests/pack_plan.rs`:

```rust
//! M2.5 Task 6: what `Engine::plan` makes of a set's raw maps (spec §6.4, DECISIONS Note M).
use engine::api::*;
use engine::codes::Code;
use engine::config::*;
use engine::events::*;
use engine::overrides::parse_set;
use engine::policy::*;
use engine::presets::Provenance;
use image::{ImageBuffer, Luma, Rgb, Rgba};
use std::collections::BTreeSet;
use std::path::Path;
use std::sync::Arc;

fn put(dir: &Path, name: &str, w: u32, f: impl Fn(u32, u32) -> u8) {
    let p = dir.join("in/textures/a").join(name);
    std::fs::create_dir_all(p.parent().unwrap()).unwrap();
    ImageBuffer::from_fn(w, w, |x, y| Luma([f(x, y)])).save(p).unwrap();
}

fn request(dir: &Path, target: Target, sets: &[&str]) -> BatchRequest {
    let mut req = BatchRequest::new(vec![dir.join("in")], dir.join("out"), target);
    req.recursive = true;
    req.overrides = sets.iter().map(|s| parse_set(s).unwrap()).collect();
    req
}

fn plan_of(req: BatchRequest) -> (Plan, Vec<EngineEvent>) {
    let sink = Arc::new(VecSink::default());
    let rep = Reporter::new(sink.clone(), "pack-plan");
    let p = Engine::new().unwrap().plan(req, &rep, &CancelToken::default()).unwrap();
    (p, sink.take())
}

fn plan(dir: &Path, target: Target, sets: &[&str]) -> (Plan, Vec<EngineEvent>) {
    plan_of(request(dir, target, sets))
}

/// The dry run of a plan: what `convert --dry-run` reports.
fn dry_run(mut p: Plan) -> (BatchSummary, Vec<EngineEvent>) {
    p.request.dry_run = true;
    let sink = Arc::new(VecSink::default());
    let rep = Reporter::new(sink.clone(), "pack-plan");
    let s = Engine::new().unwrap().run(p, &rep, &CancelToken::default()).unwrap();
    (s, sink.take())
}

fn job<'a>(p: &'a Plan, name: &str) -> &'a ResolvedJob {
    p.jobs
        .iter()
        .find(|j| j.input.file_name().is_some_and(|n| n == name))
        .unwrap_or_else(|| panic!("no job for {name}"))
}

fn codes(j: &ResolvedJob) -> Vec<Code> {
    j.warnings.iter().map(|w| w.code).collect()
}

fn recipes(ev: &[EngineEvent]) -> Vec<RecipeReport> {
    ev.iter()
        .flat_map(|e| match &e.kind {
            EventKind::SetResolved { recipes, .. } => recipes.clone(),
            _ => vec![],
        })
        .collect()
}

#[test]
fn a_lone_raw_map_packs_into_rmaos() {
    let d = tempfile::tempdir().unwrap();
    put(d.path(), "iron_roughness.png", 64, |x, _| x as u8 * 4);
    let (p, ev) = plan(d.path(), Target::Cs, &[]);
    assert_eq!(p.jobs.len(), 1, "the consumed raw job leaves the plan");
    let j = job(&p, "iron_roughness.png");
    assert!(j.output.ends_with("textures/a/iron_rmaos.dds"), "{}", j.output.display());
    assert_eq!(j.profile.subtype, Subtype::Rmaos);
    assert_eq!(j.status, JobStatus::Ready);
    assert_eq!(j.provenance["subtype"], Provenance::Recipe("rmaos".into()));
    // Like every job of the batch, the recipe job records where the target came from.
    assert!(j.provenance.contains_key("target"), "{:?}", j.provenance);
    assert_eq!(j.provenance["pack.r"], Provenance::Source("iron_roughness.png:R".into()));
    assert_eq!(j.provenance["pack.g"], Provenance::Default("0".into()));
    assert_eq!(j.provenance["pack.b"], Provenance::Default("1".into()));
    assert_eq!(j.provenance["pack.a"], Provenance::Default("1".into()));
    let defaulted: Vec<&str> = j
        .warnings
        .iter()
        .filter(|w| w.code == Code::PackDefaulted)
        .map(|w| w.data["channel"].as_str().unwrap())
        .collect();
    assert_eq!(defaulted, ["G", "B", "A"]);
    let r = recipes(&ev);
    assert_eq!(r.len(), 1);
    assert_eq!((r[0].status, r[0].subtype), (RecipeStatus::Run, Subtype::Rmaos));
    assert!(r[0].output.ends_with("iron_rmaos.dds"));
    assert_eq!(r[0].channels, ["source:iron_roughness.png:R", "default:0", "default:1", "default:1"]);
}

#[test]
fn unused_raw_maps_are_reported_not_converted() {
    let d = tempfile::tempdir().unwrap();
    put(d.path(), "iron_roughness.png", 64, |x, _| x as u8 * 4);
    put(d.path(), "iron_opacity.png", 64, |_, _| 255);
    let (p, _) = plan(d.path(), Target::Cs, &[]);
    let o = job(&p, "iron_opacity.png");
    assert_eq!(o.status, JobStatus::PackingInput { role: RawMap::Opacity });
    assert_eq!(codes(o), [Code::PackUnusedInput]);
    let (s, ev) = dry_run(p);
    assert_eq!((s.skipped, s.exit_code()), (0, 0));
    assert!(ev.iter().any(|e| matches!(
        e.kind,
        EventKind::FileSkipped { code: Some(Code::PackUnusedInput), .. }
    ) && e.path.as_deref().is_some_and(|p| p.ends_with("iron_opacity.png"))));
}

#[test]
fn packing_can_be_turned_off_per_output() {
    for set in [
        "packing.enabled=false",
        "subtype:rmaos.packing.enabled=false",
        "file:**/iron_rmaos.png.packing.enabled=false",
    ] {
        let d = tempfile::tempdir().unwrap();
        put(d.path(), "iron_roughness.png", 64, |x, _| x as u8 * 4);
        let (p, ev) = plan(d.path(), Target::Cs, &[set]);
        let j = job(&p, "iron_roughness.png");
        assert!(j.pack.is_none(), "{set}");
        assert_eq!(j.status, JobStatus::PackingInput { role: RawMap::Roughness }, "{set}");
        assert_eq!(codes(j), [Code::InPackingInput], "{set}");
        assert!(j.warnings[0].message.contains("packing.enabled = false"), "{set}");
        assert_eq!(recipes(&ev)[0].status, RecipeStatus::Disabled, "{set}");
    }
}

fn prepacked(dir: &Path) {
    let p = dir.join("in/textures/a/iron_rmaos.png");
    ImageBuffer::from_fn(64, 64, |x, _| Rgba([x as u8 * 4, 0, 255, 255])).save(p).unwrap();
}

#[test]
fn a_prepacked_file_wins_by_default() {
    let d = tempfile::tempdir().unwrap();
    prepacked(d.path());
    put(d.path(), "iron_roughness.png", 64, |x, _| x as u8 * 4);
    let (p, ev) = plan(d.path(), Target::Cs, &[]);
    let pre = job(&p, "iron_rmaos.png");
    assert!(pre.pack.is_none());
    assert!(codes(pre).contains(&Code::PackConflict), "{:?}", pre.warnings);
    let raw = job(&p, "iron_roughness.png");
    assert_eq!(codes(raw), [Code::PackUnusedInput]);
    assert!(raw.warnings[0].message.contains("iron_rmaos.png"));
    assert_eq!(recipes(&ev)[0].status, RecipeStatus::Conflict);
}

/// DECISIONS Note M9: with `packing.prefer = raw` the pre-packed file is an intentional skip.
#[test]
fn prefer_raw_replaces_the_prepacked_file() {
    let d = tempfile::tempdir().unwrap();
    prepacked(d.path());
    put(d.path(), "iron_roughness.png", 64, |x, _| x as u8 * 4);
    let (p, ev) = plan(d.path(), Target::Cs, &["packing.prefer=raw"]);
    assert!(job(&p, "iron_roughness.png").pack.is_some());
    assert!(!p.jobs.iter().any(|j| j.input.ends_with("iron_rmaos.png")));
    let sk = p.skipped.iter().find(|s| s.path.ends_with("iron_rmaos.png")).unwrap();
    assert_eq!(sk.code, Some(Code::PackConflict));
    assert_eq!(recipes(&ev)[0].status, RecipeStatus::Conflict);
    let (s, _) = dry_run(p);
    assert_eq!((s.skipped, s.skipped_intentional, s.exit_code()), (1, 1, 0));
}

/// DECISIONS Note M9: a recipe that cannot be packed never costs the user the file they had. With
/// `packing.prefer = raw` and sources of different sizes the pre-packed file stays a job (no `PACK-CONFLICT`
/// skip) and the recipe fails with `PACK-SIZE-MISMATCH`.
#[test]
fn prefer_raw_with_mismatched_sources_keeps_the_prepacked_file() {
    let d = tempfile::tempdir().unwrap();
    prepacked(d.path());
    put(d.path(), "iron_roughness.png", 64, |_, _| 100);
    put(d.path(), "iron_ao.png", 32, |_, _| 200);
    let (p, ev) = plan(d.path(), Target::Cs, &["packing.prefer=raw"]);
    assert!(p.skipped.is_empty(), "{:?}", p.skipped);
    assert!(job(&p, "iron_rmaos.png").pack.is_none());
    assert_eq!(
        job(&p, "iron_roughness.png").status,
        JobStatus::Error { codes: vec![Code::PackSizeMismatch] }
    );
    let r = &recipes(&ev)[0];
    assert_eq!(r.status, RecipeStatus::Failed);
    assert!(r.reason.as_deref().unwrap().contains("iron_rmaos.png"), "{:?}", r.reason);
    let (s, _) = dry_run(p);
    assert_eq!((s.skipped, s.failed, s.exit_code()), (0, 1, 1));
}

/// Review Focus 1, DECISIONS Note M11.
#[test]
fn a_source_that_fails_to_load_fails_its_recipe() {
    let d = tempfile::tempdir().unwrap();
    put(d.path(), "iron_roughness.png", 64, |x, _| x as u8 * 4);
    std::fs::write(d.path().join("in/textures/a/iron_ao.png"), b"not a png").unwrap();
    let (p, ev) = plan(d.path(), Target::Cs, &[]);
    assert!(p.failed.iter().any(|f| f.path.ends_with("iron_ao.png")));
    assert!(p.jobs.is_empty(), "no recipe job and no leftover raw job: {:?}", p.jobs);
    let r = &recipes(&ev)[0];
    assert_eq!(r.status, RecipeStatus::Failed);
    assert!(r.reason.as_deref().unwrap().contains("iron_ao.png"));
    assert_eq!(dry_run(p).0.exit_code(), 1);
}

#[test]
fn sources_of_different_sizes_fail_the_recipe() {
    let d = tempfile::tempdir().unwrap();
    put(d.path(), "iron_roughness.png", 64, |_, _| 100);
    put(d.path(), "iron_ao.png", 32, |_, _| 200);
    let (p, ev) = plan(d.path(), Target::Cs, &[]);
    let j = job(&p, "iron_roughness.png");
    assert_eq!(j.status, JobStatus::Error { codes: vec![Code::PackSizeMismatch] });
    let m = &j.warnings[0].message;
    assert!(m.contains("iron_roughness.png 64x64") && m.contains("iron_ao.png 32x32"), "{m}");
    assert_eq!(recipes(&ev)[0].status, RecipeStatus::Failed);
    let (s, _) = dry_run(p);
    assert_eq!((s.failed, s.exit_code()), (1, 1));
}

/// Review Focus 5.
#[test]
fn a_raw_map_given_a_subtype_leaves_the_recipes() {
    let d = tempfile::tempdir().unwrap();
    put(d.path(), "iron_roughness.png", 64, |x, _| x as u8 * 4);
    put(d.path(), "iron_height.png", 64, |_, y| y as u8 * 4);
    let (p, ev) = plan(d.path(), Target::Cs, &["file:**/iron_height.png.subtype=height"]);
    let h = job(&p, "iron_height.png");
    assert!(h.pack.is_none() && h.output.ends_with("iron_height.dds"));
    assert_eq!(h.profile.subtype, Subtype::Height);
    assert!(job(&p, "iron_roughness.png").pack.is_some());
    let kinds: Vec<Subtype> = recipes(&ev).iter().map(|r| r.subtype).collect();
    assert_eq!(kinds, [Subtype::Rmaos]);
}

/// Spec §6.4 "Set membership", DECISIONS Note M7.
#[test]
fn set_has_rules_see_the_packed_suffix() {
    let d = tempfile::tempdir().unwrap();
    let n = |name: &str| {
        ImageBuffer::from_fn(16, 16, |_, _| Rgb([128u8, 128, 255]))
            .save(d.path().join("in/textures/a").join(name))
            .unwrap()
    };
    put(d.path(), "iron_ao.png", 16, |_, _| 255);
    n("iron_n.png");
    n("steel_n.png");
    let cfg = d.path().join("sdc.toml");
    std::fs::write(
        &cfg,
        "[[rules]]\nmatch = { suffix = \"n\", set_has = \"rmaos\" }\noverrides = { mips = { levels = 3 } }\n",
    )
    .unwrap();
    let mut req = request(d.path(), Target::Cs, &[]);
    req.project_config = Some(cfg);
    let (p, _) = plan_of(req);
    assert_eq!(job(&p, "iron_n.png").profile.mips.levels, MipLevels::Count(3));
    assert_eq!(job(&p, "steel_n.png").profile.mips.levels, MipLevels::Keyword(MipKeyword::Full));
}

/// Review Focus 4, DECISIONS Note M8, final review I5.
#[test]
fn file_planned_paths_stay_unique() {
    let d = tempfile::tempdir().unwrap();
    prepacked(d.path());
    put(d.path(), "iron_roughness.png", 64, |x, _| x as u8 * 4);
    put(d.path(), "iron_height.png", 64, |_, y| y as u8 * 4);
    ImageBuffer::from_fn(16, 16, |_, _| Rgb([128u8, 128, 255]))
        .save(d.path().join("in/textures/a/iron_n.png"))
        .unwrap();
    let (p, ev) = plan(d.path(), Target::Cs, &["packing.prefer=raw"]);
    let planned: Vec<String> = ev
        .iter()
        .filter(|e| matches!(e.kind, EventKind::FilePlanned { .. }))
        .map(|e| e.path.clone().unwrap())
        .collect();
    let unique: BTreeSet<&String> = planned.iter().collect();
    assert_eq!(unique.len(), planned.len(), "{planned:?}");
    assert_eq!(planned.len(), 3, "iron_n, the _rmaos recipe and the _p recipe: {planned:?}");
    for s in &p.skipped {
        let sp = s.path.to_string_lossy().replace('\\', "/");
        assert!(!planned.contains(&sp), "{sp} is both skipped and planned");
    }
}
```

Rewrite `crates/engine/tests/api.rs` `packing_inputs_are_reported_but_do_not_count_as_skipped` as:

```rust
/// Spec §6.4 (M2.5): a lone raw roughness map packs into `_rmaos`; a raw map no recipe uses is reported but does
/// not make the batch partial (DECISIONS Note M4).
#[test]
fn raw_maps_pack_and_unused_ones_do_not_count_as_skipped() {
    let d = tempfile::tempdir().unwrap();
    fixtures(d.path());
    std::fs::remove_file(d.path().join("textures/armor/leaf.png")).unwrap();
    for name in ["stone_roughness.png", "stone_opacity.png"] {
        ImageBuffer::from_fn(64, 64, |x, _| Rgb([x as u8, x as u8, x as u8]))
            .save(d.path().join("textures/armor").join(name))
            .unwrap();
    }
    let (s, ev) = run(request(d.path()));
    assert_eq!((s.converted, s.skipped), (3, 0), "{ev:#?}");
    assert_eq!(s.exit_code(), 0);
    assert!(d.path().join("out/armor/stone_rmaos.dds").exists());
    assert!(ev.iter().any(|e| matches!(
        e.kind,
        EventKind::FileSkipped {
            code: Some(Code::PackUnusedInput),
            ..
        }
    )));
}
```

Rewrite `crates/cli/tests/cli_convert.rs` `raw_packing_inputs_are_reported_but_not_a_failure` as:

```rust
#[test]
fn raw_maps_pack_and_unused_ones_are_not_a_failure() {
    let fx = Fx::new();
    fx.normal("wall_n.png");
    fx.rgb("tile_roughness.png");
    fx.rgb("tile_opacity.png");
    let o = fx.convert(&["--accept-suggestions", "high"]);
    assert_eq!(code(&o), 0, "{}", text(&o.stderr));
    assert!(fx.dds("textures/a/wall_n.dds").exists());
    assert!(fx.dds("textures/a/tile_rmaos.dds").exists());
    assert!(skips(&o).contains(&("tile_opacity.png".into(), Some("PACK-UNUSED-INPUT".into()))));
    assert!(!fx.dds("textures/a/tile_roughness.dds").exists());
    assert!(!fx.dds("textures/a/tile_opacity.dds").exists());
}
```

In `crates/engine/tests/events_manifest.rs`, add `recipes: vec![],` to the `EventKind::SetResolved { … }` literal.

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo test -p engine --test pack_plan 2>&1 | tail -5`
Expected: compile errors (`RecipeReport`, `RecipeStatus` not found; `SetResolved` has no field `recipes`).

- [ ] **Step 3: Split `resolve_decided` out of `resolve_file`**

In `crates/engine/src/policy.rs`, replace the head of `resolve_file` so that it reads:

```rust
/// Resolve one file: sub-type decision, layered profile, §10 checks and status (spec §5.2).
pub fn resolve_file(ctx: &ResolveCtx, f: &FileFacts) -> ResolvedJob {
    let d = decide_subtype(ctx, f);
    // Spec §4.4: a raw map is a packing input unless a rule or --set names its sub-type.
    if let Some(Vocab {
        term,
        role: VocabRole::PackingInput(role),
    }) = vocab_for(f)
    {
        if !matches!(d.provenance, Provenance::User | Provenance::Rule(_)) {
            return packing_input_job(ctx, f, term, *role);
        }
    }
    resolve_decided(ctx, f, d)
}

/// Resolve a file whose sub-type is decided, by `decide_subtype` or by a packing recipe (spec §6.4): layered
/// profile, §10 checks and status.
pub fn resolve_decided(ctx: &ResolveCtx, f: &FileFacts, d: SubtypeDecision) -> ResolvedJob {
    let suffix_alias = alias_use(ctx, f);
    let mut reasons = vec![];
```

and keep the rest of the former `resolve_file` body (from `if !d.decided {` to the end) unchanged as the body of `resolve_decided`.

- [ ] **Step 4: Recipe reports in `set_resolved`**

In `crates/engine/src/events.rs`, add `use crate::config::Subtype;` and, after `pub enum Stage { … }`:

```rust
/// What became of one channel-packing recipe of a set (spec §6.4 "Reporting").
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum RecipeStatus {
    /// Planned as a job.
    Run,
    /// The set already holds this output as a file (`PACK-CONFLICT`); `reason` says which side is used.
    Conflict,
    /// A source could not be read or the sources differ in size.
    Failed,
    /// `packing.enabled = false`: the raw maps stay packing inputs.
    Disabled,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct RecipeReport {
    /// The packed output, '/'-separated.
    pub output: String,
    pub subtype: Subtype,
    /// Provenance of R, G, B and A (`source:…`, `default:…`, `fill:…`); empty for a mirrored or opaque channel.
    pub channels: Vec<String>,
    pub status: RecipeStatus,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
}
```

and extend `EventKind::SetResolved`:

```rust
    SetResolved {
        members: Vec<String>,
        pbr: bool,
        bake: bool,
        /// The set's channel-packing recipes (spec §6.4); absent when it packs nothing.
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        recipes: Vec<RecipeReport>,
    },
```

- [ ] **Step 5: Create `crates/engine/src/pack/plan.rs`**

Add `pub mod plan;` under `pub mod assemble;` in `crates/engine/src/pack/mod.rs`, then:

```rust
//! Plan-time wiring of channel packing (spec §6.4, plan M2.5, DECISIONS Note M). `Engine::plan` calls
//! [`plan_packing`] once every file is resolved (pass 1: recipes, packing settings, conflicts, consumed and unused
//! raw jobs), then assembles, analyses and resolves each [`Candidate`] with [`finish_recipe`] (pass 2).
use super::assemble::{check_sizes, sizes, PackError};
use super::{
    file_name as name, recipes_for, resolve_channels, ChannelSource, PackJob, PackSource, RawInput, RecipeKind,
    Slot,
};
use crate::analyse::{analyse, Analysis};
use crate::api::SkippedFile;
use crate::codes::{Code, Warning};
use crate::config::{GraySource, PackPrefer, RawMap, Subtype};
use crate::discover::{InputFile, InputKind};
use crate::events::{RecipeReport, RecipeStatus};
use crate::group::{split_suffix_with, split_vocab, TextureSet, VocabRole};
use crate::image::{Rgba32F, SourceInfo};
use crate::policy::{
    layered_profile, resolve_decided, FileFacts, JobStatus, ResolveCtx, ResolvedJob, SourceStamp,
    SubtypeDecision,
};
use crate::presets::Provenance;
use serde_json::json;
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

const CHANNELS: [&str; 4] = ["R", "G", "B", "A"];

// `name` is `pack::file_name` (imported above), the file-name text used in every message of this module.

fn slash(p: &Path) -> String {
    p.to_string_lossy().replace('\\', "/")
}

fn list(paths: &[PathBuf]) -> String {
    paths.iter().map(|p| name(p)).collect::<Vec<_>>().join(", ")
}

/// The raw-map role of an input file, from its name (spec §4.4).
pub fn raw_role(f: &InputFile) -> Option<RawMap> {
    if f.kind != InputKind::Image {
        return None;
    }
    let stem = f.rel.file_stem()?.to_string_lossy().into_owned();
    match split_vocab(&stem)?.1.role {
        VocabRole::PackingInput(r) => Some(r),
        _ => None,
    }
}

/// The packed output as an input file: the first source's `path` (so `job.input` is a real file, Note M8), with
/// the output's name `<base>_<suffix>.<ext>` in `rel` and `game_path` (rule globs and `file:` scopes match it)
/// and the packed `output` next to the source's.
pub fn virtual_input(first: &InputFile, kind: RecipeKind) -> InputFile {
    let stem = first
        .rel
        .file_stem()
        .unwrap_or_default()
        .to_string_lossy()
        .into_owned();
    let base = split_vocab(&stem).map_or(stem.clone(), |(b, _)| b);
    let ext = first
        .rel
        .extension()
        .map_or_else(|| "png".to_string(), |e| e.to_string_lossy().into_owned());
    let file = format!("{base}_{}.{ext}", kind.suffix());
    let lower = file.to_lowercase();
    let game_path = match first.game_path.rsplit_once('/') {
        Some((dir, _)) => format!("{dir}/{lower}"),
        None => lower,
    };
    InputFile {
        path: first.path.clone(),
        rel: first.rel.with_file_name(&file),
        game_path,
        output: first
            .output
            .with_file_name(format!("{base}_{}.dds", kind.suffix())),
        kind: InputKind::Image,
    }
}

/// A recipe that passed pass 1: assembled, analysed and resolved in pass 2.
pub struct Candidate {
    pub set_key: String,
    pub kind: RecipeKind,
    pub virtual_input: InputFile,
    /// Carries `gray`, the `channels.grayscale_source` every later step uses (Note M6).
    pub job: PackJob,
    /// Channels that took a neutral default (index, slot): one `PACK-DEFAULTED` each.
    pub defaulted: Vec<(usize, Slot)>,
}

pub struct PackPlan {
    pub candidates: Vec<Candidate>,
    /// Recipe reports per set key, for `set_resolved`.
    pub reports: BTreeMap<String, Vec<RecipeReport>>,
}

/// The analysis pass 1 resolves a packed profile with: the packed image does not exist yet (DECISIONS Note M6).
fn placeholder() -> (Analysis, SourceInfo) {
    let info = SourceInfo {
        bit_depth: 8,
        channels: 4,
        has_alpha: true,
        is_float: false,
    };
    (analyse(&Rgba32F::new(4, 4, [0.5; 4]), &info), info)
}

/// Replaces a waiting raw job's packing-input warning (`policy::packing_input_job` gives it exactly one).
fn retag(jobs: &mut [ResolvedJob], path: &Path, code: Code, message: String) {
    if let Some(j) = jobs
        .iter_mut()
        .find(|j| j.input == path && matches!(j.status, JobStatus::PackingInput { .. }))
    {
        let data = j.warnings.first().map(|w| w.data.clone()).unwrap_or_default();
        j.warnings = vec![Warning::new(code, message).with_data(data)];
    }
}

/// A planned non-raw image member that already provides `kind`'s output (spec §6.4 "Pre-packed conflict",
/// DECISIONS Note M9).
fn prepacked_member<'a>(
    ctx: &ResolveCtx,
    set: &TextureSet,
    files: &'a [InputFile],
    jobs: &[ResolvedJob],
    kind: RecipeKind,
) -> Option<&'a InputFile> {
    set.members.iter().map(|&i| &files[i]).find(|f| {
        let stem = f
            .rel
            .file_stem()
            .unwrap_or_default()
            .to_string_lossy()
            .into_owned();
        f.kind == InputKind::Image
            && raw_role(f).is_none()
            && split_suffix_with(&stem, ctx.registry.alias_table())
                .suffix
                .as_deref()
                == Some(kind.suffix())
            && jobs.iter().any(|j| j.input == f.path)
    })
}

/// A recipe whose sources differ in size (spec §6.4 "Dimensions"): an error job, so `run` fails it.
fn size_mismatch_job(
    ctx: &ResolveCtx,
    vin: &InputFile,
    set: &TextureSet,
    st: Subtype,
    found: &[(PathBuf, u32, u32)],
) -> ResolvedJob {
    let text = sizes(found);
    let data: Vec<_> = found
        .iter()
        .map(|(p, w, h)| json!({ "path": slash(p), "width": w, "height": h }))
        .collect();
    ResolvedJob {
        input: vin.path.clone(),
        output: vin.output.clone(),
        game_path: vin.game_path.clone(),
        set_key: set.key.clone(),
        profile: ctx.registry.base_profile(ctx.target, st),
        provenance: BTreeMap::new(),
        suggestions: vec![],
        warnings: vec![Warning::new(
            Code::PackSizeMismatch,
            format!(
                "sources of {} differ in size: {text}; no resizing in v1",
                name(&vin.output)
            ),
        )
        .with_data(json!({ "sources": data }))],
        status: JobStatus::Error {
            codes: vec![Code::PackSizeMismatch],
        },
        suffix_alias: None,
        source: None,
        pack: None,
    }
}

/// Pass 1 over every set. `jobs` holds every resolved job of the batch and is changed in place: consumed raw jobs
/// are removed, unused ones re-tagged `PACK-UNUSED-INPUT` (`IN-PACKING-INPUT` when their recipe is disabled), a
/// pre-packed job gets `PACK-CONFLICT` or (with `packing.prefer = raw`) moves to `skipped`, and a recipe whose
/// sources differ in size becomes an error job. `failed` holds the paths that failed planning and `cancelled` those
/// whose Load never started because the batch was cancelled: a recipe with a member in either is `Failed` (Note M11).
pub fn plan_packing(
    ctx: &ResolveCtx,
    sets: &[TextureSet],
    files: &[InputFile],
    jobs: &mut Vec<ResolvedJob>,
    failed: &[PathBuf],
    cancelled: &[PathBuf],
    skipped: &mut Vec<SkippedFile>,
) -> PackPlan {
    let (pa, pinfo) = placeholder();
    let mut out = PackPlan {
        candidates: vec![],
        reports: BTreeMap::new(),
    };
    let mut consumed: BTreeSet<PathBuf> = BTreeSet::new();
    for set in sets {
        // Raw members still waiting for a recipe: a planned packing input, or one that failed to load or was
        // cancelled before its Load. A raw map whose sub-type a rule or `--set` named is an ordinary file
        // (spec §4.4) and takes no part.
        let mut raw = vec![];
        let mut members: BTreeMap<PathBuf, &InputFile> = BTreeMap::new();
        for &i in &set.members {
            let f = &files[i];
            let Some(role) = raw_role(f) else { continue };
            let waiting = jobs.iter().any(|j| {
                j.input == f.path && matches!(j.status, JobStatus::PackingInput { .. })
            });
            if waiting || failed.contains(&f.path) || cancelled.contains(&f.path) {
                raw.push(RawInput {
                    path: f.path.clone(),
                    role,
                });
                members.insert(f.path.clone(), f);
            }
        }
        if raw.is_empty() {
            continue;
        }
        let (recipes, unused) = recipes_for(ctx.target, &raw);
        let mut unused: Vec<(PathBuf, String)> =
            unused.into_iter().map(|u| (u.path, u.reason)).collect();
        let mut reports = vec![];
        for recipe in recipes {
            let sources: Vec<PathBuf> = recipe.sources().into_iter().map(|(p, _)| p).collect();
            let vin = virtual_input(members[&sources[0]], recipe.kind);
            let st = recipe.kind.subtype();
            // Pass 1: the packed output's own profile decides `packing.*` (DECISIONS Note M6).
            let facts = FileFacts {
                input: &vin,
                suffix: Some(recipe.kind.suffix()),
                alias: None,
                set,
                analysis: &pa,
                info: &pinfo,
            };
            let profile = layered_profile(ctx, &facts, st)
                .map(|(p, _)| p)
                .unwrap_or_else(|_| ctx.registry.base_profile(ctx.target, st));
            let (packing, gray) = (profile.packing, profile.channels.grayscale_source);
            let (channels, defaulted) = resolve_channels(&recipe, &packing);
            let mut report = RecipeReport {
                output: slash(&vin.output),
                subtype: st,
                channels: channels
                    .iter()
                    .map(|c| c.provenance(gray).map(|p| p.to_string()).unwrap_or_default())
                    .collect(),
                status: RecipeStatus::Run,
                reason: None,
            };
            // DECISIONS Note M11: a source that failed to load fails the recipe. So does one whose Load was
            // cancelled: the report must not claim `default:` channels for sources that exist.
            if let Some(bad) = sources.iter().find(|p| failed.contains(p)) {
                report.status = RecipeStatus::Failed;
                report.reason = Some(format!("source {} could not be read", name(bad)));
                reports.push(report);
                consumed.extend(sources.iter().cloned());
                continue;
            }
            if sources.iter().any(|p| cancelled.contains(p)) {
                report.status = RecipeStatus::Failed;
                report.reason = Some("cancelled".into());
                reports.push(report);
                consumed.extend(sources.iter().cloned());
                continue;
            }
            if !packing.enabled {
                report.status = RecipeStatus::Disabled;
                report.reason = Some("packing.enabled = false".into());
                reports.push(report);
                for p in &sources {
                    retag(
                        jobs,
                        p,
                        Code::InPackingInput,
                        format!(
                            "{}: packing is off for {} (packing.enabled = false); a raw map is not converted on its own",
                            name(p),
                            name(&vin.output)
                        ),
                    );
                }
                continue;
            }
            // Headers only. Checked before the conflict below so that a recipe which cannot be packed never
            // replaces the pre-packed file the user had (DECISIONS Note M9: never write neither).
            let mismatch = match check_sizes(&sources) {
                Err(PackError::SizeMismatch(found)) => Some(found),
                _ => None,
            };
            let mut kept = None;
            if let Some(pre) = prepacked_member(ctx, set, files, jobs, recipe.kind) {
                let pre = pre.path.clone();
                report.status = RecipeStatus::Conflict;
                if packing.prefer == PackPrefer::Prepacked {
                    report.reason = Some(format!(
                        "{} is used (packing.prefer = prepacked)",
                        name(&pre)
                    ));
                    reports.push(report);
                    if let Some(j) = jobs.iter_mut().find(|j| j.input == pre) {
                        j.warnings.push(Warning::new(
                            Code::PackConflict,
                            format!(
                                "{} and the raw maps {} both make {}; the pre-packed file is used (packing.prefer = prepacked)",
                                name(&pre),
                                list(&sources),
                                name(&vin.output)
                            ),
                        ));
                    }
                    for p in &sources {
                        unused.push((
                            p.clone(),
                            format!("the pre-packed {} is used (packing.prefer = prepacked)", name(&pre)),
                        ));
                    }
                    continue;
                }
                if mismatch.is_some() {
                    // The recipe fails below; the pre-packed job stays: no `retain`, no `PACK-CONFLICT` skip.
                    kept = Some(name(&pre));
                } else {
                    report.reason =
                        Some(format!("{} is replaced (packing.prefer = raw)", name(&pre)));
                    jobs.retain(|j| j.input != pre);
                    skipped.push(SkippedFile {
                        path: pre.clone(),
                        set: Some(set.key.clone()),
                        code: Some(Code::PackConflict),
                        reason: format!(
                            "replaced by {} packed from {} (packing.prefer = raw)",
                            name(&vin.output),
                            list(&sources)
                        ),
                    });
                }
            }
            consumed.extend(sources.iter().cloned());
            if let Some(found) = mismatch {
                report.status = RecipeStatus::Failed;
                report.reason = Some(match kept {
                    Some(pre) => format!("sources differ in size; {pre} is kept (packing.prefer = raw)"),
                    None => "sources differ in size".into(),
                });
                reports.push(report);
                jobs.push(size_mismatch_job(ctx, &vin, set, st, &found));
                continue;
            }
            reports.push(report);
            let job = PackJob {
                kind: recipe.kind,
                channels,
                gray,
                sources: recipe
                    .sources()
                    .into_iter()
                    .map(|(path, role)| PackSource {
                        stamp: SourceStamp::of(&path),
                        path,
                        role,
                    })
                    .collect(),
            };
            out.candidates.push(Candidate {
                set_key: set.key.clone(),
                kind: recipe.kind,
                virtual_input: vin,
                job,
                defaulted,
            });
        }
        for (p, reason) in unused {
            if !failed.contains(&p) {
                retag(
                    jobs,
                    &p,
                    Code::PackUnusedInput,
                    format!("{}: {reason}; not converted", name(&p)),
                );
            }
        }
        out.reports.insert(set.key.clone(), reports);
    }
    // DECISIONS Note M8: the recipe job stands for the raw jobs it consumed.
    jobs.retain(|j| {
        !(consumed.contains(&j.input) && matches!(j.status, JobStatus::PackingInput { .. }))
    });
    out
}

/// Pass 2 for one candidate: resolve the assembled image as the packed sub-type (provenance `recipe:<kind>`),
/// then record the channel provenance (`pack.r` …), one `PACK-DEFAULTED` per defaulted channel and one
/// `CHAN-NOT-GRAY` per non-grey source. The caller adds `provenance["target"]`, as for every job. Everything about
/// the packed channels uses `c.job.gray`, never the final profile's `channels.grayscale_source` (Note M6).
pub fn finish_recipe(
    ctx: &ResolveCtx,
    c: &Candidate,
    set: &TextureSet,
    analysis: &Analysis,
    info: &SourceInfo,
    non_gray: &[PathBuf],
) -> ResolvedJob {
    let facts = FileFacts {
        input: &c.virtual_input,
        suffix: Some(c.kind.suffix()),
        alias: None,
        set,
        analysis,
        info,
    };
    let decision = SubtypeDecision {
        subtype: c.kind.subtype(),
        provenance: Provenance::Recipe(c.kind.id().into()),
        decided: true,
        suggestions: vec![],
    };
    let mut job = resolve_decided(ctx, &facts, decision);
    for (ch, src) in ["r", "g", "b", "a"].iter().zip(&c.job.channels) {
        if let Some(p) = src.provenance(c.job.gray) {
            job.provenance.insert(format!("pack.{ch}"), p);
        }
    }
    let out = name(&c.virtual_input.output);
    for &(i, slot) in &c.defaulted {
        if let ChannelSource::Default { value } = &c.job.channels[i] {
            job.warnings.push(
                Warning::new(
                    Code::PackDefaulted,
                    format!(
                        "{out} {}: no {} source; neutral default {value}",
                        CHANNELS[i],
                        slot.id()
                    ),
                )
                .with_data(json!({
                    "output": slash(&c.virtual_input.output),
                    "channel": CHANNELS[i],
                    "slot": slot,
                    "value": value,
                })),
            );
        }
    }
    for p in non_gray {
        job.warnings.push(Warning::new(
            Code::ChanNotGray,
            format!(
                "{}: R/G/B differ by more than 4/255; packing its {}",
                name(p),
                match c.job.gray {
                    GraySource::R => "R",
                    GraySource::Luma => "luma",
                }
            ),
        ));
    }
    job.pack = Some(c.job.clone());
    job
}
```

- [ ] **Step 6: Wire both passes into `Engine::plan` and update `run`**

In `crates/engine/src/api.rs`:

1. Imports: add `use crate::events::RecipeStatus;` and `use crate::pack;`.
2. `enum PlanFailure` gains a variant:

```rust
    /// Assembling a packed image failed other than by a load error (spec §6.4): a source changed size after
    /// pass 1. A load error of a source is a `PlanFailure::Load`, handled like a pass-1 load failure.
    Pack(String),
```

   and the first result loop's `match r` gains the arm `Err(PlanFailure::Pack(_)) => unreachable!("pass 1 never assembles"),` (needed for exhaustiveness only). Factor the existing inline `match e { LoadError::TooLarge { .. } => Code::DimTooLarge, LoadError::Decode { .. } => Code::InDecode }` of that loop into `fn load_code(e: &LoadError) -> Code` next to `PlanFailure`, and call it there; pass 2 below uses it too.
3. Make the group result mutable and predict packed suffixes right after grouping (before `set_of`):

```rust
        let (mut sets, dups) = group_sets_with(&files, registry.alias_table());
        // Spec §6.4 "Set membership", DECISIONS Note M7: a recipe's output joins its set before anything resolves.
        for set in &mut sets {
            let roles: BTreeSet<RawMap> = set
                .members
                .iter()
                .filter_map(|&i| pack::plan::raw_role(&files[i]))
                .collect();
            for s in pack::predicted_suffixes(target, &roles) {
                set.suffixes.insert(s.to_string());
            }
        }
```

4. Delete `let peak_admitted_bytes = budget.peak.load(Ordering::Relaxed);` where it is now, and after the loop that collects `jobs`, `failed` and `cancelled` insert:

```rust
        // Spec §6.4 (M2.5): channel packing. Pass 1 decides the recipes; pass 2 assembles, analyses and resolves
        // each under the same memory admission and panic isolation as the files above.
        let failed_paths: Vec<PathBuf> = failed.iter().map(|f| f.path.clone()).collect();
        let mut packed = pack::plan::plan_packing(
            &ctx,
            &sets,
            &files,
            &mut jobs,
            &failed_paths,
            &cancelled,
            &mut skipped,
        );
        // This admission/spawn/`catch_unwind`/slot block mirrors the pass-1 block above (`resolve_one` and its
        // loop) on purpose: the two differ in the work item, the stage they start at and what they return, so one
        // helper would need as many parameters as it saves lines.
        let pack_slots: Vec<Mutex<Option<Result<ResolvedJob, PlanFailure>>>> =
            packed.candidates.iter().map(|_| Mutex::new(None)).collect();
        pool.in_place_scope(|scope| {
            for (c, slot) in packed.candidates.iter().zip(&pack_slots) {
                if cancel.is_cancelled() {
                    continue;
                }
                let est = estimated_bytes(&c.job.sources[0].path);
                budget.acquire(est);
                let admitted = Admitted(&budget, est);
                let (ctx, sets) = (&ctx, &sets);
                scope.spawn(move |_| {
                    if cancel.is_cancelled() {
                        return;
                    }
                    let stage = Cell::new(Stage::Load);
                    let r = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                        // `load_image` enforces MAX_DIM per source, so DIM-TOO-LARGE needs nothing in `check_sizes`.
                        let asm = pack::assemble::assemble(&c.job, c.job.gray).map_err(|e| match e {
                            pack::assemble::PackError::Load(e) => PlanFailure::Load(e),
                            e => PlanFailure::Pack(e.to_string()),
                        })?;
                        stage.set(Stage::Analyse);
                        let a = analyse(&asm.image, &asm.info);
                        let (info, non_gray) = (asm.info, asm.non_gray);
                        drop(asm.image);
                        drop(admitted);
                        stage.set(Stage::Resolve);
                        let set = sets
                            .iter()
                            .find(|s| s.key == c.set_key)
                            .expect("a candidate's set is planned");
                        Ok(pack::plan::finish_recipe(ctx, c, set, &a, &info, &non_gray))
                    }));
                    *lock(slot) = Some(r.unwrap_or_else(|panic| {
                        Err(PlanFailure::Panic(
                            stage.get(),
                            format!("internal error: {}", panic_message(&*panic)),
                        ))
                    }));
                });
            }
        });
        for (c, slot) in packed.candidates.iter().zip(pack_slots) {
            let path = c.job.sources[0].path.clone();
            let Some(r) = slot.into_inner().unwrap_or_else(|e| e.into_inner()) else {
                cancelled.push(path);
                continue;
            };
            match r {
                Ok(mut job) => {
                    // As `resolve_one` does for every other job; `finish_recipe` cannot (it has no `target_src`).
                    job.provenance.insert("target".into(), target_src.clone());
                    jobs.push(job);
                }
                Err(f) => {
                    let (stage, error) = match f {
                        PlanFailure::Load(e) => {
                            // The same §10 warning as a pass-1 load failure, so a JSON consumer sees the code;
                            // it names the source that failed, the `file_failed` below names the recipe's input.
                            let (LoadError::TooLarge { path: bad, .. } | LoadError::Decode { path: bad, .. }) =
                                &e;
                            rep.emit(
                                None,
                                Some(bad),
                                EventKind::Warning(Warning::new(load_code(&e), e.to_string())),
                            );
                            (Stage::Load, e.to_string())
                        }
                        PlanFailure::Pack(e) => (Stage::Load, e),
                        PlanFailure::Panic(stage, e) => (stage, e),
                    };
                    let output = slash(&c.virtual_input.output);
                    if let Some(report) = packed
                        .reports
                        .get_mut(&c.set_key)
                        .and_then(|v| v.iter_mut().find(|r| r.output == output))
                    {
                        report.status = RecipeStatus::Failed;
                        report.reason = Some(error.clone());
                    }
                    failed.push(FailedFile { path, stage, error });
                }
            }
        }
        let peak_admitted_bytes = budget.peak.load(Ordering::Relaxed);
```

5. In the `set_resolved` emission add the field `recipes: packed.reports.remove(&set.key).unwrap_or_default(),`.
6. In `run`, replace the `JobStatus::PackingInput { role } => …` arm with:

```rust
                // Spec §4.4/§6.4 (DECISIONS Note M4): a raw map that is not packed is reported, with its
                // IN-PACKING-INPUT or PACK-UNUSED-INPUT warning, but does not make the batch partial.
                JobStatus::PackingInput { role } => {
                    let w = job
                        .warnings
                        .iter()
                        .find(|w| matches!(w.code, Code::InPackingInput | Code::PackUnusedInput));
                    rep.emit(
                        set,
                        path,
                        EventKind::FileSkipped {
                            code: Some(w.map_or(Code::InPackingInput, |w| w.code)),
                            reason: w.map_or_else(
                                || format!("raw {} map, not converted on its own", role.id()),
                                |w| w.message.clone(),
                            ),
                        },
                    )
                }
```

7. In `run`'s loop over `plan.skipped`, count `PACK-CONFLICT` skips as intentional (DECISIONS Note M9):

```rust
            // IN-DUPLICATE is a conflict the user must resolve; IN-DDS-SKIPPED (Note I) and a pre-packed file
            // replaced under `packing.prefer = raw` (Note M9) are deliberate.
            if matches!(s.code, Some(Code::InDdsSkipped | Code::PackConflict)) {
                summary.skipped_intentional += 1;
            }
```

8. Update the doc comments that the new behaviour makes stale:
   - `Plan::jobs` (`api.rs`): "Every resolved file, plus one job per packed output (spec §6.4); raw maps consumed by a recipe are not listed."
   - `Plan::skipped` (`api.rs`): add "and `PACK-CONFLICT` (a pre-packed file replaced under `packing.prefer = raw`)" to its list of what it holds.
   - `BatchSummary::skipped_intentional` (`api.rs`) and `EventKind::BatchDone.skipped_intentional` (`events.rs`): add `PACK-CONFLICT` next to `IN-DDS-SKIPPED`. The `events.rs` one is schema-bearing, which is why it is fixed here, before the regeneration in Step 7.
   - `JobStatus::PackingInput` and `packing_input_job` (`policy.rs`): replace "milestone M2.5 consumes these" with "a raw map waiting for `pack::plan::plan_packing`, which consumes it into a recipe or re-tags it `PACK-UNUSED-INPUT`".

- [ ] **Step 7: Regenerate the event schema and run the tests**

Run: `cargo run -q -p sdc -- presets schema all --out schema && cargo test -p engine --test pack_plan --test api --test events_manifest --test schemas && cargo test -p sdc --test cli_convert raw_maps_pack`
Expected: PASS.

- [ ] **Step 8: Full check and commit**

Run the full check of Task 1 Step 6.

```bash
git add crates/engine/src/policy.rs crates/engine/src/events.rs crates/engine/src/pack/plan.rs crates/engine/src/pack/mod.rs crates/engine/src/api.rs schema/event.schema.json crates/engine/tests/pack_plan.rs crates/engine/tests/api.rs crates/engine/tests/events_manifest.rs crates/cli/tests/cli_convert.rs
git commit -m "plan: pack raw maps into recipe jobs; set_resolved reports recipes (M2.5 Task 6)"
```

---
### Task 7: CLI `--fill` and recipes in `inspect`

**Files:**
- Modify: `crates/cli/src/args.rs` (`BatchArgs.fill`, `parse_fill`, `build_request`)
- Modify: `crates/cli/src/verbs.rs` (`print_event`)
- Test: `crates/cli/tests/cli_convert.rs`, `crates/cli/tests/cli_inspect_validate.rs`

**Interfaces:**
- Consumes: field paths `packing.fill.<role>` (Task 1), `RecipeReport` (Task 6).
- Produces: `sdc convert|inspect --fill <role>=<0..255>` (repeatable); `args::parse_fill(&str) -> Result<SetOverride, String>`; `inspect` human output lines `set <key>`, `  pack <output> (<subtype>) <status>[: reason]`, `    R <prov>  G <prov>  B <prov>  A <prov>`.

- [ ] **Step 1: Write the failing tests**

Append to `crates/cli/tests/cli_convert.rs`:

```rust
/// DECISIONS Note M5: `--fill` writes a packed channel that has no source; it is provenance `fill:<v>` and not a
/// defaulted channel.
#[test]
fn fill_sets_a_packed_channel_without_a_source() {
    let fx = Fx::new();
    fx.rgb("tile_ao.png");
    let o = fx.convert(&["--fill", "metallic=255"]);
    assert_eq!(code(&o), 0, "{}", text(&o.stderr));
    let job = &planned(&o)["tile_ao.png"];
    assert_eq!(job["provenance"]["pack.g"], "fill:255");
    assert_eq!(job["profile"]["packing"]["fill"]["metallic"], 255);
    assert_eq!(
        job["pack"]["channels"][1],
        serde_json::json!({ "from": "fill", "role": "metallic", "code": 255 })
    );
    let defaulted: Vec<String> = events(&o)
        .iter()
        .filter(|e| e["kind"] == "warning" && e["code"] == "PACK-DEFAULTED")
        .map(|e| e["data"]["channel"].as_str().unwrap().to_string())
        .collect();
    assert_eq!(defaulted, ["R", "A"]);
    assert!(fx.dds("textures/a/tile_rmaos.dds").exists());
}

#[test]
fn fill_rejects_unknown_roles_and_bad_values() {
    let fx = Fx::new();
    fx.rgb("tile_ao.png");
    for (arg, needle) in [
        ("metalic=1", "unknown role"),
        ("metallic=256", "0..255"),
        ("metallic=-1", "0..255"),
        ("metallic", "expected <role>=<0..255>"),
    ] {
        let o = fx.convert(&["--fill", arg]);
        assert_eq!(code(&o), 2, "{arg}");
        assert!(text(&o.stderr).contains(needle), "{arg}: {}", text(&o.stderr));
    }
}

/// DECISIONS Note M5: `--fill` ranks after every `--set`, so it beats an unscoped `--set` of the same field.
#[test]
fn fill_beats_an_unscoped_set_of_the_same_field() {
    let fx = Fx::new();
    fx.rgb("tile_ao.png");
    let o = fx.convert(&["--fill", "metallic=200", "--set", "packing.fill.metallic=10"]);
    assert_eq!(code(&o), 0, "{}", text(&o.stderr));
    assert_eq!(planned(&o)["tile_ao.png"]["provenance"]["pack.g"], "fill:200");
}
```

Append to `crates/cli/tests/cli_inspect_validate.rs` (it imports `image::{ImageBuffer, Rgb}`; add `Luma` if missing):

```rust
#[test]
fn inspect_prints_the_packing_recipes_of_a_set() {
    let d = tempfile::tempdir().unwrap();
    ImageBuffer::from_fn(16, 16, |x, _| Luma([x as u8 * 16]))
        .save(d.path().join("iron_roughness.png"))
        .unwrap();
    let o = sdc().arg("inspect").arg(d.path()).output().unwrap();
    assert_eq!(code(&o), 0, "{}", text(&o.stderr));
    let out = text(&o.stdout);
    assert!(out.contains("pack iron_rmaos.dds (rmaos) run"), "{out}");
    assert!(
        out.contains("R source:iron_roughness.png:R")
            && out.contains("G default:0")
            && out.contains("A default:1"),
        "{out}"
    );
}
```

- [ ] **Step 2: Run the tests to verify they fail**

Run (one name filter per command; cargo takes only one):
`cargo test -p sdc --test cli_convert fill_ 2>&1 | tail -15`
`cargo test -p sdc --test cli_inspect_validate inspect_prints_the_packing 2>&1 | tail -15`
Expected: FAIL (`--fill` is an unknown argument: exit 2 with clap's message; inspect prints no `pack` line).

- [ ] **Step 3: Implement `--fill`**

In `crates/cli/src/args.rs`:
- imports: `use engine::config::{RawMap, Target};` (replace the `Target` import) and `use engine::overrides::{parse_set, SetOverride};`.
- `BatchArgs`, after `set`:

```rust
    /// Fill value for a packed channel without a source map: <role>=<0..255>, e.g. metallic=255 (repeatable). Same
    /// as `--set packing.fill.<role>=<value>`, applied after every `--set`.
    #[arg(long = "fill", value_name = "ROLE=VALUE")]
    pub fill: Vec<String>,
```

- above `build_request`:

```rust
/// `--fill <role>=<0..255>` (spec §6.4, DECISIONS Note M5): an unscoped `packing.fill.<role>` override.
pub fn parse_fill(s: &str) -> Result<SetOverride, String> {
    let (role, value) = s
        .split_once('=')
        .ok_or_else(|| format!("--fill '{s}': expected <role>=<0..255>"))?;
    let role = role.trim().to_ascii_lowercase();
    if !RawMap::ALL.iter().any(|r| r.id() == role) {
        let roles: Vec<String> = RawMap::ALL.iter().map(|r| r.id()).collect();
        return Err(format!(
            "--fill '{s}': unknown role '{role}' (one of {})",
            roles.join(", ")
        ));
    }
    let code: u8 = value
        .trim()
        .parse()
        .map_err(|_| format!("--fill '{s}': the value must be a whole number 0..255"))?;
    parse_set(&format!("packing.fill.{role}={code}"))
}
```

- in `build_request`, right after `r.overrides = …?;`:

```rust
    // DECISIONS Note M5: fills rank after every --set (the same precedence group as an unscoped --set, later).
    for f in &b.fill {
        r.overrides.push(parse_fill(f)?);
    }
```

- in the `tests` module of `args.rs`:

```rust
    #[test]
    fn fill_is_an_unscoped_packing_override() {
        let o = parse_fill("Metallic=255").unwrap();
        assert_eq!(o.field, "packing.fill.metallic");
        assert_eq!(o.value, serde_json::json!(255));
        assert!(parse_fill("metal=1").unwrap_err().contains("unknown role"));
        assert!(parse_fill("ao=300").unwrap_err().contains("0..255"));
    }
```

- [ ] **Step 4: Print recipes in `inspect`**

In `crates/cli/src/verbs.rs` `print_event`, add an arm before `_ => {}`:

```rust
        EventKind::SetResolved { recipes, .. } if !recipes.is_empty() => {
            println!("set {}", ev.set.as_deref().unwrap_or(""));
            for r in recipes {
                let status = serde_json::to_value(r.status)
                    .ok()
                    .and_then(|v| v.as_str().map(str::to_string))
                    .unwrap_or_default();
                let reason = r.reason.as_deref().map(|x| format!(": {x}")).unwrap_or_default();
                println!("  pack {} ({}) {status}{reason}", r.output, r.subtype.id());
                let channels: Vec<String> = ["R", "G", "B", "A"]
                    .iter()
                    .zip(&r.channels)
                    .filter(|(_, p)| !p.is_empty())
                    .map(|(c, p)| format!("{c} {p}"))
                    .collect();
                println!("    {}", channels.join("  "));
            }
        }
```

- [ ] **Step 5: Run the tests to verify they pass**

Run: `cargo test -p sdc --test cli_convert fill_ && cargo test -p sdc --test cli_inspect_validate inspect_prints_the_packing && cargo test -p sdc --bin sdc fill_is`
Expected: PASS.

- [ ] **Step 6: Full check and commit**

Run the full check of Task 1 Step 6.

```bash
git add crates/cli/src/args.rs crates/cli/src/verbs.rs crates/cli/tests/cli_convert.rs crates/cli/tests/cli_inspect_validate.rs
git commit -m "cli: --fill <role>=<0..255>; inspect prints packing recipes (M2.5 Task 7)"
```

---

### Task 8: Golden cases (M2.5 exit)

**Files:**
- Test: `crates/engine/tests/packing.rs` (create)

**Interfaces:**
- Consumes: everything above, through `Engine::plan` + `Engine::run` only (no internal types).

Each test is one bullet of the M2.5 exit list in `docs/superpowers/plans/2026-10-08-skyrim-dds-converter-phase0-2.md`, plus the Spike S8 grey `_m` case and `if-changed`. "Bit-exact before encode" for the reference packing is proven by Task 4's `channels_come_from_sources_and_constants`; here the decoded output is compared with the reference within BC tolerance.

- [ ] **Step 1: Write the tests**

Create `crates/engine/tests/packing.rs`:

```rust
//! M2.5 exit (plan §M2.5): golden cases for channel packing through `Engine::plan` + `Engine::run`. Fixtures are
//! generated; nothing is committed but this file.
use engine::api::*;
use engine::codes::{Code, Warning};
use engine::config::Target;
use engine::dds::{read_dds, DdsFile, DdsPixelFormat, HeaderKind};
use engine::decode::{decode_level, Decoded};
use engine::events::*;
use engine::manifest::{sha256_file, Manifest};
use engine::overrides::parse_set;
use engine::policy::AcceptSuggestions;
use engine::validate::ValidationSummary;
use image::{ImageBuffer, Luma, Rgba};
use std::path::Path;
use std::sync::Arc;

const W: u32 = 64;

struct Out {
    summary: BatchSummary,
    events: Vec<EngineEvent>,
}

fn convert_with(dir: &Path, target: Target, sets: &[&str], on_existing: OnExisting) -> Out {
    let mut req = BatchRequest::new(vec![dir.join("in")], dir.join("out"), target);
    req.recursive = true;
    req.accept = AcceptSuggestions::High;
    req.overrides = sets.iter().map(|s| parse_set(s).unwrap()).collect();
    req.on_existing = on_existing;
    let sink = Arc::new(VecSink::default());
    let rep = Reporter::new(sink.clone(), "packing");
    let e = Engine::new().unwrap();
    let plan = e.plan(req, &rep, &CancelToken::default()).unwrap();
    let summary = e.run(plan, &rep, &CancelToken::default()).unwrap();
    Out { summary, events: sink.take() }
}

fn convert(dir: &Path, target: Target, sets: &[&str]) -> Out {
    convert_with(dir, target, sets, OnExisting::Overwrite)
}

/// An 8-bit grey source at `in/textures/a/<name>`.
fn put(dir: &Path, name: &str, f: impl Fn(u32, u32) -> u8) {
    put_sized(dir, name, W, f)
}

fn put_sized(dir: &Path, name: &str, w: u32, f: impl Fn(u32, u32) -> u8) {
    let p = dir.join("in/textures/a").join(name);
    std::fs::create_dir_all(p.parent().unwrap()).unwrap();
    ImageBuffer::from_fn(w, w, |x, y| Luma([f(x, y)])).save(p).unwrap();
}

fn is_of(e: &EngineEvent, suffix: &str) -> bool {
    e.path.as_deref().is_some_and(|p| p.ends_with(suffix))
}

fn warnings(o: &Out) -> Vec<Code> {
    o.events
        .iter()
        .filter_map(|e| match &e.kind {
            EventKind::Warning(w) => Some(w.code),
            _ => None,
        })
        .collect()
}

fn warnings_of(o: &Out, suffix: &str) -> Vec<Warning> {
    o.events
        .iter()
        .filter_map(|e| match &e.kind {
            EventKind::Warning(w) if is_of(e, suffix) => Some(w.clone()),
            _ => None,
        })
        .collect()
}

fn defaulted(o: &Out, suffix: &str) -> Vec<String> {
    warnings_of(o, suffix)
        .iter()
        .filter(|w| w.code == Code::PackDefaulted)
        .map(|w| w.data["channel"].as_str().unwrap().to_string())
        .collect()
}

fn planned(o: &Out, suffix: &str) -> serde_json::Value {
    o.events
        .iter()
        .find_map(|e| match &e.kind {
            EventKind::FilePlanned { job } if is_of(e, suffix) => Some(job.clone()),
            _ => None,
        })
        .unwrap_or_else(|| panic!("no file_planned for {suffix}"))
}

fn validation_of(o: &Out, suffix: &str) -> ValidationSummary {
    o.events
        .iter()
        .find_map(|e| match &e.kind {
            EventKind::FileDone { validation, .. } if is_of(e, suffix) => Some(validation.clone()),
            _ => None,
        })
        .unwrap_or_else(|| panic!("no file_done for {suffix}"))
}

fn skip_code(o: &Out, suffix: &str) -> Option<Code> {
    o.events
        .iter()
        .find_map(|e| match &e.kind {
            EventKind::FileSkipped { code, .. } if is_of(e, suffix) => Some(*code),
            _ => None,
        })
        .unwrap_or_else(|| panic!("no file_skipped for {suffix}"))
}

fn out_path(dir: &Path, name: &str) -> std::path::PathBuf {
    dir.join("out/textures/a").join(name)
}

fn dds(dir: &Path, name: &str) -> DdsFile {
    read_dds(&std::fs::read(out_path(dir, name)).unwrap()).unwrap()
}

fn levels(f: &DdsFile) -> Vec<Vec<[u8; 4]>> {
    f.levels
        .iter()
        .map(|(w, h, r)| match decode_level(f.format, *w, *h, &f.data[r.clone()], None).unwrap() {
            Decoded::Ldr(px) => px,
            Decoded::Hdr(_) => panic!("expected an LDR format"),
        })
        .collect()
}

fn assert_dds(f: &DdsFile, format: DdsPixelFormat, header: HeaderKind, mips: u32) {
    assert_eq!((f.format, f.header_kind, f.mip_count), (format, header, mips));
}

/// Every texel of `px` within `tol` of `want(x, y)` on channel `c`.
fn assert_channel(px: &[[u8; 4]], c: usize, tol: i32, want: impl Fn(u32, u32) -> u8) {
    for (i, p) in px.iter().enumerate() {
        let (x, y) = (i as u32 % W, i as u32 / W);
        let d = (i32::from(p[c]) - i32::from(want(x, y))).abs();
        assert!(d <= tol, "channel {c} at {x},{y}: {} vs {}", p[c], want(x, y));
    }
}

fn r(x: u32, _: u32) -> u8 {
    (x * 4) as u8
}
fn m(x: u32, _: u32) -> u8 {
    (255 - x * 4) as u8
}
fn ao(x: u32, _: u32) -> u8 {
    (128 + x) as u8
}
fn spec(x: u32, _: u32) -> u8 {
    (40 + x * 2) as u8
}

fn full_set(dir: &Path) {
    put(dir, "iron_roughness.png", r);
    put(dir, "iron_metallic.png", m);
    put(dir, "iron_ao.png", ao);
    put(dir, "iron_specular.png", spec);
}

/// Exit: cs roughness + metallic + AO + specular → `_rmaos` equal to a reference packing; the manifest lists
/// every source.
#[test]
fn cs_full_set_packs_rmaos_like_a_reference_packing() {
    let d = tempfile::tempdir().unwrap();
    full_set(d.path());
    let o = convert(d.path(), Target::Cs, &[]);
    assert_eq!(o.summary.converted, 1, "{:?}", warnings(&o));
    assert!(defaulted(&o, "iron_roughness.png").is_empty());
    let f = dds(d.path(), "iron_rmaos.dds");
    assert_dds(&f, DdsPixelFormat::Bc7 { srgb: false }, HeaderKind::Dx10, 7);
    let px = &levels(&f)[0];
    assert_channel(px, 0, 4, r);
    assert_channel(px, 1, 4, m);
    assert_channel(px, 2, 4, ao);
    assert_channel(px, 3, 4, spec);
    assert!(validation_of(&o, "iron_roughness.png").passed);
    let man = Manifest::at(&d.path().join("out")).load().unwrap();
    let e = &man["textures/a/iron_rmaos.dds"];
    let names: Vec<&str> = e.sources.iter().map(|s| s.path.rsplit('/').next().unwrap()).collect();
    assert_eq!(names, ["iron_roughness.png", "iron_metallic.png", "iron_ao.png", "iron_specular.png"]);
    for s in &e.sources {
        let p = d.path().join("in/textures/a").join(s.path.rsplit('/').next().unwrap());
        assert_eq!(s.sha256, sha256_file(&p).unwrap());
    }
}

/// Exit: AO-only set → roughness 0.5, metallic 0, specular 1, one PACK-DEFAULTED per channel, `default:<v>`.
#[test]
fn ao_only_set_defaults_three_channels() {
    let d = tempfile::tempdir().unwrap();
    put(d.path(), "iron_ao.png", ao);
    let o = convert(d.path(), Target::Cs, &[]);
    assert_eq!(o.summary.converted, 1, "{:?}", warnings(&o));
    assert_eq!(defaulted(&o, "iron_ao.png"), ["R", "G", "A"]);
    let job = planned(&o, "iron_ao.png");
    assert_eq!(job["provenance"]["pack.r"], "default:0.5");
    assert_eq!(job["provenance"]["pack.g"], "default:0");
    assert_eq!(job["provenance"]["pack.a"], "default:1");
    let px = &levels(&dds(d.path(), "iron_rmaos.dds"))[0];
    assert_channel(px, 0, 2, |_, _| 128);
    assert_channel(px, 1, 2, |_, _| 0);
    assert_channel(px, 2, 4, ao);
    assert_channel(px, 3, 2, |_, _| 255);
}

/// Exit: the same set with `--fill metallic=255` (here its `--set` form) → metallic 255 with `fill:255`, the
/// other channels unchanged.
#[test]
fn a_metallic_fill_replaces_the_default() {
    let d = tempfile::tempdir().unwrap();
    put(d.path(), "iron_ao.png", ao);
    let o = convert(d.path(), Target::Cs, &["packing.fill.metallic=255"]);
    assert_eq!(defaulted(&o, "iron_ao.png"), ["R", "A"]);
    assert_eq!(planned(&o, "iron_ao.png")["provenance"]["pack.g"], "fill:255");
    let px = &levels(&dds(d.path(), "iron_rmaos.dds"))[0];
    assert_channel(px, 1, 2, |_, _| 255);
    assert_channel(px, 0, 2, |_, _| 128);
}

/// Exit: fuzz set → `_rfaos`; metallic next to fuzz is unused.
#[test]
fn a_fuzz_set_packs_rfaos() {
    let d = tempfile::tempdir().unwrap();
    put(d.path(), "iron_roughness.png", r);
    put(d.path(), "iron_fuzz.png", ao);
    put(d.path(), "iron_metallic.png", m);
    let o = convert(d.path(), Target::Cs, &[]);
    assert!(out_path(d.path(), "iron_rfaos.dds").exists(), "{:?}", warnings(&o));
    assert!(!out_path(d.path(), "iron_rmaos.dds").exists());
    assert_eq!(skip_code(&o, "iron_metallic.png"), Some(Code::PackUnusedInput));
    assert_channel(&levels(&dds(d.path(), "iron_rfaos.dds"))[0], 1, 4, ao);
}

/// Exit: height → `_p` BC4; a 16-bit source reports its bit-depth loss like any file.
#[test]
fn height_packs_p_as_bc4() {
    let d = tempfile::tempdir().unwrap();
    let p = d.path().join("in/textures/a/iron_height.png");
    std::fs::create_dir_all(p.parent().unwrap()).unwrap();
    ImageBuffer::from_fn(W, W, |x, _| Luma([(x * 1024) as u16])).save(&p).unwrap();
    let o = convert(d.path(), Target::Cs, &[]);
    let f = dds(d.path(), "iron_p.dds");
    assert_dds(&f, DdsPixelFormat::Bc4, HeaderKind::Dx10, 7);
    assert_channel(&levels(&f)[0], 0, 3, |x, _| ((x * 1024) as f32 / 65535.0 * 255.0).round() as u8);
    assert!(warnings_of(&o, "iron_height.png").iter().any(|w| w.code == Code::LossBitdepth));
}

/// Exit: gloss-only cs set → roughness = 1 − gloss.
#[test]
fn gloss_only_cs_set_inverts_into_roughness() {
    let d = tempfile::tempdir().unwrap();
    put(d.path(), "iron_gloss.png", r);
    let o = convert(d.path(), Target::Cs, &[]);
    assert_eq!(planned(&o, "iron_gloss.png")["provenance"]["pack.r"], "source:1-iron_gloss.png:R");
    assert_channel(&levels(&dds(d.path(), "iron_rmaos.dds"))[0], 0, 4, |x, y| 255 - r(x, y));
}

/// Exit: cpm → complex `_m` whose smallest-mip alpha stays below CS's complex gate (Spike S8).
#[test]
fn cpm_packs_a_complex_m_below_the_gate() {
    let d = tempfile::tempdir().unwrap();
    put(d.path(), "iron_roughness.png", r);
    put(d.path(), "iron_metallic.png", |_, _| 40);
    put(d.path(), "iron_height.png", |_, y| (y * 2) as u8);
    let o = convert(d.path(), Target::Cpm, &[]);
    assert_eq!(o.summary.converted, 1, "{:?}", warnings(&o));
    let f = dds(d.path(), "iron_m.dds");
    assert_dds(&f, DdsPixelFormat::Bc7 { srgb: false }, HeaderKind::Dx10, 7);
    let lv = levels(&f);
    assert!(lv.last().unwrap()[0][3] < 251, "{:?}", lv.last());
    let w = warnings(&o);
    assert!(!w.contains(&Code::ValMComplexMargin) && !w.contains(&Code::ValMGrayscale), "{w:?}");
    assert_eq!(planned(&o, "iron_metallic.png")["profile"]["subtype"], "complex_material");
}

/// Spike S8: a packed cpm `_m` whose RGB is grey at the smallest mip is not complex in CS; VAL-M-GRAYSCALE says so.
#[test]
fn a_grey_packed_cpm_m_is_flagged() {
    let d = tempfile::tempdir().unwrap();
    put(d.path(), "iron_metallic.png", |_, _| 128);
    put(d.path(), "iron_glossiness.png", |_, _| 128);
    let o = convert(d.path(), Target::Cpm, &[]);
    assert_eq!(o.summary.converted, 1);
    assert!(warnings(&o).contains(&Code::ValMGrayscale), "{:?}", warnings(&o));
}

/// Exit: vanilla metallic → `_m` env mask (BC4); a height-only vanilla set → PACK-UNUSED-INPUT, nothing written.
#[test]
fn vanilla_packs_an_env_mask_only_from_metallic_or_specular() {
    let d = tempfile::tempdir().unwrap();
    put(d.path(), "iron_metallic.png", m);
    put(d.path(), "steel_height.png", r);
    let o = convert(d.path(), Target::Vanilla, &[]);
    let f = dds(d.path(), "iron_m.dds");
    assert_dds(&f, DdsPixelFormat::Bc4, HeaderKind::Dx10, 7);
    assert_channel(&levels(&f)[0], 0, 3, m);
    assert_eq!(skip_code(&o, "steel_height.png"), Some(Code::PackUnusedInput));
    assert!(!out_path(d.path(), "steel_m.dds").exists());
    assert_eq!(o.summary.exit_code(), 0);
}

/// Exit: size mismatch → PACK-SIZE-MISMATCH naming both files.
#[test]
fn sources_of_different_sizes_fail_with_both_named() {
    let d = tempfile::tempdir().unwrap();
    put_sized(d.path(), "iron_roughness.png", 64, |_, _| 100);
    put_sized(d.path(), "iron_ao.png", 32, |_, _| 200);
    let o = convert(d.path(), Target::Cs, &[]);
    let w = warnings_of(&o, "iron_roughness.png");
    let mm = w.iter().find(|w| w.code == Code::PackSizeMismatch).unwrap();
    assert!(mm.message.contains("iron_roughness.png 64x64") && mm.message.contains("iron_ao.png 32x32"));
    assert!(!out_path(d.path(), "iron_rmaos.dds").exists());
    assert_eq!((o.summary.failed, o.summary.exit_code()), (1, 1));
}

/// Exit: pre-packed conflict → PACK-CONFLICT and the pre-packed output.
#[test]
fn a_prepacked_file_is_kept_with_a_conflict_warning() {
    let d = tempfile::tempdir().unwrap();
    let p = d.path().join("in/textures/a/iron_rmaos.png");
    std::fs::create_dir_all(p.parent().unwrap()).unwrap();
    ImageBuffer::from_fn(W, W, |x, _| Rgba([(x * 4) as u8, 0, 255, 255])).save(&p).unwrap();
    put(d.path(), "iron_roughness.png", m);
    let o = convert(d.path(), Target::Cs, &[]);
    assert!(warnings_of(&o, "iron_rmaos.png").iter().any(|w| w.code == Code::PackConflict));
    assert_eq!(skip_code(&o, "iron_roughness.png"), Some(Code::PackUnusedInput));
    let man = Manifest::at(&d.path().join("out")).load().unwrap();
    let e = &man["textures/a/iron_rmaos.dds"];
    assert!(e.sources.is_empty());
    assert_eq!(e.source_sha256, sha256_file(&p).unwrap());
    // The pre-packed R ramp, not the raw (inverted-looking) roughness.
    assert_channel(&levels(&dds(d.path(), "iron_rmaos.dds"))[0], 0, 4, r);
}

/// Spec §6.4 "Provenance": `if-changed` keeps a current packed output and re-encodes when any source changes.
#[test]
fn if_changed_follows_every_source() {
    let d = tempfile::tempdir().unwrap();
    full_set(d.path());
    let run = || convert_with(d.path(), Target::Cs, &[], OnExisting::IfChanged).summary;
    assert_eq!(run().converted, 1);
    let s = run();
    assert_eq!((s.converted, s.up_to_date), (0, 1));
    put(d.path(), "iron_specular.png", |x, _| (200 - x) as u8);
    let s = run();
    assert_eq!((s.converted, s.up_to_date), (1, 0));
}
```

- [ ] **Step 2: Run the tests**

Run: `cargo test -p engine --test packing`
Expected: PASS (12 tests). A failure here is a defect in Tasks 1–7: fix it in the code (with a focused test in that task's test file) rather than loosening a tolerance; report any tolerance you believe is wrong instead of changing it.

- [ ] **Step 3: Full check and commit**

Run the full check of Task 1 Step 6.

```bash
git add crates/engine/tests/packing.rs
git commit -m "test: M2.5 golden cases for channel packing (M2.5 Task 8)"
```

---

### Task 9: Docs

**Files:**
- Modify: `docs/superpowers/specs/2026-10-08-skyrim-dds-converter-design.md`
- Modify: `docs/superpowers/plans/2026-10-08-skyrim-dds-converter-phase0-2.md` (M2.5 section)

No code changes. Each edit cites the task or DECISIONS Note M item it reflects.

- [ ] **Step 1: Spec edits**

1. §3.2 profile field table: the `packing` row already exists (with `fill` and a `**[Proposal]**` for `packing.fill.<role>`). Update it in place, do not add a second one: change that `**[Proposal]**` tag to `**[Decided]** (plan M2.5, DECISIONS Note M5)`, keep its text (field `packing.fill.<role>`, a raw-map role id of §4.4, 8-bit code 0–255, `--fill <role>=<0..255>`), and replace "arrives with milestone M2.5" with "shipped in M2.5".
2. §4.4 "Raw maps before M2.5" row: rename to "Raw maps not packed" and state: a raw map consumed by a recipe has no job of its own (Note M8); one whose recipe is off stays `packing_input` with `IN-PACKING-INPUT`; one that no recipe uses is `packing_input` with `PACK-UNUSED-INPUT`; neither makes the batch exit 1 (Note M4).
3. §6.4: change the `**[Proposal]**` tags of the "Fill values" paragraph and of the "Rules" table to `**[Decided]** (plan M2.5, DECISIONS Note M)`; set the "Env mask" default row's tag to `**[Decided]** (Note M1)` and its value to "metallic, else specular; under `cpm` without either: `fill.metallic`, else `fill.specular`, else 0"; add a row to the "Rules" table: "Packing settings | Read from the packed output's profile resolved with a placeholder analysis (Note M6); a rule matching on `alpha` cannot change them. The same profile's `channels.grayscale_source` is the one used to assemble the image (stored in the job at plan time)." and one: "Duplicate roles | The first raw map of a role by path is used; others are `PACK-UNUSED-INPUT` (Note M13)."
4. §8 CLI: the `--fill <role>=<0..255>` (repeatable) option is already listed in the options line ("from M2.5"). Do not add it again: align the sentence that describes it so it says "an unscoped `--set packing.fill.<role>` applied after every `--set` (Note M5)" and drop any "from M2.5" qualifier that says it is still to come.
5. §10 codes table: the four rows `PACK-DEFAULTED`, `PACK-SIZE-MISMATCH`, `PACK-CONFLICT` and `PACK-UNUSED-INPUT` already exist. Do not add rows: remove the "(M2.5)" marker from their code cells and align the text with Note M: `PACK-CONFLICT` also covers the `packing.prefer = raw` skip of the pre-packed file (an intentional skip, Note M9) and `PACK-SIZE-MISMATCH` lists every source and size.
6. §5.9 events: `set_resolved` carries `recipes` (output, sub-type, channel provenance, status `run`/`conflict`/`failed`/`disabled`, reason).
7. §14 #18, #19, #20, #21, #22: the table has no status column; closed items carry "Closed" in the **Owner** column and "Decided: …" in **Shipped default**. In each of these rows set **Owner** to "Closed (DECISIONS Note M1 / M2 / M3 / M4 / M5 respectively)" and prefix **Shipped default** with "Decided: ", keeping the existing text.
8. §2 module table, the `pack` (M2.5) row: change "Between Group and Resolve" to "After Resolve (pass 1, Note M6), then around the packed output's own Resolve (pass 2: assemble, analyse, resolve)". The plan resolves every file first, plans recipes from the result, then resolves each packed output.

- [ ] **Step 2: Plan edit**

In the phase 0–2 plan's `### M2.5 — Channel packing` section, add as its first line: "Expanded into `docs/superpowers/plans/2026-10-09-sdc-m2-5-channel-packing.md` (Tasks 1–9); decisions in DECISIONS Note M."

- [ ] **Step 3: Check and commit**

Run: `grep -n "PACK-UNUSED-INPUT\|Note M" docs/superpowers/specs/2026-10-08-skyrim-dds-converter-design.md | head` (expect the new rows) and `cargo test --workspace 2>&1 | grep -E "test result|FAILED"` (docs only: still green).

```bash
git add docs/superpowers/specs/2026-10-08-skyrim-dds-converter-design.md docs/superpowers/plans/2026-10-08-skyrim-dds-converter-phase0-2.md
git commit -m "docs: spec and plan synced with M2.5 channel packing (M2.5 Task 9)"
```
