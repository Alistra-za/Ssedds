# Complex grass sub-type Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** A new sub-type `complex_grass` (CS and ENB/`cpm` targets) that encodes a 1:2 complex-grass texture correctly as one BC7 UNORM file, warns when one is malformed, guards the bottom-left detection texel after compression, and is tagged automatically only when the plugin scanner, the shape and the content all agree.

**Architecture:** `complex_grass` is a Mixed-Packed sub-type whose profile is the stored view of the file (linear, UNORM, BC7). A new module `crates/engine/src/grass.rs` holds everything grass-specific: the analysis facts of a 1:2 image (`GrassFacts`, computed by `analyse`), the per-half build (`grass::build`: split at `h/2`, top half through the existing diffuse path with coverage, bottom half through the existing normal path with data alpha, stacked per level in stored form), and the auto-detect predicate. `pipeline::prepare_then` calls `grass::build` in place of Preprocess + Mipmaps for this sub-type. The encoder re-encodes the bottom-left block at bc7e's slowest level when it decodes off unit length. `validate` names a file whose texel still fails (`VAL-GRASS-DETECT`). Resolve reports the §4 checks from the analysis facts. Auto-detect is a High suggestion added by `policy::resolve_file_with` when `BatchRequest::gras_diffuses` (filled by the GUI from the scan DB) names the file. The GUI pickers hide the sub-type under vanilla.

**Tech Stack:** Rust workspace; `bcdec_rs` and `encoders-sys` (pub API only, already engine deps); rusqlite in `modscan`; egui GUI model. No new crates.

**Spec:** `docs/superpowers/specs/2026-10-12-complex-grass-design.md` (approved 2026-10-12; binding, including the ENB assumption and the vanilla ruling). Read it with this plan. Task 7 also reads the scanner plan `docs/superpowers/plans/2026-10-12-plan-esp-scanner.md`. Plan written against `nif/main @ 89b56d3`; executed on `feat/complex-grass` from main 672a26f (see the ledger Ruling 0).

## Global Constraints

- Branch `nif/g-grass`, created from `nif/main` **after the plugin scanner branch `nif/f-esp` is merged into it** (Tasks 1–6 do not need the scanner; Task 7 does). Own worktree `F:\ai\dds\converter-nif-grass`. Read `F:\ai\dds\nifparseref\plan\CONCURRENCY.md` first. Never touch `F:\ai\dds\converter`. Never push. Never merge into `main`.
- `crates/engine` is editable **for this feature only** (design §1, §7). `presets/` is not edited (no preset change is needed).
- Shared files are add-only, in small separate commits: `Cargo.toml`, `Cargo.lock`, `crates/gui/Cargo.toml`, `crates/gui/src/ui/mod.rs`, `crates/gui/src/model/mod.rs`, `crates/gui/src/app.rs`, `crates/gui/src/settings.rs`. This plan touches only `crates/gui/src/model/mod.rs` (Task 7, own commit). `Cargo.lock` must stay unchanged; if it changes, stop and report.
- Off-limits (read pub items only): `crates/cli`, `crates/encoders-sys`, `crates/gui/src/pgp`, `crates/gui/src/sets_doc.rs`, `crates/gui/src/model/sets.rs`, `crates/gui/src/model/pbr_export.rs`, `docs/DECISIONS.md`, golden and `gui_repro` tests. `schema/` is off-limits **except the one flagged regeneration commit of Task 1 (needs the owner's OK before merge)**.
- No new crate dependencies.
- Owner data is read-only. Tests never read owner data, except the `#[ignore]` test gated on `SDC_ACHERUS` (Task 4), and never the real `%LOCALAPPDATA%\sdc\scan.db` (every test DB is in a `tempfile` dir; GUI tests keep `scan_db = None` unless they set a temp DB). Unit-test images are synthetic, generated in test code.
- Golden tests must keep passing **unchanged**: a new sub-type must not change any existing output. `default_profiles_hash_as_before` (golden) plans a 64×64 file tagged with every `Subtype::ALL` id; for `complex_grass` that job resolves with `GRASS-SHAPE` (Error) and still emits `file_planned` with a profile, which is all the test reads.
- Gates before every commit: `cargo fmt --all && cargo clippy --workspace --all-targets -- -D warnings && cargo test --workspace`. One exception, Task 1 Step 5 only: the types commit runs `cargo test --workspace -- --skip committed_schemas_match_the_types`, because the schema is regenerated in the very next commit (owner asked for `schema/` in its own commit). Commit messages end with `Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>`. `git add` explicit paths only.
- Values from the spec, verbatim: CS `ComplexGrassThreshold` = **0.03**; guard margin **0.01** (encoded texel must reach `|L−1| < 0.02`); unit-length = **`|L−1| < 0.1`**; bottom-half share **≥ 99 %**; output **BC7 UNORM**; detection texel = **`(0, height−1)` of mip 0**.
- Paths are relative to the worktree root. Edits are anchored by quoted text. Line numbers are those at `89b56d3` and move as earlier tasks land.
- Review routing (owner): per-task reviews on Codex gpt-6-astra (low/medium/high by risk); Opus only for the final whole-branch review.

## Rulings on the spec (the plan argues from these)

- **R1 Vanilla.** "Same handling as `complex_material`" cannot be meant literally: `complex_material` under vanilla is `CFG-UNAVAILABLE` with an `envmask` fallback, which would not "encode as `complex_grass` unchanged". The explicit sentence wins: `availability(Vanilla, ComplexGrass)` stays `Available` (so the file encodes unchanged), `check_profile` adds `GRASS-VANILLA` (Warning), the GUI pickers hide it under vanilla through a new `policy::offered(t, s)`, and auto-detect never fires under vanilla. A new `Availability` variant is impossible: `crates/cli/src/verbs.rs` matches it exhaustively and `cli` is off-limits.
- **R2 ENB = the `cpm` target** ("CPM / ENB complex material" preset). `complex_grass` is `Available` there and under `cs`; the guard applies to both (owner ruling: same detector).
- **R3 Schema.** `schema/*.json` are regenerated byte-for-byte from the types (`tests/schemas.rs::committed_schemas_match_the_types`). The new `Subtype` variant and the six new `Code` variants all land there, so `schema/` **must** change. Task 1 declares every new code at once so there is exactly one regeneration, in its own commit, flagged for the owner.
- **R4 Category and profile.** `complex_grass` is `MixedPacked` with the base defaults (content `linear`, tag `unorm`, `bc7`, bc7e veryslow): the profile describes the stored file ("one view, raw data", §3.5). The top half is always built as sRGB colour, the bottom half as a DirectX normal map; `channels.*` (swizzle, flip_green, reconstruct_z) apply to both halves as written. No preset TOML changes.
- **R5 Alpha (§3.2 "as the diffuse rules decide").** `complex_grass` joins `Subtype::alpha_from_mesh`, so it gets exactly the diffuse rules: mesh evidence from the scan (`Provenance::MeshAlpha`, M6 Track E) when there is some, else the low-confidence cutout/blend proposal and the alpha question. The proposal reads the **top half's** near-binary share (`GrassFacts::top_near_binary`), because the whole-image share is dragged down by the specular half. The bottom half's alpha is always `data`, except when the job's policy is `remove` (a source with no alpha channel, or the user's choice): then both halves are opaque 255, so validation (`remove` guarantee), the GUI preview and `LOSS-ALPHA-DATA` stay consistent.
- **R6 Mip chain length.** Each half is `w`×`w`, so the chain has `log2(w)+1` levels and ends at 1×2 (§3.4), one level short of the full chain of a `w`×`2w` image. `check_header` rejects only *more* than the full chain; `expected_levels` follows the inputs, so nothing else changes.
- **R7 Resize.** Built-in presets never resize `complex_grass` (the Compact presets list sub-types explicitly). A user `resize` that would change the size is `CFG-INVALID` for `complex_grass` (each half would need its own resize; `half` disagrees between whole and half at `w = 512`). `grass::build` never resizes.
- **R8 Guard placement and "the file fails".** The encoder checks the bottom-left block of **every** level it encodes for this sub-type (one block per level, negligible) and re-encodes it at `Effort::Slowest` when it decodes at `|L−1| ≥ 0.02`; a preview that starts below mip 0 is guarded the same way as the export. `validate` reads the first decoded level and emits `VAL-GRASS-DETECT` (Error) when it still fails. That makes `passed = false`, the manifest records it and the CLI exits 4: the existing contract for every validation Error (the output is written, never silently). If the owner wants such a file not written at all, that is a separate pipeline change.
- **R9 Source corner already failing (§4 last bullet).** The bottom half's mip 0 is renormalised by the existing normal path (`mips.normal_base = renormalize`), which turns the corner texel into a unit vector. `GRASS-DETECT-SOURCE` still warns (the source is malformed) and its message says the written file passes unless `mips.normal_base = keep`.
- **R10 §4 checks live in Resolve** (`check_profile`), from `Analysis` facts computed while the image is loaded (`resolve_one` drops the image before Resolve). `GRASS-SHAPE` is an Error, so a malformed file never reaches the encoder.
- **R11 Auto-detect = a High suggestion** placed first, the same mechanism as `_m` → `complex_material`. The GUI plans with `AcceptSuggestions::High` (M6 Track E), so there it is a real tag; the CLI never fills `gras_diffuses`, so the CLI never auto-tags (§5 last line holds without the scanner). A `--set`, rule, `[files]` entry or explicit set shader decides before suggestions are read, so "a user tag always wins, both ways" needs no extra code.
- **R12 Scanner interface.** The merged scanner provides no GRAS query. Task 7 adds `Store::gras_diffuse_textures` in `crates/modscan/src/plugin_store.rs` (modscan is this team's crate), on the scanner's schema v4 tables (`rec`, `rec_winner`, `model_ref`; scanner plan Task 4) and the NIF tables. Paths are the store form (lower case, `/`, `textures/…`; scanner ruling R11).

## Review Focus

1. **A non-1:2 or non-power-of-two file tagged `complex_grass` (square, 4096×2048 landscape orientation, 48×96) is a named `GRASS-SHAPE` error, never a panic**, including when the GUI previews that job (`grass::build` returns `Err`). Pinned by Task 2 `shape_errors_are_grass_shape` and Task 3 `a_wrong_shape_is_an_error_not_a_panic`.
2. **A complex-grass source with no alpha channel** resolves `remove` (high-confidence, as diffuse), encodes with 255 alpha in both halves, warns `GRASS-TOP-OPAQUE` and passes validation (no `VAL-ALPHA-GUARANTEE`). Pinned by Task 4 `an_rgb_grass_source_encodes_opaque_and_passes`.
3. **An ordinary tall (1:2) texture that is not tagged complex grass is untouched**: no `GRASS-*` code, same profile as before, and no auto-detect without a GRAS record. Pinned by Task 2 `a_tall_diffuse_raises_no_grass_code` and Task 5 `each_condition_false_alone_leaves_the_classification`.
4. **Vanilla:** a hand-tagged file encodes as `complex_grass` with `GRASS-VANILLA` (no `CFG-UNAVAILABLE`), the pickers do not offer it, and auto-detect does not fire. Pinned by Task 2 `vanilla_tag_warns_and_stays_complex_grass`, Task 5 `vanilla_is_never_auto_tagged`, Task 6 `pickers_hide_complex_grass_under_vanilla`.
5. **A few stray non-unit texels in the bottom half** (a painting slip): below 1 % no `GRASS-NORMALS`, at 2 % the warning; neither affects the corner test. Pinned by Task 2 `bottom_normals_share_threshold`.

Accepted limits (not tested; listed for the reviewer):
- `VAL-COVERAGE` and `base_coverage` stay whole-level: the bottom half's specular alpha counts as a constant. Bottom alpha that straddles the cutout threshold can produce a spurious `VAL-COVERAGE` warning (never an error). The owner's example (alpha 10–16) is far below 127. The synthetic e2e tests (binary dot alpha) may also log `VAL-COVERAGE` warnings, because a level with few distinct alpha values cannot reach the base coverage exactly; `passed` stays true and no test asserts their absence.
- Auto-detect needs a scan DB and a plan after it; the GUI reads the GRAS diffuses with the mesh evidence after each plan (`refresh_mesh_votes`), so a new scan shows up on the next plan.
- `colour_mesh_key` finds mesh alpha for a grass texture through its suffix-less (or `_d`) output stem, not through the sub-type list; a grass diffuse with another suffix gets no mesh evidence and asks the alpha question.
- Non-box `mips.filter` on `complex_grass`: `filter_allowed` rejects it for a Mixed-Packed profile, so the existing `CFG-FILTER-IGNORED`/`CFG-INVALID` paths apply and the halves use box.

## Task order and dependencies

**1 → 2 → 3 → 4 → 5 → 6 → 7 → 8.** Task 1 adds the ids and regenerates `schema/` (two commits). Task 2 adds `grass.rs` facts, the Resolve checks, the alpha rule and `offered`. Task 3 the per-half build. Task 4 the guard, `VAL-GRASS-DETECT` and the owner `#[ignore]` test. Task 5 the engine side of auto-detect (tests fill `BatchRequest::gras_diffuses` directly). Task 6 the GUI picker filter. **Task 7 depends on the plugin scanner (`nif/f-esp` merged into `nif/main`)** and is the last code task. Task 8 is the owner checklist. Tasks 2–5 append to `crates/engine/tests/complex_grass.rs`, which Task 2 creates.

---

### Task 1: sub-type and code ids, schema regeneration

**Files**
- Modify: `crates/engine/src/config.rs` (enum `Subtype`, `Subtype::ALL`, `category()`, two tests)
- Modify: `crates/engine/src/codes.rs` (six codes, header doc, count test)
- Regenerate (own commit, flagged): `schema/config.schema.json`, `schema/dds-validation.schema.json`, `schema/event.schema.json`, `schema/manifest.schema.json`, `schema/preset.schema.json`, `schema/profile.schema.json`

**Interfaces**
- Produces:
  - `Subtype::ComplexGrass` (id `"complex_grass"`), last in the enum and in `Subtype::ALL` (`[Subtype; 29]`); `category()` = `Category::MixedPacked`.
  - `Code::GrassShape` `"GRASS-SHAPE"` Error; `Code::GrassTopOpaque` `"GRASS-TOP-OPAQUE"` Warning; `Code::GrassNormals` `"GRASS-NORMALS"` Warning; `Code::GrassDetectSource` `"GRASS-DETECT-SOURCE"` Warning; `Code::GrassVanilla` `"GRASS-VANILLA"` Warning; `Code::ValGrassDetect` `"VAL-GRASS-DETECT"` Error. Appended after `DdsHeaderUnpatchable`, so no existing code's order moves. `Code::ALL.len() == 81`.

- [ ] **Step 1: Write the failing tests**

In `crates/engine/src/config.rs`, test `subtypes_map_to_the_categories_of_section_4_1`: replace `assert_eq!(Subtype::ALL.len(), 28);` with

```rust
        assert_eq!(Subtype::ALL.len(), 29);
        assert_eq!(Subtype::ComplexGrass.id(), "complex_grass");
        assert_eq!(Subtype::ComplexGrass.category(), Category::MixedPacked);
        assert_eq!(Subtype::ALL.last(), Some(&Subtype::ComplexGrass));
```

and in `subtype_ids_are_unique_and_round_trip` replace `assert_eq!(ids.len(), 28);` with `assert_eq!(ids.len(), 29);`.

In `crates/engine/src/codes.rs` tests, replace `assert_eq!(Code::ALL.len(), 75);` with

```rust
        assert_eq!(Code::ALL.len(), 81);
        assert_eq!(Code::GrassShape.severity(), Severity::Error);
        assert_eq!(Code::ValGrassDetect.severity(), Severity::Error);
        assert_eq!(Code::GrassVanilla.severity(), Severity::Warning);
        assert_eq!(Code::parse("GRASS-DETECT-SOURCE"), Some(Code::GrassDetectSource));
        assert_eq!(Code::ALL.last(), Some(&Code::ValGrassDetect));
```

- [ ] **Step 2: Run them to see them fail**

Run: `cargo test -p engine --lib config::tests codes::tests`
Expected: compile error `E0599: no variant or associated item named ComplexGrass found for enum Subtype` (and the same for `GrassShape` …).

- [ ] **Step 3: Implement**

`crates/engine/src/config.rs`, enum `Subtype`: after `    SkinWet,` (the last variant, followed by `}`) add

```rust
    // Complex grass (design 2026-10-12): one 1:2 image, top = diffuse + alpha, bottom = normal + specular in alpha.
    // A `//` comment on purpose: a `///` doc on one variant makes schemars emit `oneOf` instead of the flat `enum`,
    // which rewrites every schema and drops the `rfaos` alias `subtype_schema` pushes into `enum`.
    ComplexGrass,
```

`Subtype::ALL`: change `pub const ALL: [Subtype; 28]` to `pub const ALL: [Subtype; 29]` and after the last `            SkinWet,` inside the array add `            ComplexGrass,`.

`category()`: change

```rust
            ComplexMaterial | Rmaos | SkinRfaos | Flow | GenericTwoChannel | GenericData => {
```

to

```rust
            ComplexMaterial | Rmaos | SkinRfaos | Flow | GenericTwoChannel | GenericData
            | ComplexGrass => {
```

`crates/engine/src/codes.rs`: replace

```rust
    DdsHeaderUnpatchable = "DDS-HEADER-UNPATCHABLE" => Warning,
}
```

with

```rust
    DdsHeaderUnpatchable = "DDS-HEADER-UNPATCHABLE" => Warning,
    GrassShape = "GRASS-SHAPE" => Error,
    GrassTopOpaque = "GRASS-TOP-OPAQUE" => Warning,
    GrassNormals = "GRASS-NORMALS" => Warning,
    GrassDetectSource = "GRASS-DETECT-SOURCE" => Warning,
    GrassVanilla = "GRASS-VANILLA" => Warning,
    ValGrassDetect = "VAL-GRASS-DETECT" => Error,
}
```

and in the module doc replace the whole last doc line `` //! `flip_green` that contradicts `green_convention`) (M6 Track D). `` with these two lines:

```rust
//! `flip_green` that contradicts `green_convention`) (M6 Track D) and the five `GRASS-*` codes and VAL-GRASS-DETECT
//! (complex grass design 2026-10-12 §3.6, §3.7, §4).
```

- [ ] **Step 4: Run the tests**

Run: `cargo test -p engine --lib config::tests codes::tests`
Expected: PASS. Then `cargo test -p engine --test schemas` FAILS with `schema/config.schema.json is stale` (expected until Step 6).

- [ ] **Step 5: Commit the types (schema test skipped once)**

```bash
cargo fmt --all && cargo clippy --workspace --all-targets -- -D warnings && cargo test --workspace -- --skip committed_schemas_match_the_types
git add crates/engine/src/config.rs crates/engine/src/codes.rs
git commit -m "engine: complex_grass sub-type id (Mixed-Packed) and GRASS-*/VAL-GRASS-DETECT codes (complex grass design)

schema/ is regenerated in the next commit.

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

- [ ] **Step 6: Regenerate `schema/` (own commit, flagged for the owner)**

```bash
cargo run -q -p sdc -- presets schema all --out schema
git diff --stat -- schema/
```

Expected: exactly the six files listed above change. Review `git diff schema/`: every added line belongs to a `complex_grass` entry (the sub-type enums and the `subtypes` table of `config`/`preset`), or to a `GRASS-*`/`VAL-GRASS-DETECT` code, or is the comma before one. If any other line changes, stop and report. Then:

```bash
cargo fmt --all && cargo clippy --workspace --all-targets -- -D warnings && cargo test --workspace
git add schema/config.schema.json schema/dds-validation.schema.json schema/event.schema.json schema/manifest.schema.json schema/preset.schema.json schema/profile.schema.json
git commit -m "schema: regenerate for complex_grass and the GRASS-*/VAL-GRASS-DETECT codes (NEEDS OWNER OK: schema/ edit)

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

Expected: all tests pass, golden included.

---

### Task 2: grass facts in `analyse`, the §4 checks, alpha rule, `offered`

**Files**
- Create: `crates/engine/src/grass.rs`
- Modify: `crates/engine/src/lib.rs` (`pub mod grass;`)
- Modify: `crates/engine/src/analyse.rs` (`Analysis::grass`)
- Modify: `crates/engine/src/config.rs` (`alpha_from_mesh`)
- Modify: `crates/engine/src/policy.rs` (`alpha_share`, two `suggest_alpha` call sites, `offered`, grass block in `check_profile`)
- Create: `crates/engine/tests/complex_grass.rs`

**Interfaces**
- Consumes: Task 1 ids.
- Produces:
  - `pub const CS_GRASS_THRESHOLD: f32 = 0.03`, `pub const GRASS_GUARD_MARGIN: f32 = 0.01`, `pub const GRASS_UNIT_TOL: f32 = 0.1`, `pub const GRASS_UNIT_SHARE: f64 = 0.99` (in `engine::grass`)
  - `pub struct GrassFacts { pub top_opaque: bool, pub top_near_binary: f64, pub bottom_unit_share: f64, pub corner_err: f32 }` (`Debug, Clone, Copy, PartialEq, Serialize, JsonSchema`)
  - `pub fn unit_err(rgb: [f32; 3]) -> f32` (`|length(rgb*2−1) − 1|` of stored values; non-finite → `f32::INFINITY`)
  - `pub fn facts(img: &Rgba32F) -> Option<GrassFacts>` (`None` unless `height == 2 × width`)
  - `Analysis::grass: Option<GrassFacts>` (last field)
  - `pub fn policy::offered(t: Target, s: Subtype) -> bool`

- [ ] **Step 1: Write the failing tests**

Create `crates/engine/src/grass.rs` with only its test module for now is not possible (the module must compile), so the unit tests go in Step 3 together with the code; write the integration tests first. Create `crates/engine/tests/complex_grass.rs`:

```rust
//! Complex grass sub-type (design `F:\ai\dds\nifparseref\plan\esp\2026-10-12-complex-grass-design.md`).
use engine::analyse::code8;
use engine::api::*;
use engine::codes::Code;
use engine::config::*;
use engine::events::*;
use engine::image::Rgba32F;
use engine::policy::{availability, offered, AcceptSuggestions, Availability, JobStatus, Need, ResolvedJob};
use image::{DynamicImage, ImageBuffer, Rgba};
use std::sync::Arc;
use tempfile::TempDir;

const TAG: &str = "file:**/g.png.subtype=complex_grass";
const CUTOUT: &str = "file:**/g.png.alpha.policy=cutout";
const GRASS_CODES: [Code; 6] = [
    Code::GrassShape,
    Code::GrassTopOpaque,
    Code::GrassNormals,
    Code::GrassDetectSource,
    Code::GrassVanilla,
    Code::ValGrassDetect,
];

/// A `w` × 2`w` complex-grass source: top = colour with a ~30 % cutout alpha (all texels 0 or 255), bottom = unit
/// normals tilted along x with specular 12 in alpha. Its bottom-left texel is a unit vector.
fn grass(w: u32) -> Rgba32F {
    let mut img = Rgba32F::new(w, 2 * w, [0.0; 4]);
    for y in 0..2 * w {
        for x in 0..w {
            *img.at_mut(x, y) = if y < w {
                let a = if (x * 7 + y * 13) % 10 < 3 { 1.0 } else { 0.0 };
                [0.25, 0.5, 0.15, a]
            } else {
                let t = (x as f32 / w as f32 - 0.5) * 0.8;
                [t.sin() * 0.5 + 0.5, 0.5, t.cos() * 0.5 + 0.5, 12.0 / 255.0]
            };
        }
    }
    img
}

fn png(img: &Rgba32F) -> DynamicImage {
    DynamicImage::ImageRgba8(ImageBuffer::from_fn(img.width, img.height, |x, y| {
        Rgba(img.at(x, y).map(code8))
    }))
}

/// Writes `img` as `in/textures/plants/<name>`.
fn put(d: &TempDir, name: &str, img: &Rgba32F) {
    let p = d.path().join("in/textures/plants").join(name);
    std::fs::create_dir_all(p.parent().unwrap()).unwrap();
    png(img).save(p).unwrap();
}

fn req(d: &TempDir, target: Target, sets: &[&str]) -> BatchRequest {
    let mut r = BatchRequest::new(vec![d.path().join("in")], d.path().join("out"), target);
    r.recursive = true;
    r.accept = AcceptSuggestions::High;
    r.on_existing = OnExisting::Overwrite;
    r.overrides = sets
        .iter()
        .map(|s| engine::overrides::parse_set(s).unwrap())
        .collect();
    r
}

fn plan_req(r: BatchRequest) -> Plan {
    let rep = Reporter::new(Arc::new(VecSink::default()), "t");
    Engine::new()
        .unwrap()
        .plan(r, &rep, &CancelToken::default())
        .unwrap()
}

fn plan(d: &TempDir, target: Target, sets: &[&str]) -> Plan {
    plan_req(req(d, target, sets))
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

fn grass_codes(j: &ResolvedJob) -> Vec<Code> {
    codes(j).into_iter().filter(|c| GRASS_CODES.contains(c)).collect()
}

/// A clean source tagged by hand with its alpha meaning given: Ready, no grass code.
#[test]
fn a_clean_tagged_source_is_ready_with_no_grass_code() {
    let d = tempfile::tempdir().unwrap();
    put(&d, "g.png", &grass(32));
    let p = plan(&d, Target::Cs, &[TAG, CUTOUT]);
    let j = job(&p, "g.png");
    assert_eq!(j.profile.subtype, Subtype::ComplexGrass);
    assert_eq!((j.profile.format, j.profile.srgb_tag()), (Format::Bc7, false));
    assert_eq!(j.status, JobStatus::Ready, "{:?}", j.warnings);
    assert!(grass_codes(j).is_empty(), "{:?}", j.warnings);
}

/// §4 first bullet (Review Focus 1): not exactly 1:2 or not a power of two is an error.
#[test]
fn shape_errors_are_grass_shape() {
    for (w, h) in [(32, 32), (64, 32), (48, 96)] {
        let d = tempfile::tempdir().unwrap();
        put(&d, "g.png", &Rgba32F::new(w, h, [0.5, 0.5, 1.0, 1.0]));
        let p = plan(&d, Target::Cs, &[TAG, CUTOUT]);
        let j = job(&p, "g.png");
        assert!(codes(j).contains(&Code::GrassShape), "{w}x{h}: {:?}", j.warnings);
        assert!(
            matches!(&j.status, JobStatus::Error { codes } if codes.contains(&Code::GrassShape)),
            "{w}x{h}: {:?}",
            j.status
        );
    }
}

/// §4 second bullet: the top half has no transparency.
#[test]
fn an_opaque_top_half_warns() {
    let d = tempfile::tempdir().unwrap();
    let mut img = grass(32);
    for px in &mut img.px[..32 * 32] {
        px[3] = 1.0;
    }
    put(&d, "g.png", &img);
    let p = plan(&d, Target::Cs, &[TAG, CUTOUT]);
    assert_eq!(grass_codes(job(&p, "g.png")), [Code::GrassTopOpaque]);
}

/// §4 third bullet (Review Focus 5): fewer than 99 % unit normals in the bottom half.
#[test]
fn bottom_normals_share_threshold() {
    for (bad, want) in [(5usize, vec![]), (21, vec![Code::GrassNormals])] {
        // 32 × 32 bottom half = 1024 texels: 5 is 0.5 %, 21 is 2.05 %. Zero vectors, never the corner texel.
        let d = tempfile::tempdir().unwrap();
        let mut img = grass(32);
        for i in 0..bad {
            img.px[32 * 32 + i] = [0.5, 0.5, 0.5, 12.0 / 255.0];
        }
        put(&d, "g.png", &img);
        let p = plan(&d, Target::Cs, &[TAG, CUTOUT]);
        assert_eq!(grass_codes(job(&p, "g.png")), want, "{bad} bad texels");
    }
}

/// §4 last bullet: the source's bottom-left texel already fails CS's 0.03 test.
#[test]
fn a_failing_source_corner_warns() {
    let d = tempfile::tempdir().unwrap();
    let mut img = grass(32);
    *img.at_mut(0, 63) = [0.5, 0.5, 0.75, 12.0 / 255.0]; // decodes to (0, 0, 0.5): |L-1| = 0.5
    put(&d, "g.png", &img);
    let p = plan(&d, Target::Cs, &[TAG, CUTOUT]);
    let j = job(&p, "g.png");
    assert_eq!(grass_codes(j), [Code::GrassDetectSource]);
    let w = j.warnings.iter().find(|w| w.code == Code::GrassDetectSource).unwrap();
    assert!(w.message.contains("renormalised"), "{}", w.message);
}

/// R1 (Review Focus 4): under vanilla the tag stays, with a warning, and the file is not blocked.
#[test]
fn vanilla_tag_warns_and_stays_complex_grass() {
    let d = tempfile::tempdir().unwrap();
    put(&d, "g.png", &grass(32));
    let p = plan(&d, Target::Vanilla, &[TAG, CUTOUT]);
    let j = job(&p, "g.png");
    assert_eq!(j.profile.subtype, Subtype::ComplexGrass);
    assert_eq!(grass_codes(j), [Code::GrassVanilla]);
    assert!(!codes(j).contains(&Code::CfgUnavailable));
    assert_eq!(j.status, JobStatus::Ready, "{:?}", j.warnings);
    assert_eq!(availability(Target::Vanilla, Subtype::ComplexGrass), Availability::Available);
    assert!(!offered(Target::Vanilla, Subtype::ComplexGrass));
    assert!(offered(Target::Cs, Subtype::ComplexGrass) && offered(Target::Cpm, Subtype::ComplexGrass));
    assert!(offered(Target::Vanilla, Subtype::Diffuse));
}

/// R4, R7: BC7 UNORM only, and no resize.
#[test]
fn another_format_an_srgb_tag_or_a_resize_is_cfg_invalid() {
    for extra in [
        "file:**/g.png.format=bc3",
        "file:**/g.png.color.tag=srgb",
        "file:**/g.png.resize=half",
    ] {
        let d = tempfile::tempdir().unwrap();
        put(&d, "g.png", &grass(512)); // 512 x 1024: `half` applies (larger side > 512)
        let p = plan(&d, Target::Cs, &[TAG, CUTOUT, extra]);
        let j = job(&p, "g.png");
        assert!(
            j.warnings
                .iter()
                .any(|w| w.code == Code::CfgInvalid && w.message.contains("complex_grass")),
            "{extra}: {:?}",
            j.warnings
        );
    }
}

/// R5: the alpha question is asked as for a diffuse, and its proposal reads the top half (the whole image is only
/// ~50 % near-binary because of the specular half, which would propose blend).
#[test]
fn the_alpha_proposal_reads_the_top_half() {
    let d = tempfile::tempdir().unwrap();
    put(&d, "g.png", &grass(32));
    let p = plan(&d, Target::Cs, &[TAG]);
    match &job(&p, "g.png").status {
        JobStatus::Unresolved { reasons, needs } => {
            assert!(needs.contains(&Need::AlphaMeaning), "{needs:?}");
            assert!(reasons.iter().any(|r| r.contains("Cutout")), "{reasons:?}");
        }
        s => panic!("expected the alpha question, got {s:?}"),
    }
}

/// Review Focus 3: a tall texture that is not tagged complex grass gets no grass code.
#[test]
fn a_tall_diffuse_raises_no_grass_code() {
    let d = tempfile::tempdir().unwrap();
    put(&d, "tall.png", &grass(32));
    let p = plan(&d, Target::Cs, &["file:**/tall.png.alpha.policy=cutout"]);
    let j = job(&p, "tall.png");
    assert_ne!(j.profile.subtype, Subtype::ComplexGrass);
    assert!(grass_codes(j).is_empty(), "{:?}", j.warnings);
}
```

- [ ] **Step 2: Run them to see them fail**

Run: `cargo test -p engine --test complex_grass`
Expected: compile error `unresolved import engine::policy::offered`.

- [ ] **Step 3: Implement**

Create `crates/engine/src/grass.rs`:

```rust
//! Complex grass (design `2026-10-12-complex-grass-design.md`): one image, height = 2 × width. The top half is a
//! diffuse with alpha transparency, the bottom half a normal map with specular in alpha. Community Shaders'
//! `RunGrass.hlsl` reads both halves from one view and treats a texture as complex when the decoded bottom-left texel
//! of mip 0 is a unit vector (`|length(rgb * 2 - 1) - 1| < ComplexGrassThreshold`); ENB is assumed to do the same
//! (owner ruling 2026-10-12).
use crate::analyse::code8;
use crate::image::Rgba32F;
use schemars::JsonSchema;
use serde::Serialize;

/// The owner's `ComplexGrassThreshold` (CS `SettingsUser.json`, "Grass Lighting"), design §2.
pub const CS_GRASS_THRESHOLD: f32 = 0.03;
/// What the encoded detection texel keeps below the threshold, so the file passes any threshold >= 0.02 (§3.6).
pub const GRASS_GUARD_MARGIN: f32 = 0.01;
/// `|L - 1|` below this is a unit-length normal (design §4, §5).
pub const GRASS_UNIT_TOL: f32 = 0.1;
/// Share of bottom-half texels that must be unit-length normals (design §4, §5).
pub const GRASS_UNIT_SHARE: f64 = 0.99;

/// What `analyse` records of an image whose height is twice its width (design §4, §5).
#[derive(Debug, Clone, Copy, PartialEq, Serialize, JsonSchema)]
pub struct GrassFacts {
    /// Every top-half alpha code is 251 or more (DECISIONS Note L: 251..=254 is BC encode noise).
    pub top_opaque: bool,
    /// Share of top-half texels whose alpha code is <= 5 or >= 250: the diffuse cutout/blend proposal reads it.
    pub top_near_binary: f64,
    /// Share of bottom-half texels that decode to `|L - 1| < 0.1`.
    pub bottom_unit_share: f64,
    /// `|L - 1|` of the bottom-left texel `(0, h - 1)`, the one CS tests.
    pub corner_err: f32,
}

/// `|length(rgb * 2 - 1) - 1|` of stored values in [0, 1]; a non-finite input is infinitely far from unit length.
pub fn unit_err(rgb: [f32; 3]) -> f32 {
    let v = rgb.map(|c| c * 2.0 - 1.0);
    let e = ((v[0] * v[0] + v[1] * v[1] + v[2] * v[2]).sqrt() - 1.0).abs();
    if e.is_finite() {
        e
    } else {
        f32::INFINITY
    }
}

/// The facts of `img` when its height is twice its width; `None` for every other shape.
pub fn facts(img: &Rgba32F) -> Option<GrassFacts> {
    let (w, h) = (img.width as usize, img.height as usize);
    if w == 0 || h != 2 * w {
        return None;
    }
    let (top, bottom) = img.px.split_at(w * w);
    let n = (w * w) as f64;
    let share = |half: &[[f32; 4]], keep: &dyn Fn(&[f32; 4]) -> bool| {
        half.iter().filter(|p| keep(p)).count() as f64 / n
    };
    let c = img.px[(h - 1) * w];
    Some(GrassFacts {
        top_opaque: top.iter().all(|p| code8(p[3]) >= 251),
        top_near_binary: share(top, &|p| {
            let a = code8(p[3]);
            a <= 5 || a >= 250
        }),
        bottom_unit_share: share(bottom, &|p| unit_err([p[0], p[1], p[2]]) < GRASS_UNIT_TOL),
        corner_err: unit_err([c[0], c[1], c[2]]),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn facts_only_for_one_by_two() {
        assert!(facts(&Rgba32F::new(4, 4, [0.5; 4])).is_none());
        assert!(facts(&Rgba32F::new(8, 4, [0.5; 4])).is_none());
        let mut img = Rgba32F::new(4, 8, [0.5, 0.5, 1.0, 12.0 / 255.0]);
        for px in &mut img.px[..16] {
            *px = [0.3, 0.6, 0.2, 1.0];
        }
        img.px[1][3] = 0.0;
        img.px[2][3] = 0.5;
        let f = facts(&img).unwrap();
        assert!(!f.top_opaque);
        assert!((f.top_near_binary - 15.0 / 16.0).abs() < 1e-12);
        assert_eq!(f.bottom_unit_share, 1.0);
        assert!(f.corner_err < 0.01, "{}", f.corner_err);
    }

    /// The owner's example corner (127, 126, 255) has length 1.0001 (design §2).
    #[test]
    fn owner_corner_is_unit() {
        let e = unit_err([127.0 / 255.0, 126.0 / 255.0, 1.0]);
        assert!(e < 0.005, "{e}");
        assert_eq!(unit_err([0.5, 0.5, f32::NAN]), f32::INFINITY);
    }
}
```

`crates/engine/src/lib.rs`: after `pub mod filter;` add `pub mod grass;`.

`crates/engine/src/analyse.rs`: in `pub struct Analysis`, after the `nonfinite` field add

```rust
    /// Complex grass facts (design §4, §5), for an image whose height is twice its width; `None` otherwise.
    pub grass: Option<crate::grass::GrassFacts>,
```

and in `analyse()`'s returned literal, after `        nonfinite,` add `        grass: crate::grass::facts(img),`.

`crates/engine/src/config.rs`, `alpha_from_mesh`: replace

```rust
            Subtype::Diffuse | Subtype::PbrAlbedo | Subtype::UiSky | Subtype::SkinAlbedo
```

with

```rust
            Subtype::Diffuse
                | Subtype::PbrAlbedo
                | Subtype::UiSky
                | Subtype::SkinAlbedo
                | Subtype::ComplexGrass
```

`crates/engine/src/policy.rs`:

(a) After the `pub fn near_binary(a: &Analysis) -> f64 { … }` function add

```rust
/// The near-binary share the alpha proposal reads (design §3.2, ruling R5): complex grass looks at its top (diffuse)
/// half only, because the bottom half's alpha is specular data.
fn alpha_share(st: Subtype, a: &Analysis) -> f64 {
    match (st, &a.grass) {
        (Subtype::ComplexGrass, Some(g)) => g.top_near_binary,
        _ => near_binary(a),
    }
}
```

and replace the two calls: `suggest_alpha(st, f.analysis.alpha, near_binary(f.analysis))` → `suggest_alpha(st, f.analysis.alpha, alpha_share(st, f.analysis))`, and `suggest_alpha(profile.subtype, f.analysis.alpha, near_binary(f.analysis))` → `suggest_alpha(profile.subtype, f.analysis.alpha, alpha_share(profile.subtype, f.analysis))`.

(b) After `pub fn availability(…) { … }` add

```rust
/// Whether the sub-type pickers offer `s` under `t`: available, and not `complex_grass` under vanilla, whose grass
/// shader has no complex grass (design §3.7, ruling R1). A file tagged `complex_grass` there still encodes, with
/// `GRASS-VANILLA`.
pub fn offered(t: Target, s: Subtype) -> bool {
    availability(t, s) == Availability::Available
        && !(t == Target::Vanilla && s == Subtype::ComplexGrass)
}
```

(c) In `check_profile`, insert before `    if a.icc_profile {`:

```rust
    // Complex grass (design §3.5, §3.7, §4; rulings R1, R4, R7, R9).
    if st == Subtype::ComplexGrass {
        if p.format != Format::Bc7
            || p.srgb_tag()
            || crate::resize::target_dims((a.width, a.height), p.resize) != (a.width, a.height)
        {
            add(
                CfgInvalid,
                "complex_grass is written as BC7 UNORM at its own size (resize would have to run on each half)"
                    .into(),
            );
        }
        if p.target == Target::Vanilla {
            add(
                GrassVanilla,
                "the vanilla grass shader has no complex grass: written as complex_grass unchanged; only Community \
                 Shaders or ENB read it"
                    .into(),
            );
        }
        match &a.grass {
            Some(g) if a.is_pot => {
                if g.top_opaque {
                    add(
                        GrassTopOpaque,
                        "top half (diffuse) alpha is fully opaque: the grass has no transparency".into(),
                    );
                }
                if g.bottom_unit_share < crate::grass::GRASS_UNIT_SHARE {
                    add(
                        GrassNormals,
                        format!(
                            "{:.1}% of bottom-half texels are unit-length normals (|L-1| < 0.1); 99% expected",
                            g.bottom_unit_share * 100.0
                        ),
                    );
                }
                if g.corner_err >= crate::grass::CS_GRASS_THRESHOLD {
                    add(
                        GrassDetectSource,
                        format!(
                            "source bottom-left texel has |L-1| = {:.3} >= 0.03: Community Shaders would not detect \
                             it as complex grass; mip 0 is renormalised, so the written file passes unless \
                             mips.normal_base = keep",
                            g.corner_err
                        ),
                    );
                }
            }
            _ => add(
                GrassShape,
                format!(
                    "{}x{}: complex_grass needs a power-of-two width and height = 2 x width",
                    a.width, a.height
                ),
            ),
        }
    }
```

(`corner_err` is never NaN: `unit_err` maps non-finite to infinity.)

- [ ] **Step 4: Run the tests**

Run: `cargo test -p engine --test complex_grass && cargo test -p engine --lib grass::tests`
Expected: PASS (10 + 2 tests).

- [ ] **Step 5: Commit**

```bash
cargo fmt --all && cargo clippy --workspace --all-targets -- -D warnings && cargo test --workspace
git add crates/engine/src/grass.rs crates/engine/src/lib.rs crates/engine/src/analyse.rs crates/engine/src/config.rs crates/engine/src/policy.rs crates/engine/tests/complex_grass.rs
git commit -m "engine: complex grass facts in analyse, GRASS-* checks, top-half alpha proposal, offered() (design §4, R1, R5)

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

---

### Task 3: per-half mip build in the pipeline

**Files**
- Modify: `crates/engine/src/grass.rs` (`build`, `split`, `stack`, `half_profile`, tests)
- Modify: `crates/engine/src/pipeline.rs` (`prepare_then`)
- Test: `crates/engine/tests/complex_grass.rs` (append)

**Interfaces**
- Consumes: Task 2 `grass.rs`.
- Produces:
  - `pub(crate) fn grass::build(img: Rgba32F, p: &Profile, transfer: SourceTransfer) -> Result<(Work, Vec<Rgba32F>), String>`: the stacked chain in stored form (codes / 255) and a `Work` with `domain = Stored`, `alpha_present = p.alpha.policy != Remove` and an empty image (only `domain` and `alpha_present` are read after the mips). Top-half coverage is already preserved; the caller must not run `preserve_coverage` on it.

- [ ] **Step 1: Write the failing tests**

Append to `crates/engine/src/grass.rs`'s `mod tests` (and add only `use crate::presets::PresetRegistry;` at the top of `mod tests`: `use super::*` already brings in `crate::config::*`, `SourceTransfer`, `preprocess_with`, `build_chain`, `preserve_coverage`, `to_encoder_input` and `EncoderInput` once Step 3 adds them to `grass.rs`; a second import would trip `unused_imports` under `-D warnings`):

```rust
    fn profile() -> Profile {
        let mut p = PresetRegistry::builtin()
            .unwrap()
            .profile("cs-quality", Target::Cs, Subtype::ComplexGrass)
            .unwrap();
        p.alpha.policy = AlphaPolicy::Cutout;
        p
    }

    /// Top = colour with ~30 % cutout dots, bottom = unit normals tilted along x with specular 12.
    fn source(w: u32) -> Rgba32F {
        let mut img = Rgba32F::new(w, 2 * w, [0.0; 4]);
        for y in 0..2 * w {
            for x in 0..w {
                *img.at_mut(x, y) = if y < w {
                    let a = if (x * 7 + y * 13) % 10 < 3 { 1.0 } else { 0.0 };
                    [0.25, 0.5, 0.15, a]
                } else {
                    let t = (x as f32 / w as f32 - 0.5) * 0.8;
                    [t.sin() * 0.5 + 0.5, 0.5, t.cos() * 0.5 + 0.5, 12.0 / 255.0]
                };
            }
        }
        img
    }

    fn codes(l: &Rgba32F) -> Vec<[u8; 4]> {
        l.px.iter().map(|q| q.map(code8)).collect()
    }

    /// R6: one level per half level, ending at 1 x 2.
    #[test]
    fn the_chain_stops_at_one_by_two() {
        let (_, chain) = build(source(8), &profile(), SourceTransfer::Stored).unwrap();
        let dims: Vec<_> = chain.iter().map(|l| (l.width, l.height)).collect();
        assert_eq!(dims, [(8, 16), (4, 8), (2, 4), (1, 2)]);
    }

    /// §3.4: constant halves stay exactly constant at every level; a filter tap across the seam would mix them.
    #[test]
    fn no_texel_crosses_the_seam() {
        let w = 16;
        let mut src = Rgba32F::new(w, 2 * w, [0.5, 0.5, 1.0, 12.0 / 255.0]);
        for px in &mut src.px[..(w * w) as usize] {
            *px = [1.0, 1.0, 1.0, 1.0];
        }
        let (_, chain) = build(src, &profile(), SourceTransfer::Stored).unwrap();
        for (l, level) in chain.iter().enumerate() {
            let n = (level.width * level.width) as usize;
            let c = codes(level);
            assert!(c[..n].iter().all(|q| *q == [255, 255, 255, 255]), "top, level {l}: {:?}", &c[..n]);
            assert!(c[n..].iter().all(|q| *q == [128, 128, 255, 12]), "bottom, level {l}: {:?}", &c[n..]);
        }
    }

    /// §3.3: the bottom half is renormalised per level and its alpha is data (box-averaged, never coverage-adjusted).
    #[test]
    fn bottom_normals_stay_unit_and_alpha_is_data() {
        let (_, chain) = build(source(16), &profile(), SourceTransfer::Stored).unwrap();
        for (l, level) in chain.iter().enumerate() {
            let n = (level.width * level.width) as usize;
            for q in &level.px[n..] {
                assert!(unit_err([q[0], q[1], q[2]]) < 0.01, "level {l}: {q:?}");
                assert_eq!(code8(q[3]), 12, "level {l}");
            }
        }
    }

    /// Each half is exactly what the existing path gives that half on its own: diffuse (with coverage preservation,
    /// §3.2) on top, normal map below. This is also the coverage test: a direct coverage-tolerance assertion on a
    /// binary dot source cannot pass, because `preserve_coverage` can only reach the cumulative counts of a level's
    /// few distinct alpha values.
    #[test]
    fn each_half_is_the_existing_path() {
        let p = profile();
        let src = source(16);
        let (_, chain) = build(src.clone(), &p, SourceTransfer::Stored).unwrap();
        let (top, bottom) = split(src);
        let (tp, bp) = (half_profile(&p, false), half_profile(&p, true));
        let tw = preprocess_with(top, &tp, SourceTransfer::Stored);
        let mut tc = build_chain(&tw, &tp);
        preserve_coverage(&mut tc, tp.alpha.threshold, tp.alpha.compare);
        let bw = preprocess_with(bottom, &bp, SourceTransfer::Stored);
        let bc = build_chain(&bw, &bp);
        let ldr = |e: EncoderInput| match e {
            EncoderInput::Ldr(v) => v,
            EncoderInput::Hdr(_) => panic!("LDR expected"),
        };
        for (l, level) in chain.iter().enumerate() {
            let n = (level.width * level.width) as usize;
            let got = codes(level);
            assert_eq!(got[..n], ldr(to_encoder_input(&tc[l], &tw, &tp))[..], "top, level {l}");
            assert_eq!(got[n..], ldr(to_encoder_input(&bc[l], &bw, &bp))[..], "bottom, level {l}");
        }
    }

    /// Review Focus 1: a job with the wrong shape (e.g. previewed while it is `GRASS-SHAPE`) is an error, not a panic.
    #[test]
    fn a_wrong_shape_is_an_error_not_a_panic() {
        for (w, h) in [(16, 16), (32, 16), (12, 24)] {
            let e = build(Rgba32F::new(w, h, [0.5; 4]), &profile(), SourceTransfer::Stored).unwrap_err();
            assert!(e.contains("complex_grass"), "{e}");
        }
    }

    /// R5: `remove` makes both halves opaque.
    #[test]
    fn remove_makes_both_halves_opaque() {
        let mut p = profile();
        p.alpha.policy = AlphaPolicy::Remove;
        let (work, chain) = build(source(8), &p, SourceTransfer::Stored).unwrap();
        assert!(!work.alpha_present);
        assert!(chain[0].px.iter().all(|q| q[3] == 1.0));
    }
```

Append to `crates/engine/tests/complex_grass.rs`:

```rust
fn run(d: &TempDir, target: Target, sets: &[&str]) -> (Vec<EngineEvent>, BatchSummary) {
    let sink = Arc::new(VecSink::default());
    let rep = Reporter::new(sink.clone(), "t");
    let e = Engine::new().unwrap();
    let p = e
        .plan(req(d, target, sets), &rep, &CancelToken::default())
        .unwrap();
    let s = e.run(p, &rep, &CancelToken::default()).unwrap();
    (sink.take(), s)
}

fn out(d: &TempDir) -> engine::dds::DdsFile {
    let b = std::fs::read(d.path().join("out/textures/plants/g.dds")).unwrap();
    engine::dds::read_dds(&b).unwrap()
}

/// §3.5, R6: one BC7 UNORM file, 64 x 128 down to 1 x 2 (7 levels).
#[test]
fn a_tagged_grass_texture_is_one_bc7_unorm_file_down_to_one_by_two() {
    let d = tempfile::tempdir().unwrap();
    put(&d, "g.png", &grass(64));
    let (_, s) = run(&d, Target::Cs, &[TAG, CUTOUT]);
    assert_eq!(s.converted, 1);
    let f = out(&d);
    assert_eq!(f.format.as_format(), Some((Format::Bc7, false)));
    assert_eq!((f.width, f.height, f.mip_count), (64, 128, 7));
}
```

- [ ] **Step 2: Run them to see them fail**

Run: `cargo test -p engine --lib grass::tests`
Expected: compile error `cannot find function build in this scope`.

- [ ] **Step 3: Implement**

In `crates/engine/src/grass.rs`, extend the imports:

```rust
use crate::analyse::code8;
use crate::config::*;
use crate::coverage::preserve_coverage;
use crate::image::Rgba32F;
use crate::mips::build_chain;
use crate::ops::{preprocess_with, Domain, SourceTransfer, Work};
use crate::quantize::{to_encoder_input, EncoderInput};
use schemars::JsonSchema;
use serde::Serialize;
```

and add after `facts`:

```rust
/// Preprocess + Mipmaps of a complex-grass job (design §3.1-3.4): each half is built on its own by the existing
/// path (top: diffuse with coverage preservation; bottom: normal map, renormalised per level, alpha as data), so no
/// filter crosses the seam, and the halves are stacked per level as stored values (codes / 255). The chain ends at
/// 1 x 2 (ruling R6). The returned `Work` quantises the stacked levels as they are (`Stored`).
pub(crate) fn build(
    img: Rgba32F,
    p: &Profile,
    transfer: SourceTransfer,
) -> Result<(Work, Vec<Rgba32F>), String> {
    let (w, h) = (img.width, img.height);
    if !(w.is_power_of_two() && h == 2 * w) {
        return Err(format!(
            "{w}x{h}: complex_grass needs a power-of-two width and height = 2 x width (GRASS-SHAPE)"
        ));
    }
    let (top, bottom) = split(img);
    let (tp, bp) = (half_profile(p, false), half_profile(p, true));
    let tw = preprocess_with(top, &tp, transfer);
    let bw = preprocess_with(bottom, &bp, transfer);
    let mut tc = build_chain(&tw, &tp);
    if tw.alpha_present && tp.coverage_enabled() {
        preserve_coverage(&mut tc, tp.alpha.threshold, tp.alpha.compare);
    }
    let bc = build_chain(&bw, &bp);
    let chain = tc
        .iter()
        .zip(&bc)
        .map(|(t, b)| {
            stack(
                &to_encoder_input(t, &tw, &tp),
                &to_encoder_input(b, &bw, &bp),
                t.width,
            )
        })
        .collect::<Result<Vec<_>, _>>()?;
    let work = Work {
        img: Rgba32F::new(0, 0, [0.0; 4]),
        alpha_present: p.alpha.policy != AlphaPolicy::Remove,
        domain: Domain::Stored,
        roughness_r_squared: false,
        nonfinite_samples: tw.nonfinite_samples + bw.nonfinite_samples,
    };
    Ok((work, chain))
}

/// The top and bottom halves of a `w` x `2w` image.
fn split(img: Rgba32F) -> (Rgba32F, Rgba32F) {
    let (w, h) = (img.width, img.height / 2);
    let mut px = img.px;
    let bottom = px.split_off((w * h) as usize);
    (
        Rgba32F { width: w, height: h, px },
        Rgba32F { width: w, height: h, px: bottom },
    )
}

/// One stacked level, top rows first, as stored floats (8-bit code / 255, so `to_encoder_input` gives the codes back).
fn stack(top: &EncoderInput, bottom: &EncoderInput, w: u32) -> Result<Rgba32F, String> {
    let (EncoderInput::Ldr(t), EncoderInput::Ldr(b)) = (top, bottom) else {
        return Err("complex_grass is written as BC7: an HDR format cannot hold it".into());
    };
    let px: Vec<[f32; 4]> = t
        .iter()
        .chain(b)
        .map(|c| c.map(|v| f32::from(v) / 255.0))
        .collect();
    Ok(Rgba32F {
        width: w,
        height: (px.len() / w as usize) as u32,
        px,
    })
}

/// The profile each half is built with (design §3.2-3.3, ruling R5): the top half is a diffuse (sRGB colour, the job's
/// alpha policy, coverage and filter); the bottom half a normal map whose alpha is data, unless the job removes
/// alpha. Never resized (ruling R7).
fn half_profile(p: &Profile, bottom: bool) -> Profile {
    let mut h = p.clone();
    h.resize = ResizeMode::None;
    if bottom {
        h.subtype = Subtype::Normal;
        h.category = Category::Normal;
        h.color.content = ColorContent::Linear;
        if h.alpha.policy != AlphaPolicy::Remove {
            h.alpha.policy = AlphaPolicy::Data;
        }
    } else {
        h.subtype = Subtype::Diffuse;
        h.category = Category::Color;
        h.color.content = ColorContent::Srgb;
    }
    h
}
```

In `crates/engine/src/pipeline.rs`, change `use crate::config::{Format, Profile};` to `use crate::config::{Format, Profile, Subtype};` and replace

```rust
    let work = preprocess_with(img, p, transfer);
    stop()?;

    enter(Stage::Mipmaps);
    let mut chain = build_chain_with(&work, p, bake.as_ref());
    drop(bake);
    let alpha_present = work.alpha_present;
    // Spec §5.5 (Task 30): coverage runs after the mips and before quantise; for `cutout_blend` it rewrites the
    // only alpha channel.
    let base_coverage = base_coverage(&chain[0], p, alpha_present);
    if base_coverage.is_some() {
        preserve_coverage(&mut chain, p.alpha.threshold, p.alpha.compare);
    }
```

with

```rust
    // Complex grass builds each half's chain on its own (design §3.4); the stacked chain comes with its Work.
    let grass = p.subtype == Subtype::ComplexGrass;
    let (work, grass_chain) = if grass {
        let (w, c) = crate::grass::build(img, p, transfer).map_err(fail(Stage::Preprocess))?;
        (w, Some(c))
    } else {
        (preprocess_with(img, p, transfer), None)
    };
    stop()?;

    enter(Stage::Mipmaps);
    let mut chain = match grass_chain {
        Some(c) => c,
        None => build_chain_with(&work, p, bake.as_ref()),
    };
    drop(bake);
    let alpha_present = work.alpha_present;
    // Spec §5.5 (Task 30): coverage runs after the mips and before quantise; for `cutout_blend` it rewrites the
    // only alpha channel. Complex grass preserved its top half already; its bottom half's alpha is specular data.
    let base_coverage = base_coverage(&chain[0], p, alpha_present);
    if base_coverage.is_some() && !grass {
        preserve_coverage(&mut chain, p.alpha.threshold, p.alpha.compare);
    }
```

- [ ] **Step 4: Run the tests**

Run: `cargo test -p engine --lib grass::tests && cargo test -p engine --test complex_grass`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
cargo fmt --all && cargo clippy --workspace --all-targets -- -D warnings && cargo test --workspace
git add crates/engine/src/grass.rs crates/engine/src/pipeline.rs crates/engine/tests/complex_grass.rs
git commit -m "engine: complex grass per-half mip chains stacked per level, coverage on top, renormalised bottom (design §3.1-3.4)

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

---

### Task 4: detection-texel guard, `VAL-GRASS-DETECT`, owner example

**Files**
- Modify: `crates/engine/src/encode.rs` (`guard_grass_corner`, `grass_corner_ok`, the call in `encode_level_with`, one test)
- Modify: `crates/engine/src/validate/mod.rs` (the `VAL-GRASS-DETECT` check)
- Test: `crates/engine/tests/complex_grass.rs` (append)

**Interfaces**
- Consumes: Task 2 `grass::{unit_err, CS_GRASS_THRESHOLD, GRASS_GUARD_MARGIN}`.
- Produces: for `subtype == ComplexGrass` and `format == Bc7`, every encoded level's bottom-left block decodes at `|L−1| < 0.02` or was re-encoded once at `Effort::Slowest`; `validate` emits `Code::ValGrassDetect` (Error) when the first decoded level's texel `(0, h−1)` still fails.

- [ ] **Step 1: Write the failing tests**

In `crates/engine/src/encode.rs`'s `mod tests` add:

```rust
    /// Design §3.6: a corner block that decodes off unit length is encoded again from its own texels.
    #[test]
    fn grass_guard_reencodes_a_failing_corner_block() {
        let p = PresetRegistry::builtin()
            .unwrap()
            .profile("cs-quality", Target::Cs, Subtype::ComplexGrass)
            .unwrap();
        // 4 x 8: top block = opaque colour, bottom block = flat +Z normal with specular 12.
        let px: Vec<[u8; 4]> = (0..32)
            .map(|i| if i < 16 { [200, 180, 90, 255] } else { [128, 128, 255, 12] })
            .collect();
        let blocks = gather_blocks(&px, 4, 8);
        let mut out = vec![0u8; 32];
        encode_band(&blocks, &mut out, &p, BC7E_EXACT_OPAQUE_NATIVE);
        let top = out[..16].to_vec();
        assert!(grass_corner_ok(&out[16..], 12), "a flat normal encodes within the guard");
        // A red block in the corner's place decodes to (1, -1, -1): far from unit length.
        let red = gather_blocks(&[[255, 0, 0, 12]; 16], 4, 4);
        encode_band(&red, &mut out[16..], &p, BC7E_EXACT_OPAQUE_NATIVE);
        assert!(!grass_corner_ok(&out[16..], 12));
        guard_grass_corner(&mut out, &blocks, 4, 8, &p);
        assert!(grass_corner_ok(&out[16..], 12));
        assert_eq!(out[..16], top[..], "the top block is left alone");
    }
```

Append to `crates/engine/tests/complex_grass.rs`:

```rust
fn decoded_corner(f: &engine::dds::DdsFile) -> [u8; 4] {
    use engine::decode::{decode_level, Decoded};
    let (w, h, r) = f.levels[0].clone();
    match decode_level(f.format, w, h, &f.data[r], None).unwrap() {
        Decoded::Ldr(px) => px[((h - 1) * w) as usize],
        Decoded::Hdr(_) => panic!("LDR expected"),
    }
}

fn corner_err(t: [u8; 4]) -> f32 {
    engine::grass::unit_err([t[0], t[1], t[2]].map(|c| f32::from(c) / 255.0))
}

fn file_done(events: &[EngineEvent]) -> engine::validate::ValidationSummary {
    events
        .iter()
        .find_map(|e| match &e.kind {
            EventKind::FileDone { validation, .. } => Some(validation.clone()),
            _ => None,
        })
        .expect("file_done")
}

/// §3.6: the written file's decoded bottom-left texel is within the guard.
#[test]
fn the_written_corner_passes_the_guard() {
    let d = tempfile::tempdir().unwrap();
    put(&d, "g.png", &grass(64));
    let (events, _) = run(&d, Target::Cs, &[TAG, CUTOUT]);
    assert!(file_done(&events).passed);
    let e = corner_err(decoded_corner(&out(&d)));
    assert!(e < 0.02, "{e}");
}

/// §3.6: a corner no BC7 encode can bring to unit length is the named error.
#[test]
fn a_corner_no_encode_can_fix_is_val_grass_detect() {
    use engine::dds::{dds_bytes, resolve_header, DdsDesc};
    use engine::encode::{encode_level, EncodeCtx};
    use engine::quantize::EncoderInput;
    use engine::validate::{validate, ValidationInput};
    let mut p = engine::presets::PresetRegistry::builtin()
        .unwrap()
        .profile("cs-quality", Target::Cs, Subtype::ComplexGrass)
        .unwrap();
    p.alpha.policy = AlphaPolicy::Cutout;
    for (grey, want) in [(false, false), (true, true)] {
        let mut px = vec![[200u8, 180, 90, 255]; 16];
        px.extend(vec![[128u8, 128, 255, 12]; 16]);
        if grey {
            px[28] = [128, 128, 128, 12]; // texel (0, 7): length ~0
        }
        let input = EncoderInput::Ldr(px);
        let level = encode_level(&input, 4, 8, &p, &EncodeCtx::default()).unwrap();
        let desc = DdsDesc {
            width: 4,
            height: 8,
            format: Format::Bc7,
            srgb: false,
            header: resolve_header(Format::Bc7, false, p.header),
        };
        let bytes = dds_bytes(&desc, &[level]).unwrap();
        let r = validate(&ValidationInput {
            bytes: &bytes,
            profile: &p,
            encoder_input: Some(std::slice::from_ref(&input)),
            base_coverage: None,
            source_peak: None,
            bc1_model: encoders_sys::Bc1Model::Ideal,
            expected_levels: Some(1),
        });
        let named = r.warnings.iter().any(|w| w.code == Code::ValGrassDetect);
        assert_eq!(named, want, "grey {grey}: {:?}", r.warnings);
        assert_eq!(r.summary.passed, !want);
    }
}

/// Review Focus 2: a source without an alpha channel resolves `remove`, encodes opaque in both halves and passes.
#[test]
fn an_rgb_grass_source_encodes_opaque_and_passes() {
    let d = tempfile::tempdir().unwrap();
    let rgb = image::DynamicImage::ImageRgb8(png(&grass(32)).to_rgb8());
    let p = d.path().join("in/textures/plants/g.png");
    std::fs::create_dir_all(p.parent().unwrap()).unwrap();
    rgb.save(&p).unwrap();
    let plan = plan(&d, Target::Cs, &[TAG]);
    let j = job(&plan, "g.png");
    assert_eq!(j.profile.alpha.policy, AlphaPolicy::Remove);
    assert_eq!(grass_codes(j), [Code::GrassTopOpaque]);
    let (events, s) = run(&d, Target::Cs, &[TAG]);
    assert_eq!(s.converted, 1);
    let v = file_done(&events);
    assert!(v.passed, "{v:?}");
    // `exact_opaque` is off for this sub-type, so the `remove` guarantee is 254 (mode-6 p-bit), as for diffuse.
    assert!(decoded_corner(&out(&d))[3] >= 254);
}

/// Design §6: the owner's example (2048 x 4096, bottom-left texel (127, 126, 255, 10)) encodes, the guard passes and the
/// decoded bottom-left texel passes CS's 0.03 test. Owner data, read only (copied into a temp dir):
/// `$env:SDC_ACHERUS='C:\_mount\moddingvol\Acherus'; cargo test -p engine --test complex_grass -- --ignored`
#[test]
#[ignore]
fn owner_example_encodes_and_stays_complex() {
    std::env::var("SDC_ACHERUS").expect("owner-data test: set SDC_ACHERUS (it marks the owner's machine)");
    let src = std::path::Path::new(r"F:\ai\dds\nifparseref\grassfieldtundra.png");
    assert!(src.is_file(), "{} is missing", src.display());
    let d = tempfile::tempdir().unwrap();
    let dst = d.path().join("in/textures/plants/g.png");
    std::fs::create_dir_all(dst.parent().unwrap()).unwrap();
    std::fs::copy(src, &dst).unwrap();
    let p = plan(&d, Target::Cs, &[TAG, CUTOUT]);
    let j = job(&p, "g.png");
    assert!(grass_codes(j).is_empty(), "{:?}", j.warnings);
    let (img, info) = engine::load::load_image(&dst).unwrap();
    let g = engine::analyse::analyse(&img, &info).grass.unwrap();
    drop(img);
    assert!(g.corner_err < 0.001 && g.bottom_unit_share == 1.0, "{g:?}");
    let (events, s) = run(&d, Target::Cs, &[TAG, CUTOUT]);
    assert_eq!(s.converted, 1);
    assert!(file_done(&events).passed);
    let f = out(&d);
    assert_eq!(f.format.as_format(), Some((Format::Bc7, false)));
    assert_eq!((f.width, f.height, f.mip_count), (2048, 4096, 12));
    let e = corner_err(decoded_corner(&f));
    assert!(e < 0.03, "decoded corner |L-1| = {e}");
    eprintln!("owner example: decoded corner {:?}, |L-1| = {e:.5}", decoded_corner(&f));
}
```

(`engine::load::load_image(path) -> Result<(Rgba32F, SourceInfo), _>` is the loader `pipeline.rs` uses.)

- [ ] **Step 2: Run them to see them fail**

Run: `cargo test -p engine --lib encode::tests::grass_guard && cargo test -p engine --test complex_grass`
Expected: compile error `cannot find function grass_corner_ok`; once it compiles, `a_corner_no_encode_can_fix_is_val_grass_detect` fails (no `VAL-GRASS-DETECT`).

- [ ] **Step 3: Implement**

`crates/engine/src/encode.rs`: add `use crate::grass::{unit_err, CS_GRASS_THRESHOLD, GRASS_GUARD_MARGIN};` after `use crate::quantize::EncoderInput;`. In `encode_level_with`, replace

```rust
                        encode_band(b, o, p, BC7E_EXACT_OPAQUE_NATIVE);
                    }
                });
            out
```

with

```rust
                        encode_band(b, o, p, BC7E_EXACT_OPAQUE_NATIVE);
                    }
                });
            if p.subtype == Subtype::ComplexGrass
                && p.format == Format::Bc7
                && !skipped.load(Ordering::Relaxed)
            {
                guard_grass_corner(&mut out, &blocks, w, h, p);
            }
            out
```

and add before `#[cfg(test)]`:

```rust
/// Design §3.6 (ruling R8): the decoded bottom-left texel of a complex-grass level must stay a unit vector within
/// `CS_GRASS_THRESHOLD - GRASS_GUARD_MARGIN`; when it does not, that one block is encoded again at bc7e's slowest
/// level (with the same opaque fix-ups). `validate` reports `VAL-GRASS-DETECT` when even that fails.
fn guard_grass_corner(out: &mut [u8], blocks: &[[u8; 4]], w: u32, h: u32, p: &Profile) {
    let i = (h.div_ceil(4) as usize - 1) * w.div_ceil(4) as usize; // column 0 of the last block row
    let texel = ((h - 1) % 4) as usize * 4; // x = 0 inside that block
    let blk = &mut out[i * BC7_BLOCK_BYTES..(i + 1) * BC7_BLOCK_BYTES];
    if grass_corner_ok(blk, texel) {
        return;
    }
    let mut best = p.clone();
    best.encoder.effort = Effort::Slowest;
    encode_band(
        &blocks[i * BLOCK_TEXELS..(i + 1) * BLOCK_TEXELS],
        blk,
        &best,
        BC7E_EXACT_OPAQUE_NATIVE,
    );
}

/// Whether texel `t` (0..16, row-major) of BC7 block `blk` decodes within the complex-grass guard.
fn grass_corner_ok(blk: &[u8], t: usize) -> bool {
    let mut d = [0u8; 64];
    bcdec_rs::bc7(blk, &mut d, 16);
    let q = [d[t * 4], d[t * 4 + 1], d[t * 4 + 2]].map(|c| f32::from(c) / 255.0);
    unit_err(q) < CS_GRASS_THRESHOLD - GRASS_GUARD_MARGIN
}
```

`crates/engine/src/validate/mod.rs`: insert before `    if p.format.has_alpha() {`:

```rust
    // Design §3.6 (ruling R8): CS (and, by owner ruling, ENB) detects complex grass from the decoded bottom-left texel
    // of mip 0; the encoder re-encoded that block once at the slowest level when it failed.
    if p.subtype == Subtype::ComplexGrass {
        if let Some((lw, lh, Decoded::Ldr(px))) = decoded.first() {
            let q = px[((lh - 1) * lw) as usize];
            let e = crate::grass::unit_err([q[0], q[1], q[2]].map(|c| f32::from(c) / 255.0));
            if e >= crate::grass::CS_GRASS_THRESHOLD - crate::grass::GRASS_GUARD_MARGIN {
                w.push(
                    Warning::new(
                        Code::ValGrassDetect,
                        format!(
                            "decoded bottom-left texel ({}, {}, {}) has |L-1| = {e:.4} >= 0.02 after re-encoding at \
                             the slowest BC7 level: Community Shaders may not detect this texture as complex grass",
                            q[0], q[1], q[2]
                        ),
                    )
                    .with_data(json!({ "texel": [q[0], q[1], q[2]], "err": e })),
                );
            }
        }
    }
```

- [ ] **Step 4: Run the tests**

Run: `cargo test -p engine --lib encode::tests && cargo test -p engine --test complex_grass`
Expected: PASS; the owner test is listed as ignored.

On the owner's machine (owner or controller runs it, read-only on owner data):
`$env:SDC_ACHERUS='C:\_mount\moddingvol\Acherus'; cargo test --release -p engine --test complex_grass -- --ignored --nocapture`
Expected: PASS, with a line `owner example: decoded corner [..], |L-1| = 0.0xxxx` (below 0.02). Record the line in the task report.

- [ ] **Step 5: Commit**

```bash
cargo fmt --all && cargo clippy --workspace --all-targets -- -D warnings && cargo test --workspace
git add crates/engine/src/encode.rs crates/engine/src/validate/mod.rs crates/engine/tests/complex_grass.rs
git commit -m "engine: complex grass detection-texel guard (re-encode at slowest) and VAL-GRASS-DETECT; owner example test (design §3.6, §6)

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

---

### Task 5: auto-detect, engine side

**Files**
- Modify: `crates/engine/src/grass.rs` (`path_key`, `looks_like_grass`, test)
- Modify: `crates/engine/src/policy.rs` (`resolve_file_with`, `suggest_grass`)
- Modify: `crates/engine/src/api.rs` (`BatchRequest::gras_diffuses`, both `resolve_file` call sites)
- Test: `crates/engine/tests/complex_grass.rs` (append)

**Interfaces**
- Consumes: Task 2 `GrassFacts`, constants.
- Produces:
  - `pub fn grass::path_key(path: &str) -> String`: lower case, `/`, without the extension (`"textures/plants/g.png"` and `"textures\\Plants\\G.dds"` → `"textures/plants/g"`).
  - `pub fn grass::looks_like_grass(a: &Analysis) -> bool` (design §5 conditions 2–4).
  - `BatchRequest::gras_diffuses: BTreeSet<String>` (`path_key` keys; empty in `::new`, so the CLI never auto-tags).
  - `pub fn policy::resolve_file_with(ctx: &ResolveCtx, f: &FileFacts, gras_diffuse: bool) -> ResolvedJob`; `resolve_file(ctx, f)` = `resolve_file_with(ctx, f, false)`.

- [ ] **Step 1: Write the failing tests**

Append to `grass.rs`'s `mod tests`:

```rust
    #[test]
    fn path_key_normalises_and_drops_the_extension() {
        assert_eq!(path_key("textures/plants/g.png"), "textures/plants/g");
        assert_eq!(path_key("textures\\Plants\\G.dds"), "textures/plants/g");
        assert_eq!(path_key("textures/a.b/c"), "textures/a.b/c");
    }
```

Append to `crates/engine/tests/complex_grass.rs`:

```rust
fn plan_gras(d: &TempDir, target: Target, gras: bool, sets: &[&str]) -> Plan {
    let mut r = req(d, target, sets);
    if gras {
        r.gras_diffuses = ["textures/plants/g".to_string()].into();
    }
    plan_req(r)
}

/// §5: all four conditions hold: a high-confidence complex_grass, decided under --accept-suggestions high.
#[test]
fn all_four_conditions_tag_complex_grass() {
    let d = tempfile::tempdir().unwrap();
    put(&d, "g.png", &grass(32));
    let p = plan_gras(&d, Target::Cs, true, &[]);
    let j = job(&p, "g.png");
    assert_eq!(
        (j.suggestions[0].subtype, j.suggestions[0].confidence),
        (Subtype::ComplexGrass, engine::suggest::Confidence::High)
    );
    assert_eq!(j.profile.subtype, Subtype::ComplexGrass);
}

/// §5 (Review Focus 3): each condition false alone leaves the existing classification.
#[test]
fn each_condition_false_alone_leaves_the_classification() {
    let mut bad_corner = grass(32);
    *bad_corner.at_mut(0, 63) = [0.5, 0.5, 0.5, 12.0 / 255.0];
    let mut bad_bottom = grass(32);
    for i in 0..21 {
        bad_bottom.px[32 * 32 + i] = [0.5, 0.5, 0.5, 12.0 / 255.0];
    }
    let square = Rgba32F::new(32, 32, [0.25, 0.5, 0.15, 1.0]);
    for (what, img, gras) in [
        ("no GRAS record", grass(32), false),
        ("not 1:2", square, true),
        ("corner fails 0.03", bad_corner, true),
        ("bottom < 99 % unit", bad_bottom, true),
    ] {
        let d = tempfile::tempdir().unwrap();
        put(&d, "g.png", &img);
        let j = job(&plan_gras(&d, Target::Cs, gras, &[]), "g.png").clone();
        assert!(
            j.suggestions.iter().all(|s| s.subtype != Subtype::ComplexGrass),
            "{what}: {:?}",
            j.suggestions
        );
        assert_ne!(j.profile.subtype, Subtype::ComplexGrass, "{what}");
    }
}

/// §5 last paragraph: a user tag always wins, both ways.
#[test]
fn a_user_tag_wins_both_ways() {
    let d = tempfile::tempdir().unwrap();
    put(&d, "g.png", &grass(32));
    let p = plan_gras(&d, Target::Cs, true, &["file:**/g.png.subtype=diffuse"]);
    assert_eq!(job(&p, "g.png").profile.subtype, Subtype::Diffuse);
    let p = plan_gras(&d, Target::Cs, false, &[TAG]);
    assert_eq!(job(&p, "g.png").profile.subtype, Subtype::ComplexGrass);
}

/// R1 (Review Focus 4).
#[test]
fn vanilla_is_never_auto_tagged() {
    let d = tempfile::tempdir().unwrap();
    put(&d, "g.png", &grass(32));
    let p = plan_gras(&d, Target::Vanilla, true, &[]);
    let j = job(&p, "g.png");
    assert!(j.suggestions.iter().all(|s| s.subtype != Subtype::ComplexGrass));
    assert_ne!(j.profile.subtype, Subtype::ComplexGrass);
}
```

- [ ] **Step 2: Run them to see them fail**

Run: `cargo test -p engine --test complex_grass`
Expected: compile error `no field gras_diffuses on type BatchRequest`.

- [ ] **Step 3: Implement**

`crates/engine/src/grass.rs`, after `facts`:

```rust
/// The key `BatchRequest::gras_diffuses` holds for a texture path: lower case, `/`, without the extension, so an
/// input `textures/plants/g.png` meets the scan's `textures/plants/g.dds`.
pub fn path_key(path: &str) -> String {
    let p = path.replace('\\', "/").to_ascii_lowercase();
    match p.rsplit_once('.') {
        Some((stem, ext)) if !ext.contains('/') => stem.to_string(),
        _ => p,
    }
}

/// Design §5 conditions 2-4 (condition 1, the GRAS record, is the caller's): 1:2, the bottom-left texel passes CS's
/// 0.03 test, >= 99 % unit-length normals in the bottom half.
pub fn looks_like_grass(a: &crate::analyse::Analysis) -> bool {
    a.grass.is_some_and(|g| {
        g.corner_err < CS_GRASS_THRESHOLD && g.bottom_unit_share >= GRASS_UNIT_SHARE
    })
}
```

`crates/engine/src/policy.rs`: replace

```rust
pub fn resolve_file(ctx: &ResolveCtx, f: &FileFacts) -> ResolvedJob {
    let d = decide_subtype(ctx, f);
```

with

```rust
pub fn resolve_file(ctx: &ResolveCtx, f: &FileFacts) -> ResolvedJob {
    resolve_file_with(ctx, f, false)
}

/// [`resolve_file`] for a file the plugin scanner names as the diffuse of a GRAS model (`gras_diffuse`, complex grass
/// design §5, ruling R11): when nothing explicit chose its sub-type and its analysis passes the detection tests, its
/// top suggestion becomes a high-confidence `complex_grass`. Never under vanilla (ruling R1).
pub fn resolve_file_with(ctx: &ResolveCtx, f: &FileFacts, gras_diffuse: bool) -> ResolvedJob {
    let mut d = decide_subtype(ctx, f);
    if gras_diffuse {
        suggest_grass(ctx, f, &mut d);
    }
```

and add after `resolve_file_with`:

```rust
/// Design §5: the auto-tag, only over a suggestion (a `--set`, rule, `[files]` entry or set shader has decided already).
fn suggest_grass(ctx: &ResolveCtx, f: &FileFacts, d: &mut SubtypeDecision) {
    if !matches!(d.provenance, Provenance::Suggestion)
        || ctx.target == Target::Vanilla
        || !crate::grass::looks_like_grass(f.analysis)
    {
        return;
    }
    d.suggestions.insert(
        0,
        Suggestion {
            subtype: Subtype::ComplexGrass,
            confidence: Confidence::High,
            reason: "diffuse of a GRAS model; 1:2; bottom-left texel passes CS's 0.03 test; >= 99% unit normals in \
                     the bottom half"
                .into(),
        },
    );
    d.subtype = Subtype::ComplexGrass;
    d.decided = ctx.accept == AcceptSuggestions::High;
}
```

`crates/engine/src/api.rs`:
- In `pub struct BatchRequest`, after the `mesh_alpha` field add

```rust
    /// GUI (complex grass design §5): the slot-0 textures of the NIFs that GRAS records of the active load order use,
    /// as `grass::path_key` keys. A listed 1:2 file that passes the detection tests gets a high-confidence
    /// `complex_grass` suggestion. Empty for the CLI, which therefore never auto-tags.
    pub gras_diffuses: BTreeSet<String>,
```

- In `BatchRequest::new`, after `            mesh_alpha: BTreeMap::new(),` add `            gras_diffuses: BTreeSet::new(),`.
- Before the line `        let resolve_one = |&(set, f): &(&TextureSet, &InputFile),` add `        let gras = &req.gras_diffuses;`.
- In `resolve_one`, replace `            let mut job = resolve_file(&ctx, &facts);` with

```rust
            let gras_diffuse = gras.contains(&crate::grass::path_key(&f.game_path));
            let mut job = resolve_file_with(&ctx, &facts, gras_diffuse);
```

- In the 6c re-resolve, replace `                resolve_file(&ctx, &facts)` with

```rust
                resolve_file_with(
                    &ctx,
                    &facts,
                    gras.contains(&crate::grass::path_key(&f.game_path)),
                )
```

(`api.rs` imports `crate::policy::*`, so `resolve_file_with` is in scope.)

- [ ] **Step 4: Run the tests**

Run: `cargo test -p engine --test complex_grass && cargo test -p engine --lib grass::tests`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
cargo fmt --all && cargo clippy --workspace --all-targets -- -D warnings && cargo test --workspace
git add crates/engine/src/grass.rs crates/engine/src/policy.rs crates/engine/src/api.rs crates/engine/tests/complex_grass.rs
git commit -m "engine: complex grass auto-detect from BatchRequest::gras_diffuses, user tag wins (design §5, R11)

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

---

### Task 6: GUI pickers hide `complex_grass` under vanilla

**Files**
- Modify: `crates/gui/src/ui/settings_panel.rs` (`offered_groups`, `options_of`, `subtype_decision`, import)
- Modify: `crates/gui/src/ui/dialogs.rs` (proceed-anyway picker, imports)
- Create: `crates/gui/tests/complex_grass.rs`

**Interfaces**
- Consumes: Task 2 `engine::policy::offered`.
- Produces: `pub fn sdc_gui::ui::settings_panel::offered_groups(target: Target) -> Vec<(&'static str, Vec<Subtype>)>`.

- [ ] **Step 1: Write the failing test**

Create `crates/gui/tests/complex_grass.rs`:

```rust
//! Complex grass in the GUI (design `2026-10-12-complex-grass-design.md`, rulings R1, R12).
use engine::config::{Subtype, Target};
use sdc_gui::ui::settings_panel::{offered_groups, subtype_groups};

/// R1 (Review Focus 4): offered under cs and cpm (ENB), not under vanilla; grouped with the packed maps.
#[test]
fn pickers_hide_complex_grass_under_vanilla() {
    for (t, want) in [(Target::Cs, true), (Target::Cpm, true), (Target::Vanilla, false)] {
        let listed = offered_groups(t)
            .into_iter()
            .any(|(_, m)| m.contains(&Subtype::ComplexGrass));
        assert_eq!(listed, want, "{t:?}");
    }
    let groups = subtype_groups(|_| true);
    let (heading, _) = groups
        .iter()
        .find(|(_, m)| m.contains(&Subtype::ComplexGrass))
        .unwrap();
    assert_eq!(*heading, "Packed and data");
}
```

- [ ] **Step 2: Run it to see it fail**

Run: `cargo test -p sdc-gui --test complex_grass`
Expected: compile error `unresolved import sdc_gui::ui::settings_panel::offered_groups`.

- [ ] **Step 3: Implement**

`crates/gui/src/ui/settings_panel.rs`:
- Replace `use engine::policy::{availability, Availability, ResolvedJob};` with `use engine::policy::{offered, ResolvedJob};`.
- After `pub fn subtype_groups(…) { … }` add

```rust
/// The sub-type groups a picker offers under `target` (`engine::policy::offered`: not `complex_grass` under vanilla,
/// complex grass design §3.7).
pub fn offered_groups(target: Target) -> Vec<(&'static str, Vec<Subtype>)> {
    subtype_groups(|s| offered(target, s))
}
```

- In `options_of`, replace

```rust
        "subtype" => subtype_groups(|s| {
            s == p.subtype || matches!(availability(target, s), Availability::Available)
        })
```

with

```rust
        "subtype" => subtype_groups(|s| s == p.subtype || offered(target, s))
```

- In `subtype_decision`, replace

```rust
                for (heading, members) in subtype_groups(|s| {
                    matches!(availability(cx.target, s), Availability::Available)
                }) {
```

with `                for (heading, members) in offered_groups(cx.target) {`.

`crates/gui/src/ui/dialogs.rs`, in `proceed_anyway`: replace

```rust
    use super::settings_panel::{subtype_groups, subtype_label};
    use engine::policy::{availability, Availability};
```

with `    use super::settings_panel::{offered_groups, subtype_label};`, and replace

```rust
                                for (heading, members) in subtype_groups(|s| {
                                    matches!(availability(target, s), Availability::Available)
                                }) {
```

with `                                for (heading, members) in offered_groups(target) {`.

- [ ] **Step 4: Run the tests**

Run: `cargo test -p sdc-gui --test complex_grass --test packing_fills --test pbr_members`
Expected: PASS (the two existing tests check every sub-type is in exactly one group).

- [ ] **Step 5: Commit**

```bash
cargo fmt --all && cargo clippy --workspace --all-targets -- -D warnings && cargo test --workspace
git add crates/gui/src/ui/settings_panel.rs crates/gui/src/ui/dialogs.rs crates/gui/tests/complex_grass.rs
git commit -m "gui: sub-type pickers use engine::policy::offered (complex_grass hidden under vanilla, design §3.7)

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

---

### Task 7: GRAS diffuses from the scan DB into the plan — DEPENDS ON THE PLUGIN SCANNER

Start only when `nif/f-esp` (plan `2026-10-12-plan-esp-scanner.md`) is merged into `nif/main` and merged into `nif/g-grass` (`git merge nif/main`, no rebase). Check first: `crates/modscan/src/plugin_store.rs` exists, `modscan::store::SCHEMA_VERSION` is 4, and `crates/esp/tests/common/mod.rs` exports `edid, plugin, rec, sub, top, zstr`. If the scanner renamed `rec`, `rec_winner` or `model_ref` (or their columns `formid`, `local`, `plugin_id`, `sig`, `slot`, `path_id`), adapt the SQL below to its schema and say so in the task report.

**Files**
- Modify: `crates/modscan/src/plugin_store.rs` (`Store::gras_diffuse_textures`)
- Create: `crates/modscan/tests/gras_diffuse.rs`
- Modify: `crates/gui/src/model/meshes.rs` (`pub fn gras_diffuses`)
- Modify (shared, add-only, own commit): `crates/gui/src/model/mod.rs`
- Test: `crates/gui/tests/complex_grass.rs` (append)

**Interfaces**
- Consumes: scanner schema v4 (`rec`, `rec_winner`, `model_ref`; scanner plan Task 4), NIF tables (`nif_winner`, `nif_copy`, `origin`, `shape`, `tex_use`, `path`); Task 5 `BatchRequest::gras_diffuses`, `engine::grass::path_key`.
- Produces:
  - `pub fn Store::gras_diffuse_textures(&self) -> Result<BTreeSet<String>, ScanError>`: store-form paths (lower case, `/`, `textures/…`) in slot 0 of the lighting shapes of the winning NIF named by `MODL` (model slot 0) of every winning GRAS record.
  - `pub fn sdc_gui::model::meshes::gras_diffuses(db: &Path) -> Result<BTreeSet<String>, String>` (`path_key` keys).
  - `AppModel::gras_diffuses: BTreeSet<String>`, read with the mesh evidence in `refresh_mesh_votes`, part of `EngineInputs`, sent as `BatchRequest::gras_diffuses`.

- [ ] **Step 1: Write the failing tests**

Create `crates/modscan/tests/gras_diffuse.rs`:

```rust
//! Complex grass design §5 condition 1: the diffuses of the NIFs GRAS records use.
mod common;
#[path = "../../esp/tests/common/mod.rs"]
mod b;
use b::{edid, plugin, rec, sub, top, zstr};
use common::instance_fixture;
use modscan::mo2::Instance;
use modscan::scan::{run, Source};
use modscan::store::Store;
use std::collections::BTreeSet;
use std::path::Path;
use std::sync::atomic::AtomicBool;

fn put(root: &Path, rel: &str, bytes: &[u8]) {
    let p = root.join(rel);
    std::fs::create_dir_all(p.parent().unwrap()).unwrap();
    std::fs::write(p, bytes).unwrap();
}

/// Skyrim.esm with one record `sig` (EDID Tundra) whose MODL is `x.nif`; mod A ships `meshes/x.nif`.
fn scanned(sig: &[u8; 4]) -> (tempfile::TempDir, Store) {
    let d = instance_fixture("+A\n");
    let nif = std::fs::read(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../nif/tests/fixtures/TestNifFile_Skinned_SE.nif"
    ))
    .unwrap();
    put(d.path(), "mods/A/meshes/x.nif", &nif);
    let r = rec(sig, 0xA01, &[edid("Tundra"), sub(b"MODL", &zstr("x.nif"))]);
    put(
        d.path(),
        "Game/Data/Skyrim.esm",
        &plugin(esp::FLAG_ESM, &[], &[top(sig, &[r])]),
    );
    put(d.path(), "profiles/Default/plugins.txt", b"");
    let mut s = Store::open(&d.path().join("scan.db")).unwrap();
    let src = Source::Mo2(Instance::load(d.path(), None).unwrap());
    run(&src, &mut s, &AtomicBool::new(false), &|_| {}, &|| vec![]).unwrap();
    (d, s)
}

/// Slot-0 textures of the lighting shapes of `meshes/x.nif`, read through the NIF API.
fn x_diffuses(s: &Store) -> BTreeSet<String> {
    let a = s.mods().unwrap().into_iter().find(|m| m.name == "A").unwrap().id;
    let copy = s.nifs_of(a).unwrap()[0].copy_id;
    s.shapes_of(copy)
        .unwrap()
        .into_iter()
        .filter(|sh| sh.shader_kind == 1)
        .filter_map(|sh| sh.textures.first().cloned().flatten())
        .collect()
}

#[test]
fn a_gras_record_names_its_model_diffuses() {
    let (_d, s) = scanned(b"GRAS");
    let want = x_diffuses(&s);
    assert!(!want.is_empty());
    assert_eq!(s.gras_diffuse_textures().unwrap(), want);
}

#[test]
fn a_stat_with_the_same_model_names_none() {
    let (_d, s) = scanned(b"STAT");
    assert!(s.gras_diffuse_textures().unwrap().is_empty());
}
```

Append to `crates/gui/tests/complex_grass.rs`:

```rust
mod support;
use std::time::Duration;

/// A `w` x `2w` grass PNG (as the engine tests' `grass`): top colour with cutout dots, bottom unit normals, alpha 12.
fn grass_png(path: &std::path::Path, w: u32) {
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    image::ImageBuffer::from_fn(w, 2 * w, |x, y| {
        if y < w {
            image::Rgba([64u8, 128, 38, if (x * 7 + y * 13) % 10 < 3 { 255 } else { 0 }])
        } else {
            let t = (x as f32 / w as f32 - 0.5) * 0.8;
            let c = |v: f32| ((v * 0.5 + 0.5) * 255.0).round() as u8;
            image::Rgba([c(t.sin()), 128, c(t.cos()), 12])
        }
    })
    .save(path)
    .unwrap();
}

/// A temp scan DB holding one GRAS record whose model `meshes/plants/tundra.nif` uses `tex` in slot 0. NIF rows go
/// through the store API; the scanner's plugin rows are written as the scanner would (schema v4).
fn gras_db(dir: &std::path::Path, tex: &str) -> std::path::PathBuf {
    use modscan::store::{CopyResult, OriginSpec, Store, KIND_MOD};
    let p = dir.join("scan.db");
    let mut s = Store::open(&p).unwrap();
    s.begin_scan().unwrap();
    let o = s
        .upsert_origin(&OriginSpec {
            kind: KIND_MOD,
            name: "Grass".into(),
            root: dir.join("mods/Grass").to_string_lossy().into_owned(),
            parent: None,
            priority: 0,
            derived_by: None,
            derived_state: None,
        })
        .unwrap();
    let mesh = "meshes/plants/tundra.nif";
    let scan = nif::scan::NifScan {
        bsver: 100,
        le_mesh: false,
        shapes: vec![lighting_shape(3, &[tex])],
        problems: vec![],
    };
    let id = s
        .upsert_copy(mesh, o, 1, 1, CopyResult::Scanned(scan), None)
        .unwrap();
    let pid = s.path_ids(&[mesh]).unwrap()[0];
    s.set_winners(&[(pid, (id, 1))].into_iter().collect(), &Default::default())
        .unwrap();
    let c = s.conn();
    c.execute(
        "INSERT INTO plugin (id, name, path, size, mtime, lo_index, light, flags, masters_json)
         VALUES (1, 'Skyrim.esm', 'Skyrim.esm', 1, 1, 0, 0, 1, '[]')",
        [],
    )
    .unwrap();
    c.execute(
        "INSERT INTO rec (formid, local, plugin_id, sig, edid, owner_ok) VALUES (2561, 2561, 1, 'GRAS', 'Tundra', 1)",
        [],
    )
    .unwrap();
    c.execute(
        "INSERT INTO rec_winner (formid, plugin_id, n_overrides) VALUES (2561, 1, 0)",
        [],
    )
    .unwrap();
    c.execute(
        "INSERT INTO model_ref (formid, local, plugin_id, slot, path_id) VALUES (2561, 2561, 1, 0, ?1)",
        [pid],
    )
    .unwrap();
    p
}

/// `set_preview_model.rs`'s lighting shape (block `block`, slots `slots`), copied: test binaries cannot share it.
fn lighting_shape(block: u32, slots: &[&str]) -> nif::scan::ShapeRow {
    use nif::props::{FieldOffsets, Lighting, LightingParams, LightingTrailer};
    use nif::shape_head::{ShapeHead, ShapeKind};
    nif::scan::ShapeRow {
        block,
        head: ShapeHead {
            kind: ShapeKind::BSTriShape,
            name: Some("Grass".into()),
            skinned: false,
            shader: 1,
            alpha: -1,
            vertex_desc: None,
            num_triangles: Some(1),
            num_vertices: Some(3),
        },
        shader: nif::scan::Shader::Lighting(
            Lighting {
                name: None,
                shader_type: 0,
                slsf1: 0,
                slsf2: 0,
                texture_set: 2,
                params: LightingParams {
                    uv_offset: [0.0; 2],
                    uv_scale: [1.0; 2],
                    emissive_color: [0.0; 3],
                    emissive_multiple: 1.0,
                    texture_clamp_mode: 3,
                    alpha: 1.0,
                    refraction_strength: 0.0,
                    glossiness: 80.0,
                    specular_color: [1.0; 3],
                    specular_strength: 1.0,
                    lighting_effect_1: 0.3,
                    lighting_effect_2: 2.0,
                    trailer: LightingTrailer::None,
                },
                field_offsets: FieldOffsets {
                    block_offset: 0,
                    slsf1: 0,
                    uv_offset: 0,
                    emissive_color: 0,
                    glossiness: 0,
                    trailer: 0,
                },
            },
            slots
                .iter()
                .map(|p| nif::path::canon_texture(p.as_bytes()))
                .collect(),
        ),
        alpha: None,
    }
}

/// Design §5 end to end in the model: the scan names the texture as a GRAS diffuse, the next plan tags it.
#[test]
fn a_gras_diffuse_from_the_scan_db_is_tagged_after_the_plan() {
    let d = tempfile::tempdir().unwrap();
    grass_png(&d.path().join("textures/plants/g.png"), 32);
    let mut m = support::opened(d.path());
    m.accept = engine::policy::AcceptSuggestions::High;
    let key = "plants/g.png";
    let subtype = |m: &sdc_gui::model::AppModel| {
        m.items
            .iter()
            .find(|i| i.key == key)
            .and_then(|i| i.job.as_ref())
            .map(|j| j.profile.subtype)
    };
    assert_ne!(subtype(&m), Some(Subtype::ComplexGrass), "no scan DB, no tag");
    let dbdir = tempfile::tempdir().unwrap();
    m.scan_db = Some(gras_db(dbdir.path(), r"textures\plants\g.dds"));
    m.replan();
    assert!(m.wait_idle(Duration::from_secs(60)), "planning did not finish");
    assert_eq!(
        m.gras_diffuses,
        ["textures/plants/g".to_string()].into_iter().collect()
    );
    assert_eq!(m.request().unwrap().gras_diffuses, m.gras_diffuses);
    assert_eq!(subtype(&m), Some(Subtype::ComplexGrass));
}
```

(`opened` plans once with `scan_db = None`; the `replan` plans, `refresh_mesh_votes` then reads the DB, the inputs differ, and the model plans again by itself, bounded by `auto_replans`.)

Before relying on the real NIF fixture, check that it has a slot-0 texture on a lighting shape: temporarily add `eprintln!("{:?}", x_diffuses(&s));` to `a_gras_record_names_its_model_diffuses` after Step 3, run it with `--nocapture`, and remove the line. If the set is empty, replace `scanned`'s NIF with rows written through the store API (`upsert_copy` + `set_winners` + a lighting shape with a slot-0 path, as `gras_db` in the GUI test below) and keep the plugin built by the esp builders.

- [ ] **Step 2: Run them to see them fail**

Run: `cargo test -p modscan --test gras_diffuse; cargo test -p sdc-gui --test complex_grass`
Expected: compile errors `no method named gras_diffuse_textures found for struct Store` and `no field gras_diffuses on type AppModel`.

- [ ] **Step 3: Implement the store query and the meshes reader (one commit)**

`crates/modscan/src/plugin_store.rs`, inside the scanner's `impl Store` block, add (add `use std::collections::BTreeSet;` to its imports if absent):

```rust
    /// Complex grass design §5 condition 1: slot-0 textures of the lighting shapes of the winning NIF that the `MODL`
    /// (model slot 0) of each winning GRAS record names, in an enabled origin. Store form: lower case, `/`.
    pub fn gras_diffuse_textures(&self) -> Result<BTreeSet<String>, ScanError> {
        let mut st = self.conn.prepare(
            "SELECT DISTINCT tp.p
             FROM model_ref m
             JOIN rec_winner w ON w.formid = m.formid AND w.plugin_id = m.plugin_id
             JOIN rec r ON r.plugin_id = m.plugin_id AND r.local = m.local AND r.sig = 'GRAS'
             JOIN nif_winner nw ON nw.path_id = m.path_id
             JOIN nif_copy c ON c.id = nw.copy_id
             JOIN origin o ON o.id = c.origin_id AND o.enabled = 1
             JOIN shape s ON s.copy_id = c.id AND s.shader_kind = 1
             JOIN tex_use tu ON tu.texset_id = s.texset_id AND tu.slot = 0
             JOIN path tp ON tp.id = tu.tex_path_id
             WHERE m.slot = 0",
        )?;
        let v = st
            .query_map([], |r| r.get::<_, String>(0))?
            .collect::<Result<_, _>>()?;
        Ok(v)
    }
```

`crates/gui/src/model/meshes.rs`, after `pub fn mesh_alpha(…) { … }`:

```rust
/// Complex grass design §5 condition 1: the GRAS diffuses of the scan, as `engine::grass::path_key` keys (sent as
/// `BatchRequest::gras_diffuses`).
pub fn gras_diffuses(db: &Path) -> Result<BTreeSet<String>, String> {
    let store = Store::open_read_only(db).map_err(db_err)?;
    let paths = store.gras_diffuse_textures().map_err(db_err)?;
    Ok(paths.iter().map(|p| engine::grass::path_key(p)).collect())
}
```

Run: `cargo test -p modscan --test gras_diffuse` → PASS. Gates, then:

```bash
git add crates/modscan/src/plugin_store.rs crates/modscan/tests/gras_diffuse.rs crates/gui/src/model/meshes.rs
git commit -m "modscan, gui: GRAS diffuse textures query and reader (complex grass design §5 condition 1)

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

- [ ] **Step 4: Wire the model (shared file, add-only, own commit)**

`crates/gui/src/model/mod.rs`, additions only:
- `struct EngineInputs`: after `    mesh_alpha: std::collections::BTreeMap<String, Vec<engine::shader::MeshAlpha>>,` add `    gras_diffuses: std::collections::BTreeSet<String>,`.
- `EngineInputs::of`: after `            mesh_alpha: m.mesh_alpha.clone(),` add `            gras_diffuses: m.gras_diffuses.clone(),`.
- `pub struct AppModel`: after the `pub mesh_alpha: …` field add

```rust
    /// Complex grass design §5: the GRAS diffuses of the scan (`engine::grass::path_key` keys), read with the mesh
    /// evidence; sent as `BatchRequest::gras_diffuses`.
    pub gras_diffuses: std::collections::BTreeSet<String>,
```

- `AppModel::placeholder`: after `            mesh_alpha: Default::default(),` add `            gras_diffuses: Default::default(),`.
- `request()`: after `        req.mesh_alpha = self.mesh_alpha.clone();` add `        req.gras_diffuses = self.gras_diffuses.clone();`.
- `refresh_mesh_votes()`: before its final `        changed` line add

```rust
        // Complex grass design §5 condition 1, from the same database.
        let gras = match &self.scan_db {
            Some(db) if db.is_file() => meshes::gras_diffuses(db).unwrap_or_default(),
            _ => Default::default(),
        };
        let changed = changed || gras != self.gras_diffuses;
        self.gras_diffuses = gras;
```

- `replan()`: in the `EngineInputs { … }` literal, after `            mesh_alpha: req.mesh_alpha.clone(),` add `            gras_diffuses: req.gras_diffuses.clone(),`.

- [ ] **Step 5: Run the tests**

Run: `cargo test -p sdc-gui --test complex_grass --test sets_model --test model`
Expected: PASS.

- [ ] **Step 6: Commit**

```bash
cargo fmt --all && cargo clippy --workspace --all-targets -- -D warnings && cargo test --workspace
git add crates/gui/src/model/mod.rs crates/gui/tests/complex_grass.rs
git commit -m "gui: send the scan's GRAS diffuses with each plan (add-only model wiring; complex grass design §5)

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

---

### Task 8: owner checklist

No code. The owner (or the controller, with the owner) runs these on the release build and records the results in the branch hand-off; nothing is committed unless a step fails and a fix task is opened.

- [ ] **1. Owner example, hand tag (CS).** In the GUI open the folder holding a copy of `grassfieldtundra.png`; set its sub-type to *complex grass* (under "Packed and data") and alpha to *Cutout* (or let mesh evidence decide). Export. Expect: no `GRASS-*` warning, BC7 UNORM, 2048×4096, 12 mips, validation passed. Confirm with `sdc inspect <out>.dds` and `sdc validate <out>.dds`.
- [ ] **2. In game, Community Shaders** (`ComplexGrassThreshold` = 0.03, "Grass Lighting" on): install the exported DDS in an MO2 test mod over the original; load a tundra cell. Expect the grass lit with its normal/specular half (same look as the original source DDS), not the stretched 1:2 diffuse.
- [ ] **3. In game, ENB** (if the owner runs ENB complex grass): same file. Confirms the R2 assumption (same bottom-left detector). If ENB does not detect it, report: the assumption in design §2 is wrong.
- [ ] **4. Vanilla target.** Switch the target to vanilla: the dropdown no longer lists complex grass; the tagged file shows `GRASS-VANILLA` and still exports as BC7 UNORM.
- [ ] **5. Malformed copy.** Paint the bottom-left texel of a copy grey (128,128,128): `GRASS-DETECT-SOURCE` appears; the export still passes (R9, renormalised mip 0) and the decoded corner is unit length.
- [ ] **6. Auto-detect (needs the scanner).** Run an MO2 scan on the Meshes page, then open the texture folder of a GRAS grass diffuse: after the plan it resolves `complex_grass` with the reason "diffuse of a GRAS model …"; a non-grass 1:2 texture in the same folder does not. Set one grass file to *diffuse* by hand: it stays diffuse after another scan and plan.
- [ ] **7. Timing.** Note the export time of the owner example (bc7e veryslow, plus at most one slowest re-encoded block per level).

---

## Self-review

- **Spec coverage.** §1 subtype, both CS and ENB targets: Tasks 1–2 (R2). §2 facts: constants in Task 2, ENB assumption R2. §3.1–3.4 per-half build: Task 3. §3.5 BC7 UNORM: Task 2 CFG-INVALID + Task 3 e2e. §3.6 guard + named VAL- error: Task 4 (R8). §3.7 vanilla: Tasks 2, 5, 6 (R1). §4 four checks: Task 2. §5 four conditions, user tag wins, nothing before the scanner: Tasks 5, 7 (R11, R12). §6 tests: Tasks 2–5, 7; owner `#[ignore]`: Task 4. §7 constraints: Global Constraints; schema flag: Task 1 Step 6.
- **Placeholder scan.** No TBD/TODO; every code step carries the code. Task 7's only conditional is the scanner-name check, with the exact schema the SQL assumes.
- **Type consistency.** `GrassFacts` fields (`top_opaque`, `top_near_binary`, `bottom_unit_share`, `corner_err`), `unit_err`, `facts`, `build`, `path_key`, `looks_like_grass`, `offered`, `offered_groups`, `resolve_file_with`, `BatchRequest::gras_diffuses`, `AppModel::gras_diffuses`, `Store::gras_diffuse_textures`, `meshes::gras_diffuses` are used with the same names and types in every task.
- **Review Focus.** Five items, each pinned by a named test in its owning task.

## Owner decisions (2026-10-12, after drafting)

- schema/ edit ALLOWED (R3): the schema-only commit may land; the implementer still checks its diff is generator output only.
- R8 confirmed: on guard failure the file is written and VAL-GRASS-DETECT fails validation (exit 4).
- R1 confirmed: vanilla hides the sub-type in pickers, never auto-tags, manual tag encodes as complex_grass with GRASS-VANILLA.

