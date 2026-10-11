# sdc M6 Track C: mip filters and resize, implementation plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** `mips.filter` gains `tent`, `mitchell` and `kaiser` (sRGB colour; `tent` also for HDR). They are built **directly from mip 0** with the kernel widened 2^N, in float, as mipcheck B8 measured. `mips.edge` (`wrap`/`clamp`) arrives. `resize = pot-down` turns a non-power-of-two source into a power-of-two base in Preprocess, so `DIM-NPOT` stops blocking when it is set. The sdc-gui "Filter" row in the basic Mipmaps group becomes a live dropdown. With the defaults (`box`, `wrap`, `resize = none`), no output byte and no `profile_sha256` changes.

**Architecture:**
- **Kernels** (new `crates/engine/src/filter.rs`, Task 2): tent, Mitchell (B = C = 1/3, radius 2) and Kaiser (width 3, α 4, stretch 1). There are per-axis polyphase tap tables (f64 weights, normalised per output texel, `wrap` or `clamp` edges) and a separable, rayon row-parallel RGB resample from any n to any m. One implementation serves both the mips and the resize.
- **Mips** (`mips.rs`, Task 3): `build_chain_with` keeps the box chain exactly as today. For an allowed non-box filter, the RGB of each level ≥ 1 is then **replaced** by `filter::resample_rgb(base, level dims)`, from mip 0 and never recursive. `finalize` clamps it as before. Alpha always stays the recursive box, so coverage, the `_cnr` self-bake, the bake's `LengthPyramid` and the roughness r² path see bytes identical to today. The spec's box-only contents can never take a non-box filter (Task 1 check).
- **Resize** (new `crates/engine/src/resize.rs`, Tasks 4–5): Preprocess step 6 (spec §5.4) runs in the Work's domain. Each axis goes to the largest power of two ≤ its size. The effective mip filter is used (box means area-exact), with the same edge. Alpha is always area box. Alpha-weighted colour is weighted by area × alpha. Roughness is resized on r². Normals are box-resized and then renormalised by the existing mip-0 step. Policy, the bake plan and admission all use the post-resize size.
- **GUI** (Task 6): there is a live Filter combo in the basic Mipmaps group, with per-option locks, and an Edge row under it. "Resize" goes in Advanced texture fields. A "Resize to a power of two" button sits next to Acknowledge on `DIM-NPOT`.

**Tech Stack:** Rust 2021 workspace (rustc 1.99.0, MSRV 1.88), crates `engine`, `cli`, `gui` (egui 0.36.2), with `rayon`, `serde`, `schemars`. No new dependencies. The Bessel I0 for Kaiser is a 25-term power series written in `filter.rs`.

**Spec:** `docs/superpowers/specs/2026-10-08-skyrim-dds-converter-design.md`:
- §3.2: `mips.filter` and `mips.edge` rows. "Mitchell and Kaiser for Color only"; edge default `wrap`.
- §5.4 step 6: resize is polyphase, in the content's filtering domain.
- §5.5: the content/filter table, "each level is built from the base footprint", Mitchell (1/3, 1/3), Kaiser (w3, α4). Normals, data, roughness and alpha-weighted colour are box only; HDR is "Box (tent at most)".
- §6.2–6.3: the bake.
- §9: the Mipmaps group holds levels, filter, edge and normal handling.
- §10: the `DIM-NPOT` remedy is `--set resize=pot-down`.

Other sources:
- The roadmap `docs/superpowers/plans/2026-10-08-skyrim-dds-converter-phase0-2.md`, "M6 — Presets completion and remaining inputs": the scope items "NPOT resize policy" and "tent/Mitchell/Kaiser filters and `mips.edge`", and the carried items "GUI filter dropdown", "Tent, Mitchell and Kaiser mips", "`FIELD_PATHS` lacks … `resize`" and "16K peak memory".
- `F:/ai/dds/mipcheck/RESULTS-2.md`, B8 and B4. B8's script is `F:/ai/dds/mipcheck/scripts/b8.py`, with kernels and `weight_matrix`.
- `docs/DECISIONS.md` Note R (R1, R2, R4, R5, R11, R12, R13).

## Global Constraints

- **Order:** Tracks B (`2026-10-11-sdc-m6-inputs-formats.md`) and A (`2026-10-11-sdc-m6-shader-modes.md`) execute and merge **first**. This plan executes **third**, on branch `feat/m6-mips-resize`, created from `main` after both have merged. Never commit to `main`. Never push.
  - Before Task 1, read each shared file at its merged state and add to it rather than replace it: `config.rs`, `overrides.rs` `FIELD_PATHS` (Track B adds `reencode`), `codes.rs`, `policy.rs`, `ops.rs`, `api.rs`, `crates/gui/src/model/fields.rs`, `crates/gui/src/ui/settings_panel.rs`, `docs/DECISIONS.md` (Note S) and the spec.
  - **Code count:** written below as **merged + 1**. Let `M` be `Code::ALL.len()` on the merged `main`; this plan's count test asserts `M + 1`.
  - **Versions:** written as "next minor after main's version" where one applies. This plan bumps none (ruling S-mips-9); Task 8 checks that.
- **Gates:** run them unpiped before every commit, and all three must pass. In Git Bash, first run `export PATH="$HOME/.cargo/bin:$PATH"`.
  ```bash
  cargo fmt --all --check
  cargo clippy --workspace --all-targets -- -D warnings
  cargo test --workspace
  ```
  (`cargo fmt --all` first if the check fails.)
- **Golden and `gui_repro`:** `crates/engine/tests/golden.rs` and `crates/cli/tests/gui_repro.rs` must stay green **with no expectation changed**. With default settings this plan changes no output byte. If any existing golden or test assertion fails, stop and report `DONE_WITH_CONCERNS` with the test name. Do not edit the test.
- **Schemas:** a change to a code, a config field or a type in `schema/*.json` needs `cargo run -p sdc -- presets schema all --out schema`. Commit the regenerated files.
- **Python:** never run `python -` or `python3 -` with a heredoc or stdin. They hang on this machine. A reference computation belongs in Rust test code (an f64 `#[cfg(test)]` helper), not in Python.
- **Commits:**
  - Commit only the files you changed, by explicit path. No `git add -A` and no `git add .`.
  - End each message with `Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>`.
- **Off limits:** never touch `F:/ai/dds/bench`, `F:/ai/dds/converter-nif` or any `nif/*` branch. Do not touch `crates/nif`, `crates/vfs` or `crates/modscan`, or DECISIONS Note Q. `G:/` is read-only.
- **Downloads:** none are needed. If a step seems to need a crate that is not in `Cargo.lock`, stop and report `NEEDS_CONTEXT`.

## Rulings (recorded in DECISIONS Note S as `S-mips-*` by Task 8)

| Id | Ruling | Evidence / why |
|---|---|---|
| S-mips-1 | **The default filter stays `box`** for every sub-type and preset. No preset gains a non-box default, so existing outputs are unchanged | Spec §3.2 default `box`; §5.5 "box is area-exact and never overshoots" |
| S-mips-2 | **Where non-box filters are allowed** (spec §3.2, §5.5 read literally). `tent`, `mitchell` and `kaiser` require category Color, sRGB content (domain `LinearLight`) and an alpha policy that does not weight (`remove` or `data`). `tent` is also allowed for HDR (`format = bc6h_uf16`). Anything else with a non-box filter is **`CFG-INVALID`** (Error), with the reason. That covers normals, `_msn`, `_flow`, data, grayscale, packed, roughness members, linear-content colour, and colour with `cutout`/`blend`/`cutout_blend`. The GUI shows these as locked options. The engine also falls back to box in `mips.rs`, but only as a guard; a blocked job never reaches it | Spec: "Mitchell and Kaiser for Color only"; normals/data/roughness "Box"; alpha-weighted colour "Alpha-weighted box on RGB"; HDR "Box (tent at most)". `CFG-INVALID` is the existing pattern for impossible combinations (e.g. `bc1_transparent_black`), so no new code is needed |
| S-mips-3 | **Build tent/Mitchell/Kaiser levels directly from mip 0**, with the kernel widened by the level's scale (2^N at power-of-two sizes; `src/dst` per axis in general). Values stay f32 between steps; weights are f64. **Box stays recursive**, with today's code untouched. Only RGB is non-box: **alpha keeps the recursive box** (spec §5.5 "box on alpha", "Alpha separate; never sharpened") | RESULTS-2 B8: recursion costs tent 1.1–2.7 dB and Mitchell 0.8–1.8 dB (Kaiser ≤ 0.5 dB) against Lanczos-8; 8-bit intermediates add up to 5 codes with Kaiser; box is identical either way |
| S-mips-4 | **Kaiser parameters are fixed at width 3, α 4, stretch 1.** Mitchell is B = C = 1/3 with radius 2, and tent has radius 1. They are constants in `filter.rs`, not profile fields, so the spec's `kaiser{width,alpha,stretch}` syntax is deferred (owner question 1) | Spec §5.5 "Kaiser (w3, α4)"; B8 measured exactly w3 α4 s1; B4: that is also nvtt_export's default (not nvcompress's Kaiser, which no setting reproduces) |
| S-mips-5 | **`mips.edge` = `wrap` (default, every sub-type) or `clamp`.** Only the polyphase kernels read it, in the mips and in a non-box resize. Box (recursive pair, area-exact odd path, area-exact resize) never reads outside the image, so it ignores the edge | Spec §3.2 default `wrap`, "irrelevant to box"; minimal |
| S-mips-6 | **`resize` = `none` (default) or `pot-down`**, a top-level profile field (spec §8/§10 spelling `--set resize=pot-down`). `pot-down` maps each axis independently to the largest power of two ≤ its size (1000×600 → 512×512; a power-of-two axis is kept). The aspect may change; UVs are normalised, so meshes map the same. Resizing runs in Preprocess as step 6 (after sRGB decode), in the Work's domain, with the **effective mip filter** and edge: box is area-exact (the generalised `downsample_area`), and tent/Mitchell/Kaiser are polyphase on RGB. Alpha is always area box. Alpha-weighted colour is weighted by area × alpha. A roughness member is resized on r² (square, resize, √). Normals, `_msn` and `_flow` are box-resized as vectors; mip 0 is then renormalised by the existing `base_level` unless `mips.normal_base = keep`. Upscaling and a size cap are not in scope | Spec §5.4 step 6; §5.5 domains; R1, R11 |
| S-mips-7 | **Resize in planning:** `DIM-NPOT`, `DIM-SMALL` and `DIM-FLOW-SMALL` test the **post-resize** size. A resized job gets the new **`DIM-RESIZED`** (Info) code: "1000x600 resized to 512x512 (resize = pot-down)", data `{"from":[w,h],"to":[w',h']}`. Code count is merged + 1. The bake plans its shift on the post-resize sizes, and `BakeJob::load_lengths` already runs `preprocess` with the `_n`'s profile, so the pyramid is built from the resized, renormalised `_n` exactly as the `_n`'s job writes it (R1, R5). `BakeJob::identity_sha256` adds the `_n`'s `resize` only when it is not `none`, so existing identities are unchanged. `PACK-SIZE-MISMATCH` still compares source sizes before assembly; the packed image is resized after assembly | One rule (post-resize size) everywhere; the bake stays exact at power-of-two ratios |
| S-mips-8 | **Coverage reference after a resize** is the resized mip 0 (`base_coverage(&chain[0])`, unchanged code). Matching mip 0's coverage to the source is deferred | Coverage compares levels with mip 0 (spec §5.5); the resize is a new mip 0 |
| S-mips-9 | **Hash stability, no version bumps.** `mips.edge` and `resize` serialise only when not default (`skip_serializing_if`), so `profile_sha256` and every manifest stay as they are for unchanged settings. An opt-in changes the hash and `if-changed` re-encodes it. No output for an unchanged configuration changes, so the workspace version is **not** bumped and `sdc-base` is not touched (Task 8 proves it with the unchanged golden suite) | R12 re-encoded everything for two new fields; here nothing changes for defaults |
| S-mips-10 | **GUI:** the basic Mipmaps group's "Filter" row becomes a live combo (Box / Tent / Mitchell / Kaiser). Disallowed options are greyed out with the S-mips-2 reason. It **stays in the basic panel** and never moves to Advanced (owner, 2026-10-11). An "Edge" row (Wrap / Clamp) sits under it, locked "only tent, Mitchell and Kaiser read the edge" while the filter is box. "Resize" (None / Down to a power of two) goes in Advanced texture fields. The `DIM-NPOT` decision box gains a "Resize to a power of two" button next to "Acknowledge for this texture"; it writes `resize = "pot-down"` on that file's `[files]` entry. The stale hover text promising "Lanczos" goes | Roadmap carried item; spec §9 |
| S-mips-11 | **Memory and speed (carried 16K item).** The new kernels are separable and row-parallel on the shared rayon pool. Each level costs about `radius·2·w·h` taps and holds one horizontal intermediate of `h × w/2^N` texels. Admission (`api::job_bytes`) adds `8·w·h` bytes for a non-box filter and `32·w·h` for a resize. The 13 GiB box peak at 16K and the single-threaded per-channel `powf` in Preprocess stay **deferred** to the scheduler revisit; this track does not make them worse beyond the counted bytes | Roadmap carried item ("revisit admission and the scheduler together") |

**Owner answers (2026-10-11, "agreed with your recommendations"):** all three keep the plan default. Record them in Note S (S-mips-4, S-mips-2, S-mips-6).
1. **Kaiser knobs:** keep w3 α4 s1 fixed (S-mips-4), or expose `mips.kaiser = { width, alpha, stretch }` as the spec's syntax suggests → **Fixed** (owner).
2. **Foliage colour:** spec §5.5 keeps alpha-tested and blended colour on alpha-weighted box (S-mips-2). Should alpha-weighted **tent** (non-negative, safe to weight) be allowed for `cutout`/`blend` diffuse? Mitchell and Kaiser would stay box there, because negative lobes make Σ(k·α) unstable. → **No, box stays** (owner); revisit later.
3. **NPOT default:** `resize` stays `none`, so `DIM-NPOT` still blocks until the user opts in. Should a preset (or `ui_sky`, whose interface art is often NPOT) default to `pot-down` → **No preset defaults to pot-down**; DIM-NPOT blocks until the user opts in (owner).

## Review Focus

Most likely first.

1. **Default bytes unchanged.** Box chains are bit-identical (the box code path is untouched). `profile_sha256` of every built-in profile is unchanged (Task 1 test), and every golden passes unedited (Task 7).
2. **Weights.** Every tap row sums to 1 in f64, including small images where the widened kernel wraps the whole axis several times (a 4-texel axis at level 2 with Kaiser), and `clamp` at both ends. Tests are in Task 2.
3. **Direct, not recursive.** A level-2 Kaiser/tent/Mitchell level equals the direct resample from mip 0 and differs from resampling level 1 (Task 3). Box equals the recursive chain (Task 3).
4. **Alpha and coverage untouched.** On a `data`-alpha diffuse with Kaiser, alpha is bit-identical to box. A `cutout` diffuse with Kaiser is `CFG-INVALID`, never a sharpened alpha (Tasks 1 and 3).
5. **Clamping.** Kaiser and Mitchell overshoot below 0 and above 1; `finalize` clamps LinearLight. HDR only allows tent, which cannot go negative (Task 3).
6. **Resize semantics:** per-axis pot-down; roughness on r²; alpha always area box; normals renormalised at mip 0; the bake shift on post-resize sizes (Tasks 4–5).
7. **GUI:** the Filter row is live, in the basic panel, with greyed-out options and reasons (Task 6).

## Interface registry (names every task must use)

```rust
// config.rs
cfg_enum!(MipFilter, "lowercase" { Box, Tent, Mitchell, Kaiser });
cfg_enum!(MipEdge, "lowercase" { Wrap, Clamp });
cfg_enum!(ResizeMode, "kebab-case" { None, PotDown });        // "none" | "pot-down"
impl Default for MipEdge { fn default() -> Self { MipEdge::Wrap } }
impl Default for ResizeMode { fn default() -> Self { ResizeMode::None } }
impl MipEdge { pub fn is_wrap(&self) -> bool }
impl ResizeMode { pub fn is_none(&self) -> bool }
// MipSpec gains, after `filter`:
//   #[serde(default, skip_serializing_if = "MipEdge::is_wrap")] pub edge: MipEdge,
// Profile gains, after `header`:
//   #[serde(default, skip_serializing_if = "ResizeMode::is_none")] pub resize: ResizeMode,

// filter.rs (new, `pub mod filter;` in lib.rs)
pub const MITCHELL_B: f64 = 1.0 / 3.0;
pub const MITCHELL_C: f64 = 1.0 / 3.0;
pub const KAISER_WIDTH: f64 = 3.0;
pub const KAISER_ALPHA: f64 = 4.0;
pub const KAISER_STRETCH: f64 = 1.0;
pub fn radius(f: MipFilter) -> f64;                 // box 0.5, tent 1, mitchell 2, kaiser 3
pub fn kernel(f: MipFilter, x: f64) -> f64;         // x in destination texels
pub fn filter_allowed(p: &Profile) -> Result<(), &'static str>;   // S-mips-2
pub(crate) type Taps = Vec<Vec<(u32, f64)>>;        // per destination texel: (source index, weight)
pub(crate) fn axis_taps(n: u32, m: u32, f: MipFilter, edge: MipEdge) -> Taps;   // f != Box
pub(crate) fn resample_rgb(src: &Rgba32F, dims: (u32, u32), f: MipFilter, edge: MipEdge) -> Rgba32F;
//   RGB polyphase, A of the result = 0.0 (the caller writes alpha)

// mips.rs
pub(crate) fn area_resample(src: &Rgba32F, dims: (u32, u32), weighted: bool) -> Rgba32F;
//   today's `downsample_area` generalised to any target size (`axis_weights(n, m)` already is)
pub(crate) fn effective_filter(work: &Work, p: &Profile) -> MipFilter;   // Box unless filter_allowed && !weighted_rgb

// resize.rs (new, `pub mod resize;` in lib.rs)
pub fn target_dims(dims: (u32, u32), mode: ResizeMode) -> (u32, u32);
pub(crate) fn resize_work(work: &mut Work, p: &Profile);   // Preprocess step 6; no-op when dims unchanged

// codes.rs
DimResized = "DIM-RESIZED" => Info,
```

## File structure

| File | Change | Task |
|---|---|---|
| `crates/engine/src/config.rs` | `MipFilter` variants, `MipEdge`, `ResizeMode`, `MipSpec::edge`, `Profile::resize` | 1 |
| `crates/engine/src/overrides.rs` | `FIELD_PATHS` += `mips.edge`, `resize` | 1 |
| `crates/engine/src/policy.rs` | `check_profile`: S-mips-2 `CFG-INVALID`; post-resize `DIM-*`, `DIM-RESIZED` | 1, 5 |
| `crates/engine/src/filter.rs` (new) | kernels, taps, `resample_rgb`, `filter_allowed` | 1 (`filter_allowed` only), 2 |
| `crates/engine/src/mips.rs` | `area_resample`, `effective_filter`, direct-from-base RGB branch | 3, 4 |
| `crates/engine/src/resize.rs` (new) | `target_dims`, `resize_work` | 4 |
| `crates/engine/src/ops.rs` | call `resize_work` as step 6; module doc | 4 |
| `crates/engine/src/codes.rs` | `DIM-RESIZED`; count test merged + 1 | 5 |
| `crates/engine/src/bake/plan.rs`, `bake/mod.rs`, `pipeline.rs` | post-resize dims for the shift; identity hash | 5 |
| `crates/engine/src/api.rs` | `job_bytes` extra bytes | 5 |
| `schema/*.json` | regenerated | 1, 5 |
| `crates/gui/src/model/fields.rs`, `ui/settings_panel.rs` | Filter/Edge rows, Resize field, `DIM-NPOT` button | 6 |
| `crates/engine/tests/golden.rs`, `crates/cli/tests/cli_convert.rs`, `crates/gui/tests/*.rs` | new cases | 6, 7 |
| `docs/DECISIONS.md`, the spec, the roadmap, `docs/gui.md` | Note S `S-mips-*`, spec rows | 8 |

---

### Task 1: Config fields `mips.filter` variants, `mips.edge`, `resize`, and the filter check

**Files:**
- Modify:
  - `crates/engine/src/config.rs` (enums, `MipSpec`, `Profile`, tests);
  - `crates/engine/src/overrides.rs` (`FIELD_PATHS`);
  - `crates/engine/src/policy.rs` (`check_profile`);
  - `crates/engine/src/lib.rs` (`pub mod filter;`);
  - every `Profile { .. }` and `MipSpec { .. }` struct literal the compiler reports (about 13; add `resize: ResizeMode::None` and `edge: MipEdge::Wrap`).
- Create: `crates/engine/src/filter.rs`, with `filter_allowed` only for now.
- Regenerate: `schema/*.json`.
- Test: `config.rs` unit tests, `crates/engine/tests/policy_checks.rs`, `crates/engine/tests/overrides.rs`, `crates/engine/src/manifest.rs` unit test.

**Interfaces:** produces `MipFilter::{Tent, Mitchell, Kaiser}`, `MipEdge`, `ResizeMode`, `MipSpec::edge`, `Profile::resize` and `filter::filter_allowed` (registry above).

`filter_allowed(p)`:
```rust
pub fn filter_allowed(p: &Profile) -> Result<(), &'static str> {
    use crate::ops::{domain_for, Domain};
    let f = p.mips.filter;
    if f == MipFilter::Box {
        return Ok(());
    }
    let domain = domain_for(p);
    if domain == Domain::Hdr {
        return if f == MipFilter::Tent { Ok(()) } else { Err("HDR mips use box or tent (spec §5.5)") };
    }
    if p.category != Category::Color || domain != Domain::LinearLight {
        return Err("tent, Mitchell and Kaiser are for sRGB colour only; this texture uses box (spec §5.5)");
    }
    if p.alpha_weighted_mips() {
        return Err("alpha-tested and blended colour uses the alpha-weighted box (spec §5.5)");
    }
    Ok(())
}
```
In `check_profile`, next to the other `CfgInvalid` checks: `if let Err(why) = filter::filter_allowed(p) { add(CfgInvalid, format!("mips.filter = {}: {why}", <id>)) }`. Get the id through serde (`serde_json::to_value(p.mips.filter)`), the way the other ids are produced.

- [ ] **Step 1: Write the failing tests.**

In `config.rs` `mod tests`:
```rust
#[test]
fn m6_mip_and_resize_ids_match_the_spec() {
    for (f, id) in [(MipFilter::Box, "box"), (MipFilter::Tent, "tent"), (MipFilter::Mitchell, "mitchell"), (MipFilter::Kaiser, "kaiser")] {
        assert_eq!(serde_json::to_value(f).unwrap(), id);
    }
    assert_eq!(serde_json::to_value(MipEdge::Clamp).unwrap(), "clamp");
    assert_eq!(serde_json::to_value(ResizeMode::PotDown).unwrap(), "pot-down");
    assert_eq!(serde_json::to_value(ResizeMode::None).unwrap(), "none");
}

#[test]
fn default_edge_and_resize_are_not_serialised() {
    // S-mips-9: profile_sha256 must not change for unchanged settings.
    let p = crate::presets::PresetRegistry::builtin().unwrap().profile("cs-quality", Target::Cs, Subtype::Diffuse).unwrap();
    assert_eq!((p.mips.edge, p.resize), (MipEdge::Wrap, ResizeMode::None));
    let v = serde_json::to_value(&p).unwrap();
    assert!(v.get("resize").is_none() && v["mips"].get("edge").is_none(), "{v}");
    let mut q = p.clone();
    q.resize = ResizeMode::PotDown;
    q.mips.edge = MipEdge::Clamp;
    let v = serde_json::to_value(&q).unwrap();
    assert_eq!((v["resize"].as_str(), v["mips"]["edge"].as_str()), (Some("pot-down"), Some("clamp")));
    assert_eq!(serde_json::from_value::<Profile>(v).unwrap(), q);
}
```
In `manifest.rs` `mod tests`: add `profile_hash_is_unchanged_by_the_m6_fields`. It serialises the cs-quality `diffuse` profile and a JSON copy with `"resize":"none"` and `"mips.edge":"wrap"` inserted, deserialises the copy, and asserts `profile_sha256` of both are equal.

In `crates/engine/tests/overrides.rs`:
```rust
#[test]
fn resize_and_edge_are_settable_fields() {
    let s = parse_set("resize=pot-down").unwrap();
    assert_eq!((s.field.as_str(), s.value.as_str()), ("resize", Some("pot-down")));
    let s = parse_set("file:odd.png.resize=pot-down").unwrap();
    assert_eq!(s.field, "resize");
    assert_eq!(parse_set("subtype:diffuse.mips.edge=clamp").unwrap().field, "mips.edge");
}
```
In `crates/engine/tests/policy_checks.rs`, one test per row of S-mips-2. Use the file's existing `profile`/`check` helpers; read them first.
- `kaiser_on_srgb_diffuse_without_alpha_is_allowed`: `diffuse`, `alpha.policy = remove`, `mips.filter = kaiser` → no `CfgInvalid`. The same for `mitchell` and `tent`, and with `alpha.policy = data`.
- `non_box_filters_are_cfg_invalid_outside_srgb_colour`: for each of `normal`, `pbr_normal`, `msn`, `flow`, `rmaos`, `generic_data`, `height`, `envmask`, with each of `tent`, `mitchell`, `kaiser` → `CfgInvalid`, whose message contains `"sRGB colour only"`.
- `alpha_weighted_colour_keeps_box`: `diffuse` with `cutout`, `blend` or `cutout_blend` and `kaiser` → `CfgInvalid` containing `"alpha-weighted box"`.
- `hdr_allows_tent_only`: `pbr_emissive` with `format = bc6h_uf16` and `tent` → none; with `mitchell` → `CfgInvalid` containing `"box or tent"`.
- `linear_content_colour_keeps_box`: `diffuse` with `color.content = linear` and `kaiser` → `CfgInvalid`.

- [ ] **Step 2: Run them and confirm they fail** (`cargo test -p engine m6_ default_edge resize_and_edge non_box kaiser_on alpha_weighted_colour hdr_allows linear_content profile_hash_is_unchanged`). They fail to compile.
- [ ] **Step 3: Implement.** Add the enums, the fields (exact serde attributes from the registry), the `FIELD_PATHS` entries (`"mips.edge"` after `"mips.filter"`, `"resize"` after `"header"`), `filter.rs` with `filter_allowed` only, and the `check_profile` line. Fix the struct literals the compiler lists. In `config.rs`, a `MipSpec` doc comment on `edge` says "read only by tent, Mitchell and Kaiser (spec §3.2)", and one on `resize` says "Preprocess step 6 (spec §5.4); `pot-down` = largest power of two ≤ each axis".
- [ ] **Step 4: Regenerate schemas** with `cargo run -p sdc -- presets schema all --out schema`. Check `git diff schema` and confirm only the new enum values and fields appear.
- [ ] **Step 5: Run the gates unpiped.** All existing golden and GUI tests pass unchanged (`crates/gui/tests/fields.rs` still checks that every GUI field is in `FIELD_PATHS`).
- [ ] **Step 6: Commit.**
```bash
git add crates/engine/src/config.rs crates/engine/src/overrides.rs crates/engine/src/policy.rs crates/engine/src/lib.rs crates/engine/src/filter.rs crates/engine/src/manifest.rs crates/engine/tests/overrides.rs crates/engine/tests/policy_checks.rs schema/ <each file whose struct literal you fixed>
git commit -m "feat(config): mips.filter tent/mitchell/kaiser, mips.edge and resize fields; non-box filters only on sRGB colour (S-mips-2)

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

---

### Task 2: Kernels and the polyphase resample (`filter.rs`)

**Files:**
- Modify: `crates/engine/src/filter.rs`.
- Test: `filter.rs` `mod tests`. The f64 reference is test-only.

**Interfaces:** produces `radius`, `kernel`, `axis_taps` and `resample_rgb` (registry). Consumed by Tasks 3 and 4.

**Definitions** (mirroring `mipcheck/scripts/b8.py`; x is in destination texels):
- tent `max(0, 1 − |x|)`, radius 1.
- Mitchell B = C = 1/3, radius 2, the standard piecewise cubic (b8.py `mitchell`).
- Kaiser `sinc(x·s) · I0(α·√(1 − (x/w)²)) / I0(α)` for |x| < w, else 0; `sinc(t) = sin(πt)/(πt)`, `sinc(0) = 1`; w = 3, α = 4, s = 1. `I0(z) = Σ_{k=0}^{24} ((z/2)^k / k!)²`.
- `axis_taps(n, m, f, edge)`: let `s = n as f64 / m as f64`. For destination texel `i`, `centre = (i + 0.5)·s`, and `reach = ceil(radius·s) + 1`. For every integer `j` in `floor(centre − reach) ..= ceil(centre + reach)`, compute `w = kernel(f, ((j + 0.5) − centre)/s)`, skip it if 0, and map `j` to a source index with `wrap = j.rem_euclid(n)` or `clamp = j.clamp(0, n−1)`. Merge duplicate indices by summing their weights, then divide by the row sum. If the row sum is 0 (it cannot be for these kernels, but guard it), fall back to `[(min(i·n/m, n−1), 1.0)]`. When `n == m` the result is the identity: `[(i, 1.0)]` for each `i`.
- `resample_rgb(src, (w, h), f, edge)`: build `tx = axis_taps(src.width, w, ..)` and `ty = axis_taps(src.height, h, ..)`. The horizontal pass makes an intermediate of `src.height × w` RGB in f32, accumulated in f64 per tap row and parallel over rows with `rayon::prelude::par_chunks_mut`. The vertical pass writes `h × w`, also parallel over rows. Alpha of the result is 0.0. No clamping here; `finalize` and the resize clamp later.

- [ ] **Step 1: Write the failing tests** in `filter.rs` `mod tests`:
```rust
#[test]
fn kernel_values_at_known_points() {
    let close = |a: f64, b: f64| (a - b).abs() < 1e-12;
    assert!(close(kernel(MipFilter::Tent, 0.5), 0.5) && close(kernel(MipFilter::Tent, 1.0), 0.0));
    assert!(close(kernel(MipFilter::Mitchell, 0.0), 8.0 / 9.0));
    assert!(close(kernel(MipFilter::Mitchell, 1.0), 1.0 / 18.0));
    assert!(close(kernel(MipFilter::Mitchell, 2.0), 0.0));
    assert!(close(kernel(MipFilter::Kaiser, 0.0), 1.0));
    assert!(kernel(MipFilter::Kaiser, 1.0).abs() < 1e-12 && kernel(MipFilter::Kaiser, 2.0).abs() < 1e-12);
    assert_eq!(kernel(MipFilter::Kaiser, 3.0), 0.0);
    assert!(kernel(MipFilter::Kaiser, 1.5) < 0.0); // a negative lobe
}

#[test]
fn every_tap_row_sums_to_one() {
    for f in [MipFilter::Tent, MipFilter::Mitchell, MipFilter::Kaiser] {
        for edge in [MipEdge::Wrap, MipEdge::Clamp] {
            for (n, m) in [(4u32, 2u32), (4, 1), (8, 1), (16, 4), (1000, 512), (600, 512), (5, 2), (3, 1)] {
                for row in axis_taps(n, m, f, edge) {
                    let s: f64 = row.iter().map(|t| t.1).sum();
                    assert!((s - 1.0).abs() < 1e-12, "{f:?} {edge:?} {n}->{m}: {s}");
                    assert!(row.iter().all(|t| t.0 < n));
                }
            }
        }
    }
}

#[test]
fn tent_edges_wrap_and_clamp_differ_as_computed_by_hand() {
    // 4 -> 2, input [1, 0, 0, 0]. Taps of each output: 1/8, 3/8, 3/8, 1/8 at j-1..j+2 around centre 2i+1.
    let mut img = Rgba32F::new(4, 1, [0.0; 4]);
    img.px[0][0] = 1.0;
    let w = resample_rgb(&img, (2, 1), MipFilter::Tent, MipEdge::Wrap);
    let c = resample_rgb(&img, (2, 1), MipFilter::Tent, MipEdge::Clamp);
    let r = |i: &Rgba32F| [i.px[0][0], i.px[1][0]];
    assert!((r(&w)[0] - 0.375).abs() < 1e-6 && (r(&w)[1] - 0.125).abs() < 1e-6, "{:?}", r(&w));
    assert!((r(&c)[0] - 0.5).abs() < 1e-6 && r(&c)[1].abs() < 1e-6, "{:?}", r(&c));
}

#[test]
fn a_constant_image_stays_constant() {
    let img = Rgba32F::new(12, 7, [0.25, 0.5, 0.75, 1.0]);
    for f in [MipFilter::Tent, MipFilter::Mitchell, MipFilter::Kaiser] {
        for edge in [MipEdge::Wrap, MipEdge::Clamp] {
            let o = resample_rgb(&img, (4, 2), f, edge);
            assert!(o.px.iter().all(|p| (p[0] - 0.25).abs() < 1e-6 && (p[1] - 0.5).abs() < 1e-6 && (p[2] - 0.75).abs() < 1e-6 && p[3] == 0.0));
        }
    }
}

#[test]
fn resample_matches_an_f64_weight_matrix_reference() {
    // b8.py `weight_matrix` + einsum, restated in f64 (test-only), for an LCG-random 16x8 image, factor 4, wrap.
    // Assert max |sdc - reference| < 1e-5 on every RGB sample, for each of tent, Mitchell, Kaiser.
}

#[test]
fn identity_size_is_a_copy() {
    let mut img = Rgba32F::new(3, 2, [0.0; 4]);
    for (i, p) in img.px.iter_mut().enumerate() { p[0] = i as f32; }
    let o = resample_rgb(&img, (3, 2), MipFilter::Kaiser, MipEdge::Wrap);
    assert!(o.px.iter().zip(&img.px).all(|(a, b)| a[0] == b[0]));
}
```
Write the body of `resample_matches_an_f64_weight_matrix_reference` in full when you implement it: a `fn reference(img, factor, f) -> Vec<[f64; 3]>` builds the dense `n_out × n_in` matrix exactly as `b8.py` does (`reach = ceil(radius·factor) + 1`, wrap with `rem_euclid`, rows normalised) and applies `Wy · img · Wxᵀ`.
- [ ] **Step 2: Run them and confirm they fail** (`cargo test -p engine filter::`).
- [ ] **Step 3: Implement** to the definitions above. Doc comments must cite RESULTS-2 B8 and S-mips-4.
- [ ] **Step 4: Run the gates unpiped.**
- [ ] **Step 5: Commit.**
```bash
git add crates/engine/src/filter.rs
git commit -m "feat(filter): tent, Mitchell (1/3,1/3) and Kaiser (w3 a4 s1) polyphase resample with wrap/clamp edges (B8)

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

---

### Task 3: Mips: tent/Mitchell/Kaiser levels built directly from mip 0 (B8)

**Files:**
- Modify: `crates/engine/src/mips.rs` (module doc, `effective_filter`, `build_chain_with`, tests).
- Test: `mips.rs` `mod tests`.

**Interfaces:**
- Consumes: `filter::{filter_allowed, resample_rgb}`.
- Produces: `pub(crate) fn effective_filter(work: &Work, p: &Profile) -> MipFilter` (Box unless `filter_allowed(p).is_ok() && !weighted_rgb(work, p)`). `build_chain` and `build_chain_with` keep their signatures.

**How it composes** (state these in the module doc):
- **Box** (every default): the code path is unchanged. Nothing new runs.
- **Non-box:** the existing loop runs unchanged, so `acc`, alpha, the roughness bake and the coat bake are exactly as before. After `let mut fin = finalize(&next, work);` and **before** the bakes, when `effective_filter != Box`, overwrite RGB:
  ```rust
  let direct = crate::filter::resample_rgb(&out[0], (next.width, next.height), filt, p.mips.edge);
  for (px, d) in fin.px.iter_mut().zip(&direct.px) { px[..3].copy_from_slice(&d[..3]); }
  // then re-apply the domain clamp of `finalize` to RGB (LinearLight: [0, 1]; Hdr: max 0)
  ```
  Factor the clamp out of `finalize` as `fn clamp_rgb(px: &mut [f32; 4], domain: Domain)` so both use it. `out[0]` is the colour base (`base_level` returns it unchanged for non-vector domains).
- **Coverage:** `preserve_coverage` reads only alpha, which is box. A non-box filter is never allowed with `cutout`/`cutout_blend` (S-mips-2), so coverage never meets it.
- **Normal renormalisation per level, `_cnr` self-bake, `LengthPyramid`, roughness r²:** all are box-only contents (S-mips-2). `effective_filter` returns Box for them even if a blocked profile reached the chain.
- **Alpha-weighted RGB:** `effective_filter` returns Box when `weighted_rgb` is true.
- **Odd sizes** (an acknowledged `DIM-NPOT` without resize): the level size is the box chain's `next` size, and `axis_taps` takes any n → m.

- [ ] **Step 1: Write the failing tests** in `mips.rs` `mod tests`. Use `profile(Subtype::Diffuse)` with `alpha.policy = Remove` unless stated, and `rnd_image(w, h, seed)`, a test helper with an LCG like `area_path_agrees_with_pair_path_on_even_sizes`.
```rust
#[test]
fn box_is_the_default_and_unchanged() {
    assert_eq!(profile(Subtype::Diffuse).mips.filter, MipFilter::Box);
    // The pre-M6 chain is restated by the existing tests; this asserts the non-box branch is not taken.
    let w = work(rnd_image(8, 8, 1), Domain::LinearLight, false);
    let p = profile(Subtype::Diffuse);
    assert_eq!(effective_filter(&w, &p), MipFilter::Box);
}

#[test]
fn non_box_levels_are_resampled_directly_from_mip_0() {
    for f in [MipFilter::Tent, MipFilter::Mitchell, MipFilter::Kaiser] {
        let mut p = profile(Subtype::Diffuse);
        p.alpha.policy = AlphaPolicy::Remove;
        p.mips.filter = f;
        let img = rnd_image(16, 16, 7);
        let c = build_chain(&work(img.clone(), Domain::LinearLight, false), &p);
        for (k, level) in c.iter().enumerate().skip(1) {
            let d = crate::filter::resample_rgb(&img, (level.width, level.height), f, MipEdge::Wrap);
            for (a, b) in level.px.iter().zip(&d.px) {
                for ch in 0..3 { assert!((a[ch] - b[ch].clamp(0.0, 1.0)).abs() < 1e-6, "{f:?} level {k}"); }
            }
        }
        // Not recursive: resampling level 1 again gives a different level 2.
        let rec = crate::filter::resample_rgb(&c[1], (c[2].width, c[2].height), f, MipEdge::Wrap);
        let diff = c[2].px.iter().zip(&rec.px).map(|(a, b)| (a[0] - b[0]).abs()).fold(0.0f32, f32::max);
        assert!(diff > 1e-4, "{f:?}: {diff}");
    }
}

#[test]
fn non_box_alpha_stays_the_box_alpha_bit_for_bit() {
    let img = rnd_image(16, 16, 3); // random alpha
    let mut bx = profile(Subtype::Diffuse);
    bx.alpha.policy = AlphaPolicy::Data;
    let mut k = bx.clone();
    k.mips.filter = MipFilter::Kaiser;
    let w = work(img, Domain::LinearLight, true);
    let (a, b) = (build_chain(&w, &bx), build_chain(&w, &k));
    for (la, lb) in a.iter().zip(&b) {
        assert!(la.px.iter().zip(&lb.px).all(|(x, y)| x[3] == y[3]));
    }
    assert_ne!(a[1].px, b[1].px); // RGB did change
}

#[test]
fn kaiser_overshoot_is_clamped_in_linear_light() {
    // A one-texel white spike on black: Kaiser rings below 0 next to it at level 1.
    let mut img = Rgba32F::new(16, 16, [0.0, 0.0, 0.0, 1.0]);
    img.at_mut(8, 8)[..3].copy_from_slice(&[1.0, 1.0, 1.0]);
    let mut p = profile(Subtype::Diffuse);
    p.alpha.policy = AlphaPolicy::Remove;
    p.mips.filter = MipFilter::Kaiser;
    let c = build_chain(&work(img.clone(), Domain::LinearLight, false), &p);
    let raw = crate::filter::resample_rgb(&img, (8, 8), MipFilter::Kaiser, MipEdge::Wrap);
    assert!(raw.px.iter().any(|q| q[0] < 0.0), "the fixture must ring");
    assert!(c.iter().skip(1).all(|l| l.px.iter().all(|q| (0.0..=1.0).contains(&q[0]))));
}

#[test]
fn hdr_tent_keeps_highlights_and_never_goes_negative() {
    let mut img = Rgba32F::new(8, 8, [0.1, 0.1, 0.1, 1.0]);
    img.at_mut(3, 3)[0] = 100.0;
    let mut p = profile(Subtype::PbrEmissive);
    p.format = Format::Bc6hUf16;
    p.mips.filter = MipFilter::Tent;
    let c = build_chain(&work(img, Domain::Hdr, false), &p);
    assert!(c[1].px.iter().any(|q| q[0] > 1.0));
    assert!(c.iter().all(|l| l.px.iter().all(|q| q[0] >= 0.0)));
}

#[test]
fn box_only_contents_ignore_a_non_box_filter() {
    // A blocked profile (CFG-INVALID) never runs, but the chain itself must still be the box chain.
    for (st, dom) in [(Subtype::Normal, Domain::Normal), (Subtype::Msn, Domain::Msn), (Subtype::Flow, Domain::SignedRg), (Subtype::GenericData, Domain::Stored)] {
        let img = rnd_image(8, 8, 5);
        let p = profile(st);
        let mut k = p.clone();
        k.mips.filter = MipFilter::Kaiser;
        let w = work(img, dom, true);
        assert_eq!(build_chain(&w, &p), build_chain(&w, &k), "{st:?}");
    }
    // Alpha-weighted colour.
    let mut p = profile(Subtype::Diffuse);
    p.alpha.policy = AlphaPolicy::Cutout;
    let mut k = p.clone();
    k.mips.filter = MipFilter::Kaiser;
    let w = work(rnd_image(8, 8, 9), Domain::LinearLight, true);
    assert_eq!(build_chain(&w, &p), build_chain(&w, &k));
}

#[test]
fn baked_roughness_is_identical_under_any_filter_setting() {
    // A roughness member is box-only: build_chain_with + a NormalVariance gives equal chains for box and kaiser.
    // Reuse the bake fixture of `bake::tests` (read it first); assert_eq!(chain_box, chain_kaiser).
}

#[test]
fn thin_lines_are_exact_for_box_and_tent() {
    // B8 "Thin lines: box and tent are exact either way": a 1-texel line every 4 columns of a 16x16 image; level 2
    // (4x4) is 0.25 everywhere for box and for tent (wrap).
}
```
Write the bodies of the last two tests in full at implementation time, to the comment in each.
- [ ] **Step 2: Run them and confirm they fail** (`cargo test -p engine mips::`).
- [ ] **Step 3: Implement** `effective_filter`, `clamp_rgb` and the branch. Rewrite the first line of the module doc ("Box filter only in v1 Phase 1; other kernels arrive in milestone M6") to describe S-mips-3 and the composition list above.
- [ ] **Step 4: Run the gates unpiped.** Every existing `mips.rs`, bake and golden test passes unchanged.
- [ ] **Step 5: Commit.**
```bash
git add crates/engine/src/mips.rs
git commit -m "feat(mips): tent/Mitchell/Kaiser RGB levels built directly from mip 0, alpha stays box (B8, S-mips-3)

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

---

### Task 4: Resize `pot-down` in Preprocess (step 6)

**Files:**
- Create: `crates/engine/src/resize.rs` (and add `pub mod resize;` to `lib.rs`).
- Modify:
  - `crates/engine/src/mips.rs`: generalise `downsample_area` into `area_resample(src, dims, weighted)`; `downsample_area(prev, w)` becomes a one-line call with `((w/2).max(1), (h/2).max(1))`.
  - `crates/engine/src/ops.rs`: `preprocess` ends with `crate::resize::resize_work(&mut work, p)`; update the module doc ("Resize (step 6) is milestone M6; NPOT is blocked by `DIM-NPOT`").
- Test: `resize.rs` `mod tests`, `ops.rs` tests.

**Interfaces:** produces `resize::target_dims` and `resize::resize_work` (registry). `preprocess` keeps its signature. `BakeJob::load_lengths` calls `preprocess`, so a resized `_n` reaches the bake by itself (Task 5 wires the dims).

`target_dims((w, h), PotDown)` is `(prev_pow2(w), prev_pow2(h))` with `prev_pow2(v) = 1 << (31 - v.leading_zeros())` for v ≥ 1. `None` → unchanged.

`resize_work(work, p)`:
1. Let `to = target_dims(..)`. Return if `to` equals the current size.
2. `weighted = weighted_rgb(work, p)` (make it `pub(crate)` in `mips.rs`), and `filt = mips::effective_filter(work, p)`.
3. If `work.roughness_r_squared`, square R in place.
4. `let area = mips::area_resample(&work.img, to, weighted);` (this carries the alpha, and the RGB for box).
5. If `filt != Box`, overwrite RGB from `filter::resample_rgb(&work.img, to, filt, p.mips.edge)`. `filt` is never non-box while `weighted`.
6. If roughness, take √ of `max(R, 0)`. Clamp RGB by domain: LinearLight to [0, 1], Hdr to ≥ 0. Normal, Msn and SignedRg are left alone; renormalisation is `base_level`'s job under `normal_base = renormalize`.
7. `work.img = area`.

- [ ] **Step 1: Write the failing tests.**

In `resize.rs`:
```rust
#[test]
fn pot_down_takes_the_largest_power_of_two_per_axis() {
    let t = |w, h| target_dims((w, h), ResizeMode::PotDown);
    assert_eq!(t(1000, 600), (512, 512));
    assert_eq!(t(1536, 1024), (1024, 1024));
    assert_eq!(t(48, 40), (32, 32));
    assert_eq!(t(1024, 3), (1024, 2));
    assert_eq!(t(1, 1), (1, 1));
    assert_eq!(target_dims((1000, 600), ResizeMode::None), (1000, 600));
}

#[test]
fn box_resize_is_area_exact() {
    // 6x1 -> 4x1 (pot-down), data content: each output covers 1.5 source texels.
    // Source R = [0, 0, 1, 1, 0, 0] -> [0, 2/3, 2/3, 0].
}

#[test]
fn power_of_two_sources_are_untouched_bit_for_bit() {
    // 8x8 random diffuse with resize = pot-down: preprocess output equals preprocess with resize = none.
}

#[test]
fn alpha_is_area_box_even_with_kaiser() {
    // 12x12 data-alpha diffuse, kaiser: resized alpha == area_resample(.., (8, 8), false) alpha exactly.
}

#[test]
fn roughness_is_resized_on_r_squared() {
    // rmaos 3x1, R = [0, 1, 0] -> 2x1: each output covers 1.5 texels; mean r^2 = (0.5*0 + 1*1... ) restate in f64,
    // assert R = sqrt(area mean of r^2), not the area mean of r.
}

#[test]
fn weighted_colour_resize_ignores_invisible_colour() {
    // cutout diffuse 3x1: [red a=1, green a=0, blue a=1] -> 2x1: output 0 has no green (weight area*alpha).
}

#[test]
fn resized_normals_are_unit_length_at_mip_0_by_default() {
    // 6x6 bumpy normal source, resize = pot-down: build_chain(preprocess(..))[0] has |n| = 1 +- 1e-5 and is 4x4.
}
```
Write each body in full to its comment. Make the arithmetic exact by hand in the test, the way `odd_width_alpha_weighted_rgb_uses_area_times_alpha` does.
In `ops.rs` add `preprocess_resizes_last_after_srgb_decode`: a 3×1 sRGB diffuse `[1.0, 0.0, 1.0]` (stored) → 2×1. Assert the linear-light average, `(1·1 + 0.5·0)/1.5` and `(0.5·0 + 1·1)/1.5` = 2/3 per output, not the stored-value average decoded afterwards.
- [ ] **Step 2: Run them and confirm they fail** (`cargo test -p engine resize:: ops::`).
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Run the gates unpiped.** All existing odd-size tests (`odd_width_*`, `area_path_agrees_with_pair_path_on_even_sizes`) pass unchanged.
- [ ] **Step 5: Commit.**
```bash
git add crates/engine/src/resize.rs crates/engine/src/lib.rs crates/engine/src/mips.rs crates/engine/src/ops.rs
git commit -m "feat(resize): resize = pot-down in Preprocess step 6, in the content's domain with the mip filter (S-mips-6)

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

---

### Task 5: Resize in planning: `DIM-*` on the post-resize size, `DIM-RESIZED`, the bake shift, admission

**Files:**
- Modify:
  - `crates/engine/src/codes.rs` (`DimResized = "DIM-RESIZED" => Info`; count test `M + 1`; module doc);
  - `crates/engine/src/policy.rs` (the `!a.is_pot` block, `DimSmall`, `DimFlowSmall`);
  - `crates/engine/src/bake/plan.rs` (`ndims`, `mdims` through `target_dims` with each job's `profile.resize`);
  - `crates/engine/src/pipeline.rs` (`NormalVariance::new(l, target_dims((img.width, img.height), p.resize), ..)`);
  - `crates/engine/src/bake/mod.rs` (`identity_sha256`);
  - `crates/engine/src/api.rs` (`job_bytes`).
- Regenerate: `schema/*.json` (the event schema lists codes).
- Test: `crates/engine/tests/policy_checks.rs`, `crates/engine/tests/bake_plan.rs`, `crates/engine/tests/bake_run.rs`, `api.rs` unit tests, `codes.rs`.

**Interfaces:**
- Consumes: `resize::target_dims`.
- Produces: `Code::DimResized`. `BakeJob::identity_sha256` is unchanged for `resize = none`.

Policy block (replace the `!a.is_pot` and `DimSmall`/`DimFlowSmall` lines):
```rust
let (ow, oh) = crate::resize::target_dims((a.width, a.height), p.resize);
if (ow, oh) != (a.width, a.height) {
    w.push(Warning::new(DimResized, format!("{}x{} resized to {ow}x{oh} (resize = pot-down)", a.width, a.height))
        .with_data(json!({ "from": [a.width, a.height], "to": [ow, oh] })));
}
if !(ow.is_power_of_two() && oh.is_power_of_two()) { add(DimNpot, format!("{ow}x{oh} is not a power of two")); }
if ow.min(oh) < 4 { add(DimSmall, "base dimension below 4 px".into()); }
if st == Subtype::Flow && ow.min(oh) <= 32 { add(DimFlowSmall, "CS ignores _flow maps of 32 px or less".into()); }
```
(Keep the existing `DimNpot` message text for `resize = none`; it reads the same.) Read how `w` and `add` are built in `check_profile` first and match them.

`identity_sha256`: hash the existing 4-tuple when `normal_profile.resize == ResizeMode::None`, else the 5-tuple with `resize` appended.

`job_bytes`: `+ w·h·8` when `job.profile.mips.filter != Box`, and `+ w·h·32` when `target_dims(..) != (w, h)`, with `(w, h)` from `image::image_dimensions(&job.input)` (0 if unreadable). Doc-comment S-mips-11.

- [ ] **Step 1: Write the failing tests.**
  - `policy_checks.rs`:
    - `pot_down_turns_dim_npot_into_dim_resized`: a 48×40 diffuse with `resize = pot-down` has no `DimNpot` and has a `DimResized` warning (Info) with data `from [48,40]`, `to [32,32]`; the same file with `resize = none` has `DimNpot` (blocking) and no `DimResized`.
    - `dim_small_tests_the_resized_size`: 7×5 with pot-down → 4×4, no `DimSmall`; 7×3 → 4×2, `DimSmall`.
    - `flow_small_tests_the_resized_size`: a 40×40 `_flow` with pot-down → 32×32 → `DimFlowSmall`.
  - `codes.rs`: the count test asserts `M + 1` (write the literal number after reading the merged count), and `DIM-RESIZED` is Info in `severities_follow_spec_section_10`.
  - `bake_plan.rs`, `resized_npot_pair_bakes_on_post_resize_sizes`: `_n` 48×48 and `_rmaos` 48×48 with `--set resize=pot-down` → `BakeJob.shift == 0`, no `SET-BAKE-RATIO`. Without the set → `SET-BAKE-RATIO` (unchanged behaviour).
  - `bake_run.rs`, `a_resized_pair_bakes_from_the_resized_normal`: run the pair above. The member's level-1 R equals the f64 reference computed from the **resized, renormalised** `_n` (reuse the file's reference helper; resize the `_n` in the test with `resize::resize_work` on a `preprocess` result).
  - `bake/mod.rs` unit test, `identity_is_unchanged_without_resize`: the hash of a `BakeJob` with `resize = none` equals the 4-tuple hash computed in the test, and `pot-down` differs.
  - `api.rs` unit test, `admission_counts_the_filter_and_resize_intermediates`: on a 48×40 PNG written to a temp dir, `job_bytes` with box/none equals `input_bytes(48, 40)`; with kaiser it adds `48·40·8`; with pot-down it adds `48·40·32`.
- [ ] **Step 2: Run them and confirm they fail.**
- [ ] **Step 3: Implement**, then regenerate schemas.
- [ ] **Step 4: Run the gates unpiped.** Existing tests that acknowledge `DIM-NPOT` (`file_scopes.rs`, `project_files.rs`, `gui_repro.rs`, the GUI tests) pass unchanged, because `resize` defaults to `none`.
- [ ] **Step 5: Commit.**
```bash
git add crates/engine/src/codes.rs crates/engine/src/policy.rs crates/engine/src/bake/plan.rs crates/engine/src/bake/mod.rs crates/engine/src/pipeline.rs crates/engine/src/api.rs crates/engine/tests/policy_checks.rs crates/engine/tests/bake_plan.rs crates/engine/tests/bake_run.rs schema/
git commit -m "feat(policy): DIM-* on the post-resize size, DIM-RESIZED, bake shift and admission after resize (S-mips-7, S-mips-11)

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

---

### Task 6: GUI: live Filter dropdown (basic panel), Edge row, Resize field, `DIM-NPOT` resize button

**Files:**
- Modify:
  - `crates/gui/src/model/fields.rs`: `FILTER_FIELD`, `EDGE_FIELD`, `ADVANCED_FIELDS` += `("resize", "Resize")`, `field_lock` cases, and `filter_option_lock`;
  - `crates/gui/src/ui/settings_panel.rs`: `mipmaps_rows`, `options_of` (`"mips.edge"`, `"resize"`) and `decision_box`.
- Test: `crates/gui/tests/fields.rs`, `crates/gui/tests/model.rs` (or `m3b_flow.rs`, wherever `decide(.., Acknowledge(DimNpot))` lives), `crates/gui/tests/ui_smoke.rs`.

**Interfaces:**
```rust
pub const FILTER_FIELD: (&str, &str) = ("mips.filter", "Filter");
pub const EDGE_FIELD: (&str, &str) = ("mips.edge", "Edge");
pub fn filter_view(target: Target, job: &ResolvedJob) -> FieldView;   // like levels_view
pub fn edge_view(target: Target, job: &ResolvedJob) -> FieldView;
/// Why `f` cannot be picked for this profile, or None (engine::filter::filter_allowed on a copy with `filter = f`).
pub fn filter_option_lock(p: &Profile, f: MipFilter) -> Option<&'static str>;
```
- `field_lock` adds `"mips.edge"` when `p.mips.filter == Box`, with "only tent, Mitchell and Kaiser read the edge". `"mips.filter"` gets no whole-row lock; each option is locked by `filter_option_lock`. If every non-box option is locked, the combo is still live but shows only Box enabled, and its hover gives the first reason.
- `mipmaps_rows`: replace the disabled combo (and its "Lanczos" hover) with a live `egui::ComboBox` labelled "Filter". It has 4 options; locked options use `ui.add_enabled(false, SelectableLabel)` with `on_disabled_hover_text(reason)`. A change pushes `Action::Set("mips.filter", json!(id))`. It shows the provenance and ↺ like the levels row. Then add the Edge row through `field_row` with `edge_view`. **The rows stay in the basic Mipmaps group**, and `mips.filter` is **not** added to `ADVANCED_FIELDS`.
- `options_of`: `"mips.edge"` → Wrap / Clamp; `"resize"` → `None` / "Down to a power of two" (`"pot-down"`).
- `decision_box`, in the `Decision::Acknowledge(code)` arm: when `*code == Code::DimNpot`, add a second button "Resize to a power of two" in the same `ui.horizontal`, pushing `Action::Set("resize", json!("pot-down"))`. Hover: "writes resize = pot-down for this texture; DIM-RESIZED reports the new size".

- [ ] **Step 1: Write the failing tests.**
  - `fields.rs`:
    - `filter_and_edge_stay_in_the_basic_mipmaps_group`: `group_of("mips.filter") == "Mipmaps"`, `group_of("mips.edge") == "Mipmaps"`, `!ADVANCED_FIELDS.iter().any(|f| f.0 == "mips.filter")`, and `ADVANCED_FIELDS` contains `"resize"`. Every one is in `FIELD_PATHS`.
    - `filter_options_lock_per_s_mips_2`: for an opaque `diffuse` profile, `filter_option_lock` is `None` for all four. For `normal`, Tent/Mitchell/Kaiser are `Some(..)` and Box is `None`. For `cutout` `diffuse`, the non-box options are locked with a reason containing "alpha-weighted". For a BC6H `pbr_emissive`, Tent is free and Mitchell/Kaiser are locked.
    - `edge_is_locked_while_the_filter_is_box`.
  - `model.rs`, `npot_resize_choice_clears_the_block`: using the support fixture's 48 px NPOT diffuse (`odd.png`), `m.set_field("landscape/odd.png", "resize", json!("pot-down"))` and replan. The row has no `DimNpot` decision, its warnings contain `DimResized`, and the `[files]` entry holds `resize = "pot-down"`. The existing `decide(.., Acknowledge(DimNpot))` tests stay unchanged.
  - `ui_smoke.rs`: extend the existing settings-panel smoke test to run one frame with a Kaiser profile selected (no panic).
- [ ] **Step 2: Run them and confirm they fail** (`cargo test -p sdc-gui`; use the crate name from `crates/gui/Cargo.toml`).
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Run the gates unpiped.**
- [ ] **Step 5: Commit.**
```bash
git add crates/gui/src/model/fields.rs crates/gui/src/ui/settings_panel.rs crates/gui/tests/fields.rs crates/gui/tests/model.rs crates/gui/tests/ui_smoke.rs
git commit -m "feat(gui): live Filter dropdown in the basic Mipmaps group, Edge row, Resize field and DIM-NPOT resize button (S-mips-10)

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

---

### Task 7: Golden and CLI cases (M6 exit for this track)

**Files:**
- Modify: `crates/engine/tests/golden.rs` and `crates/cli/tests/cli_convert.rs`.
- Test: the same files.

- [ ] **Step 1: Write the tests.** Each one passes once Tasks 1–6 are in. If one fails, debug it with superpowers:systematic-debugging. Do not loosen an assertion.
  - `golden.rs`:
    - `npot_diffuse_with_pot_down_converts_without_acknowledgement`: `put(dir, "d/odd.png", ..)` with a 48×40 RGB image built inline (`DynamicImage::ImageRgb8(RgbImage::from_fn(48, 40, ..))`). Run `convert(dir, Target::Cs, &["resize=pot-down"], &[])`. The output DDS is 32×32 with 6 mips, the summary has 0 blocked, and `warning_of(&o, "odd.png", Code::DimResized).data` has `to [32,32]`. Match the suffix argument to how `warning_of`'s existing callers pass it.
    - `kaiser_diffuse_differs_from_box_and_its_alpha_does_not`: a 64×64 diffuse with a data alpha ramp and a high-frequency checker RGB, converted twice (unscoped `mips.filter=kaiser` on a batch that holds only this file, and default). Do not use a `subtype:diffuse` scope: under `cs` the file may resolve as `pbr_albedo`, and the scoped set would silently do nothing. Assert that the planned sub-type is a colour sub-type with sRGB content. Decode `levels(..)`. Mip 0 is identical. Mips ≥ 1 differ in RGB on at least one texel. Alpha codes at every level are within the BC7 tolerance of each other (`assert_alpha_kept`-style, tol 2).
    - `kaiser_on_a_normal_map_is_cfg_invalid`: `--set mips.filter=kaiser` on a set with a `_n` makes the `_n` fail with `CfgInvalid`; the diffuse converts.
    - `default_profiles_hash_as_before`: for every built-in sub-type under `cs-quality`, `planned(..)["profile"]` has no `resize` key and no `mips.edge` key.
  - `cli_convert.rs`, `set_resize_pot_down_converts_an_npot_file`: `sdc convert <dir> --set file:odd.png.resize=pot-down` exits 0 and writes `odd.dds` at 32×32. Without the set it exits 1 (blocked), as today. Read the file's existing helpers first.
- [ ] **Step 2: Run the gates unpiped.** Every existing golden passes **unedited**.
- [ ] **Step 3: Commit.**
```bash
git add crates/engine/tests/golden.rs crates/cli/tests/cli_convert.rs
git commit -m "test(golden): pot-down resize, Kaiser diffuse, CFG-INVALID Kaiser normal, unchanged default hashes

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

---

### Task 8: Docs: DECISIONS Note S (`S-mips-*`), spec, roadmap, `docs/gui.md`; the version check

**Files:**
- Modify:
  - `docs/DECISIONS.md`;
  - `docs/superpowers/specs/2026-10-08-skyrim-dds-converter-design.md`;
  - `docs/superpowers/plans/2026-10-08-skyrim-dds-converter-phase0-2.md`;
  - `docs/gui.md`.

- [ ] **Step 1: DECISIONS.** Under the existing `## Note S` heading (created by Track B or A; if neither created one, add `## Note S — M6 decisions (owner and M6 plans, 2026-10-11)` after Note R), append a block **"M6 Track C: mip filters and resize"** with S-mips-1 … S-mips-11, one bullet each, as in the Rulings table. Then add the three owner questions under "Owner checks pending". Do not touch Note Q or any `S-shader-*`/other-track ids.
- [ ] **Step 2: Spec.**
  - §3.2: in the `mips.filter` row, Values becomes `box` / `tent` / `mitchell` / `kaiser` (the `{width,alpha,stretch}` syntax is deferred, S-mips-4), and Notes becomes "Tent/Mitchell/Kaiser for sRGB colour without alpha weighting; tent also for HDR; else `CFG-INVALID` (S-mips-2). Kaiser fixed at w3 α4 s1 (S-mips-4)". In the `mips.edge` row, drop "Not a `Profile` field yet". Add a `resize` row: `none` / `pot-down`, default `none`, S-mips-6.
  - §5.4 step 6: append "[Decided] (M6, S-mips-6)" with the per-axis rule.
  - §5.5: add "[Decided] (M6, S-mips-3): tent, Mitchell and Kaiser build each level directly from mip 0; box stays recursive; alpha always box".
  - §10: add a `DIM-RESIZED` (info) row; the `DIM-NPOT` remedy is now live.
  - §8: drop "(like `resize`)" from the DDS edge-case row, if Track B has not already rewritten it.
- [ ] **Step 3: Roadmap.** In "M6 — Presets completion and remaining inputs", mark done (strike through or "Done (Track C)") the scope items "NPOT resize policy" and "tent/Mitchell/Kaiser filters and `mips.edge`", and the carried items "GUI filter dropdown", "Tent, Mitchell and Kaiser mips" and the `resize` half of "`FIELD_PATHS` lacks …". Annotate "16K peak memory" with "admission counts the new intermediates (S-mips-11); scheduler revisit still open".
- [ ] **Step 4: `docs/gui.md`.** Describe the Filter and Edge rows in the basic Mipmaps group, Resize in Advanced, and the `DIM-NPOT` button.
- [ ] **Step 5: Version check (S-mips-9).** Confirm that the golden suite passed unedited in Task 7 and that `default_profiles_hash_as_before` holds. Then **do not** bump `Cargo.toml` `version` or `sdc-base`. If either check failed, stop and report `DONE_WITH_CONCERNS`; a needed bump goes to the next minor after main's version.
- [ ] **Step 6: Run the gates unpiped**, then commit.
```bash
git add docs/DECISIONS.md docs/superpowers/specs/2026-10-08-skyrim-dds-converter-design.md docs/superpowers/plans/2026-10-08-skyrim-dds-converter-phase0-2.md docs/gui.md
git commit -m "docs: Note S S-mips-1..11, spec rows for mips.filter/edge/resize and DIM-RESIZED, roadmap M6 Track C done

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

## Deferred (listed in Note S)

- Exposing the Kaiser width, alpha and stretch (owner question 1).
- Alpha-weighted tent for foliage (owner question 2).
- `resize` upscaling (`pot-up`, `pot-nearest`) and a size cap (spec §5.4 "size cap").
- Matching the resized mip 0's coverage to the source (S-mips-8).
- Resizing pack sources of different sizes to a common size (`PACK-SIZE-MISMATCH` still errors).
- The 16K box peak and the single-threaded Preprocess `powf` (scheduler revisit, S-mips-11).
