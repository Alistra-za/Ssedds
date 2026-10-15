# GUI Suggestions, NIF Side: Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Ship the seven render3d / pgpsim / nif / modscan features the GUI asked for (SLSF names, pgpsim `alphaprop_*`, missing-texture fill, HDRI yaw + RGB background, `override_slot`, PGPatcher 3D shape index, debug views). Each feature ships end to end, with its API, its GUI consumer change and its tests. At the end the GUI has no per-path magenta swap, does not reload to remove the checker, has no grey-only background, carries no copy of the SLSF tables, and numbers Meshes rows by PGPatcher's 3D index.

**Architecture:** Everything new is additive inside the NIF-owned crates. In render3d: `Renderer` fields (`missing_fill`, `overrides`, `debug`) that `bind_textures` / `rebind` / `wanted()` read, a `Scene::env_yaw`, `Background::Solid([f32; 3])` and a new `debug.rs` that injects one output block into the user's own `Lighting.hlsl` expansion. pgpsim gets one integer-key rule. `nif` gets a pre-order tree walk (`graph::index3d`) carried on `NifScan` and stored by modscan (schema 8). The GUI consumer files (owner-approved, D1) drop their workarounds and call the new API.

**Tech Stack:** Rust 2021 workspace `sdc`: render3d (D3D11 through `windows`, WARP in tests, the repo stand-in `Lighting.hlsl`), pgpsim (serde_json), nif, modscan (rusqlite), sdc-gui (egui 0.36 / eframe, `egui_kittest`).

**Spec:** `F:\ai\dds\nifparseref\plan\gui-suggestions-nif\2026-10-15-gui-suggestions-nif-design.md` (APPROVED by the owner 2026-10-15). Evidence: `01-research.md` (its "Owner answers" are binding) and `NIF-HANDOFF.md` in the same folder. Read the spec with this plan; the plan argues from it.

**Scope note:** the spec spans four crates plus the GUI. The owner asked for one plan on one branch, so the tasks are ordered so that each one builds and passes on its own (spec "Order").

**Rebase point:** `nif/main` **after `nif/l-cm` (complex material) merges into it**. The line numbers below are from `nif/main` at `f5e60dc` (= bd495d2 for every source file this plan touches). `nif/l-cm` touches none of this plan's source files; the two shared files are listed under "Overlap with nif/l-cm" below.

## Global Constraints

- Branch `nif/m-guisug` in the worktree `F:\ai\dds\converter-nif-guisug`, made from `nif/main` after the `nif/l-cm` merge (Task 1, Step 0). Every command below runs in that folder, in Git Bash.
- Every cargo command sets `CARGO_TARGET_DIR=F:/ai/dds/converter-nif-p1/target` (written out in each command). Use `--release` throughout, as the other `nif/*` plans do. Check free space on F: before each gate (`df -h /f`); if it is under 15 GB, stop and tell the controller (memory: F: fills from worktree targets).
- Never push. Never merge to `main`. Never touch `F:\ai\dds\converter` (no checkout, cargo or git command there).
- No new crates or dependencies: `Cargo.lock` stays unchanged (`git diff --stat Cargo.lock` is empty at every gate).
- Owner-approved converter-team files (D1 and the approval line of the spec): `crates/gui/src/model/viewport3d.rs`, `crates/gui/src/ui/viewport3d.rs`, `crates/gui/src/settings/layout.rs`, `crates/gui/tests/viewport3d.rs`, `crates/gui/tests/pgp.rs`, `crates/gui/tests/window_layout.rs`, `crates/gui/tests/color_fields.rs`, `crates/gui/tests/set_preview_model.rs`, `crates/gui/tests/open_in_textures.rs`.
- NIF-owned GUI files (CONCURRENCY.md §3 / complex-grass work): `crates/gui/src/model/meshes.rs`, `crates/gui/src/ui/meshes.rs`, `crates/gui/tests/meshes_view.rs`, `crates/gui/tests/complex_grass.rs`.
- Still off-limits: `crates/engine`, `crates/cli` source, `crates/encoders-sys`, the golden tests and the `gui_repro` tests, `crates/gui/src/sets_doc.rs`, `crates/gui/src/model/sets.rs`, `crates/gui/src/model/pbr_export.rs`, `crates/gui/src/pgp/*`. If a golden or `gui_repro` test fails, stop and ask; do not change it.
- Shared files are add-only, each in its own commit: root `Cargo.toml`, `Cargo.lock`, `crates/gui/Cargo.toml`, `crates/gui/src/ui/mod.rs`, `crates/gui/src/model/mod.rs`, `crates/gui/src/app.rs`, `crates/gui/src/settings.rs`, `crates/gui/src/worker.rs`. **This plan touches none of them.**
- Every commit builds (`cargo build --workspace --release --all-targets` passes at each commit, not only at the task's end).
- Tests never read owner data except `#[ignore]` tests gated on `SDC_ACHERUS` (MO2 instance root, `C:\_mount\moddingvol\Acherus`) or `SDC_CS_SHADERS` (the CS `Shaders` folder, `C:\_mount\moddingvol\Acherus\mods\CommunityShaders_AIO-2026-10-03T06-46Z\Shaders`). Never the real `%LOCALAPPDATA%\sdc\scan.db`.
- Commit only explicit paths (`git add <path>…`, never `-A` or `.`). Every commit message ends with the line `Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>`.
- Never run `python -` or `python3 -` with stdin.
- `crates/gui/tests/render.rs` GPU tests may hang under contention. The workspace gate is `cargo test --workspace --release -- --skip render_` plus a separate `cargo test --workspace --release render_ -- --test-threads=1`.
- `docs/DECISIONS.md`: one new note, **Note AU** (nif/main ends at AS; `nif/l-cm` adds AT). Before writing it, run `grep -n "^## Note" docs/DECISIONS.md | tail -3` and take the next free letter if AT is not the last.
- The snippets below are written for meaning; rustfmt owns layout. Run `CARGO_TARGET_DIR=F:/ai/dds/converter-nif-p1/target cargo fmt --all` before every gate.
- Per-task gate (before the task's last commit), with the `CARGO_TARGET_DIR` above: `cargo fmt --all --check`, `cargo clippy --workspace --all-targets --release -- -D warnings`, then the task's own test commands. The full workspace gate runs in Task 8.

## Rulings (ambiguities the spec leaves open, decided here)

- **R1 One branch.** The spec's "each item is its own `nif/` branch" yields to the brief: one branch `nif/m-guisug`, tasks in spec order, one commit series per task.
- **R2 Open question 1** (test-only edits in `window_layout.rs`, `color_fields.rs`, `set_preview_model.rs`, `open_in_textures.rs`) is settled by the approved list above: yes.
- **R3 Open question 2** (which copies the ignored 3D-index test reads), settled from the owner's PGPatcher 2.2 logs (`C:\_mount\moddingvol\Acherus\tools\ParallaxGen\log\`, "Mapping file to mod" / "Found mesh" lines):
  - `mods\Skyking Alchemy Ingredients\meshes\plants\birdsnest02.nif`
  - `mods\Rudy HQ - Miscellaneous SE\meshes\loadscreenart\loadscreenadventure01.nif`
  - `mods\Improved Solitude Windmill\meshes\rbimprovedsolitudewindmillmeshes\rbfarmhousewindmill.nif`
  - `meshes\vibrant weapons\magic\fireshockwarhammer.nif` inside `mods\Vibrant Weapons EAE\Vibrant weapons EAE.bsa` (the log's `Stock Game\Data\…bsa` is MO2's virtual path).
  The `nif` crate has no `vfs` dependency, so the test lives in `crates/modscan/tests/index3d.rs` (modscan depends on both). It asserts only the logged `(block → index)` pairs (the log omits shapes it did not process).
- **R4 Debug block, Metallic.** CS's non-PBR `MaterialProperties` has no `Metallic` field (`Common/LightingCommon.hlsli:67-114`), so the spec's `sdc = material.Metallic.xxx;` would fail to compile for every non-PBR shape once any PBR shape makes Metallic offered. The block guards it: `#if defined(TRUE_PBR)` Metallic, else `0`. Availability for non-PBR stays `None`.
- **R5 Anchor names.** `get_debug` checks the declarations `MaterialProperties material =`, `float3 worldNormal`, `float glossiness`, `float vertexAO` (all present in the installed CS: L:1828, 1642/1645, 1361, 2673/2689/2691). The bare `MaterialProperties material` also matches function parameters in `LightingEval.hlsli`/`PBR.hlsli`, so it would not detect a rename.
- **R6 `get_debug` memo.** The inject result (Ok or Err) is memoised per `rel`; an I/O error from `get` stays unmemoised, as in `get`. Otherwise `wanted()` would rescan the 128 KB text per shape per `begin_all` on a CS build without the anchor.
- **R7 `set_debug_output` never short-circuits.** It always stores and calls `begin_all()`, so a test can re-permute after `set_encode_srgb`. The GUI calls it only when `debug_output()` differs.
- **R8 Error channel.** No new reply beyond the spec's `V3Reply::DebugSupport(bool)`. `set_debug_output` errs only when the anchor is missing, so the render thread answers an Err with `DebugSupport(false)`. On `false` while a CS debug view is selected, the model calls `set_debug(Lit)` (the thread's renderer goes to Lit too) and sets `status` to `DEBUG_UNSUPPORTED`.
- **R9 Override vs `Bound`.** The spec has `texture_targets` skip pairs under an override. This plan instead keeps those pairs in `rebind`'s plan and skips only the SRV write. `Bound` (the material flags) therefore keeps tracking the real texture during a swap under an override, and clearing the override needs no flag fix-up. `rebind`'s returned "touched" list leaves out covered pairs, so `set_texture_variant` returns `false` when every reader is covered (the entry is still updated).
- **R10 No re-apply after `make`.** The existing re-send of `SetDebug` in `ensure_thread` (viewport3d.rs:1363) re-applies the checker on a fresh thread, and `rebuild_device_on` carries the overrides. No other re-send is added.
- **R11 Kind-mismatch warning test.** The stand-in reflects no cube register, so the warning path is tested with a cube override against the 2D t0 (the same code path as the spec's 2D-into-cube case).
- **R12 `NiMeshParticleSystem`.** It also takes a 3D index (it is a nif.xml `NiParticleSystem`, and PGPatcher's `NiParticleSystem` check counts subclasses), although `ShapeKind` does not scan it. The closure test pins the set: closure(`NiTriBasedGeom` ∪ `BSTriShape` ∪ `NiParticleSystem`) minus `NiTriBasedGeom` (abstract) and `NiClod`, `NiLines`, `NiScreenElements` (not Skyrim; nifly does not read them).
- **R13 Test hooks.** Two `#[doc(hidden)]` hooks are added to `Renderer`: `draw_default_at(id, reg) -> Option<(SrvKind, [u8; 4])>` and `draw_bound(id) -> Option<Vec<u32>>`. They let the fill tests see which 1×1 default is bound and what `Bound` holds.
- **R14 "No reload" in the GUI** is tested on the real stand-in thread by clearing `report` and checking that no `Loaded` reply sets it again (`Loaded` for the loaded id always sets `report`, viewport3d.rs:1585-1588). `perf_log::capture` is thread-local and cannot see the render thread.
- **R15 Shape-card ids.** `ui/meshes.rs` `shape()` loses its `index` parameter; its two egui ids use `s.block` alone (unique per NIF copy).

## Review Focus

- **An old or hand-edited `gui.toml`:** `grey = 0.25` with no `bg` must load as `[0.25; 3]`. A `bg` of the wrong arity or type must fall back to `grey`, then to the default. Out-of-range channels must be clamped and non-finite ones dropped. Owner: Task 4, test `an_old_grey_background_loads_as_that_grey_and_a_bad_bg_falls_back`.
- **A CS update that removes the debug anchor while a debug view is on** must fall back to Lit with a status line, not draw garbage or panic. Owner: Task 7, test `losing_debug_support_falls_back_to_lit_with_a_status_line`.
- **A device loss (TDR) while the checker, the missing fill or a debug view is on** must keep them after `rebuild_device_on`. Owners: Task 3 `the_fill_survives_a_device_rebuild`, Task 5 `the_override_survives_a_device_rebuild`, Task 7 the last assertion of `installed_cs_reads_rmaos_back_as_its_values` (`#[ignore]`: the stand-in cannot hold a debug view, so only the installed CS can show it is kept).
- **Toggling the checker while shapes are still compiling (async):** the override must land when the program lands. Owner: Task 5, test `an_override_set_while_shapes_compile_lands_with_their_program`.
- **A NIF whose block 0 is not a node, or that has no node at all:** the first node is the root; with none, every row reads `#– (not in scene tree)` and the store holds NULL. Owner: Task 6, tests `the_root_is_the_first_node_when_block_0_is_not_one` and `shapes_of_round_trips_the_3d_index`.

## Overlap with nif/l-cm (merge risk)

- `crates/gui/tests/pgp.rs`: l-cm edits 917-938 and appends a test. This plan edits only `pgpsim_warns_on_exactly_the_gui_ranges` (140-177). Low risk, but rebase on the merged file.
- `docs/DECISIONS.md`: l-cm appends Note AT; this plan appends Note AU after it.
- No other file in this plan is on l-cm's list (`engine` policy/suggest, `sets_doc.rs`, `pgp/validate.rs`, `model/cm_json.rs`, `ui/cm_card.rs`, `set_editor.rs`, `model/mod.rs`, `ui/mod.rs`).

---

## File Structure

| File | Status | Responsibility | Task |
|---|---|---|---|
| `crates/nif/src/material.rs` | modify | `SLSF1_NAMES`, `SLSF2_NAMES` (+ unit tests) | 1 |
| `crates/gui/src/model/meshes.rs:1440-1511` | modify (NIF-owned) | re-export the tables | 1 |
| `crates/pgpsim/src/check.rs` | modify | `INT_KEYS`, `uint_in`, range warnings | 2 |
| `crates/pgpsim/src/patch.rs` | modify | `alphaprop_*` simulation | 2 |
| `crates/pgpsim/src/lib.rs:1` | modify | version doc line | 2 |
| `crates/pgpsim/tests/patch.rs` | modify | alphaprop sim tests | 2 |
| `crates/gui/tests/pgp.rs:140-177` | modify (approved) | Int probes | 2 |
| `crates/render3d/src/lib.rs` | modify | fill, overrides, debug field + API, `env_yaw` pass, RGB clear, test hooks | 3, 4, 5, 7 |
| `crates/render3d/tests/missing_fill.rs` | create | fill tests | 3 |
| `crates/render3d/src/scene.rs` | modify | `Scene::env_yaw`, `Background::Solid([f32;3])`, `rgb` | 4 |
| `crates/render3d/src/hdri.rs` | modify | `cells`, `integrate`, `Studio::ambient6_at` | 4 |
| `crates/render3d/src/background.rs` | modify | `Flags.y` yaw | 4 |
| `crates/render3d/tests/background.rs`, `tests/cs_features.rs:84` | modify | `[f32;3]`, RGB clear, yaw sign | 4 |
| `crates/render3d/tests/override_slot.rs` | create | override tests | 5 |
| `crates/render3d/src/debug.rs` | create | `DebugOutput`, `Availability`, `inject` | 7 |
| `crates/render3d/src/source.rs` | modify | `SourceSet::get_debug` | 7 |
| `crates/render3d/tests/debug.rs` | create | stand-in refusal + `#[ignore]` CS readback | 7 |
| `crates/nif/src/graph.rs` | modify | `index3d`, `takes_index` (+ closure test) | 6 |
| `crates/nif/src/scan.rs` | modify | `NifScan::index3d` | 6 |
| `crates/nif/tests/graph.rs`, `tests/fixtures.rs` | modify | index3d tests | 6 |
| `crates/modscan/src/store.rs` | modify | schema 8, `index3d` column, `ShapeView::index3d` | 6 |
| `crates/modscan/tests/store.rs` | modify | literal, round trip, v7 rebuild | 6 |
| `crates/modscan/tests/index3d.rs` | create | `#[ignore]` owner logs | 6 |
| `crates/gui/src/ui/meshes.rs:534-536, 728-790` | modify (NIF-owned) | `#index3d` header | 6 |
| `crates/gui/tests/meshes_view.rs:185-207, 1574-1593` | modify (NIF-owned) | literals, header test | 6 |
| `crates/gui/tests/open_in_textures.rs:13-31`, `complex_grass.rs:69`, `set_preview_model.rs:777` | modify (approved / NIF) | literals | 6 |
| `crates/gui/src/model/viewport3d.rs` | modify (approved) | fill, yaw, `bg`, override, debug views | 3, 4, 5, 7 |
| `crates/gui/src/ui/viewport3d.rs` | modify (approved) | colour picker, radio list, HDRI note | 4, 7 |
| `crates/gui/src/settings/layout.rs:101-133` | modify (approved) | `LookSettings::bg` | 4 |
| `crates/gui/tests/viewport3d.rs` | modify (approved) | per task | 3, 4, 5, 7 |
| `crates/gui/tests/window_layout.rs`, `color_fields.rs` | modify (approved) | `grey` → `bg` | 4 |
| `docs/DECISIONS.md` | modify | Note AU | 8 |

---

### Task 1: SLSF flag names in `nif::material`

**Files:**
- Modify: `crates/nif/src/material.rs` (append the two tables after `SLSF2_TRUE_PBR`, line 22; append a test module at the end)
- Modify: `crates/gui/src/model/meshes.rs:1440-1511` (replace both tables with one `pub use`)
- Test: `crates/nif/src/material.rs` (`#[cfg(test)] mod slsf_names`)

**Interfaces:**
- Produces: `pub const nif::material::SLSF1_NAMES: [&str; 32]`, `pub const nif::material::SLSF2_NAMES: [&str; 32]`. `sdc_gui::model::meshes::{SLSF1_NAMES, SLSF2_NAMES}` stay importable (re-export), so `ui/meshes.rs:6` and `tests/meshes_view.rs:1620` compile unchanged.

- [ ] **Step 0: Make the worktree**

```bash
df -h /f
git -C F:/ai/dds/converter-nif-p1 log --oneline -3 nif/main   # the top commit must be the nif/l-cm merge; if not, stop and ask
git -C F:/ai/dds/converter-nif-p1 worktree add F:/ai/dds/converter-nif-guisug -b nif/m-guisug nif/main
cd /f/ai/dds/converter-nif-guisug && git status --short && grep -n "^## Note" docs/DECISIONS.md | tail -3
```
Expected: a clean worktree on `nif/m-guisug`; the last note is AT.

- [ ] **Step 1: Write the failing test** (append to the end of `crates/nif/src/material.rs`)

```rust
#[cfg(test)]
mod slsf_names {
    use super::*;

    fn bit(mask: u32) -> usize {
        mask.trailing_zeros() as usize
    }

    #[test]
    fn the_documented_deviations_from_nif_xml_are_kept() {
        // nif.xml spells bit 8 "Recieve_Shadows" (sic); sdc shows it corrected. SLSF2 bit 23 is the TruePBR marker.
        assert_eq!(SLSF1_NAMES[8], "Receive_Shadows");
        assert_eq!(SLSF2_NAMES[23], "Unused01 (TruePBR)");
    }

    #[test]
    fn every_bit_constant_names_the_bit_the_table_names() {
        assert_eq!(SLSF1_NAMES[bit(SLSF1_SPECULAR)], "Specular");
        assert_eq!(SLSF1_NAMES[bit(SLSF1_RECEIVE_SHADOWS)], "Receive_Shadows");
        assert_eq!(SLSF1_NAMES[bit(SLSF1_CAST_SHADOWS)], "Cast_Shadows");
        assert_eq!(SLSF2_NAMES[bit(SLSF2_ZBUFFER_WRITE)], "ZBuffer_Write");
        assert_eq!(SLSF2_NAMES[bit(SLSF2_TRUE_PBR)], "Unused01 (TruePBR)");
    }
}
```

- [ ] **Step 2: Run it to see it fail**

Run: `cd /f/ai/dds/converter-nif-guisug && CARGO_TARGET_DIR=F:/ai/dds/converter-nif-p1/target cargo test -p nif --release --lib slsf_names`
Expected: compile error, `cannot find value SLSF1_NAMES`.

- [ ] **Step 3: Move the tables**

Cut `crates/gui/src/model/meshes.rs` lines 1440-1511 (both doc comments and both `pub const … NAMES: [&str; 32] = [ … ];` arrays, verbatim, GUI spellings kept). Paste them into `crates/nif/src/material.rs` right after line 22 (`pub const SLSF2_TRUE_PBR: u32 = 1 << 23;`). Then change the two doc lines to:

```rust
/// SkyrimShaderPropertyFlags1 bit names (nif.xml / NifSkope), moved from the GUI (GUI suggestions item 7). Bit 8 is
/// spelled `Receive_Shadows` (nif.xml: `Recieve_Shadows`, sic).
pub const SLSF1_NAMES: [&str; 32] = [
    // … the 32 names exactly as they were in model/meshes.rs …
];

/// SkyrimShaderPropertyFlags2 bit names. Bit 23 is the TruePBR marker (`SLSF2_TRUE_PBR`), shown as `Unused01 (TruePBR)`.
pub const SLSF2_NAMES: [&str; 32] = [
    // … the 32 names exactly as they were in model/meshes.rs …
];
```

In `crates/gui/src/model/meshes.rs`, put this one line where the tables were:

```rust
pub use nif::material::{SLSF1_NAMES, SLSF2_NAMES};
```

- [ ] **Step 4: Run the tests**

```bash
cd /f/ai/dds/converter-nif-guisug
CARGO_TARGET_DIR=F:/ai/dds/converter-nif-p1/target cargo test -p nif --release --lib slsf_names
CARGO_TARGET_DIR=F:/ai/dds/converter-nif-p1/target cargo test -p sdc-gui --release --test meshes_view
```
Expected: PASS (2 nif tests; meshes_view unchanged and green).

- [ ] **Step 5: Gate and commit**

```bash
cd /f/ai/dds/converter-nif-guisug
CARGO_TARGET_DIR=F:/ai/dds/converter-nif-p1/target cargo fmt --all --check
CARGO_TARGET_DIR=F:/ai/dds/converter-nif-p1/target cargo clippy --workspace --all-targets --release -- -D warnings
git add crates/nif/src/material.rs crates/gui/src/model/meshes.rs
git commit -m "nif: SLSF1/SLSF2 flag names move from the GUI into nif::material

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

---

### Task 2: pgpsim `alphaprop_*` keys (D2)

**Files:**
- Modify: `crates/pgpsim/src/check.rs` (after `UNIT_KEYS`, line 142; `range_warnings` 147-175; tests 177-207)
- Modify: `crates/pgpsim/src/patch.rs:3,8` (imports) and after the `zbuffer_write` block (lines 143-145)
- Modify: `crates/pgpsim/src/lib.rs:1`
- Test: `crates/pgpsim/tests/patch.rs` (append), `crates/pgpsim/src/check.rs` (`mod tests`), `crates/gui/tests/pgp.rs:140-177`

**Interfaces:**
- Produces: `pub(crate) fn check::uint_in(v: &serde_json::Value, max: u32) -> Option<u32>`; `pgpsim::range_warnings` also warns on `INT_KEYS`. No public API change.

- [ ] **Step 1: Write the failing tests**

Append to the `mod tests` in `crates/pgpsim/src/check.rs` (and change its `use super::range_warnings;` to `use super::{range_warnings, uint_in};`):

```rust
    #[test]
    fn uint_in_is_unsigned_from_json() {
        assert_eq!(uint_in(&json!(0), 255), Some(0));
        assert_eq!(uint_in(&json!(255), 255), Some(255));
        assert_eq!(uint_in(&json!(8.0), 255), Some(8), "an integral float counts");
        for bad in [json!(256), json!(-1), json!(1.5), json!(-0.5), json!("8"), json!(true), json!(null)] {
            assert_eq!(uint_in(&bad, 255), None, "{bad}");
        }
    }

    #[test]
    fn alphaprop_numbers_outside_their_range_warn_and_other_types_do_not() {
        let w = range_warnings(&json!({"alphaprop_thresh": 300}));
        assert_eq!(w, ["alphaprop_thresh is 300, outside 0–255 (PGPatcher ignores it)"]);
        assert_eq!(range_warnings(&json!({"alphaprop_flags": -1})).len(), 1);
        assert_eq!(range_warnings(&json!({"alphaprop_flags": 1.5})).len(), 1);
        assert!(range_warnings(&json!({"alphaprop_flags": 65535, "alphaprop_thresh": 0})).is_empty());
        assert!(range_warnings(&json!({"alphaprop_thresh": "8", "alphaprop_flags": true})).is_empty());
    }
```

Append to `crates/pgpsim/tests/patch.rs` (it already has `use common::*;`, `md()`, `with()`):

```rust
// ---- PGPatcher 2.2 alphaprop_* (GUI suggestions item 5) ----

fn alpha_after(entry: Value, source: &nif::material::MaterialState) -> Option<nif::props::Alpha> {
    patched(sim(&with(entry), &sdc_default(), source)).0.alpha
}

fn with_alpha(flags: u16, threshold: u8) -> nif::material::MaterialState {
    let mut m = rock();
    m.alpha = Some(nif::props::Alpha { flags, threshold });
    m
}

#[test]
fn alphaprop_values_in_range_apply_to_an_existing_property() {
    let a = alpha_after(json!({"alphaprop_flags": 4845, "alphaprop_thresh": 200}), &with_alpha(237, 10));
    assert_eq!(a, Some(nif::props::Alpha { flags: 4845, threshold: 200 }));
}

#[test]
fn bad_alphaprop_values_are_ignored_and_never_skip() {
    for (flags, thresh) in [
        (json!(70000), json!(300)),
        (json!(-1), json!(-1)),
        (json!(1.5), json!(2.5)),
        (json!("8"), json!("8")),
        (json!(true), json!(true)),
    ] {
        let r = sim(&with(json!({"alphaprop_flags": flags, "alphaprop_thresh": thresh})), &sdc_default(), &with_alpha(237, 10));
        assert!(matches!(r, SimResult::Patched { .. }), "{flags} / {thresh}: {r:?}");
        assert_eq!(patched(r).0.alpha, Some(nif::props::Alpha { flags: 237, threshold: 10 }), "{flags} / {thresh}");
    }
}

#[test]
fn delete_beats_create() {
    let a = alpha_after(json!({"alphaprop_delete": true, "alphaprop_create": true, "alphaprop_thresh": 5}), &with_alpha(237, 10));
    assert_eq!(a, None);
}

#[test]
fn create_on_a_shape_without_alpha_uses_niflys_defaults_then_the_given_values() {
    assert_eq!(
        alpha_after(json!({"alphaprop_create": true}), &rock()),
        Some(nif::props::Alpha { flags: 4844, threshold: 128 })
    );
    assert_eq!(
        alpha_after(json!({"alphaprop_create": true, "alphaprop_thresh": 64}), &rock()),
        Some(nif::props::Alpha { flags: 4844, threshold: 64 })
    );
}

#[test]
fn without_create_a_shape_without_alpha_stays_without() {
    assert_eq!(alpha_after(json!({"alphaprop_flags": 4845, "alphaprop_thresh": 64}), &rock()), None);
}
```

- [ ] **Step 2: Run them to see them fail**

Run: `cd /f/ai/dds/converter-nif-guisug && CARGO_TARGET_DIR=F:/ai/dds/converter-nif-p1/target cargo test -p pgpsim --release`
Expected: compile error `cannot find function uint_in` (the patch tests would then fail on the alpha values).

- [ ] **Step 3: Implement**

In `crates/pgpsim/src/check.rs`, after `UNIT_KEYS` (line 142):

```rust
/// PGPatcher 2.2 `alphaprop_*` whole-number keys and their maxima. Never in `NUM_KEYS`: 2.2 ignores a bad value
/// (`unsignedFromJSON`) where 1.2.0 threw, so they never make a `Skip`.
const INT_KEYS: [(&str, u32); 2] = [("alphaprop_flags", 65535), ("alphaprop_thresh", 255)];

/// PGPatcher 2.2 `unsignedFromJSON`: `Some` only for a JSON number that is a whole value in 0..=max (an integral
/// float counts). Booleans, strings, fractions, negatives and out-of-range values are `None` (ignored, never thrown).
pub(crate) fn uint_in(v: &Value, max: u32) -> Option<u32> {
    if !v.is_number() {
        return None;
    }
    v.as_f64()
        .filter(|f| f.fract() == 0.0 && (0.0..=f64::from(max)).contains(f))
        .map(|f| f as u32)
}
```

In `range_warnings`, replace the final `nums.into_iter() … .collect()` expression with:

```rust
    let mut out: Vec<String> = nums
        .into_iter()
        .filter_map(|(k, x)| {
            let unit = UNIT_KEYS.contains(&k.as_str());
            let hi = if unit { 1.0 } else { f64::INFINITY };
            (!(0.0..=hi).contains(&x)).then(|| {
                let range = if unit { "0–1" } else { "≥ 0" };
                format!("{k} is {x}, outside {range}")
            })
        })
        .collect();
    // A number PGPatcher 2.2 would ignore; strings and booleans are ignored too but are not sdc's range business.
    for (k, max) in INT_KEYS {
        if let Some(v) = e.get(k).filter(|v| v.is_number() && uint_in(v, max).is_none()) {
            out.push(format!("{k} is {v}, outside 0–{max} (PGPatcher ignores it)"));
        }
    }
    out
```

In `crates/pgpsim/src/patch.rs`: change line 3 to `use crate::check::{check_types, uint_in};` and line 8 to `use nif::props::{Alpha, LightingTrailer};`. After the `zbuffer_write` block (`if let Some(z) = get_bool(e, "zbuffer_write") { … }`, lines 143-145), insert:

```rust
        // PGPatcher 2.2 `setAlphaProperty` / `deleteAlphaProperty`. Delete wins; create makes nifly's default
        // NiAlphaProperty (flags 4844, threshold 128); only values `unsignedFromJSON` accepts are written.
        // Sharing/unsharing of the property has no effect on a per-shape preview.
        if flag(e, "alphaprop_delete") {
            self.m.alpha = None;
        } else if self.m.alpha.is_some() || flag(e, "alphaprop_create") {
            let old = self.m.alpha.unwrap_or(Alpha { flags: 4844, threshold: 128 });
            let int = |k: &str, max: u32| e.get(k).and_then(|v| uint_in(v, max));
            self.m.alpha = Some(Alpha {
                flags: int("alphaprop_flags", 65535).map_or(old.flags, |v| v as u16),
                threshold: int("alphaprop_thresh", 255).map_or(old.threshold, |v| v as u8),
            });
        }
```

In `crates/pgpsim/src/lib.rs`, line 1 becomes:

```rust
//! PGPatcher 1.2.0 TruePBR patch, simulated in memory for the 3D preview (P1 spec §2.1), plus the 2.2.0 `alphaprop_*` keys.
```

In `crates/gui/tests/pgp.rs`, replace `pgpsim_warns_on_exactly_the_gui_ranges` (lines 138-177) with:

```rust
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
        // The alphaprop_* keys are whole numbers 0..=max (pgpsim INT_KEYS, PGPatcher 2.2 unsignedFromJSON).
        if let NumRange::Int(max) = range {
            let max = f64::from(max);
            for (x, warnings) in [(-1.0, 1), (1.0, 0), (max + 1.0, 1)] {
                assert_eq!(pgpsim::range_warnings(&entry(x)).len(), warnings, "{} = {x}", k.key);
            }
            continue;
        }
        assert_eq!(
            pgpsim::range_warnings(&entry(-0.5)).len(),
            usize::from(range != NumRange::Any),
            "{} below 0",
            k.key
        );
        assert!(
            pgpsim::range_warnings(&entry(0.5)).is_empty(),
            "{} inside",
            k.key
        );
        assert_eq!(
            pgpsim::range_warnings(&entry(2.0)).len(),
            usize::from(range == NumRange::Unit),
            "{} above 1",
            k.key
        );
    }
}
```

- [ ] **Step 4: Run the tests**

```bash
cd /f/ai/dds/converter-nif-guisug
CARGO_TARGET_DIR=F:/ai/dds/converter-nif-p1/target cargo test -p pgpsim --release
CARGO_TARGET_DIR=F:/ai/dds/converter-nif-p1/target cargo test -p sdc-gui --release --test pgp --test alpha_prop --test set_preview_model
```
Expected: PASS, including the existing pgpsim `golden.rs` and `patch.rs` tests (no existing entry uses `alphaprop_*`). The set preview simulates entries through pgpsim, so a GUI test that previews an `alphaprop_*` entry may now see the alpha change: if one fails, check that the new alpha is what PGPatcher 2.2 would write before changing the expectation, and report it.

- [ ] **Step 5: Gate and commit**

```bash
cd /f/ai/dds/converter-nif-guisug
CARGO_TARGET_DIR=F:/ai/dds/converter-nif-p1/target cargo fmt --all --check
CARGO_TARGET_DIR=F:/ai/dds/converter-nif-p1/target cargo clippy --workspace --all-targets --release -- -D warnings
git add crates/pgpsim/src/check.rs crates/pgpsim/src/patch.rs crates/pgpsim/src/lib.rs crates/pgpsim/tests/patch.rs crates/gui/tests/pgp.rs
git commit -m "pgpsim: PGPatcher 2.2 alphaprop_* keys (simulated, range warnings, never a skip)

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

---

### Task 3: Missing-texture fill, colour registers only (D3)

**Files:**
- Modify: `crates/render3d/src/lib.rs`: `TexEntry` (197-202), `Renderer` struct (271-325) and `with_gpu` literal (398-431), `texture()` (670-702), `bind_textures` (707-772), `resolve_textures` (776-798), `rebuild_device_on` (1058-1070), `rebind` (1424-1476); new `set_missing_texture`, `colour_register`, two test hooks after `draw_identity` (1532)
- Create: `crates/render3d/tests/missing_fill.rs`
- Modify: `crates/gui/src/model/viewport3d.rs`: `make` (2027-2033), `load_all` (1959-1978), delete `magenta_dds` (520-523)
- Modify: `crates/gui/tests/viewport3d.rs`: delete `magenta_is_a_solid_magenta_dds` (2625-2631)

**Interfaces:**
- Produces: `pub fn Renderer::set_missing_texture(&mut self, fill: Option<[u8; 4]>)`; `#[doc(hidden)] pub fn Renderer::draw_default_at(&self, id: ShapeId, reg: u32) -> Option<(SrvKind, [u8; 4])>`; `#[doc(hidden)] pub fn Renderer::draw_bound(&self, id: ShapeId) -> Option<Vec<u32>>`; private `fn colour_register(reg: u32, kind: SrvKind) -> bool`; `TexEntry.missing: bool`; `Renderer.missing_fill: Option<[u8; 4]>`. Tasks 5 and 7 reuse `colour_register` and the hooks.

- [ ] **Step 1: Write the failing tests** (`crates/render3d/tests/missing_fill.rs`)

```rust
//! `Renderer::set_missing_texture` (GUI suggestions item 3, owner D3): the fill goes to colour registers only (t0, and
//! t4 when it reads a cube); data maps keep their neutral defaults and `Bound` is not changed. WARP + stand-in.
mod common;
use common::{channel, dds_rgba8, fixture, renderer, FLAT};
use render3d::compile::SrvKind;
use render3d::device::Gpu;
use render3d::scene::Scene;
use render3d::{Builtin, Renderer, ShapeId, Variant};
use std::path::Path;

const CYL1: ShapeId = ShapeId(9);
const MAGENTA: [u8; 4] = [255, 0, 255, 255];
const DIFFUSE: &str = "textures/standin/missing_d.dds";

/// cylinder_1 of the furniture fixture with `slots` set (none resolves) and `shader_type`; the default lit stand-in
/// (no FLAT/TBN) reads both t0 and t1.
fn cylinder(cache: &Path, features: &[&str], fill: Option<[u8; 4]>, shader_type: u32, slots: &[(usize, &str)]) -> Renderer {
    let mut r = renderer(cache, features);
    r.set_missing_texture(fill);
    let rep = r.load(&fixture("TestNifFile_Furniture_Col_SE.nif"), &|_| None);
    assert_eq!(rep.shapes, 2, "{rep:?}");
    let mut m = r.shapes().into_iter().find(|s| s.id == CYL1).unwrap().material;
    m.shader_type = shader_type;
    for (i, p) in slots {
        m.set_slot_raw(*i, p);
    }
    assert!(r.set_material(CYL1, m, &|_| None).is_ok());
    r
}

#[test]
fn a_missing_diffuse_shows_the_fill_and_a_missing_normal_stays_flat() {
    let cache = tempfile::tempdir().unwrap();
    let r = cylinder(cache.path(), &["STANDIN_OPAQUE_OK"], Some(MAGENTA), 0, &[(0, r"textures\standin\missing_d.dds"), (1, r"textures\standin\missing_n.dds")]);
    assert_eq!(r.draw_default_at(CYL1, 0), Some((SrvKind::Tex2D, MAGENTA)));
    assert_eq!(r.draw_default_at(CYL1, 1), Some((SrvKind::Tex2D, [128, 128, 255, 255])));
    assert_eq!(r.draw_bound(CYL1), Some(vec![]), "a fill is not a real texture");
}

#[test]
fn the_fill_shows_in_pixels_and_none_keeps_todays_white() {
    let cache = tempfile::tempdir().unwrap();
    let slots = [(0, r"textures\standin\missing_d.dds")];
    let mut filled = cylinder(cache.path(), FLAT, Some(MAGENTA), 0, &slots);
    let cache2 = tempfile::tempdir().unwrap();
    let mut plain = cylinder(cache2.path(), FLAT, None, 0, &slots);
    assert_eq!(plain.draw_default_at(CYL1, 0), Some((SrvKind::Tex2D, [255, 255, 255, 255])));
    let (a, b) = (filled.render(&Scene::neutral(), 64, 64).unwrap(), plain.render(&Scene::neutral(), 64, 64).unwrap());
    assert!(channel(&a, 1) + 1000 < channel(&b, 1), "magenta has no green: {} vs {}", channel(&a, 1), channel(&b, 1));
}

#[test]
fn a_facegen_shape_without_rfaos_keeps_t71_black() {
    let cache = tempfile::tempdir().unwrap();
    let r = cylinder(
        cache.path(),
        &["STANDIN_OPAQUE_OK", "STANDIN_FLAT", "STANDIN_SKIN"],
        Some(MAGENTA),
        4,
        &[(1, r"textures\standin\head_N.dds"), (2, r"textures\standin\head_sk.dds"), (7, r"Textures\Standin\Light\head_s.dds")],
    );
    assert_eq!(r.draw_default_at(CYL1, 71), Some((SrvKind::Tex2D, [0, 0, 0, 0])), "RFAOS guesses never fill");
}

#[test]
fn a_dropped_swap_of_a_missing_path_falls_back_to_the_fill() {
    let cache = tempfile::tempdir().unwrap();
    let mut r = cylinder(cache.path(), FLAT, Some(MAGENTA), 0, &[(0, r"textures\standin\missing_d.dds")]);
    r.replace_texture(DIFFUSE, &dds_rgba8(1, 1, &[[255, 0, 0, 255]]), Variant::Source).unwrap();
    assert_eq!(r.draw_default_at(CYL1, 0), None, "a real texture now");
    assert_eq!(r.draw_bound(CYL1), Some(vec![0]));
    r.replace_texture(DIFFUSE, &[], Variant::Source).unwrap();
    assert_eq!(r.draw_default_at(CYL1, 0), Some((SrvKind::Tex2D, MAGENTA)));
    assert_eq!(r.draw_bound(CYL1), Some(vec![]));
}

#[test]
fn the_fill_survives_a_device_rebuild() {
    let cache = tempfile::tempdir().unwrap();
    let mut r = renderer(cache.path(), FLAT);
    r.set_missing_texture(Some(MAGENTA));
    r.rebuild_device_on(Gpu::new_warp().unwrap()).unwrap();
    let mut m = nif::material::MaterialState::builtin_default();
    m.alpha = Some(nif::props::Alpha { flags: 0x200, threshold: 0 });
    m.set_slot_raw(0, r"textures\standin\missing_d.dds");
    r.load_builtin(Builtin::Sphere, m, &|_| None);
    assert_eq!(r.draw_default_at(ShapeId(0), 0), Some((SrvKind::Tex2D, MAGENTA)));
}

/// Opt-in: an env-mapped shape (type 1) whose cube map is missing gets a cube of the fill at t4 on the installed CS
/// (the stand-in reflects no TextureCube). `SDC_CS_SHADERS=…\Shaders cargo test -p render3d --release --test missing_fill -- --ignored`
#[test]
#[ignore]
fn installed_cs_fills_a_missing_env_cube_with_a_cube() {
    let root = std::path::PathBuf::from(std::env::var("SDC_CS_SHADERS").expect("set SDC_CS_SHADERS to the CS Shaders folder"));
    let fp = render3d::locate::fingerprint(&root, None).unwrap();
    let cache = tempfile::tempdir().unwrap();
    let mut r = Renderer::with_gpu(Gpu::new_warp().unwrap(), fp, cache.path().to_path_buf()).unwrap();
    r.set_missing_texture(Some(MAGENTA));
    let mut m = nif::material::MaterialState::builtin_default();
    m.shader_type = 1;
    m.slsf1 |= 1 << 7; // Environment_Mapping
    m.set_slot_raw(4, r"textures\cubemaps\sdc_missing_e.dds");
    let rep = r.load_builtin(Builtin::Sphere, m, &|_| None);
    assert!(rep.compile_errors.is_empty(), "{rep:?}");
    assert_eq!(r.draw_default_at(ShapeId(0), 4), Some((SrvKind::Cube, MAGENTA)));
}
```

- [ ] **Step 2: Run them to see them fail**

Run: `cd /f/ai/dds/converter-nif-guisug && CARGO_TARGET_DIR=F:/ai/dds/converter-nif-p1/target cargo test -p render3d --release --test missing_fill`
Expected: compile error, `no method named set_missing_texture`.

- [ ] **Step 3: Implement in render3d**

`TexEntry` (lib.rs:197-202) gains a field:

```rust
#[derive(Clone, Default)]
struct TexEntry {
    source: Option<textures::Texture>,
    encoded: Option<textures::Texture>,
    active: Variant,
    /// A texture-set slot path the resolver did not return (a `LoadReport::missing_textures` line); never set for an
    /// RFAOS guess. Kept through swaps, so a dropped swap falls back to the missing-texture fill.
    missing: bool,
}
```

`Renderer` gains, after `backdrop` (line 324), and `with_gpu` sets it to `None` (next to `backdrop: None,` at line 403):

```rust
    /// [`Renderer::set_missing_texture`]: the colour a missing colour-slot texture shows; `None` = neutral defaults.
    missing_fill: Option<[u8; 4]>,
```

Add after `set_encode_srgb` (line 605):

```rust
    /// `Some(rgba)`: a slot path the resolver did not return shows `rgba` (1×1, 2D or cube as the register reads it) in
    /// colour registers only: t0, and t4 when it reflects as a cube (the env map). Data registers keep `default_for`.
    /// `Bound` is not changed (material flags stay those of a missing file). Applies from the next load or rebind.
    pub fn set_missing_texture(&mut self, fill: Option<[u8; 4]>) {
        self.missing_fill = fill;
    }
```

`texture()` (670-702) gets a `report_missing: bool` parameter and records it:

```rust
    fn texture(
        &mut self,
        norm: &str,
        linear: bool,
        report_missing: bool,
        textures: &dyn Fn(&str) -> Option<Vec<u8>>,
        rep: &mut LoadReport,
    ) -> TexSlot {
        // … unchanged up to the match …
        match textures(norm) {
            None => {
                entry.missing = report_missing;
                if !rep.missing_textures.iter().any(|m| m == norm) {
                    rep.missing_textures.push(norm.to_string());
                }
            }
            // … Some arm unchanged …
        }
        // … unchanged …
    }
```

`resolve_textures` (776-798): the slot call becomes `self.texture(&path.norm, false, true, textures, rep);` and the RFAOS call becomes `self.texture(&c, true, false, textures, &mut quiet)`. (`set_material` still does `tex_cache.retain(|_, e| e.any())`, which drops missing entries so they are asked again; `texture()` then sets `missing` again. Leave that as it is.)

`bind_textures` (707-772): collect the registers whose slot path is missing, and use the fill in the default loop. Replace the slot loop's `match` (727-741) and the default loop (756-767) with:

```rust
            let entry = self.tex_cache.get(&path.norm);
            match entry.and_then(TexEntry::current).map(|t| (t.srv.clone(), t.kind)) {
                Some((srv, k)) if k == kind => {
                    real.insert(reg, srv);
                }
                Some(_) => rep.warnings.push(format!(
                    "{name}: {} is not a {kind:?} texture (t{reg}); default bound",
                    path.norm
                )),
                None => {
                    if entry.is_some_and(|e| e.missing) {
                        missing.insert(reg);
                    }
                }
            }
```

with `let mut missing: HashSet<u32> = HashSet::new();` declared next to `real` (line 716), and

```rust
        let mut srvs = vec![];
        for (_, reg, kind) in &program.ps.reflection.srvs {
            if let Some(v) = real.get(reg) {
                srvs.push((*reg, v.clone()));
                continue;
            }
            let rgba = match self.missing_fill {
                Some(fill) if missing.contains(reg) && colour_register(*reg, *kind) => fill,
                _ => textures::default_for(*reg, m.shader_type, pbr),
            };
            let v = self
                .default_srv(*kind, rgba)
                .map_err(|e| format!("default texture: {e}"))?;
            srvs.push((*reg, v));
        }
```

`rebind` (1424-1476): the `None` arm of `let srv = match &real` (line 1444) becomes:

```rust
                None => {
                    let rgba = match self.missing_fill {
                        Some(fill) if entry.missing && colour_register(reg, kind) => fill,
                        _ => textures::default_for(reg, shader_type, pbr),
                    };
                    self.default_srv(kind, rgba)?
                }
```

`rebuild_device_on` (1058-1070): add `n.missing_fill = self.missing_fill;` before `*self = n;`.

After `draw_identity` (line 1532), add the two hooks:

```rust
    /// Test hook: the `(kind, rgba)` of the 1×1 default bound at `reg` of the shape's draw; `None` when a real
    /// texture (or an override) is bound there, or there is no draw.
    #[doc(hidden)]
    pub fn draw_default_at(&self, id: ShapeId, reg: u32) -> Option<(SrvKind, [u8; 4])> {
        use windows::core::Interface;
        let d = self.shapes.get(&id)?.draw.as_ref()?;
        let bound = d.srvs.iter().find(|(r, _)| *r == reg)?.1.as_raw();
        self.defaults
            .iter()
            .find(|(_, v)| v.as_raw() == bound)
            .map(|(k, _)| *k)
    }

    /// Test hook: the registers the shape's draw counts as real textures (`Bound`), sorted.
    #[doc(hidden)]
    pub fn draw_bound(&self, id: ShapeId) -> Option<Vec<u32>> {
        let d = self.shapes.get(&id)?.draw.as_ref()?;
        let mut v: Vec<u32> = d.bound.regs.iter().copied().collect();
        v.sort_unstable();
        Some(v)
    }
```

After `settings_define` (line 1740), add:

```rust
/// Owner D3: the missing-texture fill goes to colour registers only, the base colour t0 and the env cube at t4.
/// PBR t4 (`m_p`) and FaceGen t4 (detail) reflect as 2D and stay neutral.
fn colour_register(reg: u32, kind: SrvKind) -> bool {
    reg == 0 || (reg == 4 && kind == SrvKind::Cube)
}
```

- [ ] **Step 4: Run the render3d tests**

```bash
cd /f/ai/dds/converter-nif-guisug
CARGO_TARGET_DIR=F:/ai/dds/converter-nif-p1/target cargo test -p render3d --release --test missing_fill
CARGO_TARGET_DIR=F:/ai/dds/converter-nif-p1/target cargo test -p render3d --release --test hotswap --test skin_rfaos --test edit
```
Expected: PASS (the 5 new tests; the existing hot-swap / RFAOS / edit tests unchanged).

- [ ] **Step 5: Commit render3d**

```bash
git add crates/render3d/src/lib.rs crates/render3d/tests/missing_fill.rs
git commit -m "render3d: set_missing_texture, a fill for missing colour-slot textures (t0, env cube)

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

- [ ] **Step 6: GUI consumer**

In `crates/gui/src/model/viewport3d.rs`:
- In `render_loop`'s `make` closure (2027-2033), after `r.set_async_compile(true);` add:

```rust
        // AG76 → render3d's fill (GUI suggestions item 3): colour slots only, so a missing normal no longer tints.
        r.set_missing_texture(Some([255, 0, 255, 255]));
```

- In `load_all` (1959-1978), delete the four lines from the `// AG76: …` comment through the `for p in &rep.missing_textures { … }` loop (1967-1972).
- Delete `magenta_dds` and its doc comment (520-523).

In `crates/gui/tests/viewport3d.rs`, delete the test `magenta_is_a_solid_magenta_dds` (2625-2631).

- [ ] **Step 7: Run the GUI tests, gate, commit**

```bash
cd /f/ai/dds/converter-nif-guisug
CARGO_TARGET_DIR=F:/ai/dds/converter-nif-p1/target cargo test -p sdc-gui --release --test viewport3d
CARGO_TARGET_DIR=F:/ai/dds/converter-nif-p1/target cargo fmt --all --check
CARGO_TARGET_DIR=F:/ai/dds/converter-nif-p1/target cargo clippy --workspace --all-targets --release -- -D warnings
git add crates/gui/src/model/viewport3d.rs crates/gui/tests/viewport3d.rs
git commit -m "gui: the 3D view uses render3d's missing-texture fill instead of per-path magenta swaps

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```
Expected: viewport3d tests PASS (`missing_textures_are_listed_in_magenta_with_show_in_textures` stays; the `MAGENTA` colour in `ui/viewport3d.rs:16` still colours that list and stays).

---

### Task 4: HDRI yaw shared with the sun (D4) and an RGB solid background

**Files:**
- Modify: `crates/render3d/src/scene.rs`: `Background` (45-62), `Scene` (96-106), `ambient()` (137-142), `neutral()` (151-170)
- Modify: `crates/render3d/src/hdri.rs`: `ambient6` (172-220), `Studio` + `studio()` (222-236), tests
- Modify: `crates/render3d/src/background.rs:42, 199-202`
- Modify: `crates/render3d/src/lib.rs`: clear (1648-1654), backdrop draw (1669-1671)
- Modify: `crates/render3d/tests/background.rs` (30, 45-46, 99-152 + new tests), `crates/render3d/tests/cs_features.rs:84`
- Modify: `crates/gui/src/model/viewport3d.rs`: `Look` (353-450)
- Modify: `crates/gui/src/settings/layout.rs`: `LookSettings` (101-133)
- Modify: `crates/gui/src/ui/viewport3d.rs`: 168-192, 218-220
- Modify: `crates/gui/tests/viewport3d.rs` (1014, 1033-1058), `crates/gui/tests/window_layout.rs` (35-42, 129, 159-178, 331-339 + new test), `crates/gui/tests/color_fields.rs:179`

**Interfaces:**
- Produces: `Scene.env_yaw: f32` (radians about world +Z, the sun's sense; `Scene::neutral()` = 0); `Background::Solid([f32; 3])` (linear RGB); `Background::rgb(srgb: [f32; 3]) -> Background`; `Background::grey(srgb: f32)` unchanged in meaning; `hdri::Cell = ([f32; 3], f64, [f64; 3])`; `pub fn hdri::cells(img: &Hdr) -> Vec<Cell>`; `pub fn hdri::integrate(cells: &[Cell], yaw: f32) -> [[f32; 3]; 6]`; `pub fn Studio::ambient6_at(&self, yaw: f32) -> [[f32; 3]; 6]`; `Backdrop::draw(&self, gpu, view, encode, yaw: f32)`. GUI: `Look.bg: [f32; 3]` (replaces `Look.grey`); `LookSettings.bg: Option<[f32; 3]>` (`grey` kept for reading).

- [ ] **Step 1: Write the failing render3d tests**

Append to `mod tests` in `crates/render3d/src/hdri.rs`:

```rust
    #[test]
    fn yaw_zero_is_todays_ambient_and_a_half_turn_swaps_the_sides() {
        let s = studio();
        assert_eq!(s.ambient6_at(0.0), s.ambient6, "bit-equal at yaw 0");
        assert_eq!(integrate(&cells(&s.image), 0.0), ambient6(&s.image));
        let (a, b) = (s.ambient6_at(0.0), s.ambient6_at(PI));
        // +X −X +Y −Y +Z −Z: the half turn swaps ±X and ±Y and keeps ±Z.
        for (got, from) in [(0, 1), (1, 0), (2, 3), (3, 2), (4, 4), (5, 5)] {
            for k in 0..3 {
                assert!((b[got][k] - a[from][k]).abs() < 1e-4, "axis {got}: {b:?} vs {a:?}");
            }
        }
    }

    /// The sign test: the environment turns by +yaw about Z, as `Look::rotate_sun` turns the sun, so what lit +X at
    /// yaw 0 lights +Y at a quarter turn (and the backdrop shader samples `azimuth - yaw`, tests/background.rs).
    #[test]
    fn a_quarter_turn_moves_the_ambient_the_way_the_sun_turns() {
        let s = studio();
        let (a, b) = (s.ambient6_at(0.0), s.ambient6_at(PI / 2.0));
        for (got, from) in [(2, 0), (1, 2), (3, 1), (0, 3), (4, 4)] {
            for k in 0..3 {
                assert!((b[got][k] - a[from][k]).abs() < 1e-4, "axis {got}");
            }
        }
    }
```

In `crates/render3d/tests/background.rs`:
- line 30: `assert_eq!(Scene::neutral().background, Background::Solid([0.0; 3]));`
- lines 45-48 become:

```rust
    match Background::grey(0.5) {
        Background::Solid(g) => assert!(g.iter().all(|c| (c - 0.214).abs() < 1e-3), "{g:?}"),
        b => panic!("{b:?}"),
    }
```

- Extract the smooth-azimuth search of `hdri_background_matches_the_cpu_lookup_of_the_same_direction` (lines 103-133) into a helper and add a turned case. Replace lines 99-152 with:

```rust
/// The horizontal camera yaw whose view direction lands on the smoothest unclipped part of the image, and the image
/// mean there (a 13×13 window), so a 1-pixel centre can be compared with it.
fn smooth_azimuth() -> (f32, f32) {
    let img = &hdri::studio().image;
    let at = |yaw: f32| -> Vec<f32> {
        // The camera sits at (sin yaw, -cos yaw, 0) from the target and looks along the opposite.
        let (u, v) = hdri::dir_to_uv([-yaw.sin(), yaw.cos(), 0.0]);
        let (cx, cy) = ((u * img.width as f32) as i32, (v * img.height as f32) as i32);
        let mut g = vec![];
        for dy in -6..=6 {
            for dx in -6..=6 {
                let x = (cx + dx).rem_euclid(img.width as i32) as usize;
                let y = (cy + dy).clamp(0, img.height as i32 - 1) as usize;
                g.push(img.rgb[(y * img.width + x) * 3 + 1]);
            }
        }
        g
    };
    let mut best = (f32::MAX, 0.0f32, 0.0f32);
    for k in 0..72 {
        let yaw = k as f32 * std::f32::consts::TAU / 72.0;
        let g = at(yaw);
        let mean = g.iter().sum::<f32>() / g.len() as f32;
        let var = g.iter().map(|v| (v - mean).powi(2)).sum::<f32>() / g.len() as f32;
        if mean > 0.05 && mean < 0.8 && var < best.0 {
            best = (var, yaw, mean);
        }
    }
    (best.1, best.2)
}

/// The centre pixel of a frame looking along camera yaw `cam_yaw` with the environment turned by `env_yaw`.
fn centre_green(cam_yaw: f32, env_yaw: f32) -> f32 {
    let cache = tempfile::tempdir().unwrap();
    let mut r = renderer(cache.path(), &["STANDIN_OPAQUE_OK"]);
    let mut s = Scene::neutral();
    s.background = Background::Hdri;
    s.env_yaw = env_yaw;
    s.camera = Camera::Orbit { target: [0.0; 3], yaw: cam_yaw, pitch: 0.0, distance: 5.0 };
    let f = r.render(&s, 64, 64).unwrap();
    // encode_srgb is off here: the shader gives display (sRGB) values.
    render3d::scene::srgb_to_linear(px(&f, 64, 32, 32)[1] as f32 / 255.0)
}

#[test]
fn hdri_background_matches_the_cpu_lookup_of_the_same_direction() {
    let (yaw, mean) = smooth_azimuth();
    let got = centre_green(yaw, 0.0);
    assert!((got - mean).abs() < 0.06, "rendered {got} vs image {mean} (yaw {yaw})");
}

/// The sign test, backdrop half: turning the environment by +π/2 (the sun's sense) moves what was straight ahead a
/// quarter turn on, where the camera turned by the same quarter finds it again.
#[test]
fn a_turned_hdri_shows_the_same_spot_a_quarter_turn_on() {
    let (yaw, mean) = smooth_azimuth();
    let q = std::f32::consts::FRAC_PI_2;
    let got = centre_green(yaw + q, q);
    assert!((got - mean).abs() < 0.06, "rendered {got} vs image {mean} (yaw {yaw} + π/2)");
}

#[test]
fn an_rgb_background_clears_to_that_colour_with_and_without_encoding() {
    let cache = tempfile::tempdir().unwrap();
    let mut r = sphere(cache.path());
    let mut s = Scene::neutral();
    s.background = Background::rgb([1.0, 0.0, 0.0]);
    for encode in [true, false] {
        r.set_encode_srgb(encode);
        let f = r.render(&s, 64, 64).unwrap();
        assert_eq!(px(&f, 64, 0, 0), [255, 0, 0], "encode {encode}");
    }
    // Clamped per channel before the sRGB decode.
    assert_eq!(
        Background::rgb([0.5, 2.0, -1.0]),
        Background::Solid([render3d::scene::srgb_to_linear(0.5), 1.0, 0.0])
    );
}
```

`crates/render3d/tests/cs_features.rs:84`: `scene.background = Background::Solid([0.5; 3]);`

- [ ] **Step 2: Run them to see them fail**

Run: `cd /f/ai/dds/converter-nif-guisug && CARGO_TARGET_DIR=F:/ai/dds/converter-nif-p1/target cargo test -p render3d --release --test background`
Expected: compile errors (`Solid` takes one `f32`, no `env_yaw`, no `rgb`).

- [ ] **Step 3: Implement in render3d**

`crates/render3d/src/scene.rs`, `Background` (45-62):

```rust
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Background {
    /// A flat colour: **linear** RGB. With `Renderer::set_encode_srgb(true)` it is encoded with the rest of the frame
    /// at readback; without it (gamma-space output) the renderer writes its sRGB encoding, so the same value looks the
    /// same either way. A perceptual (sRGB) colour from the UI goes through [`Background::rgb`] / [`Background::grey`].
    Solid([f32; 3]),
    /// The bundled studio HDRI ([`crate::hdri`]) by view direction, turned by `Scene::env_yaw`. It also replaces
    /// `Scene::ambient6`: the ambient light is taken from the same image ([`Scene::ambient`]).
    Hdri,
}

impl Background {
    /// A solid grey from an sRGB-perceptual value in 0..1 (0 = black, 1 = white).
    pub fn grey(srgb: f32) -> Background {
        Background::rgb([srgb; 3])
    }

    /// A solid colour from sRGB-perceptual channels in 0..1, each clamped then decoded to linear.
    pub fn rgb(srgb: [f32; 3]) -> Background {
        Background::Solid(srgb.map(|c| srgb_to_linear(c.clamp(0.0, 1.0))))
    }
}
```

`Scene` (96-106) gains, after `ambient6`:

```rust
    /// Environment turn about world +Z, radians, in the sun's sense (a feature at azimuth φ shows at φ + yaw). Turns the
    /// HDRI backdrop and the ambient taken from it; 0 = the image as bundled. No effect with a solid background.
    pub env_yaw: f32,
```

`ambient()` (137-142):

```rust
    pub fn ambient(&self) -> [[f32; 3]; 6] {
        match self.background {
            Background::Solid(_) => self.ambient6,
            Background::Hdri => crate::hdri::studio().ambient6_at(self.env_yaw),
        }
    }
```

`neutral()`: `background: Background::Solid([0.0; 3]),` and add `env_yaw: 0.0,` after `ambient6: [ … ],`.

`crates/render3d/src/hdri.rs`: replace `ambient6` (172-220) and `Studio`/`studio()` (222-236) with:

```rust
/// One cell of the ≤ 64×32 solid-angle box-downsample: world direction of its centre, solid-angle weight, mean
/// radiance.
pub type Cell = ([f32; 3], f64, [f64; 3]);

const AXES: [[f32; 3]; 6] = [
    [1.0, 0.0, 0.0],
    [-1.0, 0.0, 0.0],
    [0.0, 1.0, 0.0],
    [0.0, -1.0, 0.0],
    [0.0, 0.0, 1.0],
    [0.0, 0.0, -1.0],
];

/// The image box-downsampled to at most 64×32 cells, solid-angle weighted (so the poles do not dominate).
pub fn cells(img: &Hdr) -> Vec<Cell> {
    let (ow, oh) = (img.width.min(64), img.height.min(32));
    let mut out = Vec::with_capacity(ow * oh);
    for oy in 0..oh {
        let (y0, y1) = (oy * img.height / oh, (oy + 1) * img.height / oh);
        let v = (oy as f32 + 0.5) / oh as f32;
        let solid = (v * PI).sin() as f64;
        for ox in 0..ow {
            let (x0, x1) = (ox * img.width / ow, (ox + 1) * img.width / ow);
            let mut c = [0.0f64; 3];
            for y in y0..y1 {
                for x in x0..x1 {
                    for (k, s) in c.iter_mut().enumerate() {
                        *s += img.rgb[(y * img.width + x) * 3 + k] as f64;
                    }
                }
            }
            let n = ((y1 - y0) * (x1 - x0)).max(1) as f64;
            out.push((uv_to_dir((ox as f32 + 0.5) / ow as f32, v), solid, c.map(|s| s / n)));
        }
    }
    out
}

/// Ambient colours for the six DALC axes (+X −X +Y −Y +Z −Z, world space) of the environment turned by `yaw` about
/// +Z: for each axis the cosine-weighted mean radiance over the hemisphere around it. A turned environment seen from
/// axis `a` is the original seen from `a` turned by −yaw.
pub fn integrate(cells: &[Cell], yaw: f32) -> [[f32; 3]; 6] {
    let (s, c) = (-yaw).sin_cos();
    let axes = AXES.map(|a| [a[0] * c - a[1] * s, a[0] * s + a[1] * c, a[2]]);
    let mut sum = [[0.0f64; 3]; 6];
    let mut wsum = [0.0f64; 6];
    for (d, solid, mean) in cells {
        for (a, axis) in axes.iter().enumerate() {
            let cos = (d[0] * axis[0] + d[1] * axis[1] + d[2] * axis[2]).max(0.0) as f64;
            let w = cos * solid;
            wsum[a] += w;
            for k in 0..3 {
                sum[a][k] += w * mean[k];
            }
        }
    }
    let mut out = [[0.0f32; 3]; 6];
    for a in 0..6 {
        for k in 0..3 {
            out[a][k] = (sum[a][k] / wsum[a].max(1e-12)) as f32;
        }
    }
    out
}

/// [`integrate`] of the unturned image (the ambient the preview always had).
pub fn ambient6(img: &Hdr) -> [[f32; 3]; 6] {
    integrate(&cells(img), 0.0)
}

/// The bundled studio HDRI with its ambient, decoded on first use.
pub struct Studio {
    pub image: Hdr,
    /// = `ambient6_at(0.0)`.
    pub ambient6: [[f32; 3]; 6],
    cells: Vec<Cell>,
}

impl Studio {
    /// The ambient of the environment turned by `yaw` (radians about +Z), [`AMBIENT_SCALE`] applied. About 12k
    /// multiply-adds per call; `Scene::ambient` calls it at most twice a frame.
    /// shortcut: no memo; add a `(yaw bits, result)` memo if a profile shows it.
    pub fn ambient6_at(&self, yaw: f32) -> [[f32; 3]; 6] {
        integrate(&self.cells, yaw).map(|c| c.map(|v| v * AMBIENT_SCALE))
    }
}

/// Decodes the bundled HDRI once (about 0.1 s) and caches it for the process.
pub fn studio() -> &'static Studio {
    static STUDIO: OnceLock<Studio> = OnceLock::new();
    STUDIO.get_or_init(|| {
        let image = decode_rgbe(STUDIO_HDR).expect("the bundled HDRI decodes");
        let cells = cells(&image);
        let ambient6 = integrate(&cells, 0.0).map(|c| c.map(|v| v * AMBIENT_SCALE));
        Studio { image, ambient6, cells }
    })
}
```

(The two existing `ambient6` unit tests stay valid: `ambient6` is `integrate(cells, 0)` with the same arithmetic; the per-cell mean `c[k] / n` is now taken once per cell instead of per axis. If `bright_top_gives_bright_up_ambient` or `uniform_image_gives_uniform_ambient` moves by more than their tolerances, stop and report: it would mean the refactor changed the maths.)

`crates/render3d/src/background.rs`: line 42 becomes

```hlsl
    float u = frac((atan2(d.y, d.x) + YAW - Flags.y) / 6.2831853 + 0.5);
```

and `draw` (199-202):

```rust
    /// Draws the background into the bound target (viewport and render target already set), the environment turned by
    /// `yaw` radians about +Z (`Flags.y`). Leaves the default depth state and no input layout.
    pub fn draw(&self, gpu: &Gpu, view: &View, encode: bool, yaw: f32) {
        let flags = [if encode { 1.0f32 } else { 0.0 }, yaw, 0.0, 0.0];
```

`crates/render3d/src/lib.rs` `render`: the clear (1648-1654) becomes

```rust
        let clear = match scene.background {
            // The target is read back as is when not sRGB-encoding: give it the display value then.
            Background::Solid(c) if self.encode_srgb => c,
            Background::Solid(c) => c.map(scene::linear_to_srgb),
            Background::Hdri => [0.0; 3],
        };
        target.clear(gpu, [clear[0], clear[1], clear[2], 1.0]);
```

and line 1670: `b.draw(gpu, &view, !self.encode_srgb, scene.env_yaw);`. (`Scene != last`, lib.rs:1626, already re-uploads the light rows when `env_yaw` changes.)

- [ ] **Step 4: Run the render3d tests, commit**

```bash
cd /f/ai/dds/converter-nif-guisug
CARGO_TARGET_DIR=F:/ai/dds/converter-nif-p1/target cargo test -p render3d --release --lib hdri
CARGO_TARGET_DIR=F:/ai/dds/converter-nif-p1/target cargo test -p render3d --release --test background --test cs_features --test lights --test frame
git add crates/render3d/src/scene.rs crates/render3d/src/hdri.rs crates/render3d/src/background.rs crates/render3d/src/lib.rs crates/render3d/tests/background.rs crates/render3d/tests/cs_features.rs
```
Expected: PASS. Do not commit yet: the GUI does not build until Step 6 (`Background::Solid(f32)` is gone). Steps 5-7 finish the task; the commit is in Step 7.

- [ ] **Step 5: Write the failing GUI tests**

`crates/gui/tests/viewport3d.rs`:
- line 1014: `|l| l.bg = [0.5, 0.25, 0.0],`
- in `the_default_look_is_the_unchanged_scene` (1033-1058): line 1040 becomes `assert_eq!(s.background, Background::Solid([0.0; 3]));` and append before the closing brace:

```rust
    // The HDRI turns with the sun (owner D4): one yaw for both.
    let turned = v3::Look { sun_yaw: 1.25, ..l }.scene(Camera::Auto, None);
    assert_eq!(turned.env_yaw, 1.25);
    assert_eq!(l.scene(Camera::Auto, None).env_yaw, 0.0);
```

`crates/gui/tests/window_layout.rs`:
- `full()` look (35-42): `grey: None,` and add `bg: Some([0.25; 3]),` after it.
- line 129 stays (`grey` is still read and clamped). After it add:

```rust
    assert_eq!(look.bg, None, "no bg in that file");
```

- the `Look { … }` literal at 160-167: `grey: 0.5,` → `bg: [0.5, 0.25, 0.0],`
- the `Look { … }` literal at 332-339: `grey: 0.25,` → `bg: [0.25; 3],`
- new test at the end of the file:

```rust
/// GUI suggestions item 2: `bg` replaced the grey slider. An old `grey` still loads (as that grey), and a broken
/// `bg` falls back to it.
#[test]
fn an_old_grey_background_loads_as_that_grey_and_a_bad_bg_falls_back() {
    let (s, err) = load_text("[layout.look]\ngrey = 0.25\n");
    assert_eq!(err, None);
    assert_eq!(Look::from_settings(s.layout.look.as_ref()).bg, [0.25; 3]);
    let (s, _) = load_text("[layout.look]\ngrey = 0.5\nbg = [1.0, 0.5]\n");
    assert_eq!(Look::from_settings(s.layout.look.as_ref()).bg, [0.5; 3], "wrong arity: bg ignored");
    let (s, _) = load_text("[layout.look]\nbg = \"red\"\n");
    assert_eq!(Look::from_settings(s.layout.look.as_ref()).bg, Look::default().bg);
    let (s, _) = load_text("[layout.look]\nbg = [2.0, 0.5, -1.0]\n");
    assert_eq!(s.layout.look.unwrap().bg, Some([1.0, 0.5, 0.0]), "clamped per channel");
    // What the app writes back has bg and no grey.
    let text = toml::to_string(&LookSettings::of(&Look { bg: [0.1, 0.2, 0.3], ..Look::default() })).unwrap();
    assert!(text.contains("bg") && !text.contains("grey"), "{text}");
}
```

(If `toml` is not already imported in `window_layout.rs`, use the full path `toml::to_string`; `toml` is a dependency of sdc-gui. If `LookSettings` is not in scope there, it is `sdc_gui::settings::LookSettings`, as `full()` already names it.)

`crates/gui/tests/color_fields.rs:179`:

```rust
    assert!(h.state().look.bg.iter().all(|c| (c - 128.0 / 255.0).abs() < 1e-6), "{:?}", h.state().look.bg);
```

- [ ] **Step 6: Implement the GUI**

`crates/gui/src/model/viewport3d.rs`, `Look` (353-371): replace the `grey` field and the `sun_yaw` doc with

```rust
    /// The flat background colour, sRGB 0..=1 per channel: `[0.0; 3]` = black (GUI suggestions item 2; was `grey`).
    pub bg: [f32; 3],
```

```rust
    /// The sun's turn about the vertical (world Z), radians in 0..2π (Shift+right drag). The studio HDRI turns with it
    /// (owner D4: `Scene::env_yaw`).
    pub sun_yaw: f32,
```

`Look::scene` (378-387):

```rust
        let mut s = Scene {
            camera,
            background: if self.hdri {
                Background::Hdri
            } else {
                Background::rgb(self.bg)
            },
            sun_brightness: self.sun.clamp(Self::SUN_RANGE.0, Self::SUN_RANGE.1),
            env_yaw: self.sun_yaw,
            ..Scene::neutral()
        };
```

`from_settings` (415): `bg: s.bg.or(s.grey.map(|g| [g; 3])).unwrap_or(d.bg),` (instead of `grey: …`).
`LookSettings::of` (428): `grey: None,` and add `bg: Some(l.bg),`.
`Default for Look` (442): `bg: [0.0; 3],` instead of `grey: 0.0,`.

`crates/gui/src/settings/layout.rs`, `LookSettings` (101-115): keep `grey` and add after it:

```rust
    /// The solid background colour, sRGB 0..=1 per channel (GUI suggestions item 2). `grey` is read only when this is
    /// missing (a gui.toml from before), and never written.
    #[serde(deserialize_with = "lenient")]
    pub bg: Option<[f32; 3]>,
```

and in `clamp` (118-132), after `fix(&mut self.grey, (0.0, 1.0));`:

```rust
        self.bg = self
            .bg
            .filter(|c| c.iter().all(|f| f.is_finite()))
            .map(|c| c.map(|f| f.clamp(0.0, 1.0)));
```

`crates/gui/src/ui/viewport3d.rs`, the solid-background block (172-191) becomes:

```rust
                if !l.hdri {
                    // GUI suggestions item 2: one sRGB colour; the swatch's square and the hex box replace the grey slider.
                    let id = egui::Id::new("viewport3d-bg");
                    if let Some(rgb) = super::color::color_field(ui, id, l.bg.map(super::color::unit_to_u8)) {
                        l.bg = rgb.map(super::color::u8_to_unit);
                    }
                }
```

and the HDRI note (219): `ui.weak("Studio HDRI by Poly Haven (CC0): background and ambient light. Shift+right drag turns the sun and the HDRI.");`

- [ ] **Step 7: Run the GUI tests, gate, commit**

```bash
cd /f/ai/dds/converter-nif-guisug
CARGO_TARGET_DIR=F:/ai/dds/converter-nif-p1/target cargo test -p sdc-gui --release --test viewport3d --test window_layout --test color_fields
CARGO_TARGET_DIR=F:/ai/dds/converter-nif-p1/target cargo fmt --all --check
CARGO_TARGET_DIR=F:/ai/dds/converter-nif-p1/target cargo clippy --workspace --all-targets --release -- -D warnings
git add crates/gui/src/model/viewport3d.rs crates/gui/src/settings/layout.rs crates/gui/src/ui/viewport3d.rs crates/gui/tests/viewport3d.rs crates/gui/tests/window_layout.rs crates/gui/tests/color_fields.rs
git commit -m "render3d+gui: the HDRI turns with the sun (Scene::env_yaw) and the solid background is RGB

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```
Expected: PASS. The render3d files staged in Step 4 go into this commit, so the commit builds.

---

### Task 5: `override_slot` (UV checker by slot)

**Files:**
- Modify: `crates/render3d/src/lib.rs`: `Renderer` struct + `with_gpu`, `bind_textures`, `rebuild_device_on`, `rebind`; new `override_slot`, `override_at`, `slot_targets`, `plain_srv`
- Create: `crates/render3d/tests/override_slot.rs`
- Modify: `crates/gui/src/model/viewport3d.rs`: `V3Cmd::SetDebug` doc (584-586), `diffuse_paths` (525-536, delete), `load_all` (1974-1976), `checker_over` (1980-1988, delete), the `SetDebug` arm (2160-2170), the refresh at 2280-2283 (delete)
- Modify: `crates/gui/tests/viewport3d.rs`: 2604-2623 (drop the `diffuse_paths` part), new real-thread test

**Interfaces:**
- Consumes: `colour_register`, `TexEntry.missing`, `Renderer.missing_fill` (Task 3).
- Produces: `pub fn Renderer::override_slot(&mut self, slot: usize, dds: Option<&[u8]>) -> Result<Vec<(ShapeId, u32)>, String>`; `Renderer.overrides: BTreeMap<usize, (Arc<[u8]>, textures::Texture)>`; private `fn override_at(&self, m: &MaterialState, reg: u32, kind: SrvKind) -> Option<ID3D11ShaderResourceView>` (Task 7 does not need it).

- [ ] **Step 1: Write the failing tests** (`crates/render3d/tests/override_slot.rs`)

```rust
//! `Renderer::override_slot` (GUI suggestions item 4) on WARP with the stand-in. `STANDIN_FLAT`: pixels =
//! 0.25·diffuse, so the slot-0 texture shows in the colour.
mod common;
use common::{cache_files, channel, dds_cube_rgba8, dds_rgba8, fixture, renderer, FLAT};
use nif::material::MaterialState;
use render3d::device::Gpu;
use render3d::scene::Scene;
use render3d::{parse, EditKind, Renderer, ShapeId, Variant};
use std::path::Path;

const CYL1: ShapeId = ShapeId(9);
const PATH: &str = "textures/standin/swap.dds";

fn frame(r: &mut Renderer) -> Vec<u8> {
    r.render(&Scene::neutral(), 64, 64).unwrap()
}

fn green() -> Vec<u8> {
    dds_rgba8(1, 1, &[[0, 255, 0, 255]])
}

/// The furniture NIF with cylinder_1's slot 0 naming `PATH`, resolved to red (the Source variant).
fn furniture(cache: &Path) -> Renderer {
    let mut r = renderer(cache, FLAT);
    let rep = r.load(&fixture("TestNifFile_Furniture_Col_SE.nif"), &|_| None);
    assert_eq!(rep.shapes, 2, "{rep:?}");
    let mut m: MaterialState = r.shapes().into_iter().find(|s| s.id == CYL1).unwrap().material;
    m.set_slot_raw(0, r"Textures\Standin\Swap.dds");
    let red = dds_rgba8(1, 1, &[[255, 0, 0, 255]]);
    assert_eq!(r.set_material(CYL1, m, &|p| (p == PATH).then(|| red.clone())), Ok(EditKind::Textures));
    r
}

#[test]
fn an_override_shows_now_and_none_restores_without_a_load() {
    let cache = tempfile::tempdir().unwrap();
    let mut r = furniture(cache.path());
    let before = frame(&mut r);
    let (files, mesh) = (cache_files(cache.path()), r.draw_identity(CYL1));
    let touched = r.override_slot(0, Some(&green())).unwrap();
    assert!(touched.contains(&(CYL1, 0)), "{touched:?}");
    let on = frame(&mut r);
    assert!(channel(&on, 1) > channel(&before, 1) + 1000, "the checker (green) shows");
    assert!(r.override_slot(0, None).unwrap().contains(&(CYL1, 0)));
    assert_eq!(frame(&mut r), before, "the real texture is back");
    assert_eq!(r.draw_identity(CYL1), mesh, "no reload: same buffers");
    assert_eq!(cache_files(cache.path()), files, "nothing was compiled");
}

#[test]
fn the_override_survives_a_load_and_covers_an_empty_slot() {
    let cache = tempfile::tempdir().unwrap();
    let mut r = renderer(cache.path(), FLAT);
    r.override_slot(0, Some(&green())).unwrap();
    r.load(&fixture("TestNifFile_Furniture_Col_SE.nif"), &|_| None);
    let mut m: MaterialState = r.shapes().into_iter().find(|s| s.id == CYL1).unwrap().material;
    m.set_slot_raw(0, "");
    r.set_material(CYL1, m, &|_| None).unwrap();
    let f = frame(&mut r);
    assert!(channel(&f, 1) > channel(&f, 0) + 1000, "green over every slot 0, empty or not");
}

#[test]
fn a_variant_flip_under_an_override_shows_once_it_is_cleared() {
    let cache = tempfile::tempdir().unwrap();
    let mut r = furniture(cache.path());
    r.replace_texture(PATH, &dds_rgba8(1, 1, &[[0, 0, 255, 255]]), Variant::Encoded).unwrap();
    r.override_slot(0, Some(&green())).unwrap();
    let covered = frame(&mut r);
    r.set_texture_variant(PATH, Variant::Encoded);
    assert_eq!(frame(&mut r), covered, "the flip does not clobber the override");
    r.override_slot(0, None).unwrap();
    let f = frame(&mut r);
    assert!(channel(&f, 2) > channel(&f, 0) + 1000, "the flipped (blue) variant shows");
}

#[test]
fn a_bad_override_is_an_error_and_a_cube_for_a_2d_register_is_a_warning() {
    let cache = tempfile::tempdir().unwrap();
    let mut r = furniture(cache.path());
    let before = frame(&mut r);
    assert!(r.override_slot(0, Some(b"not a dds")).is_err());
    assert_eq!(frame(&mut r), before, "nothing changed");
    assert_eq!(r.override_slot(0, Some(&dds_cube_rgba8([0, 255, 0, 255]))), Ok(vec![]), "t0 reads 2D");
    assert_eq!(frame(&mut r), before);
    let rep = r.load(&fixture("TestNifFile_Furniture_Col_SE.nif"), &|_| None);
    assert!(rep.warnings.iter().any(|w| w.contains("slot 0 override")), "{:?}", rep.warnings);
}

#[test]
fn an_override_set_while_shapes_compile_lands_with_their_program() {
    let cache = tempfile::tempdir().unwrap();
    let mut r = renderer(cache.path(), FLAT);
    r.set_async_compile(true);
    let rep = r.load_parsed(&parse(&fixture("TestNifFile_Furniture_Col_SE.nif")), &|_| None);
    assert!(rep.pending > 0, "{rep:?}");
    assert_eq!(r.override_slot(0, Some(&green())), Ok(vec![]), "no draw yet");
    r.settle();
    let f = frame(&mut r);
    assert!(channel(&f, 1) > channel(&f, 0) + 1000);
}

#[test]
fn the_override_survives_a_device_rebuild() {
    let cache = tempfile::tempdir().unwrap();
    let mut r = renderer(cache.path(), FLAT);
    r.override_slot(0, Some(&green())).unwrap();
    r.rebuild_device_on(Gpu::new_warp().unwrap()).unwrap();
    r.load(&fixture("TestNifFile_Furniture_Col_SE.nif"), &|_| None);
    let f = frame(&mut r);
    assert!(channel(&f, 1) > channel(&f, 0) + 1000);
}
```

- [ ] **Step 2: Run them to see them fail**

Run: `cd /f/ai/dds/converter-nif-guisug && CARGO_TARGET_DIR=F:/ai/dds/converter-nif-p1/target cargo test -p render3d --release --test override_slot`
Expected: compile error, `no method named override_slot`.

- [ ] **Step 3: Implement in render3d**

`Renderer` gains (after `missing_fill`), and `with_gpu` sets `overrides: BTreeMap::new(),`:

```rust
    /// [`Renderer::override_slot`]: NIF slot → the DDS bytes (kept to re-upload on a new device) and the texture every
    /// shape shows there. Not cleared by a load.
    overrides: BTreeMap<usize, (Arc<[u8]>, textures::Texture)>,
```

`bind_textures`: the default loop (as Task 3 left it) starts with the override, and the kind mismatches become warnings. Add this as the first statement inside `for (_, reg, kind) in &program.ps.reflection.srvs {`:

```rust
            if let Some(v) = self.override_at(m, *reg, *kind) {
                srvs.push((*reg, v));
                continue;
            }
```

and right before `let mut srvs = vec![];`:

```rust
        for (slot, (_, t)) in &self.overrides {
            let Some(reg) = textures::register_for_slot(m.shader_type, pbr, msn, *slot) else {
                continue;
            };
            if let Some(kind) = draw::srv_kind_at(&program.ps.reflection, reg).filter(|k| *k != t.kind) {
                rep.warnings.push(format!(
                    "{name}: the slot {slot} override is not a {kind:?} texture (t{reg}); not shown"
                ));
            }
        }
```

(The slot loop is left as it is: `real`, and so `Bound`, keep the material's real textures. Ruling R9.)

New methods, after `set_texture_variant` (line 1386):

```rust
    /// Shows `dds` in NIF texture slot `slot` of every shape (through each shape's slot→register rule), now and for
    /// later loads, over both variants, even where the slot path is empty. `None` removes it and rebinds what a load
    /// would bind (the active texture, the missing fill or the default). `Bound` and the material are never changed.
    /// A register of another kind keeps its binding (a load lists it under `warnings`). Err, with nothing changed, for
    /// a DDS that does not decode. Returns the `(shape, register)` pairs rebound.
    pub fn override_slot(
        &mut self,
        slot: usize,
        dds: Option<&[u8]>,
    ) -> Result<Vec<(ShapeId, u32)>, String> {
        match dds {
            Some(b) => {
                let t = textures::load_dds(&self.gpu, b)
                    .map_err(|e| format!("slot {slot} override: {e}"))?;
                self.overrides.insert(slot, (Arc::from(b), t));
            }
            None => {
                if self.overrides.remove(&slot).is_none() {
                    return Ok(vec![]);
                }
            }
        }
        let mut touched = vec![];
        for (id, reg, kind) in self.slot_targets(slot) {
            let Some(built) = self.shapes[&id].draw.as_ref().map(|d| d.built.clone()) else {
                continue;
            };
            let srv = match self.overrides.get(&slot) {
                Some((_, t)) if t.kind == kind => t.srv.clone(),
                Some(_) => continue,
                None => self.plain_srv(&built, slot, reg, kind)?,
            };
            if let Some(e) = self
                .shapes
                .get_mut(&id)
                .and_then(|s| s.draw.as_mut())
                .and_then(|d| d.srvs.iter_mut().find(|(r, _)| *r == reg))
            {
                e.1 = srv;
                touched.push((id, reg));
            }
        }
        Ok(touched)
    }

    /// The override texture a draw built for `m` shows at `reg`, if an override's slot maps there and its kind is the
    /// register's.
    fn override_at(
        &self,
        m: &MaterialState,
        reg: u32,
        kind: SrvKind,
    ) -> Option<ID3D11ShaderResourceView> {
        let msn = m.slsf1 & (1 << 12) != 0;
        self.overrides
            .iter()
            .find(|(slot, (_, t))| {
                t.kind == kind
                    && textures::register_for_slot(m.shader_type, m.is_pbr(), msn, **slot) == Some(reg)
            })
            .map(|(_, (_, t))| t.srv.clone())
    }

    /// `(shape, register, kind)` for every visible draw whose built material maps NIF slot `slot` to a register it
    /// binds, with or without a path: [`Renderer::texture_targets`] keyed by slot.
    fn slot_targets(&self, slot: usize) -> Vec<(ShapeId, u32, SrvKind)> {
        let mut out = vec![];
        for (id, s) in &self.shapes {
            let Some(d) = s.draw.as_ref().filter(|_| s.status != ShapeStatus::Hidden) else {
                continue;
            };
            let m = &d.built;
            let msn = m.slsf1 & (1 << 12) != 0;
            let Some(reg) = textures::register_for_slot(m.shader_type, m.is_pbr(), msn, slot) else {
                continue;
            };
            let Some(view) = d.srvs.iter().find(|(r, _)| *r == reg).map(|(_, v)| v) else {
                continue;
            };
            if let Some(kind) = textures::view_kind(view) {
                out.push((*id, reg, kind));
            }
        }
        out
    }

    /// What `bind_textures` binds at `reg` for slot `slot` of `m` without an override: the active texture of the
    /// slot's path, else the missing fill (colour registers of a missing path) or the register's default.
    fn plain_srv(
        &mut self,
        m: &MaterialState,
        slot: usize,
        reg: u32,
        kind: SrvKind,
    ) -> Result<ID3D11ShaderResourceView, String> {
        let entry = m
            .slots
            .get(slot)
            .filter(|p| !p.norm.is_empty())
            .and_then(|p| self.tex_cache.get(&p.norm));
        if let Some(t) = entry.and_then(TexEntry::current).filter(|t| t.kind == kind) {
            return Ok(t.srv.clone());
        }
        let missing = entry.is_some_and(|e| e.missing);
        let rgba = match self.missing_fill {
            Some(fill) if missing && colour_register(reg, kind) => fill,
            _ => textures::default_for(reg, m.shader_type, m.is_pbr()),
        };
        self.default_srv(kind, rgba)
    }
```

`rebind` (1424-1476): a pair under an override keeps its SRV but still tracks `Bound`. In the planning loop, compute `under` from the built material and carry it:

```rust
        for (id, reg, kind) in self.texture_targets(path) {
            let (shader_type, pbr, name, under) = {
                let s = &self.shapes[&id];
                let b = s.draw.as_ref().map_or(&s.material, |d| &d.built);
                (b.shader_type, b.is_pbr(), s.name.clone(), self.override_at(b, reg, kind).is_some())
            };
            // … `real` and `srv` unchanged (with Task 3's fill arm) …
            plan.push((id, reg, srv, real.is_some(), under));
        }
```

and in the apply loop, `for (id, reg, srv, real, under) in plan {`, guard the SRV write and the touched entry:

```rust
            if !under {
                if let Some(slot) = d.srvs.iter_mut().find(|(r, _)| *r == reg) {
                    slot.1 = srv;
                }
            }
            // … `was`, the Bound insert/remove and the rewrite stay as they are …
            if !under {
                touched.push((id, reg));
            }
```

Update the `replace_texture` doc (1338-1344): add the sentence "A register under a slot override (`override_slot`) keeps showing the override; the entry and `Bound` still change, and the pair is not returned."

`rebuild_device_on`: before `*self = n;` add

```rust
        // Device objects do not move: upload each override again (it decoded before, so a failure is not expected).
        for (slot, (bytes, _)) in std::mem::take(&mut self.overrides) {
            match textures::load_dds(&n.gpu, &bytes) {
                Ok(t) => {
                    n.overrides.insert(slot, (bytes, t));
                }
                Err(e) => eprintln!("render3d: slot {slot} override dropped on the new device: {e}"),
            }
        }
```

- [ ] **Step 4: Run the render3d tests, commit**

```bash
cd /f/ai/dds/converter-nif-guisug
CARGO_TARGET_DIR=F:/ai/dds/converter-nif-p1/target cargo test -p render3d --release --test override_slot --test hotswap --test missing_fill --test edit
git add crates/render3d/src/lib.rs crates/render3d/tests/override_slot.rs
git commit -m "render3d: override_slot, a texture over one NIF slot of every shape, kept across loads

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```
Expected: PASS.

- [ ] **Step 5: Write the failing GUI test** (append to `crates/gui/tests/viewport3d.rs`, after `a_real_load_compiles_in_the_background_then_draws_and_goes_idle`)

```rust
/// GUI suggestions item 4: the UV checker goes on and off through render3d's slot override. Neither direction loads
/// the content again: a `Loaded` reply for the loaded id would set `report` (R14).
#[test]
fn the_uv_checker_toggles_without_a_reload() {
    let cache = tempfile::tempdir().unwrap();
    let mut m = MaterialState::builtin_default();
    m.alpha = Some(nif::props::Alpha {
        flags: 0x200,
        threshold: 0,
    }); // DO_ALPHA_TEST: the stand-in's pixel permutation compiles
    let mut v = real_viewport(cache.path(), m);
    run_idle(&mut v);
    v.report = None;
    v.set_debug(v3::DebugView::UvChecker);
    run_idle(&mut v);
    v.set_debug(v3::DebugView::Lit);
    run_idle(&mut v);
    assert!(v.report.is_none(), "no Loaded reply: nothing was loaded again");
}
```

In `the_uv_checker_is_a_checker_and_replaces_the_diffuse_slot` (2604-2623), rename it `the_uv_checker_is_a_checker` and delete its last block (the comment `// The paths swapped: …` through `assert_eq!(v3::diffuse_paths(…), …);`, lines 2615-2622).

Run: `cd /f/ai/dds/converter-nif-guisug && CARGO_TARGET_DIR=F:/ai/dds/converter-nif-p1/target cargo test -p sdc-gui --release --test viewport3d the_uv_checker_toggles_without_a_reload`
Expected: FAIL (`no Loaded reply` assertion: today Lit reloads).

- [ ] **Step 6: Implement the GUI**

`crates/gui/src/model/viewport3d.rs`:
- `V3Cmd::SetDebug` doc (584-585): `/// The debug view (sticky, answered with `Reconfigured`). UV checker = render3d's slot-0 override (kept across loads, no reload either way).`
- delete `diffuse_paths` and its doc (525-536);
- in `load_all`, delete `if tex.checker { checker_over(r); }` (1974-1976);
- delete `checker_over` and its doc (1980-1988);
- the `SetDebug` arm (2160-2170) becomes:

```rust
                V3Cmd::SetDebug(d) => {
                    let on = d == DebugView::UvChecker;
                    if tex.checker != on {
                        tex.checker = on;
                        // render3d keeps the override across loads, swaps and edits: no reload either way.
                        let _ = r.override_slot(0, on.then(checker_dds).as_deref());
                        debug_changed = true;
                    }
                    ack.0 += 1;
                    None
                }
```

- delete the refresh at 2280-2283 (`// The checker goes (back) over …` and its `if tex.checker && last.is_some() && … { checker_over(&mut r); }`).
- `TexState.checker`'s doc (1924): `/// The UV checker debug view is on (render3d's slot-0 override is set).`

- [ ] **Step 7: Run the GUI tests, gate, commit**

```bash
cd /f/ai/dds/converter-nif-guisug
CARGO_TARGET_DIR=F:/ai/dds/converter-nif-p1/target cargo test -p sdc-gui --release --test viewport3d
CARGO_TARGET_DIR=F:/ai/dds/converter-nif-p1/target cargo test -p sdc-gui --release --test set_preview_model
CARGO_TARGET_DIR=F:/ai/dds/converter-nif-p1/target cargo fmt --all --check
CARGO_TARGET_DIR=F:/ai/dds/converter-nif-p1/target cargo clippy --workspace --all-targets --release -- -D warnings
git add crates/gui/src/model/viewport3d.rs crates/gui/tests/viewport3d.rs
git commit -m "gui: the UV checker is render3d's slot-0 override (no per-path swap, no reload to undo)

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```
Expected: PASS. `a_debug_view_change_goes_to_the_render_thread_and_waits_for_its_answer` (2585-2599) and `the_3d_keys_reset_cycle_and_are_listed` (2672-2703) are unchanged and green.

---

### Task 6: PGPatcher 3D shape index (D5, schema 7 → 8)

**Files:**
- Modify: `crates/nif/src/graph.rs` (new `takes_index`, `index3d`; closure test in `mod tests`)
- Modify: `crates/nif/src/scan.rs:27-33, 72-77` (`NifScan::index3d`)
- Modify: `crates/nif/tests/graph.rs` (append), `crates/nif/tests/fixtures.rs` (append)
- Modify: `crates/modscan/src/store.rs`: 11, 23-28, 98-99, 106-110, 211-239 (`ShapeView`), 597, 640-656, 1116-1121, 1394-1430
- Modify: `crates/modscan/tests/store.rs`: 75-82, 226, new tests
- Create: `crates/modscan/tests/index3d.rs`
- Modify: `crates/gui/src/ui/meshes.rs`: 534-536, 728-747, 765, 790
- Modify: `crates/gui/tests/meshes_view.rs`: `shape()` (185-207), 1574-1593; `crates/gui/tests/open_in_textures.rs:13-31`; `crates/gui/tests/complex_grass.rs:69-74`; `crates/gui/tests/set_preview_model.rs:777-782`

**Interfaces:**
- Produces: `pub fn nif::graph::index3d(header: &Header, bytes: &[u8]) -> BTreeMap<u32, u32>`; `pub(crate) fn nif::graph::takes_index(kind: &[u8]) -> bool`; `NifScan.index3d: BTreeMap<u32, u32>`; `modscan::store::SCHEMA_VERSION == 8`; `ShapeView.index3d: Option<u32>`.

- [ ] **Step 1: Write the failing nif tests**

Append to `crates/nif/tests/graph.rs` (it already has `node(t, kids)` and `use builder::{Nif, W};`):

```rust
// ---- PGPatcher 3D index (GUI suggestions item 6) ----

fn idx(f: &Nif) -> Vec<(u32, u32)> {
    let bytes = f.bytes();
    nif::graph::index3d(&Header::parse(&bytes).unwrap(), &bytes)
        .into_iter()
        .collect()
}

#[test]
fn shapes_are_numbered_in_tree_order_not_block_order() {
    let mut f = Nif::new(100);
    f.block("BSFadeNode", node([0.0; 3], &[3, 1])); // 0
    f.block("NiNode", node([0.0; 3], &[2])); // 1
    f.block("BSTriShape", vec![]); // 2
    f.block("BSTriShape", vec![]); // 3
    assert_eq!(idx(&f), [(2, 1), (3, 0)]);
}

#[test]
fn a_particle_system_takes_an_index() {
    let mut f = Nif::new(100);
    f.block("BSFadeNode", node([0.0; 3], &[1, 2, 3]));
    f.block("BSTriShape", vec![]);
    f.block("NiParticleSystem", vec![]);
    f.block("BSTriShape", vec![]);
    assert_eq!(idx(&f), [(1, 0), (2, 1), (3, 2)]);
}

#[test]
fn an_unreachable_shape_has_no_index() {
    let mut f = Nif::new(100);
    f.block("BSFadeNode", node([0.0; 3], &[1]));
    f.block("BSTriShape", vec![]);
    f.block("BSTriShape", vec![]); // nobody's child
    assert_eq!(idx(&f), [(1, 0)]);
}

#[test]
fn a_shared_shape_keeps_its_first_visit() {
    let mut f = Nif::new(100);
    f.block("BSFadeNode", node([0.0; 3], &[1, 3])); // 0
    f.block("NiNode", node([0.0; 3], &[2])); // 1
    f.block("BSTriShape", vec![]); // 2: under 1 and 3
    f.block("NiNode", node([0.0; 3], &[2, 4])); // 3
    f.block("BSTriShape", vec![]); // 4
    assert_eq!(idx(&f), [(2, 0), (4, 1)]);
}

#[test]
fn a_cycle_terminates_and_a_shape_below_max_depth_is_absent() {
    let mut f = Nif::new(100);
    f.block("BSFadeNode", node([0.0; 3], &[1])); // 0
    f.block("NiNode", node([0.0; 3], &[0, 2])); // 1: points back at the root
    f.block("BSTriShape", vec![]); // 2
    assert_eq!(idx(&f), [(2, 0)], "the cycle is walked once");
    let mut g = Nif::new(100);
    g.block("BSFadeNode", node([0.0; 3], &[1, 302]));
    for i in 1..=300 {
        g.block("NiNode", node([0.0; 3], &[i + 1])); // a chain 1 → 2 → … → 301
    }
    g.block("BSTriShape", vec![]); // 301, depth 301 > MAX_DEPTH
    g.block("BSTriShape", vec![]); // 302, a child of the root
    assert_eq!(idx(&g), [(302, 0)]);
}

#[test]
fn the_root_is_the_first_node_when_block_0_is_not_one() {
    let mut f = Nif::new(100);
    f.block("BSTriShape", vec![]); // 0: not a node, not under the root
    f.block("BSFadeNode", node([0.0; 3], &[2])); // 1
    f.block("BSTriShape", vec![]); // 2
    assert_eq!(idx(&f), [(2, 0)]);
    let mut g = Nif::new(100);
    g.block("BSTriShape", vec![]);
    assert!(idx(&g).is_empty(), "no node at all: no index");
}
```

Append to `mod tests` in `crates/nif/src/graph.rs` (it already reads `nifxml_inherit.txt`):

```rust
    /// The blocks that take a 3D index are the nif.xml closures of NiTriBasedGeom, BSTriShape and NiParticleSystem
    /// (nifly's NiShape plus PGPatcher's particle increment), minus the abstract NiTriBasedGeom and the non-Skyrim
    /// NiClod, NiLines and NiScreenElements (nifly does not read them). `ShapeKind` covers all but
    /// NiMeshParticleSystem, which `takes_index` adds.
    #[test]
    fn index_takers_match_the_nifxml_shape_and_particle_closures() {
        let mut inherit: HashMap<&str, &str> = HashMap::new();
        for l in include_str!("../tests/fixtures/nifxml_inherit.txt").lines() {
            let (n, p) = l.split_once(' ').unwrap();
            inherit.insert(n, p);
        }
        let under = |mut n: &str| loop {
            if matches!(n, "NiTriBasedGeom" | "BSTriShape" | "NiParticleSystem") {
                return true;
            }
            match inherit.get(n) {
                Some(p) => n = p,
                None => return false,
            }
        };
        let not_skyrim = ["NiTriBasedGeom", "NiClod", "NiLines", "NiScreenElements"];
        let mut want: Vec<&str> = inherit
            .keys()
            .copied()
            .filter(|n| under(n) && !not_skyrim.contains(n))
            .collect();
        want.sort_unstable();
        let mut got: Vec<&str> = inherit.keys().copied().filter(|n| takes_index(n.as_bytes())).collect();
        got.sort_unstable();
        assert_eq!(got, want);
        assert_eq!(want.len(), 11, "the 10 ShapeKind names and NiMeshParticleSystem");
    }
```

Append to `crates/nif/tests/fixtures.rs`:

```rust
#[test]
fn every_furniture_shape_has_a_3d_index_numbered_from_zero() {
    let s = scan(&read("TestNifFile_Furniture_Col_SE.nif")).unwrap();
    assert!(s.shapes.iter().all(|r| s.index3d.contains_key(&r.block)), "{:?} vs {:?}", s.index3d, s.shapes.iter().map(|r| r.block).collect::<Vec<_>>());
    let mut v: Vec<u32> = s.index3d.values().copied().collect();
    v.sort_unstable();
    assert_eq!(v, (0..v.len() as u32).collect::<Vec<_>>());
}
```

(If this fixture test fails on "all shapes indexed", print the scan: a nifly test fixture can have a shape no node lists. Then report it rather than changing `index3d`.)

- [ ] **Step 2: Run them to see them fail**

Run: `cd /f/ai/dds/converter-nif-guisug && CARGO_TARGET_DIR=F:/ai/dds/converter-nif-p1/target cargo test -p nif --release --test graph --test fixtures --lib graph`
Expected: compile errors (`index3d`, `takes_index`, `NifScan::index3d` missing).

- [ ] **Step 3: Implement in nif**

`crates/nif/src/graph.rs`: change `use std::collections::HashMap;` to `use std::collections::{BTreeMap, HashMap, HashSet};`, add `use crate::shape_head::ShapeKind;`, and append after `world` (line 192):

```rust
/// A block that takes PGPatcher's next 3D index: every [`ShapeKind`] (nifly's NiShape family plus the two particle
/// systems sdc scans) and `NiMeshParticleSystem` (a nif.xml NiParticleSystem sdc does not scan; PGPatcher counts it).
pub(crate) fn takes_index(kind: &[u8]) -> bool {
    ShapeKind::from_name(kind).is_some() || kind == b"NiMeshParticleSystem"
}

/// PGPatcher's 3D index (PGNIFUtil::shapesWith3DIdx over nifly's GetTree): a pre-order walk from the root over node
/// `children`, first visit wins, iterative, nodes deeper than [`MAX_DEPTH`] not expanded. Every block
/// [`takes_index`] takes the next index (a particle system too: PGPatcher increments for it). Unreachable blocks are
/// absent. The root is block 0 when it is a [`NODE_TYPES`] block, else the first block that is.
/// Assumption: property, collision, controller and effect refs never reach a shape in a Skyrim NIF, so walking
/// `children` alone matches nifly's `GetChildIndices` order.
pub fn index3d(header: &Header, bytes: &[u8]) -> BTreeMap<u32, u32> {
    let total = header.sizes.len();
    let is_node = |i: usize| NODE_TYPES.iter().any(|t| t.as_bytes() == header.kind(i));
    let mut out = BTreeMap::new();
    let Some(root) = (0..total).find(|&i| is_node(i)) else {
        return out;
    };
    let mut seen: HashSet<usize> = HashSet::new();
    let mut stack = vec![(root, 0usize)];
    let mut next = 0u32;
    while let Some((b, depth)) = stack.pop() {
        if !seen.insert(b) {
            continue;
        }
        if takes_index(header.kind(b)) {
            out.insert(b as u32, next);
            next += 1;
            continue;
        }
        if !is_node(b) || depth >= MAX_DEPTH {
            continue;
        }
        let Ok((_, kids)) = children(header, bytes, b) else {
            continue;
        };
        // Reversed so the first child pops first (pre-order).
        for k in kids.into_iter().rev() {
            if let Some(c) = usize::try_from(k).ok().filter(|&c| c < total) {
                stack.push((c, depth + 1));
            }
        }
    }
    out
}
```

`crates/nif/src/scan.rs`: add `use std::collections::BTreeMap;`; `NifScan` gains after `problems`:

```rust
    /// Block → PGPatcher 3D index ([`crate::graph::index3d`]); a shape no node reaches is absent.
    pub index3d: BTreeMap<u32, u32>,
```

and in `scan()` the literal (72-77) gets `index3d: crate::graph::index3d(&h, b),`.

- [ ] **Step 4: Run the nif tests, fix the three literals, commit**

The three other `NifScan` literals each get `index3d: Default::default(),` (after `problems: vec![],`): `crates/modscan/tests/store.rs:76-81` (`scan_of`), `crates/gui/tests/complex_grass.rs:69-74`, `crates/gui/tests/set_preview_model.rs:777-782`.

```bash
cd /f/ai/dds/converter-nif-guisug
CARGO_TARGET_DIR=F:/ai/dds/converter-nif-p1/target cargo test -p nif --release
CARGO_TARGET_DIR=F:/ai/dds/converter-nif-p1/target cargo build --workspace --release --all-targets
git add crates/nif/src/graph.rs crates/nif/src/scan.rs crates/nif/tests/graph.rs crates/nif/tests/fixtures.rs crates/modscan/tests/store.rs crates/gui/tests/complex_grass.rs crates/gui/tests/set_preview_model.rs
git commit -m "nif: PGPatcher 3D index (graph::index3d) on NifScan

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```
Expected: nif PASS; the workspace builds.

- [ ] **Step 5: Write the failing modscan tests**

In `crates/modscan/tests/store.rs`: line 226 `assert_eq!(v, 7);` → `assert_eq!(v, 8);`. Add `use std::collections::BTreeMap;` and `use modscan::store::ChunkItem;` to the imports, and append:

```rust
#[test]
fn shapes_of_round_trips_the_3d_index() {
    let (_d, mut s, o) = store();
    let mut scan = scan_of(vec![
        ShapeRow { block: 3, head: head(false), shader: Shader::None, alpha: None },
        ShapeRow { block: 7, head: head(false), shader: Shader::None, alpha: None },
    ]);
    scan.index3d = BTreeMap::from([(7, 0)]);
    let id = s.upsert_copy("meshes/x.nif", o, 1, 1, CopyResult::Scanned(scan), None).unwrap();
    let v: Vec<(u32, Option<u32>)> = s.shapes_of(id).unwrap().iter().map(|x| (x.block, x.index3d)).collect();
    assert_eq!(v, [(3, None), (7, Some(0))], "NULL when no node reaches the shape");
}

#[test]
fn a_derived_copy_keeps_its_3d_index_through_commit_pending() {
    let (_d, mut s, o) = store();
    let mut scan = scan_of(vec![ShapeRow { block: 2, head: head(false), shader: Shader::None, alpha: None }]);
    scan.index3d = BTreeMap::from([(2, 5)]);
    let pid = s.path_ids(&["meshes/d.nif"]).unwrap()[0];
    s.write_chunk(&[ChunkItem {
        path_id: pid,
        origin_id: o,
        size: 1,
        mtime: 1,
        crc32: None,
        derived: true,
        result: CopyResult::Scanned(scan),
    }])
    .unwrap();
    s.commit_pending().unwrap();
    let id = s.existing_copies().unwrap()[&(pid, o)].id;
    assert_eq!(s.shapes_of(id).unwrap()[0].index3d, Some(5));
}

#[test]
fn a_schema_7_database_is_rebuilt_with_the_index3d_column() {
    let d = tempfile::tempdir().unwrap();
    let p = d.path().join("v7.db");
    {
        let c = rusqlite::Connection::open(&p).unwrap();
        c.execute_batch(
            "CREATE TABLE shape (copy_id INTEGER, block INTEGER, PRIMARY KEY(copy_id, block));
             INSERT INTO shape VALUES (1, 1); PRAGMA user_version = 7;",
        )
        .unwrap();
    }
    let s = Store::open(&p).unwrap();
    let n: i64 = s.conn().query_row("SELECT count(index3d) FROM shape", [], |r| r.get(0)).unwrap();
    assert_eq!(n, 0, "rebuilt: the column exists and the old row is gone");
}
```

(`write_chunk`, `commit_pending`, `path_ids`, `existing_copies` are `pub` on `Store`; `ChunkItem` is `pub` in `modscan::store`. If `upsert_copy` panics on `Shader::None` rows, use `Shader::Lighting(lighting(), vec![])` as the other tests do.)

Create `crates/modscan/tests/index3d.rs`:

```rust
//! PGPatcher 2.2's logged 3D indices (owner logs `C:\_mount\moddingvol\Acherus\tools\ParallaxGen\log\`, 2026-10-14,
//! `[block/name/index]` line prefixes) reproduced from the exact copies PGPatcher read (plan ruling R3). Read-only.
//! `SDC_ACHERUS='C:\_mount\moddingvol\Acherus' cargo test -p modscan --release --test index3d -- --ignored`

fn check(bytes: &[u8], logged: &[(u32, u32)], what: &str) {
    let scan = nif::scan::scan(bytes).unwrap_or_else(|e| panic!("{what}: {e}"));
    for (block, index) in logged {
        assert_eq!(scan.index3d.get(block), Some(index), "{what}: block {block} ({:?})", scan.index3d);
    }
}

#[test]
#[ignore]
fn owner_meshes_match_the_pgpatcher_log() {
    let mods = std::path::PathBuf::from(std::env::var("SDC_ACHERUS").expect("set SDC_ACHERUS to the MO2 instance root")).join("mods");
    let loose: [(&str, &[(u32, u32)]); 3] = [
        (
            r"Skyking Alchemy Ingredients\meshes\plants\birdsnest02.nif",
            &[(10, 2), (15, 0), (20, 1)],
        ),
        (
            r"Rudy HQ - Miscellaneous SE\meshes\loadscreenart\loadscreenadventure01.nif",
            &[
                (3, 10), (6, 11), (9, 12), (12, 13), (15, 0), (19, 1), (23, 14), (26, 15), (30, 16),
                (36, 17), (39, 2), (43, 3), (46, 4), (49, 5), (52, 6), (55, 7), (58, 8), (61, 9),
            ],
        ),
        (
            r"Improved Solitude Windmill\meshes\rbimprovedsolitudewindmillmeshes\rbfarmhousewindmill.nif",
            &[
                (133, 2), (137, 3), (141, 4), (145, 5), (149, 6), (153, 1), (160, 7), (163, 0), (167, 8),
                (171, 9), (174, 10), (177, 11), (181, 12), (189, 13), (201, 14),
            ],
        ),
    ];
    for (file, logged) in loose {
        check(&std::fs::read(mods.join(file)).unwrap(), logged, file);
    }
    let bsa = vfs::bsa::Bsa::open(&mods.join("Vibrant Weapons EAE").join("Vibrant weapons EAE.bsa")).unwrap();
    let want = nif::path::canon_mesh(r"meshes\vibrant weapons\magic\fireshockwarhammer.nif");
    let e = bsa.entries.iter().find(|e| e.norm == want).expect("fireshockwarhammer.nif is in the BSA");
    check(&bsa.read(e).unwrap(), &[(34, 1), (79, 0)], "fireshockwarhammer.nif");
}
```

Run: `cd /f/ai/dds/converter-nif-guisug && CARGO_TARGET_DIR=F:/ai/dds/converter-nif-p1/target cargo test -p modscan --release --test store`
Expected: FAIL (schema still 7; no `index3d` field).

- [ ] **Step 6: Implement in modscan**

`crates/modscan/src/store.rs`:
- line 11: `pub const SCHEMA_VERSION: i64 = 8;` and extend its doc: `/// 8: shape.index3d (PGPatcher 3D index, GUI suggestions item 6).`
- `CREATE TABLE shape` (23-28): `num_triangles INTEGER,` → `num_triangles INTEGER, index3d INTEGER,`
- `pending_shape` (106-110): `num_triangles INTEGER, PRIMARY KEY(pid, block));` → `num_triangles INTEGER, index3d INTEGER, PRIMARY KEY(pid, block));`
- `SHAPE_COLS` (98-99): append `, index3d` (`… vertex_desc, num_vertices, num_triangles, index3d`).
- `write_shapes` (597): the VALUES list ends `?15,?16,?17)`; in the `params![…]` (640-656) add after `h.num_triangles.map(i64::from),`:

```rust
            scan.index3d.get(&s.block).map(|&i| i64::from(i)),
```

- `commit_pending` (1116-1121): the column list ends `vertex_desc, num_vertices, num_triangles, index3d)` and the SELECT ends `s.vertex_desc, s.num_vertices, s.num_triangles, s.index3d`.
- `ShapeView` (211-239) gains at the end:

```rust
    /// PGPatcher's 3D index of the shape (`nif::graph::index3d`): the number plugins use for its alternate textures.
    /// `None` when no node reaches it.
    pub index3d: Option<u32>,
```

- `shapes_of` (1394-1430): the SELECT's third line becomes `s.skinned, s.vertex_desc, s.num_vertices, s.num_triangles, s.index3d,`; `const T: usize = 15;`; the `ShapeView` literal gains `index3d: r.get::<_, Option<i64>>(14)?.map(|x| x as u32),`.

Fix the GUI `ShapeView` literals (add `index3d: Some(0),` to each): `crates/gui/tests/meshes_view.rs:187-206` (`shape()`), `crates/gui/tests/open_in_textures.rs:14-31`.

- [ ] **Step 7: Run modscan, commit**

```bash
cd /f/ai/dds/converter-nif-guisug
CARGO_TARGET_DIR=F:/ai/dds/converter-nif-p1/target cargo test -p modscan --release
CARGO_TARGET_DIR=F:/ai/dds/converter-nif-p1/target cargo build --workspace --release --all-targets
git add crates/modscan/src/store.rs crates/modscan/tests/store.rs crates/modscan/tests/index3d.rs crates/gui/tests/meshes_view.rs crates/gui/tests/open_in_textures.rs
git commit -m "modscan: shape.index3d (PGPatcher 3D index), schema 8

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```
Expected: PASS (the ignored owner test is not run here).

- [ ] **Step 8: Write the failing Meshes test**

Replace `shape_cards_number_from_zero_and_show_the_block_in_the_hover` (`crates/gui/tests/meshes_view.rs:1574-1593`) with:

```rust
#[test]
fn shape_cards_show_the_pgpatcher_3d_index_and_the_block_in_the_hover() {
    let a = ShapeView { block: 12, name: Some("First".into()), index3d: Some(1), ..shape(vec![None]) };
    let b = ShapeView { block: 20, name: Some("Second".into()), index3d: Some(0), ..shape(vec![None]) };
    let c = ShapeView { block: 30, name: Some("Orphan".into()), index3d: None, ..shape(vec![None]) };
    let mut h = harness(shapes_model(vec![a, b, c]));
    h.run();
    h.get_by_label_contains("#1 First");
    h.get_by_label_contains("#0 Second");
    h.get_by_label_contains("#– (not in scene tree) Orphan");
    assert!(h.query_by_label_contains("#12").is_none(), "the block is not the number");
    h.get_by_label_contains("#1 First").hover();
    h.run();
    h.get_by_label_contains("NIF block 12");
    h.get_by_label_contains("PGPatcher 3D index");
}
```

Run: `cd /f/ai/dds/converter-nif-guisug && CARGO_TARGET_DIR=F:/ai/dds/converter-nif-p1/target cargo test -p sdc-gui --release --test meshes_view shape_cards_show`
Expected: FAIL (`#1 First` not found: rows are still numbered 0, 1, 2).

- [ ] **Step 9: Implement the Meshes header**

`crates/gui/src/ui/meshes.rs`:
- the loop (534-536): `for s in &m.shapes { if let Some(p) = shape(ui, m, s, &mut open) { clicked = Some(p); } ui.separator(); }`
- `shape()` (728-747): drop the `index: usize,` parameter and its doc words; the header becomes

```rust
/// One shape card, numbered by PGPatcher's 3D index. Returns the texture path that was clicked.
fn shape(
    ui: &mut egui::Ui,
    m: &MeshesModel,
    s: &ShapeView,
    open: &mut Option<String>,
) -> Option<String> {
    let mut clicked = None;
    let number = s
        .index3d
        .map_or_else(|| "#– (not in scene tree)".to_string(), |i| format!("#{i}"));
    ui.label(
        egui::RichText::new(format!(
            "{number} {} — {}",
            s.name.as_deref().unwrap_or("(unnamed)"),
            s.kind
        ))
        .strong(),
    )
    .on_hover_text(format!(
        "NIF block {}\nPGPatcher 3D index: the number plugins use for this shape's alternate textures",
        s.block
    ));
```

- line 765: `.id_salt(("slsf", s.block, label))`; line 790: `ui.indent(("slots", s.block), |ui| {`.

- [ ] **Step 10: Run the GUI tests, gate, commit**

```bash
cd /f/ai/dds/converter-nif-guisug
CARGO_TARGET_DIR=F:/ai/dds/converter-nif-p1/target cargo test -p sdc-gui --release --test meshes_view --test open_in_textures --test complex_grass --test set_preview_model
CARGO_TARGET_DIR=F:/ai/dds/converter-nif-p1/target cargo fmt --all --check
CARGO_TARGET_DIR=F:/ai/dds/converter-nif-p1/target cargo clippy --workspace --all-targets --release -- -D warnings
git add crates/gui/src/ui/meshes.rs crates/gui/tests/meshes_view.rs
git commit -m "gui: Meshes shape cards are numbered by PGPatcher's 3D index

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```
Expected: PASS (`#0 Body` at meshes_view.rs:1556 and 1602 still matches through `shape()`'s `index3d: Some(0)`).

- [ ] **Step 11 (owner data, optional now, required before the final review): the ignored log test**

```bash
cd /f/ai/dds/converter-nif-guisug
SDC_ACHERUS='C:\_mount\moddingvol\Acherus' CARGO_TARGET_DIR=F:/ai/dds/converter-nif-p1/target cargo test -p modscan --release --test index3d -- --ignored
```
Expected: PASS. A mismatch means the walk order differs from nifly's (for example, a shape reached through a ref other than `children`). Report the block and the map; do not "fix" by special-casing a file.

---

### Task 7: Debug views (Albedo, NormalWorld, Roughness, Metallic, AO)

**Files:**
- Create: `crates/render3d/src/debug.rs`
- Modify: `crates/render3d/src/lib.rs`: `pub mod debug;` (line 9 area), `pub use debug::{Availability, DebugOutput};` (after line 25), `Renderer` struct + `with_gpu`, `wanted()` (853-877), `rebuild_device_on`, new `set_debug_output` / `debug_output` / `debug_supported` (after `set_encode_srgb`)
- Modify: `crates/render3d/src/source.rs` (`SourceSet` gains `debug_memo` and `get_debug`)
- Create: `crates/render3d/tests/debug.rs`
- Modify: `crates/gui/src/model/viewport3d.rs`: `DebugView` (454-480), new `DEBUG_UNSUPPORTED`, `V3Reply::DebugSupport`, `Viewport3d.debug_support` + `new()` (822), `offered`, `stand_in`, `handle` (Shapes 1622-1640, new arm), `render_loop` (after 2050, the `SetShaders` and `SetDebug` arms, after `pump`)
- Modify: `crates/gui/src/ui/viewport3d.rs`: the V key (53-55) and the radio list (155-166)
- Modify: `crates/gui/tests/viewport3d.rs`: 2571-2583 and new tests

**Interfaces:**
- Consumes: `override_slot` (Task 5), `colour_register` is not needed here.
- Produces (render3d): `pub enum DebugOutput { Lit, Albedo, NormalWorld, Roughness, Metallic, Ao }` with `ALL: [DebugOutput; 6]`, `fn label(self) -> &'static str`, `fn define(self) -> Option<u32>`, `fn availability(self, m: &MaterialState) -> Availability`; `pub enum Availability { Exact, StandIn(&'static str), None }`; `pub fn debug::inject(text: &str) -> Result<String, String>`; `pub fn SourceSet::get_debug(&self, rel: &Path) -> Result<Arc<ExpandedSource>, String>`; `pub fn Renderer::set_debug_output(&mut self, d: DebugOutput) -> Result<(), String>`; `pub fn Renderer::debug_output(&self) -> DebugOutput`; `pub fn Renderer::debug_supported(&self) -> bool`.
- Produces (GUI): `DebugView::{Albedo, Normals, Roughness, Metallic, Ao}`; `fn DebugView::output(self) -> DebugOutput`; `fn DebugView::next(self, offered: &[DebugView]) -> DebugView` (signature changes); `V3Reply::DebugSupport(bool)`; `pub fn Viewport3d::offered(&self) -> Vec<DebugView>`; `pub fn Viewport3d::stand_in(&self, d: DebugView) -> Option<&'static str>`; `pub fn Viewport3d::debug_support(&self) -> bool`; `pub const DEBUG_UNSUPPORTED: &str`.

- [ ] **Step 1: Write the failing render3d unit tests** (in the new `crates/render3d/src/debug.rs`, the tests first; the module body comes in Step 3)

```rust
#[cfg(test)]
mod tests {
    use super::*;

    const MINI: &str = "MaterialProperties material = (MaterialProperties)0;\nfloat3 worldNormal = n;\nfloat glossiness = 0;\nfloat vertexAO = 1;\n\treturn psout;\n}\n";

    #[test]
    fn the_block_goes_right_before_the_one_return() {
        let out = inject(MINI).unwrap();
        let at = out.find("\treturn psout;").unwrap();
        assert!(out[..at].ends_with(BLOCK), "{out}");
        assert_eq!(out.matches("return psout;").count(), 1);
    }

    #[test]
    fn zero_or_two_returns_or_a_missing_name_is_an_error() {
        assert!(inject(&MINI.replace("return psout;", "")).is_err());
        assert!(inject(&format!("{MINI}\treturn psout;\n")).is_err());
        for name in ["MaterialProperties material =", "float3 worldNormal", "float glossiness", "float vertexAO"] {
            let e = inject(&MINI.replace(name, "x")).unwrap_err();
            assert!(e.contains(name), "{e}");
        }
    }

    #[test]
    fn the_standin_shader_has_no_anchor() {
        assert!(inject(include_str!("../tests/fixtures/standin/Lighting.hlsl")).is_err());
    }

    #[test]
    fn availability_table() {
        use Availability::*;
        let mut v = MaterialState::builtin_default();
        let mut p = v.clone();
        p.slsf2 |= nif::material::SLSF2_TRUE_PBR;
        for d in DebugOutput::ALL {
            assert_eq!(d.availability(&p), Exact, "{d:?} TruePBR");
        }
        assert_eq!(DebugOutput::Albedo.availability(&v), Exact);
        assert_eq!(DebugOutput::NormalWorld.availability(&v), Exact);
        assert_eq!(DebugOutput::Roughness.availability(&v), StandIn("1 − specular mask"));
        assert_eq!(DebugOutput::Metallic.availability(&v), None);
        assert_eq!(DebugOutput::Ao.availability(&v), StandIn("vertex AO"));
        for t in [4, 5, 6, 16] {
            v.shader_type = t;
            assert_eq!(DebugOutput::Ao.availability(&v), None, "type {t}: vertexAO is constant 1");
        }
        assert_eq!(DebugOutput::Lit.define(), Option::None);
        assert_eq!(DebugOutput::Ao.define(), Some(5));
    }
}
```

Create `crates/render3d/tests/debug.rs`:

```rust
//! Debug views (GUI suggestions item 1): the stand-in has no anchor, so only Lit; the installed CS reads RMAOS back
//! as its values (opt-in).
mod common;
use common::{cache_files, dds_rgba8, renderer};
use nif::material::{MaterialState, SLSF2_TRUE_PBR};
use render3d::device::Gpu;
use render3d::scene::Scene;
use render3d::{Builtin, DebugOutput, Renderer, ShapeId};

#[test]
fn the_standin_refuses_debug_views_and_lit_keys_are_unchanged() {
    let cache = tempfile::tempdir().unwrap();
    let mut r = renderer(cache.path(), &["STANDIN_OPAQUE_OK"]);
    let rep = r.load_builtin(Builtin::Sphere, MaterialState::builtin_default(), &|_| None);
    assert!(rep.compile_errors.is_empty() && rep.errors.is_empty(), "{rep:?}");
    let files = cache_files(cache.path());
    assert!(!r.debug_supported());
    assert!(r.set_debug_output(DebugOutput::Albedo).is_err());
    assert_eq!(r.debug_output(), DebugOutput::Lit, "nothing changed");
    assert_eq!(r.set_debug_output(DebugOutput::Lit), Ok(()));
    assert_eq!(cache_files(cache.path()), files, "the Lit programs are memory hits: same keys, nothing compiled");
    let (ps, _, _) = r.draw_diag(ShapeId(0)).unwrap();
    assert!(!ps.iter().any(|(k, _)| k.starts_with("SDC_DEBUG")), "{ps:?}");
}

/// Opt-in: a TruePBR sphere with roughness scale 1 (PBRParams1.x, L:1439) and a 1×1 RMAOS (64, 128, 192) reads back
/// as those values in Roughness / Metallic / AO (vertexAO 1: no vertex colours, L:269), encode on and off.
/// `SDC_CS_SHADERS=…\Shaders cargo test -p render3d --release --test debug -- --ignored`
#[test]
#[ignore]
fn installed_cs_reads_rmaos_back_as_its_values() {
    let root = std::path::PathBuf::from(std::env::var("SDC_CS_SHADERS").expect("set SDC_CS_SHADERS to the CS Shaders folder"));
    let fp = render3d::locate::fingerprint(&root, None).unwrap();
    let cache = tempfile::tempdir().unwrap();
    let mut r = Renderer::with_gpu(Gpu::new_warp().unwrap(), fp, cache.path().to_path_buf()).unwrap();
    assert!(r.debug_supported(), "the installed CS has the anchor");
    let mut m = MaterialState::builtin_default();
    m.slsf2 |= SLSF2_TRUE_PBR;
    m.params.specular_strength = 1.0; // the roughness scale
    m.set_slot_raw(5, r"textures\sdc\rmaos.dds");
    let rmaos = dds_rgba8(1, 1, &[[64, 128, 192, 255]]);
    let rep = r.load_builtin(Builtin::Sphere, m, &|p| (p == "textures/sdc/rmaos.dds").then(|| rmaos.clone()));
    assert!(rep.errors.is_empty() && rep.compile_errors.is_empty(), "{rep:?}");
    for encode in [true, false] {
        r.set_encode_srgb(encode);
        for (d, want) in [(DebugOutput::Roughness, 64), (DebugOutput::Metallic, 128), (DebugOutput::Ao, 192)] {
            // set_debug_output always re-permutes (R7), so the PRELINEAR define follows the encode just set.
            r.set_debug_output(d).unwrap();
            let f = r.render(&Scene::neutral(), 64, 64).unwrap();
            let c = &f[(32 * 64 + 32) * 4..][..3];
            for k in 0..3 {
                assert!((c[k] as i32 - want).abs() <= 2, "{d:?} encode {encode}: {c:?}");
            }
        }
    }
    r.rebuild_device_on(Gpu::new_warp().unwrap()).unwrap();
    assert_eq!(r.debug_output(), DebugOutput::Ao, "kept across a device rebuild");
}
```

- [ ] **Step 2: Run them to see them fail**

Run: `cd /f/ai/dds/converter-nif-guisug && CARGO_TARGET_DIR=F:/ai/dds/converter-nif-p1/target cargo test -p render3d --release --test debug`
Expected: compile error (no `DebugOutput`).

- [ ] **Step 3: Implement in render3d**

`crates/render3d/src/debug.rs` (above the tests from Step 1):

```rust
//! Debug views (GUI suggestions item 1): one output block injected into the user's own `Lighting.hlsl` expansion
//! right before its single `return psout;`, so CS does all of its own sampling and remaps and every term is right for
//! every permutation. The block writes `psout.Diffuse.xyz` only (alpha test and blend behave as in Lit). It is used
//! only when the view is not Lit, so Lit program keys and the user's warm disk cache are untouched.
use nif::material::MaterialState;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum DebugOutput {
    #[default]
    Lit,
    Albedo,
    NormalWorld,
    Roughness,
    Metallic,
    Ao,
}

/// What a view shows for one material.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Availability {
    Exact,
    /// Not the physical term: the named stand-in (shown in the GUI's hover text).
    StandIn(&'static str),
    /// Constant for this material: not worth a view.
    None,
}

impl DebugOutput {
    pub const ALL: [DebugOutput; 6] = [
        DebugOutput::Lit,
        DebugOutput::Albedo,
        DebugOutput::NormalWorld,
        DebugOutput::Roughness,
        DebugOutput::Metallic,
        DebugOutput::Ao,
    ];

    pub fn label(self) -> &'static str {
        match self {
            DebugOutput::Lit => "Lit",
            DebugOutput::Albedo => "Albedo",
            DebugOutput::NormalWorld => "Normals (world)",
            DebugOutput::Roughness => "Roughness",
            DebugOutput::Metallic => "Metallic",
            DebugOutput::Ao => "AO",
        }
    }

    /// The `SDC_DEBUG` value of the view; `None` for Lit (no define, no injected source).
    pub fn define(self) -> Option<u32> {
        match self {
            DebugOutput::Lit => None,
            DebugOutput::Albedo => Some(1),
            DebugOutput::NormalWorld => Some(2),
            DebugOutput::Roughness => Some(3),
            DebugOutput::Metallic => Some(4),
            DebugOutput::Ao => Some(5),
        }
    }

    /// TruePBR: Exact for all. Non-PBR: Albedo and NormalWorld Exact; Roughness StandIn("1 − specular mask"); Metallic
    /// None (CS's non-PBR material has no metallic term); Ao StandIn("vertex AO"), None for shader types 4, 5, 6 and
    /// 16 (FaceGen, FaceGen tint, Hair, Eye: vertexAO is the constant 1, L:2673, 2689).
    pub fn availability(self, m: &MaterialState) -> Availability {
        if self == DebugOutput::Lit || m.is_pbr() {
            return Availability::Exact;
        }
        match self {
            DebugOutput::Lit | DebugOutput::Albedo | DebugOutput::NormalWorld => Availability::Exact,
            DebugOutput::Roughness => Availability::StandIn("1 − specular mask"),
            DebugOutput::Metallic => Availability::None,
            DebugOutput::Ao if matches!(m.shader_type, 4 | 5 | 6 | 16) => Availability::None,
            DebugOutput::Ao => Availability::StandIn("vertex AO"),
        }
    }
}

/// The one statement the block goes before.
const ANCHOR: &str = "return psout;";
/// The `main`-scope declarations the block reads (installed CS: L:1828, 1642/1645, 1361, 2673/2689/2691). Checked as
/// declarations: a bare `MaterialProperties material` also matches include-file function parameters (plan R5).
const NAMES: [&str; 4] = [
    "MaterialProperties material =",
    "float3 worldNormal",
    "float glossiness",
    "float vertexAO",
];

/// The injected output. Data views are pre-decoded when the readback sRGB-encodes (`SDC_DEBUG_PRELINEAR`), so the
/// shown value is the data value; Albedo is a colour and is written as is. Non-PBR materials have no `Metallic`
/// field, so that view writes 0 there (plan R4). The EOTF is written out: no CS helper name is relied on.
pub const BLOCK: &str = "#if defined(SDC_DEBUG)
\t{
\t\tfloat3 sdc;
#\tif SDC_DEBUG == 1
\t\tsdc = material.BaseColor;
#\telse
#\t\tif SDC_DEBUG == 2
\t\tsdc = worldNormal * 0.5 + 0.5;
#\t\telif SDC_DEBUG == 3
#\t\t\tif defined(TRUE_PBR)
\t\tsdc = material.Roughness.xxx;
#\t\t\telse
\t\tsdc = (1 - glossiness).xxx;
#\t\t\tendif
#\t\telif SDC_DEBUG == 4
#\t\t\tif defined(TRUE_PBR)
\t\tsdc = material.Metallic.xxx;
#\t\t\telse
\t\tsdc = 0.0.xxx;
#\t\t\tendif
#\t\telse
#\t\t\tif defined(TRUE_PBR)
\t\tsdc = (material.AO * vertexAO).xxx;
#\t\t\telse
\t\tsdc = vertexAO.xxx;
#\t\t\tendif
#\t\tendif
#\t\tif defined(SDC_DEBUG_PRELINEAR)
\t\tsdc = sdc <= 0.04045 ? sdc / 12.92 : pow((saturate(sdc) + 0.055) / 1.055, 2.4);
#\t\tendif
#\tendif
\t\tpsout.Diffuse.xyz = sdc;
\t}
#endif
";

/// `text` with [`BLOCK`] inserted right before its single `return psout;`. Err when the text does not have exactly
/// one, or lacks one of the declarations the block reads.
pub fn inject(text: &str) -> Result<String, String> {
    let n = text.matches(ANCHOR).count();
    if n != 1 {
        return Err(format!(
            "Lighting.hlsl has {n} `{ANCHOR}`; the debug views need exactly one"
        ));
    }
    if let Some(name) = NAMES.iter().find(|name| !text.contains(*name)) {
        return Err(format!(
            "Lighting.hlsl has no `{name}`; the debug views read it"
        ));
    }
    let at = text.find(ANCHOR).expect("counted once above");
    // Before the anchor's own line, so its indentation stays with it.
    let line = text[..at].rfind('\n').map_or(0, |i| i + 1);
    Ok(format!("{}{BLOCK}{}", &text[..line], &text[line..]))
}
```

(The unit test `the_block_goes_right_before_the_one_return` looks for `"\treturn psout;"` and checks that the text before it ends with `BLOCK`, which matches the line-start insertion.)

`crates/render3d/src/source.rs`: `SourceSet` gains a second memo, and `new` initialises it:

```rust
pub struct SourceSet {
    root: PathBuf,
    memo: Mutex<HashMap<PathBuf, Arc<ExpandedSource>>>,
    /// [`SourceSet::get_debug`]: the debug-block expansion (or why there is none), per entry file.
    debug_memo: Mutex<HashMap<PathBuf, Result<Arc<ExpandedSource>, String>>>,
}
```

(`new`: `debug_memo: Mutex::new(HashMap::new()),`.) Add after `get`:

```rust
    /// The expansion of `rel` with [`crate::debug::BLOCK`] before its one `return psout;`, with its own SHA-256 (so
    /// debug programs key apart from Lit ones). Memoised separately, Err included (the anchor check is a property of
    /// the text); an I/O error from [`SourceSet::get`] is not memoised.
    pub fn get_debug(&self, rel: &Path) -> Result<Arc<ExpandedSource>, String> {
        let base = self.get(rel)?;
        let mut memo = self.debug_memo.lock().unwrap_or_else(PoisonError::into_inner);
        if let Some(r) = memo.get(rel) {
            return r.clone();
        }
        let r = crate::debug::inject(&base.text).map(|text| {
            let sha256: [u8; 32] = Sha256::digest(text.as_bytes()).into();
            Arc::new(ExpandedSource {
                text: text.into(),
                sha256,
                files: base.files.clone(),
            })
        });
        memo.insert(rel.to_path_buf(), r.clone());
        r
    }
```

`crates/render3d/src/lib.rs`:
- `pub mod debug;` in the module list (alphabetical, after `pub mod cs_settings;`), and `pub use debug::{Availability, DebugOutput};` after line 25.
- `Renderer` gains (after `overrides`), and `with_gpu` sets `debug: DebugOutput::Lit,`:

```rust
    /// [`Renderer::set_debug_output`]: the term the next permutations write instead of the lit colour.
    debug: DebugOutput,
```

- After `set_missing_texture`, add:

```rust
    /// The debug view for every shape (GUI suggestions item 1). Err, with nothing changed, when `d` is not Lit and this
    /// CS build has no usable anchor ([`Renderer::debug_supported`]). Otherwise stores `d` and re-permutes every shape
    /// (`begin_all`): the old draw stays on screen while programs compile. Never short-circuits on an unchanged value.
    pub fn set_debug_output(&mut self, d: DebugOutput) -> Result<(), String> {
        if d != DebugOutput::Lit {
            self.sources.get_debug(Path::new("Lighting.hlsl"))?;
        }
        self.debug = d;
        self.begin_all();
        Ok(())
    }

    pub fn debug_output(&self) -> DebugOutput {
        self.debug
    }

    /// The loaded CS `Lighting.hlsl` has the single `return psout;` and the names the debug block reads. After
    /// `set_shaders` to a build without them, `debug_output()` keeps the request but every shape compiles Lit.
    pub fn debug_supported(&self) -> bool {
        self.sources.get_debug(Path::new("Lighting.hlsl")).is_ok()
    }
```

- `wanted()` (853-877): replace from `let vs = defines(…)` through `let src = …?;` with:

```rust
        let vs = defines(&pm, Stage::Vertex, &f, false);
        let mut ps = defines(&pm, Stage::Pixel, &f, false);
        let lighting = Path::new("Lighting.hlsl");
        // A debug view: the injected source for both stages (one extra VS compile per permutation, shared by the
        // five views) and SDC_DEBUG on the PS only. Without an anchor, Lit (no failure per shape).
        let debug = match self.debug.define() {
            Some(n) => self.sources.get_debug(lighting).ok().map(|s| (n, s)),
            None => None,
        };
        let src = match debug {
            Some((n, s)) => {
                ps.0.push(("SDC_DEBUG".into(), n.to_string()));
                if self.encode_srgb {
                    ps.0.push(("SDC_DEBUG_PRELINEAR".into(), "1".into()));
                }
                s
            }
            None => self.sources.get(lighting)?,
        };
```

(The rest of `wanted()` is unchanged; `Wanted`, `CompileJob`, `pool.rs` and `program_key` need no change.)
- `rebuild_device_on`: add `n.debug = self.debug;`.

- [ ] **Step 4: Run the render3d tests, commit**

```bash
cd /f/ai/dds/converter-nif-guisug
CARGO_TARGET_DIR=F:/ai/dds/converter-nif-p1/target cargo test -p render3d --release --lib debug
CARGO_TARGET_DIR=F:/ai/dds/converter-nif-p1/target cargo test -p render3d --release --test debug --test source --test async_compile --test settings
SDC_CS_SHADERS='C:\_mount\moddingvol\Acherus\mods\CommunityShaders_AIO-2026-10-03T06-46Z\Shaders' CARGO_TARGET_DIR=F:/ai/dds/converter-nif-p1/target cargo test -p render3d --release --test debug --test missing_fill -- --ignored
git add crates/render3d/src/debug.rs crates/render3d/src/source.rs crates/render3d/src/lib.rs crates/render3d/tests/debug.rs
git commit -m "render3d: debug views (Albedo, NormalWorld, Roughness, Metallic, AO) injected into the user's Lighting.hlsl

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```
Expected: PASS, including the two ignored installed-CS tests (WARP compiles of the real CS take minutes; run them once). If the AO readback is 0 rather than 192, the VS did not write `Color = 1` for this permutation (L:266-270): report it, do not loosen the tolerance.

- [ ] **Step 5: Write the failing GUI tests**

In `crates/gui/tests/viewport3d.rs`, replace the first lines of `the_debug_selector_lists_only_views_the_renderer_can_make` (2571-2574) with:

```rust
#[test]
fn the_debug_selector_lists_only_views_the_renderer_can_make() {
    use v3::DebugView;
    // Without render3d's support (no thread yet) only Lit and UV checker are offered.
    assert_eq!(DebugView::ALL.len(), 7);
```

(the rest of that test stays: with no support the five labels are absent.) Append:

```rust
fn shape_info(id: u32, m: MaterialState) -> ShapeInfo {
    ShapeInfo { id: ShapeId(id), name: format!("s{id}"), material: m, status: ShapeStatus::Drawing }
}

fn pbr() -> MaterialState {
    let mut m = MaterialState::builtin_default();
    m.slsf2 |= nif::material::SLSF2_TRUE_PBR;
    m
}

#[test]
fn offered_views_follow_support_and_the_loaded_materials() {
    use v3::DebugView::*;
    let (mut v, _cmds) = rendered();
    v.handle(v3::V3Reply::Shapes(vec![shape_info(0, MaterialState::builtin_default())]));
    assert_eq!(v.offered(), [Lit, UvChecker], "no support reported yet");
    v.handle(v3::V3Reply::DebugSupport(true));
    assert_eq!(v.offered(), [Lit, UvChecker, Albedo, Normals, Roughness, Ao], "non-PBR: no Metallic");
    assert_eq!(v.stand_in(Roughness), Some("1 − specular mask"));
    assert_eq!(v.stand_in(Albedo), None);
    let mut face = MaterialState::builtin_default();
    face.shader_type = 4;
    v.handle(v3::V3Reply::Shapes(vec![shape_info(0, face.clone())]));
    assert!(!v.offered().contains(&Ao), "FaceGen: vertex AO is constant");
    v.handle(v3::V3Reply::Shapes(vec![shape_info(0, face), shape_info(1, pbr())]));
    assert_eq!(v.offered().len(), 7, "one PBR shape offers everything");
    assert_eq!(Lit.next(&v.offered()), UvChecker);
    assert_eq!(Ao.next(&v.offered()), Lit, "wraps over the offered views");
    assert_eq!(Metallic.next(&[Lit, UvChecker]), Lit, "a view not offered goes to the first");
}

#[test]
fn a_load_that_drops_the_current_view_falls_back_to_lit() {
    let (mut v, cmds) = rendered();
    v.handle(v3::V3Reply::DebugSupport(true));
    v.handle(v3::V3Reply::Shapes(vec![shape_info(0, pbr())]));
    v.set_debug(v3::DebugView::Metallic);
    assert!(matches!(cmds.try_recv(), Ok(v3::V3Cmd::SetDebug(v3::DebugView::Metallic))));
    v.handle(v3::V3Reply::Reconfigured(1));
    v.handle(v3::V3Reply::Shapes(vec![shape_info(0, MaterialState::builtin_default())]));
    assert_eq!(v.debug(), v3::DebugView::Lit);
    assert!(matches!(cmds.try_recv(), Ok(v3::V3Cmd::SetDebug(v3::DebugView::Lit))));
}

#[test]
fn losing_debug_support_falls_back_to_lit_with_a_status_line() {
    let (mut v, cmds) = rendered();
    v.handle(v3::V3Reply::DebugSupport(true));
    v.handle(v3::V3Reply::Shapes(vec![shape_info(0, pbr())]));
    v.set_debug(v3::DebugView::Albedo);
    let _ = cmds.try_recv();
    v.handle(v3::V3Reply::Reconfigured(1));
    // A CS update without the anchor (SetShaders), or a refused SetDebug.
    v.handle(v3::V3Reply::DebugSupport(false));
    assert_eq!(v.debug(), v3::DebugView::Lit);
    assert_eq!(v.status.as_deref(), Some(v3::DEBUG_UNSUPPORTED));
    assert!(matches!(cmds.try_recv(), Ok(v3::V3Cmd::SetDebug(v3::DebugView::Lit))));
    assert_eq!(v.offered(), [v3::DebugView::Lit, v3::DebugView::UvChecker]);
}

#[test]
fn the_view_section_lists_the_offered_views_with_the_stand_in_hover() {
    let (mut v, _cmds) = rendered();
    v.handle(v3::V3Reply::DebugSupport(true));
    v.handle(v3::V3Reply::Shapes(vec![shape_info(0, MaterialState::builtin_default())]));
    let mut h = harness(v);
    h.run();
    h.get_by_label("View").click();
    h.run();
    for offered in ["Lit", "UV checker", "Albedo", "Normals", "Roughness", "AO"] {
        h.get_by_label(offered);
    }
    assert!(h.query_by_label("Metallic").is_none());
    h.get_by_label("Roughness").hover();
    h.run();
    h.get_by_label_contains("1 − specular mask");
}
```

and in `the_uv_checker_toggles_without_a_reload` (Task 5) add at the end:

```rust
    assert_eq!(v.offered(), [v3::DebugView::Lit, v3::DebugView::UvChecker], "the stand-in has no debug anchor");
    assert!(!v.debug_support());
```

Run: `cd /f/ai/dds/converter-nif-guisug && CARGO_TARGET_DIR=F:/ai/dds/converter-nif-p1/target cargo test -p sdc-gui --release --test viewport3d`
Expected: compile errors (no `DebugSupport`, `offered`, `Albedo`, …).

- [ ] **Step 6: Implement the GUI**

`crates/gui/src/model/viewport3d.rs`:
- imports (17-20): add `Availability, DebugOutput,` to the `use render3d::{ … }` list.
- `DebugView` (454-480) becomes:

```rust
/// The 3D panel's debug views (AG75, GUI suggestions items 1 and 4). Lit and UV checker are always offered; the others
/// when render3d reports the CS anchor (`V3Reply::DebugSupport`) and a loaded shape has the term ([`Viewport3d::offered`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum DebugView {
    #[default]
    Lit,
    /// A generated checker over every shape's slot 0 (diffuse / base colour): render3d's `override_slot(0, …)`.
    UvChecker,
    Albedo,
    Normals,
    Roughness,
    Metallic,
    Ao,
}

impl DebugView {
    pub const ALL: [DebugView; 7] = [
        DebugView::Lit,
        DebugView::UvChecker,
        DebugView::Albedo,
        DebugView::Normals,
        DebugView::Roughness,
        DebugView::Metallic,
        DebugView::Ao,
    ];

    pub fn label(self) -> &'static str {
        match self {
            DebugView::Lit => "Lit",
            DebugView::UvChecker => "UV checker",
            DebugView::Albedo => "Albedo",
            DebugView::Normals => "Normals",
            DebugView::Roughness => "Roughness",
            DebugView::Metallic => "Metallic",
            DebugView::Ao => "AO",
        }
    }

    /// The render3d output of this view; the UV checker draws Lit (its checker is a slot override).
    pub fn output(self) -> DebugOutput {
        match self {
            DebugView::Lit | DebugView::UvChecker => DebugOutput::Lit,
            DebugView::Albedo => DebugOutput::Albedo,
            DebugView::Normals => DebugOutput::NormalWorld,
            DebugView::Roughness => DebugOutput::Roughness,
            DebugView::Metallic => DebugOutput::Metallic,
            DebugView::Ao => DebugOutput::Ao,
        }
    }

    /// The next of `offered`, wrapping (the V key); a view not in it goes to the first.
    pub fn next(self, offered: &[DebugView]) -> DebugView {
        match offered.iter().position(|d| *d == self) {
            Some(i) => offered[(i + 1) % offered.len()],
            None => offered.first().copied().unwrap_or(DebugView::Lit),
        }
    }
}

/// The status line when render3d cannot draw the selected debug view any more (`DebugSupport(false)`).
pub const DEBUG_UNSUPPORTED: &str =
    "These Community Shaders have no debug-view anchor in Lighting.hlsl: back to Lit";
```

- `V3Reply` gains, after `Coverage`:

```rust
    /// `Renderer::debug_supported()`: after the thread starts, after `SetShaders`, and `false` when a `SetDebug` was
    /// refused.
    DebugSupport(bool),
```

- `Viewport3d` gains a field after `debug: DebugView,` (and `new()` sets `debug_support: false,` after `debug: DebugView::Lit,`):

```rust
    /// The last `V3Reply::DebugSupport`: render3d can draw Albedo … AO with these shaders.
    debug_support: bool,
```

- after `set_debug` (1375-1385), add:

```rust
    pub fn debug_support(&self) -> bool {
        self.debug_support
    }

    /// The views the selector and the V key offer: Lit and UV checker always; each other view when render3d supports
    /// debug views and at least one loaded shape has that term (`Availability` not `None`).
    pub fn offered(&self) -> Vec<DebugView> {
        DebugView::ALL
            .into_iter()
            .filter(|d| match d.output() {
                DebugOutput::Lit => true,
                o => {
                    self.debug_support
                        && self
                            .shapes
                            .iter()
                            .any(|s| o.availability(&s.material) != Availability::None)
                }
            })
            .collect()
    }

    /// The stand-in a view uses for some loaded shape, for the hover text.
    pub fn stand_in(&self, d: DebugView) -> Option<&'static str> {
        self.shapes
            .iter()
            .find_map(|s| match d.output().availability(&s.material) {
                Availability::StandIn(n) => Some(n),
                _ => None,
            })
    }
```

- `handle`, the `Shapes` arm (1622-1640): after `self.shapes = s;` add

```rust
                    // A load that leaves the current view not offered falls back to Lit.
                    if !self.offered().contains(&self.debug) {
                        self.set_debug(DebugView::Lit);
                    }
```

- `handle`, new arm after `V3Reply::Coverage(c) => …`:

```rust
            V3Reply::DebugSupport(on) => {
                self.debug_support = on;
                if !on && self.debug.output() != DebugOutput::Lit {
                    self.set_debug(DebugView::Lit);
                    self.status = Some(DEBUG_UNSUPPORTED.into());
                }
            }
```

- `render_loop`: after `send(V3Reply::OffInPreview(r.off_in_preview()));` (2050) add `send(V3Reply::DebugSupport(r.debug_supported()));`. Declare `let mut reshaded = false;` next to `let mut debug_changed = false;` (2098). The `SetShaders` arm becomes `V3Cmd::SetShaders(f) => { r.set_shaders(f); reload = true; reshaded = true; None }`. After `pump(&mut r);` (the one following the tex-command block) add:

```rust
        if reshaded {
            send(V3Reply::DebugSupport(r.debug_supported()));
        }
```

- the `SetDebug` arm (as Task 5 left it) becomes:

```rust
                V3Cmd::SetDebug(d) => {
                    let on = d == DebugView::UvChecker;
                    if tex.checker != on {
                        tex.checker = on;
                        // render3d keeps the override across loads, swaps and edits: no reload either way.
                        let _ = r.override_slot(0, on.then(checker_dds).as_deref());
                        debug_changed = true;
                    }
                    if r.debug_output() != d.output() {
                        match r.set_debug_output(d.output()) {
                            Ok(()) => debug_changed = true,
                            // Only a missing anchor refuses (R8): the model goes back to Lit.
                            Err(_) => send(V3Reply::DebugSupport(false)),
                        }
                    }
                    ack.0 += 1;
                    None
                }
```

`crates/gui/src/ui/viewport3d.rs`:
- the V key (53-55):

```rust
    if pressed("3d-debug") {
        let next = v.debug().next(&v.offered());
        v.set_debug(next);
    }
```

- the radio list (155-166):

```rust
            ui.horizontal_wrapped(|ui| {
                ui.label("Debug view");
                let mut d = v.debug();
                for o in v.offered() {
                    let r = ui.radio_value(&mut d, o, o.label());
                    if let Some(n) = v.stand_in(o) {
                        r.on_hover_text(format!("Some shapes show a stand-in: {n}"));
                    }
                }
                v.set_debug(d);
                ui.weak(format!("({})", super::actions::keys_text("3d-debug")));
            });
```

- [ ] **Step 7: Run the GUI tests, gate, commit**

```bash
cd /f/ai/dds/converter-nif-guisug
CARGO_TARGET_DIR=F:/ai/dds/converter-nif-p1/target cargo test -p sdc-gui --release --test viewport3d --test set_preview_model --test set_preview_ui
CARGO_TARGET_DIR=F:/ai/dds/converter-nif-p1/target cargo fmt --all --check
CARGO_TARGET_DIR=F:/ai/dds/converter-nif-p1/target cargo clippy --workspace --all-targets --release -- -D warnings
git add crates/gui/src/model/viewport3d.rs crates/gui/src/ui/viewport3d.rs crates/gui/tests/viewport3d.rs
git commit -m "gui: the 3D debug selector offers render3d's Albedo/Normals/Roughness/Metallic/AO when supported

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```
Expected: PASS. `the_3d_keys_reset_cycle_and_are_listed` still cycles Lit → UV checker → Lit (a bare `with_state(Ready)` has no support).

---

### Task 8: DECISIONS Note AU and the branch gate

**Files:**
- Modify: `docs/DECISIONS.md` (append after the last note)

- [ ] **Step 1: Check the letter**

Run: `cd /f/ai/dds/converter-nif-guisug && grep -n "^## Note" docs/DECISIONS.md | tail -3`
Expected: the last heading is `## Note AT — …`. If it is not, use the next free letter and change `AU` throughout the text below.

- [ ] **Step 2: Append the note** (at the end of `docs/DECISIONS.md`)

```markdown
## Note AU — GUI suggestions, NIF side (plan rulings, 2026-10-15)

Spec `F:\ai\dds\nifparseref\plan\gui-suggestions-nif\2026-10-15-gui-suggestions-nif-design.md` (owner-approved 2026-10-15, including edits to the converter-team GUI files `model/viewport3d.rs`, `ui/viewport3d.rs`, `settings/layout.rs` and the tests `viewport3d.rs`, `pgp.rs`, `window_layout.rs`, `color_fields.rs`, `set_preview_model.rs`, `open_in_textures.rs`). Plan `…\2026-10-15-plan-gui-suggestions-nif.md`, rulings R1-R15.

- **AU1 SLSF names.** `nif::material::{SLSF1_NAMES, SLSF2_NAMES}` (moved verbatim; bit 8 `Receive_Shadows`, SLSF2 bit 23 `Unused01 (TruePBR)`). `model::meshes` re-exports them.
- **AU2 pgpsim `alphaprop_*`.** The four PGPatcher 2.2.0 keys on top of the 1.2.0 simulator (owner D2; a full 2.2 audit is a later plan). Integers go through `uint_in` (2.2 `unsignedFromJSON`): a bad value is ignored with a range warning, never a Skip. Delete beats create; create starts from nifly's 4844/128.
- **AU3 Missing-texture fill (replaces AG76's swap).** `Renderer::set_missing_texture`: colour registers only, t0 and a cube t4 (owner D3); normal, glow, specular and RMAOS stay neutral, and `Bound` is not changed, so a missing data map no longer tints. RFAOS guesses never fill.
- **AU4 HDRI yaw and RGB background (amends AG74, AG9).** `Scene::env_yaw` turns the backdrop and its ambient; the GUI passes `sun_yaw` (owner D4). `Background::Solid` is linear RGB; `[layout.look] bg` (sRGB) replaces `grey`, which is still read when `bg` is missing and is never written. The grey slider is gone (the colour picker covers it).
- **AU5 UV checker by slot (amends AG75).** `Renderer::override_slot(0, …)`, kept across loads; Lit no longer reloads. A swap or A/B flip under the override updates the entry and `Bound` but not the screen (R9).
- **AU6 3D index.** `nif::graph::index3d` (PGPatcher `shapesWith3DIdx`: pre-order over node children, first visit wins; particle systems, `NiMeshParticleSystem` included, take an index). `shape.index3d`, modscan schema 8 (one full rescan). The Meshes card reads `#index3d`, or `#– (not in scene tree)`.
- **AU7 Debug views (amends AG75).** Albedo, Normals, Roughness, Metallic, AO by one block injected before CS's single `return psout;` (no vendored shader). Non-PBR Roughness is `1 − specular mask` and AO is vertex AO (stand-ins, named in the hover); non-PBR Metallic and FaceGen/Hair/Eye AO are not offered. Without the anchor the views are not offered (`V3Reply::DebugSupport`).
- **AU8 One branch.** All seven items landed on `nif/m-guisug`, not one branch each (R1).
```

- [ ] **Step 3: Full gate**

```bash
cd /f/ai/dds/converter-nif-guisug
df -h /f
git diff --stat nif/main -- Cargo.lock   # must print nothing
CARGO_TARGET_DIR=F:/ai/dds/converter-nif-p1/target cargo fmt --all --check
CARGO_TARGET_DIR=F:/ai/dds/converter-nif-p1/target cargo clippy --workspace --all-targets --release -- -D warnings
CARGO_TARGET_DIR=F:/ai/dds/converter-nif-p1/target cargo test --workspace --release -- --skip render_
CARGO_TARGET_DIR=F:/ai/dds/converter-nif-p1/target cargo test --workspace --release render_ -- --test-threads=1
```
Expected: everything passes; `Cargo.lock` unchanged. If a golden or `gui_repro` test fails, stop and report it; do not edit it.

- [ ] **Step 4: Commit**

```bash
git add docs/DECISIONS.md
git commit -m "docs: DECISIONS Note AU (GUI suggestions, NIF side)

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

---

## Self-review

**1. Spec coverage**

| Spec item | Task |
|---|---|
| §7 SLSF names, re-export, bit-8/bit-23 and constant tests | 1 |
| §5 `INT_KEYS`, `uint_in`, warnings, patch, version line, GUI probes, unit tests (in range, 300/−1/1.5/"8"/true, delete > create, 4844/128, no create) | 2 |
| §3 `set_missing_texture`, `TexEntry.missing`, `report_missing`, colour registers, `rebind` None branch, `rebuild_device_on`, GUI `make` + delete loop and `magenta_dds`, tests (diffuse, cube `#[ignore]`, normal + `Bound`, t71, None), drop GUI magenta test | 3 |
| §2 `env_yaw`, `Background::Solid([f32;3])`/`grey`/`rgb`, `ambient6_at`, `cells`/`integrate`, backdrop `Flags.y`, per-channel clear, GUI `Look.bg`/`LookSettings.bg`/picker/HDRI note/`env_yaw`, tests (bit-equal, π swap, sign, background.rs, cs_features.rs, RGB clear, GUI 1040/1014/env_yaw, window_layout, color_fields, old grey) | 4 |
| §4 `override_slot`, storage, `bind_textures`, live apply, no clobber, `rebuild_device_on`, GUI SetDebug no reload, delete `checker_over`/`diffuse_paths`/refresh, tests (survives load, empty slot, variant, None restores, mismatch warning) | 5 |
| §6 `index3d`, `NifScan.index3d`, root rule, particle index, `ShapeView.index3d`, schema 8 at all SQL sites + `shapes_of` T=15, GUI header and hover, tests (order, particle, unreachable, shared, cycle/depth, closure, `#[ignore]` owner logs, round trip, v7 rebuild, literals) | 6 |
| §1 `DebugOutput`, `Availability`, `get_debug`, inject text, `wanted()` keys, fallback, switching, `rebuild_device_on`, GUI `DebugView` + `output`, `DebugSupport`, `offered`, radio list, V key, load fallback, fresh renderer, tests (unit, stand-in, Lit key, `#[ignore]` readback, GUI ALL/offered/next/no reload) | 7 |
| DECISIONS one note | 8 |

Gaps found and fixed inline: Metallic for non-PBR (R4), the anchor false positive (R5), the Err channel (R8), `Bound` under an override (R9), the test hooks needed to observe defaults (R13).

**2. Placeholder scan:** no TBD/TODO. Two steps say "verbatim" for moved text (the SLSF arrays in Task 1, the lines Task 3 Step 6 deletes); both name the exact source lines.

**3. Type consistency:** `override_slot(usize, Option<&[u8]>) -> Result<Vec<(ShapeId, u32)>, String>`, `set_missing_texture(Option<[u8; 4]>)`, `set_debug_output(DebugOutput) -> Result<(), String>`, `draw_default_at(ShapeId, u32) -> Option<(SrvKind, [u8; 4])>`, `draw_bound(ShapeId) -> Option<Vec<u32>>`, `DebugView::next(self, &[DebugView])`, `Look.bg: [f32; 3]`, `LookSettings.bg: Option<[f32; 3]>`, `NifScan.index3d: BTreeMap<u32, u32>` and `ShapeView.index3d: Option<u32>` are used with the same names and types in every task.

**4. Review Focus:** each of its five lines has a named test in its owning task (Tasks 3, 4, 5, 6, 7).
