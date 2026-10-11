# sdc M4: texture-set variance bake and coverage fixes, implementation plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** A PBR set's normal-map variance widens the roughness of its `_rmaos`/`_rfaos`, and a `_cnr`'s own variance widens its coat roughness. Mip 0 of every normal map is stored at unit length by default. The four `SET-*` codes are emitted. The two mipcheck coverage defects (B1, B6) are fixed.

**Architecture:**
- **Coverage first** (`coverage.rs`, Tasks 1–2):
  - every level passes exactly `k` texels: ties at the coverage boundary are split in 8×8 Bayer order, so thin strands keep their coverage at the low mips (B1, owner);
  - remapped texels sit two codes clear of the threshold, so BC7 cannot flip them (B6).
- **Mips** (`mips.rs`, Tasks 4–6):
  - Mip 0 of a normal map is renormalised unless `mips.normal_base = keep`.
  - The accumulation chain starts from mip 0 **as written**. Levels ≥ 1, the bake's footprint lengths and the `_cnr` self-bake therefore all average the vectors the file stores. Under the default those are unit length, so the variance reflects real divergence of the normals, not a short exporter.
  - Vector and roughness chains are never alpha-weighted.
  - `build_chain_with(work, profile, Option<&NormalVariance>)` widens a roughness member's R channel.
  - A `_cnr` with `mips.normal = renormalize_bake` widens its own alpha from its own footprint lengths.
- **The bake** (new `crates/engine/src/bake/`, Tasks 5, 7, 8):
  - **Math** (`mod.rs`): `variance_add`, `baked_roughness`, `bake_shift`, `LengthPyramid` (a box pyramid of footprint lengths, exact for power-of-two ratios) and `NormalVariance`.
  - **`BakeJob`** on `ResolvedJob`: the roughness job decodes its `_n` itself, keeps only the lengths, then loads its own image. There is no cross-job state and no scheduler ordering.
  - **`plan::plan_bakes`** runs in `Engine::plan` after packing. It attaches `BakeJob`s, records `mips.normal = renormalize_bake` on the `_n` with provenance `set`, and emits the four `SET-*` codes. It also honours the GUI's shared roughness slots.
- **Manifest:** the baked `_n` is a recorded source with a decode-settings identity, so `if-changed` re-encodes the roughness member when the `_n` or its settings change.

**Tech Stack:** Rust 2021 workspace (rustc 1.99.0, MSRV 1.88). Crates in use: `serde`, `schemars`, `image`, `sha2`. No new dependencies.

**Spec:** `docs/superpowers/specs/2026-10-08-skyrim-dds-converter-design.md`. The plan argues from these sections:
- §6.2 (variance bake) and §6.3 (ordering and edge cases);
- the §4.1 rows for `normal`, `pbr_normal`, `coat_normal`, `rmaos` and `rfaos` (the sub-type is `skin_rfaos` after the packing-fills plan);
- §5.5 (mipmaps and coverage), §5.8 (validation) and §10 (`SET-*`).

It also uses:
- `docs/DECISIONS.md`: #6, Note B, Note E (t = 0.25), "MVP without the variance bake", Note M and Note P;
- the roadmap section "M4 — Texture-set variance bake" in `docs/superpowers/plans/2026-10-08-skyrim-dds-converter-phase0-2.md`;
- `F:/ai/dds/mipcheck/RESULTS-2.md` (B1, B2, B6).

## Global Constraints

- **Branch:** `feat/m4-variance-bake` (checked out at `main` cc47a93). Never commit to `main`. Never push.
- **Gates:** run them unpiped before every commit, and all three must pass. In Git Bash, first run `export PATH="$HOME/.cargo/bin:$PATH"`.
  ```bash
  cargo fmt --all && cargo clippy --workspace --all-targets -- -D warnings && cargo test --workspace
  ```
- **Golden and `gui_repro`:** `crates/engine/tests/golden.rs` and `crates/cli/tests/gui_repro.rs` must stay green.
  - Every expectation that changes is listed by name in the task that changes it. Change only those.
  - If any other test fails, stop. Report `DONE_WITH_CONCERNS` with the test name and the failure. Do not edit the test.
- **Schemas:** a change to a code, a config field or a type in `schema/*.json` needs `cargo run -p sdc -- presets schema all --out schema`. Commit the regenerated files.
- **Python:** never run `python -` or `python3 -` with stdin. They hang on this machine.
- **Commits:**
  - Commit only the files you changed, by explicit path. No `git add -A` and no `git add .`.
  - End each message with `Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>`.
- **Concurrent NIF team:** it works in the worktree `F:/ai/dds/converter-nif` on `nif/*` branches.
  - M4 does not touch `crates/nif`, `crates/vfs` or `crates/modscan`.
  - M4 does not touch DECISIONS Note Q, which is reserved for that team.
- **Order:** runs after the packing-fills plan (`docs/superpowers/plans/2026-10-11-sdc-packing-fills.md`) is merged; rebase or merge main before Task 1.
  - That plan renames the sub-type `rfaos` to `skin_rfaos` (`Subtype::SkinRfaos`). The old id stays as a deserialise alias, and the file suffix and output name stay `_rfaos`.
  - In this plan, a **sub-type** is always `skin_rfaos`, while a **file suffix**, a canonical suffix string (`"rfaos"` in `bake/plan.rs`) or an output name stays `rfaos`/`_rfaos`.
  - M4 relies on `Subtype::is_roughness_member()` being `Rmaos | SkinRfaos` after that plan.
- **Files shared with the packing-fills plan:** read each at its merged state before editing, and add to it rather than replace.
  - `config.rs`: it adds `packing.recipe` and the skin sub-types.
  - `presets/sdc-base.toml`.
  - `overrides.rs` `FIELD_PATHS`.
  - `codes.rs`: it adds `SKIN-WET-FORM`, so the code count is 60 before Task 4.
  - `policy.rs`.
  - `crates/gui/src/model/{sets,fields}.rs` and `settings_panel.rs`.
  - DECISIONS Note R: it adds R-fills-1 and R-skin-1. Task 11 resolves the numbering.
- **f64 reference:** the reference implementation of the bake formula lives in test code only: `#[cfg(test)]` modules and `crates/*/tests`.
- **Downloads:** none are needed. If a step seems to need a crate that is not in `Cargo.lock`, stop and report `NEEDS_CONTEXT`.
- **Formula constants** (spec §6.2, DECISIONS Note E):
  - κ = (3L − L³)/(1 − L²) and v = 0.25/κ;
  - v = 0 when L ≥ 1 − 10⁻⁶;
  - α′² = saturate(α² + min(2v, t²)) and r′ = (α′²)^¼;
  - t = **0.25**, tunable per preset.
- **Normal-map default** (owner, 2026-10-11): mip 0 is renormalised by default, which **changes outputs for existing normal maps with short sources** (mip 0, and the levels averaged from it).

## Owner decisions (2026-10-11), recorded as DECISIONS Note R in Task 11

| # | Decision | Where |
|---|---|---|
| R1 | **Mip 0 of normal maps is renormalised by default.**<br>• Field `mips.normal_base`: `renormalize` (default) or `keep`, settable per preset, rule, `[files]` entry or `--set`.<br>• An Info code, `CHAN-NORMAL-SHORT`, fires when a normal source's mean length at mip 0 is below 0.9 (not for Z-rebuilt sources).<br>• The owner decided only "per-preset option, default renormalise". The controller corrected an earlier wording: the bake averages the mip-0 normals **as they will be written**, unit length under `renormalize` and as authored under `keep`. Variance then reflects real normal divergence, not exporter shortness. The stored chain accumulates from the same written mip 0.<br>• Outputs change for existing normal maps with short sources, at every level (mip 0 rescaled; lower levels average unit vectors). | Tasks 3, 4, 5, 7 |
| R2 | **`_cnr` self-bake:**<br>• The coat normal's own variance goes into its own alpha (coat roughness), with the same formula, 1:1 and no companion.<br>• It is on by default (`coat_normal` `mips.normal = renormalize_bake`).<br>• The alpha keeps today's plain box filter, so a flat coat normal's alpha is unchanged bit for bit. | Task 6 |
| R3 | The DDS header-patch idea and the CS/Vanilla two-mode idea are **not** part of M4. | — |

## Rulings made while planning (they refine the spec; Task 11 records each one in Note R)

| # | Ruling | Why | Cost if wrong |
|---|---|---|---|
| R4 | **The roughness member decodes its own `_n`.** `BakeJob { normal, normal_profile, shift, stamp }` sits on the roughness member's `ResolvedJob`. The job decodes the `_n` with the `_n`'s profile, keeps only the footprint lengths, drops the image, then loads its own source. This replaces spec §6.3's "kept as a summed-area table until the member's mips are built", and the M4 roadmap's set ordering in the scheduler | No cross-job state: it works when the `_n` is up to date, skipped or not run, in the GUI preview, and under any `--jobs`. A roughness member never waits on another job | One extra decode of each baked `_n` |
| R5 | **Footprint lengths come from a box pyramid**, not a summed-area table. The bake needs both bases to be powers of two with the same aspect, so the ratio is a power of two on both axes. Anything else raises `SET-BAKE-RATIO` (warning) and the set does not bake | At power-of-two sizes each footprint is whole texels of one pyramid level, so the pyramid gives the same averages as a summed-area table. It holds 1.33 bytes per `_n` texel instead of 24 (an f64 table: 6 GiB at 16K). Aspect mismatch and NPOT have no well-defined footprint in the box chain | A non-power-of-two pair (with `--allow DIM-NPOT`) never bakes |
| R6 | **The bake decision lives on the `_n`.** `plan_bakes` upgrades a `pbr_normal`'s built-in `mips.normal = renormalize` to `renormalize_bake`, with provenance `set` (new). An explicit `renormalize` from any other layer opts the set out. The `_n` is renormalised either way. `normal`, `landscape_normal` and `msn` never bake (spec §6.3, non-PBR), and `renormalize_bake` is inert on them | Spec §3.2 already says the set job may upgrade `mips.normal`, and §5.2 promised a set-level provenance source with M4 | None |
| R7 | **Shared roughness slots** (M3b spec §3.3, v1 instruction): `BatchRequest::shared_roughness` lists the ids of every set whose `_rmaos`/`_rfaos` slot is one output used by several sets (owner and borrowers). Those sets never bake and raise no missing-companion code. The CLI leaves the list empty | The engine has no notion of borrowing; only the GUI knows | None |
| R8 | **`SET-*` triggers:**<br>• `SET-MISSING-COMPANION` (info): a PBR set (`cs`) has a `pbr_normal` but no roughness member, or a roughness member but no `pbr_normal`; an `_msn` has no `_s`; a `_flow` has no `_n`.<br>• `SET-COMPANION-DDS` (warning): the missing companion exists only as a DDS, in the set or on disk.<br>• `SET-COMPANION-OUTSIDE` (info): the missing companion is an image on disk, next to the file, that is not in the batch.<br>• All three apply in both directions (`_n` missing and roughness missing).<br>• `SET-BAKE-RATIO` (warning): see R5.<br>Codes go on the job they concern. A PBR set holding only an albedo raises none | Each code names the file the artist actually converted. A lone albedo makes no bake decision to explain | One info line per affected file |
| R9 | **What counts as a usable `_n`:**<br>• The bake needs the set's `pbr_normal` to be decided: `Ready`, or `Blocked` (for example by `FMT-BC5-NORMAL`; the bake reads the source, not the encoded file).<br>• An `Unresolved` `_n` (for example a green convention not yet stated) gives its members `SET-MISSING-COMPANION` with "not decided yet".<br>• With two `pbr_normal` jobs in one GUI-built set, the first by path is used | The `_n`'s decode settings must be final before they are baked in | None |
| R10 | **sRGB roughness:** a roughness member with `color.content = srgb` raises `TAG-CONTENT-MISMATCH` (warning) and does not bake | Roughness is linear data. sRGB content would decode it before squaring and baking | None |
| R11 | **No alpha weighting for vectors or roughness.** Normal, `_msn`, `_flow` and roughness chains are never alpha-weighted, even when a cutout policy is forced on them (Task 29 carry). Preprocess dilation under α = 0 still runs for alpha-tested policies; it only fills invisible texels | Weighting bends vectors, and the bake's lengths, toward opaque texels | None |
| R12 | **`if-changed`:** the baked `_n` is recorded in the manifest's `sources`. Its `SourceRef.bake` holds the SHA-256 of its decode settings (`channels`, `alpha.policy`, `mips.normal_base`) and the level shift. A changed `_n`, changed decode settings, or a bake turned on or off re-encodes the roughness member. `Profile` gains two fields, so every `profile_sha256` changes once and the first M4 run re-encodes every output, as for M2.5 (Note M10) | The output depends on the `_n`, so `if-changed` has to see it | One full re-encode |
| R13 | **Coverage fixes (mipcheck B1, B6), with the owner's dither decision (2026-10-11):**<br>• **B1:** every level ≥ 1 passes exactly `k = round(p·n)` texels: those above the k-th largest alpha, plus texels tied with it chosen in 8×8 Bayer order of position (deterministic, no RNG). Owner, verbatim: "I would probably encourage a dithering at lower mips, the whole point of coverage is to ensure it isn't lost, especially for foliage and grass, when your likely seeing mostly lower mip versions of them".<br>• "No texel passes" stays the target when `k = 0` (box alpha kept); zero alpha is never raised; the box alpha is also kept when no texel is positive. The "more than 2× off keeps the box alpha" fallback is not built.<br>• **B6:** remapped passing texels land at least 2 codes above the first passing code, failing ones at least 2 codes below the last failing code, the split's texels included | Exact coverage at the mips players mostly see; monotone whole-group remaps cannot split ties (up to ×32 overshoot measured) | Thin strands become a sparse ordered pattern at distance instead of thick or vanishing |
| R14 | **`sdc-base` gets a minor version bump** from whatever the packing-fills plan left (1.0.0 → 1.1.0, or 1.1.0 → 1.2.0). It gains `mips.normal_base`, `mips.bake_clamp` and the `coat_normal` self-bake default (spec §3.5: changing a built-in default bumps the version) | — | None |

## Review Focus

Most likely first. Each item's test is in the task named.

1. **Size ratios.** These pairs bake with the right level shift:
   - 2:1 (`_n` 128, roughness 64);
   - 1:2 (`_n` 32, roughness 64);
   - non-square pairs with the same aspect (`_n` 128×64, roughness 64×32).

   These raise `SET-BAKE-RATIO` and write the roughness unbaked, never with a wrong footprint:
   - an aspect mismatch (64×64 with 64×32);
   - an NPOT pair accepted with `--allow DIM-NPOT`.

   Tests: Tasks 5, 8 and 9.
2. **The companion is not where the bake expects it:**
   - The `_n` or the roughness member is a DDS (skipped as input).
   - It is on disk but not in the batch, because only one file was converted.
   - It sits in another folder but was moved into the set in the GUI. The bake reads the job's real path.
   - It is missing entirely.

   The converted file always gets the code that explains it: `SET-COMPANION-DDS`, `SET-COMPANION-OUTSIDE` or `SET-MISSING-COMPANION`. It is never silently unbaked. Tests: Task 8 (folder-crossing members: Task 8's `a_member_from_another_folder_bakes_from_its_own_path`).
3. **What the bake reads:**
   - a BC5 normal (blocked until allowed): the bake reads the source;
   - an alpha-tested normal (forced cutout): no alpha weighting;
   - a uniformly short source normal (|n| = 0.5, or down to 0.34): it adds no roughness under the default, because the bake reads mip 0 as written (unit length). It adds roughness under `keep`;
   - an sRGB mistake on the roughness member: `TAG-CONTENT-MISMATCH`, no bake;
   - an `_n` whose green convention changes after a run: `if-changed` re-encodes the roughness member.

   Tests: Tasks 3, 4, 7 and 8.
4. **Memory at 8K/16K.** Each baked `_n` adds 24 bytes per `_n` texel to the roughness member's admission against `--mem-budget`. The `_n` image is dropped before the member's image loads. Only the 1.33 B/texel length pyramid stays. Test: Task 7.
5. **`_rfaos` and `_rmaos` in one set, packed or pre-packed.** Each bakes from the same `_n`, and a packed member bakes like a pre-packed one. Tests: Tasks 8 and 9.

## Interface registry (names every task must use)

**`crates/engine/src/coverage.rs`** (Tasks 1–2):
- `pub const EDGE_MARGIN_CODES: u8 = 2;`
- private helpers: `bayer8(x: u32, y: u32) -> u32`, `exact_pass_set(w: &[f32], width: u32, k: usize) -> Option<Vec<bool>>`, `remap(level: &mut Rgba32F, w: &[f32], pass: &[bool], t_pass: f32, t_fail: f32)` (Task 1); `first_float_with_code(code: u8) -> f32`, `first_passing_code(threshold: u8, compare: AlphaCompare) -> Option<u8>` (Task 2). `nearest_group` and `first_passing` are deleted.
- `preserve_coverage(chain: &mut [Rgba32F], threshold: u8, compare: AlphaCompare)`: the signature is unchanged.

**`crates/engine/src/config.rs`** (Task 3):
- `cfg_enum!(NormalBase, "lowercase" { Renormalize, Keep })`.
- `MipSpec` gains two fields:
  - `normal_base: NormalBase` (serde default `Renormalize`);
  - `bake_clamp: f64` (serde default `0.25`).
- Field paths `"mips.normal_base"` and `"mips.bake_clamp"` are added to `overrides::FIELD_PATHS`.

**`crates/engine/src/presets.rs`** (Task 3): `Provenance::Set`, which serialises as `"set"`.

**`crates/engine/src/codes.rs`** (Task 4): `Code::ChanNormalShort = "CHAN-NORMAL-SHORT" => Info`. This makes 60 codes.

**`crates/engine/src/analyse.rs`** (Task 4): `Analysis::normal_len_mean: f64`, the mean length of RGB decoded as `*2 − 1`.

**`crates/engine/src/mips.rs`** (Tasks 4–6):
- `pub fn build_chain(work: &Work, p: &Profile) -> Vec<Rgba32F>`: the signature is unchanged; it calls `build_chain_with(work, p, None)`.
- `pub fn build_chain_with(work: &Work, p: &Profile, bake: Option<&crate::bake::NormalVariance>) -> Vec<Rgba32F>`.
- `pub(crate) fn box_down(prev: &Rgba32F) -> Rgba32F`: an unweighted box step.
- `pub(crate) fn normalize_vector(px: &mut [f32; 4], domain: Domain)`: Task 4 makes it `pub(crate)` so `BakeJob::load_lengths` (Task 7) writes mip 0 exactly as the `_n`'s own chain does.

**`crates/engine/src/bake/mod.rs`** (Tasks 5, 7):
- `pub const UNIT_LENGTH: f64 = 1.0 - 1e-6;`
- `pub fn variance_add(len: f32, clamp_t: f64) -> f32`: returns `min(2v, t²)`.
- `pub fn baked_roughness(mean_alpha: f32, add: f32) -> f32`: returns `(min(α² + add, 1))^¼`, or `√α` when `add <= 0`.
- `pub fn bake_shift(normal: (u32, u32), member: (u32, u32)) -> Result<i32, String>`: log2 of the `_n` : member ratio.
- `pub struct LengthLevel { pub width: u32, pub height: u32, pub len: Vec<f32> }`
- `pub struct LengthPyramid { pub base: (u32, u32), pub levels: Vec<LengthLevel> }`, with:
  - `LengthPyramid::build(base: &Rgba32F) -> LengthPyramid`;
  - `LengthPyramid::level(&self, m: usize) -> Option<&LengthLevel>`, for `m ≥ 1`.
- `pub struct NormalVariance` (private fields), with:
  - `NormalVariance::new(lengths: LengthPyramid, member: (u32, u32), clamp_t: f64) -> Result<NormalVariance, String>`;
  - `shift(&self) -> i32`;
  - `add_at(&self, level: usize, x: u32, y: u32) -> f32`.
- Task 7: `pub struct BakeJob { pub normal: PathBuf, pub normal_profile: Profile, pub shift: i32, pub stamp: Option<SourceStamp> }` (`Serialize`, `Deserialize`, `JsonSchema`), with:
  - `identity_sha256(&self) -> String`;
  - `load_lengths(&self) -> Result<LengthPyramid, String>`;
  - `estimated_bytes(&self) -> u64`.

**`crates/engine/src/bake/plan.rs`** (Task 8):
- `pub struct BakeCtx<'a> { pub target: Target, pub sets: &'a [TextureSet], pub files: &'a [InputFile], pub aliases: &'a BTreeMap<String, String>, pub shared: &'a BTreeSet<String> }`. Here `shared` holds set **keys**.
- `pub fn plan_bakes(cx: &BakeCtx, jobs: &mut [ResolvedJob]) -> BTreeSet<String>`: returns the keys of the sets that bake.

**`crates/engine/src/policy.rs`** (Task 7): `ResolvedJob::bake: Option<crate::bake::BakeJob>`, with `#[serde(default, skip_serializing_if = "Option::is_none")]`.

**`crates/engine/src/manifest.rs`** (Task 7):
- `SourceRef::bake: Option<String>`, with `#[serde(default, skip_serializing_if = "Option::is_none")]`;
- `SourceRef::of_bake(path: &Path, identity: String) -> io::Result<SourceRef>`.

**`crates/engine/src/api.rs`** (Task 8): `BatchRequest::shared_roughness: BTreeSet<String>` (set **ids**). `BatchRequest::new` leaves it empty.

**GUI** (Task 10):
- `sdc_gui::model::sets::shared_roughness(m: &AppModel) -> BTreeSet<String>`;
- `FIELDS` gains `("mips.normal_base", "Normal base level")`.

## File structure

| File | Status | Responsibility | Tasks |
|---|---|---|---|
| `crates/engine/src/coverage.rs` | modify | B1 exact coverage by Bayer tie-split, B6 edge margin | 1, 2 |
| `crates/engine/src/config.rs` | modify | `NormalBase`, `MipSpec::{normal_base, bake_clamp}` | 3 |
| `presets/sdc-base.toml` | modify | New defaults; `coat_normal` self-bake; version 1.1.0 | 3, 6 |
| `crates/engine/src/overrides.rs` | modify | `FIELD_PATHS` | 3 |
| `crates/engine/src/presets.rs` | modify | `Provenance::Set` | 3 |
| `crates/engine/src/policy.rs` | modify | Clamp check, sRGB roughness, `CHAN-NORMAL-SHORT`, `ResolvedJob::bake` | 3, 4, 7 |
| `crates/engine/src/analyse.rs` | modify | `normal_len_mean` | 4 |
| `crates/engine/src/codes.rs` | modify | `CHAN-NORMAL-SHORT` | 4 |
| `crates/engine/src/mips.rs` | modify | Mip-0 renormalise, no vector weighting, `build_chain_with`, `_cnr` self-bake | 4, 5, 6 |
| `crates/engine/src/bake/mod.rs` | create | Formula, pyramid, ratio, `NormalVariance`, `BakeJob` | 5, 7 |
| `crates/engine/src/bake/plan.rs` | create | Set-level decisions and `SET-*` codes | 8 |
| `crates/engine/src/lib.rs` | modify | `pub mod bake;` | 5 |
| `crates/engine/src/pipeline.rs` | modify | Stamp check, companion load, `build_chain_with`, manifest sources | 7 |
| `crates/engine/src/manifest.rs` | modify | `SourceRef::bake`, `of_bake` | 7 |
| `crates/engine/src/api.rs` | modify | `current_sources`, admission bytes, `shared_roughness`, `plan_bakes` wiring, `set_resolved.bake` | 7, 8 |
| `crates/engine/src/pack/plan.rs` | modify | `bake: None` in its `ResolvedJob` literal | 7 |
| `crates/engine/tests/bake_config.rs` | create | Task 3 tests | 3 |
| `crates/engine/tests/bake_run.rs` | create | Task 7 tests | 7 |
| `crates/engine/tests/bake_plan.rs` | create | Task 8 tests | 8 |
| `crates/engine/tests/golden.rs` | modify | Named expectation updates; new bake cases | 8, 9 |
| `crates/engine/tests/{events_manifest,pack_run}.rs` | modify | `bake: None` in struct literals | 7 |
| `schema/*.json` | regenerate | Profile, code, `ResolvedJob`, `SourceRef` | 3, 4, 7 |
| `crates/gui/src/model/{sets,mod,fields}.rs`, `crates/gui/src/ui/settings_panel.rs` | modify | Shared roughness; the `normal_base` row; provenance `set` | 10 |
| `crates/gui/tests/{sets_model,fields}.rs` | modify | GUI tests | 10 |
| `docs/DECISIONS.md`, the spec, the phase plan, `docs/gui.md` | modify | Note R and the doc updates | 11 |

Tasks run in order. Tasks 1–2 depend on nothing. Task 4 needs Task 3. Task 5 needs Task 4. Task 6 needs Task 5. Task 7 needs Task 5. Task 8 needs Tasks 3 and 7. Task 9 needs Tasks 6 and 8. Task 10 needs Task 8. Task 11 comes last.

---

### Task 1: Coverage reaches the target exactly: ties are split in Bayer order (mipcheck B1, owner)

**Files:**
- Modify: `crates/engine/src/coverage.rs`:
  - module doc ("Ties" and "Remap shape" bullets);
  - `preserve_coverage` and its doc;
  - delete `nearest_group`;
  - new `bayer8`, `exact_pass_set` and `remap`;
  - unit tests.
- Test: `crates/engine/src/coverage.rs` (`mod tests`)

**Interfaces:**
- Consumes: `first_passing`, `passes`, `code8`, `unit_alpha`, `coverage_fraction` (all existing).
- Produces private helpers:
  - `bayer8(x: u32, y: u32) -> u32`;
  - `exact_pass_set(w: &[f32], width: u32, k: usize) -> Option<Vec<bool>>`;
  - `remap(level: &mut Rgba32F, w: &[f32], pass: &[bool], t_pass: f32, t_fail: f32)`.
- Task 2 changes only how `t_pass` and `t_fail` are computed.
- `preserve_coverage` keeps its signature.

**Owner decision (2026-10-11), verbatim:** "I would probably encourage a dithering at lower mips, the whole point of coverage is to ensure it isn't lost, especially for foliage and grass, when your likely seeing mostly lower mip versions of them".

**Background (RESULTS-2 B1):**
- A monotone remap can only pass whole groups of tied alpha. On one-pixel strands every texel touching a strand has the same alpha, so the reachable counts jump. The measured overshoot was up to ×32 (mip 5: k = 20, 655 passing).

**Rules (DECISIONS R13):**
- **The target** for each level ≥ 1 is `k = round(p · n)`, where `p` is mip 0's pass fraction.
- **`k = 0`:** the level keeps its box alpha. This is the "no texel passes" candidate, kept as the spec has it.
- **Already right:** when the level already passes exactly `k`, it is left unchanged.
- **Exact pass set.** Otherwise:
  - take `v`, the `k`-th largest alpha;
  - every texel above `v` passes;
  - among the texels tied at `v`, exactly as many pass as are still needed;
  - those are chosen in **8×8 Bayer order** of their (x, y) position, ties by texel index;
  - this is deterministic, with no RNG.
- **Result:** exactly `k` texels pass at every level, including the low mips where tied groups are the rule.
- **Zero alpha is never raised.** With fewer than `k` positive texels, every positive texel passes (the most a remap can reach without inventing coverage). With none, the level keeps its box alpha.
- **The remap:**
  - Passing texels map monotonically onto `[t_pass, 1]`. The smallest passing alpha goes to `t_pass`, and 1.0 stays 1.0.
  - Failing texels map onto `[0, t_fail]`. The largest failing alpha goes to `t_fail` exactly.
  - Within each side the order is kept; only the tied group is split.
  - A 1.0 texel that the split leaves out fails at `t_fail`. Under box filtering this needs a level whose count of 1.0 texels exceeds `k`, which only rounding can cause.
- **The "more than 2× off keeps the box alpha" fallback is not built.** The dither replaces it, and the box alpha is kept only where the dither cannot apply (`k = 0`, or no positive texel).

**Unit tests that change (named):**
- `binary_tie_takes_the_only_achievable_count` becomes `binary_tie_lets_exactly_k_through_in_bayer_order`.
- `tie_picks_the_nearer_achievable_group` becomes `ties_are_split_in_bayer_order_to_reach_k_exactly`.
- `opaque_anchor_with_lower_passing_group_demotes_the_lower_group` is unchanged and still passes: the largest failing alpha lands on `t_fail` exactly.
- No golden expectation changes. The golden cutout fixtures have continuous alpha, and their counts move from "within one texel of `k`" to exactly `k`.

- [ ] **Step 1: Write the failing tests.** In `crates/engine/src/coverage.rs` `mod tests`, delete `binary_tie_takes_the_only_achievable_count` and `tie_picks_the_nearer_achievable_group`, and add:

```rust
    #[test]
    fn bayer8_is_the_standard_8x8_ordered_dither() {
        let mut seen = [false; 64];
        for y in 0..8 {
            for x in 0..8 {
                seen[bayer8(x, y) as usize] = true;
                assert_eq!(bayer8(x, y), bayer8(x + 8, y + 16), "the matrix tiles");
            }
        }
        assert!(seen.iter().all(|&s| s), "a permutation of 0..64");
        let row0: Vec<u32> = (0..8).map(|x| bayer8(x, 0)).collect();
        assert_eq!(row0, [0, 32, 8, 40, 2, 34, 10, 42]);
        assert_eq!(bayer8(0, 1), 48);
    }

    #[test]
    fn binary_tie_lets_exactly_k_through_in_bayer_order() {
        // One opaque texel per 2x2 footprint: level 1 is four tied 0.25s and k = 1. The old solver passed all four;
        // now exactly one passes, the texel with the lowest Bayer rank, (0, 0).
        let mut base = Rgba32F::new(4, 4, [0.0; 4]);
        for (x, y) in [(0, 0), (2, 0), (0, 2), (2, 2)] {
            base.at_mut(x, y)[3] = 1.0;
        }
        let mut c = box_chain(base);
        assert!(alpha_of(&c[1]).all(|a| a == 0.25));
        preserve_coverage(&mut c, 127, AlphaCompare::Gt);
        assert_eq!(pass_count(&c[1], 127, AlphaCompare::Gt), 1);
        assert!(passes(code8(c[1].px[0][3]), 127, AlphaCompare::Gt));
    }

    #[test]
    fn mipcheck_b1_three_texel_repro_gets_exactly_k() {
        // RESULTS-2 B1, smallest reproduction: alpha 255 at (0,0), (0,2), (2,0). Level 1 holds three tied 0.25s and
        // k = round(3/16 * 4) = 1; the old solver passed all three.
        let mut base = Rgba32F::new(4, 4, [0.0; 4]);
        for (x, y) in [(0, 0), (0, 2), (2, 0)] {
            base.at_mut(x, y)[3] = 1.0;
        }
        let mut c = box_chain(base);
        preserve_coverage(&mut c, 128, AlphaCompare::Ge);
        assert_eq!(pass_count(&c[1], 128, AlphaCompare::Ge), 1);
    }

    #[test]
    fn ties_are_split_in_bayer_order_to_reach_k_exactly() {
        // Footprints with 3, 1, 1, 1 opaque texels: level 1 = [0.75, 0.25, 0.25, 0.25], k = round(1.5) = 2. The 0.75
        // passes, and one of the three tied 0.25s: (1, 1) has the lowest Bayer rank of (1,0), (0,1), (1,1).
        let mut base = Rgba32F::new(4, 4, [0.0; 4]);
        for (x, y) in [(0, 0), (1, 0), (0, 1), (2, 0), (0, 2), (2, 2)] {
            base.at_mut(x, y)[3] = 1.0;
        }
        let mut c = box_chain(base);
        preserve_coverage(&mut c, 127, AlphaCompare::Gt);
        assert_eq!(pass_count(&c[1], 127, AlphaCompare::Gt), 2);
        let pass: Vec<bool> = alpha_of(&c[1])
            .map(|a| passes(code8(a), 127, AlphaCompare::Gt))
            .collect();
        assert_eq!(pass, [true, false, false, true]);
    }

    /// 64x64 with a one-texel vertical strand every 16 columns (p = 1/16): every level's alphas are tied.
    fn strands() -> Vec<Rgba32F> {
        let mut base = Rgba32F::new(64, 64, [0.0; 4]);
        for y in 0..64 {
            for x in (0..64).step_by(16) {
                base.at_mut(x, y)[3] = 1.0;
            }
        }
        box_chain(base)
    }

    #[test]
    fn thin_tied_strands_keep_coverage_within_one_texel_down_to_4x4() {
        let mut c = strands();
        preserve_coverage(&mut c, 127, AlphaCompare::Gt);
        let p = coverage_fraction(alpha_of(&c[0]), 127, AlphaCompare::Gt);
        for l in c.iter().skip(1).filter(|l| l.px.len() >= 16) {
            let k = p * l.px.len() as f64;
            let got = pass_count(l, 127, AlphaCompare::Gt) as f64;
            assert!(
                (got - k).abs() <= 1.0,
                "{}x{}: {got} passing for k = {k}",
                l.width,
                l.height
            );
        }
    }

    #[test]
    fn the_dither_is_deterministic() {
        let (mut a, mut b) = (strands(), strands());
        preserve_coverage(&mut a, 127, AlphaCompare::Gt);
        preserve_coverage(&mut b, 127, AlphaCompare::Gt);
        assert_eq!(a, b);
        let (mut a, mut b) = (foliage_chain(), foliage_chain());
        preserve_coverage(&mut a, 64, AlphaCompare::Ge);
        preserve_coverage(&mut b, 64, AlphaCompare::Ge);
        assert_eq!(a, b);
    }

    #[test]
    fn foliage_every_level_reaches_k_exactly() {
        let mut c = foliage_chain();
        preserve_coverage(&mut c, 127, AlphaCompare::Gt);
        let p = coverage_fraction(alpha_of(&c[0]), 127, AlphaCompare::Gt);
        for l in &c[1..] {
            let k = ((p * l.px.len() as f64).round() as usize).min(l.px.len());
            if k > 0 {
                assert_eq!(pass_count(l, 127, AlphaCompare::Gt), k, "{}x{}", l.width, l.height);
            }
        }
    }
```

- [ ] **Step 2: Run the tests to see them fail.**

Run: `cargo test -p engine --lib coverage::tests`
Expected:
- FAIL to compile (`bayer8` does not exist).
- With `bayer8` stubbed, these fail: `binary_tie_lets_exactly_k_through_in_bayer_order` (4 pass), `mipcheck_b1_three_texel_repro_gets_exactly_k` (3 pass), `ties_are_split_in_bayer_order_to_reach_k_exactly` (1 pass) and `thin_tied_strands_keep_coverage_within_one_texel_down_to_4x4` (the 16×16 level passes 64 for k = 16).

- [ ] **Step 3: Implement.** In `crates/engine/src/coverage.rs`:

1. Replace the module doc's "**Ties.**" and "**Remap shape**" bullets with:

```rust
//! - **Ties, dithered (owner 2026-10-11, DECISIONS R13).** On thin strands every texel touching
//!   a strand has the same alpha, so whole tied groups cannot reach the target (mipcheck round
//!   2, B1: up to 32x over). Each level instead passes exactly `k = round(p · n)` texels: those
//!   above the k-th largest alpha `v`, plus as many texels tied at `v` as are still needed,
//!   chosen in 8x8 Bayer order of their position. That is deterministic, with no RNG, and spreads
//!   the passing texels along the strand, so foliage and grass keep their coverage at the low
//!   mips players mostly see. Zero alpha is never raised.
//! - **Remap shape (deviation from Castaño).** Castaño scales alpha uniformly by `T/a`, which
//!   halves opaque interiors and breaks `cutout_blend`. Here passing texels map monotonically
//!   onto `[T_pass, 1]` (1.0 stays 1.0) and failing ones onto `[0, T_fail]`, each side keeping
//!   the order of its alphas; only the tied boundary group is split (controller ruling, owner
//!   check; spec §5.5 note).
```

2. Delete `nearest_group`.

3. Add:

```rust
/// Rank of texel (x, y) in the 8x8 ordered-dither (Bayer) matrix, 0..64: the order in which texels tied at the
/// coverage boundary are let through (owner 2026-10-11, DECISIONS R13). The bit-reversed interleave of `x ^ y` and
/// `y`; deterministic, no RNG.
fn bayer8(x: u32, y: u32) -> u32 {
    let (a, b) = ((x ^ y) & 7, y & 7);
    ((a & 1) << 5) | ((b & 1) << 4) | ((a & 2) << 2) | ((b & 2) << 1) | ((a & 4) >> 1) | ((b & 4) >> 2)
}

/// Which texels pass so that exactly `k` do: every texel above the k-th largest alpha `v`, plus as many texels tied
/// at `v` as are still needed, in Bayer order of their position (ties by index). Zero alpha is never raised, so with
/// fewer than `k` positive texels every positive one passes; `None` when none is positive. `w` is the level's alpha
/// as [`unit_alpha`] reads it, row-major with `width` texels per row.
fn exact_pass_set(w: &[f32], width: u32, k: usize) -> Option<Vec<bool>> {
    let positive = w.iter().filter(|&&a| a > 0.0).count();
    let k = k.min(positive);
    if k == 0 {
        return None;
    }
    let mut scratch = w.to_vec();
    let (_, &mut v, _) = scratch.select_nth_unstable_by(k - 1, |x, y| y.total_cmp(x));
    let mut pass: Vec<bool> = w.iter().map(|&a| a > v).collect();
    let above = pass.iter().filter(|&&p| p).count();
    let mut tied: Vec<usize> = (0..w.len()).filter(|&i| w[i] == v).collect();
    tied.sort_by_key(|&i| (bayer8(i as u32 % width, i as u32 / width), i));
    for &i in tied.iter().take(k - above) {
        pass[i] = true;
    }
    Some(pass)
}

/// Rewrite alpha so exactly the texels in `pass` pass. Passing texels map monotonically onto `[t_pass, 1]` (the
/// smallest goes to `t_pass`, 1.0 stays 1.0); failing ones onto `[0, t_fail]` (the largest goes to `t_fail`). Each
/// side keeps the order of its alphas.
fn remap(level: &mut Rgba32F, w: &[f32], pass: &[bool], t_pass: f32, t_fail: f32) {
    let lo_pass = w.iter().zip(pass).filter(|(_, p)| **p).map(|(a, _)| *a).fold(1.0f32, f32::min);
    let hi_fail = w.iter().zip(pass).filter(|(_, p)| !**p).map(|(a, _)| *a).fold(0.0f32, f32::max);
    for (q, (&a, &p)) in level.px.iter_mut().zip(w.iter().zip(pass)) {
        q[3] = if p {
            if a >= 1.0 {
                1.0
            } else {
                // a >= lo_pass and a < 1, so lo_pass < 1; the result is >= t_pass, and a = lo_pass gives t_pass.
                (t_pass + (a - lo_pass) * ((1.0 - t_pass) / (1.0 - lo_pass))).min(1.0)
            }
        } else if a <= 0.0 {
            0.0 // zero alpha stays zero (invisible texels are never lifted toward the threshold)
        } else if a >= hi_fail {
            t_fail // the largest failing alpha (an unchosen tied texel included) lands exactly on t_fail
        } else {
            // 0 < a < hi_fail here; capped so f32 rounding cannot lift it past t_fail.
            (a * (t_fail / hi_fail)).min(t_fail)
        };
    }
}
```

4. Replace the whole body of `preserve_coverage` after `let Some((base, rest)) = chain.split_first_mut() else { return; };` with:

```rust
    // No passing code (`gt 255`): p = 0. Every code passes (`ge 0`): every level is full.
    let Some(t) = first_passing(threshold, compare) else {
        return;
    };
    if t == 0.0 {
        return;
    }
    // Where remapped texels land: passing ones at or above `t_pass`, failing ones at or below `t_fail` (Task 2 moves
    // both clear of the threshold).
    let (t_pass, t_fail) = (t, t.next_down());
    let p = coverage_fraction(base.px.iter().map(|q| q[3]), threshold, compare);
    let pass_count = |l: &Rgba32F| {
        l.px.iter()
            .filter(|q| passes(code8(q[3]), threshold, compare))
            .count()
    };
    for level in rest {
        let n = level.px.len();
        let k = ((p * n as f64).round() as usize).min(n);
        // "No texel passes" is the target: the box alpha is left alone (spec §5.5). So is a level already on target.
        if k == 0 || pass_count(level) == k {
            continue;
        }
        let w: Vec<f32> = level.px.iter().map(|q| unit_alpha(q[3])).collect();
        // `None`: no positive alpha to let through, so the dither cannot apply and the box alpha stays.
        let Some(pass) = exact_pass_set(&w, level.width, k) else {
            continue;
        };
        remap(level, &w, &pass, t_pass, t_fail);
        debug_assert_eq!(pass_count(level), pass.iter().filter(|&&b| b).count());
    }
```

5. Replace the `preserve_coverage` doc comment with:

```rust
/// Rewrite the alpha of every level `>= 1` so it passes exactly `k = round(p · n)` texels, `p` the pass fraction of
/// level 0 (the reference, left untouched): the texels above the k-th largest alpha, plus texels tied with it in
/// Bayer order of position (owner 2026-10-11, DECISIONS R13). Passing texels are remapped onto `[T_pass, 1]` and
/// failing ones onto `[0, T_fail]`. A level is left unchanged when `k = 0`, when it already passes `k`, or when it has
/// no positive alpha; zero alpha is never raised, so a level with fewer than `k` positive texels passes all of them.
```

- [ ] **Step 4: Run the tests to see them pass.**

Run: `cargo test -p engine --lib coverage::tests`
Expected: PASS, every test in the module, including the existing `foliage_*`, `near_boundary_*`, `opaque_*`, `boundary_*`, `zero_target_*`, `degenerate_*` and `mostly_empty_*` tests.

Run: `cargo test -p engine --test golden cutout`
Expected: PASS.

- [ ] **Step 5: Run every gate and commit.**

```bash
cargo fmt --all && cargo clippy --workspace --all-targets -- -D warnings && cargo test --workspace
git add crates/engine/src/coverage.rs
git commit -m "fix(engine): coverage passes exactly k texels per level, splitting tied alpha in Bayer order (mipcheck B1, owner)

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

---

### Task 2: Remapped alpha keeps two codes clear of the threshold (mipcheck B6)

**Files:**
- Modify: `crates/engine/src/coverage.rs` (module doc "Boundary" bullet; replace `first_passing`; how `preserve_coverage` computes `t_pass` and `t_fail`; unit tests)
- Test: `crates/engine/src/coverage.rs` (`mod tests`)

**Interfaces:**
- Consumes: `exact_pass_set`, `remap` and the `t_pass`/`t_fail` pair of `preserve_coverage` (Task 1).
- Produces: `pub const EDGE_MARGIN_CODES: u8 = 2;` (used by the tests), plus the private `first_float_with_code(code: u8) -> f32` and `first_passing_code(threshold: u8, compare: AlphaCompare) -> Option<u8>`. `first_passing` is deleted.

**Background (RESULTS-2 B1/B6):**
- The remap puts the anchor exactly on the first passing code (128 for `ge 128`). BC7 then moves it to 127.
- Measured losses:
  - the sparse image at 8² goes from 35 passing texels to 0;
  - Fence007A at 16² goes from 94 to 78;
  - Fence007A at 4² goes from 6 to 0.
- Fix: passing texels land at least `EDGE_MARGIN_CODES` above the first passing code, and failing ones at least that far below the last failing code. Pass counts do not change; only the distance from the edge does.

**Unit tests that change (named):**
- `boundary_texel_lands_on_the_first_passing_float` is replaced by `boundary_texel_lands_two_codes_above_the_threshold`.
- `opaque_anchor_with_lower_passing_group_demotes_the_lower_group` now expects the demoted group at the margin float.
- The test helper `first_passing_float` is replaced by `smallest_float_with_code` and `first_code`.
- No golden expectation changes. Golden decoded coverage can only move closer to the base.

- [ ] **Step 1: Write the failing tests.** In `mod tests`:
  - delete `first_passing_float` and `boundary_texel_lands_on_the_first_passing_float`;
  - add the helpers and the tests below;
  - in `opaque_anchor_with_lower_passing_group_demotes_the_lower_group`, replace
    `let t_fail = first_passing_float(127, compare).next_down();`
    with
    `let t_fail = smallest_float_with_code(first_code(127, compare) - EDGE_MARGIN_CODES).next_down();`.

```rust
    /// Test-local walk: the smallest f32 in [0, 1] whose 8-bit code is at least `c`.
    fn smallest_float_with_code(c: u8) -> f32 {
        let mut x = ((f32::from(c) - 0.5) / 255.0).max(0.0);
        while x > 0.0 && code8(x) >= c {
            x = x.next_down();
        }
        while code8(x) < c {
            x = x.next_up();
        }
        x
    }

    /// The first 8-bit code that passes.
    fn first_code(threshold: u8, compare: AlphaCompare) -> u8 {
        (0..=255u8)
            .find(|&c| passes(c, threshold, compare))
            .unwrap()
    }

    #[test]
    fn boundary_texel_lands_two_codes_above_the_threshold() {
        for compare in [AlphaCompare::Gt, AlphaCompare::Ge] {
            let first = first_code(127, compare);
            // p = 4/16, so k = 1 at level 1: only the 0.3 texel passes.
            let mut base = Rgba32F::new(4, 4, [0.0; 4]);
            for x in 0..4 {
                base.at_mut(x, 0)[3] = 1.0;
            }
            let mut lvl1 = Rgba32F::new(2, 2, [0.0; 4]);
            lvl1.px[0][3] = 0.3;
            lvl1.px[1][3] = 0.2997;
            lvl1.px[2][3] = 0.1;
            let mut c = vec![base, lvl1];
            preserve_coverage(&mut c, 127, compare);
            let t_pass = smallest_float_with_code(first + EDGE_MARGIN_CODES);
            assert_eq!(c[1].px[0][3], t_pass, "{compare:?}");
            assert_eq!(code8(c[1].px[0][3]), first + EDGE_MARGIN_CODES);
            // The runner-up sits at least two codes under the last failing code (first - 1).
            assert!(
                code8(c[1].px[1][3]) <= first - 1 - EDGE_MARGIN_CODES,
                "{compare:?}: {}",
                code8(c[1].px[1][3])
            );
            assert_eq!(pass_count(&c[1], 127, compare), 1);
        }
    }

    #[test]
    fn remapped_levels_keep_a_margin_around_the_threshold() {
        // gt 127: the first passing code is 128 and the last failing one 127, so a remapped level holds no code in
        // 126..=129.
        let boxed = foliage_chain();
        let mut c = boxed.clone();
        preserve_coverage(&mut c, 127, AlphaCompare::Gt);
        let mut remapped = 0;
        for (l, b) in c.iter().zip(&boxed).skip(1) {
            if l == b {
                continue;
            }
            remapped += 1;
            for a in alpha_of(l) {
                let code = code8(a);
                assert!(
                    code <= 125 || code >= 130,
                    "{}x{}: code {code} inside the margin",
                    l.width,
                    l.height
                );
            }
        }
        assert!(remapped > 0, "the fixture must exercise the remap");
    }

    #[test]
    fn the_margin_holds_after_the_bayer_split() {
        // Thin tied strands (Task 1): the texels the split lets through sit two codes above the threshold, the tied
        // texels it holds back two codes below, and the count stays exact.
        let boxed = strands();
        let mut c = boxed.clone();
        preserve_coverage(&mut c, 127, AlphaCompare::Gt);
        let p = coverage_fraction(alpha_of(&c[0]), 127, AlphaCompare::Gt);
        let mut split = 0;
        for (l, b) in c.iter().zip(&boxed).skip(1) {
            if l == b {
                continue;
            }
            split += 1;
            let k = ((p * l.px.len() as f64).round() as usize).min(l.px.len());
            assert_eq!(pass_count(l, 127, AlphaCompare::Gt), k, "{}x{}", l.width, l.height);
            for a in alpha_of(l) {
                let code = code8(a);
                assert!(code <= 125 || code >= 130, "{}x{}: code {code}", l.width, l.height);
            }
        }
        assert!(split > 0, "the fixture must exercise the split");
    }
```

- [ ] **Step 2: Run the tests to see them fail.**

Run: `cargo test -p engine --lib coverage::tests`
Expected: FAIL. `EDGE_MARGIN_CODES` does not exist yet (compile error).

- [ ] **Step 3: Implement.** In `crates/engine/src/coverage.rs`:

1. Replace the module doc's "**Boundary.**" bullet with:

```rust
//! - **Boundary.** Remapped passing texels land on the smallest float whose code is
//!   [`EDGE_MARGIN_CODES`] above the first passing code, and remapped failing texels at or below
//!   the largest float that many codes under the last failing code (found by search, not code
//!   centres), so BC7's one- or two-code error does not flip them (mipcheck round 2, B6). This
//!   holds for the texels the Bayer split lets through or holds back too.
```

2. Replace the whole `first_passing` function with:

```rust
/// mipcheck round 2, B6: BC7 moves alpha by a code or two, so a remapped texel left exactly on the first passing
/// code can fall below the threshold after encoding (Fence007A at 16²: 94 → 78 passing texels). Remapped texels
/// are placed this many codes clear of the threshold on each side (DECISIONS R13).
pub const EDGE_MARGIN_CODES: u8 = 2;

/// The smallest `x` in `[0, 1]` whose 8-bit code is at least `code`. `code8` is monotone and non-negative f32 bit
/// patterns order like their values, so a binary search over the bits finds the exact boundary.
fn first_float_with_code(code: u8) -> f32 {
    if code == 0 {
        return 0.0;
    }
    let ok = |bits: u32| code8(f32::from_bits(bits)) >= code;
    // Invariant: `lo` fails, `hi` passes (every code is reached by 1.0).
    let (mut lo, mut hi) = (0u32, 1.0f32.to_bits());
    while hi - lo > 1 {
        let mid = lo + (hi - lo) / 2;
        if ok(mid) {
            hi = mid;
        } else {
            lo = mid;
        }
    }
    f32::from_bits(hi)
}

/// The first 8-bit code that passes, or `None` when none does (`gt 255`).
fn first_passing_code(threshold: u8, compare: AlphaCompare) -> Option<u8> {
    (0..=255u8).find(|&c| passes(c, threshold, compare))
}
```

3. In `preserve_coverage` (as Task 1 left it), replace

```rust
    // No passing code (`gt 255`): p = 0. Every code passes (`ge 0`): every level is full.
    let Some(t) = first_passing(threshold, compare) else {
        return;
    };
    if t == 0.0 {
        return;
    }
    // Where remapped texels land: passing ones at or above `t_pass`, failing ones at or below `t_fail` (Task 2 moves
    // both clear of the threshold).
    let (t_pass, t_fail) = (t, t.next_down());
```

with

```rust
    // No passing code (`gt 255`): p = 0. Every code passes (`ge 0`): every level is full.
    let Some(c) = first_passing_code(threshold, compare) else {
        return;
    };
    if c == 0 {
        return;
    }
    // Remapped texels that pass land on code `c + margin` or above, those that fail on `c - 1 - margin` or below
    // (each clamped to the code range), so BC7 error cannot flip them (mipcheck B6).
    let t_pass = first_float_with_code(c.saturating_add(EDGE_MARGIN_CODES));
    let t_fail = first_float_with_code(c.saturating_sub(EDGE_MARGIN_CODES).max(1)).next_down();
```

`remap` (Task 1) is unchanged: it already puts the smallest passing alpha on `t_pass` and the largest failing one on `t_fail`.

4. In the `preserve_coverage` doc comment, replace "Passing texels are remapped onto `[T_pass, 1]` and failing ones onto `[0, T_fail]`." with "Passing texels are remapped onto `[T_pass, 1]` and failing ones onto `[0, T_fail]`, `T_pass` and `T_fail` [`EDGE_MARGIN_CODES`] codes clear of the threshold (mipcheck B6)."

- [ ] **Step 4: Run the tests to see them pass.**

Run: `cargo test -p engine --lib coverage::tests`
Expected: PASS.

Run: `cargo test -p engine --test golden cutout` and `cargo test -p engine --test golden per_texture_threshold`
Expected: PASS.

- [ ] **Step 5: Run every gate and commit.**

```bash
cargo fmt --all && cargo clippy --workspace --all-targets -- -D warnings && cargo test --workspace
git add crates/engine/src/coverage.rs
git commit -m "fix(engine): remapped alpha keeps two codes clear of the threshold so BC7 cannot flip it (mipcheck B6)

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

---

### Task 3: Profile fields `mips.normal_base` and `mips.bake_clamp`, provenance `set`, and their checks

**Files:**
- Modify:
  - `crates/engine/src/config.rs` (`NormalBase`, `MipSpec`, unit tests);
  - `presets/sdc-base.toml` (version, `defaults.mips`);
  - `crates/engine/src/overrides.rs` (`FIELD_PATHS`);
  - `crates/engine/src/presets.rs` (`Provenance::Set`);
  - `crates/engine/src/policy.rs` (`check_profile`: clamp range, sRGB roughness).
- Regenerate: `schema/*.json`
- Test: `crates/engine/tests/bake_config.rs` (create)

**Interfaces:**
- Consumes: nothing new.
- Produces:
  - `NormalBase`;
  - `MipSpec::normal_base` and `MipSpec::bake_clamp`;
  - the field paths `mips.normal_base` and `mips.bake_clamp`;
  - `Provenance::Set` (`"set"`);
  - `CFG-INVALID` for a clamp outside (0, 1] on a roughness member or `coat_normal`;
  - `TAG-CONTENT-MISMATCH` for a roughness member with sRGB content (DECISIONS R10).

**Expectations that change:** none. Every profile gains two fields, so every `profile_sha256` changes, but no test pins a profile hash. `--on-existing if-changed` re-encodes every output once (R12).

- [ ] **Step 1: Write the failing tests.** Create `crates/engine/tests/bake_config.rs`:

```rust
//! M4 Task 3: the profile fields `mips.normal_base` and `mips.bake_clamp`, their checks, and provenance `set`.
use engine::api::*;
use engine::codes::Code;
use engine::config::*;
use engine::events::*;
use engine::overrides::parse_set;
use engine::policy::{AcceptSuggestions, JobStatus, Provenance};
use engine::presets::PresetRegistry;
use image::{ImageBuffer, Rgba};
use std::path::Path;
use std::sync::Arc;

fn plan(dir: &Path, sets: &[&str]) -> Plan {
    let mut req = BatchRequest::new(vec![dir.join("in")], dir.join("out"), Target::Cs);
    req.recursive = true;
    req.accept = AcceptSuggestions::High;
    req.overrides = sets.iter().map(|s| parse_set(s).unwrap()).collect();
    Engine::new()
        .unwrap()
        .plan(
            req,
            &Reporter::new(Arc::new(NullSink), "t"),
            &CancelToken::default(),
        )
        .unwrap()
}

fn rmaos(dir: &Path) {
    let p = dir.join("in/textures/pbr/iron_rmaos.png");
    std::fs::create_dir_all(p.parent().unwrap()).unwrap();
    ImageBuffer::from_fn(16, 16, |x, _| Rgba([(x * 16) as u8, 0, 200, 255]))
        .save(p)
        .unwrap();
}

#[test]
fn every_builtin_profile_renormalises_mip_0_and_clamps_at_a_quarter() {
    let r = PresetRegistry::builtin().unwrap();
    for t in [Target::Cs, Target::Cpm, Target::Vanilla] {
        for st in Subtype::ALL {
            let p = r.base_profile(t, st);
            assert_eq!(
                (p.mips.normal_base, p.mips.bake_clamp),
                (NormalBase::Renormalize, 0.25),
                "{t:?} {st:?}"
            );
        }
    }
}

#[test]
fn the_new_fields_are_settable_at_every_scope() {
    assert!(parse_set("mips.normal_base=keep").is_ok());
    assert!(parse_set("file:**/iron_n.png.mips.normal_base=keep").is_ok());
    assert!(parse_set("subtype:rmaos.mips.bake_clamp=0.5").is_ok());
    assert!(parse_set("mips.normal_base_typo=keep").is_err());
}

#[test]
fn a_bake_clamp_outside_zero_to_one_is_cfg_invalid_on_a_roughness_member() {
    for (v, bad) in [
        ("0.0", true),
        ("1.5", true),
        ("-0.1", true),
        ("1.0", false),
        ("0.25", false),
    ] {
        let d = tempfile::tempdir().unwrap();
        rmaos(d.path());
        let p = plan(
            d.path(),
            &[&format!("file:**/iron_rmaos.png.mips.bake_clamp={v}")],
        );
        let j = &p.jobs[0];
        let invalid = j.warnings.iter().any(|w| w.code == Code::CfgInvalid);
        assert_eq!(invalid, bad, "{v}: {:?}", j.warnings);
        if bad {
            assert!(
                matches!(&j.status, JobStatus::Error { codes } if codes.contains(&Code::CfgInvalid)),
                "{v}: {:?}",
                j.status
            );
        }
    }
}

#[test]
fn srgb_content_on_a_roughness_member_is_tag_content_mismatch() {
    let d = tempfile::tempdir().unwrap();
    rmaos(d.path());
    let p = plan(d.path(), &["file:**/iron_rmaos.png.color.content=srgb"]);
    let w = p.jobs[0]
        .warnings
        .iter()
        .find(|w| w.code == Code::TagContentMismatch)
        .expect("TAG-CONTENT-MISMATCH");
    assert!(w.message.contains("roughness"), "{}", w.message);
}

#[test]
fn provenance_set_round_trips() {
    assert_eq!(Provenance::Set.to_string(), "set");
    assert_eq!("set".parse::<Provenance>(), Ok(Provenance::Set));
    assert_eq!(
        serde_json::to_value(Provenance::Set).unwrap(),
        serde_json::json!("set")
    );
}
```

In `crates/engine/src/config.rs` `mod tests`, add `"normal_base"` and `"keep"` to the `needle` list of `schema_lists_enum_values`, and add:

```rust
    #[test]
    fn mip_spec_defaults_the_m4_fields() {
        let m: MipSpec = serde_json::from_str(
            r#"{"levels":"full","filter":"box","normal":"renormalize","coverage":"auto"}"#,
        )
        .unwrap();
        assert_eq!((m.normal_base, m.bake_clamp), (NormalBase::Renormalize, 0.25));
        assert_eq!(serde_json::to_string(&NormalBase::Keep).unwrap(), "\"keep\"");
    }
```

- [ ] **Step 2: Run the tests to see them fail.**

Run: `cargo test -p engine --test bake_config` and `cargo test -p engine --lib config::tests`
Expected: FAIL to compile. `NormalBase`, `normal_base`, `bake_clamp` and `Provenance::Set` do not exist yet.

- [ ] **Step 3: Implement.**

`crates/engine/src/config.rs`:
- Add `cfg_enum!(NormalBase, "lowercase" { Renormalize, Keep });` after the `NormalMips` line.
- In `MipSpec`, after `pub normal: NormalMips,`, add:

```rust
    /// Mip 0 of a normal map (spec §5.4, DECISIONS R1): `renormalize` (default) stores every base vector at unit
    /// length; `keep` stores the source as authored. Read only for the Normal category.
    #[serde(default = "default_normal_base")]
    pub normal_base: NormalBase,
    /// Clamp `t` of the variance bake (spec §6.2, DECISIONS Note E): the added variance is at most `t²`. 0.25;
    /// allowed (0, 1]. Read from the profile the bake writes into: a roughness member, or a `_cnr`.
    #[serde(default = "default_bake_clamp")]
    pub bake_clamp: f64,
```

- Below the `MipSpec` struct add:

```rust
fn default_normal_base() -> NormalBase {
    NormalBase::Renormalize
}

fn default_bake_clamp() -> f64 {
    0.25
}
```

`presets/sdc-base.toml`:
- bump `version` by one minor step from its merged value (R14);
- replace the `defaults` `mips` line with:

```toml
mips = { levels = "full", filter = "box", normal = "renormalize", normal_base = "renormalize", bake_clamp = 0.25, coverage = "auto" }
```

`crates/engine/src/overrides.rs`: in `FIELD_PATHS`, after `"mips.normal",`, add `"mips.normal_base",` and `"mips.bake_clamp",`.

`crates/engine/src/presets.rs`:
- Add the variant after `Recipe(String),`:

```rust
    /// Decided by the texture set, not by a layer: the variance bake's `mips.normal = renormalize_bake` on a
    /// `pbr_normal` (spec §5.2, §6.2, DECISIONS R6). Shown as `set`.
    Set,
```

- In `Display`, add `Provenance::Set => f.write_str("set"),`.
- In `FromStr`, add `"set" => Provenance::Set,` to the final `match`.
- In the doc comment above the enum, add `` `set` `` to the list of plain names (it falls under "the snake_case variant name for the rest", so only check the wording).

`crates/engine/src/policy.rs`, in `check_profile`:
- After the `packing.roughness_default` block, add:

```rust
    // Spec §6.2 clamp t (DECISIONS Note E, R2): read where a bake writes, a roughness member or a `_cnr`.
    if (st.is_roughness_member() || st == Subtype::CoatNormal)
        && (p.mips.bake_clamp.is_nan() || p.mips.bake_clamp <= 0.0 || p.mips.bake_clamp > 1.0)
    {
        add(
            CfgInvalid,
            format!("mips.bake_clamp {} outside (0, 1]", p.mips.bake_clamp),
        );
    }
```

- After the existing `TagContentMismatch` block, add:

```rust
    // DECISIONS R10: roughness is linear data. sRGB content would decode it before the mips and the variance bake.
    if st.is_roughness_member() && p.color.content == ColorContent::Srgb {
        add(
            TagContentMismatch,
            "roughness member with sRGB content: roughness is linear data, so it is not variance-baked".into(),
        );
    }
```

Regenerate the schemas: `cargo run -p sdc -- presets schema all --out schema`.

- [ ] **Step 4: Run the tests to see them pass.**

Run: `cargo test -p engine --test bake_config`, `cargo test -p engine --lib config::tests` and `cargo test -p engine --test schemas`
Expected: PASS.

- [ ] **Step 5: Run every gate and commit.**

```bash
cargo fmt --all && cargo clippy --workspace --all-targets -- -D warnings && cargo test --workspace
git add crates/engine/src/config.rs presets/sdc-base.toml crates/engine/src/overrides.rs crates/engine/src/presets.rs crates/engine/src/policy.rs crates/engine/tests/bake_config.rs schema
git commit -m "feat(engine): mips.normal_base and mips.bake_clamp, provenance set, sRGB roughness check (M4)

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

---

### Task 4: Normal mip 0 renormalised by default, no alpha weighting for vector or roughness chains, `CHAN-NORMAL-SHORT`

**Files:**
- Modify:
  - `crates/engine/src/mips.rs` (module doc, `normalize_vector`, `base_level`, `weighted_rgb`, `finalize`, `build_chain`, tests);
  - `crates/engine/src/analyse.rs` (`normal_len_mean`, test);
  - `crates/engine/src/codes.rs` (`CHAN-NORMAL-SHORT`, module doc, count test);
  - `crates/engine/src/policy.rs` (`check_profile`).
- Regenerate: `schema/*.json` (the event schema lists codes)
- Test: `crates/engine/src/mips.rs`, `crates/engine/src/analyse.rs`, `crates/engine/src/codes.rs` unit tests

**Interfaces:**
- Consumes: `NormalBase`, `MipSpec::normal_base` (Task 3).
- Produces:
  - `Analysis::normal_len_mean: f64`;
  - `Code::ChanNormalShort` (Info);
  - in `mips.rs`: `pub(crate) fn normalize_vector(px: &mut [f32; 4], domain: Domain)`, and the private `base_level(work: &Work, p: &Profile) -> Rgba32F` and `weighted_rgb(work: &Work, p: &Profile) -> bool`;
  - `build_chain` keeps its signature.

**Owner decision R1:**
- Mip 0 of a normal map (the `Normal` and `Msn` domains) is renormalised unless `mips.normal_base = keep`.
- For `Normal` and `Msn`, the accumulation chain starts from mip 0 **as written** (controller correction to R1). Levels ≥ 1 therefore average the vectors the file stores, and so does the bake (Tasks 5–7). With `keep` the chain is the same as before M4.
- **Outputs change for every normal map whose source is not unit length.** Mip 0 is rescaled, and levels ≥ 1 average unit vectors, so short and long texels weigh the same.

**Golden outputs that change bytes, with no assertion to update:**
- `normal_maps_are_bc7_under_cs_and_bc5_only_for_flow` (`wall_n`, `iron_n`: `rgb(64)` sources, not unit length). Its new `CHAN-NORMAL-SHORT` infos do not touch its assertions.
- `bc5_on_a_normal_map_blocks_until_allowed` (`iron_n`, `rgb(64)`).

Every other golden normal source is unit length (`flat_normal`, `[128, 128, 255]`), so it is unchanged within rounding. No golden assertion changes in this task.

- [ ] **Step 1: Write the failing tests.**

In `crates/engine/src/mips.rs` `mod tests`, add:

```rust
    #[test]
    fn short_normals_at_mip_0_are_renormalised_by_default() {
        let mut img = Rgba32F::new(2, 1, [0.0, 0.0, 0.5, 0.7]);
        img.px[1] = [0.0, 0.0, 0.0, 0.7]; // a zero vector becomes flat +Z
        let c = build_chain(&work(img, Domain::Normal, true), &profile(Subtype::Normal));
        assert_eq!(c[0].px[0], [0.0, 0.0, 1.0, 0.7]);
        assert_eq!(c[0].px[1], [0.0, 0.0, 1.0, 0.7]); // the (specular) alpha is untouched
    }

    #[test]
    fn the_chain_accumulates_mip_0_as_written() {
        let mut img = Rgba32F::new(2, 1, [0.6, 0.0, 0.0, 1.0]);
        img.px[1] = [0.0, 0.0, 0.3, 1.0];
        let mut p = profile(Subtype::Normal);
        let renormalised = build_chain(&work(img.clone(), Domain::Normal, true), &p);
        p.mips.normal_base = NormalBase::Keep;
        let kept = build_chain(&work(img.clone(), Domain::Normal, true), &p);
        assert_eq!(kept[0].px, img.px);
        assert_ne!(renormalised[0].px, img.px);
        let h = std::f32::consts::FRAC_1_SQRT_2;
        // Default: level 1 averages the written unit vectors (1,0,0) and (0,0,1).
        let v = renormalised[1].px[0];
        assert!((v[0] - h).abs() < 1e-5 && (v[2] - h).abs() < 1e-5, "{v:?}");
        // keep: level 1 averages the authored vectors, (0.3, 0, 0.15) normalised (the pre-M4 chain).
        let len = (0.3f32 * 0.3 + 0.15 * 0.15).sqrt();
        let v = kept[1].px[0];
        assert!(
            (v[0] - 0.3 / len).abs() < 1e-5 && (v[2] - 0.15 / len).abs() < 1e-5,
            "{v:?}"
        );
    }

    #[test]
    fn msn_mip_0_is_renormalised_by_length_only() {
        let mut img = Rgba32F::new(2, 1, [0.0, -0.3, -0.4, 1.0]);
        img.px[1] = [0.0, 0.0, 0.0, 1.0];
        let c = build_chain(&work(img, Domain::Msn, true), &profile(Subtype::Msn));
        let v = c[0].px[0];
        assert!(v[0].abs() < 1e-6 && (v[1] + 0.6).abs() < 1e-6 && (v[2] + 0.8).abs() < 1e-6);
        assert_eq!(&c[0].px[1][..3], &[0.0, 0.0, 0.0]); // no flat default in model space
    }

    #[test]
    fn colour_and_data_mip_0_is_never_renormalised() {
        let img = Rgba32F::new(2, 1, [0.2, 0.2, 0.2, 1.0]);
        let c = build_chain(
            &work(img.clone(), Domain::Stored, false),
            &profile(Subtype::GenericData),
        );
        assert_eq!(c[0].px, img.px);
    }

    #[test]
    fn a_forced_cutout_does_not_alpha_weight_normals_or_roughness() {
        // An opaque +X and an invisible +Z: weighting would give +X, the vector chain averages both.
        let mut img = Rgba32F::new(2, 1, [0.0, 0.0, 1.0, 0.0]);
        img.px[0] = [1.0, 0.0, 0.0, 1.0];
        let mut p = profile(Subtype::Normal);
        p.alpha.policy = AlphaPolicy::Cutout;
        let c = build_chain(&work(img, Domain::Normal, true), &p);
        let v = c[1].px[0];
        assert!(
            (v[0] - std::f32::consts::FRAC_1_SQRT_2).abs() < 1e-5
                && (v[2] - std::f32::consts::FRAC_1_SQRT_2).abs() < 1e-5,
            "{v:?}"
        );
        // Roughness: r = 1 opaque and r = 0 invisible average to sqrt(0.5) on alpha = r², not 1.
        let mut p = profile(Subtype::Rmaos);
        p.alpha.policy = AlphaPolicy::Cutout;
        let mut img = Rgba32F::new(2, 1, [0.0, 0.0, 0.0, 0.0]);
        img.px[0] = [1.0, 0.0, 0.0, 1.0];
        let w = Work {
            img,
            alpha_present: true,
            domain: Domain::Stored,
            roughness_r_squared: true,
            nonfinite_samples: 0,
        };
        let c = build_chain(&w, &p);
        assert!(
            (c[1].px[0][0] - 0.5f32.sqrt()).abs() < 1e-6,
            "{:?}",
            c[1].px[0]
        );
    }
```

In `crates/engine/src/analyse.rs` `mod tests`, add:

```rust
    #[test]
    fn normal_length_mean() {
        let flat = Rgba32F::new(4, 4, [0.5, 0.5, 1.0, 1.0]);
        assert!((analyse(&flat, &RGBA8).normal_len_mean - 1.0).abs() < 0.01);
        let short = Rgba32F::new(4, 4, [0.75, 0.5, 0.5, 1.0]); // decodes to (0.5, 0, 0)
        assert!((analyse(&short, &RGBA8).normal_len_mean - 0.5).abs() < 1e-6);
    }
```

In `crates/engine/src/codes.rs` `mod tests`:
- in `codes_serialise_as_their_strings_and_are_unique`, increase the count in `assert_eq!(Code::ALL.len(), …);` by one (60 → 61 once the packing-fills plan has added `SKIN-WET-FORM`; use the merged value plus one);
- in `severities_follow_spec_section_10`, add `assert_eq!(Code::ChanNormalShort.severity(), Severity::Info);`.

- [ ] **Step 2: Run the tests to see them fail.**

Run: `cargo test -p engine --lib -- mips::tests analyse::tests codes::tests`
Expected:
- FAIL to compile on `normal_len_mean` and `ChanNormalShort`.
- With those two temporarily stubbed, `short_normals_at_mip_0_are_renormalised_by_default`, `the_chain_accumulates_mip_0_as_written` and `a_forced_cutout…` fail.

- [ ] **Step 3: Implement.**

`crates/engine/src/mips.rs`:
- Replace the module doc's last sentence ("Mip 0 is always the base as authored: …") with: "Mip 0 is the base as authored, except a normal map's vectors, which are renormalised unless `mips.normal_base = keep` (DECISIONS R1); a vector chain accumulates from that written mip 0, so its lower levels and the variance bake see the stored vectors."
- Add:

```rust
/// Rescale an RGB vector to unit length in place (spec §5.5). A zero `Normal` vector becomes flat +Z; a zero
/// `_msn` vector stays zero (model space has no flat default).
pub(crate) fn normalize_vector(px: &mut [f32; 4], domain: Domain) {
    let len = (px[0] * px[0] + px[1] * px[1] + px[2] * px[2]).sqrt();
    if len > 1e-8 {
        for c in &mut px[..3] {
            *c /= len;
        }
    } else if domain == Domain::Normal {
        px[..3].copy_from_slice(&[0.0, 0.0, 1.0]);
    }
}

/// Mip 0 as stored (spec §5.4, DECISIONS R1): a normal map's base vectors at unit length unless
/// `mips.normal_base = keep`; anything else as authored. A vector chain accumulates from this level, so the levels
/// below and the variance bake's footprint lengths average the vectors the file stores (controller correction to R1).
fn base_level(work: &Work, p: &Profile) -> Rgba32F {
    let mut out = work.img.clone();
    if p.mips.normal_base == NormalBase::Renormalize
        && matches!(work.domain, Domain::Normal | Domain::Msn)
    {
        for px in &mut out.px {
            normalize_vector(px, work.domain);
        }
    }
    out
}

/// Alpha-weighted RGB filtering (spec §5.5) is for colour. A vector chain (normal, `_msn`, `_flow`) or a roughness
/// chain is never weighted, even with an alpha-tested policy forced on it (Task 29 carry, DECISIONS R11):
/// weighting would bend the vectors, and the bake's footprint lengths, toward opaque texels.
fn weighted_rgb(work: &Work, p: &Profile) -> bool {
    work.alpha_present
        && p.alpha_weighted_mips()
        && !work.roughness_r_squared
        && !matches!(work.domain, Domain::Normal | Domain::Msn | Domain::SignedRg)
}
```

- In `finalize`, replace the whole `Domain::Normal | Domain::Msn => { … }` arm with `Domain::Normal | Domain::Msn => normalize_vector(px, work.domain),`.
- In `build_chain`:
  - replace `let weighted = work.alpha_present && p.alpha_weighted_mips();` with `let weighted = weighted_rgb(work, p);`;
  - replace `out.push(work.img.clone());` with `out.push(base_level(work, p));`;
  - in the first-step `match`, add this arm directly before the final `None => downsample(&work.img, weighted),` arm:

```rust
            // A vector chain accumulates from mip 0 as written (unit length unless `normal_base = keep`).
            None if matches!(work.domain, Domain::Normal | Domain::Msn) => downsample(&out[0], weighted),
```

- Replace the `build_chain` doc comment with:

```rust
/// Finalised mip levels, level 0 first, in the Work's domain.
///
/// Levels >= 1 are finalised and clamped (roughness √, vector renormalisation, domain clamps). Mip 0 is the base as
/// authored, except a normal map's vectors, renormalised unless `mips.normal_base = keep` (DECISIONS R1); a vector
/// chain accumulates from that written mip 0. The consumer (Quantise, Task 31) clamps mip 0 for the target format.
```

- **After the packing-fills merge:** `skin_wet`'s normal form is in the Normal category, so it gets the mip-0 renormalise like any normal. Its grey height form must not resolve to `Domain::Normal`. Check `ops::domain_for` for a BC4 `skin_wet`. If it does resolve to `Domain::Normal`, that is a packing-fills defect: report it and do not work around it here.

`crates/engine/src/analyse.rs`:
- Add the field after `normal_len_err`:

```rust
    /// Mean length of RGB decoded as `*2 - 1` (DECISIONS R1: below 0.9 a normal map's mip 0 is noticeably short).
    pub normal_len_mean: f64,
```

- In `analyse`:
  - add `let mut len_sum = 0.0f64;` before the loop;
  - replace the `len_err += …` line with:

```rust
        let len = (v[0] * v[0] + v[1] * v[1] + v[2] * v[2]).sqrt();
        len_err += f64::from((len - 1.0).abs());
        len_sum += f64::from(len);
```

  - add `normal_len_mean: len_sum / n,` to the `Analysis` literal after `normal_len_err`.

`crates/engine/src/codes.rs`:
- Add `ChanNormalShort = "CHAN-NORMAL-SHORT" => Info,` after `ChanNormalLength`.
- In the module doc, add after "SET-EDIT-STALE (…, M3b)": ", and CHAN-NORMAL-SHORT (a normal map whose mip 0 is noticeably short, M4, DECISIONS R1)".

`crates/engine/src/policy.rs`:
- Next to `NORMAL_LEN_TOL`, add:

```rust
/// Mean decoded normal length below which mip 0 is "noticeably short" (CHAN-NORMAL-SHORT, DECISIONS R1).
const NORMAL_SHORT_MEAN: f64 = 0.9;
```

- In `check_profile`, directly after the `ChanNormalLength` block, add:

```rust
    // Owner 2026-10-11 (DECISIONS R1): a noticeably short normal map, and what happens to its mip 0. A rebuilt Z is
    // unit length by construction.
    if p.category == Category::Normal
        && !p.channels.reconstruct_z
        && a.normal_len_mean < NORMAL_SHORT_MEAN
    {
        let what = match p.mips.normal_base {
            NormalBase::Renormalize => {
                "mip 0 is renormalised to unit length (set mips.normal_base = keep to store it as authored)"
            }
            NormalBase::Keep => "mip 0 is stored as authored (mips.normal_base = keep)",
        };
        add(
            ChanNormalShort,
            format!(
                "mean normal length {:.3} at mip 0: {what}",
                a.normal_len_mean
            ),
        );
    }
```

Regenerate the schemas: `cargo run -p sdc -- presets schema all --out schema`.

- [ ] **Step 4: Run the tests to see them pass.**

Run: `cargo test -p engine --lib` and `cargo test -p engine --test golden`
Expected: PASS. The existing `normals_renormalised_from_level_1_only` still passes: its mip-0 vectors are already unit length.

- [ ] **Step 5: Run every gate and commit.** If a test outside this task's list fails because it asserts an exact warning list that now holds `CHAN-NORMAL-SHORT`, stop and report it (Global Constraints).

```bash
cargo fmt --all && cargo clippy --workspace --all-targets -- -D warnings && cargo test --workspace
git add crates/engine/src/mips.rs crates/engine/src/analyse.rs crates/engine/src/codes.rs crates/engine/src/policy.rs schema
git commit -m "feat(engine): normal maps store mip 0 at unit length by default (mips.normal_base), CHAN-NORMAL-SHORT, no alpha weighting for vectors or roughness

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

---

### Task 5: The bake formula, the length pyramid and the roughness chain (`bake/mod.rs`, `build_chain_with`)

**Files:**
- Create: `crates/engine/src/bake/mod.rs`
- Modify:
  - `crates/engine/src/lib.rs` (`pub mod bake;`);
  - `crates/engine/src/mips.rs` (`box_down`, `build_chain_with`, `bake_roughness`, doc).
- Test: `crates/engine/src/bake/mod.rs` (`mod tests`, including the f64 reference)

**Interfaces:**
- Consumes:
  - `mips::level_count`;
  - `ops::preprocess` (tests only);
  - `MipSpec::bake_clamp` (Task 3);
  - `base_level` and `weighted_rgb` (Task 4).
- Produces:
  - `bake::{UNIT_LENGTH, variance_add, baked_roughness, bake_shift, LengthLevel, LengthPyramid, NormalVariance}` (exact signatures in the registry);
  - `mips::build_chain_with(work: &Work, p: &Profile, bake: Option<&NormalVariance>) -> Vec<Rgba32F>`;
  - `pub(crate) fn mips::box_down(prev: &Rgba32F) -> Rgba32F`.

**The level mapping (DECISIONS R5):**
- Let `shift = log2(_n width / member width)`. Member level ℓ reads `_n` level `m = ℓ + shift`.
- When `m ≤ 0`, the footprint covers at most one `_n` texel, so v = 0 (spec §6.2 step 1). For example, mip 0 of a member with a half-size `_n`.
- When `m ≥ 1`, the length is the `_n` box pyramid's level `m` at the same (x, y). Its dimensions equal the member level's, because both chains halve the same way, including the tail of a non-square image.
- With `shift = 1`, mip 0 of the member receives variance (spec §6.2: "If the `_n` base is larger than the roughness member's base, its mip 0 correctly receives variance").
- Texels with nothing to add keep the plain chain's value bit for bit.

**Expectations that change:** none. `build_chain` calls `build_chain_with(…, None)` and is bit-identical to Task 4.

- [ ] **Step 1: Write the failing tests.** Create `crates/engine/src/bake/mod.rs` with the module skeleton below, and the tests at its end:

```rust
//! Normal-variance bake (spec §6.2, DECISIONS #6, Note R): a normal map's footprint length L → κ = (3L − L³)/(1 − L²)
//! → v = 0.25/κ → α′² = saturate(α² + min(2v, t²)), r′ = (α′²)^¼ [MIP, Unity HDRP `CommonMaterial.hlsl`]. The
//! footprint lengths come from a box pyramid of the `_n`'s mip 0 as written: unit length unless `mips.normal_base = keep`
//! (DECISIONS R1, R5).
use crate::config::{MipKeyword, MipLevels};
use crate::image::Rgba32F;

/// Lengths at or above this count as unit: no variance (spec §6.2 step 2).
pub const UNIT_LENGTH: f64 = 1.0 - 1e-6;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{Profile, Subtype, Target};
    use crate::mips::{build_chain, build_chain_with};
    use crate::ops::preprocess;
    use crate::presets::PresetRegistry;

    fn profile(st: Subtype) -> Profile {
        PresetRegistry::builtin()
            .unwrap()
            .profile("cs-quality", Target::Cs, st)
            .unwrap()
    }

    /// An encoded normal map whose unit-length texels change direction from texel to texel.
    fn bumpy(w: u32) -> Rgba32F {
        let mut img = Rgba32F::new(w, w, [0.0, 0.0, 0.0, 1.0]);
        for y in 0..w {
            for x in 0..w {
                let nx = 0.6 * (x as f32 * 1.3 + y as f32 * 0.7).sin();
                let ny = 0.6 * (x as f32 * 0.9 - y as f32 * 1.1).cos();
                let nz = (1.0 - nx * nx - ny * ny).sqrt();
                *img.at_mut(x, y) = [nx * 0.5 + 0.5, ny * 0.5 + 0.5, nz * 0.5 + 0.5, 1.0];
            }
        }
        img
    }

    /// A roughness member: R a ramp in [0.15, 0.85), G/B/A constant.
    fn ramp(w: u32) -> Rgba32F {
        let mut img = Rgba32F::new(w, w, [0.0, 0.0, 0.8, 1.0]);
        for y in 0..w {
            for x in 0..w {
                img.at_mut(x, y)[0] = 0.15 + 0.7 * ((x * 3 + y * 2) % w) as f32 / w as f32;
            }
        }
        img
    }

    /// Spec §6.2 in f64 (test code only, Global Constraints): the stored R of member texel (x, y) at `level`, with
    /// the `_n` `shift` levels larger (negative: smaller). Square images.
    fn reference(
        normals: &Rgba32F,
        rough: &Rgba32F,
        level: u32,
        shift: i32,
        x: u32,
        y: u32,
        t: f64,
    ) -> f64 {
        let f = 1u32 << level;
        let mut alpha = 0.0f64;
        for j in 0..f {
            for i in 0..f {
                let r = f64::from(rough.at(x * f + i, y * f + j)[0]);
                alpha += r * r;
            }
        }
        alpha /= f64::from(f * f);
        let m = level as i32 + shift;
        let add = if m < 1 {
            0.0
        } else {
            let g = 1u32 << m;
            let mut s = [0.0f64; 3];
            for j in 0..g {
                for i in 0..g {
                    let q = normals.at(x * g + i, y * g + j);
                    for c in 0..3 {
                        s[c] += f64::from(q[c]);
                    }
                }
            }
            let n = f64::from(g * g);
            let l = ((s[0] / n).powi(2) + (s[1] / n).powi(2) + (s[2] / n).powi(2)).sqrt();
            if l >= 1.0 - 1e-6 {
                0.0
            } else if l <= 0.0 {
                t * t
            } else {
                // 2v = 2 * 0.25 / κ = 0.5 (1 − L²) / (3L − L³)
                (0.5 * (1.0 - l * l) / (3.0 * l - l * l * l)).min(t * t)
            }
        };
        if add == 0.0 {
            alpha.sqrt()
        } else {
            (alpha * alpha + add).min(1.0).powf(0.25)
        }
    }

    /// Mip 0 of a `_n` as its job writes it under the default `normal_base = renormalize` (DECISIONS R1).
    fn written(w: crate::ops::Work) -> Rgba32F {
        let mut img = w.img;
        for px in &mut img.px {
            crate::mips::normalize_vector(px, w.domain);
        }
        img
    }

    fn close(a: f32, b: f64) -> bool {
        (f64::from(a) - b).abs() < 1e-6
    }

    #[test]
    fn variance_add_follows_the_formula_and_the_clamp() {
        assert_eq!(variance_add(1.0, 0.25), 0.0);
        assert_eq!(variance_add(1.2, 0.25), 0.0); // longer than unit: no variance
        assert_eq!(variance_add(f32::NAN, 0.25), 0.0);
        assert!(close(variance_add(0.0, 0.25), 0.0625)); // t²
        // L = 0.9: κ = (2.7 − 0.729) / 0.19 = 10.3737, 2v = 0.0482 < t².
        let l = 0.9f64;
        let want = 0.5 * (1.0 - l * l) / (3.0 * l - l * l * l);
        assert!(close(variance_add(0.9, 0.25), want), "{want}");
        // L = 0.5: 2v = 0.27, clamped to t² = 0.0625; a wider clamp lets more through.
        assert!(close(variance_add(0.5, 0.25), 0.0625));
        assert!(close(variance_add(0.5, 1.0), 0.5 * 0.75 / 1.375));
    }

    #[test]
    fn baked_roughness_is_the_plain_sqrt_without_variance_and_saturates() {
        assert_eq!(baked_roughness(0.25, 0.0), 0.5);
        assert!((baked_roughness(0.25, 0.0625) - (0.0625f32 + 0.0625).powf(0.25)).abs() < 1e-6);
        assert!((baked_roughness(0.9, 0.0625) - (0.9f32 * 0.9 + 0.0625).powf(0.25)).abs() < 1e-6);
        assert_eq!(baked_roughness(1.0, 0.0625), 1.0); // saturate
    }

    #[test]
    fn bake_shift_needs_power_of_two_sizes_with_one_aspect() {
        assert_eq!(bake_shift((128, 128), (64, 64)), Ok(1));
        assert_eq!(bake_shift((32, 32), (64, 64)), Ok(-1));
        assert_eq!(bake_shift((64, 64), (64, 64)), Ok(0));
        assert_eq!(bake_shift((2048, 1024), (1024, 512)), Ok(1));
        assert!(bake_shift((64, 64), (64, 32)).is_err()); // aspect mismatch
        assert!(bake_shift((96, 96), (48, 48)).is_err()); // NPOT
    }

    #[test]
    fn the_pyramid_holds_footprint_lengths_of_the_base_it_is_given() {
        // (0.6, 0, 0.8) and (-0.6, 0, 0.8) alternate in x: every level-1 footprint averages to (0, 0, 0.8).
        let mut base = Rgba32F::new(4, 2, [0.6, 0.0, 0.8, 1.0]);
        for y in 0..2 {
            for x in (1..4).step_by(2) {
                base.at_mut(x, y)[0] = -0.6;
            }
        }
        let p = LengthPyramid::build(&base);
        assert_eq!(p.base, (4, 2));
        assert_eq!(p.levels.len(), 2); // 2x1 and 1x1
        let l1 = p.level(1).unwrap();
        assert_eq!((l1.width, l1.height), (2, 1));
        assert!(l1.len.iter().all(|&v| (v - 0.8).abs() < 1e-6), "{:?}", l1.len);
        assert!(p.level(0).is_none() && p.level(3).is_none());
    }

    #[test]
    fn normal_variance_rejects_a_member_whose_size_does_not_line_up() {
        let flat = Rgba32F::new(16, 16, [0.0, 0.0, 1.0, 1.0]);
        assert!(NormalVariance::new(LengthPyramid::build(&flat), (16, 8), 0.25).is_err());
        assert!(NormalVariance::new(LengthPyramid::build(&flat), (8, 8), 0.25).is_ok());
    }

    #[test]
    fn roughness_chain_matches_the_f64_reference_at_1_to_1_2_to_1_and_1_to_2() {
        for (nw, mw, shift) in [(16u32, 16u32, 0i32), (32, 16, 1), (8, 16, -1)] {
            let normals = written(preprocess(bumpy(nw), &profile(Subtype::PbrNormal)));
            let pr = profile(Subtype::Rmaos);
            let rough = preprocess(ramp(mw), &pr);
            let nv = NormalVariance::new(LengthPyramid::build(&normals), (mw, mw), 0.25).unwrap();
            assert_eq!(nv.shift(), shift);
            let baked = build_chain_with(&rough, &pr, Some(&nv));
            let plain = build_chain(&rough, &pr);
            let mut widened = 0;
            for (level, (l, q)) in baked.iter().zip(&plain).enumerate() {
                for y in 0..l.height {
                    for x in 0..l.width {
                        let want = reference(&normals, &rough.img, level as u32, shift, x, y, 0.25);
                        let got = f64::from(l.at(x, y)[0]);
                        assert!(
                            (got - want).abs() < 1e-4,
                            "{nw}:{mw} level {level} ({x},{y}): {got} vs {want}"
                        );
                        if l.at(x, y)[0] > q.at(x, y)[0] + 1e-3 {
                            widened += 1;
                        }
                        // G, B and A are never touched by the bake.
                        assert_eq!(&l.at(x, y)[1..], &q.at(x, y)[1..]);
                    }
                }
            }
            assert!(widened > 0, "{nw}:{mw}: the bake must widen something");
        }
    }

    #[test]
    fn a_flat_normal_map_leaves_the_roughness_chain_bit_identical() {
        let flat = preprocess(
            Rgba32F::new(32, 32, [0.5, 0.5, 1.0, 1.0]),
            &profile(Subtype::PbrNormal),
        )
        .img;
        let pr = profile(Subtype::Rmaos);
        let rough = preprocess(ramp(16), &pr);
        let nv = NormalVariance::new(LengthPyramid::build(&flat), (16, 16), 0.25).unwrap();
        assert_eq!(build_chain_with(&rough, &pr, Some(&nv)), build_chain(&rough, &pr));
    }

    #[test]
    fn a_bake_given_to_a_non_roughness_chain_is_ignored() {
        let normals = written(preprocess(bumpy(16), &profile(Subtype::PbrNormal)));
        let pn = profile(Subtype::PbrNormal);
        let w = preprocess(bumpy(16), &pn);
        let nv = NormalVariance::new(LengthPyramid::build(&normals), (16, 16), 0.25).unwrap();
        assert_eq!(build_chain_with(&w, &pn, Some(&nv)), build_chain(&w, &pn));
    }
}
```

Add `pub mod bake;` to `crates/engine/src/lib.rs`, in alphabetical order after `pub mod api;`.

- [ ] **Step 2: Run the tests to see them fail.**

Run: `cargo test -p engine --lib bake::tests`
Expected: FAIL to compile. `variance_add`, `baked_roughness`, `bake_shift`, `LengthPyramid`, `NormalVariance` and `build_chain_with` do not exist.

- [ ] **Step 3: Implement.** In `crates/engine/src/bake/mod.rs`, between `UNIT_LENGTH` and `#[cfg(test)]`, add:

```rust
/// `min(2v, t²)` for a footprint whose average normal has length `len` (spec §6.2 steps 2 and 4): 0 for a unit or
/// longer average (or NaN), `t²` for a zero-length one. Computed in f64.
pub fn variance_add(len: f32, clamp_t: f64) -> f32 {
    let l = f64::from(len);
    let cap = clamp_t * clamp_t;
    if l.is_nan() || l >= UNIT_LENGTH {
        return 0.0;
    }
    if l <= 0.0 {
        return cap as f32;
    }
    let kappa = (3.0 * l - l * l * l) / (1.0 - l * l);
    let v = 0.25 / kappa;
    (2.0 * v).min(cap) as f32
}

/// r′ = (min(α² + add, 1))^¼ from the footprint mean `mean_alpha` of α = r² (spec §6.2 steps 3–4). With nothing to
/// add it is √α, the plain chain's value.
pub fn baked_roughness(mean_alpha: f32, add: f32) -> f32 {
    let a = mean_alpha.max(0.0);
    if add <= 0.0 {
        a.sqrt()
    } else {
        (a * a + add).min(1.0).powf(0.25)
    }
}

/// log2 of the `_n` : roughness-member size ratio (1 for a 2:1 `_n`, −1 for a half-size one), or why the set does
/// not bake (`SET-BAKE-RATIO`, DECISIONS R5). Both bases must be powers of two with one aspect, so every footprint
/// is whole texels of one pyramid level.
pub fn bake_shift(normal: (u32, u32), member: (u32, u32)) -> Result<i32, String> {
    let pot = |(w, h): (u32, u32)| w.is_power_of_two() && h.is_power_of_two();
    if !pot(normal) || !pot(member) {
        return Err(format!(
            "_n {}x{} and roughness {}x{} must both be powers of two",
            normal.0, normal.1, member.0, member.1
        ));
    }
    let log = |v: u32| v.trailing_zeros() as i32;
    let (sx, sy) = (
        log(normal.0) - log(member.0),
        log(normal.1) - log(member.1),
    );
    if sx != sy {
        return Err(format!(
            "_n {}x{} and roughness {}x{} differ in aspect, so their size ratio is not one power of two",
            normal.0, normal.1, member.0, member.1
        ));
    }
    Ok(sx)
}

/// One pyramid level: the length of the average base normal over each texel's footprint.
#[derive(Debug, Clone, PartialEq)]
pub struct LengthLevel {
    pub width: u32,
    pub height: u32,
    pub len: Vec<f32>,
}

/// Footprint lengths of a normal map for every level >= 1 of its full box chain (level `m` at index `m − 1`), from
/// the `_n`'s mip 0 as written (the caller renormalises it unless `keep`; DECISIONS R1, R5). About 1.33 bytes per base texel.
#[derive(Debug, Clone, PartialEq)]
pub struct LengthPyramid {
    /// Base width and height.
    pub base: (u32, u32),
    pub levels: Vec<LengthLevel>,
}

impl LengthPyramid {
    /// Box-filter `base` (a `Normal`-domain image) down to 1x1, keeping only each level's lengths. Only one
    /// accumulation level is alive at a time.
    pub fn build(base: &Rgba32F) -> LengthPyramid {
        let full = crate::mips::level_count(
            base.width,
            base.height,
            &MipLevels::Keyword(MipKeyword::Full),
        );
        let mut levels = Vec::with_capacity(full.saturating_sub(1) as usize);
        let mut acc: Option<Rgba32F> = None;
        for _ in 1..full {
            let next = crate::mips::box_down(acc.as_ref().unwrap_or(base));
            levels.push(LengthLevel {
                width: next.width,
                height: next.height,
                len: next
                    .px
                    .iter()
                    .map(|p| (p[0] * p[0] + p[1] * p[1] + p[2] * p[2]).sqrt())
                    .collect(),
            });
            acc = Some(next);
        }
        LengthPyramid {
            base: (base.width, base.height),
            levels,
        }
    }

    /// Level `m` (>= 1), or `None` for 0 or past the last level.
    pub fn level(&self, m: usize) -> Option<&LengthLevel> {
        m.checked_sub(1).and_then(|i| self.levels.get(i))
    }
}

/// The bake of one roughness member: its `_n`'s footprint lengths, the level shift and the clamp `t`.
#[derive(Debug, Clone, PartialEq)]
pub struct NormalVariance {
    lengths: LengthPyramid,
    shift: i32,
    clamp_t: f64,
}

impl NormalVariance {
    /// For a member whose base is `member` (width, height); an error when the sizes cannot bake ([`bake_shift`]).
    pub fn new(
        lengths: LengthPyramid,
        member: (u32, u32),
        clamp_t: f64,
    ) -> Result<NormalVariance, String> {
        let shift = bake_shift(lengths.base, member)?;
        Ok(NormalVariance {
            lengths,
            shift,
            clamp_t,
        })
    }

    pub fn shift(&self) -> i32 {
        self.shift
    }

    /// `min(2v, t²)` for texel (x, y) of the member's mip `level`; 0 where the footprint covers at most one `_n`
    /// texel (`level + shift <= 0`).
    pub fn add_at(&self, level: usize, x: u32, y: u32) -> f32 {
        let m = level as i64 + i64::from(self.shift);
        if m < 1 {
            return 0.0;
        }
        match self.lengths.level(m as usize) {
            Some(l) if x < l.width && y < l.height => {
                variance_add(l.len[(y * l.width + x) as usize], self.clamp_t)
            }
            _ => 0.0,
        }
    }
}
```

In `crates/engine/src/mips.rs`:
- Add `use crate::bake::{baked_roughness, NormalVariance};` to the imports.
- Add after `downsample`:

```rust
/// One unweighted box step (the bake's length pyramid, `bake::LengthPyramid::build`).
pub(crate) fn box_down(prev: &Rgba32F) -> Rgba32F {
    downsample(prev, false)
}
```

- Replace the whole `build_chain` function (and its doc comment) with:

```rust
/// Finalised mip levels, level 0 first, in the Work's domain.
///
/// Levels >= 1 are finalised and clamped (roughness √, vector renormalisation, domain clamps). Mip 0 is the base as
/// authored, except a normal map's vectors, renormalised unless `mips.normal_base = keep` (DECISIONS R1); the
/// consumer (Quantise, Task 31) clamps it for the target format.
pub fn build_chain(work: &Work, p: &Profile) -> Vec<Rgba32F> {
    build_chain_with(work, p, None)
}

/// [`build_chain`] with a roughness member's variance bake (spec §6.2): where `bake` adds variance, R is stored as
/// `(min(α² + add, 1))^¼`, α the footprint mean of r²; every other value is bit-identical to [`build_chain`].
/// `bake` is ignored unless the Work is a roughness member.
pub fn build_chain_with(work: &Work, p: &Profile, bake: Option<&NormalVariance>) -> Vec<Rgba32F> {
    let n = level_count(work.img.width, work.img.height, &p.mips.levels);
    let weighted = weighted_rgb(work, p);
    let rough = bake.filter(|_| work.roughness_r_squared);
    let mut out = Vec::with_capacity(n as usize);
    let mut base = base_level(work, p);
    if let Some(b) = rough {
        let src = &work.img;
        bake_roughness(&mut base, 0, b, |i| src.px[i][0] * src.px[i][0]);
    }
    out.push(base);
    // `acc` is the unnormalised accumulation level; the first step reads the base directly unless roughness needs
    // squaring, so the base is not cloned a second time.
    let mut acc: Option<Rgba32F> = None;
    for level in 1..n as usize {
        let next = match &acc {
            Some(prev) => downsample(prev, weighted),
            None if work.roughness_r_squared => {
                let mut squared = work.img.clone();
                for px in &mut squared.px {
                    px[0] *= px[0];
                }
                downsample(&squared, weighted)
            }
            // A vector chain accumulates from mip 0 as written (unit length unless `normal_base = keep`).
            None if matches!(work.domain, Domain::Normal | Domain::Msn) => downsample(&out[0], weighted),
            None => downsample(&work.img, weighted),
        };
        let mut fin = finalize(&next, work);
        if let Some(b) = rough {
            bake_roughness(&mut fin, level, b, |i| next.px[i][0]);
        }
        out.push(fin);
        acc = Some(next);
    }
    out
}

/// Spec §6.2 step 4 on R: a texel whose `_n` footprint adds variance gets `(min(α² + add, 1))^¼`, α = `alpha(i)` (the
/// footprint mean of r²); the rest keep the plain chain's value bit for bit.
fn bake_roughness(
    level: &mut Rgba32F,
    index: usize,
    b: &NormalVariance,
    alpha: impl Fn(usize) -> f32,
) {
    let w = level.width;
    for (i, px) in level.px.iter_mut().enumerate() {
        let add = b.add_at(index, i as u32 % w, i as u32 / w);
        if add > 0.0 {
            px[0] = baked_roughness(alpha(i), add);
        }
    }
}
```

- [ ] **Step 4: Run the tests to see them pass.**

Run: `cargo test -p engine --lib -- bake::tests mips::tests`
Expected: PASS.

- [ ] **Step 5: Run every gate and commit.**

```bash
cargo fmt --all && cargo clippy --workspace --all-targets -- -D warnings && cargo test --workspace
git add crates/engine/src/bake/mod.rs crates/engine/src/lib.rs crates/engine/src/mips.rs
git commit -m "feat(engine): variance-bake formula, footprint length pyramid and the baked roughness chain (M4)

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

---

### Task 6: `_cnr` self-bake: the coat normal's own variance widens its coat roughness (owner R2)

**Files:**
- Modify:
  - `crates/engine/src/mips.rs` (`coat_self_bake`, `bake_coat`, the call in `build_chain_with`, module doc, tests);
  - `presets/sdc-base.toml` (`[subtypes.coat_normal]`).
- Test: `crates/engine/src/mips.rs` (`mod tests`)

**Interfaces:**
- Consumes: `bake::{variance_add, baked_roughness}` (Task 5); `MipSpec::bake_clamp` (Task 3).
- Produces:
  - `coat_normal` profiles default to `mips.normal = renormalize_bake`;
  - `build_chain` reads `p.mips.normal` (the roadmap carry: "`mips::build_chain` must read `p.mips.normal`").

**Rules:**
- The self-bake runs when all of these hold:
  - the sub-type is `coat_normal` and the domain is `Normal`;
  - `mips.normal = renormalize_bake`;
  - the alpha is present and `alpha.policy = data`.
- For each level ≥ 1:
  - L is the length of that level's own accumulation before renormalising. The chain accumulates from mip 0 as written (Task 4), so under the default a uniformly short coat normal adds nothing;
  - `add = variance_add(L, t)`;
  - where `add > 0`, A = `baked_roughness(r̄², add)` = (min(r̄⁴ + add, 1))^¼, with r̄ the plain box mean the chain already holds.
- Mip 0 is untouched (1:1: a single texel has no variance).
- Where `add = 0`, A is untouched, so a flat coat normal's alpha is bit-identical to today (the roadmap exit).
- `mips.normal = renormalize` turns the self-bake off.

**Expectations that change:**
- Outputs change for `_cnr` sources with non-flat normals.
- No golden case has a `_cnr` (Task 9 adds two).
- Every `coat_normal` `profile_sha256` changes, which is already covered by R12.

- [ ] **Step 1: Write the failing tests.** In `crates/engine/src/mips.rs` `mod tests`, add:

```rust
    fn coat(img: Rgba32F) -> (Work, Profile) {
        let p = profile(Subtype::CoatNormal);
        (preprocess(img, &p), p)
    }

    #[test]
    fn cnr_profiles_self_bake_by_default() {
        assert_eq!(
            profile(Subtype::CoatNormal).mips.normal,
            NormalMips::RenormalizeBake
        );
        assert_eq!(profile(Subtype::PbrNormal).mips.normal, NormalMips::Renormalize);
    }

    #[test]
    fn a_flat_coat_normal_keeps_its_coat_roughness_bit_for_bit() {
        let mut img = Rgba32F::new(4, 4, [0.5, 0.5, 1.0, 0.0]);
        for (i, px) in img.px.iter_mut().enumerate() {
            px[3] = i as f32 / 15.0;
        }
        let (w, p) = coat(img);
        let mut off = p.clone();
        off.mips.normal = NormalMips::Renormalize;
        assert_eq!(build_chain(&w, &p), build_chain(&w, &off));
    }

    #[test]
    fn a_bumpy_coat_normal_widens_its_own_coat_roughness_by_the_formula() {
        // (0.6, 0, 0.8) and (-0.6, 0, 0.8): the level-1 footprint length is 0.8.
        let mut img = Rgba32F::new(2, 1, [0.8, 0.5, 0.9, 0.4]);
        img.px[1] = [0.2, 0.5, 0.9, 0.2];
        let (w, p) = coat(img);
        let c = build_chain(&w, &p);
        let r = (0.4f64 + 0.2) / 2.0;
        let l = 0.8f64;
        let add = (0.5 * (1.0 - l * l) / (3.0 * l - l * l * l)).min(0.0625);
        let want = (r.powi(4) + add).min(1.0).powf(0.25);
        let got = f64::from(c[1].px[0][3]);
        assert!((got - want).abs() < 1e-5, "{got} vs {want}");
        assert!(got > r + 0.01);
        assert_eq!(c[0].px[0][3], 0.4); // mip 0 keeps the source coat roughness
    }

    #[test]
    fn renormalize_turns_the_coat_self_bake_off() {
        let mut img = Rgba32F::new(2, 1, [0.8, 0.5, 0.9, 0.4]);
        img.px[1] = [0.2, 0.5, 0.9, 0.2];
        let (w, mut p) = coat(img);
        p.mips.normal = NormalMips::Renormalize;
        let c = build_chain(&w, &p);
        assert!((c[1].px[0][3] - 0.3).abs() < 1e-6, "{}", c[1].px[0][3]);
    }
```

- [ ] **Step 2: Run the tests to see them fail.**

Run: `cargo test -p engine --lib mips::tests`
Expected: FAIL. `cnr_profiles_self_bake_by_default` fails (the default is still `renormalize`), and `a_bumpy_coat_normal_widens_…` fails (the alpha stays at 0.3).

- [ ] **Step 3: Implement.**

`presets/sdc-base.toml`: replace

```toml
[subtypes.coat_normal]
alpha = { policy = "data" }
```

with

```toml
[subtypes.coat_normal]
alpha = { policy = "data" }
mips = { normal = "renormalize_bake" }
```

`crates/engine/src/mips.rs`:
- Change the bake import to `use crate::bake::{baked_roughness, variance_add, NormalVariance};`.
- Add:

```rust
/// `_cnr` self-bake (owner 2026-10-11, DECISIONS R2): the clamp `t` when a coat normal with
/// `mips.normal = renormalize_bake` keeps a data alpha (its coat roughness), else `None`.
fn coat_self_bake(work: &Work, p: &Profile) -> Option<f64> {
    (p.subtype == Subtype::CoatNormal
        && work.domain == Domain::Normal
        && p.mips.normal == NormalMips::RenormalizeBake
        && work.alpha_present
        && p.alpha.policy == AlphaPolicy::Data)
        .then_some(p.mips.bake_clamp)
}

/// The coat roughness of one level: where the level's own footprint length (`acc`, before renormalising) adds
/// variance, A = (min(r̄⁴ + add, 1))^¼ with r̄ the plain box mean the level already holds; elsewhere A is unchanged,
/// so a flat coat normal keeps its alpha bit for bit.
fn bake_coat(level: &mut Rgba32F, acc: &Rgba32F, clamp_t: f64) {
    for (px, a) in level.px.iter_mut().zip(&acc.px) {
        let len = (a[0] * a[0] + a[1] * a[1] + a[2] * a[2]).sqrt();
        let add = variance_add(len, clamp_t);
        if add > 0.0 {
            px[3] = baked_roughness(px[3] * px[3], add);
        }
    }
}
```

- In `build_chain_with`, after `let rough = bake.filter(|_| work.roughness_r_squared);`, add `let coat = coat_self_bake(work, p);`. After the `if let Some(b) = rough { bake_roughness(&mut fin, level, b, |i| next.px[i][0]); }` block, add:

```rust
        if let Some(t) = coat {
            bake_coat(&mut fin, &next, t);
        }
```

- Append to the module doc: "A `_cnr` with `mips.normal = renormalize_bake` (the default) widens its own coat roughness (alpha) from its own footprint lengths (DECISIONS R2); `mips.normal` is inert on every other sub-type here: the `_n` → `_rmaos` bake arrives through [`build_chain_with`]."

- [ ] **Step 4: Run the tests to see them pass.**

Run: `cargo test -p engine --lib -- mips::tests bake::tests`
Expected: PASS.

- [ ] **Step 5: Run every gate and commit.**

```bash
cargo fmt --all && cargo clippy --workspace --all-targets -- -D warnings && cargo test --workspace
git add crates/engine/src/mips.rs presets/sdc-base.toml
git commit -m "feat(engine): _cnr self-bake: a coat normal's own variance widens its coat roughness (M4, owner)

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

---

### Task 7: `BakeJob` at run time: companion load, stamp check, manifest source, `if-changed`, memory admission

**Files:**
- Modify:
  - `crates/engine/src/bake/mod.rs` (`BakeJob`, tests);
  - `crates/engine/src/policy.rs` (`ResolvedJob::bake` and the struct literals);
  - `crates/engine/src/pack/plan.rs` (struct literal);
  - `crates/engine/src/pipeline.rs` (`check_stamps`, `prepare_then`, `process_file`);
  - `crates/engine/src/manifest.rs` (`SourceRef::bake`, `of_bake`, docs);
  - `crates/engine/src/api.rs` (`current_sources`, `pack_roles_match`, `is_up_to_date`, `job_bytes`, admission in `run`, test literals);
  - `crates/engine/tests/events_manifest.rs` and `crates/engine/tests/pack_run.rs` (struct literals only).
- Regenerate: `schema/*.json` (`ResolvedJob` in the event schema, `SourceRef` in the manifest schema)
- Test: `crates/engine/tests/bake_run.rs` (create); `crates/engine/src/bake/mod.rs` unit tests

**Interfaces:**
- Consumes: `LengthPyramid`, `NormalVariance`, `build_chain_with` (Task 5); `SourceStamp`; `load::load_image`; `ops::preprocess`.
- Produces:
  - `bake::BakeJob { normal: PathBuf, normal_profile: Profile, shift: i32, stamp: Option<SourceStamp> }`, with `identity_sha256(&self) -> String`, `load_lengths(&self) -> Result<LengthPyramid, String>` and `estimated_bytes(&self) -> u64`;
  - `ResolvedJob::bake: Option<BakeJob>`;
  - `SourceRef::bake: Option<String>` and `SourceRef::of_bake(path: &Path, identity: String) -> io::Result<SourceRef>`.

**Rules:**
- **Stamp check:** the stamp check covers the `_n` (carried item "source drift on the `_n` companion"). A changed `_n` fails the member at Load with `<path>: source changed since planning; re-run`.
- **Order:** the `_n` loads first. Its pyramid is built and its image dropped before the member's own image loads, so the two are never held together (R4).
- **Manifest:** the member's manifest record lists the `_n` in `sources`, last, with `bake` = `identity_sha256()`. `if-changed` compares every recorded `(sha256, bake)` pair in order (R12).
- **Admission:** a baking job is admitted for `estimated_bytes(input) + BakeJob::estimated_bytes()` (carried item "the summed-area table counts against `--mem-budget`"; now the pyramid, R5).

**Expectations that change:** none. Struct literals gain `bake: None`. A record without a bake source compares as before.

- [ ] **Step 1: Write the failing tests.** Create `crates/engine/tests/bake_run.rs`:

```rust
//! M4 Task 7: a roughness member with a `BakeJob` decodes its `_n`, widens its roughness, records the `_n` in the
//! manifest, re-encodes when the `_n` or its decode settings change, and fails when the `_n` changed after
//! planning (DECISIONS R4, R12).
use engine::api::*;
use engine::bake::BakeJob;
use engine::config::*;
use engine::dds::read_dds;
use engine::decode::{decode_level, Decoded};
use engine::events::*;
use engine::manifest::{sha256_file, Manifest};
use engine::policy::*;
use engine::presets::PresetRegistry;
use image::{ImageBuffer, Rgb, Rgba};
use std::path::{Path, PathBuf};
use std::sync::Arc;

/// A unit-length normal map whose direction changes from texel to texel; another `phase` gives another map.
fn bumpy_n(dir: &Path, w: u32, phase: f32) -> PathBuf {
    let p = dir.join("in/iron_n.png");
    std::fs::create_dir_all(p.parent().unwrap()).unwrap();
    ImageBuffer::from_fn(w, w, |x, y| {
        let nx = 0.6 * (x as f32 * 1.3 + y as f32 * 0.7 + phase).sin();
        let ny = 0.6 * (x as f32 * 0.9 - y as f32 * 1.1).cos();
        let nz = (1.0 - nx * nx - ny * ny).sqrt();
        let e = |v: f32| ((v * 0.5 + 0.5) * 255.0).round() as u8;
        Rgb([e(nx), e(ny), e(nz)])
    })
    .save(&p)
    .unwrap();
    p
}

/// A constant roughness of 100/255 (G 0, B 255, A 255).
fn rmaos(dir: &Path, w: u32) -> PathBuf {
    let p = dir.join("in/iron_rmaos.png");
    std::fs::create_dir_all(p.parent().unwrap()).unwrap();
    ImageBuffer::from_fn(w, w, |_, _| Rgba([100u8, 0, 255, 255]))
        .save(&p)
        .unwrap();
    p
}

/// The `_rmaos` job as `Engine::plan` builds it for a baking set (stamps taken now). `bake: false` leaves the
/// `BakeJob` off; `flip_green` changes the `_n`'s decode settings.
fn plan(dir: &Path, rough: &Path, normal: &Path, bake: bool, flip_green: bool) -> Plan {
    let registry = PresetRegistry::builtin().unwrap();
    let mut normal_profile = registry
        .profile("cs-quality", Target::Cs, Subtype::PbrNormal)
        .unwrap();
    normal_profile.channels.flip_green = flip_green;
    let mut request = BatchRequest::new(vec![dir.join("in")], dir.join("out"), Target::Cs);
    request.on_existing = OnExisting::IfChanged;
    let job = ResolvedJob {
        input: rough.to_path_buf(),
        output: dir.join("out/iron_rmaos.dds"),
        game_path: "iron_rmaos.png".into(),
        set_key: "out/iron".into(),
        profile: registry
            .profile("cs-quality", Target::Cs, Subtype::Rmaos)
            .unwrap(),
        provenance: Default::default(),
        suggestions: vec![],
        warnings: vec![],
        status: JobStatus::Ready,
        suffix_alias: None,
        source: SourceStamp::of(rough),
        pack: None,
        bake: bake.then(|| BakeJob {
            normal: normal.to_path_buf(),
            normal_profile,
            shift: 0,
            stamp: SourceStamp::of(normal),
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
    let rep = Reporter::new(sink.clone(), "bake-run");
    let s = Engine::new()
        .unwrap()
        .run(plan, &rep, &CancelToken::default())
        .unwrap();
    (s, sink.take())
}

/// Mean decoded R of mip `level` of `out/iron_rmaos.dds`, in codes.
fn mean_r(dir: &Path, level: usize) -> f64 {
    let f = read_dds(&std::fs::read(dir.join("out/iron_rmaos.dds")).unwrap()).unwrap();
    let (w, h, r) = f.levels[level].clone();
    let Decoded::Ldr(px) = decode_level(f.format, w, h, &f.data[r], None).unwrap() else {
        panic!("LDR expected")
    };
    px.iter().map(|p| f64::from(p[0])).sum::<f64>() / px.len() as f64
}

#[test]
fn a_baked_member_is_rougher_and_records_its_normal() {
    let (a, b) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
    for (d, bake) in [(&a, true), (&b, false)] {
        let n = bumpy_n(d.path(), 32, 0.0);
        let r = rmaos(d.path(), 32);
        let (s, ev) = run(plan(d.path(), &r, &n, bake, false));
        assert_eq!(s.converted, 1, "{ev:#?}");
    }
    for level in 1..=3 {
        let (baked, plain) = (mean_r(a.path(), level), mean_r(b.path(), level));
        assert!(baked > plain + 2.0, "mip {level}: {baked} vs {plain}");
    }
    // 1:1, so mip 0 has no variance: both encode the same constant.
    assert!((mean_r(a.path(), 0) - mean_r(b.path(), 0)).abs() < 1.0);
    let m = Manifest::at(&a.path().join("out")).load().unwrap();
    let e = m.values().next().unwrap();
    assert_eq!(e.sources.len(), 1, "{:?}", e.sources);
    assert!(e.sources[0].path.ends_with("iron_n.png"));
    assert_eq!(
        e.sources[0].sha256,
        sha256_file(&a.path().join("in/iron_n.png")).unwrap()
    );
    assert!(e.sources[0].bake.is_some());
    let plain = Manifest::at(&b.path().join("out")).load().unwrap();
    assert!(plain.values().next().unwrap().sources.is_empty());
}

#[test]
fn a_two_to_one_normal_bakes_into_mip_0_too() {
    let d = tempfile::tempdir().unwrap();
    let n = bumpy_n(d.path(), 64, 0.0);
    let r = rmaos(d.path(), 32);
    let mut p = plan(d.path(), &r, &n, true, false);
    p.jobs[0].bake.as_mut().unwrap().shift = 1;
    assert_eq!(run(p).0.converted, 1);
    assert!(mean_r(d.path(), 0) > 102.0, "{}", mean_r(d.path(), 0));
}

/// Controller correction to R1: the bake reads the `_n`'s mip 0 as written. A uniformly short flat `_n` (|n| = 0.5,
/// every texel pointing up) adds no roughness under the default `renormalize`, and adds roughness under `keep`.
#[test]
fn a_uniformly_short_flat_normal_adds_roughness_only_under_keep() {
    let mean_mip_1 = |base: Option<NormalBase>| {
        let d = tempfile::tempdir().unwrap();
        let n = d.path().join("in/iron_n.png");
        std::fs::create_dir_all(n.parent().unwrap()).unwrap();
        // (128, 128, 191) decodes to about (0.004, 0.004, 0.498): length 0.5, pointing up.
        ImageBuffer::from_fn(32, 32, |_, _| Rgb([128u8, 128, 191]))
            .save(&n)
            .unwrap();
        let r = rmaos(d.path(), 32);
        let mut p = plan(d.path(), &r, &n, base.is_some(), false);
        if let Some(b) = base {
            p.jobs[0].bake.as_mut().unwrap().normal_profile.mips.normal_base = b;
        }
        assert_eq!(run(p).0.converted, 1);
        mean_r(d.path(), 1)
    };
    let plain = mean_mip_1(None);
    let default = mean_mip_1(Some(NormalBase::Renormalize));
    let keep = mean_mip_1(Some(NormalBase::Keep));
    assert!(
        (default - plain).abs() < 1.0,
        "default {default} vs unbaked {plain}"
    );
    assert!(keep > plain + 2.0, "keep {keep} vs unbaked {plain}");
}

#[test]
fn up_to_date_until_the_normal_or_its_decode_settings_change() {
    let d = tempfile::tempdir().unwrap();
    let n = bumpy_n(d.path(), 32, 0.0);
    let r = rmaos(d.path(), 32);
    assert_eq!(run(plan(d.path(), &r, &n, true, false)).0.converted, 1);
    let (s, _) = run(plan(d.path(), &r, &n, true, false));
    assert_eq!((s.converted, s.up_to_date), (0, 1));
    // The `_n`'s green flip is part of the bake's identity.
    let (s, _) = run(plan(d.path(), &r, &n, true, true));
    assert_eq!((s.converted, s.up_to_date), (1, 0));
    let (s, _) = run(plan(d.path(), &r, &n, true, true));
    assert_eq!((s.converted, s.up_to_date), (0, 1));
    // New `_n` bytes.
    bumpy_n(d.path(), 32, 1.0);
    let (s, _) = run(plan(d.path(), &r, &n, true, true));
    assert_eq!((s.converted, s.up_to_date), (1, 0));
    // Turning the bake off re-encodes too.
    let (s, _) = run(plan(d.path(), &r, &n, false, true));
    assert_eq!((s.converted, s.up_to_date), (1, 0));
}

/// Review Focus 3: the `_n` is a source of the member (final review I7).
#[test]
fn a_normal_changed_after_planning_fails_the_member() {
    let d = tempfile::tempdir().unwrap();
    let n = bumpy_n(d.path(), 32, 0.0);
    let r = rmaos(d.path(), 32);
    let p = plan(d.path(), &r, &n, true, false);
    bumpy_n(d.path(), 64, 0.0); // another size: the stamp no longer matches
    let (s, ev) = run(p);
    assert_eq!((s.converted, s.failed), (0, 1));
    let err = ev
        .iter()
        .find_map(|e| match &e.kind {
            EventKind::FileFailed {
                stage: Stage::Load,
                error,
            } => Some(error.clone()),
            _ => None,
        })
        .unwrap();
    assert!(
        err.contains("iron_n.png") && err.contains("changed since planning"),
        "{err}"
    );
    assert!(!d.path().join("out/iron_rmaos.dds").exists());
}
```

In `crates/engine/src/bake/mod.rs` `mod tests`, add (and `use crate::config::{Format, NormalBase};` at the top of `mod tests`):

```rust
    #[test]
    fn a_bake_holds_24_bytes_per_normal_texel() {
        let d = tempfile::tempdir().unwrap();
        let n = d.path().join("iron_n.png");
        ::image::RgbImage::new(64, 32).save(&n).unwrap();
        let b = BakeJob {
            normal: n,
            normal_profile: profile(Subtype::PbrNormal),
            shift: 0,
            stamp: None,
        };
        assert_eq!(b.estimated_bytes(), 64 * 32 * 24);
    }

    #[test]
    fn the_bake_identity_follows_decode_settings_and_shift_only() {
        let b = BakeJob {
            normal: "a/iron_n.png".into(),
            normal_profile: profile(Subtype::PbrNormal),
            shift: 0,
            stamp: None,
        };
        let mut moved = b.clone();
        moved.normal = "b/iron_n.png".into();
        let mut format = b.clone();
        format.normal_profile.format = Format::Bc1;
        let mut flip = b.clone();
        flip.normal_profile.channels.flip_green = true;
        let mut shift = b.clone();
        shift.shift = 1;
        let mut keep = b.clone();
        keep.normal_profile.mips.normal_base = NormalBase::Keep;
        assert_eq!(b.identity_sha256(), moved.identity_sha256());
        assert_eq!(b.identity_sha256(), format.identity_sha256());
        assert_ne!(b.identity_sha256(), flip.identity_sha256());
        assert_ne!(b.identity_sha256(), shift.identity_sha256());
        assert_ne!(b.identity_sha256(), keep.identity_sha256());
    }
```

- [ ] **Step 2: Run the tests to see them fail.**

Run: `cargo test -p engine --test bake_run`
Expected: FAIL to compile. `BakeJob` and `ResolvedJob::bake` do not exist.

- [ ] **Step 3: Implement.**

`crates/engine/src/bake/mod.rs`:
- Extend the imports:

```rust
use crate::config::{MipKeyword, MipLevels, Profile};
use crate::image::Rgba32F;
use crate::policy::SourceStamp;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
```

- Add after `NormalVariance`'s `impl`:

```rust
/// How a roughness member bakes its set's `_n` (`ResolvedJob::bake`, spec §6.2). Set by `plan::plan_bakes`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct BakeJob {
    /// The set's `pbr_normal` source.
    pub normal: PathBuf,
    /// That `_n`'s resolved profile: the member decodes the `_n` with it exactly as the `_n`'s own job does
    /// (swizzle, green flip, Z rebuild; DECISIONS R4).
    pub normal_profile: Profile,
    /// log2(`_n` size / member size) ([`bake_shift`]) as planned.
    pub shift: i32,
    /// The `_n`'s size and modification time when `Engine::plan` read it (final review I7); `None` is not checked.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stamp: Option<SourceStamp>,
}

impl BakeJob {
    /// What the member's output depends on besides the `_n`'s bytes: the decode settings, how mip 0 is written, and
    /// the level shift (DECISIONS R12). Recorded in the manifest so `if-changed` re-encodes when any changes; the
    /// `_n`'s path and its own output settings (format, effort) are not part of it.
    pub fn identity_sha256(&self) -> String {
        crate::manifest::sha256_json(&(
            &self.normal_profile.channels,
            self.normal_profile.alpha.policy,
            self.normal_profile.mips.normal_base,
            self.shift,
        ))
        .unwrap_or_default()
    }

    /// Decode the `_n` as its own job does (spec §5.4), write its mip 0 as that job does (unit length unless
    /// `mips.normal_base = keep`, DECISIONS R1) and keep only the footprint lengths of that written base. The image is
    /// dropped before this returns (DECISIONS R4).
    pub fn load_lengths(&self) -> Result<LengthPyramid, String> {
        let (img, _) = crate::load::load_image(&self.normal).map_err(|e| e.to_string())?;
        let mut work = crate::ops::preprocess(img, &self.normal_profile);
        if self.normal_profile.mips.normal_base == crate::config::NormalBase::Renormalize {
            for px in &mut work.img.px {
                crate::mips::normalize_vector(px, work.domain);
            }
        }
        Ok(LengthPyramid::build(&work.img))
    }

    /// Peak bytes the bake holds, for `--mem-budget` admission (DECISIONS R4): the `_n`'s float base (16 per texel),
    /// one quarter-size accumulation level (4), the pyramid (1.33) and decode slack, so 24 per `_n` texel. 0 when
    /// the header cannot be read (the load then fails with its own error).
    pub fn estimated_bytes(&self) -> u64 {
        ::image::image_dimensions(&self.normal)
            .map(|(w, h)| u64::from(w) * u64::from(h) * 24)
            .unwrap_or(0)
    }
}
```

`crates/engine/src/policy.rs`:
- In `ResolvedJob`, after `pack`, add:

```rust
    /// A roughness member's normal-variance bake (spec §6.2, DECISIONS R4): the `_n` it reads and how. `None` for
    /// every other job and for a member that does not bake.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bake: Option<crate::bake::BakeJob>,
```

- Add `bake: None,` after `pack: None,` in every `ResolvedJob { … }` literal: `packing_input_job`, both literals in `resolve_decided` (the error return and the final one), `crates/engine/src/pack/plan.rs`, the `api.rs` test module, `crates/engine/tests/events_manifest.rs` and `crates/engine/tests/pack_run.rs`. `cargo build --workspace --all-targets` lists any literal missed.

`crates/engine/src/manifest.rs`:
- In `SourceRef`, after `invert`, add:

```rust
    /// The `_n` a roughness member baked its variance from (DECISIONS R12): the SHA-256 of its decode settings and
    /// level shift (`BakeJob::identity_sha256`). Absent on every other source.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bake: Option<String>,
```

- In `SourceRef::of`, add `bake: None,`.
- After `of_pack`, add:

```rust
    /// The `_n` a roughness member bakes from, with its bake identity (DECISIONS R12).
    pub fn of_bake(path: &Path, identity: String) -> io::Result<SourceRef> {
        Ok(SourceRef {
            bake: Some(identity),
            ..SourceRef::of(path)?
        })
    }
```

- Change the doc comment of `ManifestRecord::sources` to: "Every source of a multi-source recipe, then the `_n` a roughness member baked from (DECISIONS R12); empty for a single-source output that does not bake (`source` is then the only one)."
- Add `bake: None,` to the `SourceRef { … }` literals in the `api.rs` test module and in `crates/engine/tests/events_manifest.rs` (two).

`crates/engine/src/pipeline.rs`:
- Imports: replace `use crate::mips::build_chain;` with `use crate::mips::build_chain_with;` and add `use crate::bake::NormalVariance;`.
- Replace `check_stamps` with:

```rust
/// Source stamps (final review I7): the plan's decisions (alpha policy, sub-type facts, ...) hold only for the
/// file it analysed; a packed job checks every source, and a baking roughness member its `_n` too (M4 carry).
fn check_stamps(job: &ResolvedJob) -> Result<(), FileError> {
    let mut stamps: Vec<(&Path, Option<SourceStamp>)> = match &job.pack {
        Some(pk) => pk
            .sources
            .iter()
            .map(|s| (s.path.as_path(), s.stamp))
            .collect(),
        None => vec![(job.input.as_path(), job.source)],
    };
    if let Some(b) = &job.bake {
        stamps.push((b.normal.as_path(), b.stamp));
    }
    let named = job.pack.is_some() || job.bake.is_some();
    for (src, planned) in stamps {
        if let Some(planned) = planned {
            if SourceStamp::of(src) != Some(planned) {
                let what = if named {
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
    Ok(())
}
```

- In `prepare_then`, replace

```rust
    check_stamps(job)?;
    let extra = after_stamps()?;
    let img = match &job.pack {
```

with

```rust
    check_stamps(job)?;
    let extra = after_stamps()?;
    // Spec §6.2–6.3 (DECISIONS R4): a roughness member decodes its `_n` first and keeps only the footprint lengths,
    // so the two images are never held together.
    let lengths = match &job.bake {
        Some(b) => Some(
            b.load_lengths()
                .map_err(|e| fail(Stage::Load)(format!("{}: {e}", b.normal.display())))?,
        ),
        None => None,
    };
    stop()?;
    let img = match &job.pack {
```

and directly after the `let img = match &job.pack { … };` statement (before its `stop()?;`) add:

```rust
    let bake = match (lengths, &job.bake) {
        (Some(l), Some(b)) => {
            let nv = NormalVariance::new(l, (img.width, img.height), p.mips.bake_clamp).map_err(
                |e| fail(Stage::Load)(format!("variance bake with {}: {e}", b.normal.display())),
            )?;
            if nv.shift() != b.shift {
                return Err(fail(Stage::Load)(format!(
                    "{}: size changed since planning; re-run",
                    b.normal.display()
                )));
            }
            Some(nv)
        }
        _ => None,
    };
```

- Replace `let mut chain = build_chain(&work, p);` with:

```rust
    let mut chain = build_chain_with(&work, p, bake.as_ref());
    drop(bake);
```

- In `process_file`'s `after_stamps` closure, change `let sources: Vec<SourceRef> = match &job.pack {` to `let mut sources: Vec<SourceRef> = match &job.pack {`. Before `Ok((source_sha256, sources))`, add:

```rust
            // DECISIONS R12: the baked `_n`, with the identity of its decode settings.
            if let Some(b) = &job.bake {
                sources.push(
                    SourceRef::of_bake(&b.normal, b.identity_sha256())
                        .map_err(|e| fail(Stage::Load)(e.to_string()))?,
                );
            }
```

`crates/engine/src/api.rs`:
- Replace `current_source_hashes` with:

```rust
/// Every recorded source of `job` now, in record order: the pack sources, then the baked `_n` (DECISIONS R12), as
/// (SHA-256, bake identity). Empty for an ordinary file that does not bake; `None` when a source cannot be hashed
/// (then the output is not up to date).
fn current_sources(job: &ResolvedJob) -> Option<Vec<(String, Option<String>)>> {
    let mut v = vec![];
    if let Some(pk) = &job.pack {
        for s in &pk.sources {
            v.push((sha256_file(&s.path).ok()?, None));
        }
    }
    if let Some(b) = &job.bake {
        v.push((sha256_file(&b.normal).ok()?, Some(b.identity_sha256())));
    }
    Some(v)
}
```

- In `pack_roles_match`, change `Some(pk) => e.sources.iter().map(|r| (r.role, r.invert)).eq(pk` to `Some(pk) => e.sources.iter().filter(|r| r.bake.is_none()).map(|r| (r.role, r.invert)).eq(pk`.
- In `is_up_to_date`, replace

```rust
        && current_source_hashes(job)
            .is_some_and(|h| h.iter().eq(e.sources.iter().map(|r| &r.sha256)))
```

with

```rust
        && current_sources(job).is_some_and(|now| {
            now.into_iter()
                .eq(e.sources.iter().map(|r| (r.sha256.clone(), r.bake.clone())))
        })
```

- Add after `estimated_bytes`:

```rust
/// [`estimated_bytes`] of the job's input, plus what its variance bake holds (DECISIONS R4; the M4 carry "the
/// summed-area table counts against `--mem-budget`", a length pyramid since R5).
fn job_bytes(job: &ResolvedJob) -> u64 {
    estimated_bytes(&job.input)
        + job
            .bake
            .as_ref()
            .map_or(0, crate::bake::BakeJob::estimated_bytes)
}
```

- In `run`'s admission loop, replace `let est = estimated_bytes(&job.input);` with `let est = job_bytes(job);`.

Regenerate the schemas: `cargo run -p sdc -- presets schema all --out schema`.

- [ ] **Step 4: Run the tests to see them pass.**

Run: `cargo test -p engine --test bake_run`, `cargo test -p engine --lib -- bake::tests api::tests`, `cargo test -p engine --test pack_run`, `cargo test -p engine --test events_manifest` and `cargo test -p engine --test schemas`
Expected: PASS.

- [ ] **Step 5: Run every gate and commit.**

```bash
cargo fmt --all && cargo clippy --workspace --all-targets -- -D warnings && cargo test --workspace
git add crates/engine/src/bake/mod.rs crates/engine/src/policy.rs crates/engine/src/pack/plan.rs crates/engine/src/pipeline.rs crates/engine/src/manifest.rs crates/engine/src/api.rs crates/engine/tests/bake_run.rs crates/engine/tests/events_manifest.rs crates/engine/tests/pack_run.rs schema
git commit -m "feat(engine): a roughness member bakes its _n at run time; the _n is a stamped, recorded, admitted source (M4)

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

---

### Task 8: Set-level bake decisions, the four `SET-*` codes, shared roughness, `set_resolved.bake`

**Files:**
- Create: `crates/engine/src/bake/plan.rs`
- Modify:
  - `crates/engine/src/bake/mod.rs` (`pub mod plan;`);
  - `crates/engine/src/api.rs` (`BatchRequest::shared_roughness`, `BatchRequest::new`, the `plan_bakes` call in `plan_cached`, `SetResolved { bake }`);
  - `crates/engine/tests/golden.rs` (two named expectations).
- Test: `crates/engine/tests/bake_plan.rs` (create)

**Interfaces:**
- Consumes:
  - `BakeJob`, `bake_shift` (Tasks 5, 7);
  - `Provenance::Set` (Task 3);
  - `group::split_suffix_with`, `discover::{InputFile, InputKind, IMAGE_EXTS}`;
  - `sets::set_key(output_root, id)`.
- Produces:
  - `bake::plan::{BakeCtx, plan_bakes}` (registry);
  - `BatchRequest::shared_roughness: BTreeSet<String>` (set ids; empty by default);
  - `set_resolved.bake` is now true for a set in which at least one member bakes.

**Rules** (R6–R10 and the roadmap carries "a borrowed (shared) roughness slot renormalises only and is never baked into" and "emit the four SET-* codes"):
- **Any target:**
  - An `_msn` whose set has no `s` suffix gets `SET-MISSING-COMPANION` ("_msn without an _s").
  - A `_flow` whose set has no `n` gets the same code ("_flow without an _n").
  - Both apply to jobs that are not `Error` or `PackingInput`.
- **`cs` only, per set:**
  - **Normals:** the set's `pbr_normal` jobs (not `Error`), sorted by input path.
  - **Members:** its `rmaos`/`skin_rfaos` jobs (`is_roughness_member`), packed or pre-packed (not `Error` or `PackingInput`).
  - **Shared set** (its key is in the shared list): no bake and no missing-companion code.
  - **Members but no normal:** each member gets one of these:
    - `SET-MISSING-COMPANION`: an image `_n` member of the set has no `pbr_normal` job (failed, cancelled, duplicate, or another sub-type);
    - `SET-COMPANION-DDS`: a DDS `_n` member is in the set, or an `_n` DDS sits on disk outside the batch;
    - `SET-COMPANION-OUTSIDE`: an `_n` image sits on disk next to the member and outside the batch;
    - otherwise `SET-MISSING-COMPANION`.
  - **A normal but no members:** the first normal gets the same choice, with roughness companions. A companion here is `rmaos`, `rfaos`, or a raw roughness, glossiness, metallic or AO map that would pack one.
  - **Both:**
    - If the normal is not decided (`Ready`/`Blocked`), each member gets `SET-MISSING-COMPANION` ("not decided yet").
    - If the normal's `mips.normal` is `renormalize` with a provenance other than `builtin`, the user opted out: no bake, no code.
    - Otherwise, per member:
      - sRGB content: skip (Task 3's `TAG-CONTENT-MISMATCH` explains it);
      - `bake_shift` fails: `SET-BAKE-RATIO` (warning, data `{normal: [w, h], member: [w, h]}`);
      - else attach `BakeJob { normal, normal_profile, shift, stamp: normal job's source }`.
    - If any member bakes, the normal's `mips.normal` becomes `renormalize_bake` with provenance `set` (when it was not already `renormalize_bake`), and the set key goes into the returned set.
- Codes are appended to the job's `warnings`, so `file_planned` and the warning events carry them.

**Golden expectations that change (named):**
- `near_opaque_normal_alpha_is_removed_with_an_info`: a `pbr_normal` alone in `textures/pbr` now also raises `SET-MISSING-COMPANION`.
  - `assert_eq!(warnings(&o), vec![Code::AlphaNearOpaque]);` becomes `vec![Code::AlphaNearOpaque, Code::SetMissingCompanion]`.
  - `assert_eq!(warnings(&o), vec![Code::AlphaPbrNormalDropped]);` becomes `vec![Code::AlphaPbrNormalDropped, Code::SetMissingCompanion]`.
- No other golden expectation changes:
  - `normal_maps_are_bc7_under_cs_and_bc5_only_for_flow` gains infos for `h_flow` and a bake for `iron_rmaos`, but asserts no warning list;
  - `cs_pbr_albedo_is_srgb_and_vanilla_refuses_it`, `bc6h_emissive_in_a_pbr_set` and `srgb_under_lod_blocks_until_allowed` hold no normal or roughness member, so they raise nothing (R8).

**GUI fixtures (checked, no expectation changes):**
- In `crates/gui/tests/support::opened()`, `rock01_normal.png` is Unresolved (its green convention is unstated). The packed `rock01_rmaos` therefore gains a `SET-MISSING-COMPANION` info ("not decided yet").
- No GUI test pins an item's warning list. The tests that look at warnings set them by hand (`model.rs`, `set_editor_ui.rs` `with_only_warnings`), and the amber badge and the "Has warnings" filter ignore Info.
- `hovering_a_pills_warning_badge_lists_the_warnings` sums every warning of rock01's members under `pbr_opened()`. There the `_n` is decided and the packed `_rmaos` bakes, so no `SET-*` code is added and its sum still equals the badge.

- [ ] **Step 1: Write the failing tests.** Create `crates/engine/tests/bake_plan.rs`:

```rust
//! M4 Task 8: which sets bake (spec §6.2–6.3), the four SET-* codes, opting out and shared roughness slots
//! (DECISIONS R6–R10).
use engine::api::*;
use engine::codes::{Code, Severity};
use engine::config::*;
use engine::events::*;
use engine::overrides::parse_set;
use engine::policy::{AcceptSuggestions, ResolvedJob};
use engine::sets::SetEdit;
use image::{ImageBuffer, Luma, Rgb, Rgba};
use std::path::Path;
use std::sync::Arc;

/// An `_n` whose columns alternate between two tilts (unit length, short footprints).
fn normal(dir: &Path, rel: &str, w: u32, h: u32) {
    let p = dir.join("in").join(rel);
    std::fs::create_dir_all(p.parent().unwrap()).unwrap();
    ImageBuffer::from_fn(w, h, |x, _| {
        Rgb([if x % 2 == 0 { 204u8 } else { 51 }, 128, 230])
    })
    .save(p)
    .unwrap();
}

fn rough(dir: &Path, rel: &str, w: u32, h: u32) {
    let p = dir.join("in").join(rel);
    std::fs::create_dir_all(p.parent().unwrap()).unwrap();
    ImageBuffer::from_fn(w, h, |x, _| Rgba([(x * 4) as u8, 0, 200, 255]))
        .save(p)
        .unwrap();
}

fn gray(dir: &Path, rel: &str, v: u8) {
    let p = dir.join("in").join(rel);
    std::fs::create_dir_all(p.parent().unwrap()).unwrap();
    ImageBuffer::from_fn(64, 64, |x, _| Luma([v.wrapping_add(x as u8)]))
        .save(p)
        .unwrap();
}

fn fake_dds(dir: &Path, rel: &str) {
    let p = dir.join("in").join(rel);
    std::fs::create_dir_all(p.parent().unwrap()).unwrap();
    std::fs::write(p, b"DDS not a real texture").unwrap();
}

fn req(dir: &Path, sets: &[&str]) -> BatchRequest {
    let mut r = BatchRequest::new(vec![dir.join("in")], dir.join("out"), Target::Cs);
    r.recursive = true;
    r.accept = AcceptSuggestions::High;
    r.overrides = sets.iter().map(|s| parse_set(s).unwrap()).collect();
    r
}

fn plan(r: BatchRequest) -> (Plan, Vec<EngineEvent>) {
    let sink = Arc::new(VecSink::default());
    let rep = Reporter::new(sink.clone(), "t");
    let p = Engine::new()
        .unwrap()
        .plan(r, &rep, &CancelToken::default())
        .unwrap();
    (p, sink.take())
}

fn job<'a>(p: &'a Plan, tail: &str) -> &'a ResolvedJob {
    p.jobs
        .iter()
        .find(|j| j.input.to_string_lossy().replace('\\', "/").ends_with(tail))
        .unwrap_or_else(|| panic!("no job for {tail}"))
}

fn codes(j: &ResolvedJob) -> Vec<Code> {
    j.warnings.iter().map(|w| w.code).collect()
}

fn set_codes(j: &ResolvedJob) -> Vec<Code> {
    codes(j)
        .into_iter()
        .filter(|c| c.as_str().starts_with("SET-"))
        .collect()
}

/// `set_resolved.bake` of the set whose key ends with `tail`.
fn bake_flag(ev: &[EngineEvent], tail: &str) -> bool {
    ev.iter()
        .find_map(|e| match &e.kind {
            EventKind::SetResolved { bake, .. }
                if e.set.as_deref().is_some_and(|s| s.ends_with(tail)) =>
            {
                Some(*bake)
            }
            _ => None,
        })
        .unwrap_or_else(|| panic!("no set_resolved for {tail}"))
}

#[test]
fn an_n_and_its_rmaos_bake_and_the_n_records_the_set_decision() {
    let d = tempfile::tempdir().unwrap();
    normal(d.path(), "textures/pbr/iron_n.png", 64, 64);
    rough(d.path(), "textures/pbr/iron_rmaos.png", 64, 64);
    let (p, ev) = plan(req(d.path(), &[]));
    let b = job(&p, "iron_rmaos.png")
        .bake
        .as_ref()
        .expect("the rmaos bakes");
    assert!(b.normal.ends_with("iron_n.png"));
    assert_eq!(b.shift, 0);
    assert!(b.stamp.is_some());
    let n = job(&p, "iron_n.png");
    assert_eq!(n.profile.mips.normal, NormalMips::RenormalizeBake);
    assert_eq!(n.provenance["mips.normal"].to_string(), "set");
    assert!(bake_flag(&ev, "textures/pbr/iron"));
    assert!(set_codes(n).is_empty(), "{:?}", n.warnings);
    assert!(set_codes(job(&p, "iron_rmaos.png")).is_empty());
}

#[test]
fn rmaos_and_rfaos_in_one_set_both_bake() {
    let d = tempfile::tempdir().unwrap();
    normal(d.path(), "textures/pbr/skin_n.png", 64, 64);
    rough(d.path(), "textures/pbr/skin_rmaos.png", 64, 64);
    rough(d.path(), "textures/pbr/skin_rfaos.png", 64, 64);
    let (p, _) = plan(req(d.path(), &[]));
    assert!(job(&p, "skin_rmaos.png").bake.is_some());
    assert!(job(&p, "skin_rfaos.png").bake.is_some());
}

#[test]
fn a_packed_rmaos_bakes_like_a_prepacked_one() {
    let d = tempfile::tempdir().unwrap();
    normal(d.path(), "textures/pbr/iron_n.png", 64, 64);
    gray(d.path(), "textures/pbr/iron_roughness.png", 120);
    gray(d.path(), "textures/pbr/iron_ao.png", 200);
    let (p, ev) = plan(req(d.path(), &[]));
    let packed = p
        .jobs
        .iter()
        .find(|j| j.pack.is_some())
        .expect("a packed rmaos");
    assert!(packed.bake.is_some(), "{:?}", packed.warnings);
    assert!(bake_flag(&ev, "textures/pbr/iron"));
}

#[test]
fn size_ratios_give_the_level_shift() {
    for ((nw, nh), (mw, mh), shift) in [
        ((128u32, 128u32), (64u32, 64u32), 1i32),
        ((32, 32), (64, 64), -1),
        ((128, 64), (64, 32), 1),
    ] {
        let d = tempfile::tempdir().unwrap();
        normal(d.path(), "textures/pbr/iron_n.png", nw, nh);
        rough(d.path(), "textures/pbr/iron_rmaos.png", mw, mh);
        let (p, _) = plan(req(d.path(), &[]));
        let m = job(&p, "iron_rmaos.png");
        assert_eq!(
            m.bake.as_ref().map(|b| b.shift),
            Some(shift),
            "{nw}x{nh} / {mw}x{mh}: {:?}",
            m.warnings
        );
    }
}

/// Review Focus 1.
#[test]
fn an_aspect_mismatch_is_set_bake_ratio_and_does_not_bake() {
    let d = tempfile::tempdir().unwrap();
    normal(d.path(), "textures/pbr/iron_n.png", 64, 64);
    rough(d.path(), "textures/pbr/iron_rmaos.png", 64, 32);
    let (p, ev) = plan(req(d.path(), &[]));
    let m = job(&p, "iron_rmaos.png");
    assert!(m.bake.is_none());
    let w = m
        .warnings
        .iter()
        .find(|w| w.code == Code::SetBakeRatio)
        .expect("SET-BAKE-RATIO");
    assert_eq!(w.severity, Severity::Warning);
    assert_eq!(w.data["member"], serde_json::json!([64, 32]));
    assert!(!bake_flag(&ev, "textures/pbr/iron"));
    assert_eq!(
        job(&p, "iron_n.png").profile.mips.normal,
        NormalMips::Renormalize
    );
}

#[test]
fn a_lone_n_or_a_lone_rmaos_in_a_pbr_set_is_set_missing_companion_info() {
    let d = tempfile::tempdir().unwrap();
    normal(d.path(), "textures/pbr/iron_n.png", 64, 64);
    rough(d.path(), "textures/pbr/steel_rmaos.png", 64, 64);
    let (p, _) = plan(req(d.path(), &[]));
    for (tail, needle) in [
        ("iron_n.png", "renormalised only"),
        ("steel_rmaos.png", "without the variance bake"),
    ] {
        let j = job(&p, tail);
        assert_eq!(set_codes(j), vec![Code::SetMissingCompanion], "{tail}");
        let w = &j.warnings[j.warnings.len() - 1];
        assert_eq!(w.severity, Severity::Info);
        assert!(w.message.contains(needle), "{tail}: {}", w.message);
    }
}

#[test]
fn flow_without_n_and_msn_without_s_are_set_missing_companion() {
    let d = tempfile::tempdir().unwrap();
    normal(d.path(), "textures/hair/h_flow.png", 64, 64);
    normal(d.path(), "textures/a/w_msn.png", 64, 64);
    let (p, _) = plan(req(d.path(), &[]));
    assert_eq!(set_codes(job(&p, "h_flow.png")), vec![Code::SetMissingCompanion]);
    assert_eq!(set_codes(job(&p, "w_msn.png")), vec![Code::SetMissingCompanion]);
}

/// Review Focus 2.
#[test]
fn a_dds_companion_is_set_companion_dds_in_both_directions() {
    let d = tempfile::tempdir().unwrap();
    normal(d.path(), "textures/pbr/iron_n.png", 64, 64);
    fake_dds(d.path(), "textures/pbr/iron_rmaos.dds");
    fake_dds(d.path(), "textures/pbr/steel_n.dds");
    rough(d.path(), "textures/pbr/steel_rmaos.png", 64, 64);
    let (p, _) = plan(req(d.path(), &[]));
    for tail in ["iron_n.png", "steel_rmaos.png"] {
        let j = job(&p, tail);
        assert_eq!(set_codes(j), vec![Code::SetCompanionDds], "{tail}");
        let w = j.warnings.iter().find(|w| w.code == Code::SetCompanionDds).unwrap();
        assert_eq!(w.severity, Severity::Warning);
        assert!(
            w.data["companion"].as_str().unwrap().ends_with(".dds"),
            "{tail}: {}",
            w.data
        );
    }
    assert!(job(&p, "steel_rmaos.png").bake.is_none());
}

/// Review Focus 2: converting one file of a set leaves the other on disk.
#[test]
fn a_companion_on_disk_but_not_in_the_batch_is_set_companion_outside() {
    for (given, other) in [
        ("iron_n.png", "iron_rmaos.png"),
        ("iron_rmaos.png", "iron_n.png"),
    ] {
        let d = tempfile::tempdir().unwrap();
        normal(d.path(), "textures/pbr/iron_n.png", 64, 64);
        rough(d.path(), "textures/pbr/iron_rmaos.png", 64, 64);
        let mut r = req(d.path(), &[]);
        r.inputs = vec![d.path().join("in/textures/pbr").join(given)];
        let (p, _) = plan(r);
        let j = job(&p, given);
        assert_eq!(set_codes(j), vec![Code::SetCompanionOutside], "{given}");
        let w = j
            .warnings
            .iter()
            .find(|w| w.code == Code::SetCompanionOutside)
            .unwrap();
        assert!(
            w.data["companion"].as_str().unwrap().ends_with(other),
            "{given}: {}",
            w.data
        );
        assert!(j.bake.is_none());
    }
}

/// Review Focus 2: a member moved into the set from another folder bakes from the `_n`'s real path.
#[test]
fn a_member_from_another_folder_bakes_from_its_own_path() {
    let d = tempfile::tempdir().unwrap();
    normal(d.path(), "textures/pbr/iron_n.png", 64, 64);
    rough(d.path(), "other/iron_rmaos.png", 64, 64);
    let mut r = req(d.path(), &[]);
    r.set_edits = vec![SetEdit::Move {
        file: "other/iron_rmaos.png".into(),
        to: "textures/pbr/iron".into(),
    }];
    let (p, _) = plan(r);
    let b = job(&p, "other/iron_rmaos.png")
        .bake
        .as_ref()
        .expect("the moved rmaos bakes");
    assert!(b.normal.ends_with("textures/pbr/iron_n.png"));
}

#[test]
fn an_explicit_renormalize_on_the_n_opts_out() {
    let d = tempfile::tempdir().unwrap();
    normal(d.path(), "textures/pbr/iron_n.png", 64, 64);
    rough(d.path(), "textures/pbr/iron_rmaos.png", 64, 64);
    let (p, ev) = plan(req(
        d.path(),
        &["file:**/iron_n.png.mips.normal=renormalize"],
    ));
    assert!(job(&p, "iron_rmaos.png").bake.is_none());
    assert!(set_codes(job(&p, "iron_rmaos.png")).is_empty());
    assert_eq!(
        job(&p, "iron_n.png").provenance["mips.normal"].to_string(),
        "user"
    );
    assert!(!bake_flag(&ev, "textures/pbr/iron"));
}

/// M3b spec §3.3 (v1 instruction): a shared roughness renormalises only, and its borrower reports nothing missing.
#[test]
fn a_shared_roughness_is_never_baked_and_reports_nothing() {
    let d = tempfile::tempdir().unwrap();
    normal(d.path(), "textures/pbr/iron_n.png", 64, 64);
    rough(d.path(), "textures/pbr/iron_rmaos.png", 64, 64);
    normal(d.path(), "textures/pbr/steel_n.png", 64, 64);
    let mut r = req(d.path(), &[]);
    r.shared_roughness = ["textures/pbr/iron", "textures/pbr/steel"]
        .into_iter()
        .map(String::from)
        .collect();
    let (p, ev) = plan(r);
    assert!(job(&p, "iron_rmaos.png").bake.is_none());
    assert!(set_codes(job(&p, "steel_n.png")).is_empty());
    assert!(set_codes(job(&p, "iron_rmaos.png")).is_empty());
    assert!(!bake_flag(&ev, "textures/pbr/iron"));
}

#[test]
fn an_undecided_n_does_not_bake_and_says_so() {
    let d = tempfile::tempdir().unwrap();
    // `_normal` states no green convention, so the `_n` is unresolved (DECISIONS Note H).
    normal(d.path(), "textures/pbr/iron_normal.png", 64, 64);
    rough(d.path(), "textures/pbr/iron_rmaos.png", 64, 64);
    let (p, _) = plan(req(d.path(), &[]));
    let m = job(&p, "iron_rmaos.png");
    assert!(m.bake.is_none());
    let w = m
        .warnings
        .iter()
        .find(|w| w.code == Code::SetMissingCompanion)
        .expect("SET-MISSING-COMPANION");
    assert!(w.message.contains("not decided"), "{}", w.message);
}

#[test]
fn a_blocked_bc5_n_still_bakes_from_its_source() {
    let d = tempfile::tempdir().unwrap();
    normal(d.path(), "textures/pbr/iron_n.png", 64, 64);
    rough(d.path(), "textures/pbr/iron_rmaos.png", 64, 64);
    let (p, _) = plan(req(
        d.path(),
        &[
            "file:**/iron_n.png.format=bc5",
            "file:**/iron_n.png.encoder.backend=rgbcx",
        ],
    ));
    assert!(codes(job(&p, "iron_n.png")).contains(&Code::FmtBc5Normal));
    assert!(job(&p, "iron_rmaos.png").bake.is_some());
}

#[test]
fn an_srgb_roughness_member_does_not_bake() {
    let d = tempfile::tempdir().unwrap();
    normal(d.path(), "textures/pbr/iron_n.png", 64, 64);
    rough(d.path(), "textures/pbr/iron_rmaos.png", 64, 64);
    let (p, _) = plan(req(
        d.path(),
        &["file:**/iron_rmaos.png.color.content=srgb"],
    ));
    let m = job(&p, "iron_rmaos.png");
    assert!(m.bake.is_none());
    assert!(codes(m).contains(&Code::TagContentMismatch));
}

#[test]
fn vanilla_neither_bakes_nor_reports_bake_companions() {
    let d = tempfile::tempdir().unwrap();
    normal(d.path(), "textures/pbr/iron_n.png", 64, 64);
    let mut r = req(d.path(), &[]);
    r.target = Some(Target::Vanilla);
    let (p, _) = plan(r);
    let n = job(&p, "iron_n.png");
    assert_eq!(n.profile.subtype, Subtype::Normal);
    assert!(set_codes(n).is_empty(), "{:?}", n.warnings);
}
```

In `crates/engine/tests/golden.rs`, `near_opaque_normal_alpha_is_removed_with_an_info`, change the two lines named above.

- [ ] **Step 2: Run the tests to see them fail.**

Run: `cargo test -p engine --test bake_plan`
Expected: FAIL to compile (`BatchRequest::shared_roughness` does not exist). With it stubbed, every test that expects a bake or a `SET-*` code fails.

- [ ] **Step 3: Implement.** Create `crates/engine/src/bake/plan.rs`:

```rust
//! Set-level bake decisions (spec §6.2–6.3, DECISIONS R6–R10): which roughness member bakes which `_n`, and the
//! four `SET-*` codes. `Engine::plan` runs this after packing, so a packed `_rmaos`/`_rfaos` is a member like a
//! pre-packed one, and before `set_resolved` and `file_planned`, so both show the decision.
use super::{bake_shift, BakeJob};
use crate::codes::{Code, Warning};
use crate::config::{ColorContent, NormalMips, RawMap, Subtype, Target};
use crate::discover::{InputFile, InputKind, IMAGE_EXTS};
use crate::group::{split_suffix_with, TextureSet};
use crate::policy::{JobStatus, Provenance, ResolvedJob};
use serde_json::json;
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

/// What `plan_bakes` reads besides the jobs.
pub struct BakeCtx<'a> {
    pub target: Target,
    pub sets: &'a [TextureSet],
    pub files: &'a [InputFile],
    /// Preset suffix aliases (`PresetRegistry::alias_table`), for reading companions' names.
    pub aliases: &'a BTreeMap<String, String>,
    /// Keys of the sets whose roughness slot is shared (`BatchRequest::shared_roughness`, as set keys; DECISIONS R7).
    pub shared: &'a BTreeSet<String>,
}

fn is_normal_suffix(s: &str) -> bool {
    s == "n"
}

/// Canonical file suffixes (not sub-type ids): `rmaos`, `rfaos` (the `skin_rfaos` output keeps the `_rfaos` suffix),
/// or a raw map that would pack one (roughness, glossiness, metallic, AO).
fn is_roughness_suffix(s: &str) -> bool {
    s == "rmaos"
        || s == "rfaos"
        || RawMap::ALL
            .iter()
            .any(|r| r.starts_rmaos() && r.id() == s)
}

/// The base name and canonical suffix of a file name (aliases, vocabulary and §6.1 suffixes, as Group reads them).
fn base_and_suffix(
    path: &Path,
    aliases: &BTreeMap<String, String>,
) -> Option<(String, Option<String>)> {
    let stem = path.file_stem()?.to_string_lossy().into_owned();
    let s = split_suffix_with(&stem, aliases);
    Some((s.base, s.suffix))
}

/// Ready, or Blocked only by codes the user must acknowledge: the profile is final (DECISIONS R9).
fn decided(j: &ResolvedJob) -> bool {
    matches!(j.status, JobStatus::Ready | JobStatus::Blocked { .. })
}

/// Resolved into something that is (or, once decided, will be) written.
fn writable(j: &ResolvedJob) -> bool {
    !matches!(
        j.status,
        JobStatus::Error { .. } | JobStatus::PackingInput { .. }
    )
}

fn note(j: &mut ResolvedJob, code: Code, msg: String, companion: Option<&Path>) {
    let data = companion.map_or(serde_json::Value::Null, |p| {
        json!({ "companion": p.to_string_lossy().replace('\\', "/") })
    });
    j.warnings.push(Warning::new(code, msg).with_data(data));
}

/// The set's member file whose kind is `kind` and whose canonical suffix passes `want`.
fn member_file<'a>(
    cx: &BakeCtx<'a>,
    set: &TextureSet,
    kind: InputKind,
    want: fn(&str) -> bool,
) -> Option<&'a InputFile> {
    set.members.iter().map(|&i| &cx.files[i]).find(|f| {
        f.kind == kind
            && base_and_suffix(&f.rel, cx.aliases)
                .and_then(|(_, s)| s)
                .is_some_and(|s| want(&s))
    })
}

/// A file next to `beside` with the same base name (case-insensitive) whose canonical suffix passes `want`, that the
/// batch does not hold: `(path, is_dds)`, the first by path.
fn sibling_outside(
    beside: &Path,
    want: fn(&str) -> bool,
    batch: &BTreeSet<PathBuf>,
    aliases: &BTreeMap<String, String>,
) -> Option<(PathBuf, bool)> {
    let (base, _) = base_and_suffix(beside, aliases)?;
    let dir = beside.parent()?;
    let mut hits: Vec<(PathBuf, bool)> = std::fs::read_dir(dir)
        .ok()?
        .filter_map(Result::ok)
        .map(|e| e.path())
        .filter(|p| p.is_file() && !batch.contains(p))
        .filter_map(|p| {
            let ext = p.extension()?.to_string_lossy().to_ascii_lowercase();
            let dds = ext == "dds";
            if !dds && !IMAGE_EXTS.contains(&ext.as_str()) {
                return None;
            }
            let (b, s) = base_and_suffix(&p, aliases)?;
            (b.eq_ignore_ascii_case(&base) && s.as_deref().is_some_and(want)).then_some((p, dds))
        })
        .collect();
    hits.sort();
    hits.into_iter().next()
}

/// Why a roughness member has no `_n` to bake from (spec §6.3, DECISIONS R8).
fn missing_normal(cx: &BakeCtx, set: &TextureSet, batch: &BTreeSet<PathBuf>, j: &mut ResolvedJob) {
    if let Some(f) = member_file(cx, set, InputKind::Image, is_normal_suffix) {
        let msg = format!(
            "the set's _n ({}) has no pbr_normal job (it failed, was cancelled, is a duplicate or was given another sub-type): the roughness is written without the variance bake",
            f.path.display()
        );
        note(j, Code::SetMissingCompanion, msg, Some(&f.path));
        return;
    }
    if let Some(f) = member_file(cx, set, InputKind::Dds, is_normal_suffix) {
        let msg = format!(
            "the set's _n is only a DDS ({}): DDS inputs are not re-encoded, so the roughness is written without the variance bake",
            f.path.display()
        );
        note(j, Code::SetCompanionDds, msg, Some(&f.path));
        return;
    }
    match sibling_outside(&j.input, is_normal_suffix, batch, cx.aliases) {
        Some((p, true)) => {
            let msg = format!(
                "the set's _n is only a DDS ({}) and is not in this batch: the roughness is written without the variance bake",
                p.display()
            );
            note(j, Code::SetCompanionDds, msg, Some(&p));
        }
        Some((p, false)) => {
            let msg = format!(
                "{} is on disk but not in this batch: the roughness is written without the variance bake; add it to the batch",
                p.display()
            );
            note(j, Code::SetCompanionOutside, msg, Some(&p));
        }
        None => note(
            j,
            Code::SetMissingCompanion,
            "no _n in this PBR set: the roughness is written without the variance bake".into(),
            None,
        ),
    }
}

/// Why a `pbr_normal` has no roughness member to bake into (spec §6.3, DECISIONS R8).
fn missing_roughness(
    cx: &BakeCtx,
    set: &TextureSet,
    batch: &BTreeSet<PathBuf>,
    j: &mut ResolvedJob,
) {
    if let Some(f) = member_file(cx, set, InputKind::Dds, is_roughness_suffix) {
        let msg = format!(
            "the set's roughness member is only a DDS ({}): DDS inputs are not re-encoded, so the normal is renormalised only",
            f.path.display()
        );
        note(j, Code::SetCompanionDds, msg, Some(&f.path));
        return;
    }
    match sibling_outside(&j.input, is_roughness_suffix, batch, cx.aliases) {
        Some((p, true)) => {
            let msg = format!(
                "the set's roughness member is only a DDS ({}) and is not in this batch: the normal is renormalised only",
                p.display()
            );
            note(j, Code::SetCompanionDds, msg, Some(&p));
        }
        Some((p, false)) => {
            let msg = format!(
                "{} is on disk but not in this batch: the normal is renormalised only; add it to bake its variance into the roughness",
                p.display()
            );
            note(j, Code::SetCompanionOutside, msg, Some(&p));
        }
        None => note(
            j,
            Code::SetMissingCompanion,
            "no _rmaos or _rfaos in this PBR set: the normal is renormalised only".into(),
            None,
        ),
    }
}

/// Bake every member from the `_n` at `n`. Returns whether any member bakes.
fn bake_set(jobs: &mut [ResolvedJob], n: usize, members: &[usize]) -> bool {
    if !decided(&jobs[n]) {
        let p = jobs[n].input.clone();
        let msg = format!(
            "the set's _n ({}) is not decided yet: no variance bake until it is",
            p.display()
        );
        for &m in members {
            note(&mut jobs[m], Code::SetMissingCompanion, msg.clone(), Some(&p));
        }
        return false;
    }
    // DECISIONS R6: an explicit `mips.normal = renormalize` on the `_n` (any layer but the built-in default) opts out.
    let nj = &jobs[n];
    if nj.profile.mips.normal == NormalMips::Renormalize
        && nj
            .provenance
            .get("mips.normal")
            .is_some_and(|p| *p != Provenance::Builtin)
    {
        return false;
    }
    let Ok(ndims) = ::image::image_dimensions(&nj.input) else {
        return false;
    };
    let (normal, normal_profile, stamp) = (nj.input.clone(), nj.profile.clone(), nj.source);
    let mut any = false;
    for &m in members {
        // DECISIONS R10: `check_profile` already raised TAG-CONTENT-MISMATCH for an sRGB roughness.
        if jobs[m].profile.color.content == ColorContent::Srgb {
            continue;
        }
        let Ok(mdims) = ::image::image_dimensions(&jobs[m].input) else {
            continue;
        };
        match bake_shift(ndims, mdims) {
            Ok(shift) => {
                jobs[m].bake = Some(BakeJob {
                    normal: normal.clone(),
                    normal_profile: normal_profile.clone(),
                    shift,
                    stamp,
                });
                any = true;
            }
            Err(why) => {
                let w = Warning::new(Code::SetBakeRatio, format!("variance bake off: {why}"))
                    .with_data(json!({
                        "normal": [ndims.0, ndims.1],
                        "member": [mdims.0, mdims.1],
                    }));
                jobs[m].warnings.push(w);
            }
        }
    }
    if any && jobs[n].profile.mips.normal != NormalMips::RenormalizeBake {
        jobs[n].profile.mips.normal = NormalMips::RenormalizeBake;
        jobs[n]
            .provenance
            .insert("mips.normal".into(), Provenance::Set);
    }
    any
}

/// Decide every set's bake (spec §6.2–6.3). Attaches `ResolvedJob::bake` to each baking roughness member, records
/// the `_n`'s `mips.normal = renormalize_bake` with provenance `set`, and appends the `SET-*` codes to the jobs they
/// concern. Returns the keys of the sets that bake (`set_resolved.bake`).
pub fn plan_bakes(cx: &BakeCtx, jobs: &mut [ResolvedJob]) -> BTreeSet<String> {
    let batch: BTreeSet<PathBuf> = cx.files.iter().map(|f| f.path.clone()).collect();
    let mut baking = BTreeSet::new();
    for set in cx.sets {
        let idx: Vec<usize> = (0..jobs.len())
            .filter(|&i| jobs[i].set_key == set.key)
            .collect();
        // Spec §10 SET-MISSING-COMPANION, the two triggers that are not about the bake (any target).
        for &i in &idx {
            let lacking = match jobs[i].profile.subtype {
                Subtype::Msn if !set.suffixes.contains("s") => {
                    Some("_msn without an _s (specular) in its set")
                }
                Subtype::Flow if !set.suffixes.contains("n") => {
                    Some("_flow without an _n in its set")
                }
                _ => None,
            };
            if let (Some(m), true) = (lacking, writable(&jobs[i])) {
                note(&mut jobs[i], Code::SetMissingCompanion, m.into(), None);
            }
        }
        if cx.target != Target::Cs {
            continue;
        }
        let shared = cx.shared.contains(&set.key);
        let mut normals: Vec<usize> = idx
            .iter()
            .copied()
            .filter(|&i| {
                jobs[i].profile.subtype == Subtype::PbrNormal
                    && !matches!(jobs[i].status, JobStatus::Error { .. })
            })
            .collect();
        normals.sort_by(|&a, &b| jobs[a].input.cmp(&jobs[b].input));
        let members: Vec<usize> = idx
            .iter()
            .copied()
            .filter(|&i| jobs[i].profile.subtype.is_roughness_member() && writable(&jobs[i]))
            .collect();
        match (normals.first().copied(), members.is_empty()) {
            (None, true) => {}
            // M3b spec §3.3 (v1 instruction, DECISIONS R7): a shared roughness renormalises only, silently.
            _ if shared => {}
            (None, false) => {
                for &m in &members {
                    missing_normal(cx, set, &batch, &mut jobs[m]);
                }
            }
            (Some(n), true) => missing_roughness(cx, set, &batch, &mut jobs[n]),
            (Some(n), false) => {
                if bake_set(jobs, n, &members) {
                    baking.insert(set.key.clone());
                }
            }
        }
    }
    baking
}
```

In `crates/engine/src/bake/mod.rs`, add `pub mod plan;` after the module doc comment.

`crates/engine/src/api.rs`:
- In `BatchRequest`, after `output_paths`, add:

```rust
    /// GUI (M3b spec §3.3, DECISIONS R7): ids of the sets whose roughness slot (`_rmaos`/`_rfaos`) is one output used
    /// by more than one set, owner and borrowers alike. The engine bakes no normal variance into it and reports no
    /// missing companion for those sets. Empty for the CLI.
    pub shared_roughness: BTreeSet<String>,
```

- In `BatchRequest::new`, add `shared_roughness: BTreeSet::new(),` after `output_paths`.
- In `plan_cached`, directly after the `for (c, slot) in packed.candidates.iter().zip(pack_slots) { … }` loop and before `let peak_admitted_bytes = …`, add:

```rust
        // Spec §6.2–6.3 (M4): the set-level variance bake, after packing so a packed roughness member counts, and
        // before `set_resolved` and `file_planned` so both carry the decision.
        let shared: BTreeSet<String> = req
            .shared_roughness
            .iter()
            .map(|id| crate::sets::set_key(&req.output_root, id))
            .collect();
        let baking = crate::bake::plan::plan_bakes(
            &crate::bake::plan::BakeCtx {
                target,
                sets: &sets,
                files: &files,
                aliases: registry.alias_table(),
                shared: &shared,
            },
            &mut jobs,
        );
```

- In the `EventKind::SetResolved { … }` emission, replace the two lines `// The variance bake arrives with milestone M4.` and `bake: false,` with `bake: baking.contains(&set.key),`.

- [ ] **Step 4: Run the tests to see them pass.**

Run: `cargo test -p engine --test bake_plan` and `cargo test -p engine --test golden`
Expected: PASS.

- [ ] **Step 5: Run every gate and commit.** If a test outside this task's list fails because it asserts an exact warning list that now holds a `SET-*` code, stop and report it (Global Constraints).

```bash
cargo fmt --all && cargo clippy --workspace --all-targets -- -D warnings && cargo test --workspace
git add crates/engine/src/bake/plan.rs crates/engine/src/bake/mod.rs crates/engine/src/api.rs crates/engine/tests/bake_plan.rs crates/engine/tests/golden.rs
git commit -m "feat(engine): set-level variance bake decisions and the four SET-* codes; shared roughness never bakes (M4)

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

---

### Task 9: Golden bake cases against the f64 reference (roadmap M4 exit)

**Files:**
- Modify: `crates/engine/tests/golden.rs` (new helpers and tests; `two_to_one_sets_keep_their_own_chains` updated)
- Test: the same file

**Interfaces:**
- Consumes:
  - `engine::session::Session::{new, preview_source}`, which returns `SourcePreview { first_level, dims, levels: Vec<Rgba32F> }`. These are the float chain the encoder receives, after the bake;
  - `ResolvedJob::bake` (Task 7) and `Plan::jobs`.
- Produces: nothing.

**The exit criterion (roadmap M4):**
- **Roughness bake:** golden cases `_n` + `_rmaos` and `_n` + `_rfaos` at 1:1 and 2:1, plus a half-size `_n`. Every R value of every level of the roughness chain must be within 1/255 of a reference implementation of the formula in f64, computed from the source PNGs here in test code.
- **Renormalise-only paths unchanged:**
  - a flat `_n` leaves the roughness chain bit-identical to no bake;
  - `renormalize_bake` on a non-PBR `normal` is inert.
- **`_cnr`:** a bumpy `_cnr` matches the reference within 1/255, and a flat `_cnr` keeps its alpha bit for bit.

**Golden expectations that change (named):** `two_to_one_sets_keep_their_own_chains`. Its doc comment no longer says "the MVP renormalises (bake in M4)", and it now asserts that both sets bake (`set_resolved.bake = true`; flat normals, so the bytes do not move).

- [ ] **Step 1: Write the tests.** In `crates/engine/tests/golden.rs`:

1. Add the imports `use engine::session::Session;`, `use encoders_sys::Bc1Model;` and `use engine::image::Rgba32F;`.

2. Add the helpers below `flat_normal`:

```rust
/// A unit-length normal map whose direction changes from texel to texel, so every footprint is short.
fn bumpy_normal(w: u32) -> DynamicImage {
    DynamicImage::ImageRgb8(ImageBuffer::from_fn(w, w, |x, y| {
        let nx = 0.6 * (x as f32 * 1.3 + y as f32 * 0.7).sin();
        let ny = 0.6 * (x as f32 * 0.9 - y as f32 * 1.1).cos();
        let nz = (1.0 - nx * nx - ny * ny).sqrt();
        let e = |v: f32| ((v * 0.5 + 0.5) * 255.0).round() as u8;
        Rgb([e(nx), e(ny), e(nz)])
    }))
}

/// A roughness member: R a ramp, G 0, B 200 (AO), A specular 64 + y.
fn rough_member(w: u32) -> DynamicImage {
    rgba(w, |x, y| {
        [(30 + (x * 3 + y * 2) % 190) as u8, 0, 200, (64 + y) as u8]
    })
}

/// A normal texel as mip 0 writes it with the cs defaults: decoded as spec §5.4 does (no flip, no Z rebuild,
/// z >= 0), then renormalised (`mips.normal_base = renormalize`, DECISIONS R1). The bake averages these.
fn decoded_normal(p: &image::Rgb<u8>) -> [f64; 3] {
    let d = |c: u8| f64::from(c) / 255.0 * 2.0 - 1.0;
    let v = [d(p[0]), d(p[1]), d(p[2]).max(0.0)];
    let len = (v[0] * v[0] + v[1] * v[1] + v[2] * v[2]).sqrt();
    if len > 1e-8 {
        v.map(|c| c / len)
    } else {
        [0.0, 0.0, 1.0]
    }
}

/// Spec §6.2 step 2 in f64 (test code only): `min(2v, t²)` for the mean of `normals`.
fn reference_add(normals: impl Iterator<Item = [f64; 3]>) -> f64 {
    let t = 0.25f64;
    let (mut s, mut k) = ([0.0f64; 3], 0.0f64);
    for n in normals {
        for c in 0..3 {
            s[c] += n[c];
        }
        k += 1.0;
    }
    let l = ((s[0] / k).powi(2) + (s[1] / k).powi(2) + (s[2] / k).powi(2)).sqrt();
    if l >= 1.0 - 1e-6 {
        0.0
    } else if l <= 0.0 {
        t * t
    } else {
        (0.5 * (1.0 - l * l) / (3.0 * l - l * l * l)).min(t * t)
    }
}

/// Spec §6.2 in f64 (test code only): the stored R of member texel (x, y) at mip `level`, the `_n` `shift` levels
/// larger (negative: smaller). `shift` far below zero gives the unbaked value.
fn reference_roughness(
    n: &image::RgbImage,
    r: &image::RgbaImage,
    level: u32,
    shift: i32,
    x: u32,
    y: u32,
) -> f64 {
    let f = 1u32 << level;
    let mut alpha = 0.0;
    for j in 0..f {
        for i in 0..f {
            let v = f64::from(r.get_pixel(x * f + i, y * f + j)[0]) / 255.0;
            alpha += v * v;
        }
    }
    alpha /= f64::from(f * f);
    let m = level as i32 + shift;
    let add = if m < 1 {
        0.0
    } else {
        let g = 1u32 << m;
        reference_add(
            (0..g * g).map(|k| decoded_normal(n.get_pixel(x * g + k % g, y * g + k / g))),
        )
    };
    if add == 0.0 {
        alpha.sqrt()
    } else {
        (alpha * alpha + add).min(1.0).powf(0.25)
    }
}

/// The float chain the encoder receives for the planned job whose input ends with `name`, and that job.
fn planned_chain(
    session: &Session,
    req: BatchRequest,
    name: &str,
) -> (engine::policy::ResolvedJob, Vec<Rgba32F>) {
    let rep = Reporter::new(Arc::new(NullSink), "golden");
    let plan = Engine::new()
        .unwrap()
        .plan(req, &rep, &CancelToken::default())
        .unwrap();
    let job = plan
        .jobs
        .iter()
        .find(|j| j.input.ends_with(name))
        .unwrap_or_else(|| panic!("no job for {name}"))
        .clone();
    let chain = session
        .preview_source(&job, 16384, &CancelToken::default())
        .unwrap();
    assert_eq!(chain.first_level, 0);
    (job, chain.levels)
}
```

3. Add the tests:

```rust
/// Roadmap M4 exit: `_n` + `_rmaos` and `_n` + `_rfaos` at 1:1 and 2:1 (and a half-size `_n`). Every texel of every
/// level of the roughness chain the encoder receives is within 1/255 of the f64 reference, and the bake widens it.
#[test]
fn variance_bake_matches_the_f64_reference() {
    let session = Session::new(Bc1Model::Ideal).unwrap();
    for (nw, mw, member, shift) in [
        (64u32, 64u32, "rmaos", 0i32),
        (128, 64, "rmaos", 1),
        (32, 64, "rmaos", -1),
        // File suffixes: `_rfaos` resolves to the sub-type `skin_rfaos` after the packing-fills plan.
        (64, 64, "rfaos", 0),
        (128, 64, "rfaos", 1),
    ] {
        let d = tempfile::tempdir().unwrap();
        put(d.path(), "textures/pbr/iron_n.png", bumpy_normal(nw));
        let name = format!("iron_{member}.png");
        put(d.path(), &format!("textures/pbr/{name}"), rough_member(mw));
        let (job, levels) =
            planned_chain(&session, request(d.path(), Target::Cs, &[], &[]), &name);
        assert_eq!(
            job.bake.as_ref().map(|b| b.shift),
            Some(shift),
            "{nw}:{mw} {member}: {:?}",
            job.warnings
        );
        let (n, r) = (bumpy_normal(nw).to_rgb8(), rough_member(mw).to_rgba8());
        let mut widened = 0;
        for (level, l) in levels.iter().enumerate() {
            for y in 0..l.height {
                for x in 0..l.width {
                    let want = reference_roughness(&n, &r, level as u32, shift, x, y);
                    let got = f64::from(l.at(x, y)[0]);
                    assert!(
                        (got - want).abs() <= 1.0 / 255.0,
                        "{nw}:{mw} {member} mip {level} ({x},{y}): {got} vs {want}"
                    );
                    let plain = reference_roughness(&n, &r, level as u32, -100, x, y);
                    if want > plain + 1.0 / 255.0 {
                        widened += 1;
                    }
                }
            }
        }
        assert!(widened > 0, "{nw}:{mw} {member}: nothing widened");
        // And it converts: both outputs written, the set reports the bake.
        let o = convert(d.path(), Target::Cs, &[], &[]);
        assert_eq!(
            (o.summary.converted, o.summary.exit_code()),
            (2, 0),
            "{:?}",
            warnings(&o)
        );
        assert!(o.events.iter().any(|e| matches!(
            &e.kind,
            EventKind::SetResolved { bake: true, .. }
        )));
        assert_dds(
            &dds(d.path(), &format!("textures/pbr/iron_{member}.dds")),
            DdsPixelFormat::Bc7 { srgb: false },
            HeaderKind::Dx10,
            7,
        );
    }
}

/// Renormalise-only paths are unchanged: a flat `_n` adds no variance, so the roughness chain is bit-identical to the
/// same job without its bake.
#[test]
fn a_flat_normal_leaves_the_roughness_chain_bit_identical() {
    let session = Session::new(Bc1Model::Ideal).unwrap();
    let d = tempfile::tempdir().unwrap();
    put(d.path(), "textures/pbr/iron_n.png", flat_normal(64));
    put(d.path(), "textures/pbr/iron_rmaos.png", rough_member(64));
    let (job, baked) = planned_chain(
        &session,
        request(d.path(), Target::Cs, &[], &[]),
        "iron_rmaos.png",
    );
    assert!(job.bake.is_some());
    let mut plain = job.clone();
    plain.bake = None;
    let unbaked = session
        .preview_source(&plain, 16384, &CancelToken::default())
        .unwrap()
        .levels;
    assert_eq!(baked, unbaked);
}

/// Spec §6.3: a non-PBR `_n` never bakes; `renormalize_bake` set on it changes nothing.
#[test]
fn renormalize_bake_on_a_non_pbr_normal_is_inert() {
    let session = Session::new(Bc1Model::Ideal).unwrap();
    let d = tempfile::tempdir().unwrap();
    put(d.path(), "textures/a/wall_n.png", bumpy_normal(64));
    let (_, plain) = planned_chain(
        &session,
        request(d.path(), Target::Cs, &[], &[]),
        "wall_n.png",
    );
    let (job, set) = planned_chain(
        &session,
        request(
            d.path(),
            Target::Cs,
            &["file:**/wall_n.png.mips.normal=renormalize_bake"],
            &[],
        ),
        "wall_n.png",
    );
    assert_eq!(job.profile.subtype.id(), "normal");
    assert_eq!(plain, set);
}

/// Owner 2026-10-11 (DECISIONS R2), roadmap exit: a `_cnr` widens its own coat roughness (alpha) by the formula, 1:1;
/// a flat `_cnr` keeps its alpha bit for bit, as with the self-bake off.
#[test]
fn cnr_self_bake_matches_the_reference_and_a_flat_coat_keeps_its_alpha() {
    let session = Session::new(Bc1Model::Ideal).unwrap();
    let coat_alpha = |x: u32, y: u32| (40 + (x * 3 + y) % 200) as u8;
    // Bumpy.
    let d = tempfile::tempdir().unwrap();
    let bumpy = bumpy_normal(64).to_rgb8();
    put(
        d.path(),
        "textures/pbr/silk_cnr.png",
        rgba(64, |x, y| {
            let p = bumpy.get_pixel(x, y);
            [p[0], p[1], p[2], coat_alpha(x, y)]
        }),
    );
    let (job, levels) = planned_chain(
        &session,
        request(d.path(), Target::Cs, &[], &[]),
        "silk_cnr.png",
    );
    assert_eq!(job.profile.subtype.id(), "coat_normal");
    let mut widened = 0;
    for (level, l) in levels.iter().enumerate() {
        let f = 1u32 << level;
        for y in 0..l.height {
            for x in 0..l.width {
                let mut r = 0.0;
                for j in 0..f {
                    for i in 0..f {
                        r += f64::from(coat_alpha(x * f + i, y * f + j)) / 255.0;
                    }
                }
                r /= f64::from(f * f);
                let add = if level == 0 {
                    0.0
                } else {
                    reference_add((0..f * f).map(|k| {
                        decoded_normal(bumpy.get_pixel(x * f + k % f, y * f + k / f))
                    }))
                };
                let want = if add == 0.0 {
                    r
                } else {
                    (r.powi(4) + add).min(1.0).powf(0.25)
                };
                let got = f64::from(l.at(x, y)[3]);
                assert!(
                    (got - want).abs() <= 1.0 / 255.0,
                    "mip {level} ({x},{y}): {got} vs {want}"
                );
                if want > r + 1.0 / 255.0 {
                    widened += 1;
                }
            }
        }
    }
    assert!(widened > 0);
    // Flat: the same alpha as with the self-bake off.
    let d = tempfile::tempdir().unwrap();
    put(
        d.path(),
        "textures/pbr/silk_cnr.png",
        rgba(64, |x, y| [128, 128, 255, coat_alpha(x, y)]),
    );
    let (_, on) = planned_chain(
        &session,
        request(d.path(), Target::Cs, &[], &[]),
        "silk_cnr.png",
    );
    let (_, off) = planned_chain(
        &session,
        request(
            d.path(),
            Target::Cs,
            &["file:**/silk_cnr.png.mips.normal=renormalize"],
            &[],
        ),
        "silk_cnr.png",
    );
    for (a, b) in on.iter().zip(&off) {
        assert!(
            a.px.iter().zip(&b.px).all(|(p, q)| p[3] == q[3]),
            "{}x{}",
            a.width,
            a.height
        );
    }
}
```

4. Update `two_to_one_sets_keep_their_own_chains`:
   - replace its doc comment with `/// Spec §11.1 2:1 sets: a 128 px `_n` with a 64 px roughness member. 2:1 is a power-of-two ratio, so SET-BAKE-RATIO (§10) does not fire and both sets bake (flat normals, so the roughness is unchanged).`;
   - after the `SetBakeRatio` assertion, add:

```rust
    for set in ["textures/pbr/iron", "textures/pbr/skin/skin"] {
        assert!(
            o.events.iter().any(|e| matches!(&e.kind, EventKind::SetResolved { bake: true, .. })
                && e.set.as_deref().is_some_and(|s| s.ends_with(set))),
            "{set} bakes"
        );
    }
```

- [ ] **Step 2: Run the tests.**

Run: `cargo test -p engine --test golden -- variance_bake a_flat_normal renormalize_bake cnr_self_bake two_to_one`
Expected: PASS, because Tasks 3–8 implement everything these tests exercise. If one fails, the defect is in the owning task's code: fix it there and say so in the report. Do not loosen the 1/255 tolerance or the reference.

- [ ] **Step 3: Check that the reference bites.** Temporarily change `0.5 *` in `reference_add` to `0.4 *` and run `cargo test -p engine --test golden variance_bake`. Expected: FAIL. Revert the change.

- [ ] **Step 4: Run the whole golden suite and `gui_repro`.**

Run: `cargo test -p engine --test golden` and `cargo test -p sdc --test gui_repro`
Expected: PASS. `gui_repro`'s `rock01_normal` is flat ([128, 128, 255]), so its packed `_rmaos` bakes with no added variance, and GUI and CLI write the same bytes.

- [ ] **Step 5: Run every gate and commit.**

```bash
cargo fmt --all && cargo clippy --workspace --all-targets -- -D warnings && cargo test --workspace
git add crates/engine/tests/golden.rs
git commit -m "test(engine): golden variance-bake cases against the f64 reference at 1:1, 2:1 and 1:2, and the _cnr self-bake (M4 exit)

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

---

### Task 10: GUI: shared roughness slots go to the engine; the "Normal base level" field

**Files:**
- Modify:
  - `crates/gui/src/model/sets.rs` (`shared_roughness`);
  - `crates/gui/src/model/mod.rs` (`request()`);
  - `crates/gui/src/model/fields.rs` (`FIELDS`, `field_lock`, `provenance_text`);
  - `crates/gui/src/ui/settings_panel.rs` (options for `mips.normal_base`).
- Test: `crates/gui/tests/sets_model.rs`, `crates/gui/tests/fields.rs`

**Interfaces:**
- Consumes: `BatchRequest::shared_roughness` (Task 8); `set_views`, `SetView::slots`, `SlotFill`, `SlotKind::Rmaos` (M3b).
- Produces: `pub fn shared_roughness(m: &AppModel) -> BTreeSet<String>` in `sdc_gui::model::sets`.

**Rules:**
- **Sharing (R7):** every set whose Rmaos slot holds an output that another set's Rmaos slot also holds (owned or borrowed) is sent to the engine. This covers the owner and the borrowers, and applies on `cs` only.
- **Field row:** "Normal base level" sits after "Normal handling". It is locked "normal maps only" outside the Normal category. Its options are "Renormalise to unit length" and "Keep as authored".
- **Provenance text:** `set` reads "decided by the texture set (variance bake)".

**Expectations that change:** none. `FIELDS` grows by one entry, and the existing tests iterate over it.

- [ ] **Step 1: Write the failing tests.**

In `crates/gui/tests/sets_model.rs`, add:

```rust
#[test]
fn a_shared_rmaos_is_sent_to_the_engine_for_owner_and_borrower() {
    let d = tempfile::tempdir().unwrap();
    let mut m = pbr_opened(d.path());
    idle(&mut m);
    assert!(shared_roughness(&m).is_empty());
    let odd = key_of(&m, "odd.png");
    let id = m.drop_new_set(&odd).unwrap();
    idle(&mut m);
    let rmaos = m
        .items
        .iter()
        .find(|i| i.name.ends_with("rmaos"))
        .unwrap()
        .key
        .clone();
    assert_eq!(
        m.drop_on_set(&rmaos, &id),
        DropOutcome::AskChoice {
            from: "landscape/rock01".into()
        }
    );
    assert_eq!(
        m.drop_choice(&rmaos, &id, DropChoice::Share),
        DropOutcome::Joined
    );
    idle(&mut m);
    let shared = shared_roughness(&m);
    assert!(shared.contains("landscape/rock01"), "{shared:?}");
    assert!(shared.contains(&id), "{shared:?}");
    assert_eq!(m.request().unwrap().shared_roughness, shared);
}
```

In `crates/gui/tests/fields.rs`, add:

```rust
#[test]
fn normal_base_level_is_a_normal_map_field() {
    let reg = PresetRegistry::builtin().unwrap();
    let n = reg.base_profile(Target::Cs, Subtype::PbrNormal);
    let d = reg.base_profile(Target::Cs, Subtype::Diffuse);
    assert!(FIELDS.iter().any(|(f, _)| *f == "mips.normal_base"));
    assert_eq!(field_lock(Target::Cs, &n, "mips.normal_base"), None);
    assert_eq!(
        field_lock(Target::Cs, &d, "mips.normal_base"),
        Some("normal maps only")
    );
    assert_eq!(
        provenance_text("set"),
        "decided by the texture set (variance bake)"
    );
}
```

- [ ] **Step 2: Run the tests to see them fail.**

Run: `cargo test -p sdc-gui --test sets_model a_shared_rmaos` and `cargo test -p sdc-gui --test fields normal_base`
Expected: FAIL to compile (`shared_roughness`), then the assertions on `FIELDS` and `provenance_text` fail.

- [ ] **Step 3: Implement.**

`crates/gui/src/model/sets.rs`: add after `engine_inputs`:

```rust
/// Ids of every set whose Rmaos slot (`_rmaos`/`_rfaos`) holds an output another set's Rmaos slot holds too, owner
/// and borrowers alike (M3b spec §3.3, v1 instruction; DECISIONS R7). The engine bakes no normal variance into a
/// shared roughness and reports no missing companion for these sets. Empty off `cs`.
pub fn shared_roughness(m: &AppModel) -> BTreeSet<String> {
    if m.target != Target::Cs {
        return BTreeSet::new();
    }
    let mut users: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for v in set_views(m) {
        if let Some(SlotFill::Own { item } | SlotFill::Borrowed { item, .. }) =
            v.slots.get(&SlotKind::Rmaos)
        {
            users.entry(item.clone()).or_default().push(v.id.clone());
        }
    }
    users
        .into_values()
        .filter(|ids| ids.len() > 1)
        .flatten()
        .collect()
}
```

`crates/gui/src/model/mod.rs`, in `request()`: after `req.output_paths = paths;`, add `req.shared_roughness = sets::shared_roughness(self);`.

`crates/gui/src/model/fields.rs`:
- Increase the length of `pub const FIELDS: [(&str, &str); N]` by one (12 → 13 unless the packing-fills plan changed it) and insert `("mips.normal_base", "Normal base level"),` after `("mips.normal", "Normal handling"),`.
- In `field_lock`, change the arm `"mips.normal" if p.subtype.category() != Category::Normal => Some("normal maps only"),` to `"mips.normal" | "mips.normal_base" if p.subtype.category() != Category::Normal => Some("normal maps only"),`.
- In `provenance_text`, add `"set" => "decided by the texture set (variance bake)".into(),` after the `"fallback"` arm.

`crates/gui/src/ui/settings_panel.rs`, in `options_of`, after the `"mips.normal" => vec![…],` arm, add:

```rust
        "mips.normal_base" => vec![
            opt(json!("renormalize"), "Renormalise to unit length"),
            opt(json!("keep"), "Keep as authored"),
        ],
```

- [ ] **Step 4: Run the tests to see them pass.**

Run: `cargo test -p sdc-gui`
Expected: PASS (`every_field_is_a_real_engine_path` covers the new row).

- [ ] **Step 5: Run every gate and commit.**

```bash
cargo fmt --all && cargo clippy --workspace --all-targets -- -D warnings && cargo test --workspace
git add crates/gui/src/model/sets.rs crates/gui/src/model/mod.rs crates/gui/src/model/fields.rs crates/gui/src/ui/settings_panel.rs crates/gui/tests/sets_model.rs crates/gui/tests/fields.rs
git commit -m "feat(gui): shared roughness slots never bake; Normal base level field (M4)

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

---

### Task 11: Docs: DECISIONS Note R, the spec, the roadmap and `docs/gui.md`

**Files:**
- Modify:
  - `docs/DECISIONS.md` (new Note R);
  - `docs/superpowers/specs/2026-10-08-skyrim-dds-converter-design.md` (§3.2, §4.1, §5.2, §5.4, §5.5, §6.2, §6.3, §10, §14);
  - `docs/superpowers/plans/2026-10-08-skyrim-dds-converter-phase0-2.md` (M4 section);
  - `docs/gui.md`.
- Test: none (docs). Run the gates once to confirm nothing else moved.

**Interfaces:** none.

- [ ] **Step 1: DECISIONS Note R.**
  - **Note R already exists** after the packing-fills plan, which writes its entries R-fills-1, R-skin-1 and the `skin_rfaos` rename there.
    - Do **not** add a second `## Note R` heading.
    - Append the M4 block below the packing-fills entries. Use the block's heading line as a bold paragraph instead: `**M4 variance bake and coverage fixes (owner and M4 plan, 2026-10-11)**`. Then add its intro line and bullets unchanged.
    - The labels R1–R14 do not collide with `R-fills-*` and `R-skin-*`, and every reference in the code and spec uses "DECISIONS R<n>".
  - Only if no Note R exists (the packing-fills plan not merged, which breaks Global Constraints), insert the block as written. Put it directly after the last bullet of Note P (the "**Owner, in the design conversation:**" bullet) and before `## Phase 2 exit — CLI MVP (2026-10-09)`.
  - Note Q is reserved for the NIF team: do not write a Note Q. If a Note Q block sits between P and R, leave it where it is.

```markdown
## Note R — M4 variance bake and coverage fixes (owner and M4 plan, 2026-10-11)

Owner decisions of 2026-10-11 (R1–R3) and the rulings of `docs/superpowers/plans/2026-10-11-sdc-m4-variance-bake.md` (R4–R14).

- **R1 Normal mip 0 is renormalised by default** (owner: "per-preset option, default renormalise"): field `mips.normal_base` = `renormalize` (default) or `keep`, settable per preset, rule, `[files]` entry or `--set`, for authors who keep short normals on purpose. `CHAN-NORMAL-SHORT` (info) reports a normal source whose mean length at mip 0 is below 0.9 (not for Z-rebuilt sources). Controller correction (2026-10-11): the normal chain and the bake average mip 0 **as it is written**, unit length under `renormalize` and as authored under `keep`, so variance reflects real normal divergence rather than exporter shortness; a uniformly short flat `_n` adds no roughness by default. Outputs change for existing normal maps with short sources (mip 0 and the levels averaged from it).
- **R2 `_cnr` self-bake** (owner): the coat normal's own variance widens its own alpha (coat roughness) with the §6.2 formula, 1:1, no companion; on by default (`coat_normal` `mips.normal = renormalize_bake`), off with `renormalize`. The alpha keeps its plain box filter: A = (min(r̄⁴ + min(2v, t²), 1))^¼ only where the footprint adds variance, so a flat coat normal's alpha is bit-identical to before.
- **R3 Not in M4** (owner): the DDS header-patch idea and the CS/Vanilla two-mode idea.
- **R4 The roughness member decodes its own `_n`** (plan): `ResolvedJob::bake` (`BakeJob`) names the `_n` and carries its profile; the member's job decodes it, keeps only the footprint lengths, drops the image, then loads its own source. No cross-job state and no scheduler ordering; replaces spec §6.3's summed-area table kept between jobs. The admission counts 24 bytes per `_n` texel extra.
- **R5 Footprint lengths are a box pyramid** (plan): exact at power-of-two ratios, 1.33 bytes per `_n` texel. The bake needs both bases to be powers of two with one aspect; anything else is `SET-BAKE-RATIO` and no bake.
- **R6 The bake decision lives on the `_n`** (plan): a baking set upgrades its `pbr_normal`'s built-in `mips.normal = renormalize` to `renormalize_bake` with the new provenance `set`; an explicit `renormalize` from any other layer opts the set out. The `_n` is renormalised either way. `normal`, `landscape_normal` and `msn` never bake (no bake into vanilla `_n` alpha or `_m` green, spec §6.3).
- **R7 Shared roughness never bakes** (M3b v1 instruction): `BatchRequest::shared_roughness` lists the ids of every set (owner and borrowers) whose `_rmaos`/`_rfaos` slot (sub-type `rmaos` or `skin_rfaos`) is one output used by several sets; those sets renormalise only and raise no missing-companion code. The GUI fills it from its borrows; the CLI leaves it empty.
- **R8 `SET-*` triggers** (plan): `SET-MISSING-COMPANION` (info) for a `cs` PBR set with a `pbr_normal` but no roughness member or the reverse, an `_msn` without `_s`, a `_flow` without `_n`; `SET-COMPANION-DDS` (warning) when the missing companion is only a DDS (in the set or on disk); `SET-COMPANION-OUTSIDE` (info) when it is an image on disk next to the file but not in the batch; both directions. The code goes on the job it concerns. A PBR set holding only an albedo raises none.
- **R9 A usable `_n` is decided** (plan): Ready, or Blocked (e.g. `FMT-BC5-NORMAL`: the bake reads the source). An Unresolved `_n` gives its members `SET-MISSING-COMPANION` ("not decided yet"). With two `pbr_normal` jobs in one GUI-built set, the first by path is used.
- **R10 sRGB roughness** (plan): a roughness member with `color.content = srgb` raises `TAG-CONTENT-MISMATCH` and does not bake.
- **R11 No alpha weighting for vectors or roughness** (plan, Task 29 carry): normal, `_msn`, `_flow` and roughness chains are never alpha-weighted, even with a cutout policy forced on them. Preprocess dilation under α = 0 still runs for alpha-tested policies.
- **R12 `if-changed` and the bake** (plan): the baked `_n` is recorded in the manifest's `sources` with `bake` = the SHA-256 of its decode settings (`channels`, `alpha.policy`, `mips.normal_base`) and level shift; a changed `_n`, changed settings, or the bake turning on or off re-encodes the member. `Profile` gains `mips.normal_base` and `mips.bake_clamp`, so every `profile_sha256` changes once and the first M4 run re-encodes every output (as Note M10).
- **R13 Coverage, mipcheck round 2 B1/B6** (owner decision on the dither, 2026-10-11, verbatim: "I would probably encourage a dithering at lower mips, the whole point of coverage is to ensure it isn't lost, especially for foliage and grass, when your likely seeing mostly lower mip versions of them"): every level ≥ 1 passes exactly `k = round(p·n)` texels, the texels above the k-th largest alpha plus as many tied with it as are needed, chosen in 8×8 Bayer order of position (deterministic, no RNG), so tied alpha on thin strands neither overshoots (up to ×32 before) nor vanishes (B1). `k = 0` keeps the box alpha ("no texel passes"); zero alpha is never raised, and a level with no positive alpha keeps its box alpha; there is no "more than 2× off" fallback. Remapped texels, the split's included, sit at least 2 codes clear of the threshold (B6).
- **R14 `sdc-base` 1.1.0** (plan): new defaults `mips.normal_base`, `mips.bake_clamp` (0.25) and the `coat_normal` self-bake.
```

- [ ] **Step 2: The spec** (`docs/superpowers/specs/2026-10-08-skyrim-dds-converter-design.md`). Make these exact replacements.

§3.2 table. Replace the row

```
| `mips.normal` | `renormalize` / `renormalize_bake` | `renormalize`; set job may upgrade (§6) | Normal category |
```

with

```
| `mips.normal` | `renormalize` / `renormalize_bake` | `renormalize`; a baking set upgrades its `pbr_normal` (provenance `set`); `renormalize_bake` for `coat_normal` (self-bake) (§6.2, DECISIONS R2, R6) | Normal category |
| `mips.normal_base` | `renormalize` / `keep` | `renormalize` (owner 2026-10-11, DECISIONS R1) | Normal category: mip 0 stored at unit length unless `keep` |
| `mips.bake_clamp` | (0, 1] | 0.25 (DECISIONS Note E) | Clamp *t* of the variance bake (§6.2), read from the profile the bake writes into |
```

§4.1 table, `coat_normal` row: replace `| RGB renormalised; A separate |` with `| RGB renormalised; A separate, widened by the coat normal's own variance (self-bake, DECISIONS R2) |`.

§5.2: replace `there is no `set` source yet; a set-level source for decisions such as the M4 bake is added with that milestone)` with `` `set` for a decision of the texture set, so far the bake's `mips.normal = renormalize_bake` on a `pbr_normal` (M4, DECISIONS R6)) ``.

§5.4 step 3: replace `Mip 0 otherwise stays as authored.` with `Mip 0 is renormalised in Mipmaps unless `mips.normal_base = keep` (owner 2026-10-11, DECISIONS R1).`

§5.5 table: replace

```
| Roughness member R | Box on α = r² | α domain, √ back | Variance bake (§6.2) |
```

with

```
| Roughness member R | Box on α = r², never alpha-weighted | α domain, √ back | Variance bake (§6.2) |
```

and replace

```
| Normals | Box only | Vector | Mips ≥ 1 renormalised; `msn` by length only |
```

with

```
| Normals | Box only, never alpha-weighted | Vector | Every level renormalised (mip 0 unless `mips.normal_base = keep`), accumulated from mip 0 as written; `msn` by length only; a `_cnr` self-bakes its alpha (DECISIONS R1, R2, R11) |
```

§5.5 coverage paragraph: after the sentence ending `the §11.5 in-game check watches for thick distant edges.`, insert ` **[Decided]** (M4, owner 2026-10-11, DECISIONS R13) mipcheck round 2: every level passes exactly its target count; texels tied at the boundary are split in 8×8 Bayer order of position, deterministic, so thin foliage and grass keep their coverage at low mips (B1: no ×32 overshoot, no vanishing strands), and remapped texels sit at least two codes clear of the threshold so BC7 cannot flip them (B6).` In the same paragraph, the two documented limits (the tie-group over-coverage and the "nearest group" rule) no longer apply: replace the sentence beginning `A monotone remap cannot split equal alphas` with `Equal alphas at the boundary are split in Bayer order, so every count is reachable.`

§6.2: replace the sentence `**[Proposal]** The CLI MVP renormalises `_n` mips and does not bake (owner: the bake arrives in M4); `mips.normal = renormalize_bake` is accepted and behaves as `renormalize` until then.` with `**[Decided]** Shipped in M4 (DECISIONS Note R): the roughness member decodes its `_n` itself (R4); footprint lengths come from a box pyramid of the `_n`'s mip 0 as written (R1, R5), so step 1 averages unit normals unless `mips.normal_base = keep`; a `_cnr` bakes its own variance into its coat roughness (R2).`

§6.3: replace `**[Proposal]** In a set, `_n` loads first and its base normals are kept as a summed-area table (O(1) footprint averages) until the roughness member's mips are built, then released. Other members are independent.` with `**[Decided]** (M4, DECISIONS R4, R5) The roughness member's own job decodes the set's `_n` first, keeps a box pyramid of its footprint lengths (exact at power-of-two ratios), drops the image and then loads its own source; no job waits on another. The `_n` counts against `--mem-budget` with the member (24 bytes per `_n` texel).` In the table below it, add the row `| Roughness slot shared with another set (GUI borrow) | Renormalise only | — (DECISIONS R7) |` after the "Non-PBR set" row.

§10 table:
- After the `CHAN-NORMAL-LENGTH` row, add `| `CHAN-NORMAL-SHORT` | info | A normal source's mean length at mip 0 is below 0.9 → mip 0 renormalised, or kept with `mips.normal_base = keep` (DECISIONS R1) | — |`.
- `TAG-CONTENT-MISMATCH`: replace `Linear content with `_SRGB` tag, or colour sub-type with linear content` with `Linear content with `_SRGB` tag, colour sub-type with linear content, or a roughness member with sRGB content (no bake, DECISIONS R10)`.
- `SET-MISSING-COMPANION (M4)` row: drop ` (M4)` and append `; a PBR set holding only an albedo raises none (DECISIONS R8)` to the trigger.
- `SET-COMPANION-OUTSIDE (M4)` row: drop ` (M4)` and make the trigger `The missing companion (`_n` or roughness member) is on disk, not in the batch → no bake (DECISIONS R8)`.
- `SET-COMPANION-DDS (M4)` row: drop ` (M4)` and make the trigger `The missing companion (`_n` or roughness member) exists only as DDS → no bake (DECISIONS R8)`.
- `SET-BAKE-RATIO (M4)` row: drop ` (M4)` and make the trigger `` `_n`/roughness-member sizes are not powers of two with one aspect → bake off (DECISIONS R5) ``.

- [ ] **Step 3: The roadmap** (`docs/superpowers/plans/2026-10-08-skyrim-dds-converter-phase0-2.md`). At the end of the `### M4 — Texture-set variance bake (spec §6.2–6.3)` section, before `### M5`, add:

```markdown
- **Consumed by M4** (built on `feat/m4-variance-bake`, 2026-10-11; plan `docs/superpowers/plans/2026-10-11-sdc-m4-variance-bake.md`; DECISIONS Note R):
  - The scope above, with the companion decoded by the roughness member itself and a box length pyramid in place of the summed-area table and the scheduler ordering (R4, R5).
  - Every carried item:
    - shared roughness never bakes (R7);
    - `build_chain` reads `p.mips.normal` (the `_cnr` self-bake, Task 6) and `build_chain_with` takes the bake;
    - the four `SET-*` codes (R8);
    - no alpha weighting for vector or roughness chains (R11);
    - the `_n` is stamped, hashed and admitted with its member (R4, R12);
    - mipcheck B1/B6, with the owner's dithered exact coverage (R13);
    - the owner's `_cnr` self-bake (R2);
    - B2's accumulation length (before normalising) is the bake's L, taken from mip 0 as written (R1).
  - Owner decision 2026-10-11: normal mip 0 is renormalised by default (`mips.normal_base`, `CHAN-NORMAL-SHORT`, R1).
- **Still open after M4:**
  - the in-game check of the bake's look at distance (spec §14 #4, t = 0.25);
  - per-mip angular-error metrics for normals (M7).
```

- [ ] **Step 4: `docs/gui.md`.**
  - In `## Editing`, after the first paragraph, add: "**Normal base level** (normal maps only) stores mip 0 at unit length by default; choose *Keep as authored* for a normal map whose short normals are deliberate. A PBR set's normal map also bakes its detail into the set's `_rmaos`/`_rfaos` roughness, and a `_cnr` into its own coat roughness; the Normal handling row then reads *decided by the texture set*."
  - In `### Editing sets`, at the end of the **Share** bullet, add: "A shared `_rmaos` is never variance-baked, because the sets sharing it have different normal maps."

- [ ] **Step 5: Run every gate and commit.**

```bash
cargo fmt --all && cargo clippy --workspace --all-targets -- -D warnings && cargo test --workspace
git add docs/DECISIONS.md docs/superpowers/specs/2026-10-08-skyrim-dds-converter-design.md docs/superpowers/plans/2026-10-08-skyrim-dds-converter-phase0-2.md docs/gui.md
git commit -m "docs: DECISIONS Note R, spec and roadmap for the M4 variance bake and coverage fixes

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

---

## Self-review (run while writing this plan)

**1. Spec coverage.**

| Requirement | Task |
|---|---|
| §6.2 formula (L → κ → v → α′², t = 0.25 tunable per preset) | 5 (`variance_add`, `baked_roughness`, `mips.bake_clamp` from Task 3) |
| §6.2 step 1: base-level footprint average; v = 0 for a footprint of at most one `_n` texel | 5 (`LengthPyramid`, `add_at` with `m < 1`) |
| §6.2: a larger `_n` puts variance into mip 0 | 5 (level-0 bake), 7 (`a_two_to_one_normal_bakes_into_mip_0_too`), 9 |
| §6.2: a ratio that is not a power of two disables the bake (`SET-BAKE-RATIO`) | 5 (`bake_shift`), 8 |
| §6.2: `_rmaos` and `_rfaos` both bake | 8, 9 |
| §6.2 step 5: the `_n` is renormalised as usual | 4 (unchanged chain), 9 (`renormalize_bake_on_a_non_pbr_normal_is_inert`) |
| §6.3 table: bake / no member / outside / DDS / `_n` absent / non-PBR | 8 |
| §6.3 ordering and memory | 7 (R4, admission) |
| §4.1 `coat_normal` A coat roughness | 6 |
| §4.1 `normal` A specular is never baked into | 6 (self-bake only for `coat_normal`), Note R6 |
| §5.5 roughness on α = r² | 5 (kept), 4 (no weighting) |
| §5.5 coverage (B1, B6) | 1, 2 |
| §5.8 validation of the baked output | Unchanged: Validate compares against the encoder input, which now holds the baked chain (7) |
| §10 the four `SET-*` codes | 8 |
| §5.2 provenance `set` | 3, 8 |
| Roadmap exit: golden at 1:1 and 2:1 within 1/255 of an f64 reference; renormalise-only paths unchanged | 9 |
| Carries: shared roughness, `build_chain` reads `mips.normal`, `SET-*`, forced cutout, `_n` drift, memory, B1/B6, `_cnr`, B2 length | 10/8, 6, 8, 4, 7, 7, 1/2, 6, 5 (R1) |
| Owner R1 with the controller correction (mip 0, field, info code, the bake and the chain read mip 0 as written, named expectations) | 3, 4, 5, 7 (`a_uniformly_short_flat_normal_adds_roughness_only_under_keep`); expectations in 4 and 8 |
| Owner R2 | 6, 9 |
| Owner R3 | Nothing built; recorded in Note R |

**2. Placeholder scan.** No "TBD", "TODO", "similar to Task N" or "add error handling" remains. Every code step carries its code. Where an edit is a one-line replacement in a long existing function, the step quotes the old line and the new one.

**3. Type consistency.** These names are spelled the same in the registry and in every task:
- `build_chain_with(&Work, &Profile, Option<&NormalVariance>)`;
- `NormalVariance::new(LengthPyramid, (u32, u32), f64)`;
- `BakeJob { normal, normal_profile, shift, stamp }`;
- `SourceRef::bake: Option<String>`;
- `BatchRequest::shared_roughness: BTreeSet<String>` (ids; turned into keys in `api.rs`);
- `BakeCtx::shared` (keys);
- `plan_bakes(&BakeCtx, &mut [ResolvedJob]) -> BTreeSet<String>`;
- `Provenance::Set`, `NormalBase::{Renormalize, Keep}` and `Code::ChanNormalShort`.

**4. Review Focus.** Each item has a test in its owning task:
- **1 (ratios):** `bake_shift_needs_power_of_two_sizes_with_one_aspect` (5), `size_ratios_give_the_level_shift` and `an_aspect_mismatch_is_set_bake_ratio_and_does_not_bake` (8), `variance_bake_matches_the_f64_reference` (9).
- **2 (companions):** `a_dds_companion_is_set_companion_dds_in_both_directions`, `a_companion_on_disk_but_not_in_the_batch_is_set_companion_outside`, `a_member_from_another_folder_bakes_from_its_own_path` and `a_lone_n_or_a_lone_rmaos_in_a_pbr_set_is_set_missing_companion_info` (8).
- **3 (what the bake reads):** `a_blocked_bc5_n_still_bakes_from_its_source`, `an_srgb_roughness_member_does_not_bake` (8), `a_forced_cutout_does_not_alpha_weight_normals_or_roughness`, `the_chain_accumulates_mip_0_as_written` (4), `a_uniformly_short_flat_normal_adds_roughness_only_under_keep` (7), `up_to_date_until_the_normal_or_its_decode_settings_change` (7).
- **4 (memory):** `a_bake_holds_24_bytes_per_normal_texel` (7). Admission adds it through `job_bytes`.
- **5 (`_rfaos`):** `rmaos_and_rfaos_in_one_set_both_bake`, `a_packed_rmaos_bakes_like_a_prepacked_one` (8), and the `rfaos` rows of `variance_bake_matches_the_f64_reference` (9).

## Open questions (shipped defaults in this plan; ask the owner at review)

1. **What the bake averages. Resolved by a controller correction (2026-10-11), no longer open.**
   - The earlier wording "the bake reads the source before any renormalise" was the controller's, not the owner's. The owner decided only "per-preset option, default renormalise".
   - The bake averages mip 0 **as written**: unit length under the default, as authored under `mips.normal_base = keep`. This agrees with spec §6.2 step 1 ("unit normals").
   - A uniformly short flat `_n` adds no roughness by default, and adds roughness under `keep`. Test: `a_uniformly_short_flat_normal_adds_roughness_only_under_keep` (Task 7).
2. **`_cnr` coat alpha filter.**
   - Shipped: the plain box mean, with the bake on top. A flat coat is then unchanged, as the roadmap exit asks.
   - Alternative: filter it on α = r² like `_rmaos` R. That is closer to "the same formula", but it changes every `_cnr` with varying alpha, flat or not.
   - Recommendation: the plain box (shipped).
3. **Lone albedo.**
   - Question: should a PBR set holding only an albedo raise `SET-MISSING-COMPANION`?
   - Shipped: no (R8). It makes no bake decision, and it would add an info to every lone PBR texture.
   - Recommendation: no.
4. **A blocked `_n`.**
   - Question: should a blocked `_n` (for example BC5 not yet allowed) still bake its variance into the roughness member?
   - Shipped: yes (R9). The bake reads the source, and the member is right whichever way the `_n` is finally written.
   - Recommendation: yes.
5. **`CHAN-NORMAL-SHORT` threshold.**
   - Shipped: below a mean length of 0.9. It fires alongside `CHAN-NORMAL-LENGTH` (warning, mean |len − 1| > 0.1), which asks whether the file is a normal map at all.
   - Recommendation: keep both.
