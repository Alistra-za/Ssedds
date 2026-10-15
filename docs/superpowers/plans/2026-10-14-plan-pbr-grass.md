# PBR Grass Material Type Implementation Plan

> Note (2026-10-15): after main was merged into nif/main, this work's DECISIONS note is **Note AL** (main already had a Note AG). Read every "Note AG" / "AG1-AG7" below as AL / AL1-AL7. AL8 records that the grass subsurface default was dropped in favour of main's auto rule.

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** sdc gets a `pbr_grass` set shader (CS only) that a PBR set used by a GRAS record picks automatically. Its PGPatcher form shows only the keys Community Shaders' grass shader reads. It warns that PGPatcher 1.2.0 does not patch grass meshes and when the albedo would trip CS's complex-grass detector, and its 3D preview says it is approximated.

**Architecture:** The engine's shader table gets one row (`Shader::PbrGrass`, reusing the PBR sub-types) and one auto rule fed by a new `BatchRequest::gras_all_diffuses` set. The GUI fills that set from a new modscan query: the existing GRAS slot-0 query without its TruePBR filter. The GUI keeps the writer untouched. `model::sets::build_views` gives a `pbr_grass` set a "grass view" of its PGPatcher settings (hidden keys dropped, subsurface defaulted), and the form, export checks and writer all read that view. Two warning codes come from the engine: a set note on every `pbr_grass` pick, and a job note from a new `Analysis.corner_err`.

**Tech Stack:** Rust 2021 workspace `sdc` (engine, modscan with rusqlite, sdc-gui on egui 0.36 / eframe, `egui_kittest` UI tests, schemars schemas regenerated through the `sdc` CLI).

**Spec:** `F:\ai\dds\nifparseref\plan\pbr-grass\2026-10-14-pbr-grass-design.md` (design approved by the owner in chat 2026-10-14; rulings R1–R9 there). Evidence: `F:\ai\dds\nifparseref\plan\pbr-grass\01-research.md`.

## Global Constraints

- Branch `nif/k-pbrgrass` from `nif/main` at `b4c228e`, in the worktree `F:\ai\dds\converter-nif-pbrgrass` (Task 1, Step 0). Every command below runs in that folder, in Git Bash.
- Every cargo command sets `CARGO_TARGET_DIR=F:/ai/dds/converter-nif-p1/target` (written out in each command below). `.cargo/config.toml` sets the build target, so artefacts land in `F:\ai\dds\converter-nif-p1\target\x86_64-pc-windows-msvc\release\`. Use `--release` throughout, as the other `nif/*` plans do.
- Never push. Never merge to `main`. Never touch `F:\ai\dds\converter` (no checkout, cargo or git command there).
- No new crates or dependencies: `Cargo.lock` stays unchanged.
- Owner-approved edits to otherwise off-limits files: `crates/engine/src/{shader.rs,policy.rs,api.rs,codes.rs}`, `schema/`, `crates/gui/src/pgp/`, `crates/gui/src/sets_doc.rs`, `crates/gui/src/model/sets.rs`, `crates/gui/src/model/pbr_export.rs`, `docs/DECISIONS.md` (one note). This plan ends up not needing `policy.rs`, `sets_doc.rs`, `pbr_export.rs` or `pgp/writer.rs`.
- **`crates/engine/src/analyse.rs` is NOT in the approved list.** Task 3 touches it (one field), in its own commit, only after the owner says yes (spec R1). Without that yes, skip Task 3 entirely; the rest of the plan does not depend on it.
- Still off-limits: `crates/cli`, `crates/encoders-sys`, golden tests and `gui_repro` tests. Running the `sdc` CLI binary to regenerate `schema/` is allowed (it does not edit `crates/cli`). If a golden or `gui_repro` test fails, stop and ask the owner; do not change it.
- `docs/DECISIONS.md`: one new note, **Note AG** (`nif/main` ends at AD; `main` at `3be490a` already has AE and AF), appended at the end in one commit (Task 7).
- Shared files are add-only (no reordering or reformatting), each in its own commit: root `Cargo.toml`, `Cargo.lock`, `crates/gui/Cargo.toml`, `crates/gui/src/ui/mod.rs`, `crates/gui/src/model/mod.rs`, `crates/gui/src/app.rs`, `crates/gui/src/settings.rs`, and `crates/gui/src/worker.rs`. This plan touches only `model/mod.rs` and `worker.rs` (Task 4, Step 5).
- Keep engine edits small and additive: the converter team is working in `crates/engine` and the GUI. New match arms, new functions and new fields only; no renames or moves.
- Tests never read owner data except under `#[ignore]` plus the `SDC_ACHERUS` env var, and never open the real `%LOCALAPPDATA%\sdc\scan.db`. (This plan adds no owner-data test.)
- A scan-DB schema change bumps `modscan::store::SCHEMA_VERSION` (currently 7). This plan changes no table or column, so it stays 7.
- The snippets below are written for meaning; rustfmt owns layout. Run `CARGO_TARGET_DIR=F:/ai/dds/converter-nif-p1/target cargo fmt --all` before every gate.
- Gate before the last commit of every task: `cargo fmt --all --check`, `cargo clippy --workspace --all-targets --release -- -D warnings`, `cargo test --workspace --release` (all with the `CARGO_TARGET_DIR` above).
- Commit only explicit paths (`git add <path>…`, never `-A` or `.`). Every commit message ends with `Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>`.
- Never run `python -` or `python3 -` with stdin.

## Review Focus

- A scan whose GRAS mesh is already TruePBR-patched (the meshes111 kind): the set must become `pbr_grass` while complex grass gets no evidence. Owner: Task 4, test `a_pbr_set_whose_albedo_a_truepbr_gras_shape_uses_picks_pbr_grass`.
- An existing `pbr` set with emissive and `uv_scale` switched to `pbr_grass` must export neither and must not keep an emissive block the form no longer lets the user fix. Switching back restores both. Owner: Task 5, test `a_pbr_grass_set_drops_the_keys_grass_ignores`.
- A user who unticks Subsurface on a PBR grass set must see it stay off; the default must not switch it back on. Owner: Task 5, tests `the_grass_view_drops_hidden_keys_and_defaults_subsurface_with_an_s` and `a_pbr_grass_panel_shows_only_the_grass_keys`.
- An explicit `pbr` choice on a set a GRAS record uses must stay `pbr`, with no grass warning. Owner: Task 1, test `an_explicit_pbr_wins_over_gras_evidence`; Task 2, test `only_a_pbr_grass_set_gets_the_pgpatcher_note`; Task 3, test `an_albedo_that_reads_as_complex_grass_warns_only_on_pbr_grass`.
- A `pbr_grass` set under the vanilla or CPM target must fall back to `default` and say what it wanted. Owner: Task 1, test `vanilla_and_cpm_fall_back_from_pbr_grass`.

---

## File Structure

| File | Status | Responsibility | Conflict risk vs `main` |
|---|---|---|---|
| `crates/engine/src/shader.rs` | modify (approved) | `Shader::PbrGrass`, `PBR_GRASS` spec, lists, `AutoRule::PbrGrass`, `PickInput.gras`, `pick_for_set` param, `PGPATCHER_GRASS` note, `pbr_grass_albedo_note` | medium |
| `crates/engine/src/api.rs` | modify (approved) | `BatchRequest::gras_all_diffuses`, `pick_for_set` call, step 6c detect note | medium |
| `crates/engine/src/codes.rs` | modify (approved) | `PBR-GRASS-PGPATCHER`, `PBR-GRASS-DETECT` | medium |
| `crates/engine/src/analyse.rs` | modify (**owner question**) | `Analysis.corner_err` | low |
| `crates/engine/tests/pbr_grass.rs` | create | plan-level tests | none |
| `schema/*.json` | regenerate (approved) | `pbr_grass` and the two codes | medium (regenerate after merges) |
| `crates/modscan/src/plugin_store.rs` | modify (ours) | `gras_all_diffuse_textures` | none |
| `crates/modscan/tests/gras_diffuse.rs` | modify (ours) | query tests | none |
| `crates/gui/src/model/meshes.rs` | modify (ours) | `gras_all_diffuses(db)` | none |
| `crates/gui/src/worker.rs` | modify (shared, add-only) | `MeshEvidence.gras_all` | low |
| `crates/gui/src/model/mod.rs` | modify (shared, add-only) | evidence plumbing | **high** |
| `crates/gui/src/pgp/schema.rs`, `crates/gui/src/pgp/settings.rs` | modify (approved) | `grass_ignores`, `grass_view` | low |
| `crates/gui/src/model/sets.rs` | modify (approved) | `is_pbr_grass`, grass view in `build_views` | low |
| `crates/gui/src/ui/pgp_fields.rs` | modify (sdc team's file, not off-limits) | hide filter, Subsurface off stored as `false` | low |
| `crates/gui/src/model/set_preview.rs`, `crates/gui/src/ui/set_preview.rs` | modify (ours) | `SetFingerprint.grass`, `GRASS_APPROX` badge | none |
| `crates/gui/tests/{complex_grass,pgp,pbr_export,set_editor_ui,sets_model,set_preview_model,set_preview_ui}.rs` | modify | tests (none is golden or `gui_repro`) | low |
| `docs/gui.md`, `docs/DECISIONS.md` | modify | user guide section, Note AG | low / medium |

---

### Task 1: The `pbr_grass` shader and its auto-pick (engine)

**Files:**
- Modify: `crates/engine/src/shader.rs` (enum `Shader`, statics after `PBR`, `VANILLA`, `Shader::ALL`/`id`/`label`/`available`/`spec`, `AutoRule`, `PickInput`, `pick`, `pick_for_set`, tests)
- Modify: `crates/engine/src/api.rs:97-105` (field), `:142-144` (`new`), `:1134-1141` (`pick_for_set` call)
- Create: `crates/engine/tests/pbr_grass.rs`
- Modify: `crates/gui/tests/sets_model.rs:986` (dropdown count)
- Regenerate: `schema/*.json`

**Interfaces:**
- Consumes: nothing new.
- Produces:
  - `engine::shader::Shader::PbrGrass` (`id() == "pbr_grass"`, `label() == "PBR grass"`), `Shader::ALL: [Shader; 14]` with `PbrGrass` at index 1.
  - `engine::shader::AutoRule::PbrGrass` (`id() == "pbr_grass"`).
  - `engine::shader::PickInput { …, pub gras: bool, … }`.
  - `pub fn pick_for_set(ctx: &ResolveCtx, set: &TextureSet, files: &[InputFile], jobs: &JobIndex, explicit: Option<Shader>, mesh: &BTreeMap<String, Vec<Shader>>, gras: &BTreeSet<String>) -> ShaderPick`.
  - `engine::api::BatchRequest::gras_all_diffuses: BTreeSet<String>` (`grass::path_key` keys).

- [ ] **Step 0: Create the worktree**

```bash
git -C F:/ai/dds/converter-nif-p1 worktree add F:/ai/dds/converter-nif-pbrgrass -b nif/k-pbrgrass b4c228e
cd F:/ai/dds/converter-nif-pbrgrass && git log --oneline -1
```

Expected: `b4c228e Merge nif/j-cspbr into nif/main: …`.

- [ ] **Step 1: Write the failing unit tests**

In `crates/engine/src/shader.rs`, in `mod tests`:

(a) In the helper `fn input<'a>(…) -> PickInput<'a>`, add `gras: false,` after `pbr: false,`:

```rust
        PickInput {
            target: t,
            explicit: None,
            advanced_skin: false,
            pbr: false,
            gras: false,
            suffixes,
            subtypes,
            game_paths: paths,
            mesh: &[],
        }
```

(b) Replace the first lines of `lists_and_defaults_follow_the_mode`:

```rust
    #[test]
    fn lists_and_defaults_follow_the_mode() {
        assert_eq!(Shader::available(Target::Cs).len(), 14);
        assert_eq!(Shader::available(Target::Cs)[0], Shader::Pbr);
        assert_eq!(Shader::available(Target::Cs)[1], Shader::PbrGrass);
        for t in [Target::Vanilla, Target::Cpm] {
            let l = Shader::available(t);
            assert_eq!(l.len(), if t == Target::Cpm { 11 } else { 10 }, "{t:?}");
            assert!(
                !l.contains(&Shader::Pbr)
                    && !l.contains(&Shader::PbrGrass)
                    && !l.contains(&Shader::AdvancedSkin)
            );
            assert_eq!(Shader::default_for(t), Shader::Default);
        }
```

(the rest of that test stays as it is).

(c) Append at the end of `mod tests` (after `complex_grass_is_picked_by_its_member_and_is_never_vanilla`):

```rust

    /// PBR grass design §4.1-4.2: four maps; a PBR set with GRAS evidence picks it; an explicit choice wins; CS only.
    #[test]
    fn pbr_grass_needs_a_pbr_set_and_gras_evidence() {
        use Subtype::*;
        assert_eq!(
            (Shader::PbrGrass.id(), Shader::PbrGrass.label()),
            ("pbr_grass", "PBR grass")
        );
        let spec = Shader::PbrGrass.spec();
        let labels = |roles: &[MapRole]| roles.iter().map(|r| r.label).collect::<Vec<_>>();
        assert_eq!(labels(spec.required), ["albedo", "_n", "_rmaos"]);
        assert_eq!(labels(spec.optional), ["subsurface _s"]);
        let s = sx(&["n", "rmaos"]);
        let st = [PbrAlbedo, PbrNormal, Rmaos];
        let paths = ["textures/plants/g.png"];
        let mut i = input(Target::Cs, &s, &st, &paths);
        i.pbr = true;
        assert_eq!(
            auto(&pick(&i)),
            (Shader::Pbr, ShaderOrigin::Auto(AutoRule::Pbr))
        );
        i.gras = true;
        assert_eq!(
            auto(&pick(&i)),
            (Shader::PbrGrass, ShaderOrigin::Auto(AutoRule::PbrGrass))
        );
        assert_eq!(
            "auto:pbr_grass".parse::<ShaderOrigin>().unwrap(),
            ShaderOrigin::Auto(AutoRule::PbrGrass)
        );
        i.explicit = Some(Shader::Pbr);
        assert_eq!(auto(&pick(&i)), (Shader::Pbr, ShaderOrigin::Set));
        i.explicit = None;
        i.target = Target::Vanilla;
        assert_eq!(
            pick(&i),
            ShaderPick {
                shader: Shader::Default,
                provenance: ShaderOrigin::Fallback,
                wanted: Some(Shader::PbrGrass)
            }
        );
        // GRAS evidence alone does not make a set PBR grass.
        let (s2, st2) = (sx(&["n"]), [Diffuse, Normal]);
        let mut j = input(Target::Cs, &s2, &st2, &paths);
        j.gras = true;
        assert_ne!(pick(&j).shader, Shader::PbrGrass);
    }

    /// PBR grass design §4.1: `_g`, `_p`, `_cnr` and `_f` are maps the grass shader cannot read.
    #[test]
    fn pbr_grass_reports_the_maps_grass_cannot_read() {
        let p = ShaderPick {
            shader: Shader::PbrGrass,
            provenance: ShaderOrigin::Set,
            wanted: None,
        };
        let m = |name: &str, st| MemberMap {
            name: name.into(),
            suffix: None,
            subtype: Some(st),
        };
        let w = check_maps(
            &p,
            &[
                m("g", Subtype::PbrAlbedo),
                m("g_n", Subtype::PbrNormal),
                m("g_rmaos", Subtype::Rmaos),
                m("g_s", Subtype::PbrSubsurface),
                m("g_g", Subtype::PbrEmissive),
            ],
        );
        assert_eq!(
            w.iter().map(|w| w.code).collect::<Vec<_>>(),
            [Code::ShaderExtraMap]
        );
        assert_eq!(w[0].data["member"], "g_g");
    }
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `CARGO_TARGET_DIR=F:/ai/dds/converter-nif-p1/target cargo test -p engine --release --lib shader::tests`
Expected: FAIL to compile: `no variant or associated item named PbrGrass`, `struct PickInput has no field named gras`.

- [ ] **Step 3: Implement the shader row, lists and rule**

In `crates/engine/src/shader.rs`:

(a) Enum `Shader`: add the variant right after `Pbr,`:

```rust
pub enum Shader {
    Pbr,
    /// Community Shaders PBR grass (PBR grass design): `RunGrass.hlsl` reads albedo, `_n`, `_rmaos` and `_s` only.
    PbrGrass,
    AdvancedSkin,
```

(b) After `static PBR: ShaderSpec = …;` add:

```rust
/// PBR grass design §4.1: what CS's grass shader binds (t0, t2, t3, t4; research §1.3).
static PBR_GRASS: ShaderSpec = ShaderSpec {
    required: &[
        role("albedo", &[Subtype::PbrAlbedo]),
        role("_n", &[Subtype::PbrNormal]),
        role("_rmaos", &[Subtype::Rmaos]),
    ],
    optional: &[role("subsurface _s", &[Subtype::PbrSubsurface])],
};
```

(c) `VANILLA`: the CS-only shaders are now the first three:

```rust
/// `ALL[3..]` without `ComplexGrass` (vanilla has no complex grass, ruling R1); a const loop, so position-independent.
static VANILLA: [Shader; 10] = {
    let mut out = [Shader::Default; 10];
    let (mut i, mut j) = (3, 0);
```

(the rest of the block stays).

(d) `Shader::ALL`:

```rust
    pub const ALL: [Shader; 14] = [
        Shader::Pbr,
        Shader::PbrGrass,
        Shader::AdvancedSkin,
```

(the rest of the list stays).

(e) `id()`: add `Shader::PbrGrass => "pbr_grass",` after `Shader::Pbr => "pbr",`. `label()`: add `Shader::PbrGrass => "PBR grass",` after `Shader::Pbr => "PBR",`. `spec()`: add `Shader::PbrGrass => &PBR_GRASS,` after `Shader::Pbr => &PBR,`.

(f) `available`:

```rust
    /// The shaders a target can use: CS has all 14 (`pbr` first); Vanilla and CPM lack `pbr`, `pbr_grass` and
    /// `advanced_skin`.
    pub fn available(t: Target) -> &'static [Shader] {
        match t {
            Target::Cs => &Shader::ALL,
            // `ALL` starts with `pbr`, `pbr_grass`, `advanced_skin`.
            Target::Cpm => &Shader::ALL[3..],
```

(g) `AutoRule`: add `PbrGrass,` after `Pbr,` in the enum; in `AutoRule::ALL` change `[AutoRule; 14]` to `[AutoRule; 15]` and add `AutoRule::PbrGrass,` after `AutoRule::Pbr,`; in `id()` add `AutoRule::PbrGrass => "pbr_grass",` after `AutoRule::Pbr => "pbr",`.

(h) `PickInput`: add after the `pbr` field:

```rust
    /// PBR grass design §4.2: a member is the slot-0 diffuse of a GRAS record's model, TruePBR shapes included
    /// (`BatchRequest::gras_all_diffuses`).
    pub gras: bool,
```

(i) `pick`: replace

```rust
        (Shader::Pbr, AutoRule::Pbr)
    } else if has(&[ComplexGrass]) {
```

with

```rust
        if i.gras {
            (Shader::PbrGrass, AutoRule::PbrGrass)
        } else {
            (Shader::Pbr, AutoRule::Pbr)
        }
    } else if has(&[ComplexGrass]) {
```

(j) `pick_for_set`: add the parameter and the field. Signature:

```rust
/// `pick` for one set at plan time (spec §4): pass-1 `jobs` give the member sub-types (packing inputs excluded),
/// `mesh` the scanned votes keyed by `mesh_key`, `gras` the GRAS slot-0 diffuses (`grass::path_key` keys).
pub fn pick_for_set(
    ctx: &crate::policy::ResolveCtx,
    set: &crate::group::TextureSet,
    files: &[crate::discover::InputFile],
    jobs: &JobIndex,
    explicit: Option<Shader>,
    mesh: &BTreeMap<String, Vec<Shader>>,
    gras: &BTreeSet<String>,
) -> ShaderPick {
```

and in its `pick(&PickInput { … })` literal replace

```rust
        suffixes: &set.suffixes,
        subtypes: &subtypes,
```

with

```rust
        gras: set
            .members
            .iter()
            .any(|&m| gras.contains(&crate::grass::path_key(&files[m].game_path))),
        suffixes: &set.suffixes,
        subtypes: &subtypes,
```

In `crates/engine/src/api.rs`:

(k) After the `gras_diffuses` field of `BatchRequest`:

```rust
    /// GUI (PBR grass design §4.2): as `gras_diffuses` with TruePBR shapes kept (`Store::gras_all_diffuse_textures`).
    /// A PBR set with a member listed here auto-picks `pbr_grass`. Empty for the CLI, which never auto-picks it.
    pub gras_all_diffuses: BTreeSet<String>,
```

(l) In `BatchRequest::new`, after `gras_diffuses: BTreeSet::new(),` add `gras_all_diffuses: BTreeSet::new(),`.

(m) In step 6b replace

```rust
                    explicit.get(&s.key).copied(),
                    &req.mesh_shaders,
                );
```

with

```rust
                    explicit.get(&s.key).copied(),
                    &req.mesh_shaders,
                    &req.gras_all_diffuses,
                );
```

- [ ] **Step 4: Run the unit tests to verify they pass**

Run: `CARGO_TARGET_DIR=F:/ai/dds/converter-nif-p1/target cargo test -p engine --release --lib shader::tests`
Expected: PASS.

- [ ] **Step 5: Write the plan-level tests**

Create `crates/engine/tests/pbr_grass.rs`:

```rust
//! PBR grass (design `F:\ai\dds\nifparseref\plan\pbr-grass\2026-10-14-pbr-grass-design.md`): the auto-pick from
//! GRAS evidence and the PBR grass warnings, at plan level.
use engine::api::*;
use engine::config::Target;
use engine::events::*;
use engine::overrides::parse_set;
use engine::policy::AcceptSuggestions;
use engine::shader::{AutoRule, Shader, ShaderOrigin};
use image::{ImageBuffer, Rgb};
use std::path::Path;
use std::sync::Arc;

/// The set key (and GRAS key) of the fixture set.
const SET: &str = "textures/plants/g";
const BROWN: [u8; 3] = [120, 90, 60];

fn put(dir: &Path, rel: &str, px: [u8; 3]) {
    let p = dir.join("in").join(rel);
    std::fs::create_dir_all(p.parent().unwrap()).unwrap();
    ImageBuffer::from_fn(16, 16, |_, _| Rgb(px))
        .save(p)
        .unwrap();
}

/// The PBR set `textures/plants/g`: an albedo of colour `albedo`, a flat `_n`, an `_rmaos`.
fn pbr_set(dir: &Path, albedo: [u8; 3]) {
    put(dir, "textures/plants/g.png", albedo);
    put(dir, "textures/plants/g_n.png", [128, 128, 255]);
    put(dir, "textures/plants/g_rmaos.png", [128, 0, 255]);
}

/// Plans `in/` under `t` with `gras` as `gras_all_diffuses`, `sets` as `--set` and `gui` as the GUI's set shaders.
fn plan(
    dir: &Path,
    t: Target,
    gras: &[&str],
    sets: &[&str],
    gui: &[(&str, Shader)],
) -> (Plan, Vec<EngineEvent>) {
    let mut req = BatchRequest::new(vec![dir.join("in")], dir.join("out"), t);
    req.recursive = true;
    req.accept = AcceptSuggestions::High;
    req.gras_all_diffuses = gras.iter().map(|s| s.to_string()).collect();
    req.overrides = sets.iter().map(|s| parse_set(s).unwrap()).collect();
    req.set_shaders = gui.iter().map(|(k, s)| (k.to_string(), *s)).collect();
    let sink = Arc::new(VecSink::default());
    let rep = Reporter::new(sink.clone(), "pbr_grass");
    let p = Engine::new()
        .unwrap()
        .plan(req, &rep, &CancelToken::default())
        .unwrap();
    (p, sink.take())
}

/// The one set's pick.
fn picked(p: &Plan) -> (Shader, ShaderOrigin, Option<Shader>) {
    let s = p.shaders.values().next().expect("one set");
    (s.shader, s.provenance.clone(), s.wanted)
}

#[test]
fn a_pbr_set_a_gras_model_uses_picks_pbr_grass() {
    let d = tempfile::tempdir().unwrap();
    pbr_set(d.path(), BROWN);
    let (p, _) = plan(d.path(), Target::Cs, &[SET], &[], &[]);
    assert_eq!(
        picked(&p),
        (
            Shader::PbrGrass,
            ShaderOrigin::Auto(AutoRule::PbrGrass),
            None
        )
    );
    let (p, _) = plan(d.path(), Target::Cs, &[], &[], &[]);
    assert_eq!(picked(&p).0, Shader::Pbr, "no GRAS evidence, plain pbr");
}

#[test]
fn an_explicit_pbr_wins_over_gras_evidence() {
    let d = tempfile::tempdir().unwrap();
    pbr_set(d.path(), BROWN);
    let (p, _) = plan(d.path(), Target::Cs, &[SET], &[], &[(SET, Shader::Pbr)]);
    assert_eq!(picked(&p), (Shader::Pbr, ShaderOrigin::Set, None));
}

#[test]
fn the_cli_can_choose_pbr_grass() {
    let d = tempfile::tempdir().unwrap();
    pbr_set(d.path(), BROWN);
    let (p, _) = plan(
        d.path(),
        Target::Cs,
        &[],
        &["set:textures/plants/g.shader=pbr_grass"],
        &[],
    );
    assert_eq!(picked(&p), (Shader::PbrGrass, ShaderOrigin::Set, None));
}

#[test]
fn vanilla_and_cpm_fall_back_from_pbr_grass() {
    for t in [Target::Vanilla, Target::Cpm] {
        let d = tempfile::tempdir().unwrap();
        pbr_set(d.path(), BROWN);
        let (p, _) = plan(d.path(), t, &[SET], &[], &[(SET, Shader::PbrGrass)]);
        assert_eq!(
            picked(&p),
            (
                Shader::Default,
                ShaderOrigin::Fallback,
                Some(Shader::PbrGrass)
            ),
            "{t:?}"
        );
    }
}

#[test]
fn gras_evidence_on_a_set_that_is_not_pbr_is_not_pbr_grass() {
    let d = tempfile::tempdir().unwrap();
    put(d.path(), "textures/plants/g.png", BROWN);
    put(d.path(), "textures/plants/g_n.png", [128, 128, 255]);
    let (p, _) = plan(d.path(), Target::Cs, &[SET], &[], &[]);
    assert_ne!(picked(&p).0, Shader::PbrGrass);
}
```

(`use engine::codes::Code;` comes in Task 2 and the `UNIT_BLUE` constant in Task 3, where they are first used; clippy runs with `-D warnings`.)

- [ ] **Step 6: Run the plan-level tests**

Run: `CARGO_TARGET_DIR=F:/ai/dds/converter-nif-p1/target cargo test -p engine --release --test pbr_grass`
Expected: 5 PASS.

- [ ] **Step 7: Regenerate the schemas and fix the GUI dropdown count**

```bash
CARGO_TARGET_DIR=F:/ai/dds/converter-nif-p1/target cargo run -q --release -p sdc -- presets schema all --out schema
git diff --stat schema
```

Expected: the `Shader` enum in six schema files gains `"pbr_grass"` after `"pbr"`; nothing else changes.

In `crates/gui/tests/sets_model.rs`, `shader_dropdown_lists_match_the_mode`, change `assert_eq!(shader_options(Target::Cs).len(), 13);` to:

```rust
    assert_eq!(shader_options(Target::Cs).len(), 14);
    assert!(!shader_options(Target::Cpm).contains(&Shader::PbrGrass));
```

- [ ] **Step 8: Gate and commit**

```bash
CARGO_TARGET_DIR=F:/ai/dds/converter-nif-p1/target cargo fmt --all --check
CARGO_TARGET_DIR=F:/ai/dds/converter-nif-p1/target cargo clippy --workspace --all-targets --release -- -D warnings
CARGO_TARGET_DIR=F:/ai/dds/converter-nif-p1/target cargo test --workspace --release
git add crates/engine/src/shader.rs crates/engine/src/api.rs crates/engine/tests/pbr_grass.rs crates/gui/tests/sets_model.rs schema
git commit -m "feat(engine): pbr_grass set shader, auto-picked for PBR sets a GRAS model uses" -m "Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

Expected: all green. If a golden or `gui_repro` test fails, stop and ask the owner.

---

### Task 2: `PBR-GRASS-PGPATCHER` on every PBR grass set (engine)

**Files:**
- Modify: `crates/engine/src/codes.rs` (header comment, `codes!` list end, test `codes_serialise_as_their_strings_and_are_unique`)
- Modify: `crates/engine/src/shader.rs` (`PGPATCHER_GRASS`, `pick_notes`, tests)
- Modify: `crates/engine/tests/pbr_grass.rs`
- Regenerate: `schema/*.json`

**Interfaces:**
- Consumes: `Shader::PbrGrass`, `AutoRule::PbrGrass` (Task 1).
- Produces: `engine::codes::Code::PbrGrassPgpatcher` (`"PBR-GRASS-PGPATCHER"`, `Warning`); `pub const engine::shader::PGPATCHER_GRASS: &str`; `pick_notes` appends that warning for a `pbr_grass` pick.

- [ ] **Step 1: Write the failing tests**

In `crates/engine/src/shader.rs` `mod tests`, append:

```rust

    /// PBR grass design §4.4: every pbr_grass pick, set or auto, notes the PGPatcher version; nothing else does.
    #[test]
    fn a_pbr_grass_pick_notes_the_pgpatcher_version() {
        let p = |shader, provenance| ShaderPick {
            shader,
            provenance,
            wanted: None,
        };
        let codes = |p: &ShaderPick| {
            pick_notes(p, Target::Cs)
                .into_iter()
                .map(|w| w.code)
                .collect::<Vec<_>>()
        };
        assert_eq!(
            codes(&p(Shader::PbrGrass, ShaderOrigin::Set)),
            [Code::PbrGrassPgpatcher]
        );
        assert_eq!(
            codes(&p(Shader::PbrGrass, ShaderOrigin::Auto(AutoRule::PbrGrass))),
            [Code::ShaderAuto, Code::PbrGrassPgpatcher]
        );
        assert!(codes(&p(Shader::Pbr, ShaderOrigin::Set)).is_empty());
        assert_eq!(
            PGPATCHER_GRASS,
            "PGPatcher 1.2.0 does not patch grass meshes; needs 2.1.0 or later"
        );
    }
```

In `crates/engine/tests/pbr_grass.rs` add `use engine::codes::Code;` to the imports and append:

```rust

/// The `SetResolved` event of the fixture set.
fn set_warnings(ev: &[EngineEvent]) -> Vec<Code> {
    ev.iter()
        .find_map(|e| match &e.kind {
            EventKind::SetResolved { warnings, .. } if e.set.as_deref().is_some_and(|s| s.ends_with(SET)) => {
                Some(warnings.iter().map(|w| w.code).collect())
            }
            _ => None,
        })
        .expect("the set event")
}

#[test]
fn only_a_pbr_grass_set_gets_the_pgpatcher_note() {
    let d = tempfile::tempdir().unwrap();
    pbr_set(d.path(), BROWN);
    let (_, ev) = plan(d.path(), Target::Cs, &[SET], &[], &[]);
    assert!(set_warnings(&ev).contains(&Code::PbrGrassPgpatcher));
    let (_, ev) = plan(d.path(), Target::Cs, &[SET], &[], &[(SET, Shader::Pbr)]);
    assert!(!set_warnings(&ev).contains(&Code::PbrGrassPgpatcher));
}
```

In `crates/engine/src/codes.rs`, test `codes_serialise_as_their_strings_and_are_unique`: change `assert_eq!(Code::ALL.len(), 90);` to `assert_eq!(Code::ALL.len(), 91);`, change `assert_eq!(Code::ALL.last(), Some(&Code::ValGrassDetect));` to `assert_eq!(Code::ALL.last(), Some(&Code::PbrGrassPgpatcher));`, and add:

```rust
        assert_eq!(Code::PbrGrassPgpatcher.severity(), Severity::Warning);
        assert_eq!(
            Code::parse("PBR-GRASS-PGPATCHER"),
            Some(Code::PbrGrassPgpatcher)
        );
```

- [ ] **Step 2: Run them to verify they fail**

Run: `CARGO_TARGET_DIR=F:/ai/dds/converter-nif-p1/target cargo test -p engine --release --lib shader::tests codes::tests`
Expected: FAIL to compile: `no variant … named PbrGrassPgpatcher`, `cannot find value PGPATCHER_GRASS`.

- [ ] **Step 3: Implement**

`crates/engine/src/codes.rs`: after the header line `//! and IN-ALIAS-INVALID / IN-ALIAS-STALE (an `[inputs]` alias that was refused or matched no file, M6 Track F3b).` add:

```rust
//! and PBR-GRASS-PGPATCHER (PGPatcher before 2.1.0 does not patch grass meshes, PBR grass design 2026-10-14 §4.4).
```

and in `codes! { … }` after `ValGrassDetect = "VAL-GRASS-DETECT" => Error,` add:

```rust
    PbrGrassPgpatcher = "PBR-GRASS-PGPATCHER" => Warning,
```

`crates/engine/src/shader.rs`: after `static PBR_GRASS …;` add:

```rust
/// PBR grass design §4.4: PGPatcher before 2.1.0 refuses TruePBR on meshes a GRAS record uses (research §3.1); the
/// owner stays on 1.2.0 and sdc exports anyway (owner answers 2026-10-13).
pub const PGPATCHER_GRASS: &str = "PGPatcher 1.2.0 does not patch grass meshes; needs 2.1.0 or later";
```

Replace `pick_notes` with:

```rust
/// SHADER-AUTO for an `auto:` pick (data `{"shader", "rule"}`); SHADER-UNAVAILABLE for a fallback (data
/// `{"wanted", "shader"}`); nothing for `set`. A `pbr_grass` pick also gets PBR-GRASS-PGPATCHER.
pub fn pick_notes(pick: &ShaderPick, target: Target) -> Vec<Warning> {
    let id = pick.shader.id();
    let mut out = match (&pick.provenance, pick.wanted) {
        (ShaderOrigin::Auto(r), _) => vec![Warning::new(
            Code::ShaderAuto,
            format!("shader {id} picked automatically ({})", r.id()),
        )
        .with_data(serde_json::json!({"shader": id, "rule": r.id()}))],
        (ShaderOrigin::Fallback, Some(w)) => vec![Warning::new(
            Code::ShaderUnavailable,
            format!(
                "shader {} is not available under {target:?}; using {id}",
                w.id()
            ),
        )
        .with_data(serde_json::json!({"wanted": w.id(), "shader": id}))],
        _ => vec![],
    };
    if pick.shader == Shader::PbrGrass {
        out.push(Warning::new(Code::PbrGrassPgpatcher, PGPATCHER_GRASS));
    }
    out
}
```

- [ ] **Step 4: Run the tests and regenerate the schemas**

```bash
CARGO_TARGET_DIR=F:/ai/dds/converter-nif-p1/target cargo test -p engine --release --lib shader::tests codes::tests
CARGO_TARGET_DIR=F:/ai/dds/converter-nif-p1/target cargo test -p engine --release --test pbr_grass
CARGO_TARGET_DIR=F:/ai/dds/converter-nif-p1/target cargo run -q --release -p sdc -- presets schema all --out schema
CARGO_TARGET_DIR=F:/ai/dds/converter-nif-p1/target cargo test -p engine --release --test schemas
```

Expected: PASS; the schema diff adds `"PBR-GRASS-PGPATCHER"` to the `Code` enums only.

- [ ] **Step 5: Gate and commit**

```bash
CARGO_TARGET_DIR=F:/ai/dds/converter-nif-p1/target cargo fmt --all --check
CARGO_TARGET_DIR=F:/ai/dds/converter-nif-p1/target cargo clippy --workspace --all-targets --release -- -D warnings
CARGO_TARGET_DIR=F:/ai/dds/converter-nif-p1/target cargo test --workspace --release
git add crates/engine/src/codes.rs crates/engine/src/shader.rs crates/engine/tests/pbr_grass.rs schema
git commit -m "feat(engine): PBR-GRASS-PGPATCHER note on every pbr_grass set" -m "Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

---

### Task 3: `PBR-GRASS-DETECT`, the complex-grass negative check (engine; needs the owner's yes for `analyse.rs`)

Do this task only after the owner approves the `analyse.rs` change (spec R1). Without it, skip to Task 4.

**Files:**
- Modify: `crates/engine/src/analyse.rs:66-67` (field), `:172` (value)
- Modify: `crates/engine/src/codes.rs` (header, list end, test)
- Modify: `crates/engine/src/shader.rs` (`pbr_grass_albedo_note`, test)
- Modify: `crates/engine/src/api.rs` (step 6c loop, around `:1151-1180`)
- Modify: `crates/engine/tests/pbr_grass.rs`
- Regenerate: `schema/*.json`

**Interfaces:**
- Consumes: `Shader::PbrGrass` (Task 1); `grass::{unit_err, CS_GRASS_THRESHOLD, GRASS_GUARD_MARGIN}` (existing).
- Produces: `engine::analyse::Analysis::corner_err: f32`; `engine::codes::Code::PbrGrassDetect` (`"PBR-GRASS-DETECT"`, `Warning`); `pub fn engine::shader::pbr_grass_albedo_note(pick: &ShaderPick, st: Subtype, corner_err: f32) -> Option<Warning>`.

- [ ] **Step 1: Write the failing tests**

`crates/engine/src/shader.rs`, `mod tests`, append:

```rust

    /// PBR grass design §4.4: the albedo of a pbr_grass set whose corner reads as a unit vector (0.01 to spare).
    #[test]
    fn a_pbr_grass_albedo_that_reads_as_complex_grass_is_noted() {
        let grass = ShaderPick {
            shader: Shader::PbrGrass,
            provenance: ShaderOrigin::Set,
            wanted: None,
        };
        let pbr = ShaderPick {
            shader: Shader::Pbr,
            ..grass.clone()
        };
        let note = |p: &ShaderPick, st, e| pbr_grass_albedo_note(p, st, e).map(|w| w.code);
        assert_eq!(note(&grass, Subtype::PbrAlbedo, 0.0), Some(Code::PbrGrassDetect));
        assert_eq!(note(&grass, Subtype::Diffuse, 0.039), Some(Code::PbrGrassDetect));
        assert_eq!(note(&grass, Subtype::PbrAlbedo, 0.05), None);
        assert_eq!(note(&grass, Subtype::PbrAlbedo, f32::INFINITY), None);
        assert_eq!(note(&grass, Subtype::PbrNormal, 0.0), None);
        assert_eq!(note(&pbr, Subtype::PbrAlbedo, 0.0), None);
    }
```

`crates/engine/tests/pbr_grass.rs`: add the constant after `BROWN`:

```rust
/// Decodes to (0, 0, 1): CS's complex-grass detector would accept this texel.
const UNIT_BLUE: [u8; 3] = [128, 128, 255];
```

and append:

```rust

/// The warning codes of the albedo job.
fn albedo_codes(p: &Plan) -> Vec<Code> {
    p.jobs
        .iter()
        .find(|j| j.input.file_name().is_some_and(|n| n == "g.png"))
        .expect("the albedo job")
        .warnings
        .iter()
        .map(|w| w.code)
        .collect()
}

#[test]
fn an_albedo_that_reads_as_complex_grass_warns_only_on_pbr_grass() {
    let d = tempfile::tempdir().unwrap();
    pbr_set(d.path(), UNIT_BLUE);
    let (p, _) = plan(d.path(), Target::Cs, &[SET], &[], &[]);
    assert!(albedo_codes(&p).contains(&Code::PbrGrassDetect), "{:?}", albedo_codes(&p));
    let (p, _) = plan(d.path(), Target::Cs, &[SET], &[], &[(SET, Shader::Pbr)]);
    assert!(!albedo_codes(&p).contains(&Code::PbrGrassDetect));
    let d = tempfile::tempdir().unwrap();
    pbr_set(d.path(), BROWN);
    let (p, _) = plan(d.path(), Target::Cs, &[SET], &[], &[]);
    assert!(!albedo_codes(&p).contains(&Code::PbrGrassDetect));
}
```

`crates/engine/src/codes.rs` test: change `91` to `92`, change the `last()` line to `assert_eq!(Code::ALL.last(), Some(&Code::PbrGrassDetect));`, and add `assert_eq!(Code::PbrGrassDetect.severity(), Severity::Warning);`.

- [ ] **Step 2: Run them to verify they fail**

Run: `CARGO_TARGET_DIR=F:/ai/dds/converter-nif-p1/target cargo test -p engine --release --lib shader::tests codes::tests`
Expected: FAIL to compile: `no variant … named PbrGrassDetect`, `cannot find function pbr_grass_albedo_note`.

- [ ] **Step 3: Implement**

`crates/engine/src/analyse.rs`, struct `Analysis`, after `pub grass: Option<crate::grass::GrassFacts>,`:

```rust
    /// `|L - 1|` of the bottom-left texel `(0, h - 1)`, the one CS's complex-grass detector reads, for every shape
    /// (PBR grass design R1); infinite for an empty image.
    pub corner_err: f32,
```

and in `analyse()` replace

```rust
        grass: crate::grass::facts(img),
    }
}
```

with

```rust
        grass: crate::grass::facts(img),
        corner_err: img
            .px
            .get(img.height.saturating_sub(1) as usize * img.width as usize)
            .map_or(f32::INFINITY, |c| crate::grass::unit_err([c[0], c[1], c[2]])),
    }
}
```

`crates/engine/src/codes.rs`: extend the header line added in Task 2 to

```rust
//! and PBR-GRASS-PGPATCHER / PBR-GRASS-DETECT (PBR grass design 2026-10-14 §4.4).
```

and after `PbrGrassPgpatcher = "PBR-GRASS-PGPATCHER" => Warning,` add:

```rust
    PbrGrassDetect = "PBR-GRASS-DETECT" => Warning,
```

`crates/engine/src/shader.rs`, after `pick_notes`:

```rust
/// PBR grass design §4.4: CS's grass depth/shadow pass runs the complex-grass detector on a PBR grass albedo
/// (research §1.6), so an albedo whose bottom-left texel reads as a unit vector gets mismatched cutout shadows.
/// `GRASS_GUARD_MARGIN` to spare: BC encoding moves the texel (DECISIONS X2).
pub fn pbr_grass_albedo_note(pick: &ShaderPick, st: Subtype, corner_err: f32) -> Option<Warning> {
    use crate::grass::{CS_GRASS_THRESHOLD, GRASS_GUARD_MARGIN};
    (pick.shader == Shader::PbrGrass
        && matches!(st, Subtype::PbrAlbedo | Subtype::Diffuse)
        && corner_err < CS_GRASS_THRESHOLD + GRASS_GUARD_MARGIN)
        .then(|| {
            Warning::new(
                Code::PbrGrassDetect,
                format!(
                    "bottom-left texel has |L-1| = {corner_err:.3}: Community Shaders' grass shadow pass would read \
                     this albedo as complex grass, so shadows will not match; change that texel"
                ),
            )
        })
}
```

`crates/engine/src/api.rs`, step 6c. Replace

```rust
        for (old, (wi, a, info)) in pass1_jobs.into_iter().zip(pass1) {
            // An explicit set shader also re-resolves a GRAS diffuse: it suppresses the auto-detect (`suggest_grass`).
            let is_gras = gras.contains(&crate::grass::path_key(&work[wi].1.game_path));
            if !picks.get(&old.set_key).is_some_and(|p| {
                p.shader.has_member_defaults()
                    || (is_gras && p.provenance == crate::shader::ShaderOrigin::Set)
            }) {
                jobs.push(old);
                continue;
            }
```

with

```rust
        for (mut old, (wi, a, info)) in pass1_jobs.into_iter().zip(pass1) {
            // PBR grass design §4.4: the albedo's corner texel against CS's grass shadow-pass detector.
            let grass_note = picks.get(&old.set_key).and_then(|p| {
                crate::shader::pbr_grass_albedo_note(p, old.profile.subtype, a.corner_err)
            });
            // An explicit set shader also re-resolves a GRAS diffuse: it suppresses the auto-detect (`suggest_grass`).
            let is_gras = gras.contains(&crate::grass::path_key(&work[wi].1.game_path));
            if !picks.get(&old.set_key).is_some_and(|p| {
                p.shader.has_member_defaults()
                    || (is_gras && p.provenance == crate::shader::ShaderOrigin::Set)
            }) {
                old.warnings.extend(grass_note);
                jobs.push(old);
                continue;
            }
```

and a few lines below replace

```rust
                Ok(mut job) => {
                    job.source = old.source;
```

with

```rust
                Ok(mut job) => {
                    job.source = old.source;
                    job.warnings.extend(grass_note);
```

- [ ] **Step 4: Run the tests and regenerate the schemas**

```bash
CARGO_TARGET_DIR=F:/ai/dds/converter-nif-p1/target cargo test -p engine --release --lib shader::tests codes::tests analyse::tests
CARGO_TARGET_DIR=F:/ai/dds/converter-nif-p1/target cargo test -p engine --release --test pbr_grass --test complex_grass
CARGO_TARGET_DIR=F:/ai/dds/converter-nif-p1/target cargo run -q --release -p sdc -- presets schema all --out schema
CARGO_TARGET_DIR=F:/ai/dds/converter-nif-p1/target cargo test -p engine --release --test schemas
```

Expected: PASS; the schema diff adds `"PBR-GRASS-DETECT"` only (`Analysis` is in no schema).

- [ ] **Step 5: Gate and commit (own commit, so it can be reverted alone)**

```bash
CARGO_TARGET_DIR=F:/ai/dds/converter-nif-p1/target cargo fmt --all --check
CARGO_TARGET_DIR=F:/ai/dds/converter-nif-p1/target cargo clippy --workspace --all-targets --release -- -D warnings
CARGO_TARGET_DIR=F:/ai/dds/converter-nif-p1/target cargo test --workspace --release
git add crates/engine/src/analyse.rs crates/engine/src/codes.rs crates/engine/src/shader.rs crates/engine/src/api.rs crates/engine/tests/pbr_grass.rs schema
git commit -m "feat(engine): PBR-GRASS-DETECT, the complex-grass negative check on PBR grass albedos" -m "Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

---

### Task 4: GRAS evidence with TruePBR shapes kept (modscan + GUI plumbing)

**Files:**
- Modify: `crates/modscan/src/plugin_store.rs:349-375` (`gras_diffuse_textures`)
- Modify: `crates/modscan/tests/gras_diffuse.rs`
- Modify: `crates/gui/src/model/meshes.rs:331-337`
- Modify (shared, add-only): `crates/gui/src/worker.rs:24-28`, `crates/gui/src/model/mod.rs` (8 places listed in Step 5)
- Modify: `crates/gui/tests/complex_grass.rs`

**Interfaces:**
- Consumes: `BatchRequest::gras_all_diffuses` (Task 1).
- Produces: `modscan::store::Store::gras_all_diffuse_textures(&self) -> Result<BTreeSet<String>, ScanError>`; `sdc_gui::model::meshes::gras_all_diffuses(db: &Path) -> Result<BTreeSet<String>, String>`; `sdc_gui::worker::MeshEvidence::gras_all: BTreeSet<String>`; `sdc_gui::model::AppModel::gras_all_diffuses: BTreeSet<String>`.

- [ ] **Step 1: Write the failing modscan tests**

Append to `crates/modscan/tests/gras_diffuse.rs`:

```rust

/// PBR grass design §4.2 (ruling R5): the `all` variant keeps TruePBR shapes; the complex-grass query still drops them.
#[test]
fn the_all_variant_keeps_truepbr_shapes() {
    let (_d, s) = scanned(b"GRAS");
    let want = x_diffuses(&s);
    assert!(!want.is_empty());
    assert_eq!(s.gras_all_diffuse_textures().unwrap(), want);
    s.conn()
        .execute("UPDATE shape SET slsf2 = ifnull(slsf2,0) | 8388608", [])
        .unwrap();
    assert_eq!(s.gras_all_diffuse_textures().unwrap(), want);
    assert!(s.gras_diffuse_textures().unwrap().is_empty());
}

#[test]
fn the_all_variant_names_none_for_a_stat() {
    let (_d, s) = scanned(b"STAT");
    assert!(s.gras_all_diffuse_textures().unwrap().is_empty());
}
```

- [ ] **Step 2: Run them to verify they fail**

Run: `CARGO_TARGET_DIR=F:/ai/dds/converter-nif-p1/target cargo test -p modscan --release --test gras_diffuse`
Expected: FAIL to compile: `no method named gras_all_diffuse_textures`.

- [ ] **Step 3: Implement the query**

In `crates/modscan/src/plugin_store.rs` replace the whole `gras_diffuse_textures` method with:

```rust
    /// Complex grass design §5 condition 1: slot-0 textures of the lighting shapes of the winning NIF that the `MODL`
    /// (model slot 0) of each winning GRAS record names, in an enabled origin (a derived winner is read through its
    /// source copy; TruePBR shapes are skipped). Store form: lower case, `/`.
    pub fn gras_diffuse_textures(&self) -> Result<BTreeSet<String>, ScanError> {
        self.gras_slot0_textures(false)
    }

    /// PBR grass design §4.2 (ruling R5): [`Self::gras_diffuse_textures`] with TruePBR (SLSF2.23) shapes kept, so a
    /// pre-patched PBR grass NIF counts as well as a vanilla one: the evidence for the `pbr_grass` auto-pick.
    pub fn gras_all_diffuse_textures(&self) -> Result<BTreeSet<String>, ScanError> {
        self.gras_slot0_textures(true)
    }

    fn gras_slot0_textures(&self, keep_truepbr: bool) -> Result<BTreeSet<String>, ScanError> {
        let truepbr = if keep_truepbr {
            ""
        } else {
            " AND (ifnull(s.slsf2,0) & 8388608) = 0"
        };
        let mut st = self.conn.prepare(&format!(
            // CROSS JOIN pins the join order (SQLite never reorders it): start from the few GRAS records. Left to
            // the planner (no ANALYZE stats) it scanned `path` first and hung the GUI for minutes on a real modlist.
            "SELECT DISTINCT tp.p
             FROM rec r
             CROSS JOIN model_ref m ON m.plugin_id = r.plugin_id AND m.local = r.local AND m.slot = 0
             CROSS JOIN rec_winner w ON w.formid = m.formid AND w.plugin_id = m.plugin_id
             CROSS JOIN nif_winner nw ON nw.path_id = m.path_id
             CROSS JOIN nif_copy wc ON wc.id = nw.copy_id
             CROSS JOIN origin wo ON wo.id = wc.origin_id AND wo.enabled = 1
             LEFT JOIN nif_winner_src ws ON ws.path_id = nw.path_id
             CROSS JOIN nif_copy c ON c.id = CASE WHEN wc.is_derived = 1
                                            THEN coalesce(ws.copy_id, wc.source_copy_id, wc.id) ELSE wc.id END
             CROSS JOIN origin o ON o.id = c.origin_id AND o.enabled = 1
             CROSS JOIN shape s ON s.copy_id = c.id AND s.shader_kind = 1{truepbr}
             CROSS JOIN tex_use tu ON tu.texset_id = s.texset_id AND tu.slot = 0
             CROSS JOIN path tp ON tp.id = tu.tex_path_id
             WHERE r.sig = 'GRAS' AND r.owner_ok = 1"
        ))?;
        let v = st
            .query_map([], |r| r.get::<_, String>(0))?
            .collect::<Result<_, _>>()?;
        Ok(v)
    }
```

- [ ] **Step 4: Run the modscan tests**

Run: `CARGO_TARGET_DIR=F:/ai/dds/converter-nif-p1/target cargo test -p modscan --release --test gras_diffuse`
Expected: 7 PASS (the 5 existing, including `a_large_store_answers_quickly`, plus 2 new).

Commit:

```bash
git add crates/modscan/src/plugin_store.rs crates/modscan/tests/gras_diffuse.rs
git commit -m "feat(modscan): gras_all_diffuse_textures keeps TruePBR shapes (PBR grass evidence)" -m "Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

- [ ] **Step 5: Write the failing GUI test**

Append to `crates/gui/tests/complex_grass.rs`:

```rust

/// PBR grass design §4.2, R5: a TruePBR GRAS shape is evidence for pbr_grass, and none for complex grass.
#[test]
fn a_pbr_set_whose_albedo_a_truepbr_gras_shape_uses_picks_pbr_grass() {
    use engine::shader::{AutoRule, Shader, ShaderOrigin};
    let d = tempfile::tempdir().unwrap();
    for (name, px) in [
        ("g.png", [64u8, 128, 38]),
        ("g_n.png", [128, 128, 255]),
        ("g_rmaos.png", [128, 0, 255]),
    ] {
        let p = d.path().join("textures/plants").join(name);
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        image::ImageBuffer::from_fn(16, 16, |_, _| image::Rgb(px))
            .save(p)
            .unwrap();
    }
    let mut m = support::opened(d.path());
    m.accept = engine::policy::AcceptSuggestions::High;
    let dbdir = tempfile::tempdir().unwrap();
    let db = gras_db(dbdir.path(), r"textures\plants\g.dds");
    modscan::store::Store::open(&db)
        .unwrap()
        .conn()
        .execute("UPDATE shape SET slsf2 = 8388608", [])
        .unwrap();
    m.scan_db = Some(db);
    m.replan();
    assert!(
        m.wait_idle(Duration::from_secs(60)),
        "planning did not finish"
    );
    assert!(
        m.gras_diffuses.is_empty(),
        "a TruePBR shape is no complex-grass evidence"
    );
    assert_eq!(
        m.gras_all_diffuses,
        ["textures/plants/g".to_string()].into_iter().collect()
    );
    assert_eq!(m.request().unwrap().gras_all_diffuses, m.gras_all_diffuses);
    let v = sdc_gui::model::sets::set_views(&m)
        .into_iter()
        .find(|v| v.id == "plants/g")
        .expect("the plants/g set");
    let p = v.shader.expect("a shader pick");
    assert_eq!(
        (p.shader, p.provenance),
        (Shader::PbrGrass, ShaderOrigin::Auto(AutoRule::PbrGrass))
    );
}
```

Run: `CARGO_TARGET_DIR=F:/ai/dds/converter-nif-p1/target cargo test -p sdc-gui --release --test complex_grass a_pbr_set_whose`
Expected: FAIL to compile: `no field gras_all_diffuses on type AppModel`.

- [ ] **Step 6: Implement the GUI plumbing**

`crates/gui/src/model/meshes.rs`, after `gras_diffuses`:

```rust

/// PBR grass design §4.2 (ruling R5): every GRAS slot-0 diffuse of the scan, TruePBR shapes kept, as
/// `engine::grass::path_key` keys (sent as `BatchRequest::gras_all_diffuses`).
pub fn gras_all_diffuses(db: &Path) -> Result<BTreeSet<String>, String> {
    let store = Store::open_read_only(db).map_err(db_err)?;
    let paths = store.gras_all_diffuse_textures().map_err(db_err)?;
    Ok(paths.iter().map(|p| engine::grass::path_key(p)).collect())
}
```

Then the shared files, add-only (committed on their own in Step 8):

`crates/gui/src/worker.rs`, `MeshEvidence`, after `pub gras: std::collections::BTreeSet<String>,`:

```rust
    /// PBR grass design §4.2: `meshes::gras_all_diffuses`.
    pub gras_all: std::collections::BTreeSet<String>,
```

`crates/gui/src/model/mod.rs`:

1. `struct EngineInputs`, after `gras_diffuses: std::collections::BTreeSet<String>,`:

```rust
    gras_all_diffuses: std::collections::BTreeSet<String>,
```

2. `EngineInputs::of`, after `gras_diffuses: m.gras_diffuses.clone(),`:

```rust
            gras_all_diffuses: m.gras_all_diffuses.clone(),
```

3. `AppModel`, after the `pub gras_diffuses: …` field:

```rust
    /// PBR grass design §4.2: every GRAS diffuse of the scan, TruePBR shapes kept, read with the mesh evidence; sent
    /// as `BatchRequest::gras_all_diffuses`.
    pub gras_all_diffuses: std::collections::BTreeSet<String>,
```

4. `AppModel::new`, after `gras_diffuses: Default::default(),`:

```rust
            gras_all_diffuses: Default::default(),
```

5. `request()`, after `req.gras_diffuses = self.gras_diffuses.clone();`:

```rust
        req.gras_all_diffuses = self.gras_all_diffuses.clone();
```

6. `refresh_mesh_votes`, in the `MeshEvidence { … }` literal after `gras: meshes::gras_diffuses(&db)?,`:

```rust
                    gras_all: meshes::gras_all_diffuses(&db)?,
```

7. `votes_landed`, replace

```rust
                let changed = e.votes != self.mesh_votes
                    || e.alpha != self.mesh_alpha
                    || e.gras != self.gras_diffuses;
                (self.mesh_votes, self.mesh_alpha, self.gras_diffuses) = (e.votes, e.alpha, e.gras);
```

with

```rust
                let changed = e.votes != self.mesh_votes
                    || e.alpha != self.mesh_alpha
                    || e.gras != self.gras_diffuses
                    || e.gras_all != self.gras_all_diffuses;
                (self.mesh_votes, self.mesh_alpha, self.gras_diffuses) = (e.votes, e.alpha, e.gras);
                self.gras_all_diffuses = e.gras_all;
```

8. `replan`, in the `EngineInputs { … }` literal after `gras_diffuses: req.gras_diffuses.clone(),`:

```rust
            gras_all_diffuses: req.gras_all_diffuses.clone(),
```

- [ ] **Step 7: Run the GUI tests**

Run: `CARGO_TARGET_DIR=F:/ai/dds/converter-nif-p1/target cargo test -p sdc-gui --release --test complex_grass`
Expected: all PASS, including `a_gras_diffuse_from_the_scan_db_is_tagged_after_the_plan` (unchanged behaviour) and the new test.

- [ ] **Step 8: Gate and commit in three commits, each green**

```bash
CARGO_TARGET_DIR=F:/ai/dds/converter-nif-p1/target cargo fmt --all --check
CARGO_TARGET_DIR=F:/ai/dds/converter-nif-p1/target cargo clippy --workspace --all-targets --release -- -D warnings
CARGO_TARGET_DIR=F:/ai/dds/converter-nif-p1/target cargo test --workspace --release
git add crates/gui/src/model/meshes.rs
git commit -m "feat(gui): read the PBR grass evidence from the scan database" -m "Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
git add crates/gui/src/worker.rs crates/gui/src/model/mod.rs
git commit -m "feat(gui): send the PBR grass evidence with the plan request (add-only)" -m "Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
git add crates/gui/tests/complex_grass.rs
git commit -m "test(gui): a TruePBR GRAS shape makes its PBR set pbr_grass" -m "Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

(`meshes.rs` compiles alone: `gras_all_diffuses` is `pub`. The shared-file commit holds only `worker.rs` and `model/mod.rs`.)

---

### Task 5: The PBR grass view of the PGPatcher settings (form, checks, export)

**Files:**
- Modify: `crates/gui/src/pgp/schema.rs` (after `spec`)
- Modify: `crates/gui/src/pgp/settings.rs` (end of file)
- Modify: `crates/gui/src/model/sets.rs` (`is_pbr_grass` after `is_json_set`; `build_views` "PBR, derived match, prefix" loop)
- Modify: `crates/gui/src/ui/pgp_fields.rs` (imports, `Ctx`, `context`, `panel`, `control`)
- Modify: `crates/gui/tests/pgp.rs`, `crates/gui/tests/pbr_export.rs`, `crates/gui/tests/set_editor_ui.rs`

**Interfaces:**
- Consumes: `Shader::PbrGrass` (Task 1).
- Produces: `pub fn sdc_gui::pgp::schema::grass_ignores(key: &str) -> bool`; `pub fn sdc_gui::pgp::settings::grass_view(s: &mut Settings, has_subsurface_map: bool)`; `pub fn sdc_gui::model::sets::is_pbr_grass(v: &SetView) -> bool`; `pgp_fields::Ctx::grass: bool`.

- [ ] **Step 1: Write the failing unit tests**

Append to `crates/gui/tests/pgp.rs`:

```rust

/// PBR grass design R4: the keys CS's grass shader never reads, in KEYS order, and the GUI switches of glint/fuzz.
#[test]
fn grass_hides_exactly_the_keys_rungrass_ignores() {
    let hidden: Vec<&str> = KEYS
        .iter()
        .map(|k| k.key)
        .filter(|k| grass_ignores(k))
        .collect();
    assert_eq!(
        hidden,
        [
            "auto_uv",
            "uv_scale",
            "emissive",
            "lock_emissive",
            "emissive_scale",
            "emissive_color",
            "parallax",
            "lock_parallax",
            "displacement_scale",
            "hair",
            "multilayer",
            "coat_color",
            "coat_specular_level",
            "coat_roughness",
            "coat_strength",
            "coat_diffuse",
            "coat_normal",
            "coat_parallax",
            "lock_cnr",
            "inner_uv_scale",
            "glint",
            "fuzz",
            "glint.screen_space_scale",
            "glint.log_microfacet_density",
            "glint.microfacet_roughness",
            "glint.density_randomization",
            "fuzz.texture",
            "fuzz.color",
            "fuzz.weight",
        ]
    );
    assert!(grass_ignores("glint_on") && grass_ignores("fuzz_on"));
}

/// PBR grass design R2, R3: hidden keys dropped; subsurface on by default only with an `_s`; an explicit off sticks.
#[test]
fn the_grass_view_drops_hidden_keys_and_defaults_subsurface_with_an_s() {
    let raw = s(&[
        ("emissive", json!(true)),
        ("uv_scale", json!(2.0)),
        ("roughness_scale", json!(0.8)),
    ]);
    let mut with_s = raw.clone();
    grass_view(&mut with_s, true);
    assert_eq!(
        with_s,
        s(&[("roughness_scale", json!(0.8)), ("subsurface", json!(true))])
    );
    let mut no_s = raw.clone();
    grass_view(&mut no_s, false);
    assert_eq!(no_s, s(&[("roughness_scale", json!(0.8))]));
    let mut off = s(&[("subsurface", json!(false))]);
    grass_view(&mut off, true);
    assert_eq!(off, s(&[("subsurface", json!(false))]));
    assert!(!feature_on(&off, Feature::Subsurface));
}
```

Append to `crates/gui/tests/pbr_export.rs`:

```rust

/// PBR grass design R3, R4: a pbr set switched to pbr_grass exports only grass keys and loses its emissive block;
/// the project file keeps the rest, so switching back restores them.
#[test]
fn a_pbr_grass_set_drops_the_keys_grass_ignores() {
    use engine::shader::Shader;
    let d = tempfile::tempdir().unwrap();
    let mut m = ready(d.path());
    m.set_json_value("landscape/rock01", "emissive", Some(json!(true)));
    m.set_json_value("landscape/rock01", "uv_scale", Some(json!(2.0)));
    m.set_json_value("landscape/rock01", "roughness_scale", Some(json!(0.8)));
    idle(&mut m);
    assert!(export_blocks(&m)
        .iter()
        .any(|i| i.text.contains("emissive is on")));
    m.set_shader("landscape/rock01", Some(Shader::PbrGrass));
    idle(&mut m);
    assert_eq!(export_blocks(&m), vec![]);
    let rock = |m: &sdc_gui::model::AppModel| {
        json_entries(m)
            .into_iter()
            .find(|(v, _)| v.id == "landscape/rock01")
            .unwrap()
            .1
            .settings
    };
    let s = rock(&m);
    assert!(
        !s.contains_key("emissive") && !s.contains_key("uv_scale"),
        "{s:?}"
    );
    assert_eq!(s.get("roughness_scale"), Some(&json!(0.8)));
    assert!(!s.contains_key("subsurface"), "no _s, so no subsurface default");
    m.set_shader("landscape/rock01", Some(Shader::Pbr));
    idle(&mut m);
    assert_eq!(rock(&m).get("uv_scale"), Some(&json!(2.0)));
}
```

Append to `crates/gui/tests/set_editor_ui.rs` (after `panel_harness` exists in the file, so at the end):

```rust

/// PBR grass design §4.3: a pbr_grass panel has no emissive, parallax, hair, coat, glint or fuzz switch, keeps the
/// grass keys, and unticking Subsurface stores `false` (the default would turn a removed key back on).
#[test]
fn a_pbr_grass_panel_shows_only_the_grass_keys() {
    let d = tempfile::tempdir().unwrap();
    let mut m = pbr_opened(d.path());
    m.set_shader("landscape/rock01", Some(engine::shader::Shader::PbrGrass));
    m.set_json_value("landscape/rock01", "subsurface", Some(serde_json::json!(true)));
    assert!(m.wait_idle(std::time::Duration::from_secs(60)));
    let mut h = panel_harness(m);
    h.run();
    for gone in ["Emissive", "Parallax", "Hair", "Multilayer (coat)", "Glint", "Fuzz"] {
        assert!(h.query_by_label(gone).is_none(), "{gone} is hidden on PBR grass");
    }
    h.get_by_label("Roughness scale");
    h.get_by_label("Specular level");
    h.get_by_label("Subsurface").click();
    h.run();
    assert_eq!(rock_settings(&h)["subsurface"], serde_json::json!(false));
}
```

- [ ] **Step 2: Run them to verify they fail**

Run: `CARGO_TARGET_DIR=F:/ai/dds/converter-nif-p1/target cargo test -p sdc-gui --release --test pgp --test pbr_export --test set_editor_ui`
Expected: FAIL to compile: `cannot find function grass_ignores`, `cannot find function grass_view`.

- [ ] **Step 3: Implement the hide list and the grass view**

`crates/gui/src/pgp/schema.rs`, after `pub fn spec(…)`:

```rust

/// PBR grass design §4.3 (ruling R4): the keys CS's `RunGrass.hlsl` never reads, hidden from a `pbr_grass` set's
/// form and dropped from its entry. `glint_on`/`fuzz_on` are the GUI switches of `glint`/`fuzz`.
pub fn grass_ignores(key: &str) -> bool {
    ["emissive", "coat_", "glint", "fuzz"]
        .iter()
        .any(|p| key.starts_with(p))
        || matches!(
            key,
            "lock_emissive"
                | "parallax"
                | "displacement_scale"
                | "lock_parallax"
                | "hair"
                | "multilayer"
                | "inner_uv_scale"
                | "lock_cnr"
                | "uv_scale"
                | "auto_uv"
        )
}
```

`crates/gui/src/pgp/settings.rs`, at the end:

```rust

/// PBR grass design §4.3 (rulings R2, R3): the settings a `pbr_grass` set shows, checks and exports. The keys the
/// grass shader ignores are dropped; subsurface, the only translucency grass has, is on unless the set says `false`,
/// when the set has an `_s` (without one, export would block on it).
pub fn grass_view(s: &mut Settings, has_subsurface_map: bool) {
    s.retain(|k, _| !super::schema::grass_ignores(k));
    if has_subsurface_map {
        s.entry("subsurface".to_string())
            .or_insert(Value::Bool(true));
    }
}
```

`crates/gui/src/model/sets.rs`, after `is_json_set`:

```rust

/// The set's pick is `pbr_grass` (PBR grass design §4.3): its PGPatcher settings are the grass view.
pub fn is_pbr_grass(v: &SetView) -> bool {
    v.shader
        .as_ref()
        .is_some_and(|p| p.shader == Shader::PbrGrass)
}
```

and in `build_views`, in the `// PBR, derived match, prefix.` loop, replace

```rust
        v.pbr = st.is_some_and(is_pbr_diffuse);
        v.skin = st == Some(Subtype::SkinAlbedo);
```

with

```rust
        v.pbr = st.is_some_and(is_pbr_diffuse);
        v.skin = st == Some(Subtype::SkinAlbedo);
        if is_pbr_grass(v) {
            let has_s = !matches!(
                v.slots.get(&SlotKind::Subsurface),
                None | Some(SlotFill::Empty)
            );
            crate::pgp::settings::grass_view(&mut v.settings, has_s);
        }
```

- [ ] **Step 4: Implement the form changes**

`crates/gui/src/ui/pgp_fields.rs`:

(a) Imports: change

```rust
use crate::model::sets::{set_views, slot_matches, SetView, SlotFill, SlotKind, SlotMatch};
```

to

```rust
use crate::model::sets::{
    is_pbr_grass, set_views, slot_matches, SetView, SlotFill, SlotKind, SlotMatch,
};
```

and

```rust
use crate::pgp::schema::{Group, KeyKind, KeySpec, KEYS};
```

to

```rust
use crate::pgp::schema::{grass_ignores, Group, KeyKind, KeySpec, KEYS};
```

(b) `struct Ctx`, after `fuzz_filled: bool,`:

```rust
    /// PBR grass design §4.3: the set is `pbr_grass`, so the keys grass ignores are not drawn.
    pub(crate) grass: bool,
```

(c) `context`, in the `Ctx { … }` literal after `fuzz_filled: …,`:

```rust
        grass: v.as_ref().is_some_and(is_pbr_grass),
```

(d) `panel`: in the MAIN loop replace

```rust
        let keys: Vec<&KeySpec> = KEYS
            .iter()
            .filter(|k| k.group == g && !k.advanced && k.kind != KeyKind::Slot)
            .collect();
```

with

```rust
        let keys: Vec<&KeySpec> = KEYS
            .iter()
            .filter(|k| k.group == g && !k.advanced && k.kind != KeyKind::Slot)
            .filter(|k| !(c.grass && grass_ignores(k.key)))
            .collect();
```

and in the ADVANCED loop replace

```rust
                let keys: Vec<&KeySpec> =
                    KEYS.iter().filter(|k| k.group == g && k.advanced).collect();
```

with

```rust
                let keys: Vec<&KeySpec> = KEYS
                    .iter()
                    .filter(|k| k.group == g && k.advanced)
                    .filter(|k| !(c.grass && grass_ignores(k.key)))
                    .collect();
```

(e) `control`, the `OnlyTrue | Object` arm: replace

```rust
            if ui.checkbox(&mut on, k.label).changed() {
                edits.push((key, on.then_some(json!(true))));
            }
```

with

```rust
            if ui.checkbox(&mut on, k.label).changed() {
                // PBR grass: subsurface defaults on (`grass_view`), so off is stored to stick; reset restores it.
                let keep_off = c.grass && key == "subsurface";
                edits.push((key, (on || keep_off).then_some(json!(on))));
            }
```

- [ ] **Step 5: Run the tests**

Run: `CARGO_TARGET_DIR=F:/ai/dds/converter-nif-p1/target cargo test -p sdc-gui --release --test pgp --test pbr_export --test set_editor_ui --test sets_model`
Expected: PASS (existing tests unchanged: a `pbr` set gets no grass view, and `keep_off` is false for it).

- [ ] **Step 6: Gate and commit**

```bash
CARGO_TARGET_DIR=F:/ai/dds/converter-nif-p1/target cargo fmt --all --check
CARGO_TARGET_DIR=F:/ai/dds/converter-nif-p1/target cargo clippy --workspace --all-targets --release -- -D warnings
CARGO_TARGET_DIR=F:/ai/dds/converter-nif-p1/target cargo test --workspace --release
git add crates/gui/src/pgp/schema.rs crates/gui/src/pgp/settings.rs crates/gui/src/model/sets.rs crates/gui/src/ui/pgp_fields.rs crates/gui/tests/pgp.rs crates/gui/tests/pbr_export.rs crates/gui/tests/set_editor_ui.rs
git commit -m "feat(gui): PBR grass sets show and export only the keys grass reads, subsurface on with an _s" -m "Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

---

### Task 6: "Approximated" badge in the set preview

**Files:**
- Modify: `crates/gui/src/model/set_preview.rs:1030` (const), `:1102-1111` (`SetFingerprint`), `:1120-1129` (`fingerprint`)
- Modify: `crates/gui/src/ui/set_preview.rs:4-8` (import), `draw` (after the `JSON_WINS`/`NEEDS_CS` lines)
- Modify: `crates/gui/tests/set_preview_model.rs:391` (literal), `crates/gui/tests/set_preview_ui.rs`

**Interfaces:**
- Consumes: `sets::is_pbr_grass` (Task 5).
- Produces: `sdc_gui::model::set_preview::SetFingerprint::grass: bool`; `pub const sdc_gui::model::set_preview::GRASS_APPROX: &str`.

- [ ] **Step 1: Write the failing test**

Append to `crates/gui/tests/set_preview_ui.rs`:

```rust

/// PBR grass design §4.5: a pbr_grass set says its preview is approximated; a pbr set does not.
#[test]
fn a_pbr_grass_set_shows_the_approximation_badge() {
    let d = tempfile::tempdir().unwrap();
    let mut h = harness(
        rock_selected(d.path()),
        SetPreview::new(None, None, None, DEFAULT_CAP),
    );
    settle(&mut h);
    assert!(h.query_by_label(GRASS_APPROX).is_none());
    let d = tempfile::tempdir().unwrap();
    let mut m = rock_selected(d.path());
    m.set_shader("landscape/rock01", Some(engine::shader::Shader::PbrGrass));
    assert!(m.wait_idle(Duration::from_secs(60)));
    let mut h = harness(m, SetPreview::new(None, None, None, DEFAULT_CAP));
    settle(&mut h);
    h.get_by_label(GRASS_APPROX);
}
```

In `crates/gui/tests/set_preview_model.rs`, `fp_of`, add `grass: false,` after `landscape: false,`.

- [ ] **Step 2: Run it to verify it fails**

Run: `CARGO_TARGET_DIR=F:/ai/dds/converter-nif-p1/target cargo test -p sdc-gui --release --test set_preview_ui a_pbr_grass_set`
Expected: FAIL to compile: `cannot find value GRASS_APPROX`.

- [ ] **Step 3: Implement**

`crates/gui/src/model/set_preview.rs`, after `pub const NEEDS_CS: &str = …;`:

```rust
/// PBR grass design §4.5 (owner D5): the object PBR path stands in for CS's `RunGrass.hlsl`.
pub const GRASS_APPROX: &str = "PBR grass approximated: drawn with the object PBR shader (no wrapped diffuse, wind \
or grass LOD; UV scale and micro-shadows still apply). PGPatcher 1.2.0 does not patch grass meshes.";
```

`SetFingerprint`, after the `landscape` field:

```rust
    /// PBR grass design §4.5: the set's pick is `pbr_grass` (the "approximated" badge).
    pub grass: bool,
```

`fingerprint`, in the literal after the `landscape: …,` expression:

```rust
        grass: crate::model::sets::is_pbr_grass(&v),
```

`crates/gui/src/ui/set_preview.rs`: add `GRASS_APPROX` to the `use crate::model::set_preview::{…}` list (after `EncState,`), and in `draw` replace

```rust
    if fp.entry.is_some() {
        ui.label(JSON_WINS);
    } else {
        ui.colored_label(AMBER, NEEDS_CS);
    }
```

with

```rust
    if fp.entry.is_some() {
        ui.label(JSON_WINS);
    } else {
        ui.colored_label(AMBER, NEEDS_CS);
    }
    if fp.grass {
        ui.colored_label(AMBER, GRASS_APPROX);
    }
```

- [ ] **Step 4: Run the tests**

Run: `CARGO_TARGET_DIR=F:/ai/dds/converter-nif-p1/target cargo test -p sdc-gui --release --test set_preview_ui --test set_preview_model`
Expected: PASS.

- [ ] **Step 5: Gate and commit**

```bash
CARGO_TARGET_DIR=F:/ai/dds/converter-nif-p1/target cargo fmt --all --check
CARGO_TARGET_DIR=F:/ai/dds/converter-nif-p1/target cargo clippy --workspace --all-targets --release -- -D warnings
CARGO_TARGET_DIR=F:/ai/dds/converter-nif-p1/target cargo test --workspace --release
git add crates/gui/src/model/set_preview.rs crates/gui/src/ui/set_preview.rs crates/gui/tests/set_preview_model.rs crates/gui/tests/set_preview_ui.rs
git commit -m "feat(gui): set preview says PBR grass is approximated" -m "Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

---

### Task 7: User guide and Note AG

**Files:**
- Modify: `docs/gui.md` (new section after "## Complex grass", before "## Where settings live")
- Modify: `docs/DECISIONS.md` (append at the end)

**Interfaces:**
- Consumes: everything above. Produces: documentation only.

- [ ] **Step 1: Add the user-guide section**

In `docs/gui.md`, insert before `## Where settings live`:

```markdown
## PBR grass

Community Shaders draws grass with its own shader, which reads only the albedo, `_n`, `_rmaos` and an optional `_s`. A PBR set whose albedo a grass (GRAS) record uses gets the **PBR grass** shader automatically after a Meshes-page scan that included plugins; you can also pick it in the set's Shader list (Community Shaders target only). The TruePBR panel of a PBR grass set shows only what grass reads: roughness scale, specular level, subsurface (on by default when the set has an `_s`), matching, vertex colours, locks and slots. Other keys the set already has stay in `sdc-project.toml` and come back if you switch the set to PBR, but they are not exported. Every PBR grass set shows `PBR-GRASS-PGPATCHER`: PGPatcher 1.2.0 does not patch grass meshes; it needs 2.1.0 or later. `PBR-GRASS-DETECT` on the albedo means its bottom-left texel looks like a complex-grass normal, so Community Shaders' grass shadow pass would cut the shadows wrong; change that texel. The 3D preview draws PBR grass with the object PBR shader and says so. The command line never picks PBR grass by itself; use `--set set:<id>.shader=pbr_grass`.
```

(If Task 3 was skipped, delete the `PBR-GRASS-DETECT` sentence.)

- [ ] **Step 2: Append Note AG**

At the end of `docs/DECISIONS.md`:

```markdown

## Note AG — PBR grass material type (plan rulings, 2026-10-14)

Spec `F:\ai\dds\nifparseref\plan\pbr-grass\2026-10-14-pbr-grass-design.md` (owner-approved 2026-10-14, including edits to `crates/engine` `shader.rs`/`api.rs`/`codes.rs`, `schema/`, `crates/gui/src/pgp/` and `model/sets.rs`); plan `…\2026-10-14-plan-pbr-grass.md`; evidence `…\01-research.md`.

- **AG1 Shader.** `pbr_grass` ("PBR grass", CS only, second in the list after `pbr`) requires albedo, `_n`, `_rmaos` and reads an optional `_s`, with the PBR sub-types. `_g`, `_p`, `_cnr` and `_f` members get `SHADER-EXTRA-MAP`. No member defaults, no encoding change.
- **AG2 Auto-pick.** Inside the PBR rule: a PBR set with a member listed in `BatchRequest::gras_all_diffuses` picks `pbr_grass` (`auto:pbr_grass`). The list is `Store::gras_all_diffuse_textures`, the GRAS slot-0 query of X4 without X5's TruePBR filter, so both pre-patched PBR grass NIFs and vanilla grass NIFs count. The CLI sends none (as X11). An explicit choice wins.
- **AG3 Grass view.** `model::sets::build_views` gives a `pbr_grass` set `pgp::settings::grass_view` of its settings: the keys of `pgp::schema::grass_ignores` (emissive, parallax/displacement, hair, coat/multilayer, glint, fuzz, `uv_scale`, `auto_uv`) are dropped, and `subsurface` is on unless set `false`, when the set has an `_s` (without one, export would block on it). The form, export checks and writer read the view; `sdc-project.toml` keeps the hidden keys. Unticking Subsurface on a grass set stores `false`. The JSON editor window is not filtered.
- **AG4 Warnings.** `PBR-GRASS-PGPATCHER` (warning, set) on every `pbr_grass` pick: PGPatcher 1.2.0 does not patch grass meshes, 2.1.0 or later does; sdc exports anyway (owner). `PBR-GRASS-DETECT` (warning, job) on the colour member of a `pbr_grass` set whose bottom-left texel has `|L-1| < 0.03 + 0.01` (`Analysis.corner_err`, any shape): CS's grass shadow pass would treat the albedo as complex grass. No Grass Lighting warning (owner). Codes 90 → 92.
- **AG5 Preview.** The object PBR path with an amber "approximated" line (`set_preview::GRASS_APPROX`). A `RunGrass.hlsl` render is later work.
- **AG6 Not built.** Flat-lit, opacity ≥ 1 and hidden-key lints; the SLSF2.30 sphere-normal warning; pgpsim's GRAS skip under 1.2.0; hiding the CS texture-set block for grass sets.
- **AG7 Versions.** Engine version stays 0.7.0 (no preset or output change); scan database schema stays 7 (query only).
```

(If Task 3 was skipped, replace AG4's second sentence with "The complex-grass negative check is not built: it needs `Analysis.corner_err` in `analyse.rs`, not approved." and write "Codes 90 → 91".)

- [ ] **Step 3: Final gate and commit**

```bash
CARGO_TARGET_DIR=F:/ai/dds/converter-nif-p1/target cargo fmt --all --check
CARGO_TARGET_DIR=F:/ai/dds/converter-nif-p1/target cargo clippy --workspace --all-targets --release -- -D warnings
CARGO_TARGET_DIR=F:/ai/dds/converter-nif-p1/target cargo test --workspace --release
git diff --stat b4c228e -- Cargo.lock
git add docs/gui.md
git commit -m "docs(gui): PBR grass" -m "Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
git add docs/DECISIONS.md
git commit -m "docs: DECISIONS Note AG (PBR grass)" -m "Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

Expected: everything green; the `Cargo.lock` diff is empty.

---

## Self-review

**Spec coverage.**
- D1 shader row, CS only, PBR sub-types → Task 1.
- D2 auto-pick from the TruePBR-keeping query, explicit `pbr` wins → Task 1 (rule), Task 4 (query and GUI evidence).
- D3 trimmed form, subsurface default, writer unchanged → Task 5.
- D4 PGPatcher warning → Task 2; negative check → Task 3 (gated on R1); no Grass Lighting warning (nothing built).
- D5 badge → Task 6.
- R9 Note AG → Task 7.
- Spec §5 error cases: fallback (Task 1 `vanilla_and_cpm_fall_back_from_pbr_grass`), extra maps (Task 1 `pbr_grass_reports_the_maps_grass_cannot_read`), hidden keys kept in the file (Task 5), infinite corner (Task 3 unit test). The no-scan-DB case reuses the existing `votes_landed` path with no new branch.
- Gap, accepted: the subsurface default *with* an `_s` is tested at the function level (`grass_view`) only. The shared GUI fixture has no `_s` member, and adding one risks its `_s` readings (DECISIONS Note R).

**Placeholder scan.** Every code step has the code. The only conditional text is Task 7's "if Task 3 was skipped" wording, which depends on the owner's R1 answer.

**Type consistency.** `gras_all_diffuses` (engine field, `AppModel` field, `EngineInputs` field), `gras_all` (`MeshEvidence`), `gras_all_diffuse_textures` (modscan), `gras_all_diffuses(db)` (meshes.rs), `PickInput.gras`, `pick_for_set(…, gras: &BTreeSet<String>)`, `AutoRule::PbrGrass`, `Code::PbrGrassPgpatcher`, `Code::PbrGrassDetect`, `pbr_grass_albedo_note(&ShaderPick, Subtype, f32)`, `Analysis.corner_err: f32`, `grass_ignores(&str)`, `grass_view(&mut Settings, bool)`, `is_pbr_grass(&SetView)`, `Ctx.grass`, `SetFingerprint.grass`, `GRASS_APPROX`: the same names in every task that uses them.

**Review Focus.** Each of the five lines names its test, in the task that owns the code.
