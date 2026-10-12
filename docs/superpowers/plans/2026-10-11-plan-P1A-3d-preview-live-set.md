# P1-A Live Set Preview Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Selecting a PBR set in sdc's Textures view shows a real game mesh that uses the set's diffuse, patched in memory exactly as PGPatcher 1.2.0 would patch it and drawn with the user's installed Community Shaders 1.9 `Lighting.hlsl`; material slider edits show on the next frame.

**Architecture:** A new pure crate `pgpsim` simulates PGPatcher's TruePBR patch (entry JSON + source material → patched `nif::material::MaterialState`). `render3d` is restructured around stable `ShapeId`s and owned `MaterialState`s, owns the CS load-time remap (`PBRFlags`, `PBRParams`, `MultiLayerParallaxData`), and classifies an edit as cbuffer / texture / permutation. The GUI adds a right-hand panel in the Textures view with its own viewport, per-slot preview encodes served from memory, a scan-DB mesh query, and a per-frame fingerprint that turns set-editor edits into material updates or reloads.

**Tech Stack:** Rust workspace (edition per workspace), D3D11 via `windows` crate (WARP in tests), D3DCompile, egui/eframe + egui_kittest, rusqlite (modscan), serde_json.

**Spec:** `F:\ai\dds\nifparseref\plan\p1\00-p1-spec.md` (approved 2026-10-11; D1–D5 recommendations accepted; shared-file edits approved). Rule sources it cites: `01-pgpatcher-truepbr-rules.md`, `02-set-editor-integration.md`, `03-renderer-p1-design.md` (same folder). Interface contract the parts were written against: `plan-parts\00-contract.md`.

## Global Constraints

- Work only in worktree `F:\ai\dds\converter-nif-p1`, branch `nif/b-p1` (from `nif/main @ 1d357be`). Read `F:\ai\dds\nifparseref\plan\CONCURRENCY.md` first. Never touch `F:\ai\dds\converter`. Never push. Never merge into `main`.
- Off-limits (read pub items only): `crates/engine/`, `crates/cli/`, `crates/encoders-sys/`, `crates/gui/src/pgp/`, `crates/gui/src/sets_doc.rs`, `crates/gui/src/model/sets.rs`, `crates/gui/src/model/pbr_export.rs`, `schema/`, `docs/DECISIONS.md`, golden and `gui_repro` tests.
- Shared files (`Cargo.toml`, `Cargo.lock`, `crates/gui/Cargo.toml`, `crates/gui/src/ui/mod.rs`, `crates/gui/src/model/mod.rs`, `crates/gui/src/app.rs`, `crates/gui/src/settings.rs`): add-only, minimal, each in its own small commit as the tasks specify.
- Gates before every commit: `cargo fmt --all && cargo clippy --workspace --all-targets -- -D warnings && cargo test --workspace`.
- Commit messages end with `Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>`.
- Owner data (`C:\_mount\moddingvol\Acherus`, the CS install) is read-only. Tests that need it are `#[ignore]` and gated on env `SDC_ACHERUS` (path of the MO2 instance).
- PGPatcher semantics = version 1.2.0 (01 tags [Src 1.2.0]).
- D1: PBR emissive colour = NIF `emissive_color × emissive_multiple` (linearised when Linear Lighting is on), commented "approximate, spec D1, capture C1 pending". D3: sets that can't be simulated (non-CS target or non-PBR set) show plain textures plus a notice. D4: single CS features toggle + plain list of features off in the preview. D5: preview encode cap 2048 px (`gui.toml [render3d] set_preview_max`, 0 = 2048).
- P1-A limits (by design, P1-B fixes): permutation edits compile synchronously; a re-encoded texture reloads the model (no hot-swap); no parent NiNode transforms.
- The panel says, in plain words: "Assumes your sdc JSON wins over other mods' PGPatcher configs."

## Review Focus

1. **A shader compile failure never blanks the panel:** the old draw stays, status `Failed(error)`, other shapes unaffected. Pinned by Task 8b `failing_permutation_keeps_old_draw`.
2. **A user-imported JSON with a wrong-typed key** (`"emissive": "yes"`) gives `Skip` naming the key, never a panic. Pinned by Task 4 `wrong_json_type_is_a_skip`.
3. **A shape that keeps a vanilla slot packed only in a BSA** is not reported as "PGPatcher will skip": `SetPreview::exists` answers for BSA winners. Pinned by Task 14's BSA-only `exists` test; the converse (texture truly missing → `Skip`) by Task 4 `missing_untouched_source_slot_is_a_skip`.
4. **Existing GUI tests and layout are unchanged:** `SetPreview::default()` is disabled and does no IO, so kittests never open the developer's real scan DB. Pinned by Task 16 (full `cargo test --workspace` with existing kittests untouched).
5. **An unchanged frame uploads nothing, yet pixels equal the by-name reference writer** after camera, light, edit and resize. Pinned by Task 10.

## Task order and dependencies

Tasks run in this order: **1 → 2 → 3 → 4 → 5 → 6 → 7 → 8a → 8b → 9 → 10 → 11 → 12 → 13 → 14 → 15 → 16 → 17**.
- 1 (`nif::material`) is needed by everything. 2–5 (pgpsim) need only 1. 6–10 (render3d) need 1. 11 (modscan) is independent. 12–17 (gui) need all earlier tasks.
- Where a task edits a file an earlier task already changed, edits are given as exact old → new text anchors, not line numbers.
- **Task 8a replaces `crates/render3d/src/lib.rs` wholesale.** Tasks 8b, 9 and 10 give anchors against that replacement: if 8a's review changed the file, implementers anchor against 8a's committed output, not the plan text.
- **Briefs for 8a/8b:** the SDD `task-brief` script takes a numeric N only; extract the 8a and 8b sections with awk on the `### Task 8a` / `### Task 8b` headings.
- **Pre-check done while writing:** Task 1's code was compiled and its 7 tests passed in a throwaway copy (then discarded). The seams `SimInput` (Task 4 ↔ 14), `V3Cmd::LoadParsed` (12 ↔ 14), `MatchRow` (11 ↔ 14) and `load_parsed` (8a ↔ 12) agree. No other task was compiled.
- Cross-part decisions made while drafting (readings of the spec, extra pub items, value forms) are in the **Appendix** at the end. Implementers of a task get the relevant appendix bullets in their dispatch.

---

### Task 1: `nif::material::MaterialState`

**Files:**
- Create: `crates/nif/src/material.rs`
- Modify: `crates/nif/src/lib.rs:6` (add `pub mod material;` after `pub mod header;`)
- Test: `crates/nif/tests/material.rs`

**Interfaces:**
- Consumes: `nif::path::{canon_texture, TexPath}` (`path.rs:36-37, 50-78`), `nif::props::{Alpha, Lighting, LightingParams, LightingTrailer}` (`props.rs:12-37, 39-53, 69-77, 109-121`), `nif::shape_head::ShapeHead` (`shape_head.rs:55-64`), `nif::geom::{Attr, VertexDesc}` (`geom/mod.rs:7`, `geom/vertex_desc.rs:8-37`).
- Produces (contract, exact):
  ```rust
  pub struct MaterialState { pub shader_type: u32, pub slsf1: u32, pub slsf2: u32, pub alpha: Option<Alpha>,
      pub params: LightingParams, pub slots: Vec<TexPath>, pub skinned: bool, pub has_vertex_colors: bool, pub hidden: bool }
  pub const SLOT_COUNT: usize = 9;
  impl MaterialState {
      pub fn from_lighting(head: &ShapeHead, l: &Lighting, slots: &[TexPath], alpha: Option<Alpha>) -> MaterialState;
      pub fn builtin_default() -> MaterialState;
      pub fn is_pbr(&self) -> bool;
      pub fn blend(&self) -> bool;
      pub fn set_slot_raw(&mut self, i: usize, raw: &str);
  }
  pub fn empty_slot() -> TexPath;
  ```
  plus `pub const SLSF1_SPECULAR, SLSF1_RECEIVE_SHADOWS, SLSF1_CAST_SHADOWS, SLSF2_ZBUFFER_WRITE, SLSF2_TRUE_PBR: u32`.

- [ ] **Step 1: Write the failing test** — create `crates/nif/tests/material.rs`:

```rust
// MaterialState over nifly's GPL-3.0 fixtures and synthetic heads.
use nif::material::{empty_slot, MaterialState, SLOT_COUNT};
use nif::props::{Alpha, LightingTrailer};
use nif::scan::{scan, Shader};
use nif::shape_head::{ShapeHead, ShapeKind};

fn read(f: &str) -> Vec<u8> {
    std::fs::read(format!("{}/tests/fixtures/{f}", env!("CARGO_MANIFEST_DIR"))).unwrap()
}

fn head(vertex_desc: Option<u64>, skinned: bool) -> ShapeHead {
    ShapeHead {
        kind: ShapeKind::BSTriShape,
        name: Some("x".into()),
        skinned,
        shader: 1,
        alpha: -1,
        vertex_desc,
        num_triangles: Some(1),
        num_vertices: Some(3),
    }
}

#[test]
fn from_lighting_copies_the_nif_fields() {
    let b = read("TestNifFile_Furniture_Col_SE.nif");
    let s = scan(&b).unwrap();
    let mut n = 0;
    for row in &s.shapes {
        let Shader::Lighting(l, slots) = &row.shader else {
            continue;
        };
        let m = MaterialState::from_lighting(&row.head, l, slots, row.alpha);
        assert_eq!(m.slots.len(), SLOT_COUNT);
        for (i, p) in slots.iter().take(SLOT_COUNT).enumerate() {
            assert_eq!(&m.slots[i], p, "slot {i}");
        }
        assert_eq!((m.shader_type, m.slsf1, m.slsf2), (l.shader_type, l.slsf1, l.slsf2));
        assert_eq!(m.params, l.params);
        assert_eq!(m.alpha, row.alpha);
        assert_eq!(m.skinned, row.head.skinned);
        // VertexAttribute Vertex_Colors = bit 5 of the flags at bit 44 (nif.xml BSVertexDesc).
        let vc = row.head.vertex_desc.is_some_and(|d| (d >> 44) & 0x20 != 0);
        assert_eq!(m.has_vertex_colors, vc);
        assert!(!m.hidden);
        n += 1;
    }
    assert_eq!(n, 2, "cylinder_1 and cylinder_2");
}

#[test]
fn slots_are_padded_and_truncated_to_nine() {
    let b = read("TestNifFile_Furniture_Col_SE.nif");
    let s = scan(&b).unwrap();
    let (row, l, slots) = s
        .shapes
        .iter()
        .find_map(|r| match &r.shader {
            Shader::Lighting(l, slots) => Some((r, l, slots)),
            _ => None,
        })
        .unwrap();
    let short = MaterialState::from_lighting(&row.head, l, &slots[..1], None);
    assert_eq!(short.slots.len(), SLOT_COUNT);
    assert_eq!(short.slots[0], slots[0]);
    assert!(short.slots[1..].iter().all(|p| *p == empty_slot()));
    let long: Vec<_> = (0..11).map(|i| nif::path::canon_texture(format!("t{i}.dds").as_bytes())).collect();
    let m = MaterialState::from_lighting(&row.head, l, &long, None);
    assert_eq!(m.slots, long[..SLOT_COUNT].to_vec());
}

#[test]
fn vertex_colours_come_from_the_vertex_desc() {
    let b = read("TestNifFile_Furniture_Col_SE.nif");
    let s = scan(&b).unwrap();
    let l = s
        .shapes
        .iter()
        .find_map(|r| match &r.shader {
            Shader::Lighting(l, _) => Some(l.clone()),
            _ => None,
        })
        .unwrap();
    // VF_Vertex | VF_UVs | VF_Normals | VF_Tangents | VF_Vertex_Colors = 0x3B at bit 44.
    let with = MaterialState::from_lighting(&head(Some(0x3Bu64 << 44 | 0x408), true), &l, &[], None);
    assert!(with.has_vertex_colors && with.skinned);
    let without = MaterialState::from_lighting(&head(Some(0x1Bu64 << 44 | 0x408), false), &l, &[], None);
    assert!(!without.has_vertex_colors && !without.skinned);
    assert!(!MaterialState::from_lighting(&head(None, false), &l, &[], None).has_vertex_colors);
}

#[test]
fn builtin_default_is_neutral() {
    let m = MaterialState::builtin_default();
    // nif.xml SkyrimShaderPropertyFlags1: Specular 0, Recieve_Shadows 8, Cast_Shadows 9; Flags2: ZBuffer_Write 0.
    assert_eq!(m.slsf1, 1 | 1 << 8 | 1 << 9);
    assert_eq!(m.slsf2, 1);
    assert_eq!(m.shader_type, 0);
    assert!(!m.is_pbr() && !m.blend() && !m.hidden && !m.skinned && !m.has_vertex_colors);
    assert_eq!(m.alpha, None);
    let p = &m.params;
    assert_eq!(p.uv_scale, [1.0, 1.0]);
    assert_eq!(p.uv_offset, [0.0, 0.0]);
    assert_eq!(p.alpha, 1.0);
    assert_eq!(p.glossiness, 80.0);
    assert_eq!(p.specular_color, [1.0; 3]);
    assert_eq!(p.specular_strength, 1.0);
    assert_eq!(p.lighting_effect_1, 0.3);
    assert_eq!(p.lighting_effect_2, 2.0);
    assert_eq!(p.emissive_color, [0.0; 3]);
    assert_eq!(p.emissive_multiple, 1.0);
    assert_eq!(p.trailer, LightingTrailer::None);
    assert_eq!(m.slots, vec![empty_slot(); SLOT_COUNT]);
}

#[test]
fn pbr_and_blend_flags() {
    let mut m = MaterialState::builtin_default();
    m.slsf2 |= 1 << 23;
    assert!(m.is_pbr());
    m.alpha = Some(Alpha { flags: 0x0200, threshold: 128 });
    assert!(!m.blend(), "test-only alpha does not blend");
    m.alpha = Some(Alpha { flags: 0x0201, threshold: 128 });
    assert!(m.blend());
}

#[test]
fn set_slot_raw_keeps_raw_and_recomputes_norm() {
    let mut m = MaterialState::builtin_default();
    m.set_slot_raw(0, "Textures\\PBR\\Foo\\Bar.DDS");
    assert_eq!(m.slots[0].raw, "Textures\\PBR\\Foo\\Bar.DDS");
    assert_eq!(m.slots[0].norm, "textures/pbr/foo/bar.dds");
    m.set_slot_raw(0, "");
    assert_eq!(m.slots[0], empty_slot());
    assert!(m.slots[0].flags.empty);
}

#[test]
#[should_panic(expected = "slot 9 out of range")]
fn set_slot_raw_past_nine_panics() {
    MaterialState::builtin_default().set_slot_raw(9, "x.dds");
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p nif --test material`
Expected: FAIL to compile, `error[E0432]: unresolved import nif::material`.

- [ ] **Step 3: Write the implementation** — create `crates/nif/src/material.rs`:

```rust
//! A shape's material as PGPatcher leaves it in the NIF and Community Shaders reads it at load (P1 spec §2.1,
//! 03 §1.2). Owned plain data over `props`, so the set editor, the PGPatcher simulator (`pgpsim`) and the
//! renderer share one value without a GPU dependency.
use crate::geom::{Attr, VertexDesc};
use crate::path::{canon_texture, TexPath};
use crate::props::{Alpha, Lighting, LightingParams, LightingTrailer};
use crate::shape_head::ShapeHead;

/// PGPatcher `PreFixTextureSlotCount`: every `MaterialState` carries exactly this many slots.
pub const SLOT_COUNT: usize = 9;

/// nif.xml `SkyrimShaderPropertyFlags1` bit 0 `Specular` (nifxml-master `nif.xml:6373`).
pub const SLSF1_SPECULAR: u32 = 1 << 0;
/// nif.xml `SkyrimShaderPropertyFlags1` bit 8 `Recieve_Shadows` (sic, `nif.xml:6380`).
pub const SLSF1_RECEIVE_SHADOWS: u32 = 1 << 8;
/// nif.xml `SkyrimShaderPropertyFlags1` bit 9 `Cast_Shadows` (`nif.xml:6382`).
pub const SLSF1_CAST_SHADOWS: u32 = 1 << 9;
/// nif.xml `SkyrimShaderPropertyFlags2` bit 0 `ZBuffer_Write` (`nif.xml:6409`).
pub const SLSF2_ZBUFFER_WRITE: u32 = 1 << 0;
/// nif.xml `SkyrimShaderPropertyFlags2` bit 23 `Unused01`: the TruePBR marker PGPatcher sets and CS reads
/// (01 §2.2, §3.1).
pub const SLSF2_TRUE_PBR: u32 = 1 << 23;

/// A shape's material after any PGPatcher simulation: what CS reads from the NIF at load.
#[derive(Debug, Clone, PartialEq)]
pub struct MaterialState {
    /// BSLightingShaderType (0 default, 1 envmap, …, 11 multilayer parallax, 16 eye).
    pub shader_type: u32,
    pub slsf1: u32,
    pub slsf2: u32,
    /// NiAlphaProperty: presence → DO_ALPHA_TEST; flags bit 0 → blending.
    pub alpha: Option<Alpha>,
    pub params: LightingParams,
    /// Exactly 9 entries (PGPatcher PreFixTextureSlotCount); missing slots are empty `TexPath`s.
    /// `raw` = the string PGPatcher sees/writes (backslashes, any case); `norm` = canonical render path
    /// (`textures/…`, lower case, `/`, from `canon_texture`).
    pub slots: Vec<TexPath>,
    pub skinned: bool,
    pub has_vertex_colors: bool,
    /// PGPatcher `delete: true`: the shape is not drawn.
    pub hidden: bool,
}

/// The empty slot: `canon_texture(b"")` (raw "", norm "", `flags.empty`).
pub fn empty_slot() -> TexPath {
    canon_texture(b"")
}

impl MaterialState {
    /// The NIF's own material. `slots` is the texture set as scanned (`nif::scan::Shader::Lighting`), padded
    /// with empty slots or cut to [`SLOT_COUNT`]. `has_vertex_colors` comes from the shape's vertex
    /// descriptor (`Vertex_Colors` attribute), not from SLSF2.5.
    pub fn from_lighting(
        head: &ShapeHead,
        l: &Lighting,
        slots: &[TexPath],
        alpha: Option<Alpha>,
    ) -> MaterialState {
        let mut s: Vec<TexPath> = slots.iter().take(SLOT_COUNT).cloned().collect();
        s.resize(SLOT_COUNT, empty_slot());
        MaterialState {
            shader_type: l.shader_type,
            slsf1: l.slsf1,
            slsf2: l.slsf2,
            alpha,
            params: l.params.clone(),
            slots: s,
            skinned: head.skinned,
            has_vertex_colors: head
                .vertex_desc
                .is_some_and(|d| VertexDesc(d).has(Attr::Colors)),
            hidden: false,
        }
    }

    /// The material of a built-in preview shape before the set editor fills it: shader 0, SLSF1 = Specular |
    /// Receive_Shadows | Cast_Shadows, SLSF2 = ZBuffer_Write, PBR off, neutral parameters (uv scale 1, alpha 1,
    /// glossiness 80, specular colour 1, strength 1, lighting effects 0.3 / 2.0, no emissive colour, emissive
    /// multiple 1, wrap clamp mode 3, no trailer), 9 empty slots.
    pub fn builtin_default() -> MaterialState {
        MaterialState {
            shader_type: 0,
            slsf1: SLSF1_SPECULAR | SLSF1_RECEIVE_SHADOWS | SLSF1_CAST_SHADOWS,
            slsf2: SLSF2_ZBUFFER_WRITE,
            alpha: None,
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
            slots: vec![empty_slot(); SLOT_COUNT],
            skinned: false,
            has_vertex_colors: false,
            hidden: false,
        }
    }

    /// SLSF2 bit 23, the TruePBR marker.
    pub fn is_pbr(&self) -> bool {
        self.slsf2 & SLSF2_TRUE_PBR != 0
    }

    /// The NiAlphaProperty enables blending (flags bit 0).
    pub fn blend(&self) -> bool {
        self.alpha.is_some_and(|a| a.blend())
    }

    /// Sets slot `i` from a PGPatcher-form string: `raw` is kept as given, `norm` is recomputed with
    /// `canon_texture`. Panics when `i >= SLOT_COUNT` (every `MaterialState` has 9 slots; a larger index is a
    /// programming error).
    pub fn set_slot_raw(&mut self, i: usize, raw: &str) {
        assert!(i < SLOT_COUNT, "slot {i} out of range (0..{SLOT_COUNT})");
        if self.slots.len() < SLOT_COUNT {
            self.slots.resize(SLOT_COUNT, empty_slot());
        }
        self.slots[i] = canon_texture(raw.as_bytes());
    }
}
```

Then in `crates/nif/src/lib.rs`, replace line 6:

```rust
pub mod header;
```

with:

```rust
pub mod header;
pub mod material;
```

- [ ] **Step 4: Run test to verify it passes**

Run: `cargo test -p nif --test material`
Expected: PASS, 7 tests.

- [ ] **Step 5: Gates**

Run: `cargo fmt --all && cargo clippy --workspace --all-targets -- -D warnings && cargo test --workspace`
Expected: all green.

- [ ] **Step 6: Commit**

```bash
git add crates/nif/src/material.rs crates/nif/src/lib.rs crates/nif/tests/material.rs
git commit -m "feat(nif): MaterialState, the owned post-PGPatcher shape material" -m "Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

---

### Task 2: pgpsim crate scaffold, entry preprocessing, `shape_matches`

**Files:**
- Create: `crates/pgpsim/Cargo.toml`
- Create: `crates/pgpsim/src/lib.rs`
- Create: `crates/pgpsim/src/text.rs`
- Create: `crates/pgpsim/src/prep.rs`
- Create: `crates/pgpsim/src/matching.rs`
- Create: `crates/pgpsim/tests/common/mod.rs`
- Create: `crates/pgpsim/tests/matching.rs`
- Modify (shared, own commit): `Cargo.toml` (workspace `members`, append one entry at the end), `Cargo.lock` (regenerated by cargo)

**Interfaces:**
- Consumes (Task 1): `nif::material::MaterialState` (fields `slots: Vec<TexPath>` with exactly 9 entries), `MaterialState::builtin_default() -> MaterialState`, `MaterialState::set_slot_raw(&mut self, i: usize, raw: &str)`; `nif::path::TexPath { raw: String, norm: String, flags }`.
- Produces:
  - `pub use nif::material::MaterialState;`
  - `pub fn prepare(entry: &serde_json::Value, default: &serde_json::Value) -> serde_json::Value`
  - `pub fn shape_matches(prepared: &serde_json::Value, nif_path: &str, slots: &[nif::path::TexPath]) -> bool`
  - crate-private, used by Tasks 3–5: `text::{tex_base(&str) -> String, tex_base_slot(&str, usize) -> String, strip_pbr(String) -> String, icontains(&str, &str) -> bool}`, `matching::{pg_slots(&[TexPath]) -> [String; 9], str_of(&Map<String, Value>, &str) -> Option<&str>, tex_name(&Map<String, Value>, &[String; 9]) -> Option<String>}`
  - test helpers `tests/common/mod.rs`: `D`, `N`, `NIF`, `shape(&[&str]) -> MaterialState`, `rock() -> MaterialState`

- [ ] **Step 1: Create the crate scaffold**

`crates/pgpsim/Cargo.toml`:

```toml
[package]
name = "pgpsim"
version.workspace = true
edition.workspace = true
license.workspace = true
publish.workspace = true
rust-version.workspace = true

[dependencies]
nif = { path = "../nif" }
serde_json.workspace = true
```

`crates/pgpsim/src/lib.rs`:

```rust
//! PGPatcher 1.2.0 TruePBR patch, simulated in memory for the 3D preview (P1 spec §2.1).
//! Rules: `F:\ai\dds\nifparseref\plan\p1\01-pgpatcher-truepbr-rules.md` ("01"). Pure: no GPU, no egui, no IO.
//! It stops at "the NIF as PGPatcher leaves it", a `MaterialState`; CS's load-time remap belongs to render3d.
pub use nif::material::MaterialState;
```

- [ ] **Step 2: Commit the scaffold (not yet a workspace member, so the workspace build is unchanged)**

```bash
git add crates/pgpsim/Cargo.toml crates/pgpsim/src/lib.rs
git commit -m "feat(pgpsim): crate scaffold" -m "Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

- [ ] **Step 3: Add the workspace member (shared file, add-only)**

In the root `Cargo.toml`, append `"crates/pgpsim"` as the **last** entry of `members`. The line becomes:

```toml
members = ["crates/encoders-sys", "crates/engine", "crates/cli", "crates/bench", "crates/gui", "crates/nif", "crates/vfs", "crates/modscan", "crates/render3d", "crates/pgpsim"]
```

Run: `cargo build -p pgpsim`
Expected: builds; `Cargo.lock` gains a `pgpsim` package entry.

- [ ] **Step 4: Commit the shared-file edit on its own**

```bash
git add Cargo.toml Cargo.lock
git commit -m "chore: add pgpsim to the workspace members" -m "Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

- [ ] **Step 5: Write the failing tests**

`crates/pgpsim/tests/common/mod.rs`:

```rust
//! Shared fixtures for the pgpsim integration tests.
#![allow(dead_code)]
use nif::material::MaterialState;

pub const D: &str = "textures\\landscape\\rock01_d.dds";
pub const N: &str = "textures\\landscape\\rock01_n.dds";
pub const NIF: &str = "meshes\\landscape\\rocks\\rock01.nif";

/// A shape whose slots are `slots` (raw NIF strings, slot 0 first); everything else is `builtin_default`.
pub fn shape(slots: &[&str]) -> MaterialState {
    let mut m = MaterialState::builtin_default();
    for (i, s) in slots.iter().enumerate() {
        m.set_slot_raw(i, s);
    }
    m
}

/// The usual fixture: a vanilla rock with diffuse and normal.
pub fn rock() -> MaterialState {
    shape(&[D, N])
}
```

`crates/pgpsim/tests/matching.rs`:

```rust
//! 01 §1.1 (preprocessing) and §1.3 (match rules) as table tests.
mod common;
use common::{rock, shape, D, N, NIF};
use pgpsim::{prepare, shape_matches};
use serde_json::{json, Value};

fn case(name: &str, entry: Value, slots: &[&str], want: bool) {
    let m = shape(slots);
    let got = shape_matches(&prepare(&entry, &Value::Null), NIF, &m.slots);
    assert_eq!(got, want, "{name}");
}

#[test]
fn prepare_merges_the_default_shallowly() {
    let p = prepare(
        &json!({"glint": {"screen_space_scale": 2.0}, "parallax": true}),
        &json!({"glint": {"log_microfacet_density": 9.0}, "parallax": false, "emissive": false}),
    );
    assert_eq!(p["glint"], json!({"screen_space_scale": 2.0}));
    assert_eq!(p["parallax"], json!(true));
    assert_eq!(p["emissive"], json!(false));
}

#[test]
fn prepare_copies_texture_over_match_diffuse_and_adds_backslashes() {
    let p = prepare(
        &json!({"texture": "a\\b", "match_diffuse": "c", "match_normal": "n", "rename": "\\r", "path_contains": "pc"}),
        &Value::Null,
    );
    assert_eq!(p["match_diffuse"], json!("\\a\\b"));
    assert_eq!(p["texture"], json!("a\\b"));
    assert_eq!(p["match_normal"], json!("\\n"));
    assert_eq!(p["rename"], json!("\\r"));
    assert_eq!(p["path_contains"], json!("pc"));
}

#[test]
fn prepare_leaves_non_objects_alone() {
    assert_eq!(prepare(&json!(3), &json!({"a": 1})), json!(3));
}

#[test]
fn match_rule_table() {
    case("suffix on a whole component", json!({"match_diffuse": "rock01_d"}), &[D], true);
    case(
        "no partial component",
        json!({"match_diffuse": "rock01_d"}),
        &["textures\\landscape\\bigrock01_d.dds"],
        false,
    );
    case("folders in the value", json!({"match_diffuse": "landscape\\rock01_d"}), &[D], true);
    case(
        "folders must match whole",
        json!({"match_diffuse": "landscape\\rock01_d"}),
        &["textures\\otherlandscape\\rock01_d.dds"],
        false,
    );
    case("diffuse suffix is not stripped", json!({"match_diffuse": "rock01"}), &[D], false);
    case("an extension in the value is dropped", json!({"match_diffuse": "rock01_d.dds"}), &[D], true);
    case(
        "case-insensitive",
        json!({"match_diffuse": "Landscape\\ROCK01_d"}),
        &["Textures\\LandScape\\Rock01_D.DDS"],
        true,
    );
    case(
        "textures\\pbr\\ is stripped on the NIF side",
        json!({"match_diffuse": "landscape\\rock01_d"}),
        &["textures\\pbr\\landscape\\rock01_d.dds"],
        true,
    );
    case(
        "a value starting with textures\\ never matches",
        json!({"match_diffuse": "textures\\landscape\\rock01_d"}),
        &[D],
        false,
    );
    case("texture key", json!({"texture": "rock01_d"}), &[D], true);
    case("texture beats match_diffuse", json!({"texture": "rock01_d", "match_diffuse": "other"}), &[D], true);
    case(
        "match_diffuse is overwritten by texture",
        json!({"texture": "other", "match_diffuse": "rock01_d"}),
        &[D],
        false,
    );
    case("match_normal, _n stripped", json!({"match_normal": "rock01"}), &[D, N], true);
    case(
        "match_normal, _msn stripped",
        json!({"match_normal": "rock01"}),
        &[D, "textures\\landscape\\rock01_msn.dds"],
        true,
    );
    case("match_normal value must not carry _n", json!({"match_normal": "rock01_n"}), &[D, N], false);
    case(
        "match_normal on an already-PBR normal",
        json!({"match_normal": "landscape\\rock01"}),
        &[D, "textures\\pbr\\landscape\\rock01_n.dds"],
        true,
    );
    case("empty diffuse never suffix-matches", json!({"match_diffuse": "rock01_d"}), &["", N], false);
    case(
        "path_contains is a case-insensitive substring",
        json!({"path_contains": "Landscape\\Rock"}),
        &[D],
        true,
    );
    case("path_contains sees no extension", json!({"path_contains": ".dds"}), &[D], false);
    case(
        "path_contains sees no pbr folder",
        json!({"path_contains": "pbr"}),
        &["textures\\pbr\\landscape\\rock01_d.dds"],
        false,
    );
    case("match1 is the full path", json!({"match1": "landscape\\rock01_d.dds"}), &[D], true);
    case("match1 needs .dds", json!({"match1": "landscape\\rock01_d"}), &[D], false);
    case(
        "match2 with its own textures\\ prefix",
        json!({"match2": "textures\\landscape\\rock01_n.dds"}),
        &[D, N],
        true,
    );
    case(
        "the textures\\ prefix check is case-sensitive",
        json!({"match1": "Textures\\landscape\\rock01_d.dds"}),
        &[D],
        false,
    );
    case(
        "match8 is the last one",
        json!({"match8": "x\\y_b.dds"}),
        &[D, N, "", "", "", "", "", "textures\\x\\y_b.dds"],
        true,
    );
    case(
        "there is no match9",
        json!({"match9": "x\\z.dds"}),
        &[D, N, "", "", "", "", "", "", "textures\\x\\z.dds"],
        false,
    );
    case(
        "std::filesystem turns the last separator into \\",
        json!({"match_diffuse": "landscape/rock01_d"}),
        &[D],
        true,
    );
    case(
        "only the last separator is converted",
        json!({"match_diffuse": "a/landscape/rock01_d"}),
        &["textures\\a\\landscape\\rock01_d.dds"],
        false,
    );
    case("no match key at all", json!({"roughness_scale": 0.5}), &[D, N], false);
}

#[test]
fn nif_filter_is_a_case_insensitive_substring_of_the_mesh_path() {
    let m = rock();
    let e = prepare(&json!({"match_diffuse": "rock01_d", "nif_filter": "Landscape\\Rocks"}), &Value::Null);
    assert!(shape_matches(&e, "meshes\\landscape\\rocks\\rock01.nif", &m.slots));
    assert!(!shape_matches(&e, "meshes\\clutter\\rock01.nif", &m.slots));
}

#[test]
fn shape_matches_wants_the_prepared_entry() {
    // Unprepared, "ck01_d" has no leading "\" and hits "rock01_d" mid-component; prepared, it does not.
    let m = rock();
    let raw = json!({"match_diffuse": "ck01_d"});
    assert!(shape_matches(&raw, NIF, &m.slots));
    assert!(!shape_matches(&prepare(&raw, &Value::Null), NIF, &m.slots));
}
```

- [ ] **Step 6: Run the tests to verify they fail**

Run: `cargo test -p pgpsim --test matching`
Expected: FAIL to compile with `error[E0432]: unresolved imports `pgpsim::prepare`, `pgpsim::shape_matches``.

- [ ] **Step 7: Implement the string helpers, `prepare`, and `shape_matches`**

`crates/pgpsim/src/text.rs`:

```rust
//! PGPatcher's path-string helpers (01 §1.3) on ASCII-lower-cased strings.

/// `PGNIFUtil::getTexBase(path, UNKNOWN)` [Src 1.2.0 NU getTexBase]: `path.parent_path() / path.stem()`,
/// lower-cased. MSVC `std::filesystem` treats `\` and `/` as separators and joins with `\`, so the last
/// separator comes back as `\` (a leading root separator is kept). The extension goes only when its dot is
/// inside the file name and is not its first character (`stem()` of `.dds` is `.dds`).
pub(crate) fn tex_base(path: &str) -> String {
    let p = path.to_ascii_lowercase();
    let sep = p.rfind(['\\', '/']);
    let file = match sep {
        Some(i) => &p[i + 1..],
        None => p.as_str(),
    };
    let stem = match file.rfind('.') {
        Some(d) if d > 0 && file != ".." => &file[..d],
        _ => file,
    };
    match sep {
        None => stem.to_string(),
        Some(0) => format!("{}{stem}", &p[..1]),
        Some(i) => format!("{}\\{stem}", &p[..i]),
    }
}

/// `getTexSuffixMap` in `std::map` (sorted key) order: suffix → 0-based slot [Src 1.2.0 NU:40-66].
const SUFFIXES: [(&str, usize); 19] = [
    ("_b", 7),
    ("_bl", 7),
    ("_cnr", 6),
    ("_d", 0),
    ("_e", 4),
    ("_em", 5),
    ("_envmask", 5),
    ("_f", 6),
    ("_flow", 7),
    ("_g", 2),
    ("_i", 6),
    ("_m", 5),
    ("_msn", 1),
    ("_n", 1),
    ("_p", 3),
    ("_rmaos", 5),
    ("_s", 6),
    ("_sk", 2),
    ("mask", 0),
];

/// `getTexBase(path, slot)`: `tex_base`, then the first suffix of that slot (map order) is cut.
pub(crate) fn tex_base_slot(path: &str, slot: usize) -> String {
    let b = tex_base(path);
    for (suf, s) in SUFFIXES {
        if s == slot && b.ends_with(suf) {
            return b[..b.len() - suf.len()].to_string();
        }
    }
    b
}

/// A leading `textures\pbr\` becomes `textures\` [Src 1.2.0 TP:286-290].
pub(crate) fn strip_pbr(p: String) -> String {
    match p.strip_prefix("textures\\pbr\\") {
        Some(rest) => format!("textures\\{rest}"),
        None => p,
    }
}

/// `boost::icontains` on ASCII.
pub(crate) fn icontains(hay: &str, needle: &str) -> bool {
    hay.to_ascii_lowercase()
        .contains(&needle.to_ascii_lowercase())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tex_base_follows_std_filesystem() {
        assert_eq!(tex_base("Textures\\A\\B_D.DDS"), "textures\\a\\b_d");
        assert_eq!(tex_base("\\rock01_d"), "\\rock01_d");
        assert_eq!(tex_base("a/b/c.dds"), "a/b\\c");
        assert_eq!(tex_base("rock.v2.dds"), "rock.v2");
        assert_eq!(tex_base("textures\\.dds"), "textures\\.dds");
        assert_eq!(tex_base(""), "");
    }

    #[test]
    fn tex_base_slot_cuts_the_slot_suffix_in_map_order() {
        assert_eq!(tex_base_slot("x_msn.dds", 1), "x");
        assert_eq!(tex_base_slot("x_n.dds", 1), "x");
        assert_eq!(tex_base_slot("x_d.dds", 1), "x_d");
        assert_eq!(tex_base_slot("x_rmaos.dds", 5), "x");
        assert_eq!(tex_base_slot("xmask.dds", 0), "x");
        assert_eq!(tex_base_slot("x_bl.dds", 7), "x");
    }

    #[test]
    fn strip_pbr_and_icontains() {
        assert_eq!(strip_pbr("textures\\pbr\\a\\b".into()), "textures\\a\\b");
        assert_eq!(strip_pbr("textures\\a\\b".into()), "textures\\a\\b");
        assert!(icontains("textures\\landscape", "LandScape"));
        assert!(!icontains("textures", "pbr"));
    }
}
```

`crates/pgpsim/src/prep.rs`:

```rust
//! Entry preprocessing, 01 §1.1 [Src 1.2.0 TP:142-166].
use serde_json::{Map, Value};

/// Keys that get a leading `\` (`getTruePBRConfigFilenameFields`); the `\` enforces whole path components.
const FILENAME_FIELDS: [&str; 3] = ["match_normal", "match_diffuse", "rename"];

/// Default merge (shallow, entry keys win), then `texture` → `match_diffuse` (texture wins), then the leading
/// backslash on `match_normal` / `match_diffuse` / `rename`. `default` may be `Value::Null` (bare-array file).
/// A non-object entry comes back unchanged (it can match nothing).
pub fn prepare(entry: &Value, default: &Value) -> Value {
    let Some(e) = entry.as_object() else {
        return entry.clone();
    };
    let mut out: Map<String, Value> = e.clone();
    if let Some(d) = default.as_object() {
        for (k, v) in d {
            out.entry(k.clone()).or_insert_with(|| v.clone());
        }
    }
    if let Some(t) = out.get("texture").cloned() {
        out.insert("match_diffuse".to_string(), t);
    }
    for f in FILENAME_FIELDS {
        if let Some(Value::String(s)) = out.get_mut(f) {
            if !s.starts_with('\\') {
                s.insert(0, '\\');
            }
        }
    }
    Value::Object(out)
}
```

`crates/pgpsim/src/matching.rs`:

```rust
//! Which shapes an entry applies to: 01 §1.3 [Src 1.2.0 TP:274-311, 403-547].
use crate::text::{icontains, strip_pbr, tex_base, tex_base_slot};
use nif::path::TexPath;
use serde_json::{Map, Value};

/// The 9 slots as PGPatcher reads them: the NIF string ASCII-lower-cased, nothing else [NU:363-380].
/// Missing slots are empty.
pub(crate) fn pg_slots(slots: &[TexPath]) -> [String; 9] {
    std::array::from_fn(|i| {
        slots
            .get(i)
            .map(|t| t.raw.to_ascii_lowercase())
            .unwrap_or_default()
    })
}

/// A string-valued key; any other JSON type counts as absent here (`simulate` reports wrong types).
pub(crate) fn str_of<'a>(e: &'a Map<String, Value>, k: &str) -> Option<&'a str> {
    e.get(k).and_then(Value::as_str)
}

/// The first rule that hits, in PGPatcher's insert order (match_normal, match_diffuse, path_contains,
/// match1..match8; insert-first), returned as the `texName` PGPatcher hands to `insertTruePBRData`.
/// `None`: no rule hits. `nif_filter` is not checked here.
pub(crate) fn tex_name(e: &Map<String, Value>, slots: &[String; 9]) -> Option<String> {
    let pre0 = strip_pbr(tex_base(&slots[0]));
    let pre1 = strip_pbr(tex_base_slot(&slots[1], 1));
    // match_normal / match_diffuse: ends-with on the prepared value, whose leading "\" makes it whole
    // components (TP:403-466 is a reversed-key binary search with the same result).
    if str_of(e, "match_normal").is_some_and(|v| pre1.ends_with(&tex_base(v))) {
        return Some(pre1);
    }
    if str_of(e, "match_diffuse").is_some_and(|v| pre0.ends_with(&tex_base(v))) {
        return Some(pre0);
    }
    if str_of(e, "path_contains").is_some_and(|v| icontains(&pre0, v)) {
        return Some(pre0);
    }
    // matchN: the full slot path, "textures\" prepended to the value unless it starts with exactly
    // "textures\" (case-sensitive check), then lower-cased; N = 1..8 (the code loops i < 8) [TP:204-217, 499-532].
    for (i, slot) in slots.iter().enumerate().take(8) {
        let Some(v) = str_of(e, &format!("match{}", i + 1)) else {
            continue;
        };
        let want = if v.is_empty() || v.starts_with("textures\\") {
            v.to_string()
        } else {
            format!("textures\\{v}")
        };
        if !slot.is_empty() && want.to_ascii_lowercase() == *slot {
            return Some(tex_base_slot(slot, i));
        }
    }
    None
}

/// Does the prepared entry (`prepare(entry, default)`) apply to this shape? `nif_path` is the mesh path
/// relative to Data (`meshes\…`, any case), used for `nif_filter` (case-insensitive substring, TP:544-547).
/// `slots` are the shape's texture-set paths (`MaterialState::slots`).
pub fn shape_matches(prepared: &Value, nif_path: &str, slots: &[TexPath]) -> bool {
    let Some(e) = prepared.as_object() else {
        return false;
    };
    if str_of(e, "nif_filter").is_some_and(|f| !icontains(nif_path, f)) {
        return false;
    }
    tex_name(e, &pg_slots(slots)).is_some()
}
```

Replace `crates/pgpsim/src/lib.rs` with:

```rust
//! PGPatcher 1.2.0 TruePBR patch, simulated in memory for the 3D preview (P1 spec §2.1).
//! Rules: `F:\ai\dds\nifparseref\plan\p1\01-pgpatcher-truepbr-rules.md` ("01"). Pure: no GPU, no egui, no IO.
//! It stops at "the NIF as PGPatcher leaves it", a `MaterialState`; CS's load-time remap belongs to render3d.
mod matching;
mod prep;
mod text;

pub use matching::shape_matches;
pub use nif::material::MaterialState;
pub use prep::prepare;
```

- [ ] **Step 8: Run the tests to verify they pass**

Run: `cargo test -p pgpsim`
Expected: PASS: `tests/matching.rs` (6 tests) and the 3 unit tests in `text.rs`.

- [ ] **Step 9: Gates**

Run: `cargo fmt --all && cargo clippy --workspace --all-targets -- -D warnings && cargo test --workspace`
Expected: all green.

- [ ] **Step 10: Commit**

```bash
git add crates/pgpsim/src crates/pgpsim/tests
git commit -m "feat(pgpsim): PGPatcher entry preprocessing and shape matching (01 §1.1, §1.3)" -m "Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

---

### Task 3: pgpsim `matched_path`, slot rewrite, missing-texture check

**Files:**
- Modify: `crates/pgpsim/src/text.rs` (add `add_pbr`)
- Modify: `crates/pgpsim/src/matching.rs` (add `matched_path`, `prefix`, `cut_tail`)
- Create: `crates/pgpsim/src/json.rs`
- Create: `crates/pgpsim/src/slots.rs` (with unit tests)
- Modify: `crates/pgpsim/src/lib.rs`
- Create: `crates/pgpsim/tests/matched_path.rs`

**Interfaces:**
- Consumes (Task 2): `text::{tex_base, strip_pbr}`, `matching::{pg_slots, str_of, tex_name}`, `prepare`, test helpers `common::{shape, D, N}`.
- Produces:
  - `pub fn matched_path(prepared: &serde_json::Value, slots: &[nif::path::TexPath]) -> Option<String>`. `None` = no rule; `Some("")` = matched with `pbr:false`; ignores `nif_filter`.
  - crate-private for Task 4: `json::flag(&Map<String, Value>, &str) -> bool`; `slots::apply_slots(&mut [String; 9], &Map<String, Value>, &str)`; `slots::first_missing(&[String; 9], &dyn Fn(&str) -> bool) -> Option<String>`; `text::add_pbr(&str) -> String`.

- [ ] **Step 1: Write the failing tests**

`crates/pgpsim/tests/matched_path.rs`:

```rust
//! 01 §1.3 `matchedPath` (the PBR prefix), `rename`, and `pbr:false`.
mod common;
use common::{shape, D, N};
use pgpsim::{matched_path, prepare};
use serde_json::{json, Value};

fn case(name: &str, entry: Value, slots: &[&str], want: Option<&str>) {
    let m = shape(slots);
    let got = matched_path(&prepare(&entry, &Value::Null), &m.slots);
    assert_eq!(got.as_deref(), want, "{name}");
}

#[test]
fn matched_path_table() {
    case("diffuse", json!({"match_diffuse": "rock01_d"}), &[D], Some("textures\\pbr\\landscape\\rock01_d"));
    case(
        "already PBR",
        json!({"match_diffuse": "rock01_d"}),
        &["textures\\pbr\\landscape\\rock01_d.dds"],
        Some("textures\\pbr\\landscape\\rock01_d"),
    );
    case(
        "upper-case NIF path is lower-cased",
        json!({"match_diffuse": "rock01_d"}),
        &["Textures\\Landscape\\Rock01_D.dds"],
        Some("textures\\pbr\\landscape\\rock01_d"),
    );
    case("normal", json!({"match_normal": "rock01"}), &[D, N], Some("textures\\pbr\\landscape\\rock01"));
    case(
        "the normal rule runs first",
        json!({"match_normal": "rock01", "match_diffuse": "rock01_d"}),
        &[D, N],
        Some("textures\\pbr\\landscape\\rock01"),
    );
    case(
        "blind erase by the match_normal length when the diffuse rule hit",
        json!({"match_normal": "rock01", "match_diffuse": "rock01_d"}),
        &[D, "textures\\landscape\\other_n.dds"],
        Some("textures\\pbr\\landscape\\r\\rock01"),
    );
    case(
        "rename (golden: Windhelm bridge)",
        json!({"match_diffuse": "nicola89b\\windhelm_bridge\\sndiffuse", "rename": "landscape\\snow01"}),
        &["textures\\nicola89b\\windhelm_bridge\\sndiffuse.dds"],
        Some("textures\\pbr\\landscape\\snow01"),
    );
    case(
        "rename equal ignoring case keeps the field",
        json!({"match_diffuse": "rock01_d", "rename": "ROCK01_D"}),
        &[D],
        Some("textures\\pbr\\landscape\\rock01_d"),
    );
    case(
        "rename is lower-cased",
        json!({"match_diffuse": "rock01_d", "rename": "Snow\\Snow01"}),
        &[D],
        Some("textures\\pbr\\landscape\\snow\\snow01"),
    );
    case(
        "path_contains keeps the whole diffuse",
        json!({"path_contains": "landscape"}),
        &[D],
        Some("textures\\pbr\\landscape\\rock01_d"),
    );
    case(
        "match1 cuts the slot-1 suffix",
        json!({"match1": "landscape\\rock01_d.dds"}),
        &[D],
        Some("textures\\pbr\\landscape\\rock01"),
    );
    case(
        "match2 cuts _n",
        json!({"match2": "landscape\\rock01_n.dds"}),
        &[D, N],
        Some("textures\\pbr\\landscape\\rock01"),
    );
    case("pbr:false matches with an empty prefix", json!({"match_diffuse": "rock01_d", "pbr": false}), &[D], Some(""));
    case("no rule hits", json!({"match_diffuse": "other"}), &[D], None);
    case(
        "non-ASCII never panics (cut backs up to a char boundary)",
        json!({"match_normal": "aa", "match_diffuse": "é_d"}),
        &["textures\\é_d.dds", N],
        Some("textures\\pbr\\\\aa"),
    );
}
```

`crates/pgpsim/src/slots.rs` (tests only for now; the implementation is added in Step 3):

```rust
//! The slot rewrite (`applyOnePatchSlots`, 01 §2.3 slot table) and the missing-texture check (01 §1.4).

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::{json, Map, Value};
    use std::cell::RefCell;

    const MP: &str = "textures\\pbr\\landscape\\rock01";

    fn obj(v: Value) -> Map<String, Value> {
        match v {
            Value::Object(m) => m,
            other => panic!("not an object: {other}"),
        }
    }

    fn source() -> [String; 9] {
        [
            "textures\\landscape\\rock01_d.dds",
            "textures\\landscape\\rock01_n.dds",
            "textures\\landscape\\rock01_g.dds",
            "textures\\landscape\\rock01_p.dds",
            "textures\\cubemaps\\shiny_e.dds",
            "textures\\landscape\\rock01_m.dds",
            "textures\\landscape\\rock01_i.dds",
            "textures\\landscape\\rock01_b.dds",
            "textures\\landscape\\rock01_x.dds",
        ]
        .map(String::from)
    }

    fn mp(suffix: &str) -> String {
        format!("{MP}{suffix}")
    }

    fn run(entry: Value) -> [String; 9] {
        let mut s = source();
        apply_slots(&mut s, &obj(entry), MP);
        s
    }

    #[test]
    fn bare_entry_writes_diffuse_normal_rmaos_and_clears_4_6_7() {
        let src = source();
        let want = [
            mp(".dds"),
            mp("_n.dds"),
            src[2].clone(),
            src[3].clone(),
            String::new(),
            mp("_rmaos.dds"),
            String::new(),
            String::new(),
            src[8].clone(),
        ];
        assert_eq!(run(json!({})), want);
    }

    #[test]
    fn emissive_and_parallax_follow_their_keys() {
        let on = run(json!({"emissive": true, "parallax": true}));
        assert_eq!(on[2], mp("_g.dds"));
        assert_eq!(on[3], mp("_p.dds"));
        let off = run(json!({"emissive": false, "parallax": false}));
        assert_eq!(off[2], "");
        assert_eq!(off[3], "");
    }

    #[test]
    fn locks_keep_the_source_but_not_the_cubemap() {
        let got = run(json!({
            "lock_diffuse": true, "lock_normal": true, "emissive": true, "lock_emissive": true,
            "parallax": false, "lock_parallax": true, "lock_rmaos": true, "coat_normal": true,
            "lock_cnr": true, "subsurface": true, "lock_subsurface": true, "lock_cubemap": true
        }));
        let mut want = source();
        want[4] = String::new();
        assert_eq!(got, want);
    }

    #[test]
    fn cubemap_is_verbatim_unless_locked() {
        assert_eq!(run(json!({"cubemap": "Textures\\Cubemaps\\Ice_E.dds"}))[4], "Textures\\Cubemaps\\Ice_E.dds");
        assert_eq!(run(json!({"cubemap": "x.dds", "lock_cubemap": true}))[4], "");
    }

    #[test]
    fn slot_6_cnr_and_fuzz_texture_fuzz_wins() {
        assert_eq!(run(json!({"coat_normal": true}))[6], mp("_cnr.dds"));
        assert_eq!(run(json!({"coat_normal": true, "fuzz": {"texture": true}}))[6], mp("_f.dds"));
        assert_eq!(run(json!({"fuzz": {"texture": false}}))[6], "");
        assert_eq!(run(json!({"fuzz": {}}))[6], "");
    }

    #[test]
    fn slot_7_s_from_any_of_three_keys() {
        for k in ["subsurface_foliage", "subsurface", "coat_diffuse"] {
            assert_eq!(run(json!({ k: true }))[7], mp("_s.dds"), "{k}");
        }
        assert_eq!(run(json!({"subsurface": false, "coat_diffuse": false}))[7], "");
    }

    #[test]
    fn slot_n_overrides_last_lower_cased_with_prefix_and_empty_clears() {
        let got = run(json!({
            "emissive": true,
            "slot1": "Landscape\\Custom_D.dds",
            "slot3": "",
            "slot5": "textures\\black.dds",
            "slot8": "TEXTURES\\X_S.dds",
            "slot9": "textures\\nine.dds"
        }));
        assert_eq!(got[0], "textures\\landscape\\custom_d.dds");
        assert_eq!(got[2], "");
        assert_eq!(got[4], "textures\\black.dds");
        assert_eq!(got[7], "textures\\x_s.dds");
        assert_eq!(got[8], source()[8]);
    }

    #[test]
    fn empty_prefix_changes_nothing() {
        let mut s = source();
        apply_slots(&mut s, &obj(json!({"slot1": "a.dds"})), "");
        assert_eq!(s, source());
    }

    #[test]
    fn missing_check_asks_every_non_empty_slot_lower_cased() {
        let mut slots = run(json!({}));
        slots[4] = "Textures\\Cubemaps\\Ice_E.dds".to_string();
        let seen = RefCell::new(Vec::new());
        let exists = |p: &str| {
            seen.borrow_mut().push(p.to_string());
            true
        };
        assert_eq!(first_missing(&slots, &exists), None);
        let src = source();
        let want = vec![
            mp(".dds"),
            mp("_n.dds"),
            src[2].clone(),
            src[3].clone(),
            "textures\\cubemaps\\ice_e.dds".to_string(),
            mp("_rmaos.dds"),
            src[8].clone(),
        ];
        assert_eq!(*seen.borrow(), want);
    }

    #[test]
    fn missing_check_reports_the_first_missing_slot() {
        let slots = run(json!({}));
        let rmaos = mp("_rmaos.dds");
        let exists = |p: &str| p != rmaos && p != source()[8];
        assert_eq!(first_missing(&slots, &exists), Some(rmaos.clone()));
    }
}
```

In `crates/pgpsim/src/lib.rs`, add the line `mod slots;` directly below `mod prep;` so the unit tests are built. Step 3 shows the whole file.

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo test -p pgpsim`
Expected: FAIL to compile: `unresolved import pgpsim::matched_path` (tests/matched_path.rs) and `cannot find function apply_slots` / `first_missing` (src/slots.rs).

- [ ] **Step 3: Implement**

Append to `crates/pgpsim/src/text.rs`, above `#[cfg(test)]`:

```rust
/// A leading `textures\` that is not `textures\pbr\` becomes `textures\pbr\` [Src 1.2.0 TP:551-555].
pub(crate) fn add_pbr(p: &str) -> String {
    match p.strip_prefix("textures\\") {
        Some(rest) if !rest.starts_with("pbr\\") => format!("textures\\pbr\\{rest}"),
        _ => p.to_string(),
    }
}
```

`crates/pgpsim/src/json.rs`:

```rust
//! JSON accessors with nlohmann's semantics for well-typed values. `simulate` rejects wrong types first
//! (`check.rs`, Task 4), so these never see one there.
use serde_json::{Map, Value};

/// PGPatcher's `flag(json, key)`: present and `true`.
pub(crate) fn flag(e: &Map<String, Value>, k: &str) -> bool {
    e.get(k).and_then(Value::as_bool) == Some(true)
}
```

In `crates/pgpsim/src/matching.rs`, change the `use crate::text::…` line to:

```rust
use crate::text::{add_pbr, icontains, strip_pbr, tex_base, tex_base_slot};
```

and append at the end of the file:

```rust
/// PGPatcher's `matchedPath` for this entry, e.g. `textures\pbr\landscape\rocks01`, lower case.
/// `None`: no rule hits. `Some("")`: a rule hits but PBR is off (`pbr:false`); scalars still apply.
/// `nif_filter` is NOT checked (call `shape_matches` for that).
pub fn matched_path(prepared: &Value, slots: &[TexPath]) -> Option<String> {
    let e = prepared.as_object()?;
    let name = tex_name(e, &pg_slots(slots))?;
    Some(prefix(e, &name))
}

/// `insertTruePBRData` [Src 1.2.0 TP:549-581], 01 §1.3 pseudo-code.
fn prefix(e: &Map<String, Value>, tex_name: &str) -> String {
    let mut tex_path = add_pbr(tex_name);
    // match_normal wins even when the entry matched by diffuse; without either, the whole path is the field.
    let mut field = match str_of(e, "match_normal").or_else(|| str_of(e, "match_diffuse")) {
        Some(v) => tex_base(v),
        None => tex_path.clone(),
    };
    // Blind erase: the folders in front of the match (01 §1.3 [Inference] note on mixed keys).
    cut_tail(&mut tex_path, field.len());
    if let Some(r) = str_of(e, "rename") {
        if !r.eq_ignore_ascii_case(&field) {
            field = r.to_string();
        }
    }
    let mp = format!("{tex_path}{field}").to_ascii_lowercase();
    let pbr_on = e.get("pbr").is_none_or(|v| v.as_bool() == Some(true));
    if pbr_on {
        mp
    } else {
        String::new()
    }
}

/// Drops the last `n` bytes (C++ `erase(len - n, n)`); saturates instead of throwing, and backs up to a
/// char boundary for non-ASCII paths.
fn cut_tail(s: &mut String, n: usize) {
    let mut at = s.len().saturating_sub(n);
    while !s.is_char_boundary(at) {
        at -= 1;
    }
    s.truncate(at);
}
```

In `crates/pgpsim/src/slots.rs`, insert above the `#[cfg(test)]` line:

```rust
use crate::json::flag;
use serde_json::{Map, Value};

/// `applyOnePatchSlots` [Src 1.2.0 TP:865-965]. `slots` are PGPatcher's lower-cased strings; `mp` is the
/// entry's `matchedPath` (an empty one changes nothing).
pub(crate) fn apply_slots(slots: &mut [String; 9], e: &Map<String, Value>, mp: &str) {
    if mp.is_empty() {
        return;
    }
    if !flag(e, "lock_diffuse") {
        slots[0] = format!("{mp}.dds");
    }
    if !flag(e, "lock_normal") {
        slots[1] = format!("{mp}_n.dds");
    }
    if e.contains_key("emissive") && !flag(e, "lock_emissive") {
        slots[2] = if flag(e, "emissive") {
            format!("{mp}_g.dds")
        } else {
            String::new()
        };
    }
    if e.contains_key("parallax") && !flag(e, "lock_parallax") {
        slots[3] = if flag(e, "parallax") {
            format!("{mp}_p.dds")
        } else {
            String::new()
        };
    }
    // Slot 4: the `cubemap` value verbatim, otherwise always cleared (also with lock_cubemap).
    slots[4] = match e.get("cubemap").and_then(Value::as_str) {
        Some(c) if !flag(e, "lock_cubemap") => c.to_string(),
        _ => String::new(),
    };
    if !flag(e, "lock_rmaos") {
        slots[5] = format!("{mp}_rmaos.dds");
    }
    if !flag(e, "lock_cnr") {
        let fuzz_tex = e
            .get("fuzz")
            .and_then(Value::as_object)
            .is_some_and(|f| flag(f, "texture"));
        slots[6] = if fuzz_tex {
            format!("{mp}_f.dds")
        } else if flag(e, "coat_normal") {
            format!("{mp}_cnr.dds")
        } else {
            String::new()
        };
    }
    if !flag(e, "lock_subsurface") {
        let s = flag(e, "subsurface_foliage") || flag(e, "subsurface") || flag(e, "coat_diffuse");
        slots[7] = if s {
            format!("{mp}_s.dds")
        } else {
            String::new()
        };
    }
    // slot1..slot8 last: lower-cased, "textures\" prepended unless present, "" clears.
    for (i, slot) in slots.iter_mut().enumerate().take(8) {
        if let Some(v) = e.get(&format!("slot{}", i + 1)).and_then(Value::as_str) {
            let mut v = v.to_ascii_lowercase();
            if !v.is_empty() && !v.starts_with("textures\\") {
                v.insert_str(0, "textures\\");
            }
            *slot = v;
        }
    }
}

/// The 01 §1.4 check [Src 1.2.0 TP:348-375]: every non-empty slot after the slot pass must be a file in the
/// VFS. Returns the first that is not. `exists` receives the lower-cased string.
pub(crate) fn first_missing(slots: &[String; 9], exists: &dyn Fn(&str) -> bool) -> Option<String> {
    slots
        .iter()
        .filter(|s| !s.is_empty())
        .map(|s| s.to_ascii_lowercase())
        .find(|s| !exists(s))
}
```

Replace `crates/pgpsim/src/lib.rs` with:

```rust
//! PGPatcher 1.2.0 TruePBR patch, simulated in memory for the 3D preview (P1 spec §2.1).
//! Rules: `F:\ai\dds\nifparseref\plan\p1\01-pgpatcher-truepbr-rules.md` ("01"). Pure: no GPU, no egui, no IO.
//! It stops at "the NIF as PGPatcher leaves it", a `MaterialState`; CS's load-time remap belongs to render3d.

// `json` and `slots` are wired into `simulate` by Task 4; until then only their unit tests use them.
#[allow(dead_code)]
mod json;
mod matching;
mod prep;
#[allow(dead_code)]
mod slots;
mod text;

pub use matching::{matched_path, shape_matches};
pub use nif::material::MaterialState;
pub use prep::prepare;
```

- [ ] **Step 4: Run the tests to verify they pass**

Run: `cargo test -p pgpsim`
Expected: PASS: `tests/matched_path.rs` (1 table test), `tests/matching.rs` (6), and the unit tests in `text.rs` (3) and `slots.rs` (10).

- [ ] **Step 5: Gates**

Run: `cargo fmt --all && cargo clippy --workspace --all-targets -- -D warnings && cargo test --workspace`
Expected: all green.

- [ ] **Step 6: Commit**

```bash
git add crates/pgpsim/src crates/pgpsim/tests
git commit -m "feat(pgpsim): matchedPath with rename, slot rewrite, missing-texture check (01 §1.3, §1.4, §2.3)" -m "Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

---

### Task 4: pgpsim flags, type, scalars, leaks, badges, delete → `simulate`

**Files:**
- Modify: `crates/pgpsim/src/json.rs` (add `get_bool`, `num`, `get_num`, `rgb`, `GLINT_KEYS`)
- Create: `crates/pgpsim/src/check.rs`
- Create: `crates/pgpsim/src/patch.rs`
- Modify: `crates/pgpsim/src/lib.rs` (types, `simulate`, drop the Task 3 `allow`s)
- Modify: `crates/pgpsim/tests/common/mod.rs` (simulation helpers)
- Create: `crates/pgpsim/tests/patch.rs`

**Interfaces:**
- Consumes: Task 1 `MaterialState { shader_type, slsf1, slsf2, params: nif::props::LightingParams, slots, has_vertex_colors, hidden, .. }`, `MaterialState::is_pbr`, `set_slot_raw`; `nif::props::LightingTrailer`; Task 2 `prepare`, `shape_matches`, `matching::pg_slots`; Task 3 `matched_path`, `json::flag`, `slots::{apply_slots, first_missing}`.
- Produces (contract):
  - `pub struct SimInput<'a> { pub entry: &'a Value, pub default: &'a Value, pub nif_path: &'a str, pub source: &'a MaterialState, pub exists: &'a dyn Fn(&str) -> bool }`
  - `#[derive(Debug, Clone, PartialEq)] pub enum Badge { NotPreviewed(&'static str) }`
  - `#[derive(Debug, Clone, PartialEq)] pub enum SimResult { NoMatch, Patched { material: MaterialState, matched_path: String, badges: Vec<Badge> }, Skip(String) }`
  - `pub fn simulate(i: &SimInput) -> SimResult`
  - crate-private for Task 5: `patch::run(matches: &[(&Map<String, Value>, String)], source: &MaterialState, exists: &dyn Fn(&str) -> bool) -> SimResult`
  - test helpers: `MP`, `sdc_default()`, `sim`, `sim_with`, `patched`, `raws`, `bit`, `with_bits`, `f`, `mlp`

- [ ] **Step 1: Extend the test helpers**

Replace `crates/pgpsim/tests/common/mod.rs` with:

```rust
//! Shared fixtures for the pgpsim integration tests.
#![allow(dead_code)]
use nif::material::MaterialState;
use nif::props::LightingTrailer;
use pgpsim::{simulate, Badge, SimInput, SimResult};
use serde_json::{json, Value};

pub const D: &str = "textures\\landscape\\rock01_d.dds";
pub const N: &str = "textures\\landscape\\rock01_n.dds";
pub const NIF: &str = "meshes\\landscape\\rocks\\rock01.nif";
/// `matchedPath` of `{"match_diffuse": "rock01_d"}` on `rock()`.
pub const MP: &str = "textures\\pbr\\landscape\\rock01_d";

/// A shape whose slots are `slots` (raw NIF strings, slot 0 first); everything else is `builtin_default`.
pub fn shape(slots: &[&str]) -> MaterialState {
    let mut m = MaterialState::builtin_default();
    for (i, s) in slots.iter().enumerate() {
        m.set_slot_raw(i, s);
    }
    m
}

/// The usual fixture: a vanilla rock with diffuse and normal.
pub fn rock() -> MaterialState {
    shape(&[D, N])
}

/// sdc's fixed `default` block (`gui/src/pgp/writer.rs` `fixed_default_block`).
pub fn sdc_default() -> Value {
    json!({"emissive": false, "parallax": false, "subsurface": false,
           "roughness_scale": 1.0, "specular_level": 0.04, "displacement_scale": 1.0})
}

pub fn sim_with(entry: &Value, default: &Value, source: &MaterialState, exists: &dyn Fn(&str) -> bool) -> SimResult {
    simulate(&SimInput {
        entry,
        default,
        nif_path: NIF,
        source,
        exists,
    })
}

/// Every texture exists.
pub fn sim(entry: &Value, default: &Value, source: &MaterialState) -> SimResult {
    sim_with(entry, default, source, &|_| true)
}

pub fn patched(r: SimResult) -> (MaterialState, String, Vec<Badge>) {
    match r {
        SimResult::Patched {
            material,
            matched_path,
            badges,
        } => (material, matched_path, badges),
        other => panic!("expected Patched, got {other:?}"),
    }
}

pub fn raws(m: &MaterialState) -> Vec<String> {
    m.slots.iter().map(|t| t.raw.clone()).collect()
}

pub fn bit(word: u32, n: u32) -> bool {
    word & (1 << n) != 0
}

pub fn with_bits(word: u32, bits: &[u32]) -> u32 {
    bits.iter().fold(word, |w, b| w | (1 << b))
}

/// The JSON number as the sim stores it (`as_f64() as f32`).
pub fn f(x: f64) -> f32 {
    x as f32
}

/// A type-11 trailer with nifly's default envmap strength (1.0).
pub fn mlp(a: f64, b: f64, c: f64, d: f64) -> LightingTrailer {
    LightingTrailer::MultiLayer {
        inner_thickness: f(a),
        refraction_scale: f(b),
        inner_texture_scale: [f(c), f(d)],
        envmap_strength: 1.0,
    }
}
```

- [ ] **Step 2: Write the failing tests**

`crates/pgpsim/tests/patch.rs`:

```rust
//! 01 §2.1-§2.3 on one entry through `simulate`.
mod common;
use common::*;
use nif::props::LightingTrailer;
use pgpsim::{Badge, SimResult};
use serde_json::{json, Value};

fn md() -> Value {
    json!({"match_diffuse": "rock01_d"})
}

fn with(extra: Value) -> Value {
    let mut e = md();
    for (k, v) in extra.as_object().expect("object") {
        e[k] = v.clone();
    }
    e
}

#[test]
fn plain_sdc_entry() {
    let (m, mp, badges) = patched(sim(&with(json!({"roughness_scale": 0.25})), &sdc_default(), &rock()));
    assert_eq!(mp, MP);
    assert!(badges.is_empty());
    assert_eq!(m.shader_type, 0);
    assert!(m.is_pbr());
    assert_eq!(m.params.glossiness, f(0.04));
    assert_eq!(m.params.specular_strength, f(0.25));
    assert_eq!(m.params.lighting_effect_2, f(1.0));
    let want: Vec<String> = [
        format!("{MP}.dds"),
        format!("{MP}_n.dds"),
        String::new(),
        String::new(),
        String::new(),
        format!("{MP}_rmaos.dds"),
        String::new(),
        String::new(),
        String::new(),
    ]
    .to_vec();
    assert_eq!(raws(&m), want);
    assert_eq!(m.slots[1].norm, "textures/pbr/landscape/rock01_d_n.dds");
}

#[test]
fn always_on_flag_clears() {
    let mut src = rock();
    src.slsf1 = with_bits(0, &[7, 10, 11, 17, 18, 29]);
    src.slsf2 = with_bits(0, &[6, 24, 25, 27]);
    let (m, _, _) = patched(sim(&md(), &sdc_default(), &src));
    assert_eq!(m.slsf1, 0);
    assert_eq!(m.slsf2, 1 << 23);
}

#[test]
fn leaks_without_the_keys_that_clear_them() {
    let mut src = rock();
    src.slsf1 = with_bits(0, &[29]);
    src.slsf2 = with_bits(0, &[5, 12, 26, 30]);
    // No default: no "emissive" (SLSF1.29 leaks), no "subsurface" (Rim leaks), no "vertex_colors" (SLSF2.5
    // leaks); Fit_Slope and Effect_Lighting are never cleared.
    let (m, _, _) = patched(sim(&md(), &Value::Null, &src));
    assert_eq!(m.slsf1, 1 << 29);
    assert_eq!(m.slsf2, with_bits(0, &[5, 12, 23, 26, 30]));
    // sdc's default writes emissive:false and subsurface:false: those two are cleared, Fit_Slope still leaks.
    let (m, _, _) = patched(sim(&md(), &sdc_default(), &src));
    assert_eq!(m.slsf1, 0);
    assert_eq!(m.slsf2, with_bits(0, &[5, 12, 23, 30]));
}

#[test]
fn absent_emissive_and_parallax_keep_the_source_slots() {
    let src = shape(&[D, N, "textures\\landscape\\rock01_g.dds", "textures\\landscape\\rock01_p.dds"]);
    let (m, _, _) = patched(sim(&md(), &Value::Null, &src));
    assert_eq!(m.slots[2].raw, "textures\\landscape\\rock01_g.dds");
    assert_eq!(m.slots[3].raw, "textures\\landscape\\rock01_p.dds");
}

#[test]
fn emissive_true_sets_external_emittance_and_glow_slot() {
    let (m, _, _) = patched(sim(&with(json!({"emissive": true})), &sdc_default(), &rock()));
    assert!(bit(m.slsf1, 29));
    assert_eq!(m.slots[2].raw, format!("{MP}_g.dds"));
}

#[test]
fn vertex_colors_false_drops_attribute_and_flag() {
    let mut src = rock();
    src.has_vertex_colors = true;
    src.slsf2 = with_bits(0, &[5]);
    let (m, _, _) = patched(sim(&with(json!({"vertex_colors": false})), &sdc_default(), &src));
    assert!(!m.has_vertex_colors);
    assert!(!bit(m.slsf2, 5));
}

#[test]
fn zbuffer_write_false_clears_slsf2_0() {
    let mut src = rock();
    src.slsf2 = 1;
    let (m, _, _) = patched(sim(&with(json!({"zbuffer_write": false})), &sdc_default(), &src));
    assert!(!bit(m.slsf2, 0));
}

#[test]
fn multilayer_beats_glint_beats_fuzz() {
    let all = with(json!({"multilayer": true, "glint": {"screen_space_scale": 4.0}, "fuzz": {"color": [0.5, 0.5, 0.5]}}));
    let (m, _, _) = patched(sim(&all, &sdc_default(), &rock()));
    assert_eq!(m.shader_type, 11);
    assert!(bit(m.slsf2, 24));
    assert!(!bit(m.slsf2, 12));
    assert_eq!(m.params.trailer, mlp(0.0, 1.0, 1.0, 1.0));

    let glint_fuzz = with(json!({"glint": {"screen_space_scale": 4.0}, "fuzz": {"color": [0.5, 0.5, 0.5]}}));
    let (m, _, _) = patched(sim(&glint_fuzz, &sdc_default(), &rock()));
    assert_eq!(m.shader_type, 11);
    assert!(bit(m.slsf2, 12));
    assert!(!bit(m.slsf2, 24));
    assert!(!bit(m.slsf2, 25));
    assert_eq!(m.params.trailer, mlp(4.0, 1.0, 1.0, 1.0));

    let fuzz = with(json!({"fuzz": {"color": [0.5, 0.5, 0.5]}}));
    let (m, _, _) = patched(sim(&fuzz, &sdc_default(), &rock()));
    assert_eq!(m.shader_type, 11);
    assert!(bit(m.slsf2, 25));
    assert!(!bit(m.slsf2, 12));
    assert_eq!(m.params.trailer, mlp(0.5, 0.5, 0.5, 1.0));
}

#[test]
fn type_11_keeps_its_floats_and_type_0_drops_the_trailer() {
    let mut src = rock();
    src.shader_type = 11;
    src.params.trailer = LightingTrailer::MultiLayer {
        inner_thickness: 0.3,
        refraction_scale: 0.4,
        inner_texture_scale: [0.5, 0.6],
        envmap_strength: 0.7,
    };
    let (m, _, _) = patched(sim(&with(json!({"glint": {"screen_space_scale": 4.0}})), &sdc_default(), &src));
    assert_eq!(
        m.params.trailer,
        LightingTrailer::MultiLayer {
            inner_thickness: 4.0,
            refraction_scale: 0.4,
            inner_texture_scale: [0.5, 0.6],
            envmap_strength: 0.7,
        }
    );
    let (m, _, _) = patched(sim(&md(), &sdc_default(), &src));
    assert_eq!(m.shader_type, 0);
    assert_eq!(m.params.trailer, LightingTrailer::None);

    let mut env = rock();
    env.shader_type = 1;
    env.params.trailer = LightingTrailer::EnvMap { scale: 0.5 };
    let (m, _, _) = patched(sim(&md(), &sdc_default(), &env));
    assert_eq!(m.shader_type, 0);
    assert_eq!(m.params.trailer, LightingTrailer::None);
}

#[test]
fn fuzz_defaults_and_values() {
    let (m, _, _) = patched(sim(&with(json!({"fuzz": {}})), &sdc_default(), &rock()));
    assert_eq!(m.params.trailer, mlp(0.0, 0.0, 0.0, 1.0));
    let e = with(json!({"fuzz": {"color": [0.13, 0.13, 0.13], "weight": 0.5, "texture": true}}));
    let (m, _, _) = patched(sim(&e, &sdc_default(), &rock()));
    assert_eq!(m.params.trailer, mlp(0.13, 0.13, 0.13, 0.5));
    assert_eq!(m.slots[6].raw, format!("{MP}_f.dds"));
}

#[test]
fn coat_keys_override_subsurface_fields_only_with_multilayer() {
    let e = with(json!({
        "multilayer": true, "subsurface_color": [0.1, 0.2, 0.3], "subsurface_opacity": 0.5,
        "coat_color": [0.7, 0.8, 0.9], "coat_strength": 0.25, "coat_roughness": 0.6,
        "coat_specular_level": 0.02, "inner_uv_scale": 2.0,
        "coat_diffuse": true, "coat_parallax": true, "coat_normal": true
    }));
    let (m, _, _) = patched(sim(&e, &sdc_default(), &rock()));
    assert_eq!(m.shader_type, 11);
    assert_eq!(m.params.specular_color, [f(0.7), f(0.8), f(0.9)]);
    assert_eq!(m.params.lighting_effect_1, f(0.25));
    assert_eq!(m.params.trailer, mlp(0.6, 0.02, 2.0, 2.0));
    for b in [23, 24, 25, 27, 30] {
        assert!(bit(m.slsf2, b), "SLSF2.{b}");
    }
    assert!(!bit(m.slsf2, 26));
    assert_eq!(m.slots[6].raw, format!("{MP}_cnr.dds"));
    assert_eq!(m.slots[7].raw, format!("{MP}_s.dds"));

    let mut no_ml = e.clone();
    no_ml.as_object_mut().expect("object").remove("multilayer");
    let (m, _, _) = patched(sim(&no_ml, &sdc_default(), &rock()));
    assert_eq!(m.shader_type, 0);
    assert_eq!(m.params.specular_color, [f(0.1), f(0.2), f(0.3)]);
    assert_eq!(m.params.lighting_effect_1, f(0.5));
    assert_eq!(m.params.trailer, LightingTrailer::None);
    for b in [24, 25, 27, 30] {
        assert!(!bit(m.slsf2, b), "SLSF2.{b}");
    }
    assert_eq!(m.slots[6].raw, format!("{MP}_cnr.dds"));
    assert_eq!(m.slots[7].raw, format!("{MP}_s.dds"));
}

#[test]
fn hair_sets_back_lighting_unless_multilayer_coat_normal_clears_it() {
    let (m, _, _) = patched(sim(&with(json!({"hair": true})), &sdc_default(), &rock()));
    assert_eq!(m.shader_type, 0);
    assert!(bit(m.slsf2, 27));
    let e = with(json!({"hair": true, "multilayer": true, "coat_normal": false}));
    let (m, _, _) = patched(sim(&e, &sdc_default(), &rock()));
    assert_eq!(m.shader_type, 11);
    assert!(!bit(m.slsf2, 27));
}

#[test]
fn emissive_color_needs_four_numbers_scale_and_uv_apply() {
    let src = rock();
    let e = with(json!({"emissive_color": [0.1, 0.2, 0.3], "emissive_scale": 2.0, "uv_scale": 3.0}));
    let (m, _, _) = patched(sim(&e, &sdc_default(), &src));
    assert_eq!(m.params.emissive_color, src.params.emissive_color);
    assert_eq!(m.params.emissive_multiple, f(2.0));
    assert_eq!(m.params.uv_scale, [f(3.0), f(3.0)]);
    let (m, _, _) = patched(sim(&with(json!({"emissive_color": [0.1, 0.2, 0.3, 1.0]})), &sdc_default(), &src));
    assert_eq!(m.params.emissive_color, [f(0.1), f(0.2), f(0.3)]);
}

#[test]
fn delete_hides_the_shape_and_skips_the_texture_check() {
    let src = rock();
    let e = with(json!({"delete": true, "roughness_scale": 0.1}));
    let (m, mp, badges) = patched(sim_with(&e, &sdc_default(), &src, &|_| false));
    let mut want = src.clone();
    want.hidden = true;
    assert_eq!(m, want);
    assert_eq!(mp, MP);
    assert!(badges.is_empty());
}

#[test]
fn missing_patched_texture_is_a_skip() {
    let rmaos = format!("{MP}_rmaos.dds");
    let r = sim_with(&md(), &sdc_default(), &rock(), &|p: &str| p != rmaos);
    assert_eq!(r, SimResult::Skip(format!("{rmaos} missing")));
}

#[test]
fn missing_untouched_source_slot_is_a_skip() {
    let x = "textures\\landscape\\rock01_x.dds";
    let src = shape(&[D, N, "", "", "", "", "", "", x]);
    let r = sim_with(&md(), &sdc_default(), &src, &|p: &str| p != x);
    assert_eq!(r, SimResult::Skip(format!("{x} missing")));
}

#[test]
fn badges_for_geometry_keys() {
    let mut src = rock();
    src.has_vertex_colors = true;
    let e = with(json!({"smooth_angle": 60, "auto_uv": 2.0, "vertex_color_lum_mult": 0.5, "vertex_color_sat_mult": 0.5}));
    let (_, _, badges) = patched(sim(&e, &sdc_default(), &src));
    assert_eq!(
        badges,
        vec![
            Badge::NotPreviewed("smooth_angle"),
            Badge::NotPreviewed("auto_uv"),
            Badge::NotPreviewed("vertex_color_lum_mult"),
            Badge::NotPreviewed("vertex_color_sat_mult"),
        ]
    );
    // Without vertex colours PGPatcher skips the multipliers, so nothing is missing from the preview.
    let mut plain = rock();
    plain.has_vertex_colors = false;
    let (_, _, badges) = patched(sim(&e, &sdc_default(), &plain));
    assert_eq!(badges, vec![Badge::NotPreviewed("smooth_angle"), Badge::NotPreviewed("auto_uv")]);
}

#[test]
fn pbr_false_applies_scalars_only() {
    let src = rock();
    let (m, mp, badges) = patched(sim(&with(json!({"pbr": false, "roughness_scale": 0.5})), &sdc_default(), &src));
    assert_eq!(mp, "");
    assert_eq!(badges, vec![Badge::NotPreviewed("pbr")]);
    assert!(!m.is_pbr());
    assert_eq!(m.slots, src.slots);
    assert_eq!(m.shader_type, 0);
    assert_eq!(m.params.specular_strength, f(0.5));
    assert_eq!(m.params.glossiness, f(0.04));
}

#[test]
fn env_mapping_only_without_pbr() {
    let e = with(json!({"pbr": false, "env_mapping": true, "env_map_scale": 0.5, "env_map_scale_mult": 3.0}));
    let (m, _, badges) = patched(sim(&e, &sdc_default(), &rock()));
    assert_eq!(m.shader_type, 1);
    assert!(bit(m.slsf1, 7));
    assert!(bit(m.slsf2, 27));
    assert_eq!(m.params.trailer, LightingTrailer::EnvMap { scale: f(0.5) * f(3.0) });
    assert_eq!(
        badges,
        vec![
            Badge::NotPreviewed("pbr"),
            Badge::NotPreviewed("env_mapping"),
            Badge::NotPreviewed("env_map_scale"),
            Badge::NotPreviewed("env_map_scale_mult"),
        ]
    );
    let (m, _, badges) = patched(sim(&with(json!({"env_mapping": true})), &sdc_default(), &rock()));
    assert_eq!(m.shader_type, 0);
    assert!(!bit(m.slsf1, 7));
    assert_eq!(badges, vec![Badge::NotPreviewed("env_mapping")]);
}

#[test]
fn no_match_and_nif_filter() {
    assert_eq!(sim(&json!({"match_diffuse": "other"}), &sdc_default(), &rock()), SimResult::NoMatch);
    assert_eq!(sim(&with(json!({"nif_filter": "clutter"})), &sdc_default(), &rock()), SimResult::NoMatch);
}

#[test]
fn facegen_rgb_tint_is_a_skip() {
    let mut src = rock();
    src.slsf1 |= 1 << 21;
    let r = sim(&md(), &sdc_default(), &src);
    assert!(matches!(&r, SimResult::Skip(s) if s.contains("SLSF1.21")), "{r:?}");
}

#[test]
fn wrong_json_type_is_a_skip() {
    let r = sim(&with(json!({"emissive": "yes"})), &sdc_default(), &rock());
    assert!(matches!(&r, SimResult::Skip(s) if s.contains("\"emissive\"")), "{r:?}");
    let r = sim(&with(json!({"fuzz": {"color": [1, 1]}})), &sdc_default(), &rock());
    assert!(matches!(&r, SimResult::Skip(s) if s.contains("fuzz.color")), "{r:?}");
}
```

- [ ] **Step 3: Run the tests to verify they fail**

Run: `cargo test -p pgpsim`
Expected: FAIL to compile in every integration test (they all `mod common;`): `error[E0432]: unresolved imports pgpsim::simulate, pgpsim::Badge, pgpsim::SimInput, pgpsim::SimResult`.

- [ ] **Step 4: Implement**

Replace `crates/pgpsim/src/json.rs` with:

```rust
//! JSON accessors with nlohmann's semantics for well-typed values. `simulate` rejects wrong types first
//! (`check.rs`), so these never see one there.
use serde_json::{Map, Value};

/// The glint sub-keys, in MLP-float order (thickness, refraction, texture scale u, v) [TP:1077-1096].
pub(crate) const GLINT_KEYS: [&str; 4] = [
    "screen_space_scale",
    "log_microfacet_density",
    "microfacet_roughness",
    "density_randomization",
];

/// PGPatcher's `flag(json, key)`: present and `true`.
pub(crate) fn flag(e: &Map<String, Value>, k: &str) -> bool {
    e.get(k).and_then(Value::as_bool) == Some(true)
}

pub(crate) fn get_bool(e: &Map<String, Value>, k: &str) -> Option<bool> {
    e.get(k).and_then(Value::as_bool)
}

/// nlohmann `get<float>()`: any number, and booleans as 0/1.
pub(crate) fn num(v: &Value) -> Option<f32> {
    match v {
        Value::Bool(b) => Some(if *b { 1.0 } else { 0.0 }),
        _ => v.as_f64().map(|x| x as f32),
    }
}

pub(crate) fn get_num(e: &Map<String, Value>, k: &str) -> Option<f32> {
    e.get(k).and_then(num)
}

/// The first three numbers of an array with at least `min_len` (>= 3) entries (`size() >= n` in the C++).
pub(crate) fn rgb(e: &Map<String, Value>, k: &str, min_len: usize) -> Option<[f32; 3]> {
    let a = e.get(k)?.as_array()?;
    if a.len() < min_len.max(3) {
        return None;
    }
    Some([num(&a[0])?, num(&a[1])?, num(&a[2])?])
}
```

`crates/pgpsim/src/check.rs`:

```rust
//! PGPatcher 1.2.0 reads keys with nlohmann `get<T>()`, which throws on a wrong JSON type inside the per-shape
//! patch (01 §2.4). The sim reports that as a `Skip` instead of guessing. Over-approximation: a wrong-typed
//! known key is rejected even on a code path PGPatcher would not reach. A non-object `glint`/`fuzz` is NOT an
//! error (the C++ only calls `contains` on it) and a too-short colour array is ignored (`size() >= n`).
use crate::json::{num, GLINT_KEYS};
use serde_json::{Map, Value};

const BOOL_KEYS: &[&str] = &[
    "delete",
    "vertex_colors",
    "zbuffer_write",
    "emissive",
    "parallax",
    "subsurface",
    "subsurface_foliage",
    "hair",
    "multilayer",
    "coat_diffuse",
    "coat_parallax",
    "coat_normal",
    "lock_diffuse",
    "lock_normal",
    "lock_emissive",
    "lock_parallax",
    "lock_cubemap",
    "lock_rmaos",
    "lock_cnr",
    "lock_subsurface",
    "pbr",
    "env_mapping",
];

const NUM_KEYS: &[&str] = &[
    "smooth_angle",
    "auto_uv",
    "vertex_color_lum_mult",
    "vertex_color_sat_mult",
    "specular_level",
    "roughness_scale",
    "subsurface_opacity",
    "displacement_scale",
    "env_map_scale",
    "env_map_scale_mult",
    "emissive_scale",
    "uv_scale",
    "coat_specular_level",
    "coat_roughness",
    "coat_strength",
    "inner_uv_scale",
];

const STR_KEYS: &[&str] = &[
    "texture",
    "match_normal",
    "match_diffuse",
    "rename",
    "path_contains",
    "nif_filter",
    "cubemap",
    "match1",
    "match2",
    "match3",
    "match4",
    "match5",
    "match6",
    "match7",
    "match8",
    "slot1",
    "slot2",
    "slot3",
    "slot4",
    "slot5",
    "slot6",
    "slot7",
    "slot8",
];

/// Colour keys and the length from which PGPatcher reads them.
const COLOUR_KEYS: &[(&str, usize)] = &[("subsurface_color", 3), ("emissive_color", 4), ("coat_color", 3)];

fn bad(k: &str, want: &str) -> Result<(), String> {
    Err(format!("PGPatcher 1.2.0 would fail on \"{k}\": expected {want}"))
}

pub(crate) fn check_types(e: &Map<String, Value>) -> Result<(), String> {
    for k in BOOL_KEYS {
        if e.get(*k).is_some_and(|v| !v.is_boolean()) {
            return bad(k, "true or false");
        }
    }
    for k in NUM_KEYS {
        if e.get(*k).is_some_and(|v| num(v).is_none()) {
            return bad(k, "a number");
        }
    }
    for k in STR_KEYS {
        if e.get(*k).is_some_and(|v| !v.is_string()) {
            return bad(k, "a string");
        }
    }
    for (k, n) in COLOUR_KEYS {
        let Some(a) = e.get(*k).and_then(Value::as_array) else {
            continue;
        };
        if a.len() >= *n && a[..*n].iter().any(|v| num(v).is_none()) {
            return bad(k, "numbers");
        }
    }
    if let Some(g) = e.get("glint").and_then(Value::as_object) {
        for k in GLINT_KEYS {
            if g.get(k).is_some_and(|v| num(v).is_none()) {
                return bad(&format!("glint.{k}"), "a number");
            }
        }
    }
    if let Some(z) = e.get("fuzz").and_then(Value::as_object) {
        if z.get("texture").is_some_and(|v| !v.is_boolean()) {
            return bad("fuzz.texture", "true or false");
        }
        if z.get("weight").is_some_and(|v| num(v).is_none()) {
            return bad("fuzz.weight", "a number");
        }
        let colour_ok = |c: &Value| {
            c.as_array()
                .is_some_and(|a| a.len() >= 3 && a.iter().all(|v| num(v).is_some()))
        };
        if z.get("color").is_some_and(|c| !colour_ok(c)) {
            return bad("fuzz.color", "3 or more numbers");
        }
    }
    Ok(())
}
```

`crates/pgpsim/src/patch.rs`:

```rust
//! The patch itself, `applyOnePatch` + `enableTruePBROnShape` [Src 1.2.0 TP:662-1142; 01 §2.1-§2.3], on a
//! `MaterialState`, plus the per-file checks of 01 §1.4.
use crate::check::check_types;
use crate::json::{flag, get_bool, get_num, rgb, GLINT_KEYS};
use crate::matching::pg_slots;
use crate::slots::{apply_slots, first_missing};
use crate::{Badge, MaterialState, SimResult};
use nif::props::LightingTrailer;
use serde_json::{Map, Value};

// nifly SkyrimShaderPropertyFlags1/2 bit numbers (nifly include/Shaders.hpp:50-116).
const SLSF1_ENVMAP: u32 = 7;
const SLSF1_FACEGEN_DETAIL: u32 = 10;
const SLSF1_PARALLAX: u32 = 11;
const SLSF1_EYE_ENVMAP: u32 = 17;
const SLSF1_HAIR_SOFT: u32 = 18;
const SLSF1_FACEGEN_RGB_TINT: u32 = 21;
const SLSF1_EXTERNAL_EMITTANCE: u32 = 29;
const SLSF2_ZBUFFER_WRITE: u32 = 0;
const SLSF2_VERTEX_COLORS: u32 = 5;
const SLSF2_GLOW_MAP: u32 = 6;
const SLSF2_FIT_SLOPE: u32 = 12;
/// `SLSF2_UNUSED01`, the TruePBR marker.
const SLSF2_PBR: u32 = 23;
const SLSF2_MLP: u32 = 24;
const SLSF2_SOFT: u32 = 25;
const SLSF2_RIM: u32 = 26;
const SLSF2_BACK: u32 = 27;
const SLSF2_EFFECT_LIGHTING: u32 = 30;

const TYPE_DEFAULT: u32 = 0;
const TYPE_ENVMAP: u32 = 1;
const TYPE_MLP: u32 = 11;

/// Keys badged "not previewed" (spec §2.1), in display order.
const BADGE_KEYS: [&str; 8] = [
    "smooth_angle",
    "auto_uv",
    "vertex_color_lum_mult",
    "vertex_color_sat_mult",
    "pbr",
    "env_mapping",
    "env_map_scale",
    "env_map_scale_mult",
];

fn set_bit(word: &mut u32, bit: u32, on: bool) {
    if on {
        *word |= 1 << bit;
    } else {
        *word &= !(1 << bit);
    }
}

/// The shape as PGPatcher holds it in memory while it applies one file's entries.
struct Sim {
    m: MaterialState,
    /// PGPatcher's lower-cased slot strings (01 §1.3).
    slots: [String; 9],
    /// nifly keeps the MLP floats (thickness, refraction, texture scale u, v) and the envmap strength in memory
    /// whatever the type; the NIF gets them only for type 11 (01 §2.3 notes). Defaults: Shaders.hpp:699-702.
    mlp: [f32; 4],
    mlp_envmap: f32,
    /// nifly `environmentMapScale` (default 1.0); the NIF gets it only for type 1.
    env_scale: f32,
    src_type: u32,
    src_trailer: LightingTrailer,
    /// Some entry wrote the PBR slots, so PGPatcher writes back its (lower-cased) slot set.
    pbr_written: bool,
    badges: Vec<Badge>,
}

impl Sim {
    fn new(source: &MaterialState) -> Sim {
        let (mlp, mlp_envmap) = match &source.params.trailer {
            LightingTrailer::MultiLayer {
                inner_thickness,
                refraction_scale,
                inner_texture_scale,
                envmap_strength,
            } => (
                [*inner_thickness, *refraction_scale, inner_texture_scale[0], inner_texture_scale[1]],
                *envmap_strength,
            ),
            _ => ([0.0, 1.0, 1.0, 1.0], 1.0),
        };
        let env_scale = match &source.params.trailer {
            LightingTrailer::EnvMap { scale } => *scale,
            _ => 1.0,
        };
        Sim {
            m: source.clone(),
            slots: pg_slots(&source.slots),
            mlp,
            mlp_envmap,
            env_scale,
            src_type: source.shader_type,
            src_trailer: source.params.trailer.clone(),
            pbr_written: false,
            badges: Vec::new(),
        }
    }

    fn f1(&mut self, bit: u32, on: bool) {
        set_bit(&mut self.m.slsf1, bit, on);
    }

    fn f2(&mut self, bit: u32, on: bool) {
        set_bit(&mut self.m.slsf2, bit, on);
    }

    /// One entry, `applyOnePatch` [TP:662-863]. Returns true when the entry deleted the shape.
    fn apply_one(&mut self, e: &Map<String, Value>, mp: &str) -> bool {
        let pbr = !mp.is_empty();
        if flag(e, "delete") {
            self.m.hidden = true;
            return true;
        }
        // Vertex-colour multipliers run only on shapes that have vertex colours (after `vertex_colors`).
        let vc_after = get_bool(e, "vertex_colors").unwrap_or(self.m.has_vertex_colors);
        for k in BADGE_KEYS {
            let Some(v) = e.get(k) else {
                continue;
            };
            let shown = match k {
                "vertex_color_lum_mult" | "vertex_color_sat_mult" => vc_after,
                "pbr" => v.as_bool() != Some(true),
                _ => true,
            };
            let b = Badge::NotPreviewed(k);
            if shown && !self.badges.contains(&b) {
                self.badges.push(b);
            }
        }
        if let Some(vc) = get_bool(e, "vertex_colors") {
            self.m.has_vertex_colors = vc;
            self.f2(SLSF2_VERTEX_COLORS, vc);
        }
        if let Some(z) = get_bool(e, "zbuffer_write") {
            self.f2(SLSF2_ZBUFFER_WRITE, z);
        }
        if let Some(v) = get_num(e, "specular_level") {
            self.m.params.glossiness = v;
        }
        if let Some(c) = rgb(e, "subsurface_color", 3) {
            self.m.params.specular_color = c;
        }
        if let Some(v) = get_num(e, "roughness_scale") {
            self.m.params.specular_strength = v;
        }
        if let Some(v) = get_num(e, "subsurface_opacity") {
            self.m.params.lighting_effect_1 = v;
        }
        if let Some(v) = get_num(e, "displacement_scale") {
            self.m.params.lighting_effect_2 = v;
        }
        if flag(e, "env_mapping") && !pbr {
            self.m.shader_type = TYPE_ENVMAP;
            self.f1(SLSF1_ENVMAP, true);
            self.f2(SLSF2_BACK, true);
            if let Some(v) = get_num(e, "env_map_scale") {
                self.env_scale = v;
            }
            if let Some(v) = get_num(e, "env_map_scale_mult") {
                self.env_scale *= v;
            }
        }
        if let Some(v) = get_num(e, "emissive_scale") {
            self.m.params.emissive_multiple = v;
        }
        // Read only with >= 4 numbers; sdc writes RGBA. The NIF keeps RGB.
        if let Some(c) = rgb(e, "emissive_color", 4) {
            self.m.params.emissive_color = c;
        }
        if let Some(v) = get_num(e, "uv_scale") {
            self.m.params.uv_scale = [v, v];
        }
        if pbr {
            self.enable_pbr(e, mp);
        }
        false
    }

    /// `enableTruePBROnShape` [TP:967-1142]. Runs after the scalars, so coat_* overrides subsurface_*.
    fn enable_pbr(&mut self, e: &Map<String, Value>, mp: &str) {
        self.pbr_written = true;
        apply_slots(&mut self.slots, e, mp);
        if let Some(v) = get_bool(e, "emissive") {
            self.f1(SLSF1_EXTERNAL_EMITTANCE, v);
        }
        self.f1(SLSF1_ENVMAP, false);
        self.f1(SLSF1_HAIR_SOFT, false);
        self.f1(SLSF1_PARALLAX, false);
        self.f2(SLSF2_GLOW_MAP, false);
        self.f1(SLSF1_FACEGEN_DETAIL, false);
        self.f2(SLSF2_PBR, true);
        self.f1(SLSF1_EYE_ENVMAP, false);
        if let Some(v) = get_bool(e, "subsurface") {
            self.f2(SLSF2_RIM, v);
        }
        let hair = flag(e, "hair");
        if hair {
            self.f2(SLSF2_BACK, true);
        }
        let multilayer = flag(e, "multilayer");
        if multilayer {
            self.m.shader_type = TYPE_MLP;
            self.f2(SLSF2_MLP, true);
            if let Some(c) = rgb(e, "coat_color", 3) {
                self.m.params.specular_color = c;
            }
            if let Some(v) = get_num(e, "coat_specular_level") {
                self.mlp[1] = v;
            }
            if let Some(v) = get_num(e, "coat_roughness") {
                self.mlp[0] = v;
            }
            if let Some(v) = get_num(e, "coat_strength") {
                self.m.params.lighting_effect_1 = v;
            }
            if let Some(v) = get_bool(e, "coat_diffuse") {
                self.f2(SLSF2_EFFECT_LIGHTING, v);
            }
            if let Some(v) = get_bool(e, "coat_parallax") {
                self.f2(SLSF2_SOFT, v);
            }
            if let Some(v) = get_bool(e, "coat_normal") {
                self.f2(SLSF2_BACK, v);
            }
            if let Some(v) = get_num(e, "inner_uv_scale") {
                self.mlp[2] = v;
                self.mlp[3] = v;
            }
        } else if let Some(glint) = e.get("glint") {
            self.m.shader_type = TYPE_MLP;
            self.f2(SLSF2_FIT_SLOPE, true);
            if let Some(g) = glint.as_object() {
                for (slot, k) in self.mlp.iter_mut().zip(GLINT_KEYS) {
                    if let Some(v) = get_num(g, k) {
                        *slot = v;
                    }
                }
            }
        } else if let Some(fuzz) = e.get("fuzz") {
            self.m.shader_type = TYPE_MLP;
            self.f2(SLSF2_SOFT, true);
            let z = fuzz.as_object();
            let [r, g, b] = z.and_then(|z| rgb(z, "color", 3)).unwrap_or([0.0; 3]);
            let w = z.and_then(|z| get_num(z, "weight")).unwrap_or(1.0);
            self.mlp = [r, g, b, w];
        } else {
            self.m.shader_type = TYPE_DEFAULT;
        }
        if !multilayer {
            self.f2(SLSF2_MLP, false);
            if !hair {
                self.f2(SLSF2_BACK, false);
            }
            if !e.contains_key("fuzz") {
                self.f2(SLSF2_SOFT, false);
            }
        }
    }

    fn finish(mut self) -> (MaterialState, Vec<Badge>) {
        self.m.params.trailer = match self.m.shader_type {
            TYPE_MLP => LightingTrailer::MultiLayer {
                inner_thickness: self.mlp[0],
                refraction_scale: self.mlp[1],
                inner_texture_scale: [self.mlp[2], self.mlp[3]],
                envmap_strength: self.mlp_envmap,
            },
            TYPE_ENVMAP => LightingTrailer::EnvMap {
                scale: self.env_scale,
            },
            t if t == self.src_type => self.src_trailer.clone(),
            _ => LightingTrailer::None,
        };
        if self.pbr_written {
            for (i, s) in self.slots.iter().enumerate() {
                self.m.set_slot_raw(i, s);
            }
        }
        (self.m, self.badges)
    }
}

/// The core of `simulate` and `simulate_file`. `matches` = every matching prepared entry of ONE PGPatcher file
/// with its `matchedPath`, in index order (01 §1.4-§1.5: all apply, the last one wins per field).
pub(crate) fn run(
    matches: &[(&Map<String, Value>, String)],
    source: &MaterialState,
    exists: &dyn Fn(&str) -> bool,
) -> SimResult {
    if matches.is_empty() {
        return SimResult::NoMatch;
    }
    // 1.2.0 canApply [TP:230-243]; the GRASS-use half is plugin-side.
    if source.slsf1 & (1 << SLSF1_FACEGEN_RGB_TINT) != 0 {
        return SimResult::Skip("PGPatcher 1.2.0 skips shapes with SLSF1.21 (FaceGen RGB tint)".to_string());
    }
    for (e, _) in matches {
        if let Err(msg) = check_types(e) {
            return SimResult::Skip(msg);
        }
    }
    let mut sim = Sim::new(source);
    // 01 §1.4: the slot pass of all entries, then every non-empty slot must exist; `delete` skips the check.
    if !matches.iter().any(|(e, _)| flag(e, "delete")) {
        let mut slots = sim.slots.clone();
        for (e, mp) in matches {
            apply_slots(&mut slots, e, mp);
        }
        if let Some(p) = first_missing(&slots, exists) {
            return SimResult::Skip(format!("{p} missing"));
        }
    }
    let mut matched_path = String::new();
    for (e, mp) in matches {
        matched_path.clone_from(mp);
        if sim.apply_one(e, mp) {
            break;
        }
    }
    let (material, badges) = sim.finish();
    SimResult::Patched {
        material,
        matched_path,
        badges,
    }
}
```

Replace `crates/pgpsim/src/lib.rs` with:

```rust
//! PGPatcher 1.2.0 TruePBR patch, simulated in memory for the 3D preview (P1 spec §2.1).
//! Rules: `F:\ai\dds\nifparseref\plan\p1\01-pgpatcher-truepbr-rules.md` ("01"). Pure: no GPU, no egui, no IO.
//! It stops at "the NIF as PGPatcher leaves it", a `MaterialState`; CS's load-time remap belongs to render3d.
mod check;
mod json;
mod matching;
mod patch;
mod prep;
mod slots;
mod text;

pub use matching::{matched_path, shape_matches};
pub use nif::material::MaterialState;
pub use prep::prepare;
use serde_json::Value;

pub struct SimInput<'a> {
    /// One PGPatcher entry exactly as sdc writes it (`pgp::writer::entry_node(..).to_value()`).
    pub entry: &'a Value,
    /// The file's `default` object (`pgp::writer::document(&[]).to_value()["default"]`), or `Value::Null`.
    pub default: &'a Value,
    /// Mesh path relative to Data, PGPatcher form (`meshes\…`, any case).
    pub nif_path: &'a str,
    pub source: &'a MaterialState,
    /// Does a texture exist in the VFS / will sdc produce it? Called with lower-cased PGPatcher-form paths for
    /// EVERY non-empty slot after the slot pass (01 §1.4, TP:348-375): the new `textures\pbr\…` paths, `slotN`
    /// overrides, the cubemap, and untouched source slots (which may live in BSAs).
    pub exists: &'a dyn Fn(&str) -> bool,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Badge {
    /// A key the preview does not show (geometry ops, 1.2.0-only keys); the value is the key name.
    NotPreviewed(&'static str),
}

#[derive(Debug, Clone, PartialEq)]
pub enum SimResult {
    NoMatch,
    /// `matched_path` is "" for `pbr:false` (scalars only). `material.hidden` is set by `delete`.
    Patched {
        material: MaterialState,
        matched_path: String,
        badges: Vec<Badge>,
    },
    /// PGPatcher would skip this shape for this file; the shape keeps `source`.
    Skip(String),
}

/// One entry (sdc's case): prepare, match (with `nif_filter`), then patch.
pub fn simulate(i: &SimInput) -> SimResult {
    let prepared = prepare(i.entry, i.default);
    if !shape_matches(&prepared, i.nif_path, &i.source.slots) {
        return SimResult::NoMatch;
    }
    match (prepared.as_object(), matched_path(&prepared, &i.source.slots)) {
        (Some(e), Some(mp)) => patch::run(&[(e, mp)], i.source, i.exists),
        _ => SimResult::NoMatch,
    }
}
```

- [ ] **Step 5: Run the tests to verify they pass**

Run: `cargo test -p pgpsim`
Expected: PASS: `tests/patch.rs` (22 tests), plus every earlier test.

- [ ] **Step 6: Gates**

Run: `cargo fmt --all && cargo clippy --workspace --all-targets -- -D warnings && cargo test --workspace`
Expected: all green.

- [ ] **Step 7: Commit**

```bash
git add crates/pgpsim/src crates/pgpsim/tests
git commit -m "feat(pgpsim): simulate one entry: flags, type, scalars, leaks, badges, delete, skips (01 §2)" -m "Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

---

### Task 5: pgpsim `simulate_file` + golden tests (`SDC_ACHERUS`)

**Files:**
- Modify: `crates/pgpsim/src/lib.rs` (add `simulate_file`)
- Create: `crates/pgpsim/tests/file.rs`
- Create: `crates/pgpsim/tests/golden.rs`

**Interfaces:**
- Consumes: Task 4 `patch::run`, `SimInput`, `SimResult`, `simulate`; Task 2/3 `prepare`, `shape_matches`, `matched_path`; Task 1 `MaterialState::from_lighting(&ShapeHead, &Lighting, &[TexPath], Option<Alpha>)`; `nif::scan::{scan, Shader}`, `nif::scan::ShapeRow { head, shader, alpha }`.
- Produces (contract): `pub fn simulate_file(entries: &[Value], default: &Value, nif_path: &str, source: &MaterialState, exists: &dyn Fn(&str) -> bool) -> SimResult`. It applies all matching entries in index order (01 §1.5). `matched_path` = the last applied entry's.

- [ ] **Step 1: Write the failing unit tests**

`crates/pgpsim/tests/file.rs`:

```rust
//! 01 §1.4-§1.5 within one file: every matching entry applies in index order; the last one wins.
mod common;
use common::*;
use nif::material::MaterialState;
use pgpsim::{simulate, simulate_file, Badge, SimInput, SimResult};
use serde_json::{json, Value};

fn file(entries: &[Value], src: &MaterialState, exists: &dyn Fn(&str) -> bool) -> SimResult {
    simulate_file(entries, &sdc_default(), NIF, src, exists)
}

#[test]
fn later_entry_wins_and_resets_defaults() {
    let entries = [
        json!({"match_diffuse": "rock01_d", "emissive": true, "roughness_scale": 0.5}),
        json!({"path_contains": "landscape", "roughness_scale": 0.75}),
    ];
    let (m, mp, _) = patched(file(&entries, &rock(), &|_| true));
    assert_eq!(mp, MP);
    assert_eq!(m.params.specular_strength, f(0.75));
    // The second entry carries sdc's emissive:false from `default`.
    assert_eq!(m.slots[2].raw, "");
    assert!(!bit(m.slsf1, 29));
}

#[test]
fn non_matching_entries_are_ignored() {
    let entries = [
        json!({"match_diffuse": "other", "roughness_scale": 0.1}),
        json!({"match_diffuse": "rock01_d", "nif_filter": "clutter", "roughness_scale": 0.2}),
        json!({"match_diffuse": "rock01_d", "roughness_scale": 0.5}),
    ];
    let (m, _, _) = patched(file(&entries, &rock(), &|_| true));
    assert_eq!(m.params.specular_strength, f(0.5));
    assert_eq!(file(&entries[..2], &rock(), &|_| true), SimResult::NoMatch);
}

#[test]
fn a_lock_keeps_the_earlier_entrys_slot() {
    let entries = [
        json!({"match_diffuse": "rock01_d"}),
        json!({"match_diffuse": "rock01_d", "rename": "snow01", "lock_diffuse": true}),
    ];
    let (m, mp, _) = patched(file(&entries, &rock(), &|_| true));
    assert_eq!(mp, "textures\\pbr\\landscape\\snow01");
    assert_eq!(m.slots[0].raw, format!("{MP}.dds"));
    assert_eq!(m.slots[1].raw, "textures\\pbr\\landscape\\snow01_n.dds");
}

#[test]
fn the_texture_check_sees_the_final_slots() {
    let on = json!({"match_diffuse": "rock01_d", "parallax": true});
    let off = json!({"match_diffuse": "rock01_d", "parallax": false});
    let no_height = |p: &str| !p.ends_with("_p.dds");
    let (m, _, _) = patched(file(&[on.clone(), off.clone()], &rock(), &no_height));
    assert_eq!(m.slots[3].raw, "");
    assert_eq!(file(&[off, on], &rock(), &no_height), SimResult::Skip(format!("{MP}_p.dds missing")));
}

#[test]
fn delete_in_a_later_entry_hides_after_the_earlier_ones_applied() {
    let entries = [
        json!({"match_diffuse": "rock01_d", "roughness_scale": 0.5}),
        json!({"match_diffuse": "rock01_d", "delete": true}),
    ];
    let (m, _, _) = patched(file(&entries, &rock(), &|_| false));
    assert!(m.hidden);
    assert_eq!(m.params.specular_strength, f(0.5));
}

#[test]
fn mlp_floats_persist_across_entries_like_nifly_memory() {
    let entries = [
        json!({"match_diffuse": "rock01_d", "glint": {"screen_space_scale": 4.0}}),
        json!({"match_diffuse": "rock01_d"}),
        json!({"match_diffuse": "rock01_d", "glint": {"log_microfacet_density": 20.0}}),
    ];
    let (m, _, _) = patched(file(&entries, &rock(), &|_| true));
    assert_eq!(m.shader_type, 11);
    assert_eq!(m.params.trailer, mlp(4.0, 20.0, 1.0, 1.0));
}

#[test]
fn badges_are_listed_once() {
    let entries = [
        json!({"match_diffuse": "rock01_d", "smooth_angle": 60}),
        json!({"match_diffuse": "rock01_d", "smooth_angle": 75}),
    ];
    let (_, _, badges) = patched(file(&entries, &rock(), &|_| true));
    assert_eq!(badges, vec![Badge::NotPreviewed("smooth_angle")]);
}

#[test]
fn one_entry_file_equals_simulate() {
    let e = json!({"match_diffuse": "rock01_d", "multilayer": true, "coat_strength": 0.5});
    let src = rock();
    let d = sdc_default();
    let one = simulate(&SimInput {
        entry: &e,
        default: &d,
        nif_path: NIF,
        source: &src,
        exists: &|_| true,
    });
    assert_eq!(one, simulate_file(&[e.clone()], &d, NIF, &src, &|_| true));
}
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo test -p pgpsim --test file`
Expected: FAIL to compile: `error[E0432]: unresolved import pgpsim::simulate_file`.

- [ ] **Step 3: Implement `simulate_file`**

In `crates/pgpsim/src/lib.rs`, change `use serde_json::Value;` to `use serde_json::{Map, Value};` and append:

```rust
/// A whole PGPatcher file (01 §1.4-§1.5): every entry is prepared with `default` (`Value::Null` for a
/// bare-array file); all entries matching this shape apply in index order, so the last one wins per field.
/// The §1.4 texture check runs over the slot pass of all of them. `matched_path` = the last applied entry's.
pub fn simulate_file(
    entries: &[Value],
    default: &Value,
    nif_path: &str,
    source: &MaterialState,
    exists: &dyn Fn(&str) -> bool,
) -> SimResult {
    let prepared: Vec<Value> = entries.iter().map(|e| prepare(e, default)).collect();
    let matches: Vec<(&Map<String, Value>, String)> = prepared
        .iter()
        .filter(|p| shape_matches(p, nif_path, &source.slots))
        .filter_map(|p| Some((p.as_object()?, matched_path(p, &source.slots)?)))
        .collect();
    patch::run(&matches, source, exists)
}
```

- [ ] **Step 4: Run the unit tests to verify they pass**

Run: `cargo test -p pgpsim --test file`
Expected: PASS (8 tests).

- [ ] **Step 5: Write the golden tests (opt-in, owner data, read-only)**

The nine cases are 01 §5.2. Paths are relative to `%SDC_ACHERUS%\mods\`: the source is `<source mod>\<nif>`, the output is `ParallaxGen_Output\<nif>`, and the JSON is `<json mod>\PBRNifPatcher\<file>`. All exist on the owner's instance (checked 2026-10-11). Python 01 §5.1 matched all nine with one matching entry each; the expected `matched_path` values come from that run (`pgp_tp_validate_out.json`).

`crates/pgpsim/tests/golden.rs`:

```rust
//! 01 §5.2 golden cases: source NIF + the winning PGPatcher JSON → `simulate_file` → compare with the shape in
//! `ParallaxGen_Output` on the 01 §5.1 field list. Opt-in and read-only on owner data:
//! `$env:SDC_ACHERUS='C:\_mount\moddingvol\Acherus'; cargo test -p pgpsim --test golden -- --ignored`
use nif::material::MaterialState;
use nif::props::LightingTrailer;
use nif::scan::{scan, Shader};
use pgpsim::{simulate_file, SimResult};
use serde_json::Value;
use std::path::Path;

struct Case {
    nif: &'static str,
    shape: &'static str,
    source_mod: &'static str,
    json_mod: &'static str,
    json: &'static str,
    matched: &'static str,
}

const PLAIN: Case = Case {
    nif: "meshes\\xmead\\reserve.nif",
    shape: "MeadSolid:3",
    source_mod: "Xocco3's Mead Replacer PBR",
    json_mod: "Xocco3's Mead Replacer PBR",
    json: "reserve.json",
    matched: "textures\\pbr\\xmead\\reserve_bc",
};
const EMISSIVE_PARALLAX: Case = Case {
    nif: "meshes\\clutter\\eyeofmagnus\\eyeofmagnus01.nif",
    shape: "SectionD10:0",
    source_mod: "Faultier's PBR Clutter",
    json_mod: "Faultier's PBR Clutter",
    json: "Faultier's PBR Clutter.json",
    matched: "textures\\pbr\\clutter\\eyeofmagnus",
};
const SUBSURFACE: Case = Case {
    nif: "meshes\\furniture\\firespitcooking.nif",
    shape: "PotOpen:18",
    source_mod: "Faultier's PBR Clutter",
    json_mod: "Faultier's PBR Clutter",
    json: "Faultier's PBR Clutter.json",
    matched: "textures\\pbr\\clutter\\common\\stew01",
};
const COAT: Case = Case {
    nif: "meshes\\dungeons\\caves\\ice\\largehall\\caveilhall1way128u01.nif",
    shape: "CaveILHall1Way128U01:6",
    source_mod: "Icy Cave Remaster - Ice Material Patch",
    json_mod: "Exist's Caves - PBR Retexture",
    json: "dungeonscaves.json",
    matched: "textures\\pbr\\dungeons\\caves\\icefrozen01",
};
const COAT_EMISSIVE_SLOTN: Case = Case {
    nif: "meshes\\dlc01\\traps\\lighttrap\\lighttrap01.nif",
    shape: "DawnStone01:0",
    source_mod: "Faultier's PBR Clutter",
    json_mod: "Faultier's PBR Clutter",
    json: "Faultier's PBR Clutter.json",
    matched: "textures\\pbr\\clutter\\dawnbreakerpedestal\\crystal",
};
const FUZZ_TEXTURE: Case = Case {
    nif: "meshes\\dlc01\\clothes\\vampire\\vampirerobesfkha_1.nif",
    shape: "robes_bodice",
    source_mod: "CBBE 3BA Vanilla Outfits Redone",
    json_mod: "Faultier's PBR Armors and Clothes",
    json: "Faultier's vampire set.json",
    matched: "textures\\pbr\\dlc01\\clothes\\vampire\\robesf",
};
const FUZZ_SLOT7: Case = Case {
    nif: "meshes\\tapestries\\smalltreetapestry.nif",
    shape: "Plane030",
    source_mod: "Faultier's PBR blue palace terrace",
    json_mod: "Faultier's PBR blue palace terrace",
    json: "Faultier's PBR blue palace terrace.json",
    matched: "textures\\pbr\\1_castle_1\\new folder\\tapestries\\smalltreetapestry1k",
};
const GLINT: Case = Case {
    nif: "meshes\\architecture\\sovengarde\\sovstairspiece01.nif",
    shape: "SOVStairsPiece01:0",
    source_mod: "Sovngarde Meshes - FIXES",
    json_mod: "Faultier's PBR Sovngarde",
    json: "Faultier's PBR aaSovngarde.json",
    matched: "textures\\pbr\\architecture\\sovngarde\\sovbasestone01",
};
const GLINT_RENAME: Case = Case {
    nif: "meshes\\nicola89b\\architecture\\windhelm\\windhelmbridge.nif",
    shape: "Arch_snow:0",
    source_mod: "Windhelm Bridge Reforged - The Citadel of Snow",
    json_mod: "Faultier's PBR Windhelm",
    json: "Faultier's PBR Windhelm.json",
    matched: "textures\\pbr\\landscape\\snow01",
};

/// The one lighting shape named `name` in the NIF at `path`, as a `MaterialState`.
fn shape_of(path: &Path, name: &str) -> MaterialState {
    let bytes = std::fs::read(path).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
    let s = scan(&bytes).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
    let found: Vec<MaterialState> = s
        .shapes
        .iter()
        .filter(|r| r.head.name.as_deref() == Some(name))
        .filter_map(|r| match &r.shader {
            Shader::Lighting(l, slots) => Some(MaterialState::from_lighting(&r.head, l, slots, r.alpha)),
            _ => None,
        })
        .collect();
    assert_eq!(found.len(), 1, "{}: shape {name} found {} times", path.display(), found.len());
    found.into_iter().next().expect("one shape")
}

/// A PGPatcher config file: an object needs both `default` and `entries`; a bare array has no default.
fn read_config(path: &Path) -> (Vec<Value>, Value) {
    let text = std::fs::read_to_string(path).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
    let text = text.strip_prefix('\u{feff}').unwrap_or(&text);
    let v: Value = serde_json::from_str(text).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
    match v {
        Value::Array(entries) => (entries, Value::Null),
        Value::Object(mut o) => {
            let default = o.remove("default").expect("an object config needs `default`");
            let Some(Value::Array(entries)) = o.remove("entries") else {
                panic!("{}: an object config needs an `entries` array", path.display());
            };
            (entries, default)
        }
        other => panic!("{}: not a PGPatcher config: {other}", path.display()),
    }
}

fn close(a: f32, b: f32) -> bool {
    (a - b).abs() <= 1e-4
}

fn mlp4(t: &LightingTrailer) -> Option<[f32; 4]> {
    match t {
        LightingTrailer::MultiLayer {
            inner_thickness,
            refraction_scale,
            inner_texture_scale,
            ..
        } => Some([*inner_thickness, *refraction_scale, inner_texture_scale[0], inner_texture_scale[1]]),
        _ => None,
    }
}

/// 01 §5.1 field list: type, SLSF1/2, the 9 slots (lower-cased), glossiness, specular strength, specular
/// colour, LE1, LE2, emissive colour, emissive multiple, UV scale, and the 4 MLP floats when the output is
/// type 11. Floats at 1e-4.
fn diff(sim: &MaterialState, out: &MaterialState) -> Vec<String> {
    let mut d = Vec::new();
    if sim.shader_type != out.shader_type {
        d.push(format!("type sim={} out={}", sim.shader_type, out.shader_type));
    }
    if sim.slsf1 != out.slsf1 {
        d.push(format!("slsf1 sim={:#010x} out={:#010x}", sim.slsf1, out.slsf1));
    }
    if sim.slsf2 != out.slsf2 {
        d.push(format!("slsf2 sim={:#010x} out={:#010x}", sim.slsf2, out.slsf2));
    }
    let low = |m: &MaterialState| -> Vec<String> { m.slots.iter().map(|t| t.raw.to_ascii_lowercase()).collect() };
    if low(sim) != low(out) {
        d.push(format!("slots sim={:?} out={:?}", low(sim), low(out)));
    }
    let (a, b) = (&sim.params, &out.params);
    let scalars = [
        ("glossiness", a.glossiness, b.glossiness),
        ("specular_strength", a.specular_strength, b.specular_strength),
        ("lighting_effect_1", a.lighting_effect_1, b.lighting_effect_1),
        ("lighting_effect_2", a.lighting_effect_2, b.lighting_effect_2),
        ("emissive_multiple", a.emissive_multiple, b.emissive_multiple),
    ];
    for (k, x, y) in scalars {
        if !close(x, y) {
            d.push(format!("{k} sim={x} out={y}"));
        }
    }
    let vectors: [(&str, &[f32], &[f32]); 3] = [
        ("specular_color", &a.specular_color, &b.specular_color),
        ("emissive_color", &a.emissive_color, &b.emissive_color),
        ("uv_scale", &a.uv_scale, &b.uv_scale),
    ];
    for (k, x, y) in vectors {
        if x.iter().zip(y).any(|(p, q)| !close(*p, *q)) {
            d.push(format!("{k} sim={x:?} out={y:?}"));
        }
    }
    if out.shader_type == 11 {
        match (mlp4(&a.trailer), mlp4(&b.trailer)) {
            (Some(x), Some(y)) if x.iter().zip(&y).all(|(p, q)| close(*p, *q)) => {}
            (x, y) => d.push(format!("mlp sim={x:?} out={y:?}")),
        }
    }
    d
}

fn check(c: &Case) {
    let Ok(root) = std::env::var("SDC_ACHERUS") else {
        eprintln!("SDC_ACHERUS not set; skipping {}", c.nif);
        return;
    };
    let mods = Path::new(&root).join("mods");
    let source = shape_of(&mods.join(c.source_mod).join(c.nif), c.shape);
    let out = shape_of(&mods.join("ParallaxGen_Output").join(c.nif), c.shape);
    let (entries, default) = read_config(&mods.join(c.json_mod).join("PBRNifPatcher").join(c.json));
    // The output NIF proves every texture existed when PGPatcher ran (01 §1.4), so no VFS lookup here.
    let r = simulate_file(&entries, &default, c.nif, &source, &|_| true);
    let SimResult::Patched {
        material,
        matched_path,
        ..
    } = &r
    else {
        panic!("{} / {}: expected Patched, got {r:?}", c.nif, c.shape);
    };
    assert_eq!(matched_path, c.matched, "{} / {}", c.nif, c.shape);
    let d = diff(material, &out);
    assert!(d.is_empty(), "{} / {}:\n{}", c.nif, c.shape, d.join("\n"));
}

#[test]
#[ignore]
fn golden_plain_reserve() {
    check(&PLAIN);
}

#[test]
#[ignore]
fn golden_emissive_parallax_eyeofmagnus() {
    check(&EMISSIVE_PARALLAX);
}

#[test]
#[ignore]
fn golden_subsurface_firespit() {
    check(&SUBSURFACE);
}

#[test]
#[ignore]
fn golden_coat_ice_cave() {
    check(&COAT);
}

#[test]
#[ignore]
fn golden_coat_emissive_slot_n_lighttrap() {
    check(&COAT_EMISSIVE_SLOTN);
}

#[test]
#[ignore]
fn golden_fuzz_texture_vampire_robes() {
    check(&FUZZ_TEXTURE);
}

#[test]
#[ignore]
fn golden_fuzz_slot7_tapestry() {
    check(&FUZZ_SLOT7);
}

#[test]
#[ignore]
fn golden_glint_sovngarde_stairs() {
    check(&GLINT);
}

#[test]
#[ignore]
fn golden_glint_rename_windhelm_bridge() {
    check(&GLINT_RENAME);
}
```

- [ ] **Step 6: Run the golden tests**

Run (PowerShell): `$env:SDC_ACHERUS='C:\_mount\moddingvol\Acherus'; cargo test -p pgpsim --test golden -- --ignored`
Expected: PASS, 9 tests. If one fails, its message lists the mismatching fields. Debug it against 01 §2 and the C++ (`scratchpad\pgp\v120\PatcherMeshShaderTruePBR.cpp`) before touching the test; the Python reference `scratchpad\scripts\pgp_tp_validate.py` reproduces all nine.
Then run without the variable: `Remove-Item Env:SDC_ACHERUS; cargo test -p pgpsim --test golden -- --ignored`. Expected: 9 pass, each printing "SDC_ACHERUS not set; skipping …".

- [ ] **Step 7: Gates**

Run: `cargo fmt --all && cargo clippy --workspace --all-targets -- -D warnings && cargo test --workspace`
Expected: all green (golden tests are ignored).

- [ ] **Step 8: Commit**

```bash
git add crates/pgpsim/src/lib.rs crates/pgpsim/tests/file.rs crates/pgpsim/tests/golden.rs
git commit -m "feat(pgpsim): simulate_file (last entry wins) and 01 §5.2 golden tests behind SDC_ACHERUS" -m "Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```
### Task 6: render3d `parse.rs` (`ShapeId`, `ParsedShape`, `ParsedNif`, `parse`)

**Files:**
- Create: `crates/render3d/src/parse.rs`
- Modify: `crates/render3d/src/lib.rs:7-10` (module list + re-export)
- Test: `crates/render3d/tests/parse.rs`

**Interfaces:**
- Consumes: Task 1 `nif::material::MaterialState::from_lighting`; `nif::scan::{scan, Shader, ShapeRow}` (`scan.rs:11-33, 69`); `nif::header::Header::parse`; `nif::geom::{read_shape, Mesh}` (`geom/bstrishape.rs:12`, `geom/mod.rs:13-50`); `render3d::mesh::validate` (`mesh.rs:19`, pure).
- Produces (contract, exact):
  ```rust
  #[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)] pub struct ShapeId(pub u32);
  #[derive(Debug, Clone)] pub struct ParsedShape { pub id: ShapeId, pub name: String, pub mesh: std::sync::Arc<nif::geom::Mesh>, pub material: nif::material::MaterialState }
  #[derive(Debug, Clone, Default)] pub struct ParsedNif { pub le_mesh: bool, pub shapes: Vec<ParsedShape>, pub skipped: Vec<(ShapeId, String)>, pub warnings: Vec<String>, pub errors: Vec<String> }
  pub fn parse(nif_bytes: &[u8]) -> ParsedNif;
  ```
  Re-exported as `render3d::{parse, ParsedNif, ParsedShape, ShapeId}`. Every shape in `shapes` passed `mesh::validate`; `name` is the NIF name or `"block N"` (P0's label); `warnings` / `errors` hold P0's exact report lines (`"NIF: …"`, `"{label}: not a BSLightingShaderProperty shape, not drawn in P0"`, `"{label}: {geometry error}"`).

- [ ] **Step 1: Write the failing test** — create `crates/render3d/tests/parse.rs`:

```rust
//! `parse`: pure NIF → per-shape data, keyed by block index.
use nif::scan::{scan, Shader};
use render3d::{parse, ParsedNif, ShapeId};

fn fixture(name: &str) -> Vec<u8> {
    std::fs::read(format!(
        "{}/../nif/tests/fixtures/{name}",
        env!("CARGO_MANIFEST_DIR")
    ))
    .unwrap()
}

#[test]
fn parsed_nif_is_send_and_sync() {
    fn check<T: Send + Sync + 'static>() {}
    check::<ParsedNif>();
}

#[test]
fn deep_graph_shapes_are_keyed_by_block() {
    let b = fixture("TestNifFile_DeepGraph_SE.nif");
    let p = parse(&b);
    assert!(!p.le_mesh);
    assert!(p.errors.is_empty(), "{:?}", p.errors);
    let s = scan(&b).unwrap();
    let lighting: Vec<u32> = s
        .shapes
        .iter()
        .filter(|r| matches!(r.shader, Shader::Lighting(..)))
        .map(|r| r.block)
        .collect();
    assert_eq!(lighting.len(), 27);
    assert_eq!(p.shapes.iter().map(|x| x.id.0).collect::<Vec<_>>(), lighting);
    assert_eq!(p.shapes.len() + p.skipped.len(), s.shapes.len());
    let skull = p.shapes.iter().find(|x| x.name == "SkullPeg01:19").unwrap();
    assert_eq!(skull.id, ShapeId(143));
    assert!(skull.material.alpha.is_none(), "the one shape without NiAlphaProperty");
    for x in &p.shapes {
        assert_eq!(x.material.slots.len(), nif::material::SLOT_COUNT);
        assert!(render3d::mesh::validate(&x.mesh).is_ok(), "{}", x.name);
        assert!(!x.material.hidden);
    }
}

#[test]
fn furniture_names_and_materials() {
    let b = fixture("TestNifFile_Furniture_Col_SE.nif");
    let p = parse(&b);
    let names: Vec<(u32, &str)> = p.shapes.iter().map(|x| (x.id.0, x.name.as_str())).collect();
    assert_eq!(names, [(9, "cylinder_1"), (12, "cylinder_2")]);
    let s = scan(&b).unwrap();
    for x in &p.shapes {
        let row = s.shapes.iter().find(|r| r.block == x.id.0).unwrap();
        let Shader::Lighting(l, slots) = &row.shader else {
            panic!("not lighting")
        };
        let want = nif::material::MaterialState::from_lighting(&row.head, l, slots, row.alpha);
        assert_eq!(x.material, want);
    }
}

#[test]
fn le_mesh_parses_to_nothing() {
    let p = parse(&fixture("TestNifFile_Animated_LE.nif"));
    assert!(p.le_mesh);
    assert!(p.shapes.is_empty());
    assert!(p.errors.is_empty(), "{:?}", p.errors);
}

#[test]
fn garbage_is_an_error_not_a_panic() {
    let p = parse(b"not a nif at all");
    assert!(p.shapes.is_empty());
    assert!(p.errors.iter().any(|e| e.starts_with("NIF: ")), "{:?}", p.errors);
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p render3d --test parse`
Expected: FAIL to compile, `unresolved imports render3d::parse, render3d::ParsedNif, render3d::ShapeId`.

- [ ] **Step 3: Write the implementation** — create `crates/render3d/src/parse.rs`:

```rust
//! NIF bytes → owned per-shape data (03 §1.1): pure, `Send`, no GPU, so a worker can parse while the render
//! thread keeps drawing. P1-A places each shape by its own transform (`Mesh.transform`) only; parent NiNode
//! transforms are P1-B.
use nif::material::MaterialState;
use std::sync::Arc;

/// Stable for the life of a load: the NIF block index of the shape (`nif::scan::ShapeRow.block`). Built-in
/// shapes use `ShapeId(0)` (block 0 is the root node, never a shape). Storage order is not draw order.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct ShapeId(pub u32);

/// One drawable shape: a BSLightingShaderProperty shape whose geometry decoded and passed `mesh::validate`.
#[derive(Debug, Clone)]
pub struct ParsedShape {
    pub id: ShapeId,
    /// The NIF name, or `"block N"` when it has none (P0's label).
    pub name: String,
    pub mesh: Arc<nif::geom::Mesh>,
    pub material: MaterialState,
}

/// A whole NIF, parsed. `skipped` lists every scanned shape that is not in `shapes`, with its reason; the same
/// text is also a `warnings` line (not a Lighting shape) or an `errors` line (geometry), exactly as P0's
/// `LoadReport` printed them, so a load can copy `warnings` / `errors` unchanged.
#[derive(Debug, Clone, Default)]
pub struct ParsedNif {
    /// BSVER 83: nothing is parsed further (LE meshes are not rendered, 00 §6.5).
    pub le_mesh: bool,
    pub shapes: Vec<ParsedShape>,
    pub skipped: Vec<(ShapeId, String)>,
    pub warnings: Vec<String>,
    pub errors: Vec<String>,
}

/// Scans the NIF and decodes the geometry of every BSLightingShaderProperty shape, in block order.
pub fn parse(nif_bytes: &[u8]) -> ParsedNif {
    let mut out = ParsedNif::default();
    let scan = match nif::scan::scan(nif_bytes) {
        Ok(s) => s,
        Err(e) => {
            out.errors.push(format!("NIF: {e}"));
            return out;
        }
    };
    out.warnings
        .extend(scan.problems.iter().map(|p| format!("NIF: {p}")));
    if scan.le_mesh {
        out.le_mesh = true;
        return out;
    }
    let header = match nif::header::Header::parse(nif_bytes) {
        Ok(h) => h,
        Err(e) => {
            out.errors.push(format!("NIF header: {e}"));
            return out;
        }
    };
    for row in &scan.shapes {
        let id = ShapeId(row.block);
        let label = row
            .head
            .name
            .clone()
            .unwrap_or_else(|| format!("block {}", row.block));
        let nif::scan::Shader::Lighting(lighting, slots) = &row.shader else {
            let why = format!("{label}: not a BSLightingShaderProperty shape, not drawn in P0");
            out.warnings.push(why.clone());
            out.skipped.push((id, why));
            continue;
        };
        let mesh = match nif::geom::read_shape(&header, nif_bytes, row.block as usize) {
            Ok(m) => m,
            Err(e) => {
                let why = format!("{label}: {e}");
                out.errors.push(why.clone());
                out.skipped.push((id, why));
                continue;
            }
        };
        if let Err(e) = crate::mesh::validate(&mesh) {
            let why = format!("{label}: {e}");
            out.errors.push(why.clone());
            out.skipped.push((id, why));
            continue;
        }
        out.shapes.push(ParsedShape {
            id,
            name: label,
            mesh: Arc::new(mesh),
            material: MaterialState::from_lighting(&row.head, lighting, slots, row.alpha),
        });
    }
    out
}
```

In `crates/render3d/src/lib.rs` replace lines 7-10:

```rust
pub mod mesh;
pub mod permutation;
pub mod scene;
pub mod textures;
```

with:

```rust
pub mod mesh;
pub mod parse;
pub mod permutation;
pub mod scene;
pub mod textures;

pub use parse::{parse, ParsedNif, ParsedShape, ShapeId};
```

(`parse` the module and `parse` the function live in different namespaces; both are reachable.)

- [ ] **Step 4: Run test to verify it passes**

Run: `cargo test -p render3d --test parse`
Expected: PASS, 5 tests.

- [ ] **Step 5: Gates**

Run: `cargo fmt --all && cargo clippy --workspace --all-targets -- -D warnings && cargo test --workspace`
Expected: all green (`load` is untouched in this task).

- [ ] **Step 6: Commit**

```bash
git add crates/render3d/src/parse.rs crates/render3d/src/lib.rs crates/render3d/tests/parse.rs
git commit -m "feat(render3d): pure parse step keyed by ShapeId (block index)" -m "Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

---
### Task 7: render3d `material.rs`: `pbr_flags`, `write_material(&MaterialState…)`, GLINT from the remap, D1 emissive

**Files:**
- Create: `crates/render3d/src/material.rs`
- Modify: `crates/render3d/src/cbuffer.rs:66-68` (add `CbWriter::zero` after `bytes`)
- Modify: `crates/render3d/src/draw.rs:5-11` (imports) and delete `draw.rs:17-187` (`pack_specular`, `pbr_params`, `MaterialInputs`, `write_material` move to `material.rs`)
- Modify: `crates/render3d/src/permutation.rs:394-396` (GLINT)
- Modify: `crates/render3d/src/lib.rs` (module list, imports, the material lines of `load`; anchors below)
- Modify: `crates/render3d/tests/compile.rs:77-83` (GLINT assertions follow the ruling)
- Test: `crates/render3d/tests/material.rs`

**Interfaces:**
- Consumes: Task 1 `MaterialState` (`is_pbr`, `blend`, fields); `permutation::{defines, runtime_bits, Material, Stage}` (`permutation.rs:7-17, 66-132, 138`); `scene::srgb_to_linear` (`scene.rs:143`); `CbWriter::{set, set_u32}` (`cbuffer.rs:73-80`).
- Produces:
  ```rust
  // render3d::material
  #[derive(Debug, Clone, Default, PartialEq, Eq)] pub struct Bound { pub regs: std::collections::HashSet<u32> }
  pub const PBR_HAS_EMISSIVE: u32 = 1; /* … */ pub const PBR_GLINT: u32 = 2048;   // 12 consts, 01 §3.4
  pub fn remapped_glint(slsf2: u32) -> bool;
  pub fn pbr_flags(m: &MaterialState, bound: &Bound) -> u32;
  pub fn perm_material(m: &MaterialState) -> crate::permutation::Material;
  pub fn pack_specular(p: &LightingParams, linear_lighting: bool) -> [f32; 4];
  pub fn pbr_params(p: &LightingParams) -> ([f32; 3], [f32; 4]);
  pub fn write_material(w: &mut CbWriter, m: &MaterialState, bound: &Bound, linear_lighting: bool);
  // render3d::cbuffer
  impl CbWriter { pub fn zero(&mut self); }
  ```
  Re-exported: `render3d::{pbr_flags, Bound}`. `write_material` expects a zero writer (fresh from `CbWriter::new`, or after `zero()`); it never zeroes itself, because the scene block shares the writer.

- [ ] **Step 1: Write the failing tests** — create `crates/render3d/tests/material.rs`:

```rust
//! CS's TruePBR load remap (01 §3.1-3.4) and the material cbuffer fields, writer-level (no GPU).
use nif::material::MaterialState;
use nif::props::LightingTrailer;
use render3d::cbuffer::CbWriter;
use render3d::compile::CbLayout;
use render3d::material::{
    pbr_flags, perm_material, remapped_glint, write_material, Bound, PBR_COAT_NORMAL,
    PBR_COLORED_COAT, PBR_FUZZ, PBR_GLINT, PBR_HAIR_MARSCHNER, PBR_HAS_DISPLACEMENT,
    PBR_HAS_EMISSIVE, PBR_HAS_FEATURES0, PBR_HAS_FEATURES1, PBR_INTERLAYER_PARALLAX,
    PBR_SUBSURFACE, PBR_TWO_LAYER,
};
use render3d::permutation::{defines, Stage};
use render3d::scene::srgb_to_linear;

/// `builtin_default` with the TruePBR bit and the given SLSF2 bits.
fn pbr(bits: &[u32]) -> MaterialState {
    let mut m = MaterialState::builtin_default();
    m.slsf2 |= 1 << 23;
    for b in bits {
        m.slsf2 |= 1 << b;
    }
    m
}

fn bound(regs: &[u32]) -> Bound {
    Bound {
        regs: regs.iter().copied().collect(),
    }
}

/// Shader type 11 with the MultiLayer trailer floats f0..f3 (envmap strength 0.5).
fn mlp(m: &mut MaterialState, f: [f32; 4]) {
    m.shader_type = 11;
    m.params.trailer = LightingTrailer::MultiLayer {
        inner_thickness: f[0],
        refraction_scale: f[1],
        inner_texture_scale: [f[2], f[3]],
        envmap_strength: 0.5,
    };
}

#[test]
fn pbr_flags_eye_moisture_is_428() {
    // Capture oracle (answers.md Q4 table + Q1.3): Arch_Eye_Moisture, type 11, SLSF2 bits 4,7,15,23,24,25,27.
    // ev6721 binds t9 and t12 (plus the always-bound t0/t1/t5): 256 + 128 + 32 + 8 + 4 = 428.
    let mut m = pbr(&[4, 7, 15, 24, 25, 27]);
    m.shader_type = 11;
    assert_eq!(pbr_flags(&m, &bound(&[0, 1, 5, 9, 12])), 428);
    // ev6595 also binds the displacement map at t4: 430.
    assert_eq!(pbr_flags(&m, &bound(&[0, 1, 4, 5, 9, 12])), 430);
}

#[test]
fn pbr_flags_arch_eye_is_20() {
    // answers.md Q4 table: Arch_Eye type 0, SLSF2 8,15,23,26 → 20 = Features0 + Subsurface.
    assert_eq!(pbr_flags(&pbr(&[8, 15, 26]), &bound(&[0, 1, 5, 12])), 20);
}

#[test]
fn pbr_flags_branch_table() {
    let none = bound(&[]);
    let rows: &[(&[u32], u32, &str)] = &[
        (&[], 0, "plain"),
        (&[27], PBR_HAIR_MARSCHNER, "hair"),
        (&[27, 26, 25, 12], PBR_HAIR_MARSCHNER, "hair excludes subsurface/fuzz/glint"),
        (&[26], PBR_SUBSURFACE, "subsurface"),
        (&[25], PBR_FUZZ, "fuzz"),
        (&[25, 12], PBR_FUZZ, "fuzz excludes glint"),
        (&[12], PBR_GLINT, "glint, also a leaked Fit_Slope"),
        (&[26, 25], PBR_SUBSURFACE | PBR_FUZZ, "subsurface is independent of fuzz"),
        (&[26, 12], PBR_SUBSURFACE | PBR_GLINT, "subsurface + glint"),
        (&[24], PBR_TWO_LAYER, "two layer"),
        (
            &[24, 25, 27, 30],
            PBR_TWO_LAYER | PBR_INTERLAYER_PARALLAX | PBR_COAT_NORMAL | PBR_COLORED_COAT,
            "coat cave",
        ),
        (&[24, 26, 12], PBR_TWO_LAYER, "two layer excludes subsurface/glint"),
        (&[21], 0, "SLSF2.21 is not glint"),
        (&[30], 0, "Effect_Lighting alone"),
    ];
    for (bits, want, what) in rows {
        assert_eq!(pbr_flags(&pbr(bits), &none), *want, "{what}");
    }
}

#[test]
fn has_bits_follow_bound_registers_only() {
    let m = pbr(&[]);
    // Non-empty slot paths are not enough: a Has* bit needs the register bound to a real texture (01 §3.3).
    let mut with_paths = m.clone();
    for i in [2, 3, 6, 7] {
        with_paths.set_slot_raw(i, "textures\\x.dds");
    }
    assert_eq!(pbr_flags(&with_paths, &bound(&[])), 0);
    assert_eq!(pbr_flags(&m, &bound(&[6])), PBR_HAS_EMISSIVE);
    assert_eq!(pbr_flags(&m, &bound(&[4])), PBR_HAS_DISPLACEMENT);
    assert_eq!(pbr_flags(&m, &bound(&[12])), PBR_HAS_FEATURES0);
    assert_eq!(pbr_flags(&m, &bound(&[9])), PBR_HAS_FEATURES1);
    assert_eq!(pbr_flags(&m, &bound(&[0, 1, 2, 3, 5, 7, 8])), 0);
    // 01 §5.2 classes: emissive + parallax = 3; the coat cave with t4/t9/t12 resolved = 494.
    assert_eq!(pbr_flags(&m, &bound(&[0, 1, 4, 5, 6])), 3);
    let mut cave = pbr(&[24, 25, 27, 30]);
    cave.shader_type = 11;
    assert_eq!(
        pbr_flags(&cave, &bound(&[0, 1, 4, 5, 9, 12])),
        32 + 128 + 256 + 64 + 2 + 8 + 4
    );
    // Not PBR: always 0, whatever is bound.
    let mut v = MaterialState::builtin_default();
    v.slsf2 |= 1 << 24;
    assert_eq!(pbr_flags(&v, &bound(&[4, 6, 9, 12])), 0);
}

#[test]
fn glint_define_matches_the_glint_flag() {
    let rows: [&[u32]; 8] = [&[], &[12], &[12, 25], &[12, 24], &[12, 27], &[12, 26], &[21], &[21, 12]];
    for bits in rows {
        let m = pbr(bits);
        let flag = pbr_flags(&m, &Bound::default()) & PBR_GLINT != 0;
        assert_eq!(remapped_glint(m.slsf2), flag, "{bits:?}");
        let d = defines(&perm_material(&m), Stage::Pixel, &[], false);
        assert_eq!(d.0.iter().any(|(k, _)| k == "GLINT"), flag, "{bits:?}");
        assert!(!d.0.iter().any(|(k, _)| k == "ANISO_LIGHTING"), "{bits:?}");
    }
}

/// A synthetic PerMaterial layout, one 16-byte register per field.
const FIELDS: &[(&str, u32)] = &[
    ("TexcoordOffset", 16),
    ("SpecularColor", 16),
    ("PBRFlags", 4),
    ("PBRParams1", 12),
    ("PBRParams2", 16),
    ("MultiLayerParallaxData", 16),
    ("EnvmapData", 16),
    ("EmitColor", 12),
    ("emissiveMult", 4),
];

fn writer() -> CbWriter {
    let vars = FIELDS
        .iter()
        .enumerate()
        .map(|(i, (n, s))| (n.to_string(), 16 * i as u32, *s))
        .collect();
    CbWriter::new(&CbLayout {
        name: "PerMaterial".into(),
        slot: 1,
        size: 16 * FIELDS.len() as u32,
        vars,
    })
}

fn floats(w: &CbWriter, name: &str) -> Vec<f32> {
    let i = FIELDS.iter().position(|f| f.0 == name).unwrap();
    let at = 16 * i;
    w.bytes()[at..at + FIELDS[i].1 as usize]
        .chunks(4)
        .map(|c| f32::from_le_bytes(c.try_into().unwrap()))
        .collect()
}

fn flags(w: &CbWriter) -> u32 {
    let at = 16 * FIELDS.iter().position(|f| f.0 == "PBRFlags").unwrap();
    u32::from_le_bytes(w.bytes()[at..at + 4].try_into().unwrap())
}

fn written(m: &MaterialState, regs: &[u32], linear_lighting: bool) -> CbWriter {
    let mut w = writer();
    write_material(&mut w, m, &bound(regs), linear_lighting);
    w
}

#[test]
fn pbr_params_and_mlpd_follow_the_cs_branches() {
    let mut plain = pbr(&[]);
    plain.params.specular_color = [0.9, 0.7, 0.8];
    plain.params.lighting_effect_1 = 0.85;
    plain.params.specular_strength = 0.5;
    plain.params.lighting_effect_2 = 0.2;
    plain.params.glossiness = 0.08;
    let w = written(&plain, &[], false);
    assert_eq!(floats(&w, "PBRParams1"), [0.5, 0.2, 0.08]);
    assert_eq!(floats(&w, "PBRParams2"), [0.0; 4], "not written outside TwoLayer/Subsurface");
    assert_eq!(floats(&w, "MultiLayerParallaxData"), [0.0; 4]);
    assert_eq!(floats(&w, "SpecularColor"), [0.0; 4], "no SPECULAR under PBR");
    assert_eq!(flags(&w), 0);

    let mut ss = plain.clone();
    ss.slsf2 |= 1 << 26;
    let w = written(&ss, &[12], false);
    assert_eq!(flags(&w), PBR_SUBSURFACE | PBR_HAS_FEATURES0);
    assert_eq!(floats(&w, "PBRParams2"), [0.9, 0.7, 0.8, 0.85], "colour raw, not linearised");
    assert_eq!(floats(&w, "MultiLayerParallaxData"), [0.0; 4]);

    let mut coat = plain.clone();
    coat.slsf2 |= 1 << 24;
    mlp(&mut coat, [0.3, 0.018, 1.0, 1.0]);
    let w = written(&coat, &[], false);
    assert_eq!(floats(&w, "MultiLayerParallaxData"), [0.3, 0.018, 0.0, 0.0]);
    assert_eq!(floats(&w, "PBRParams2"), [0.9, 0.7, 0.8, 0.85]);

    let mut fuzz = plain.clone();
    fuzz.slsf2 |= 1 << 25;
    mlp(&mut fuzz, [0.13, 0.13, 0.13, 1.0]);
    let w = written(&fuzz, &[], false);
    assert_eq!(floats(&w, "MultiLayerParallaxData"), [0.13, 0.13, 0.13, 1.0]);
    assert_eq!(floats(&w, "PBRParams2"), [0.0; 4]);

    let mut glint = plain.clone();
    glint.slsf2 |= 1 << 12;
    mlp(&mut glint, [4.0, 15.0, 1.0, 5.0]);
    let w = written(&glint, &[], false);
    assert_eq!(floats(&w, "MultiLayerParallaxData"), [4.0, 25.0, 1.0, 5.0], "y = 40 − L");

    // Leaked Fit_Slope on type 0: CS zeroes the MLP floats, so glint with zero parameters → (0, 40, 0, 0).
    let mut leak = plain.clone();
    leak.slsf2 |= 1 << 12;
    let w = written(&leak, &[], false);
    assert_eq!(flags(&w), PBR_GLINT);
    assert_eq!(floats(&w, "MultiLayerParallaxData"), [0.0, 40.0, 0.0, 0.0]);

    // PBR never writes P0's raw trailer: a PBR type-11 shape outside the MLP branches leaves MLPD and
    // EnvmapData at 0.
    let mut t11 = plain.clone();
    mlp(&mut t11, [1.0, 2.0, 3.0, 4.0]);
    let w = written(&t11, &[5], false);
    assert_eq!(floats(&w, "EnvmapData"), [0.0; 4]);
    assert_eq!(floats(&w, "MultiLayerParallaxData"), [0.0; 4]);
}

#[test]
fn vanilla_trailer_is_unchanged_from_p0() {
    let mut v = MaterialState::builtin_default();
    mlp(&mut v, [1.0, 2.0, 3.0, 4.0]);
    let w = written(&v, &[5], false);
    assert_eq!(floats(&w, "MultiLayerParallaxData"), [1.0, 2.0, 3.0, 4.0]);
    assert_eq!(floats(&w, "EnvmapData"), [0.5, 1.0, 0.0, 0.0], "envmap strength, t5 bound");
    assert_eq!(floats(&w, "SpecularColor"), [1.0, 1.0, 1.0, 80.0]);
    assert_eq!(flags(&w), 0);
    assert_eq!(floats(&w, "PBRParams1"), [0.0; 3]);
}

#[test]
fn edit_rewrites_from_zero() {
    // 03 T-E2: a PBR → vanilla rewrite leaves no PBR bytes behind.
    let ss = pbr(&[26]);
    let mut w = writer();
    write_material(&mut w, &ss, &bound(&[12]), false);
    assert_ne!(floats(&w, "PBRParams2"), [0.0; 4]);
    assert_ne!(flags(&w), 0);
    w.zero();
    assert!(w.bytes().iter().all(|b| *b == 0));
    write_material(&mut w, &MaterialState::builtin_default(), &bound(&[]), false);
    for n in ["PBRParams1", "PBRParams2", "MultiLayerParallaxData"] {
        assert!(floats(&w, n).iter().all(|v| *v == 0.0), "{n}");
    }
    assert_eq!(flags(&w), 0);
    assert_eq!(floats(&w, "SpecularColor"), [1.0, 1.0, 1.0, 80.0]);
}

#[test]
fn emissive_d1() {
    // PBR (approximate, spec D1, capture C1 pending): NIF emissive colour × emissive multiple.
    let mut p = pbr(&[]);
    p.params.emissive_color = [0.5, 0.25, 1.0];
    p.params.emissive_multiple = 2.0;
    assert_eq!(floats(&written(&p, &[], false), "EmitColor"), [1.0, 0.5, 2.0]);
    let want = [0.5f32, 0.25, 1.0].map(|c| srgb_to_linear(c) * 2.0);
    assert_eq!(floats(&written(&p, &[], true), "EmitColor"), want, "linearised with LL");
    assert_eq!(floats(&written(&p, &[], false), "emissiveMult"), [2.0]);
    // Vanilla keeps P0's rule: only Own_Emit (SLSF1.22) shapes get the NIF colour.
    let mut v = MaterialState::builtin_default();
    v.params = p.params.clone();
    assert_eq!(floats(&written(&v, &[], false), "EmitColor"), [0.0; 3]);
    v.slsf1 |= 1 << 22;
    assert_eq!(floats(&written(&v, &[], false), "EmitColor"), [1.0, 0.5, 2.0]);
}
```

In `crates/render3d/tests/compile.rs` replace lines 77-83:

```rust
    let glint = defines(
        &m(0, 0, (1 << 21) | (1 << 23), false),
        Stage::Pixel,
        &[],
        false,
    );
    assert!(names(&glint).contains(&"GLINT") && !names(&glint).contains(&"ANISO_LIGHTING"));
```

with:

```rust
    // TruePBR: GLINT comes from CS's remapped Glint flag (Fit_Slope 12 without MLP 24 / Soft 25 / Back 27),
    // never from SLSF2.21, which CS clears at load (P1 spec §2.2).
    let pbr_ps = |slsf2: u32| -> Vec<String> {
        names(&defines(
            &m(11, 0, slsf2 | (1 << 23), false),
            Stage::Pixel,
            &[],
            false,
        ))
        .iter()
        .map(|s| s.to_string())
        .collect()
    };
    let has = |v: &[String], n: &str| v.iter().any(|x| x == n);
    assert!(!has(&pbr_ps(1 << 21), "GLINT") && !has(&pbr_ps(1 << 21), "ANISO_LIGHTING"));
    assert!(has(&pbr_ps(1 << 12), "GLINT"));
    assert!(has(&pbr_ps((1 << 12) | (1 << 26)), "GLINT"), "subsurface + glint");
    for blocker in [24, 25, 27] {
        assert!(!has(&pbr_ps((1 << 12) | (1 << blocker)), "GLINT"), "{blocker}");
    }
    assert!(!names(&defines(&m(0, 0, 1 << 12, false), Stage::Pixel, &[], false)).contains(&"GLINT"));
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo test -p render3d --test material`
Expected: FAIL to compile, `unresolved import render3d::material`.
Run: `cargo test -p render3d --test compile define_table`
Expected: FAIL, the `has(&pbr_ps(1 << 12), "GLINT")` assertion panics (P0 emits GLINT from SLSF2.21 only).

- [ ] **Step 3: `CbWriter::zero`** — in `crates/render3d/src/cbuffer.rs`, after `pub fn bytes(&self) -> &[u8] { … }` (lines 66-68), add:

```rust

    /// Every byte back to 0, as [`CbWriter::new`] leaves it. A material rewrite starts here, so no field of the
    /// previous material survives (answers.md §Surprises 3; 03 T-E2).
    pub fn zero(&mut self) {
        self.data.fill(0);
    }
```

- [ ] **Step 4: Create `crates/render3d/src/material.rs`**

```rust
//! Community Shaders' TruePBR load remap and the material cbuffer fields, from an owned `MaterialState`
//! (03 §1.2-1.3; 01 §3.1-3.4). `write_material`, `pack_specular` and `pbr_params` moved here from `draw.rs`
//! (P0 borrowed `nif::props::Lighting`).
use crate::cbuffer::CbWriter;
use crate::permutation::{runtime_bits, Material};
use crate::scene::srgb_to_linear;
use nif::material::MaterialState;
use nif::props::{LightingParams, LightingTrailer};
use std::collections::HashSet;

/// Registers bound to a real (non-default) texture in one draw. `pbr_flags` reads t4, t6, t9 and t12;
/// `EnvmapData.y` reads t5.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Bound {
    pub regs: HashSet<u32>,
}

// CS 1.9 `PBR::Flags` (01 §3.4; CS BSLightingShaderMaterialPBR.h:17-31; iLH Common/PBRMath.hlsli:23-35).
pub const PBR_HAS_EMISSIVE: u32 = 1;
pub const PBR_HAS_DISPLACEMENT: u32 = 2;
pub const PBR_HAS_FEATURES0: u32 = 4;
pub const PBR_HAS_FEATURES1: u32 = 8;
pub const PBR_SUBSURFACE: u32 = 16;
pub const PBR_TWO_LAYER: u32 = 32;
pub const PBR_COLORED_COAT: u32 = 64;
pub const PBR_INTERLAYER_PARALLAX: u32 = 128;
pub const PBR_COAT_NORMAL: u32 = 256;
pub const PBR_FUZZ: u32 = 512;
pub const PBR_HAIR_MARSCHNER: u32 = 1024;
pub const PBR_GLINT: u32 = 2048;

const fn bit(v: u32, b: u32) -> bool {
    v & (1 << b) != 0
}

/// CS's Glint flag from the NIF flags (01 §3.1): TruePBR (SLSF2.23) and Fit_Slope (SLSF2.12), and none of the
/// branches CS tests first: MLP (24), Back (27), Soft (25). Drives both `PBRFlags` bit 2048 and the `GLINT`
/// define (P1 spec §2.2: not SLSF2.21, which CS clears at load).
pub fn remapped_glint(slsf2: u32) -> bool {
    bit(slsf2, 23) && bit(slsf2, 12) && !bit(slsf2, 24) && !bit(slsf2, 25) && !bit(slsf2, 27)
}

/// `PBRFlags` as CS packs it at load (01 §3.1 remap + §3.3 bound textures); 0 for non-PBR materials.
/// ```text
/// SLSF2.24 (MLP): TwoLayer; +InterlayerParallax if .25; +CoatNormal if .27; +ColoredCoat if .30
/// elif .27 (Back): HairMarschner
/// else: Subsurface if .26; Fuzz if .25, else Glint if .12
/// t6 → HasEmissive, t4 → HasDisplacement, t12 → HasFeatures0, t9 → HasFeatures1 (only when in `bound`)
/// ```
/// Capture oracle: Arch_Eye_Moisture ev6721 = 428 (answers.md Q1.3).
pub fn pbr_flags(m: &MaterialState, bound: &Bound) -> u32 {
    if !m.is_pbr() {
        return 0;
    }
    let f2 = m.slsf2;
    let mut f = 0;
    if bit(f2, 24) {
        f |= PBR_TWO_LAYER;
        if bit(f2, 25) {
            f |= PBR_INTERLAYER_PARALLAX;
        }
        if bit(f2, 27) {
            f |= PBR_COAT_NORMAL;
        }
        if bit(f2, 30) {
            f |= PBR_COLORED_COAT;
        }
    } else if bit(f2, 27) {
        f |= PBR_HAIR_MARSCHNER;
    } else {
        if bit(f2, 26) {
            f |= PBR_SUBSURFACE;
        }
        if bit(f2, 25) {
            f |= PBR_FUZZ;
        } else if bit(f2, 12) {
            f |= PBR_GLINT;
        }
    }
    for (reg, flag) in [
        (6, PBR_HAS_EMISSIVE),
        (4, PBR_HAS_DISPLACEMENT),
        (12, PBR_HAS_FEATURES0),
        (9, PBR_HAS_FEATURES1),
    ] {
        if bound.regs.contains(&reg) {
            f |= flag;
        }
    }
    f
}

/// The permutation-relevant subset of a material (the owned replacement of `Material::from_shape`).
pub fn perm_material(m: &MaterialState) -> Material {
    Material {
        shader_type: m.shader_type,
        slsf1: m.slsf1,
        slsf2: m.slsf2,
        skinned: m.skinned,
        alpha_test: m.alpha.is_some(),
        has_vertex_colors: m.has_vertex_colors,
        specular_strength: m.params.specular_strength,
    }
}

/// `PerMaterial.SpecularColor` as the engine uploads it (answers.md Q5, frame11178 ev4217/5488/5536/6544):
/// `xyz = sRGB→linear(NIF specular colour) × NIF specular strength`, `w = NIF glossiness` (shininess,
/// iLH:973). The linearisation is the Linear Lighting effect (owner's CS has LL on); with LL off it is
/// `colour × strength` [Inference, Q5 open item]. Written only when SPECULAR is compiled; the PBR path
/// uses [`pbr_params`] instead.
pub fn pack_specular(p: &LightingParams, linear_lighting: bool) -> [f32; 4] {
    let c = p.specular_color.map(|c| {
        if linear_lighting {
            srgb_to_linear(c)
        } else {
            c
        }
    });
    let s = p.specular_strength;
    [c[0] * s, c[1] * s, c[2] * s, p.glossiness]
}

/// TruePBR `(PBRParams1, PBRParams2)` (answers.md Q5, Arch_Eye ev3891 = NIF exactly):
/// `PBRParams1 = (specStrength → roughness scale, lightingEffect2 → displacement scale, glossiness →
/// specular level)`, `PBRParams2 = (specular colour raw — not linearised —, lightingEffect1)`.
pub fn pbr_params(p: &LightingParams) -> ([f32; 3], [f32; 4]) {
    (
        [p.specular_strength, p.lighting_effect_2, p.glossiness],
        [
            p.specular_color[0],
            p.specular_color[1],
            p.specular_color[2],
            p.lighting_effect_1,
        ],
    )
}

/// The TruePBR block (01 §3.4): `PBRFlags` and `PBRParams1` always; `PBRParams2` only for TwoLayer or
/// Subsurface; `MultiLayerParallaxData` only for TwoLayer (coat roughness, coat specular level, 0, 0), Fuzz
/// (r, g, b, weight) or Glint (screen-space scale, 40 − log microfacet density, roughness, randomization).
/// The four MultiLayer floats are loaded only for shader type 11; CS zeroes them otherwise, so a leaked
/// Fit_Slope on type 0 packs (0, 40, 0, 0).
fn write_pbr(w: &mut CbWriter, m: &MaterialState, bound: &Bound) {
    let flags = pbr_flags(m, bound);
    let (p1, p2) = pbr_params(&m.params);
    w.set_u32("PBRFlags", &[flags]);
    w.set("PBRParams1", &p1);
    if flags & (PBR_TWO_LAYER | PBR_SUBSURFACE) != 0 {
        w.set("PBRParams2", &p2);
    }
    let f = match (m.shader_type, &m.params.trailer) {
        (
            11,
            LightingTrailer::MultiLayer {
                inner_thickness,
                refraction_scale,
                inner_texture_scale,
                ..
            },
        ) => [
            *inner_thickness,
            *refraction_scale,
            inner_texture_scale[0],
            inner_texture_scale[1],
        ],
        _ => [0.0; 4],
    };
    let mlpd = if flags & PBR_TWO_LAYER != 0 {
        Some([f[0], f[1], 0.0, 0.0])
    } else if flags & PBR_FUZZ != 0 {
        Some(f)
    } else if flags & PBR_GLINT != 0 {
        Some([f[0], 40.0 - f[1], f[2], f[3]])
    } else {
        None
    };
    if let Some(v) = mlpd {
        w.set("MultiLayerParallaxData", &v);
    }
}

/// P0's per-technique trailer fields, for non-PBR materials only (CS's PBR material has no envmap, tint,
/// parallax-occlusion or eye data; its MultiLayer floats go through [`write_pbr`]).
fn write_trailer(w: &mut CbWriter, p: &LightingParams, has_env_mask: bool) {
    let mask = if has_env_mask { 1.0 } else { 0.0 };
    // TintColor is not linearised (answers.md Q5: hair 6544 = NIF hairTint).
    match &p.trailer {
        LightingTrailer::SkinTint(c) | LightingTrailer::HairTint(c) => {
            w.set("TintColor", &[c[0], c[1], c[2], 1.0]);
        }
        LightingTrailer::EnvMap { scale } => {
            w.set("EnvmapData", &[*scale, mask, 0.0, 0.0]);
        }
        LightingTrailer::ParallaxOcc { max_passes, scale } => {
            w.set("ParallaxOccData", &[*max_passes, *scale, 0.0, 0.0]);
        }
        LightingTrailer::MultiLayer {
            inner_thickness,
            refraction_scale,
            inner_texture_scale,
            envmap_strength,
        } => {
            w.set(
                "MultiLayerParallaxData",
                &[
                    *inner_thickness,
                    *refraction_scale,
                    inner_texture_scale[0],
                    inner_texture_scale[1],
                ],
            );
            w.set("EnvmapData", &[*envmap_strength, mask, 0.0, 0.0]);
        }
        LightingTrailer::Eye {
            left_center,
            right_center,
            cubemap_scale,
        } => {
            w.set(
                "LeftEyeCenter",
                &[left_center[0], left_center[1], left_center[2], 0.0],
            );
            w.set(
                "RightEyeCenter",
                &[right_center[0], right_center[1], right_center[2], 0.0],
            );
            w.set("EnvmapData", &[*cubemap_scale, mask, 0.0, 0.0]);
        }
        LightingTrailer::SparkleSnow(s) => {
            w.set("SparkleParams", s);
        }
        LightingTrailer::None => {}
    }
}

/// Writes the material fields into one cbuffer of any stage. The writer must be zero (fresh from
/// `CbWriter::new` or after `CbWriter::zero`): fields a branch does not write must read 0, never a previous
/// material's value. `bound` = registers bound to a real texture in this draw.
pub fn write_material(w: &mut CbWriter, m: &MaterialState, bound: &Bound, linear_lighting: bool) {
    let p = &m.params;
    let pbr = m.is_pbr();
    w.set(
        "TexcoordOffset",
        &[p.uv_offset[0], p.uv_offset[1], p.uv_scale[0], p.uv_scale[1]],
    );
    // SPECULAR is compiled only for vanilla shapes with Specular set and a non-zero strength (permutation.rs).
    if !pbr && bit(m.slsf1, 0) && p.specular_strength > 0.0 {
        w.set("SpecularColor", &pack_specular(p, linear_lighting));
    }
    if pbr {
        write_pbr(w, m, bound);
    } else {
        write_trailer(w, p, bound.regs.contains(&5));
    }
    w.set(
        "LightingEffectParams",
        &[p.lighting_effect_1, p.lighting_effect_2, 0.0, 0.0],
    );
    w.set("MaterialObjectRGBScale", &[1.0, 1.0, 1.0]);
    // EmitColor: "sRGB by default, linear if LL on" (iLH ~2595).
    // - Vanilla (P0 rule): only Own_Emit (SLSF1.22) shapes get the NIF colour × multiple [Inference: every
    //   captured vanilla EmitColor was 0].
    // - PBR: approximate, spec D1, capture C1 pending. The NIF emissive colour × emissive multiple for every PBR
    //   shape (in game the engine supplies it; the one captured emissive PBR draw had 0).
    let emit = if pbr || bit(m.slsf1, 22) {
        p.emissive_color.map(|c| {
            (if linear_lighting {
                srgb_to_linear(c)
            } else {
                c
            }) * p.emissive_multiple
        })
    } else {
        [0.0; 3]
    };
    w.set("EmitColor", &emit);
    w.set("emissiveMult", &[p.emissive_multiple]);
    // envmap LOD fade, specular LOD fade, alpha.
    w.set("MaterialData", &[1.0, 1.0, p.alpha, 0.0]);
    // b11 AlphaTestRefRS = threshold/255 only when the NiAlphaProperty test flag is set; b2 AlphaTestRef stays
    // 0 (frame11178: Hair ev6544 test 40 → 0.1569, Lashes ev5536 test 5 → 0.0196, blend-only Scalp ev5488
    // threshold 50 → 0; b2 AlphaTestRef = 0 on all of them). Blend-only shapes still compile DO_ALPHA_TEST.
    let tref = m
        .alpha
        .filter(|a| a.test())
        .map_or(0.0, |a| a.threshold as f32 / 255.0);
    w.set("AlphaTestRefRS", &[tref]);
    // cb4: engine PS desc & ~compile key = runtime bits only (answers.md Q4.1, B5 `runtime_bits`).
    w.set_u32("PixelShaderDescriptor", &[runtime_bits(&perm_material(m))]);
    // InWorld (bit 0); SourceAlphaBlend (bit 8) only for blended passes (answers.md Q4.2).
    w.set_u32(
        "ExtraShaderDescriptor",
        &[1 | if m.blend() { 1 << 8 } else { 0 }],
    );
    // Identity bones for all 240 rows (P0 bind pose); the camera-relative shift is BonesPivot (Scene).
    let bones: Vec<f32> = (0..80)
        .flat_map(|_| [1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0])
        .collect();
    w.set("Bones", &bones);
    w.set("PreviousBones", &bones);
}
```

- [ ] **Step 5: GLINT in `permutation.rs`** — replace lines 394-396:

```rust
    if ps && bit(m.slsf2, 21) {
        out.push(if pbr { "GLINT" } else { "ANISO_LIGHTING" });
    }
```

with:

```rust
    // TruePBR: CS enables glint from its remapped Glint flag (Fit_Slope without MLP/Soft/Back; 01 §3.1, P1 spec
    // §2.2); SLSF2.21 is cleared at load. Vanilla: Anisotropic_Lighting (SLSF2.21) → ANISO_LIGHTING.
    if pbr && crate::material::remapped_glint(m.slsf2) {
        out.push("GLINT");
    } else if ps && !pbr && bit(m.slsf2, 21) {
        out.push("ANISO_LIGHTING");
    }
```

(`pbr` in `defines` is already `ps && SLSF2.23`, `permutation.rs:68`.)

- [ ] **Step 6: Strip the moved code from `draw.rs`** — delete lines 17-187 (from the doc comment ``/// `PerMaterial.SpecularColor` as the engine uploads it`` through the closing `}` of `write_material`), and replace the imports at lines 5-11:

```rust
use crate::cbuffer::CbWriter;
use crate::compile::{Compiled, Reflection, SrvKind};
use crate::device::Gpu;
use crate::mesh::GpuMesh;
use crate::permutation::{runtime_bits, Material};
use crate::scene::{srgb_to_linear, Mat3x4, Scene, View};
use nif::props::{Alpha, Lighting, LightingParams, LightingTrailer};
```

with:

```rust
use crate::cbuffer::CbWriter;
use crate::compile::{Compiled, Reflection, SrvKind};
use crate::device::Gpu;
use crate::mesh::GpuMesh;
use crate::scene::{Mat3x4, Scene, View};
```

- [ ] **Step 7: Wire `load` to the new writer** (`crates/render3d/src/lib.rs`, after Task 6). Exact text replacements:

1. Replace

```rust
pub mod locate;
pub mod mesh;
```

with

```rust
pub mod locate;
pub mod material;
pub mod mesh;
```

2. Replace `pub use parse::{parse, ParsedNif, ParsedShape, ShapeId};` with

```rust
pub use material::{pbr_flags, Bound};
pub use parse::{parse, ParsedNif, ParsedShape, ShapeId};
```

3. Replace `use draw::{cbuffers, create_program, write_material, MaterialInputs, Program, ShapeDraw};` with

```rust
use draw::{cbuffers, create_program, Program, ShapeDraw};
use material::{perm_material, write_material};
```

4. In `load`, replace `            let material = Material::from_shape(&row.head, lighting, row.alpha);` with

```rust
            let state =
                nif::material::MaterialState::from_lighting(&row.head, lighting, slots, row.alpha);
            let material = perm_material(&state);
```

5. In `load`, delete the two lines

```rust
            let specular =
                !pbr && bit(lighting.slsf1, 0) && lighting.params.specular_strength > 0.0;
```

6. In `load`, replace the block from `            let inputs = MaterialInputs {` through the closing `}` of the following `for cb in vs_cbs.iter_mut().chain(ps_cbs.iter_mut()) { write_material(&mut cb.writer, &inputs); }` loop (P0 `lib.rs:410-422`) with

```rust
            let bound_regs = Bound {
                regs: bound.keys().copied().collect(),
            };
            for cb in vs_cbs.iter_mut().chain(ps_cbs.iter_mut()) {
                write_material(&mut cb.writer, &state, &bound_regs, linear_lighting);
            }
```

(`Material` stays imported: `program(&mut self, m: &Material)` uses it. `bit` stays: `pbr` and `msn` in `load` use it. Task 8a replaces `load` anyway; this step only keeps the tree building with one writer.)

- [ ] **Step 8: Run the tests to verify they pass**

Run: `cargo test -p render3d --test material --test compile --test standin`
Expected: PASS (`material`: 9 tests; `compile::define_table` with the new GLINT rule; `standin` unchanged: exactly one `SkullPeg01:19: ps:` compile error, frame not empty).

- [ ] **Step 9: Gates**

Run: `cargo fmt --all && cargo clippy --workspace --all-targets -- -D warnings && cargo test --workspace`
Expected: all green.

- [ ] **Step 10: Commit**

```bash
git add crates/render3d/src/material.rs crates/render3d/src/cbuffer.rs crates/render3d/src/draw.rs crates/render3d/src/permutation.rs crates/render3d/src/lib.rs crates/render3d/tests/material.rs crates/render3d/tests/compile.rs
git commit -m "feat(render3d): PBRFlags, CS-branch PBRParams2/MLPD, GLINT from the Glint remap, D1 emissive" -m "Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

---

### Task 8a: Renderer restructure: `ShapeState` map, `load_parsed`, `load` wrapper, `shapes()`

**Files:**
- Modify (full replacement): `crates/render3d/src/lib.rs`
- Modify: `crates/render3d/src/draw.rs` (`ShapeDraw` gains `pub bound: Bound`; P0 `draw.rs:224-237`, after Task 7 the struct `pub struct ShapeDraw`)
- Create: `crates/render3d/tests/common/mod.rs`
- Test: `crates/render3d/tests/load.rs`

**Interfaces:**
- Consumes: Task 6 `parse`, `ParsedNif`, `ParsedShape`, `ShapeId`; Task 7 `material::{perm_material, write_material, Bound}`; `textures::{register_for_slot, default_for, load_dds}` (`textures.rs:40, 86, 289`); `draw::{cbuffers, create_program, srv_kind_at, Program, ShapeDraw}`; `mesh::build` (`mesh.rs:184`).
- Produces (contract, exact):
  ```rust
  #[derive(Debug, Clone, PartialEq)] pub enum ShapeStatus { Drawing, Hidden, Failed(String) }
  #[derive(Debug, Clone, PartialEq)] pub struct ShapeInfo { pub id: ShapeId, pub name: String, pub material: nif::material::MaterialState, pub status: ShapeStatus }
  impl Renderer {
      pub fn load_parsed(&mut self, nif: &ParsedNif, textures: &dyn Fn(&str) -> Option<Vec<u8>>) -> LoadReport;
      pub fn load(&mut self, nif_bytes: &[u8], textures: &dyn Fn(&str) -> Option<Vec<u8>>) -> LoadReport; // = load_parsed(&parse(bytes), textures)
      pub fn shapes(&self) -> Vec<ShapeInfo>;   // id order; Failed shapes listed too
  }
  ```
  Private, used by 8b/9/10: `struct ShapeState { name, mesh: Arc<Mesh>, material, draw: Option<ShapeDraw>, status }`, `Renderer.shapes: BTreeMap<ShapeId, ShapeState>`, `Renderer.draw_order: Vec<ShapeId>`, `Renderer.tex_cache`, `enum BuildError { Compile(String), Other(String) }`, `fn build_draw(&mut self, name: &str, mesh: &Mesh, m: &MaterialState, textures, rep: &mut LoadReport) -> Result<ShapeDraw, BuildError>`, `fn bind_textures(...) -> Result<(Vec<(u32, ID3D11ShaderResourceView)>, Bound), String>`, `fn sort_draw_order(&mut self)`, `fn linear_lighting(&self) -> bool`.
  `LoadReport` unchanged (`shapes` = shapes with status `Drawing`; a load-time compile failure is `Failed`, listed in `compile_errors` as `"{name}: {error}"`).

- [ ] **Step 1: Shared test helpers** — create `crates/render3d/tests/common/mod.rs`:

```rust
//! Helpers shared by the WARP integration tests: the repo-owned stand-in shader, nifly fixtures, tiny DDS files.
#![allow(dead_code)]
use render3d::device::Gpu;
use render3d::Renderer;
use std::path::{Path, PathBuf};

/// The stand-in hooks of Task 8b: opaque permutations compile, pixels = 0.25·diffuse + EmitColor (+ green when
/// PBRFlags ≠ 0), no lighting.
pub const FLAT: &[&str] = &["STANDIN_OPAQUE_OK", "STANDIN_FLAT"];

pub fn standin_root() -> PathBuf {
    PathBuf::from(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/fixtures/standin"
    ))
}

pub fn fixture(name: &str) -> Vec<u8> {
    std::fs::read(format!(
        "{}/../nif/tests/fixtures/{name}",
        env!("CARGO_MANIFEST_DIR")
    ))
    .unwrap()
}

/// A WARP renderer on the stand-in shader with exactly `features` as CS feature defines.
pub fn renderer(cache: &Path, features: &[&str]) -> Renderer {
    let fp = render3d::locate::fingerprint(&standin_root(), None).unwrap();
    let mut r =
        Renderer::with_gpu(Gpu::new_warp().unwrap(), fp, cache.to_path_buf()).unwrap();
    r.set_features(features.iter().map(|s| s.to_string()).collect());
    r
}

/// Sum of one channel (0 = red, 1 = green, 2 = blue) over an RGBA8 frame.
pub fn channel(px: &[u8], c: usize) -> u64 {
    px.chunks(4).map(|p| p[c] as u64).sum()
}

/// Pixels brighter than the clear colour.
pub fn covered(px: &[u8]) -> usize {
    px.chunks(4)
        .filter(|p| p[0] as u32 + p[1] as u32 + p[2] as u32 > 30)
        .count()
}

/// Files in the shader cache directory (`compile` writes `<key>.cso` + `<key>.json` per compile, compile.rs:111-112).
pub fn cache_files(dir: &Path) -> usize {
    std::fs::read_dir(dir).map(|d| d.count()).unwrap_or(0)
}

/// An uncompressed RGBA8 DDS, one mip, legacy header with masks R 0xff, G 0xff00, B 0xff0000, A 0xff000000
/// (`engine::dds::read_dds` reads it as `Rgba8 { srgb: false }`, dds.rs:533). `texels` row by row, top first.
pub fn dds_rgba8(w: u32, h: u32, texels: &[[u8; 4]]) -> Vec<u8> {
    assert_eq!(texels.len(), (w * h) as usize);
    let mut b = vec![0u8; 128];
    let mut put = |at: usize, v: u32| b[at..at + 4].copy_from_slice(&v.to_le_bytes());
    put(0, u32::from_le_bytes(*b"DDS "));
    put(4, 124);
    put(8, 0x1 | 0x2 | 0x4 | 0x8 | 0x1000); // CAPS | HEIGHT | WIDTH | PITCH | PIXELFORMAT
    put(12, h);
    put(16, w);
    put(20, w * 4);
    put(28, 1);
    put(76, 32);
    put(80, 0x41); // DDPF_RGB | DDPF_ALPHAPIXELS
    put(88, 32);
    put(92, 0xff);
    put(96, 0xff00);
    put(100, 0xff_0000);
    put(104, 0xff00_0000);
    put(108, 0x1000); // DDSCAPS_TEXTURE
    b.extend(texels.iter().flatten());
    b
}
```

- [ ] **Step 2: Write the failing test** — create `crates/render3d/tests/load.rs`:

```rust
//! The restructured Renderer: shapes keyed by block, `load` = `load_parsed(&parse(..))`.
mod common;
use common::{fixture, renderer};
use render3d::scene::Scene;
use render3d::{parse, ShapeId, ShapeStatus};

#[test]
fn load_lists_every_lighting_shape_by_block() {
    let cache = tempfile::tempdir().unwrap();
    // No STANDIN_OPAQUE_OK: the one shape without an NiAlphaProperty fails to compile (tests/standin.rs).
    let mut r = renderer(cache.path(), &[]);
    let rep = r.load(&fixture("TestNifFile_DeepGraph_SE.nif"), &|_| None);
    assert!(rep.errors.is_empty(), "{:?}", rep.errors);
    assert_eq!(rep.compile_errors.len(), 1, "{:?}", rep.compile_errors);
    assert!(rep.compile_errors[0].starts_with("SkullPeg01:19: ps:"));
    let shapes = r.shapes();
    assert_eq!(shapes.len(), 27);
    assert!(shapes.windows(2).all(|w| w[0].id < w[1].id), "id order");
    let skull = shapes.iter().find(|s| s.id == ShapeId(143)).unwrap();
    assert_eq!(skull.name, "SkullPeg01:19");
    assert!(
        matches!(&skull.status, ShapeStatus::Failed(e) if e.starts_with("ps:")),
        "{:?}",
        skull.status
    );
    let drawing = shapes
        .iter()
        .filter(|s| s.status == ShapeStatus::Drawing)
        .count();
    assert_eq!(drawing, 26);
    assert_eq!(rep.shapes, drawing);
    assert_eq!(shapes[0].material.slots.len(), 9);
}

#[test]
fn load_parsed_matches_load() {
    let cache = tempfile::tempdir().unwrap();
    let mut r = renderer(cache.path(), &["STANDIN_OPAQUE_OK"]);
    let nif = fixture("TestNifFile_Furniture_Col_SE.nif");
    let a = r.load(&nif, &|_| None);
    let pa = r.render(&Scene::neutral(), 64, 64).unwrap();
    let b = r.load_parsed(&parse(&nif), &|_| None);
    let pb = r.render(&Scene::neutral(), 64, 64).unwrap();
    assert_eq!(a, b);
    assert_eq!(a.shapes, 2);
    assert_eq!(pa, pb);
    assert_eq!(
        r.shapes().iter().map(|s| s.id).collect::<Vec<_>>(),
        [ShapeId(9), ShapeId(12)]
    );
}

#[test]
fn a_new_load_replaces_every_shape() {
    let cache = tempfile::tempdir().unwrap();
    let mut r = renderer(cache.path(), &["STANDIN_OPAQUE_OK"]);
    r.load(&fixture("TestNifFile_DeepGraph_SE.nif"), &|_| None);
    r.load(&fixture("TestNifFile_Furniture_Col_SE.nif"), &|_| None);
    let names: Vec<String> = r.shapes().into_iter().map(|s| s.name).collect();
    assert_eq!(names, ["cylinder_1", "cylinder_2"]);
}

#[test]
fn le_mesh_loads_nothing() {
    let cache = tempfile::tempdir().unwrap();
    let mut r = renderer(cache.path(), &["STANDIN_OPAQUE_OK"]);
    let rep = r.load(&fixture("TestNifFile_Animated_LE.nif"), &|_| None);
    assert!(rep.le_mesh);
    assert_eq!(rep.shapes, 0);
    assert!(r.shapes().is_empty());
    assert!(r.bounds().is_none());
}

#[test]
fn hidden_at_load_is_listed_but_not_drawn() {
    let cache = tempfile::tempdir().unwrap();
    let mut r = renderer(cache.path(), &["STANDIN_OPAQUE_OK"]);
    let mut p = parse(&fixture("TestNifFile_Furniture_Col_SE.nif"));
    p.shapes[0].material.hidden = true;
    let rep = r.load_parsed(&p, &|_| None);
    assert_eq!(rep.shapes, 1);
    let s = r.shapes();
    assert_eq!(s[0].status, ShapeStatus::Hidden);
    assert_eq!(s[1].status, ShapeStatus::Drawing);
}
```

- [ ] **Step 3: Run test to verify it fails**

Run: `cargo test -p render3d --test load`
Expected: FAIL to compile, `no method named shapes found for struct Renderer` / `no method named load_parsed` / `cannot find type ShapeStatus in crate render3d`.

- [ ] **Step 4: `ShapeDraw.bound`** — in `crates/render3d/src/draw.rs`, add `use crate::material::Bound;` after `use crate::device::Gpu;`, and in `pub struct ShapeDraw` replace

```rust
    pub blend: bool,
    pub skinned: bool,
}
```

with

```rust
    pub blend: bool,
    pub skinned: bool,
    /// Registers bound to a real texture (not a 1×1 default); the PBR flags and `EnvmapData.y` read it.
    pub bound: Bound,
}
```

- [ ] **Step 5: Replace `crates/render3d/src/lib.rs` completely with:**

```rust
pub mod cbuffer;
pub mod compile;
pub mod device;
pub mod draw;
pub mod includes;
pub mod locate;
pub mod material;
pub mod mesh;
pub mod parse;
pub mod permutation;
pub mod scene;
pub mod textures;

pub use material::{pbr_flags, Bound};
pub use parse::{parse, ParsedNif, ParsedShape, ShapeId};

use compile::{compile, SrvKind};
use device::{Gpu, Target};
use draw::{cbuffers, create_program, Program, ShapeDraw};
use locate::Fingerprint;
use material::{perm_material, write_material};
use nif::material::MaterialState;
use permutation::{defines, Material, Stage};
use scene::{Scene, View};
use std::collections::{BTreeMap, HashMap};
use std::path::PathBuf;
use std::sync::Arc;
use windows::Win32::Graphics::Direct3D::D3D11_PRIMITIVE_TOPOLOGY_TRIANGLELIST;
use windows::Win32::Graphics::Direct3D11::{
    ID3D11BlendState, ID3D11Query, ID3D11RasterizerState, ID3D11SamplerState,
    ID3D11ShaderResourceView, D3D11_BLEND_DESC, D3D11_BLEND_INV_SRC_ALPHA, D3D11_BLEND_ONE,
    D3D11_BLEND_OP_ADD, D3D11_BLEND_SRC_ALPHA, D3D11_COLOR_WRITE_ENABLE_ALL,
    D3D11_COMPARISON_NEVER, D3D11_CULL_NONE, D3D11_FILL_SOLID, D3D11_FILTER_MIN_MAG_MIP_LINEAR,
    D3D11_FLOAT32_MAX, D3D11_QUERY_DESC, D3D11_QUERY_EVENT, D3D11_RASTERIZER_DESC,
    D3D11_RENDER_TARGET_BLEND_DESC, D3D11_SAMPLER_DESC, D3D11_TEXTURE_ADDRESS_WRAP, D3D11_VIEWPORT,
};
use windows::Win32::Graphics::Dxgi::Common::DXGI_FORMAT_R16_UINT;

/// CS feature defines compiled by default. The owner's installed CS 1.9 has Linear Lighting on
/// (answers.md: `SettingsUser.json` `enableLinearLighting: 1`), and Linear Lighting is the compile-time
/// define `ENABLE_LL` (`Common/ColorManagement.hlsli:13,133`). P0 does no other installed-feature detection;
/// the GUI's "Community Shaders features" toggle clears this list (`Renderer::set_features`).
///
/// This is the owner's CS SettingsUser.json / frame-11178 value; P1 reads the enabled features per user.
pub const DEFAULT_FEATURES: &[&str] = &["ENABLE_LL"];

/// Result of [`Renderer::load`].
#[derive(Debug, Clone, Default, PartialEq)]
pub struct LoadReport {
    /// BSVER 83: nothing is rendered (00 §6.5).
    pub le_mesh: bool,
    /// Shapes that will be drawn.
    pub shapes: usize,
    /// Texture paths (canonical `textures/…`) the resolver did not return; their registers get defaults.
    pub missing_textures: Vec<String>,
    /// Per-shape permutation compile errors (the shape is skipped, the others still draw).
    pub compile_errors: Vec<String>,
    /// Other per-shape problems: NIF scan/geometry errors, an invalid `Mesh`, unreadable DDS, GPU errors.
    pub errors: Vec<String>,
    /// Non-fatal notes: skipped non-Lighting shapes, cbuffer names this CS build lacks (logged once).
    pub warnings: Vec<String>,
}

/// Where the time of the last [`Renderer::render`] went, in milliseconds.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct FrameTiming {
    /// CPU time to record the draws (cbuffer updates, state, `DrawIndexed`).
    pub draw_ms: f64,
    /// Waiting for the GPU to finish the draws (an event query after a flush).
    pub gpu_ms: f64,
    /// Copy to a staging texture, `Map`, and the half → RGBA8 conversion; the GPU is idle by then, so this is
    /// the readback cost alone (the B9 zero-copy gate).
    pub readback_ms: f64,
}

/// What a loaded shape is doing on screen. P1-B adds `Pending`.
#[derive(Debug, Clone, PartialEq)]
pub enum ShapeStatus {
    /// The current material is on screen.
    Drawing,
    /// `MaterialState::hidden` (PGPatcher `delete`): not drawn.
    Hidden,
    /// The current material could not be built (compile, mesh or GPU error). The previous draw, if there is
    /// one, stays on screen.
    Failed(String),
}

/// One loaded shape as the GUI sees it.
#[derive(Debug, Clone, PartialEq)]
pub struct ShapeInfo {
    pub id: ShapeId,
    pub name: String,
    /// The material last loaded or set (even when it failed to build).
    pub material: MaterialState,
    pub status: ShapeStatus,
}

/// One loaded shape: the CPU mesh (kept so an edit rebuilds the draw without re-parsing), the material last
/// loaded or set, and what is on screen (possibly built from an earlier material, see [`ShapeStatus::Failed`]).
struct ShapeState {
    name: String,
    mesh: Arc<nif::geom::Mesh>,
    material: MaterialState,
    draw: Option<ShapeDraw>,
    status: ShapeStatus,
}

/// Why a draw could not be built. A permutation compile error is reported in `LoadReport::compile_errors`,
/// everything else in `errors`.
enum BuildError {
    Compile(String),
    Other(String),
}

/// A texture resolved once per load: its view and kind, or `None` (the resolver had no bytes, or they did
/// not decode).
type TexSlot = Option<(ID3D11ShaderResourceView, SrvKind)>;

/// The 3D preview renderer: message-shaped (load bytes → report, render scene → pixels) so it can later move
/// into a sidecar process. Single-threaded, like [`Gpu`].
pub struct Renderer {
    gpu: Gpu,
    shaders: Fingerprint,
    cache_dir: PathBuf,
    features: Vec<String>,
    programs: HashMap<String, Result<Program, String>>,
    defaults: HashMap<(SrvKind, [u8; 4]), ID3D11ShaderResourceView>,
    /// Every Lighting shape of the loaded model, keyed by NIF block index (built-ins: `ShapeId(0)`).
    shapes: BTreeMap<ShapeId, ShapeState>,
    /// Shapes with a draw that are not hidden: opaque first, then alpha-blended, block order within each.
    draw_order: Vec<ShapeId>,
    /// Textures resolved since the last load, by canonical path.
    tex_cache: HashMap<String, TexSlot>,
    bounds: Option<([f32; 3], f32)>,
    sampler: ID3D11SamplerState,
    raster: ID3D11RasterizerState,
    alpha_blend: ID3D11BlendState,
    target: Option<Target>,
    encode_srgb: bool,
    timing: FrameTiming,
}

fn permutation_key(d: &permutation::Defines, target: &str) -> String {
    let mut v: Vec<String> = d.0.iter().map(|(k, v)| format!("{k}={v}")).collect();
    v.sort();
    format!("{target}|{}", v.join(";"))
}

impl Renderer {
    /// Creates the device (hardware, else WARP; see [`Gpu::new`]) and the fixed pipeline states.
    pub fn new(shaders: Fingerprint, cache_dir: PathBuf) -> Result<Renderer, String> {
        Renderer::with_gpu(Gpu::new(None)?, shaders, cache_dir)
    }

    pub fn with_gpu(
        gpu: Gpu,
        shaders: Fingerprint,
        cache_dir: PathBuf,
    ) -> Result<Renderer, String> {
        let sd = D3D11_SAMPLER_DESC {
            Filter: D3D11_FILTER_MIN_MAG_MIP_LINEAR,
            AddressU: D3D11_TEXTURE_ADDRESS_WRAP,
            AddressV: D3D11_TEXTURE_ADDRESS_WRAP,
            AddressW: D3D11_TEXTURE_ADDRESS_WRAP,
            MipLODBias: 0.0,
            MaxAnisotropy: 1,
            ComparisonFunc: D3D11_COMPARISON_NEVER,
            BorderColor: [0.0; 4],
            MinLOD: 0.0,
            MaxLOD: D3D11_FLOAT32_MAX,
        };
        // Cull none: the forward capture draw (state_6544 Arch_Hair) has CullMode.NoCull, and P0 does not
        // model the per-shape Double_Sided flag.
        // Front = counter-clockwise: the captured camera basis (FrameBuffer::CameraView rows right = f × up,
        // up, forward; reproduced by `View::new`) mirrors the Z-up world, and the captured opaque draws use
        // CullMode.Back (state_4217.json) yet show their outer faces, so the engine's front faces are CCW.
        // With D3D's default (CW) every visible face reports SV_IsFrontFace = false and Lighting.hlsl flips
        // its TBN ("Fix incorrect vertex normals on double-sided meshes", iLH ~954): lit sides go dark.
        let rd = D3D11_RASTERIZER_DESC {
            FillMode: D3D11_FILL_SOLID,
            CullMode: D3D11_CULL_NONE,
            FrontCounterClockwise: true.into(),
            DepthClipEnable: true.into(),
            ..Default::default()
        };
        let mut bd = D3D11_BLEND_DESC::default();
        bd.RenderTarget[0] = D3D11_RENDER_TARGET_BLEND_DESC {
            BlendEnable: true.into(),
            SrcBlend: D3D11_BLEND_SRC_ALPHA,
            DestBlend: D3D11_BLEND_INV_SRC_ALPHA,
            BlendOp: D3D11_BLEND_OP_ADD,
            SrcBlendAlpha: D3D11_BLEND_ONE,
            DestBlendAlpha: D3D11_BLEND_INV_SRC_ALPHA,
            BlendOpAlpha: D3D11_BLEND_OP_ADD,
            RenderTargetWriteMask: D3D11_COLOR_WRITE_ENABLE_ALL.0 as u8,
        };
        let (mut s, mut r, mut b) = (None, None, None);
        // SAFETY: descriptors and out-pointers are locals.
        unsafe {
            gpu.device
                .CreateSamplerState(&sd, Some(&mut s))
                .map_err(|e| format!("CreateSamplerState: {e}"))?;
            gpu.device
                .CreateRasterizerState(&rd, Some(&mut r))
                .map_err(|e| format!("CreateRasterizerState: {e}"))?;
            gpu.device
                .CreateBlendState(&bd, Some(&mut b))
                .map_err(|e| format!("CreateBlendState: {e}"))?;
        }
        Ok(Renderer {
            gpu,
            shaders,
            cache_dir,
            features: DEFAULT_FEATURES.iter().map(|s| s.to_string()).collect(),
            programs: HashMap::new(),
            defaults: HashMap::new(),
            shapes: BTreeMap::new(),
            draw_order: vec![],
            tex_cache: HashMap::new(),
            bounds: None,
            sampler: s.ok_or("no sampler")?,
            raster: r.ok_or("no rasterizer state")?,
            alpha_blend: b.ok_or("no blend state")?,
            target: None,
            encode_srgb: false,
            timing: FrameTiming::default(),
        })
    }

    pub fn gpu(&self) -> &Gpu {
        &self.gpu
    }

    /// CS feature defines for the next load or rebuilt draw (default [`DEFAULT_FEATURES`]).
    pub fn set_features(&mut self, features: Vec<String>) {
        self.features = features;
    }

    pub fn features(&self) -> &[String] {
        &self.features
    }

    /// Replaces the shaders (a moved or updated CS) and drops every compiled program: the in-memory cache is keyed
    /// by defines only. The loaded model still uses the old programs until the next [`Renderer::load`].
    pub fn set_shaders(&mut self, shaders: Fingerprint) {
        self.shaders = shaders;
        self.programs.clear();
    }

    pub fn shaders(&self) -> &Fingerprint {
        &self.shaders
    }

    /// sRGB-encode the frame on readback (off by default). Turn it on for display when the features include
    /// `ENABLE_LL`: Linear Lighting draws in linear working space; without it the output is already gamma-space.
    pub fn set_encode_srgb(&mut self, on: bool) {
        self.encode_srgb = on;
    }

    /// Timing of the last successful [`Renderer::render`].
    pub fn last_timing(&self) -> FrameTiming {
        self.timing
    }

    /// Bounding sphere (centre, radius) of every shape drawn at load: `positions × transform` (skinned
    /// shapes: positions as-is, since the skinned VS ignores World and P0 bones are identity). Parent NiNode
    /// transforms are not applied in P1-A. Material edits do not move it.
    pub fn bounds(&self) -> Option<([f32; 3], f32)> {
        self.bounds
    }

    fn linear_lighting(&self) -> bool {
        self.features.iter().any(|f| f == "ENABLE_LL")
    }

    fn program(&mut self, m: &Material) -> Result<Program, String> {
        let f: Vec<&str> = self.features.iter().map(String::as_str).collect();
        let vd = defines(m, Stage::Vertex, &f, false);
        let pd = defines(m, Stage::Pixel, &f, false);
        let key = format!(
            "{}#{}",
            permutation_key(&vd, "vs_5_0"),
            permutation_key(&pd, "ps_5_0")
        );
        if let Some(p) = self.programs.get(&key) {
            return p.clone();
        }
        let root = self.shaders.root.clone();
        let file = root.join("Lighting.hlsl");
        let (cache, hash) = (&self.cache_dir, &self.shaders.hash);
        let built = compile(&file, &root, &vd, "vs_5_0", cache, hash)
            .map_err(|e| format!("vs: {e}"))
            .and_then(|vs| {
                compile(&file, &root, &pd, "ps_5_0", cache, hash)
                    .map_err(|e| format!("ps: {e}"))
                    .map(|ps| (vs, ps))
            })
            .and_then(|(vs, ps)| create_program(&self.gpu, vs, ps));
        self.programs.insert(key, built.clone());
        built
    }

    fn default_srv(
        &mut self,
        kind: SrvKind,
        rgba: [u8; 4],
    ) -> Result<ID3D11ShaderResourceView, String> {
        if let Some(v) = self.defaults.get(&(kind, rgba)) {
            return Ok(v.clone());
        }
        let v = textures::default_srv(&self.gpu, kind, rgba)?;
        self.defaults.insert((kind, rgba), v.clone());
        Ok(v)
    }

    /// The texture at canonical `norm`, resolved and decoded once per load. A path the resolver does not
    /// return is listed in `rep.missing_textures`; undecodable bytes are an `rep.errors` line.
    fn texture(
        &mut self,
        norm: &str,
        textures: &dyn Fn(&str) -> Option<Vec<u8>>,
        rep: &mut LoadReport,
    ) -> TexSlot {
        if let Some(t) = self.tex_cache.get(norm) {
            return t.clone();
        }
        let loaded = match textures(norm) {
            None => {
                if !rep.missing_textures.iter().any(|m| m == norm) {
                    rep.missing_textures.push(norm.to_string());
                }
                None
            }
            Some(bytes) => match textures::load_dds(&self.gpu, &bytes) {
                Ok(t) => Some((t.srv, t.kind)),
                Err(e) => {
                    rep.errors.push(format!("{norm}: {e}"));
                    None
                }
            },
        };
        self.tex_cache.insert(norm.to_string(), loaded.clone());
        loaded
    }

    /// NIF slot → register for every slot the PS reads (a texture is loaded only then), then a 1×1 default for
    /// every reflected register left without one. Returns `(register, view)` for every reflected SRV and the
    /// registers bound to a real texture.
    fn bind_textures(
        &mut self,
        name: &str,
        program: &Program,
        m: &MaterialState,
        textures: &dyn Fn(&str) -> Option<Vec<u8>>,
        rep: &mut LoadReport,
    ) -> Result<(Vec<(u32, ID3D11ShaderResourceView)>, Bound), String> {
        let pbr = m.is_pbr();
        let msn = m.slsf1 & (1 << 12) != 0;
        let mut real: HashMap<u32, ID3D11ShaderResourceView> = HashMap::new();
        for (slot, path) in m.slots.iter().enumerate() {
            if path.norm.is_empty() {
                continue;
            }
            let Some(reg) = textures::register_for_slot(m.shader_type, pbr, msn, slot) else {
                continue;
            };
            let Some(kind) = draw::srv_kind_at(&program.ps.reflection, reg) else {
                continue;
            };
            match self.texture(&path.norm, textures, rep) {
                Some((srv, k)) if k == kind => {
                    real.insert(reg, srv);
                }
                Some(_) => rep.warnings.push(format!(
                    "{name}: {} is not a {kind:?} texture (t{reg}); default bound",
                    path.norm
                )),
                None => {}
            }
        }
        let mut srvs = vec![];
        for (_, reg, kind) in &program.ps.reflection.srvs {
            if let Some(v) = real.get(reg) {
                srvs.push((*reg, v.clone()));
                continue;
            }
            let rgba = textures::default_for(*reg, m.shader_type, pbr);
            let v = self
                .default_srv(*kind, rgba)
                .map_err(|e| format!("default texture: {e}"))?;
            srvs.push((*reg, v));
        }
        let bound = Bound {
            regs: real.keys().copied().collect(),
        };
        Ok((srvs, bound))
    }

    /// Builds one shape's draw for material `m`: program (compiled synchronously), textures, vertex streams
    /// against the VS inputs, zero cbuffers from the reflection, then the material.
    fn build_draw(
        &mut self,
        name: &str,
        mesh: &nif::geom::Mesh,
        m: &MaterialState,
        textures: &dyn Fn(&str) -> Option<Vec<u8>>,
        rep: &mut LoadReport,
    ) -> Result<ShapeDraw, BuildError> {
        let program = self
            .program(&perm_material(m))
            .map_err(BuildError::Compile)?;
        let (srvs, bound) = self
            .bind_textures(name, &program, m, textures, rep)
            .map_err(BuildError::Other)?;
        let gpu_mesh = mesh::build(
            &self.gpu,
            mesh,
            &program.vs.reflection.inputs,
            &program.vs.bytecode,
        )
        .map_err(BuildError::Other)?;
        let (mut vs_cbs, mut ps_cbs) = match (
            cbuffers(&self.gpu, &program.vs.reflection),
            cbuffers(&self.gpu, &program.ps.reflection),
        ) {
            (Ok(v), Ok(p)) => (v, p),
            (Err(e), _) | (_, Err(e)) => return Err(BuildError::Other(e)),
        };
        let ll = self.linear_lighting();
        for cb in vs_cbs.iter_mut().chain(ps_cbs.iter_mut()) {
            write_material(&mut cb.writer, m, &bound, ll);
        }
        Ok(ShapeDraw {
            name: name.to_string(),
            vs: program.vs_obj.clone(),
            ps: program.ps_obj.clone(),
            mesh: gpu_mesh,
            vs_cbs,
            ps_cbs,
            srvs,
            samplers: program.ps.reflection.samplers.iter().map(|s| s.1).collect(),
            model: mesh.transform,
            blend: m.blend(),
            skinned: m.skinned,
            bound,
        })
    }

    /// Parses the NIF and loads it: `load_parsed(&parse(nif_bytes), textures)`. `textures` resolves a canonical
    /// texture path (`textures/…`, lower case, `/`) to DDS bytes.
    pub fn load(
        &mut self,
        nif_bytes: &[u8],
        textures: &dyn Fn(&str) -> Option<Vec<u8>>,
    ) -> LoadReport {
        self.load_parsed(&parse(nif_bytes), textures)
    }

    /// Replaces the loaded model with `parsed`: one draw per shape (compiled synchronously), textures resolved
    /// through `textures`. A shape whose draw cannot be built stays listed with status `Failed`; a hidden
    /// material is listed as `Hidden` and not built.
    pub fn load_parsed(
        &mut self,
        parsed: &ParsedNif,
        textures: &dyn Fn(&str) -> Option<Vec<u8>>,
    ) -> LoadReport {
        self.shapes.clear();
        self.draw_order.clear();
        self.tex_cache.clear();
        self.bounds = None;
        let mut rep = LoadReport {
            le_mesh: parsed.le_mesh,
            errors: parsed.errors.clone(),
            warnings: parsed.warnings.clone(),
            ..LoadReport::default()
        };
        if parsed.le_mesh {
            return rep;
        }
        for s in &parsed.shapes {
            let (draw, status) = if s.material.hidden {
                (None, ShapeStatus::Hidden)
            } else {
                match self.build_draw(&s.name, &s.mesh, &s.material, textures, &mut rep) {
                    Ok(d) => (Some(d), ShapeStatus::Drawing),
                    Err(BuildError::Compile(e)) => {
                        rep.compile_errors.push(format!("{}: {e}", s.name));
                        (None, ShapeStatus::Failed(e))
                    }
                    Err(BuildError::Other(e)) => {
                        rep.errors.push(format!("{}: {e}", s.name));
                        (None, ShapeStatus::Failed(e))
                    }
                }
            };
            self.shapes.insert(
                s.id,
                ShapeState {
                    name: s.name.clone(),
                    mesh: s.mesh.clone(),
                    material: s.material.clone(),
                    draw,
                    status,
                },
            );
        }
        self.bounds = self.compute_bounds();
        self.warn_missing_fields(&mut rep);
        self.sort_draw_order();
        rep.shapes = self
            .shapes
            .values()
            .filter(|s| s.status == ShapeStatus::Drawing)
            .count();
        rep
    }

    /// Every loaded shape in id order, with the material last loaded or set and its status.
    pub fn shapes(&self) -> Vec<ShapeInfo> {
        self.shapes
            .iter()
            .map(|(id, s)| ShapeInfo {
                id: *id,
                name: s.name.clone(),
                material: s.material.clone(),
                status: s.status.clone(),
            })
            .collect()
    }

    fn compute_bounds(&self) -> Option<([f32; 3], f32)> {
        let (mut lo, mut hi) = ([f32::MAX; 3], [f32::MIN; 3]);
        let mut points: Vec<[f32; 3]> = vec![];
        for s in self.shapes.values().filter(|s| s.draw.is_some()) {
            let t = &s.mesh.transform;
            for p in &s.mesh.positions {
                let q = if s.material.skinned {
                    *p
                } else {
                    [0, 1, 2].map(|r| t[r][0] * p[0] + t[r][1] * p[1] + t[r][2] * p[2] + t[r][3])
                };
                for k in 0..3 {
                    lo[k] = lo[k].min(q[k]);
                    hi[k] = hi[k].max(q[k]);
                }
                points.push(q);
            }
        }
        if points.is_empty() {
            return None;
        }
        let c = [0, 1, 2].map(|k| (lo[k] + hi[k]) / 2.0);
        let r = points
            .iter()
            .map(|p| ((p[0] - c[0]).powi(2) + (p[1] - c[1]).powi(2) + (p[2] - c[2]).powi(2)).sqrt())
            .fold(0.0f32, f32::max);
        Some((c, r))
    }

    /// A required name no drawn shape's cbuffers have: logged once per load (CS version drift, 03 §6). The
    /// skinned VS reads neither World nor EyePosition (bones + BonesPivot instead), so those two are only
    /// expected from non-skinned shapes.
    fn warn_missing_fields(&self, rep: &mut LoadReport) {
        let draws: Vec<&ShapeDraw> = self.shapes.values().filter_map(|s| s.draw.as_ref()).collect();
        if draws.is_empty() {
            return;
        }
        for n in scene::REQUIRED {
            let world_only = matches!(*n, "World" | "EyePosition");
            let candidates: Vec<&&ShapeDraw> = draws
                .iter()
                .filter(|s| !(world_only && s.skinned))
                .collect();
            if candidates.is_empty() {
                continue;
            }
            let somewhere = candidates
                .iter()
                .any(|s| s.vs_cbs.iter().chain(&s.ps_cbs).any(|cb| cb.writer.has(n)));
            if !somewhere {
                rep.warnings
                    .push(format!("this CS build has no cbuffer field {n:?}"));
            }
        }
    }

    /// Opaque first, then alpha-blended (P0 does not sort blended shapes by depth); block order within each.
    /// Hidden shapes and shapes without a draw are left out.
    fn sort_draw_order(&mut self) {
        let mut order: Vec<(bool, ShapeId)> = self
            .shapes
            .iter()
            .filter(|(_, s)| s.status != ShapeStatus::Hidden)
            .filter_map(|(id, s)| s.draw.as_ref().map(|d| (d.blend, *id)))
            .collect();
        order.sort_by_key(|(blend, _)| *blend);
        self.draw_order = order.into_iter().map(|(_, id)| id).collect();
    }

    /// Renders the loaded model into a `w × h` RGBA8 frame (tightly packed, top row first, alpha 255: see
    /// [`Target::readback_display`]). Errors (device
    /// removed, allocation failure) are text; see [`device`] for the device-removed contract.
    pub fn render(&mut self, scene: &Scene, w: u32, h: u32) -> Result<Vec<u8>, String> {
        let t0 = std::time::Instant::now();
        let (w, h) = (w.max(1), h.max(1));
        if self
            .target
            .as_ref()
            .is_none_or(|t| t.width != w || t.height != h)
        {
            self.target = Some(Target::new(&self.gpu, w, h)?);
        }
        let view = View::new(&scene.camera, self.bounds, w, h);
        let gpu = &self.gpu;
        let target = self.target.as_ref().ok_or("no target")?;
        target.clear(gpu, [0.0, 0.0, 0.0, 1.0]);
        // SAFETY (whole block): every bound object belongs to `gpu.device`; slices outlive the calls.
        unsafe {
            let ctx = &gpu.ctx;
            ctx.OMSetRenderTargets(Some(&[Some(target.rtv.clone())]), &target.dsv);
            ctx.RSSetViewports(Some(&[D3D11_VIEWPORT {
                TopLeftX: 0.0,
                TopLeftY: 0.0,
                Width: w as f32,
                Height: h as f32,
                MinDepth: 0.0,
                MaxDepth: 1.0,
            }]));
            ctx.RSSetState(&self.raster);
            ctx.IASetPrimitiveTopology(D3D11_PRIMITIVE_TOPOLOGY_TRIANGLELIST);
            for id in &self.draw_order {
                let Some(s) = self.shapes.get_mut(id).and_then(|st| st.draw.as_mut()) else {
                    continue;
                };
                s.update(gpu, scene, &view);
                ctx.IASetInputLayout(&s.mesh.layout);
                for (slot, (b, stride)) in s.mesh.streams.iter().enumerate() {
                    ctx.IASetVertexBuffers(
                        slot as u32,
                        1,
                        Some(&Some(b.clone())),
                        Some(stride),
                        Some(&0),
                    );
                }
                ctx.IASetIndexBuffer(&s.mesh.index, DXGI_FORMAT_R16_UINT, 0);
                ctx.VSSetShader(&s.vs, None);
                ctx.PSSetShader(&s.ps, None);
                for cb in &s.vs_cbs {
                    ctx.VSSetConstantBuffers(cb.slot, Some(&[Some(cb.buffer.clone())]));
                }
                for cb in &s.ps_cbs {
                    ctx.PSSetConstantBuffers(cb.slot, Some(&[Some(cb.buffer.clone())]));
                }
                for (reg, v) in &s.srvs {
                    ctx.PSSetShaderResources(*reg, Some(&[Some(v.clone())]));
                }
                for slot in &s.samplers {
                    ctx.PSSetSamplers(*slot, Some(&[Some(self.sampler.clone())]));
                }
                if s.blend {
                    ctx.OMSetBlendState(&self.alpha_blend, None, 0xffff_ffff);
                } else {
                    ctx.OMSetBlendState(None, None, 0xffff_ffff);
                }
                ctx.DrawIndexed(s.mesh.index_count, 0, 0);
            }
            ctx.OMSetBlendState(None, None, 0xffff_ffff);
        }
        let draw_ms = t0.elapsed().as_secs_f64() * 1000.0;
        let t1 = std::time::Instant::now();
        wait_idle(gpu)?;
        let gpu_ms = t1.elapsed().as_secs_f64() * 1000.0;
        let t2 = std::time::Instant::now();
        let px = target.readback_display(gpu, self.encode_srgb)?;
        self.timing = FrameTiming {
            draw_ms,
            gpu_ms,
            readback_ms: t2.elapsed().as_secs_f64() * 1000.0,
        };
        Ok(px)
    }
}

/// Flushes and spins on an event query until the GPU has finished every submitted command, so the readback
/// that follows measures the copy alone. A failing query (removed device) is an error with the device prefix.
fn wait_idle(gpu: &Gpu) -> Result<(), String> {
    let desc = D3D11_QUERY_DESC {
        Query: D3D11_QUERY_EVENT,
        MiscFlags: 0,
    };
    let mut q: Option<ID3D11Query> = None;
    // SAFETY: `desc` and the out-pointer are locals; the query belongs to `gpu.device`.
    unsafe {
        gpu.device
            .CreateQuery(&desc, Some(&mut q))
            .map_err(|e| gpu.fail("CreateQuery", e))?;
    }
    let q = q.ok_or_else(|| gpu.fail("CreateQuery", "no query returned"))?;
    // SAFETY: `done` is a live BOOL-sized local, the size passed matches it.
    unsafe {
        gpu.ctx.End(&q);
        gpu.ctx.Flush();
        let start = std::time::Instant::now();
        loop {
            let mut done = windows::core::BOOL(0);
            gpu.ctx
                .GetData(
                    &q,
                    Some(&mut done as *mut _ as *mut core::ffi::c_void),
                    std::mem::size_of::<windows::core::BOOL>() as u32,
                    0,
                )
                .map_err(|e| gpu.fail("GetData", e))?;
            if done.as_bool() {
                return Ok(());
            }
            if start.elapsed().as_secs() > 10 {
                // The readback's Map waits anyway; the timing is then approximate.
                return Ok(());
            }
            std::thread::yield_now();
        }
    }
}
```

Notes on behaviour kept from P0 (check against `lib.rs:260-488` at 1d357be): the compile error line is `"{name}: {e}"` with `e` = `"vs: …"`/`"ps: …"`; a texture is loaded only when the PS reflects its register; a missing texture is listed once per load; undecodable DDS bytes are an `errors` line `"{norm}: {e}"`; the default-texture failure text is `"{name}: default texture: {e}"`; bounds come from the drawn shapes only; the `REQUIRED` field check and the opaque-then-blended order are unchanged. Changed: parse-time geometry errors come before build errors in `errors`.

- [ ] **Step 6: Run the tests to verify they pass**

Run: `cargo test -p render3d --test load --test standin --test parse --test material`
Expected: PASS (`load`: 5 tests; `standin` unchanged).

- [ ] **Step 7: Gates**

Run: `cargo fmt --all && cargo clippy --workspace --all-targets -- -D warnings && cargo test --workspace`
Expected: all green; the gui crate builds unchanged (`Renderer::load`, `render`, `bounds`, `set_*`, `last_timing` keep their signatures; `LoadReport` and `FrameTiming` are unchanged).

- [ ] **Step 8: Commit**

```bash
git add crates/render3d/src/lib.rs crates/render3d/src/draw.rs crates/render3d/tests/common/mod.rs crates/render3d/tests/load.rs
git commit -m "refactor(render3d): shapes keyed by ShapeId, load_parsed, load as a wrapper, shapes()" -m "Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

---

### Task 8b: `classify`, `set_material`, stand-in hooks

**Files:**
- Modify: `crates/render3d/src/material.rs` (add `EditKind`, `classify`)
- Modify: `crates/render3d/src/draw.rs` (add `ShapeDraw::rewrite_material`)
- Modify: `crates/render3d/src/lib.rs` (re-exports, `BuildError::into_text`, `set_material`, `apply_in_place`; anchors below)
- Modify (full replacement): `crates/render3d/tests/fixtures/standin/Lighting.hlsl`
- Test: `crates/render3d/tests/edit.rs`; append to `crates/render3d/tests/material.rs`

**Interfaces:**
- Consumes: Task 8a `Renderer` internals (`shapes`, `build_draw`, `bind_textures`, `program`, `sort_draw_order`, `linear_lighting`, `tex_cache`), `ShapeStatus`; Task 7 `perm_material`, `write_material`; `permutation::defines`; `textures::register_for_slot`.
- Produces (contract, exact):
  ```rust
  // render3d::material, re-exported at the crate root
  #[derive(Debug, Clone, Copy, PartialEq, Eq)] pub enum EditKind { Same, Cbuffer, Textures, Permutation }
  pub fn classify(old: &MaterialState, new: &MaterialState, features: &[&str]) -> EditKind;
  impl Renderer {
      pub fn set_material(&mut self, id: ShapeId, m: MaterialState, textures: &dyn Fn(&str) -> Option<Vec<u8>>) -> Result<EditKind, String>;
  }
  // render3d::draw
  impl ShapeDraw { pub fn rewrite_material(&mut self, m: &MaterialState, linear_lighting: bool); }
  ```
  `classify`: `Same` iff equal; `Permutation` iff the VS or PS `defines` differ; else `Textures` iff any slot's `(register, norm path)` differs; else `Cbuffer` (a `hidden`-only change is `Cbuffer`). `set_material`: `Err` only for an unknown id; on a build failure `Ok(kind)` with `status = Failed(e)` and the previous draw kept.
  Stand-in hooks (feature defines): `STANDIN_FLAT` → PS returns `0.25·diffuse·vertexColour + EmitColor.rgb + (0, 0.5 if PBRFlags & 0xFFF ≠ 0, 0)`; `STANDIN_TBN` → PS returns `saturate(N·L)` with N = normal map in the vertex frame (used by Task 9). With `VC` the stand-in VS reads `COLOR0`.
  **Vertex colours without a colour stream (binding note from part P):** pgpsim may set `has_vertex_colors = true` (+ SLSF2.5) on a mesh with no colours. Then `VC` is compiled and the VS reads `COLOR0`; `mesh::stream_for` already feeds `[255; 4]` per vertex for an empty `Mesh.colors` (`mesh.rs:119-122`, `m.colors.get(i).copied().unwrap_or([255; 4])`), so nothing changes in `mesh.rs`. 8b pins it with a unit test of `stream_for` and a WARP test (VC on a colourless shape → Permutation, `Drawing`, frame identical to before).

- [ ] **Step 1: Write the failing tests** — append to `crates/render3d/tests/material.rs`:

```rust
#[test]
fn classify_table() {
    use nif::props::Alpha;
    use render3d::material::{classify, EditKind};
    let f = ["ENABLE_LL"];
    let base = MaterialState::builtin_default();
    let with = |edit: &dyn Fn(&mut MaterialState)| {
        let mut m = base.clone();
        edit(&mut m);
        m
    };
    assert_eq!(classify(&base, &base.clone(), &f), EditKind::Same);
    let uv = with(&|m| m.params.uv_scale = [2.0, 2.0]);
    assert_eq!(classify(&base, &uv, &f), EditKind::Cbuffer);
    let emit = with(&|m| m.params.emissive_color = [1.0, 0.0, 0.0]);
    assert_eq!(classify(&base, &emit, &f), EditKind::Cbuffer);
    let hidden = with(&|m| m.hidden = true);
    assert_eq!(classify(&base, &hidden, &f), EditKind::Cbuffer);
    // Adding the alpha property toggles DO_ALPHA_TEST; flipping blend on an existing one does not.
    let test_only = with(&|m| m.alpha = Some(Alpha { flags: 0x200, threshold: 128 }));
    assert_eq!(classify(&base, &test_only, &f), EditKind::Permutation);
    let mut blended = test_only.clone();
    blended.alpha = Some(Alpha { flags: 0x201, threshold: 128 });
    assert_eq!(classify(&test_only, &blended, &f), EditKind::Cbuffer);
    // Specular strength 0 ↔ > 0 toggles SPECULAR on vanilla shapes.
    let no_spec = with(&|m| m.params.specular_strength = 0.0);
    assert_eq!(classify(&base, &no_spec, &f), EditKind::Permutation);
    // A new slot path rebinds; a raw-only change (same canonical path) does not.
    let tex = with(&|m| m.set_slot_raw(0, "textures\\a.dds"));
    assert_eq!(classify(&base, &tex, &f), EditKind::Textures);
    let mut same_norm = tex.clone();
    same_norm.set_slot_raw(0, "Textures/A.DDS");
    assert_eq!(classify(&tex, &same_norm, &f), EditKind::Cbuffer);
    // Glowmap (type 2) moves slot 2 from t12 to t6 without a define: a binding change.
    let mut glow_src = base.clone();
    glow_src.set_slot_raw(2, "textures\\g.dds");
    let mut glow = glow_src.clone();
    glow.shader_type = 2;
    assert_eq!(classify(&glow_src, &glow, &f), EditKind::Textures);
    // PBR on/off is TRUE_PBR; under PBR, roughness and the shader type (technique 0) are cbuffer edits,
    // Rim (→ Subsurface) too, but Fit_Slope toggles GLINT.
    let pbr = with(&|m| m.slsf2 |= 1 << 23);
    assert_eq!(classify(&base, &pbr, &f), EditKind::Permutation);
    let mut rough = pbr.clone();
    rough.params.specular_strength = 0.25;
    assert_eq!(classify(&pbr, &rough, &f), EditKind::Cbuffer);
    let mut t11 = pbr.clone();
    t11.shader_type = 11;
    assert_eq!(classify(&pbr, &t11, &f), EditKind::Cbuffer);
    let mut rim = pbr.clone();
    rim.slsf2 |= 1 << 26;
    assert_eq!(classify(&pbr, &rim, &f), EditKind::Cbuffer);
    let mut glint = pbr.clone();
    glint.slsf2 |= 1 << 12;
    assert_eq!(classify(&pbr, &glint, &f), EditKind::Permutation);
}
```

Create `crates/render3d/tests/edit.rs`:

```rust
//! Live material edits on WARP with the stand-in shader. `STANDIN_FLAT` makes pixels = 0.25·diffuse +
//! EmitColor (+ 0.5 green when PBRFlags ≠ 0), so cbuffer and texture edits show without lighting saturation.
mod common;
use common::{cache_files, channel, covered, dds_rgba8, fixture, renderer, FLAT};
use nif::material::MaterialState;
use nif::props::Alpha;
use render3d::scene::Scene;
use render3d::{pbr_flags, Bound, EditKind, Renderer, ShapeId, ShapeStatus};

const CYL1: ShapeId = ShapeId(9);

fn material(r: &Renderer, id: ShapeId) -> MaterialState {
    r.shapes().into_iter().find(|s| s.id == id).unwrap().material
}

fn status(r: &Renderer, id: ShapeId) -> ShapeStatus {
    r.shapes().into_iter().find(|s| s.id == id).unwrap().status
}

fn furniture(cache: &std::path::Path) -> Renderer {
    let mut r = renderer(cache, FLAT);
    let rep = r.load(&fixture("TestNifFile_Furniture_Col_SE.nif"), &|_| None);
    assert_eq!(rep.shapes, 2, "{rep:?}");
    r
}

#[test]
fn unknown_id_is_an_error() {
    let cache = tempfile::tempdir().unwrap();
    let mut r = furniture(cache.path());
    let e = r.set_material(ShapeId(9999), MaterialState::builtin_default(), &|_| None);
    assert!(e.is_err());
}

/// 03 T-E1: emissive + UV scale on a vanilla shape → Cbuffer, next frame differs, no compile.
#[test]
fn cbuffer_edit_changes_pixels_without_a_compile() {
    let cache = tempfile::tempdir().unwrap();
    let mut r = furniture(cache.path());
    let before = r.render(&Scene::neutral(), 64, 64).unwrap();
    let files = cache_files(cache.path());
    let mut m = material(&r, CYL1);
    m.slsf1 |= 1 << 22; // Own_Emit: vanilla EmitColor = colour × multiple (no define reads it)
    m.params.emissive_color = [1.0, 0.0, 0.0];
    m.params.emissive_multiple = 0.5;
    m.params.uv_scale = [2.0, 2.0];
    assert_eq!(r.set_material(CYL1, m.clone(), &|_| None), Ok(EditKind::Cbuffer));
    let after = r.render(&Scene::neutral(), 64, 64).unwrap();
    assert_eq!(cache_files(cache.path()), files, "nothing was compiled");
    assert!(
        channel(&after, 0) > channel(&before, 0) + 1000,
        "red rose: {} → {}",
        channel(&before, 0),
        channel(&after, 0)
    );
    assert_eq!(channel(&after, 2), channel(&before, 2), "blue untouched");
    assert_eq!(material(&r, CYL1), m);
    assert_eq!(status(&r, CYL1), ShapeStatus::Drawing);
}

#[test]
fn texture_edit_rebinds_without_a_reload() {
    let cache = tempfile::tempdir().unwrap();
    let mut r = furniture(cache.path());
    let before = r.render(&Scene::neutral(), 64, 64).unwrap();
    let red = dds_rgba8(1, 1, &[[255, 0, 0, 255]]);
    let resolve = |p: &str| (p == "textures/standin/red.dds").then(|| red.clone());
    let mut m = material(&r, CYL1);
    m.set_slot_raw(0, "Textures\\Standin\\Red.dds");
    assert_eq!(r.set_material(CYL1, m, &resolve), Ok(EditKind::Textures));
    let after = r.render(&Scene::neutral(), 64, 64).unwrap();
    // White default (0.25 grey) → red texel (0.25, 0, 0) on cylinder_1.
    assert_eq!(channel(&after, 0), channel(&before, 0));
    assert!(channel(&after, 2) < channel(&before, 2));
}

#[test]
fn missing_texture_is_asked_for_again() {
    let cache = tempfile::tempdir().unwrap();
    let mut r = furniture(cache.path());
    let mut m = material(&r, CYL1);
    m.set_slot_raw(0, "textures\\standin\\late.dds");
    assert_eq!(r.set_material(CYL1, m.clone(), &|_| None), Ok(EditKind::Textures));
    let grey = r.render(&Scene::neutral(), 64, 64).unwrap();
    let red = dds_rgba8(1, 1, &[[255, 0, 0, 255]]);
    let resolve = |p: &str| (p == "textures/standin/late.dds").then(|| red.clone());
    // Touch another slot path so the edit is a texture pass; the path that was missing is asked for again.
    m.set_slot_raw(7, "textures\\standin\\other.dds");
    assert_eq!(r.set_material(CYL1, m, &resolve), Ok(EditKind::Textures));
    let red_px = r.render(&Scene::neutral(), 64, 64).unwrap();
    assert!(channel(&red_px, 2) < channel(&grey, 2));
}

#[test]
fn permutation_edit_compiles_synchronously() {
    let cache = tempfile::tempdir().unwrap();
    let mut r = furniture(cache.path());
    r.render(&Scene::neutral(), 64, 64).unwrap();
    let files = cache_files(cache.path());
    let mut m = material(&r, CYL1);
    m.alpha = Some(Alpha { flags: 0x200, threshold: 0 }); // test only: adds DO_ALPHA_TEST
    assert_eq!(r.set_material(CYL1, m, &|_| None), Ok(EditKind::Permutation));
    assert!(cache_files(cache.path()) > files, "a new pixel shader was compiled");
    assert_eq!(status(&r, CYL1), ShapeStatus::Drawing);
    assert!(covered(&r.render(&Scene::neutral(), 64, 64).unwrap()) > 0);
}

/// 03 T-E4 / Review Focus #3, P1-A form: a permutation that fails to compile keeps the old draw on screen.
#[test]
fn failing_permutation_keeps_old_draw() {
    let cache = tempfile::tempdir().unwrap();
    // No STANDIN_OPAQUE_OK: any permutation without DO_ALPHA_TEST fails to compile.
    let mut r = renderer(cache.path(), &["STANDIN_FLAT"]);
    r.load(&fixture("TestNifFile_DeepGraph_SE.nif"), &|_| None);
    let peg = ShapeId(103); // PegLeft01:9, has an NiAlphaProperty
    assert_eq!(status(&r, peg), ShapeStatus::Drawing);
    let before = r.render(&Scene::neutral(), 96, 96).unwrap();
    let good = material(&r, peg);
    let mut bad = good.clone();
    bad.alpha = None;
    assert_eq!(r.set_material(peg, bad.clone(), &|_| None), Ok(EditKind::Permutation));
    assert!(
        matches!(status(&r, peg), ShapeStatus::Failed(e) if e.starts_with("ps:")),
        "{:?}",
        status(&r, peg)
    );
    assert_eq!(material(&r, peg), bad, "the wanted material is reported");
    assert_eq!(r.render(&Scene::neutral(), 96, 96).unwrap(), before, "old draw stays");
    assert_eq!(status(&r, ShapeId(107)), ShapeStatus::Drawing, "others unaffected");
    // Back to the good material: rebuilt, same pixels.
    assert_eq!(r.set_material(peg, good, &|_| None), Ok(EditKind::Permutation));
    assert_eq!(status(&r, peg), ShapeStatus::Drawing);
    assert_eq!(r.render(&Scene::neutral(), 96, 96).unwrap(), before);
}

#[test]
fn hidden_shape_is_not_drawn_and_comes_back() {
    let cache = tempfile::tempdir().unwrap();
    let mut r = furniture(cache.path());
    let before = r.render(&Scene::neutral(), 64, 64).unwrap();
    let shown = material(&r, CYL1);
    let mut hidden = shown.clone();
    hidden.hidden = true;
    assert_eq!(r.set_material(CYL1, hidden, &|_| None), Ok(EditKind::Cbuffer));
    assert_eq!(status(&r, CYL1), ShapeStatus::Hidden);
    let gone = r.render(&Scene::neutral(), 64, 64).unwrap();
    assert_ne!(gone, before);
    assert!(channel(&gone, 0) < channel(&before, 0));
    assert_eq!(r.set_material(CYL1, shown, &|_| None), Ok(EditKind::Cbuffer));
    assert_eq!(status(&r, CYL1), ShapeStatus::Drawing);
    assert_eq!(r.render(&Scene::neutral(), 64, 64).unwrap(), before);
}

/// pgpsim can switch vertex colours on for a mesh without a colour stream (JSON `vertex_colors: true`): the VC
/// permutation must build and read white, not fail or read garbage.
#[test]
fn vertex_colours_without_a_colour_stream_read_white() {
    let (f, stride, bytes) =
        render3d::mesh::stream_for(&nif::geom::Mesh { positions: vec![[0.0; 3]; 3], ..Default::default() }, "COLOR", 0);
    assert_eq!(stride, 4);
    assert_eq!(f, windows::Win32::Graphics::Dxgi::Common::DXGI_FORMAT_R8G8B8A8_UNORM);
    assert!(bytes.iter().all(|b| *b == 255));

    let cache = tempfile::tempdir().unwrap();
    let mut r = furniture(cache.path());
    let before = r.render(&Scene::neutral(), 64, 64).unwrap();
    let mut m = material(&r, CYL1);
    assert!(!m.has_vertex_colors, "cylinder_1 has no colour stream (vertex attributes 0x1B)");
    m.has_vertex_colors = true;
    m.slsf2 |= 1 << 5; // Vertex_Colors: with has_vertex_colors → VC define (VS)
    assert_eq!(r.set_material(CYL1, m, &|_| None), Ok(EditKind::Permutation));
    assert_eq!(status(&r, CYL1), ShapeStatus::Drawing);
    assert_eq!(r.render(&Scene::neutral(), 64, 64).unwrap(), before, "white vertex colour");
}

/// The packed PBRFlags reach the shader: Rim on a PBR shape is a cbuffer edit (Subsurface = 16) and turns the
/// stand-in's flag channel on.
#[test]
fn pbr_flags_reach_the_shader() {
    let cache = tempfile::tempdir().unwrap();
    let mut r = furniture(cache.path());
    let mut m = material(&r, CYL1);
    m.slsf2 &= !((1 << 12) | (1 << 24) | (1 << 25) | (1 << 26) | (1 << 27));
    m.slsf2 |= 1 << 23;
    m.params.emissive_color = [0.0; 3]; // D1 would add the NIF emissive colour under PBR
    assert_eq!(r.set_material(CYL1, m.clone(), &|_| None), Ok(EditKind::Permutation));
    assert_eq!(pbr_flags(&m, &Bound::default()), 0);
    let plain = r.render(&Scene::neutral(), 64, 64).unwrap();
    m.slsf2 |= 1 << 26;
    assert_eq!(pbr_flags(&m, &Bound::default()), 16);
    assert_eq!(r.set_material(CYL1, m, &|_| None), Ok(EditKind::Cbuffer));
    let subsurface = r.render(&Scene::neutral(), 64, 64).unwrap();
    assert!(
        channel(&subsurface, 1) > channel(&plain, 1) + 1000,
        "green {} → {}",
        channel(&plain, 1),
        channel(&subsurface, 1)
    );
    assert_eq!(channel(&subsurface, 2), channel(&plain, 2));
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test -p render3d --test edit --test material`
Expected: FAIL to compile, `unresolved imports render3d::EditKind, render3d::material::classify` and `no method named set_material`.

- [ ] **Step 3: `EditKind` and `classify`** — append to `crates/render3d/src/material.rs`:

```rust

/// How a material change reaches the screen (03 §1.2).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EditKind {
    /// Nothing changed.
    Same,
    /// Only cbuffer fields (and possibly blending / `hidden`): rewrite the cbuffers, no compile.
    Cbuffer,
    /// A slot path or the slot → register mapping changed: rebind textures, then rewrite the cbuffers.
    Textures,
    /// The VS or PS define set changed: a new program and a new draw.
    Permutation,
}

/// Which texture each slot feeds: `(register, canonical path)` per slot (`textures::register_for_slot`).
fn bindings(m: &MaterialState) -> Vec<(Option<u32>, &str)> {
    let msn = bit(m.slsf1, 12);
    m.slots
        .iter()
        .enumerate()
        .map(|(i, s)| {
            (
                crate::textures::register_for_slot(m.shader_type, m.is_pbr(), msn, i),
                s.norm.as_str(),
            )
        })
        .collect()
}

/// Sorts an edit by what it costs. The permutation oracle is `permutation::defines` (03 §1.2): `Permutation`
/// iff the VS or PS define set differs. Otherwise `Textures` iff a slot's register or canonical path differs
/// (e.g. a new path, or type 0 ↔ 2 moving slot 2 between t12 and t6), else `Cbuffer`. A change of `hidden` alone
/// is `Cbuffer`.
pub fn classify(old: &MaterialState, new: &MaterialState, features: &[&str]) -> EditKind {
    if old == new {
        return EditKind::Same;
    }
    let (a, b) = (perm_material(old), perm_material(new));
    for stage in [Stage::Vertex, Stage::Pixel] {
        if defines(&a, stage, features, false) != defines(&b, stage, features, false) {
            return EditKind::Permutation;
        }
    }
    if bindings(old) != bindings(new) {
        EditKind::Textures
    } else {
        EditKind::Cbuffer
    }
}
```

and change the import line `use crate::permutation::{runtime_bits, Material};` to:

```rust
use crate::permutation::{defines, runtime_bits, Material, Stage};
```

- [ ] **Step 4: `ShapeDraw::rewrite_material`** — in `crates/render3d/src/draw.rs` change `use crate::material::Bound;` to

```rust
use crate::material::{write_material, Bound};
use nif::material::MaterialState;
```

and add inside `impl ShapeDraw { … }`, after `update`:

```rust

    /// A cbuffer edit (03 §1.3): every writer back to zero, then the material, so no field of the previous
    /// material survives. The scene fields are written again by the next [`ShapeDraw::update`].
    pub fn rewrite_material(&mut self, m: &MaterialState, linear_lighting: bool) {
        for cb in self.vs_cbs.iter_mut().chain(self.ps_cbs.iter_mut()) {
            cb.writer.zero();
            write_material(&mut cb.writer, m, &self.bound, linear_lighting);
        }
        self.blend = m.blend();
    }
```

- [ ] **Step 5: `set_material`** — in `crates/render3d/src/lib.rs`:

1. Replace `pub use material::{pbr_flags, Bound};` with

```rust
pub use material::{classify, pbr_flags, Bound, EditKind};
```

2. Replace

```rust
enum BuildError {
    Compile(String),
    Other(String),
}
```

with

```rust
enum BuildError {
    Compile(String),
    Other(String),
}

impl BuildError {
    fn into_text(self) -> String {
        match self {
            BuildError::Compile(e) | BuildError::Other(e) => e,
        }
    }
}
```

3. Insert after the `shapes()` method (before `fn compute_bounds`):

```rust
    /// Replaces shape `id`'s material and applies it the cheapest way [`classify`] allows (03 §1.3):
    /// - `Cbuffer`: every cbuffer of the draw back to zero, then the material; on the next frame, no compile;
    /// - `Textures`: the slots resolved again through `textures` and rebound, then the `Cbuffer` path (the PBR
    ///   flags and `EnvmapData.y` depend on what is bound);
    /// - `Permutation`: a new draw is built now (synchronous compile in P1-A; background compile is P1-B).
    ///
    /// A shape that is not `Drawing` (failed, hidden) is always rebuilt. On any build error the previous draw
    /// stays on screen and the status is `Failed(error)`; the result is still `Ok(kind)`. A hidden material
    /// stops drawing the shape and keeps its draw for later. Paths the resolver did not return before are asked
    /// for again. `Err` only for an id that is not loaded.
    pub fn set_material(
        &mut self,
        id: ShapeId,
        m: MaterialState,
        textures: &dyn Fn(&str) -> Option<Vec<u8>>,
    ) -> Result<EditKind, String> {
        let (kind, rebuild) = {
            let state = self
                .shapes
                .get(&id)
                .ok_or_else(|| format!("no shape with id {} is loaded", id.0))?;
            let features: Vec<&str> = self.features.iter().map(String::as_str).collect();
            let kind = classify(&state.material, &m, &features);
            let rebuild = state.draw.is_none()
                || state.status != ShapeStatus::Drawing
                || kind == EditKind::Permutation;
            (kind, rebuild)
        };
        if m.hidden {
            if let Some(state) = self.shapes.get_mut(&id) {
                state.material = m;
                state.status = ShapeStatus::Hidden;
            }
            self.sort_draw_order();
            return Ok(kind);
        }
        if kind == EditKind::Same && !rebuild {
            return Ok(kind);
        }
        self.tex_cache.retain(|_, t| t.is_some());
        let mut scratch = LoadReport::default();
        let outcome = if rebuild {
            let (name, mesh) = {
                let s = &self.shapes[&id];
                (s.name.clone(), s.mesh.clone())
            };
            match self.build_draw(&name, &mesh, &m, textures, &mut scratch) {
                Ok(d) => {
                    if let Some(s) = self.shapes.get_mut(&id) {
                        s.draw = Some(d);
                    }
                    Ok(())
                }
                Err(e) => Err(e.into_text()),
            }
        } else {
            self.apply_in_place(id, kind, &m, textures, &mut scratch)
        };
        if let Some(state) = self.shapes.get_mut(&id) {
            state.status = match outcome {
                Ok(()) => ShapeStatus::Drawing,
                Err(e) => ShapeStatus::Failed(e),
            };
            state.material = m;
        }
        self.sort_draw_order();
        Ok(kind)
    }

    /// A `Cbuffer` or `Textures` edit on the shape's existing draw (same permutation, so the program is an
    /// in-memory hit).
    fn apply_in_place(
        &mut self,
        id: ShapeId,
        kind: EditKind,
        m: &MaterialState,
        textures: &dyn Fn(&str) -> Option<Vec<u8>>,
        rep: &mut LoadReport,
    ) -> Result<(), String> {
        if kind == EditKind::Textures {
            let name = self.shapes[&id].name.clone();
            let program = self.program(&perm_material(m))?;
            let (srvs, bound) = self.bind_textures(&name, &program, m, textures, rep)?;
            let d = self
                .shapes
                .get_mut(&id)
                .and_then(|s| s.draw.as_mut())
                .ok_or("no draw")?;
            d.srvs = srvs;
            d.bound = bound;
        }
        let ll = self.linear_lighting();
        let d = self
            .shapes
            .get_mut(&id)
            .and_then(|s| s.draw.as_mut())
            .ok_or("no draw")?;
        d.rewrite_material(m, ll);
        Ok(())
    }
```

- [ ] **Step 6: Stand-in hooks** — replace `crates/render3d/tests/fixtures/standin/Lighting.hlsl` completely with:

```hlsl
// Repo-owned stand-in for the Community Shaders Lighting.hlsl (written for sdc's render3d tests; not derived
// from CS). It declares the cbuffer, texture and sampler names render3d binds by name, so the whole render path
// (permutation defines, compile, reflection, cbuffer writes, default textures, draw, readback) runs on WARP
// without the user's CS install. GPL-3.0-or-later, like the rest of sdc.
//
// Failure hook for the tests: the pixel permutation without DO_ALPHA_TEST does not compile, so a fixture with
// one such shape gets exactly one compile error while the others still draw. A test that passes the feature
// define STANDIN_OPAQUE_OK lets those permutations compile (opaque shapes for the alpha check).
//
// Pixel hooks for the P1 tests (feature defines):
// - VC (render3d's vertex-colour define, VS only): the VS reads COLOR0 and passes it on; otherwise white.
// - STANDIN_FLAT: the PS returns 0.25 x diffuse x vertex colour + EmitColor, plus 0.5 green when PBRFlags is non-zero, with no
//   lighting, so cbuffer edits (emissive, PBR flags) and texture edits show in pixels without saturating.
// - STANDIN_TBN: the PS returns N.L only, N being the normal map turned into the vertex tangent frame. The frame
//   is read from the SSE vertex record the way render3d packs it (mesh.rs pack_tbn): row 0 =
//   (POSITION.w, NORMAL.w, BINORMAL.w) (the file's Bitangent), row 1 = BINORMAL.xyz (the file's Tangent),
//   row 2 = NORMAL.xyz; the map's x, y, z weight rows 0, 1, 2.

struct VsOut
{
    float4 Position : SV_Position;
    float2 Uv : TEXCOORD0;
    float3 Normal : TEXCOORD1;
    float3 Row0 : TEXCOORD2;
    float3 Row1 : TEXCOORD3;
    float4 Color : TEXCOORD4;
};

#if defined(VSHADER)

cbuffer StandinGeometry : register(b2)
{
    row_major float3x4 World;
    row_major float4x4 ViewProj;
    float4 EyePosition;
};

cbuffer StandinMaterial : register(b1)
{
    float4 TexcoordOffset;
};

struct VsIn
{
    float4 Position : POSITION0;
    float2 Uv : TEXCOORD0;
    float4 Normal : NORMAL0;
    float4 Binormal : BINORMAL0;
#if defined(VC)
    float4 Color : COLOR0;
#endif
};

VsOut main(VsIn v)
{
    VsOut o;
    float3 p = mul(World, float4(v.Position.xyz, 1.0)) - EyePosition.xyz;
    o.Position = mul(ViewProj, float4(p, 1.0));
    o.Uv = v.Uv * TexcoordOffset.zw + TexcoordOffset.xy;
    o.Normal = mul((float3x3)World, v.Normal.xyz * 2.0 - 1.0);
    o.Row0 = mul((float3x3)World, float3(v.Position.w, v.Normal.w * 2.0 - 1.0, v.Binormal.w * 2.0 - 1.0));
    o.Row1 = mul((float3x3)World, v.Binormal.xyz * 2.0 - 1.0);
#if defined(VC)
    o.Color = v.Color;
#else
    o.Color = float4(1.0, 1.0, 1.0, 1.0);
#endif
    return o;
}

#elif defined(PSHADER)

#if !defined(DO_ALPHA_TEST) && !defined(STANDIN_OPAQUE_OK)
#error stand-in: this permutation is meant to fail (test fixture)
#endif

cbuffer StandinLight : register(b2)
{
    float4 DirLightDirection;
    float4 DirLightColor;
    row_major float3x4 DirectionalAmbient;
};

cbuffer StandinMaterialPs : register(b1)
{
    float4 EmitColor;
    uint PBRFlags;
};

Texture2D<float4> DiffuseMap : register(t0);
Texture2D<float4> NormalMap : register(t1);
SamplerState DiffuseSampler : register(s0);

float4 main(VsOut i) : SV_Target
{
    float4 base = DiffuseMap.Sample(DiffuseSampler, i.Uv);
    float3 bump = NormalMap.Sample(DiffuseSampler, i.Uv).xyz * 2.0 - 1.0;
#if defined(STANDIN_TBN)
    float3 nt = normalize(bump.x * i.Row0 + bump.y * i.Row1 + bump.z * i.Normal);
    return float4(saturate(dot(nt, DirLightDirection.xyz)).xxx, 1.0);
#elif defined(STANDIN_FLAT)
    float flagGreen = (PBRFlags & 0xFFFu) != 0 ? 0.5 : 0.0;
    return float4(0.25 * base.rgb * i.Color.rgb + EmitColor.rgb + float3(0.0, flagGreen, 0.0), base.a * 0.5);
#else
    float3 n = normalize(i.Normal + 0.001 * bump);
    float ndl = saturate(dot(n, DirLightDirection.xyz));
    float3 ambient = mul(DirectionalAmbient, float4(n, 1.0));
    // A floor of half the base colour keeps every covered pixel visible whatever the normals are.
    float3 c = base.rgb * (0.5 + 0.5 * ndl * DirLightColor.rgb + ambient);
    // Alpha below 1 on purpose: the display readback must still be opaque.
    return float4(c, base.a * 0.5);
#endif
}

#endif
```

(The default path is P0's PS body unchanged, so `tests/standin.rs` keeps passing. The VS now also reads `BINORMAL0`, which `mesh::stream_for` fills from `pack_tbn`, `mesh.rs:107-110`; meshes without tangents get zeros.)

- [ ] **Step 7: Run the tests to verify they pass**

Run: `cargo test -p render3d --test edit --test material --test load --test standin`
Expected: PASS (`edit`: 9 tests; `material`: 10 tests).

- [ ] **Step 8: Gates**

Run: `cargo fmt --all && cargo clippy --workspace --all-targets -- -D warnings && cargo test --workspace`
Expected: all green.

- [ ] **Step 9: Commit**

```bash
git add crates/render3d/src/material.rs crates/render3d/src/draw.rs crates/render3d/src/lib.rs crates/render3d/tests/fixtures/standin/Lighting.hlsl crates/render3d/tests/edit.rs crates/render3d/tests/material.rs
git commit -m "feat(render3d): set_material with cbuffer/texture/permutation edits; failed edits keep the old draw" -m "Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

---

### Task 9: render3d `builtin.rs` + `load_builtin` (tangent convention from fixture data)

**Files:**
- Create: `crates/render3d/src/builtin.rs`
- Modify: `crates/render3d/src/lib.rs` (module, re-export, `load_builtin`; anchors below)
- Test: `crates/render3d/tests/builtin.rs`

**Interfaces:**
- Consumes: Task 8a `load_parsed`, `ParsedNif`, `ParsedShape`, `ShapeId`; Task 8b stand-in `STANDIN_TBN`; `nif::geom::Mesh` (`geom/mod.rs:13-50`); `mesh::validate`; fixture `crates/nif/tests/fixtures/TestNifFile_Furniture_Col_SE.nif` (exists; 2 BSTriShapes `cylinder_1`/`cylinder_2`, vertex attributes 0x1B incl. tangents) and `TestNifFile_DeepGraph_SE.nif`.
- Produces (contract, exact):
  ```rust
  // render3d::builtin, re-exported at the crate root
  #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)] pub enum Builtin { Sphere, Plane, Cube }
  pub fn builtin_mesh(b: Builtin) -> nif::geom::Mesh;
  pub const BUILTIN_SIZE: f32 = 32.0;
  impl Renderer {
      pub fn load_builtin(&mut self, b: Builtin, m: nif::material::MaterialState,
                          textures: &dyn Fn(&str) -> Option<Vec<u8>>) -> LoadReport;
  }
  ```
  `load_builtin` = `load_parsed` of one shape `ShapeId(0)` named `format!("{b:?}")`, with `skinned` and `has_vertex_colors` forced to `false`.

**The convention (03 §6, derived from data, not from field names).** Measured on `TestNifFile_Furniture_Col_SE.nif` before writing this task (mean over 136 triangles, the frame of each triangle's first vertex against the triangle's dP/du, dP/dv): `cos(tangents, dP/dv) = 1.000`, `cos(bitangents, dP/du) = 1.000`, `|cos(tangents, dP/du)| = 0.002`, `|cos(bitangents, dP/dv)| = 0.004`, `cos(cross(tangent, bitangent), N) = 0.941`; every triangle of both fixtures (136 + 2846) has `cross(p1 − p0, p2 − p0) · n > 0`. So `Mesh.tangents` (the file's `Tangent` bytes, BINORMAL.xyz) run along **+dP/dv**, `Mesh.bitangents` (the w's) along **+dP/du**, `cross(tangent, bitangent)` points along the normal, and triangles wind counter-clockwise around the normal. `tangent_convention_from_fixture` re-derives this in the gate; `builtins_follow_the_fixture_convention` holds the built-ins to it; the WARP test checks it in a shader that reads the frame the way CS's VS does (dev clone `Lighting.hlsl:206-209`, PS `mul(tbn, normal)`: map x weights the w-row = dP/du).

- [ ] **Step 1: Write the failing test** — create `crates/render3d/tests/builtin.rs`:

```rust
//! Built-in sphere / plane / cube: geometry checks, the tangent convention measured on nifly's fixtures, and the
//! convention seen by a shader (WARP, stand-in `STANDIN_TBN`).
mod common;
use common::{channel, covered, dds_rgba8, fixture, renderer};
use nif::geom::Mesh;
use nif::material::MaterialState;
use render3d::scene::Scene;
use render3d::{builtin_mesh, parse, Builtin, ShapeId, ShapeStatus};

fn sub(a: [f32; 3], b: [f32; 3]) -> [f32; 3] {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}
fn dot(a: [f32; 3], b: [f32; 3]) -> f32 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}
fn cross(a: [f32; 3], b: [f32; 3]) -> [f32; 3] {
    [
        a[1] * b[2] - a[2] * b[1],
        a[2] * b[0] - a[0] * b[2],
        a[0] * b[1] - a[1] * b[0],
    ]
}
fn len(a: [f32; 3]) -> f32 {
    dot(a, a).sqrt()
}
fn unit(a: [f32; 3]) -> [f32; 3] {
    let l = len(a);
    [a[0] / l, a[1] / l, a[2] / l]
}

/// Means over triangles of the cosines between the first vertex's frame and the triangle's dP/du, dP/dv.
#[derive(Debug)]
struct Frame {
    t_dv: f64,
    b_du: f64,
    t_du_abs: f64,
    b_dv_abs: f64,
    handed: f64,
    n: usize,
}

fn frame_stats(meshes: &[&Mesh]) -> Frame {
    let mut s = [0f64; 5];
    let mut n = 0;
    for m in meshes {
        if m.tangents.is_empty() || m.uvs.is_empty() {
            continue;
        }
        for t in &m.triangles {
            let [i0, i1, i2] = t.map(|i| i as usize);
            let (e1, e2) = (
                sub(m.positions[i1], m.positions[i0]),
                sub(m.positions[i2], m.positions[i0]),
            );
            let (du1, dv1) = (m.uvs[i1][0] - m.uvs[i0][0], m.uvs[i1][1] - m.uvs[i0][1]);
            let (du2, dv2) = (m.uvs[i2][0] - m.uvs[i0][0], m.uvs[i2][1] - m.uvs[i0][1]);
            let det = du1 * dv2 - du2 * dv1;
            if det.abs() < 1e-8 {
                continue;
            }
            let dpdu = [0, 1, 2].map(|k| (e1[k] * dv2 - e2[k] * dv1) / det);
            let dpdv = [0, 1, 2].map(|k| (e2[k] * du1 - e1[k] * du2) / det);
            let (tg, bt) = (m.tangents[i0], m.bitangents[i0]);
            if len(dpdu) < 1e-6 || len(dpdv) < 1e-6 || len(tg) < 0.5 || len(bt) < 0.5 {
                continue;
            }
            let (dpdu, dpdv, tg, bt) = (unit(dpdu), unit(dpdv), unit(tg), unit(bt));
            s[0] += dot(tg, dpdv) as f64;
            s[1] += dot(bt, dpdu) as f64;
            s[2] += dot(tg, dpdu).abs() as f64;
            s[3] += dot(bt, dpdv).abs() as f64;
            s[4] += dot(cross(tg, bt), m.normals[i0]) as f64;
            n += 1;
        }
    }
    let k = n.max(1) as f64;
    Frame {
        t_dv: s[0] / k,
        b_du: s[1] / k,
        t_du_abs: s[2] / k,
        b_dv_abs: s[3] / k,
        handed: s[4] / k,
        n,
    }
}

/// 03 T-B2. Measured when this plan was written: t_dv 1.000, b_du 1.000, |t·du| 0.002, |b·dv| 0.004, handed 0.941.
#[test]
fn tangent_convention_from_fixture() {
    let p = parse(&fixture("TestNifFile_Furniture_Col_SE.nif"));
    let meshes: Vec<&Mesh> = p.shapes.iter().map(|s| &*s.mesh).collect();
    let f = frame_stats(&meshes);
    assert!(f.n > 100, "{f:?}");
    assert!(f.t_dv > 0.9, "Mesh.tangents run along +dP/dv: {f:?}");
    assert!(f.b_du > 0.9, "Mesh.bitangents run along +dP/du: {f:?}");
    assert!(f.t_du_abs < 0.1 && f.b_dv_abs < 0.1, "{f:?}");
    assert!(f.handed > 0.5, "cross(tangent, bitangent) points along N: {f:?}");
}

#[test]
fn fixture_triangles_wind_around_their_normals() {
    for name in ["TestNifFile_Furniture_Col_SE.nif", "TestNifFile_DeepGraph_SE.nif"] {
        for s in parse(&fixture(name)).shapes {
            let m = &s.mesh;
            for t in &m.triangles {
                let [a, b, c] = t.map(|i| i as usize);
                let cr = cross(sub(m.positions[b], m.positions[a]), sub(m.positions[c], m.positions[a]));
                let n = [0, 1, 2].map(|k| m.normals[a][k] + m.normals[b][k] + m.normals[c][k]);
                assert!(dot(cr, n) > 0.0, "{name} {}: {t:?}", s.name);
            }
        }
    }
}

#[test]
fn builtins_follow_the_fixture_convention() {
    for b in [Builtin::Sphere, Builtin::Plane, Builtin::Cube] {
        let m = builtin_mesh(b);
        let f = frame_stats(&[&m]);
        assert!(f.n > 0, "{b:?}");
        assert!(f.t_dv > 0.95 && f.b_du > 0.95, "{b:?}: {f:?}");
        assert!(f.t_du_abs < 0.05 && f.b_dv_abs < 0.05, "{b:?}: {f:?}");
        assert!(f.handed > 0.99, "{b:?}: {f:?}");
    }
}

/// 03 T-B1.
#[test]
fn builtins_validate() {
    for b in [Builtin::Sphere, Builtin::Plane, Builtin::Cube] {
        let m = builtin_mesh(b);
        render3d::mesh::validate(&m).unwrap();
        assert!(m.colors.is_empty() && m.bone_weights.is_empty() && m.bone_indices.is_empty());
        assert!(m.eye.is_empty());
        assert_eq!(m.transform, Mesh::default().transform);
        assert!(m.positions.len() <= u16::MAX as usize);
        for i in 0..m.positions.len() {
            let (n, t, bt, uv) = (m.normals[i], m.tangents[i], m.bitangents[i], m.uvs[i]);
            for v in [n, t, bt] {
                assert!((len(v) - 1.0).abs() < 1e-4, "{b:?} vertex {i}: {v:?}");
            }
            assert!(dot(n, t).abs() < 1e-4 && dot(n, bt).abs() < 1e-4 && dot(t, bt).abs() < 1e-4);
            assert!((0.0..=1.0).contains(&uv[0]) && (0.0..=1.0).contains(&uv[1]));
        }
        for t in &m.triangles {
            let [a, bb, c] = t.map(|i| i as usize);
            let (pa, pb, pc) = (m.positions[a], m.positions[bb], m.positions[c]);
            let cr = cross(sub(pb, pa), sub(pc, pa));
            assert!(len(cr) > 1e-3, "{b:?}: degenerate {t:?}");
            let n = [0, 1, 2].map(|k| m.normals[a][k] + m.normals[bb][k] + m.normals[c][k]);
            assert!(dot(cr, n) > 0.0, "{b:?}: {t:?} winds clockwise");
            if b != Builtin::Plane {
                let centroid = [0, 1, 2].map(|k| (pa[k] + pb[k] + pc[k]) / 3.0);
                assert!(dot(cr, centroid) > 0.0, "{b:?}: {t:?} faces inwards");
            }
        }
    }
    assert_eq!(builtin_mesh(Builtin::Sphere).positions.len(), 65 * 33);
    assert_eq!(builtin_mesh(Builtin::Plane).positions.len(), 4);
    assert_eq!(builtin_mesh(Builtin::Cube).positions.len(), 24);
    // The plane faces the Auto camera (+Y).
    assert!(builtin_mesh(Builtin::Plane).normals.iter().all(|n| *n == [0.0, 1.0, 0.0]));
}

#[test]
fn load_builtin_is_shape_zero() {
    let cache = tempfile::tempdir().unwrap();
    let mut r = renderer(cache.path(), &["STANDIN_OPAQUE_OK"]);
    let mut m = MaterialState::builtin_default();
    m.skinned = true; // geometry-bound: forced off for a built-in
    m.has_vertex_colors = true;
    let rep = r.load_builtin(Builtin::Sphere, m, &|_| None);
    assert!(rep.errors.is_empty() && rep.compile_errors.is_empty(), "{rep:?}");
    assert_eq!(rep.shapes, 1);
    let s = r.shapes();
    assert_eq!(s.len(), 1);
    assert_eq!((s[0].id, s[0].name.as_str()), (ShapeId(0), "Sphere"));
    assert_eq!(s[0].status, ShapeStatus::Drawing);
    assert!(!s[0].material.skinned && !s[0].material.has_vertex_colors);
    let (c, radius) = r.bounds().unwrap();
    assert!(c.iter().all(|v| v.abs() < 1e-3) && (radius - 32.0).abs() < 1e-2, "{c:?} {radius}");
    assert!(covered(&r.render(&Scene::neutral(), 64, 64).unwrap()) > 64 * 64 / 10);
    // Edits work on built-ins like on NIF shapes.
    let mut uv = s[0].material.clone();
    uv.params.uv_scale = [4.0, 4.0];
    assert_eq!(r.set_material(ShapeId(0), uv, &|_| None), Ok(render3d::EditKind::Cbuffer));
}

/// 03 T-B3: a normal-map texel tilted towards tangent-space +x (+u) on the plane is lit by a light on the plane's
/// +u side and not by one on the −u side. The plane's +u is world −X (Builtin::Plane faces +Y).
#[test]
fn normal_map_tilt_follows_u_on_the_plane() {
    let cache = tempfile::tempdir().unwrap();
    let mut r = renderer(cache.path(), &["STANDIN_OPAQUE_OK", "STANDIN_TBN"]);
    // (0.70, 0.00, 0.70) in tangent space.
    let tilt = dds_rgba8(1, 1, &[[217, 128, 217, 255]]);
    let resolve = |p: &str| (p == "textures/standin/tilt_n.dds").then(|| tilt.clone());
    let mut m = MaterialState::builtin_default();
    m.set_slot_raw(1, "textures\\standin\\tilt_n.dds");
    let rep = r.load_builtin(Builtin::Plane, m, &resolve);
    assert!(rep.missing_textures.is_empty() && rep.errors.is_empty(), "{rep:?}");
    let lit_from = |r: &mut render3d::Renderer, d: [f32; 3]| {
        let mut s = Scene::neutral();
        s.dir_light.direction = d;
        channel(&r.render(&s, 64, 64).unwrap(), 0)
    };
    let h = std::f32::consts::FRAC_1_SQRT_2;
    let plus_u = lit_from(&mut r, [-h, h, 0.0]);
    let minus_u = lit_from(&mut r, [h, h, 0.0]);
    assert!(plus_u > 4 * minus_u.max(1), "+u {plus_u} vs −u {minus_u}");
    // Control: with the flat default normal map both lights give the same picture (within 2 %).
    let rep = r.load_builtin(Builtin::Plane, MaterialState::builtin_default(), &|_| None);
    assert_eq!(rep.shapes, 1);
    let (a, b) = (lit_from(&mut r, [-h, h, 0.0]), lit_from(&mut r, [h, h, 0.0]));
    assert!((a as f64 - b as f64).abs() < 0.02 * a as f64, "{a} vs {b}");
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p render3d --test builtin`
Expected: FAIL to compile, `unresolved imports render3d::builtin_mesh, render3d::Builtin` and `no method named load_builtin`.

- [ ] **Step 3: Create `crates/render3d/src/builtin.rs`**

```rust
//! Built-in preview shapes (03 §6): sphere, plane, cube; Z-up, centred at the origin, radius / half-extent
//! [`BUILTIN_SIZE`]. The tangent frame follows the convention measured on nifly's fixtures (test
//! `tangent_convention_from_fixture`): `Mesh.tangents` (the file's `Tangent` bytes) run along +dP/dv,
//! `Mesh.bitangents` (the w's) along +dP/du, and cross(tangent, bitangent) is the normal. Triangles wind
//! counter-clockwise around the normal (cross(p1 − p0, p2 − p0) · n > 0), as every fixture triangle does.
//! Texture v runs downwards on screen (DirectX): seen from outside with "up" up, u goes right and v goes down.
use nif::geom::Mesh;

/// Radius (sphere) or half-extent (plane, cube) in NIF units.
pub const BUILTIN_SIZE: f32 = 32.0;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Builtin {
    Sphere,
    Plane,
    Cube,
}

pub fn builtin_mesh(b: Builtin) -> Mesh {
    match b {
        Builtin::Sphere => sphere(),
        Builtin::Plane => plane(),
        Builtin::Cube => cube(),
    }
}

fn cross(a: [f32; 3], b: [f32; 3]) -> [f32; 3] {
    [
        a[1] * b[2] - a[2] * b[1],
        a[2] * b[0] - a[0] * b[2],
        a[0] * b[1] - a[1] * b[0],
    ]
}

/// 64 × 32 lat-long with a duplicated seam column (65 × 33 = 2,145 vertices). u = longitude / 2π (east),
/// v = colatitude / π (north pole v = 0). dP/dv points south, dP/du east; cross(south, east) = outward. The
/// degenerate triangle of each quad touching a pole is left out.
fn sphere() -> Mesh {
    const COLS: u16 = 64;
    const ROWS: u16 = 32;
    let mut m = Mesh::default();
    for i in 0..=ROWS {
        let v = i as f32 / ROWS as f32;
        let theta = v * std::f32::consts::PI;
        for j in 0..=COLS {
            let u = j as f32 / COLS as f32;
            let phi = u * std::f32::consts::TAU;
            let n = [theta.sin() * phi.cos(), theta.sin() * phi.sin(), theta.cos()];
            m.positions.push(n.map(|c| c * BUILTIN_SIZE));
            m.normals.push(n);
            m.uvs.push([u, v]);
            m.tangents
                .push([theta.cos() * phi.cos(), theta.cos() * phi.sin(), -theta.sin()]);
            m.bitangents.push([-phi.sin(), phi.cos(), 0.0]);
        }
    }
    let at = |i: u16, j: u16| i * (COLS + 1) + j;
    for i in 0..ROWS {
        for j in 0..COLS {
            let (a, b, c, d) = (at(i, j), at(i, j + 1), at(i + 1, j), at(i + 1, j + 1));
            if i + 1 < ROWS {
                m.triangles.push([a, c, d]);
            }
            if i > 0 {
                m.triangles.push([a, d, b]);
            }
        }
    }
    m
}

/// Two triangles facing +Y (towards `Camera::Auto`, which looks along −Y). Seen from +Y, screen right is −X and
/// screen down is −Z, so u runs along −X and v along −Z.
fn plane() -> Mesh {
    let s = BUILTIN_SIZE;
    Mesh {
        positions: vec![[s, 0.0, s], [-s, 0.0, s], [s, 0.0, -s], [-s, 0.0, -s]],
        uvs: vec![[0.0, 0.0], [1.0, 0.0], [0.0, 1.0], [1.0, 1.0]],
        normals: vec![[0.0, 1.0, 0.0]; 4],
        tangents: vec![[0.0, 0.0, -1.0]; 4],
        bitangents: vec![[-1.0, 0.0, 0.0]; 4],
        triangles: vec![[0, 2, 3], [0, 3, 1]],
        ..Mesh::default()
    }
}

/// 24 vertices, one 0..1 UV square per face. Per face with normal N: dP/dv = V (−Z on the sides, −Y on top,
/// +Y underneath), dP/du = U = N × V, so cross(V, U) = N.
fn cube() -> Mesh {
    let s = BUILTIN_SIZE;
    let faces: [[f32; 3]; 6] = [
        [1.0, 0.0, 0.0],
        [-1.0, 0.0, 0.0],
        [0.0, 1.0, 0.0],
        [0.0, -1.0, 0.0],
        [0.0, 0.0, 1.0],
        [0.0, 0.0, -1.0],
    ];
    let mut m = Mesh::default();
    for (f, n) in faces.iter().enumerate() {
        let v_dir = if n[2] > 0.5 {
            [0.0, -1.0, 0.0]
        } else if n[2] < -0.5 {
            [0.0, 1.0, 0.0]
        } else {
            [0.0, 0.0, -1.0]
        };
        let u_dir = cross(*n, v_dir);
        for (u, v) in [(0.0f32, 0.0f32), (1.0, 0.0), (0.0, 1.0), (1.0, 1.0)] {
            let p = [0, 1, 2]
                .map(|k| s * (n[k] + (2.0 * u - 1.0) * u_dir[k] + (2.0 * v - 1.0) * v_dir[k]));
            m.positions.push(p);
            m.uvs.push([u, v]);
            m.normals.push(*n);
            m.tangents.push(v_dir);
            m.bitangents.push(u_dir);
        }
        let b = (4 * f) as u16;
        m.triangles.push([b, b + 2, b + 3]);
        m.triangles.push([b, b + 3, b + 1]);
    }
    m
}
```

- [ ] **Step 4: Wire it into `lib.rs`** — exact replacements in `crates/render3d/src/lib.rs`:

1. Replace `pub mod cbuffer;` (first line) with

```rust
pub mod builtin;
pub mod cbuffer;
```

2. Replace `pub use material::{classify, pbr_flags, Bound, EditKind};` with

```rust
pub use builtin::{builtin_mesh, Builtin};
pub use material::{classify, pbr_flags, Bound, EditKind};
```

3. Insert after the `load_parsed` method (before `pub fn shapes`):

```rust
    /// Replaces the loaded model with one built-in shape: `ShapeId(0)`, named after the shape. The
    /// geometry-bound fields are forced to match the mesh (no bones, no vertex colours).
    pub fn load_builtin(
        &mut self,
        b: Builtin,
        m: MaterialState,
        textures: &dyn Fn(&str) -> Option<Vec<u8>>,
    ) -> LoadReport {
        let mut material = m;
        material.skinned = false;
        material.has_vertex_colors = false;
        let parsed = ParsedNif {
            shapes: vec![ParsedShape {
                id: ShapeId(0),
                name: format!("{b:?}"),
                mesh: Arc::new(builtin_mesh(b)),
                material,
            }],
            ..ParsedNif::default()
        };
        self.load_parsed(&parsed, textures)
    }
```

- [ ] **Step 5: Run test to verify it passes**

Run: `cargo test -p render3d --test builtin`
Expected: PASS, 6 tests. If `tangent_convention_from_fixture` fails, stop: the fixture contradicts the measured convention and `builtin.rs` must not be adjusted by guessing (report the printed `Frame`).

- [ ] **Step 6: Gates**

Run: `cargo fmt --all && cargo clippy --workspace --all-targets -- -D warnings && cargo test --workspace`
Expected: all green.

- [ ] **Step 7: Commit**

```bash
git add crates/render3d/src/builtin.rs crates/render3d/src/lib.rs crates/render3d/tests/builtin.rs
git commit -m "feat(render3d): built-in sphere/plane/cube with the fixture-measured tangent convention, load_builtin" -m "Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

---

### Task 10: render3d per-frame cost (resolved fields, static/view split, dirty uploads, cached query + staging)

**Files:**
- Modify: `crates/render3d/src/cbuffer.rs` (`Field`, `resolve`, `set_at`, `set_u32_at`; `put` re-expressed through `put_at`)
- Modify: `crates/render3d/src/scene.rs:291, 342` (`FEATURE_FLOATS` / `FEATURE_UINTS` become `pub`) and add `VIEW_NAMES`, `LIGHT_NAMES`, `ViewValues`, `LightValues`, `SceneFields`, `write_static`
- Modify (full replacement): `crates/render3d/src/draw.rs`
- Modify: `crates/render3d/src/device.rs:239-246, 306-332, 374-377` (staging texture in `Target`)
- Modify: `crates/render3d/src/lib.rs` (Renderer fields, `with_gpu`, `build_draw`, `render`, `wait_idle`, two new pub fns; anchors below)
- Test: `crates/render3d/tests/frame.rs`

**Interfaces:**
- Consumes: Task 8a/8b `Renderer`, `ShapeDraw`, `rewrite_material`; `Scene::write` (`scene.rs:368-455`, kept unchanged as the by-name reference).
- Produces:
  ```rust
  // cbuffer.rs
  #[derive(Debug, Clone, Copy, PartialEq, Eq)] pub struct Field { pub off: u32, pub size: u32 }
  impl CbWriter { pub fn resolve(&self, var: &str) -> Option<Field>; pub fn set_at(&mut self, f: Field, data: &[f32]); pub fn set_u32_at(&mut self, f: Field, data: &[u32]); }
  // scene.rs
  pub const VIEW_NAMES: [&str; 17]; pub const LIGHT_NAMES: [&str; 6];
  pub struct ViewValues { pub values: Vec<Vec<f32>> }   impl ViewValues { pub fn new(v: &View) -> ViewValues; }
  pub struct LightValues { pub values: Vec<Vec<f32>> }  impl LightValues { pub fn new(s: &Scene) -> LightValues; }
  #[derive(Debug, Clone, Default, PartialEq)] pub struct SceneFields { pub world: Option<Field>, pub previous_world: Option<Field>, pub view: Vec<Option<Field>>, pub lights: Vec<Option<Field>> }
  impl SceneFields { pub fn resolve(w: &CbWriter) -> SceneFields; pub fn write_view(&self, w: &mut CbWriter, v: &ViewValues, world: &[f32]) -> bool; pub fn write_lights(&self, w: &mut CbWriter, l: &LightValues) -> bool; }
  pub fn write_static(w: &mut CbWriter);
  pub const FEATURE_FLOATS: &[(&str, &[f32])]; pub const FEATURE_UINTS: &[(&str, u32)];
  // draw.rs
  pub struct FrameInputs<'a> { pub view: &'a View, pub view_values: &'a ViewValues, pub lights: &'a LightValues, pub lights_changed: bool, pub view_changed: bool }
  impl ShapeDraw { pub fn update(&mut self, gpu: &Gpu, f: &FrameInputs) -> usize; pub fn update_by_name(&mut self, gpu: &Gpu, scene: &Scene, view: &View) -> usize; }
  // lib.rs
  impl Renderer { pub fn last_uploads(&self) -> usize; #[doc(hidden)] pub fn set_reference_writer(&mut self, on: bool); }
  // device.rs
  pub struct Target { …, pub staging: ID3D11Texture2D }
  ```
  The split covers exactly the names `Scene::write` writes: per frame `World`, `PreviousWorld` + `VIEW_NAMES`; on a light change `LIGHT_NAMES`; at build and after a material rewrite everything else (`write_static`). Acceptance (03 §7): writer bytes and frame pixels identical to the by-name writer (tests); `draw_ms` on DeepGraph recorded with `--nocapture`, not asserted.

- [ ] **Step 1: Write the failing test** — create `crates/render3d/tests/frame.rs`:

```rust
//! 03 §7 per-frame cost: resolved fields write the same bytes and pixels as the by-name writer; an unchanged
//! frame uploads nothing; `draw_ms` is recorded (not asserted).
mod common;
use common::{dds_rgba8, fixture, renderer, FLAT};
use nif::material::MaterialState;
use render3d::cbuffer::CbWriter;
use render3d::compile::CbLayout;
use render3d::material::{write_material, Bound};
use render3d::scene::{
    rows, write_static, Camera, LightValues, Scene, SceneFields, View, ViewValues, FEATURE_FLOATS,
    FEATURE_UINTS, LIGHT_NAMES, VIEW_NAMES,
};
use render3d::{Renderer, ShapeId};

const STATIC_NAMES: &[&str] = &[
    "EyePosition",
    "FogParam",
    "ColourOutputClamp",
    "AmbientSpecularTintAndFresnelPower",
    "SSRParams",
    "Timer",
    "WaterSystemHeight",
    "WaterData",
    "FrameParams",
    "DynamicResolutionParams1",
    "DynamicResolutionParams2",
    "InInterior",
];
const MATERIAL_NAMES: &[&str] = &[
    "TexcoordOffset",
    "SpecularColor",
    "PBRFlags",
    "PBRParams1",
    "PBRParams2",
    "MultiLayerParallaxData",
    "EmitColor",
    "emissiveMult",
    "MaterialData",
    "AlphaTestRefRS",
    "PixelShaderDescriptor",
    "ExtraShaderDescriptor",
    "LightingEffectParams",
    "MaterialObjectRGBScale",
    "Bones",
    "PreviousBones",
];

fn all_names() -> Vec<String> {
    let mut v: Vec<String> = ["World", "PreviousWorld"].iter().map(|s| s.to_string()).collect();
    v.extend(VIEW_NAMES.iter().chain(LIGHT_NAMES.iter()).chain(STATIC_NAMES).chain(MATERIAL_NAMES).map(|s| s.to_string()));
    v.extend(FEATURE_FLOATS.iter().map(|(n, _)| n.to_string()));
    v.extend(FEATURE_UINTS.iter().map(|(n, _)| n.to_string()));
    v
}

fn layout(names: &[String], size: u32) -> CbLayout {
    CbLayout {
        name: "Synthetic".into(),
        slot: 0,
        size: size * names.len() as u32,
        vars: names
            .iter()
            .enumerate()
            .map(|(i, n)| (n.clone(), size * i as u32, size))
            .collect(),
    }
}

/// 03 T-F1, writer level: every scene and material field, two layouts (plain names with room for every value;
/// namespaced, reordered, subset, truncated to 32 bytes).
#[test]
fn resolved_fields_write_same_bytes_as_by_name() {
    let names = all_names();
    let mut ns: Vec<String> = names
        .iter()
        .enumerate()
        .filter(|(i, _)| i % 3 != 1)
        .map(|(_, n)| {
            if n.contains('.') {
                format!("SharedData::{n}")
            } else {
                format!("FrameBuffer::{n}")
            }
        })
        .collect();
    ns.reverse();
    let mut scene = Scene::neutral();
    scene.dir_light.direction = [0.3, -0.2, 0.93];
    let view = View::new(
        &Camera::Orbit { target: [1.0, 2.0, 3.0], yaw: 0.7, pitch: 0.3, distance: 90.0 },
        Some(([1.0, 2.0, 3.0], 20.0)),
        320,
        200,
    );
    let model = [[0.0, -2.0, 0.0, 5.0], [2.0, 0.0, 0.0, -7.0], [0.0, 0.0, 2.0, 11.0]];
    let mut m = MaterialState::builtin_default();
    m.slsf2 |= (1 << 23) | (1 << 26);
    let bound = Bound { regs: [12].into_iter().collect() };
    for l in [layout(&names, 400), layout(&ns, 32)] {
        let mut by_name = CbWriter::new(&l);
        write_material(&mut by_name, &m, &bound, true);
        scene.write(&mut by_name, &view, &model);
        let mut resolved = CbWriter::new(&l);
        write_material(&mut resolved, &m, &bound, true);
        write_static(&mut resolved);
        let f = SceneFields::resolve(&resolved);
        assert!(f.write_lights(&mut resolved, &LightValues::new(&scene)));
        assert!(f.write_view(&mut resolved, &ViewValues::new(&view), &rows(&view.world(&model))));
        assert_eq!(by_name.bytes(), resolved.bytes(), "layout {:?}…", &l.vars[..2]);
    }
}

fn checker() -> Vec<u8> {
    dds_rgba8(2, 2, &[[255, 0, 0, 255], [0, 0, 255, 255], [0, 0, 255, 255], [255, 0, 0, 255]])
}

fn same(a: &mut Renderer, b: &mut Renderer, s: &Scene, w: u32, h: u32, what: &str) {
    assert_eq!(a.render(s, w, h).unwrap(), b.render(s, w, h).unwrap(), "{what}");
}

/// 03 §7 acceptance: the resolved / dirty-upload path draws byte-identical pixels to the by-name writer through a
/// first frame, an unchanged frame, a camera move, a light change, a cbuffer edit and a resize.
#[test]
fn pixels_match_the_by_name_writer() {
    let tex = checker();
    let resolve = |_: &str| Some(tex.clone());
    let (ca, cb) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
    let mut a = renderer(ca.path(), &["STANDIN_OPAQUE_OK"]);
    let mut b = renderer(cb.path(), &["STANDIN_OPAQUE_OK"]);
    b.set_reference_writer(true);
    let nif = fixture("TestNifFile_DeepGraph_SE.nif");
    assert_eq!(a.load(&nif, &resolve), b.load(&nif, &resolve));
    let mut scene = Scene::neutral();
    same(&mut a, &mut b, &scene, 96, 96, "first frame");
    same(&mut a, &mut b, &scene, 96, 96, "unchanged frame");
    let (c, r) = a.bounds().unwrap();
    scene.camera = Camera::Orbit { target: c, yaw: 2.0, pitch: 0.5, distance: 3.0 * r };
    same(&mut a, &mut b, &scene, 96, 96, "camera move");
    scene.dir_light.direction = [-0.6, 0.3, 0.74];
    scene.ambient6[4] = [0.5; 3];
    same(&mut a, &mut b, &scene, 96, 96, "light change");
    let peg = ShapeId(103);
    let mut m = a.shapes().into_iter().find(|s| s.id == peg).unwrap().material;
    m.params.uv_scale = [3.0, 3.0];
    for x in [&mut a, &mut b] {
        assert_eq!(x.set_material(peg, m.clone(), &resolve), Ok(render3d::EditKind::Cbuffer));
    }
    same(&mut a, &mut b, &scene, 96, 96, "cbuffer edit");
    same(&mut a, &mut b, &scene, 64, 48, "resize");
}

#[test]
fn unchanged_frame_uploads_nothing_and_an_edit_uploads_one_shape() {
    let cache = tempfile::tempdir().unwrap();
    let mut r = renderer(cache.path(), FLAT);
    let rep = r.load(&fixture("TestNifFile_DeepGraph_SE.nif"), &|_| None);
    assert_eq!(rep.shapes, 27);
    let mut scene = Scene::neutral();
    r.render(&scene, 64, 64).unwrap();
    assert!(r.last_uploads() >= 27, "{}", r.last_uploads());
    r.render(&scene, 64, 64).unwrap();
    assert_eq!(r.last_uploads(), 0, "same camera, same lights");
    let peg = ShapeId(103);
    let mut m = r.shapes().into_iter().find(|s| s.id == peg).unwrap().material;
    m.params.emissive_color = [0.2, 0.0, 0.0];
    assert_eq!(r.set_material(peg, m, &|_| None), Ok(render3d::EditKind::Cbuffer));
    r.render(&scene, 64, 64).unwrap();
    // The stand-in has 4 cbuffers per draw (VS b1/b2, PS b1/b2).
    assert!((1..=4).contains(&r.last_uploads()), "{}", r.last_uploads());
    let (c, rad) = r.bounds().unwrap();
    scene.camera = Camera::Orbit { target: c, yaw: 1.0, pitch: 0.2, distance: 3.0 * rad };
    r.render(&scene, 64, 64).unwrap();
    assert!(r.last_uploads() >= 27, "a camera move rewrites every draw's view fields");
}

/// Recorded, not asserted (03 §7: "draw_ms drops by at least 2× against P0" goes into the phase report).
/// `cargo test -p render3d --test frame record_draw_ms -- --nocapture`
#[test]
fn record_draw_ms_deep_graph() {
    for reference in [true, false] {
        let cache = tempfile::tempdir().unwrap();
        let mut r = renderer(cache.path(), FLAT);
        r.set_reference_writer(reference);
        r.load(&fixture("TestNifFile_DeepGraph_SE.nif"), &|_| None);
        let (c, rad) = r.bounds().unwrap();
        let mut scene = Scene::neutral();
        r.render(&scene, 128, 128).unwrap();
        let (mut moving, mut still) = (0.0, 0.0);
        for i in 0..20 {
            scene.camera = Camera::Orbit { target: c, yaw: 0.1 * i as f32, pitch: 0.3, distance: 3.0 * rad };
            r.render(&scene, 128, 128).unwrap();
            moving += r.last_timing().draw_ms;
        }
        for _ in 0..20 {
            r.render(&scene, 128, 128).unwrap();
            still += r.last_timing().draw_ms;
        }
        eprintln!(
            "DeepGraph (27 shapes, WARP) draw_ms per frame, {}: moving camera {:.3}, still camera {:.3}",
            if reference { "by-name writer (P0)" } else { "resolved + dirty uploads" },
            moving / 20.0,
            still / 20.0
        );
    }
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p render3d --test frame`
Expected: FAIL to compile, `unresolved imports render3d::scene::{write_static, LightValues, SceneFields, ViewValues, VIEW_NAMES, LIGHT_NAMES}`, `FEATURE_FLOATS is private`, `no method named set_reference_writer / last_uploads`.

- [ ] **Step 3: Resolved fields in `cbuffer.rs`** — replace `fn put(&mut self, var: &str, words: impl Iterator<Item = [u8; 4]>) -> bool { … }` (`cbuffer.rs:41-52`) with:

```rust
    fn put(&mut self, var: &str, words: impl Iterator<Item = [u8; 4]>) -> bool {
        let Some(f) = self.resolve(var) else {
            return false;
        };
        self.put_at(f, words);
        true
    }

    fn put_at(&mut self, f: Field, words: impl Iterator<Item = [u8; 4]>) {
        let (off, size) = (f.off as usize, f.size as usize);
        for (i, w) in words.take(size / 4).enumerate() {
            let at = off + 4 * i;
            if let Some(dst) = self.data.get_mut(at..at + 4) {
                dst.copy_from_slice(&w);
            }
        }
    }

    /// Where `var` lives in this layout (namespace-tolerant, see [`name_matches`]); resolve once, then
    /// [`CbWriter::set_at`] every frame without a name search.
    pub fn resolve(&self, var: &str) -> Option<Field> {
        self.find(var).map(|(off, size)| Field {
            off: off as u32,
            size: size as u32,
        })
    }

    /// As [`CbWriter::set`] at a resolved field.
    pub fn set_at(&mut self, f: Field, data: &[f32]) {
        self.put_at(f, data.iter().map(|x| x.to_le_bytes()));
    }

    /// As [`CbWriter::set_u32`] at a resolved field.
    pub fn set_u32_at(&mut self, f: Field, data: &[u32]) {
        self.put_at(f, data.iter().map(|x| x.to_le_bytes()));
    }
```

and add before `pub struct CbWriter`:

```rust
/// A resolved cbuffer variable: byte offset and size in one writer's layout.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Field {
    pub off: u32,
    pub size: u32,
}
```

- [ ] **Step 4: The split in `scene.rs`** — change `const FEATURE_FLOATS` (line 291) to `pub const FEATURE_FLOATS` and `const FEATURE_UINTS` (line 342) to `pub const FEATURE_UINTS`; change `use crate::cbuffer::CbWriter;` (line 6) to `use crate::cbuffer::{CbWriter, Field};`; then insert before `#[cfg(test)]` (line 458):

```rust
/// Camera fields that change with the eye or the target size, in [`ViewValues`] order. `World` and
/// `PreviousWorld` depend on the shape too and are passed to [`SceneFields::write_view`] separately.
pub const VIEW_NAMES: [&str; 17] = [
    "ViewProj",
    "BonesPivot",
    "PreviousBonesPivot",
    "BufferDim",
    "CameraData",
    "CameraView",
    "CameraProj",
    "CameraProjUnjittered",
    "CameraViewProj",
    "CameraViewProjUnjittered",
    "CameraPreviousViewProjUnjittered",
    "CameraViewInverse",
    "CameraProjInverse",
    "CameraProjUnjitteredInverse",
    "CameraViewProjInverse",
    "CameraPosAdjust",
    "CameraPreviousPosAdjust",
];

/// Fields that change with the scene's lights, in [`LightValues`] order.
pub const LIGHT_NAMES: [&str; 6] = [
    "DirLightDirection",
    "DirLightColor",
    "DirectionalAmbient",
    "AmbientSHR",
    "AmbientSHG",
    "AmbientSHB",
];

/// The values of [`VIEW_NAMES`] for one frame, computed once per frame (the four inverses included) instead of
/// once per cbuffer (03 §7). Same values as [`Scene::write`].
pub struct ViewValues {
    pub values: Vec<Vec<f32>>,
}

impl ViewValues {
    pub fn new(v: &View) -> ViewValues {
        let (fw, fh) = (v.width.max(1) as f32, v.height.max(1) as f32);
        let vp = rows(&v.view_proj);
        let eye4 = vec![v.eye[0], v.eye[1], v.eye[2], 0.0];
        ViewValues {
            values: vec![
                vp.clone(),
                v.eye.to_vec(),
                v.eye.to_vec(),
                vec![fw, fh, 1.0 / fw, 1.0 / fh],
                vec![v.far, v.near, v.far - v.near, v.far * v.near],
                rows(&v.view),
                rows(&v.proj),
                rows(&v.proj),
                vp.clone(),
                vp.clone(),
                vp,
                rows(&inverse4(&v.view)),
                rows(&inverse4(&v.proj)),
                rows(&inverse4(&v.proj)),
                rows(&inverse4(&v.view_proj)),
                eye4.clone(),
                eye4,
            ],
        }
    }
}

/// The values of [`LIGHT_NAMES`] for one scene. Same values as [`Scene::write`].
pub struct LightValues {
    pub values: Vec<Vec<f32>>,
}

impl LightValues {
    pub fn new(s: &Scene) -> LightValues {
        let l = s.dir_light;
        let sh = dalc_to_sh(&s.ambient6);
        LightValues {
            values: vec![
                vec![l.direction[0], l.direction[1], l.direction[2], 0.0],
                vec![l.color[0], l.color[1], l.color[2], 0.0],
                rows(&dalc(&s.ambient6)),
                sh[0].to_vec(),
                sh[1].to_vec(),
                sh[2].to_vec(),
            ],
        }
    }
}

/// Where one cbuffer keeps the per-frame and light fields, resolved once when the draw is built.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct SceneFields {
    pub world: Option<Field>,
    pub previous_world: Option<Field>,
    pub view: Vec<Option<Field>>,
    pub lights: Vec<Option<Field>>,
}

impl SceneFields {
    pub fn resolve(w: &CbWriter) -> SceneFields {
        SceneFields {
            world: w.resolve("World"),
            previous_world: w.resolve("PreviousWorld"),
            view: VIEW_NAMES.iter().map(|n| w.resolve(n)).collect(),
            lights: LIGHT_NAMES.iter().map(|n| w.resolve(n)).collect(),
        }
    }

    /// Writes `World` / `PreviousWorld` (`world` = `rows(&view.world(model))`) and the camera fields. True if
    /// this cbuffer has any of them (it must then be uploaded).
    pub fn write_view(&self, w: &mut CbWriter, v: &ViewValues, world: &[f32]) -> bool {
        let mut any = false;
        for f in [self.world, self.previous_world].into_iter().flatten() {
            w.set_at(f, world);
            any = true;
        }
        for (f, vals) in self.view.iter().zip(&v.values) {
            if let Some(f) = f {
                w.set_at(*f, vals);
                any = true;
            }
        }
        any
    }

    /// Writes the light fields. True if this cbuffer has any of them.
    pub fn write_lights(&self, w: &mut CbWriter, l: &LightValues) -> bool {
        let mut any = false;
        for (f, vals) in self.lights.iter().zip(&l.values) {
            if let Some(f) = f {
                w.set_at(*f, vals);
                any = true;
            }
        }
        any
    }
}

/// Every field [`Scene::write`] writes that depends on neither the camera nor the lights: constants and the
/// FeatureData block (03 §7 "static"). Written when a draw is built and after every material rewrite (the
/// rewrite zeroes the whole writer).
pub fn write_static(w: &mut CbWriter) {
    // Camera-relative: the eye is the origin.
    w.set("EyePosition", &[0.0, 0.0, 0.0, 0.0]);
    // Vertex fog off: FogParam.w (max fog) = 0, and log2 of 0 → exp2(−inf) = 0, no NaN.
    w.set("FogParam", &[0.0, 0.0, 1.0, 0.0]);
    w.set("ColourOutputClamp", &[1.0, 1.0, 1.0, 0.0]);
    w.set("AmbientSpecularTintAndFresnelPower", &[0.0, 0.0, 0.0, 1.0]);
    w.set("SSRParams", &[0.1, 0.1, 1.0, 1.0]);
    w.set("Timer", &[1.0]);
    w.set("WaterSystemHeight", &[f32::MIN]);
    w.set("WaterData", &[0.58, 0.58, 0.58, f32::MIN].repeat(25));
    w.set("FrameParams", &[1.0, 0.0, 0.0, 0.0]);
    w.set("DynamicResolutionParams1", &[1.0; 4]);
    w.set("DynamicResolutionParams2", &[1.0; 4]);
    w.set_u32("InInterior", &[1]);
    for (n, d) in FEATURE_FLOATS {
        w.set(n, d);
    }
    for (n, d) in FEATURE_UINTS {
        w.set_u32(n, &[*d]);
    }
}
```

- [ ] **Step 5: `draw.rs`** — replace the whole file with:

```rust
//! One NIF shape → one forward CS Lighting draw: permutation, VS/PS, zero-initialised cbuffers filled from
//! the material and the stub scene, SRVs for every reflected register, one sampler per reflected slot.
//! P0 is forward only (no `DEFERRED`, no 8 MRTs); in-game opaque objects take the deferred path
//! (answers.md, hint 15), but the forward permutation of the same Lighting.hlsl is what P0 renders.
use crate::cbuffer::CbWriter;
use crate::compile::{Compiled, Reflection, SrvKind};
use crate::device::Gpu;
use crate::material::{write_material, Bound};
use crate::mesh::GpuMesh;
use crate::scene::{rows, write_static, LightValues, Mat3x4, Scene, SceneFields, View, ViewValues};
use nif::material::MaterialState;
use windows::Win32::Graphics::Direct3D11::{
    ID3D11Buffer, ID3D11PixelShader, ID3D11ShaderResourceView, ID3D11VertexShader,
    D3D11_BIND_CONSTANT_BUFFER, D3D11_BUFFER_DESC, D3D11_USAGE_DEFAULT,
};

/// A cbuffer of one stage: its slot, the writer, where the scene fields live in it, the GPU buffer, and whether
/// the writer changed since the last upload.
pub struct Cb {
    pub slot: u32,
    pub writer: CbWriter,
    pub fields: SceneFields,
    pub buffer: ID3D11Buffer,
    pub dirty: bool,
}

/// One zero-initialised [`Cb`] per reflected cbuffer, scene fields resolved, marked dirty.
pub fn cbuffers(gpu: &Gpu, r: &Reflection) -> Result<Vec<Cb>, String> {
    r.cbuffers
        .iter()
        .map(|l| {
            let writer = CbWriter::new(l);
            let d = D3D11_BUFFER_DESC {
                ByteWidth: writer.bytes().len().max(16) as u32,
                Usage: D3D11_USAGE_DEFAULT,
                BindFlags: D3D11_BIND_CONSTANT_BUFFER.0 as u32,
                CPUAccessFlags: 0,
                MiscFlags: 0,
                StructureByteStride: 0,
            };
            let mut b = None;
            // SAFETY: descriptor is a local; no initial data.
            unsafe { gpu.device.CreateBuffer(&d, None, Some(&mut b)) }
                .map_err(|e| gpu.fail(&format!("CreateBuffer({})", l.name), e))?;
            Ok(Cb {
                slot: l.slot,
                fields: SceneFields::resolve(&writer),
                writer,
                buffer: b.ok_or("CreateBuffer returned no buffer")?,
                dirty: true,
            })
        })
        .collect()
}

/// Everything needed to draw one shape.
pub struct ShapeDraw {
    pub name: String,
    pub vs: ID3D11VertexShader,
    pub ps: ID3D11PixelShader,
    pub mesh: GpuMesh,
    pub vs_cbs: Vec<Cb>,
    pub ps_cbs: Vec<Cb>,
    /// (register, view) for every SRV the PS reflects.
    pub srvs: Vec<(u32, ID3D11ShaderResourceView)>,
    pub samplers: Vec<u32>,
    pub model: Mat3x4,
    pub blend: bool,
    pub skinned: bool,
    /// Registers bound to a real texture (not a 1×1 default); the PBR flags and `EnvmapData.y` read it.
    pub bound: Bound,
    /// New or rewritten since the last frame: the next [`ShapeDraw::update`] writes the light and view fields
    /// whatever changed.
    pub stale: bool,
}

/// The VS/PS pair of a permutation, compiled and created once per renderer.
#[derive(Clone)]
pub struct Program {
    pub vs: Compiled,
    pub ps: Compiled,
    pub vs_obj: ID3D11VertexShader,
    pub ps_obj: ID3D11PixelShader,
}

pub fn create_program(gpu: &Gpu, vs: Compiled, ps: Compiled) -> Result<Program, String> {
    let (mut v, mut p) = (None, None);
    // SAFETY: bytecode slices are valid for the calls; out-pointers are locals.
    unsafe {
        gpu.device
            .CreateVertexShader(&vs.bytecode, None, Some(&mut v))
            .map_err(|e| gpu.fail("CreateVertexShader", e))?;
        gpu.device
            .CreatePixelShader(&ps.bytecode, None, Some(&mut p))
            .map_err(|e| gpu.fail("CreatePixelShader", e))?;
    }
    Ok(Program {
        vs_obj: v.ok_or("no vertex shader")?,
        ps_obj: p.ok_or("no pixel shader")?,
        vs,
        ps,
    })
}

/// Which reflected SRV kind lives at `reg`, if the PS reads it.
pub fn srv_kind_at(r: &Reflection, reg: u32) -> Option<SrvKind> {
    r.srvs.iter().find(|s| s.1 == reg).map(|s| s.2)
}

/// What changed since the last frame, plus the values computed once per frame.
pub struct FrameInputs<'a> {
    pub view: &'a View,
    pub view_values: &'a ViewValues,
    pub lights: &'a LightValues,
    pub lights_changed: bool,
    pub view_changed: bool,
}

fn upload(gpu: &Gpu, cb: &Cb) {
    // SAFETY: the buffer was created with exactly `bytes().len()` bytes (≥ 16).
    unsafe {
        gpu.ctx.UpdateSubresource(
            &cb.buffer,
            0,
            None,
            cb.writer.bytes().as_ptr() as _,
            0,
            0,
        )
    };
}

impl ShapeDraw {
    /// Writes only what changed (03 §7) through the resolved fields and uploads only the cbuffers it touched.
    /// Returns the number of uploads.
    pub fn update(&mut self, gpu: &Gpu, f: &FrameInputs) -> usize {
        let full = std::mem::take(&mut self.stale);
        let (lights, view) = (full || f.lights_changed, full || f.view_changed);
        let world = if view {
            rows(&f.view.world(&self.model))
        } else {
            vec![]
        };
        let mut uploads = 0;
        for cb in self.vs_cbs.iter_mut().chain(self.ps_cbs.iter_mut()) {
            if lights && cb.fields.write_lights(&mut cb.writer, f.lights) {
                cb.dirty = true;
            }
            if view && cb.fields.write_view(&mut cb.writer, f.view_values, &world) {
                cb.dirty = true;
            }
            if cb.dirty {
                upload(gpu, cb);
                cb.dirty = false;
                uploads += 1;
            }
        }
        uploads
    }

    /// P0's writer, kept as the reference for the pixel-identity test: the whole scene by name into every
    /// cbuffer, every frame, and every cbuffer uploaded. Returns the number of uploads.
    pub fn update_by_name(&mut self, gpu: &Gpu, scene: &Scene, view: &View) -> usize {
        let mut uploads = 0;
        for cb in self.vs_cbs.iter_mut().chain(self.ps_cbs.iter_mut()) {
            scene.write(&mut cb.writer, view, &self.model);
            upload(gpu, cb);
            cb.dirty = false;
            uploads += 1;
        }
        self.stale = false;
        uploads
    }

    /// A cbuffer edit (03 §1.3): every writer back to zero, then the material and the static block, so no field
    /// of the previous material survives. The light and view fields come back with the next update (`stale`).
    pub fn rewrite_material(&mut self, m: &MaterialState, linear_lighting: bool) {
        for cb in self.vs_cbs.iter_mut().chain(self.ps_cbs.iter_mut()) {
            cb.writer.zero();
            write_material(&mut cb.writer, m, &self.bound, linear_lighting);
            write_static(&mut cb.writer);
            cb.dirty = true;
        }
        self.blend = m.blend();
        self.stale = true;
    }
}
```

- [ ] **Step 6: Staging texture in `device.rs`** — in `pub struct Target` (lines 239-246) add after `pub dsv: ID3D11DepthStencilView,`:

```rust
    /// CPU-readable copy target for the readback, created with the target (03 §7).
    pub staging: ID3D11Texture2D,
```

In `Target::new` (lines 306-332) replace

```rust
        let depth = create_tex(gpu, &dd)?;
```

with

```rust
        let depth = create_tex(gpu, &dd)?;
        let mut sd = tex2d_desc(w, h, DXGI_FORMAT_R16G16B16A16_FLOAT);
        sd.Usage = D3D11_USAGE_STAGING;
        sd.CPUAccessFlags = D3D11_CPU_ACCESS_READ.0 as u32;
        let staging = create_tex(gpu, &sd)?;
```

and replace `            dsv: dsv.ok_or_else(|| gpu.fail("CreateDepthStencilView", "no view returned"))?,` with

```rust
            dsv: dsv.ok_or_else(|| gpu.fail("CreateDepthStencilView", "no view returned"))?,
            staging,
```

In `fn read` delete lines 374-377

```rust
        let mut sd = tex2d_desc(self.width, self.height, DXGI_FORMAT_R16G16B16A16_FLOAT);
        sd.Usage = D3D11_USAGE_STAGING;
        sd.CPUAccessFlags = D3D11_CPU_ACCESS_READ.0 as u32;
        let staging = create_tex(gpu, &sd)?;
```

and change the three uses `&staging` (lines 381, 383, 409) to `&self.staging`.

- [ ] **Step 7: `lib.rs`** — exact replacements:

1. Imports: replace `use draw::{cbuffers, create_program, Program, ShapeDraw};` with `use draw::{cbuffers, create_program, FrameInputs, Program, ShapeDraw};` and `use scene::{Scene, View};` with `use scene::{LightValues, Scene, View, ViewValues};`.

2. In `pub struct Renderer`, replace

```rust
    encode_srgb: bool,
    timing: FrameTiming,
}
```

with

```rust
    encode_srgb: bool,
    timing: FrameTiming,
    /// The GPU-idle event query, created once (03 §7).
    query: ID3D11Query,
    /// Scene and view of the last rendered frame: what changed decides which fields are written.
    last_frame: Option<(Scene, View)>,
    /// Cbuffer uploads of the last frame.
    uploads: usize,
    /// Test hook: draw with P0's by-name writer (every field, every cbuffer, every frame).
    reference_writer: bool,
}
```

3. In `with_gpu`, replace

```rust
        Ok(Renderer {
            gpu,
```

with

```rust
        let mut q: Option<ID3D11Query> = None;
        let qd = D3D11_QUERY_DESC {
            Query: D3D11_QUERY_EVENT,
            MiscFlags: 0,
        };
        // SAFETY: descriptor and out-pointer are locals.
        unsafe { gpu.device.CreateQuery(&qd, Some(&mut q)) }
            .map_err(|e| format!("CreateQuery: {e}"))?;
        let query = q.ok_or("no event query")?;
        Ok(Renderer {
            query,
            last_frame: None,
            uploads: 0,
            reference_writer: false,
            gpu,
```

4. Insert after `pub fn last_timing(&self) -> FrameTiming { self.timing }`:

```rust

    /// Cbuffer uploads of the last [`Renderer::render`] (0 when neither the camera, the lights nor any material
    /// changed).
    pub fn last_uploads(&self) -> usize {
        self.uploads
    }

    /// Test hook: draw with P0's by-name writer (`Scene::write` into every cbuffer and every upload, every
    /// frame), the reference the resolved path must match pixel for pixel.
    #[doc(hidden)]
    pub fn set_reference_writer(&mut self, on: bool) {
        self.reference_writer = on;
    }
```

5. In `build_draw`, replace

```rust
        for cb in vs_cbs.iter_mut().chain(ps_cbs.iter_mut()) {
            write_material(&mut cb.writer, m, &bound, ll);
        }
```

with

```rust
        for cb in vs_cbs.iter_mut().chain(ps_cbs.iter_mut()) {
            write_material(&mut cb.writer, m, &bound, ll);
            scene::write_static(&mut cb.writer);
        }
```

and in the `ShapeDraw { … }` literal replace `            bound,\n        })` with

```rust
            bound,
            stale: true,
        })
```

6. In `render`, replace

```rust
        let view = View::new(&scene.camera, self.bounds, w, h);
        let gpu = &self.gpu;
```

with

```rust
        let view = View::new(&scene.camera, self.bounds, w, h);
        let (lights_changed, view_changed) = match &self.last_frame {
            Some((s, v)) => (s != scene, *v != view),
            None => (true, true),
        };
        // Once per frame, not per cbuffer (03 §7): the four inverses, the light rows and the SH.
        let view_values = ViewValues::new(&view);
        let light_values = LightValues::new(scene);
        let inputs = FrameInputs {
            view: &view,
            view_values: &view_values,
            lights: &light_values,
            lights_changed,
            view_changed,
        };
        let mut uploads = 0;
        let gpu = &self.gpu;
```

replace `                s.update(gpu, scene, &view);` with

```rust
                uploads += if self.reference_writer {
                    s.update_by_name(gpu, scene, &view)
                } else {
                    s.update(gpu, &inputs)
                };
```

replace

```rust
            ctx.OMSetBlendState(None, None, 0xffff_ffff);
        }
        let draw_ms = t0.elapsed().as_secs_f64() * 1000.0;
        let t1 = std::time::Instant::now();
        wait_idle(gpu)?;
```

with

```rust
            ctx.OMSetBlendState(None, None, 0xffff_ffff);
        }
        self.last_frame = Some((scene.clone(), view));
        self.uploads = uploads;
        let draw_ms = t0.elapsed().as_secs_f64() * 1000.0;
        let t1 = std::time::Instant::now();
        wait_idle(gpu, &self.query)?;
```

7. Replace the whole `fn wait_idle(gpu: &Gpu) -> Result<(), String> { … }` with:

```rust
/// Flushes and spins on the renderer's event query until the GPU has finished every submitted command, so the
/// readback that follows measures the copy alone. A failing query (removed device) is an error with the device
/// prefix.
fn wait_idle(gpu: &Gpu, q: &ID3D11Query) -> Result<(), String> {
    // SAFETY: `done` is a live BOOL-sized local, the size passed matches it; the query belongs to `gpu.device`.
    unsafe {
        gpu.ctx.End(q);
        gpu.ctx.Flush();
        let start = std::time::Instant::now();
        loop {
            let mut done = windows::core::BOOL(0);
            gpu.ctx
                .GetData(
                    q,
                    Some(&mut done as *mut _ as *mut core::ffi::c_void),
                    std::mem::size_of::<windows::core::BOOL>() as u32,
                    0,
                )
                .map_err(|e| gpu.fail("GetData", e))?;
            if done.as_bool() {
                return Ok(());
            }
            if start.elapsed().as_secs() > 10 {
                // The readback's Map waits anyway; the timing is then approximate.
                return Ok(());
            }
            std::thread::yield_now();
        }
    }
}
```

(`D3D11_QUERY_DESC` / `D3D11_QUERY_EVENT` stay imported: `with_gpu` uses them now. A device rebuild constructs a new `Renderer` (gui `viewport3d.rs` render loop), which creates a new query.)

- [ ] **Step 8: Run tests to verify they pass**

Run: `cargo test -p render3d --test frame -- --nocapture`
Expected: PASS, 4 tests; two `DeepGraph … draw_ms per frame` lines printed. Copy both lines into the phase report (acceptance: resolved ≤ ½ of by-name with a still camera is the 03 §7 target; recorded, not asserted).
Run: `cargo test -p render3d`
Expected: every render3d test passes (`edit`, `load`, `builtin`, `standin`, `device`, `compile`, `material`, `parse`; the by-name `Scene::write` unit test `world_rows_pack_row_major_camera_relative` is unchanged).

- [ ] **Step 9: Gates**

Run: `cargo fmt --all && cargo clippy --workspace --all-targets -- -D warnings && cargo test --workspace`
Expected: all green.

- [ ] **Step 10: Commit**

```bash
git add crates/render3d/src/cbuffer.rs crates/render3d/src/scene.rs crates/render3d/src/draw.rs crates/render3d/src/device.rs crates/render3d/src/lib.rs crates/render3d/tests/frame.rs
git commit -m "perf(render3d): resolved cbuffer fields, static/light/view split, dirty uploads, cached query and staging" -m "Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```
### Task 11: modscan `Store::nifs_using_texture` + `Store::open_read_only`

**Files:**
- Modify: `crates/modscan/src/lib.rs:11-25` (`ScanError`: one new variant)
- Modify: `crates/modscan/src/store.rs`: types after line 199 (end of `RawSlot`), helpers after line 212 (end of `keep_raw`), `open_read_only` after line 590 (end of `open`), `nifs_using_texture` after line 1103 (end of `users_of_texture`)
- Test: `crates/modscan/tests/store.rs` (append at the end; reuses its `store()`, `head()`, `lighting()`, `effect()`, `scan_of()` helpers)

**Interfaces:**
- Consumes: `nif::path::canon_texture` form of `path.p` (`crates/nif/src/path.rs:50`); `Store::{open, upsert_origin, upsert_copy, path_ids, set_winners, set_derived}` (`store.rs:563, 601, 718, 631, 759, 867`, all pub), used by tests only.
- Produces:
```rust
// crates/modscan/src/lib.rs
pub enum ScanError { /* … */ SchemaVersion { found: i64, want: i64 } }
// crates/modscan/src/store.rs
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum TexKey { Suffix { slot: u32, stem: String }, Contains { slot: u32, frag: String }, Exact { slot: u32, path: String } }
#[derive(Debug, Clone, PartialEq)]
pub struct MatchRow { pub copy_id: i64, pub nif_path: String, pub origin: String, pub shape_block: u32,
                      pub shape_name: Option<String>, pub le_mesh: bool, pub skinned: bool,
                      pub derived: bool, pub load_copy_id: i64 }
impl Store {
    pub fn open_read_only(path: &Path) -> Result<Store, ScanError>;
    pub fn nifs_using_texture(&self, key: &TexKey, limit: usize) -> Result<Vec<MatchRow>, ScanError>;
}
```

- [ ] **Step 1: Write the failing tests** (append to `crates/modscan/tests/store.rs`)

```rust
// ---- P1-A Task 11: the set preview's mesh query and the read-only open ----

use modscan::store::{DeriveRow, OriginSpec, TexKey, KIND_MOD};
use std::collections::HashMap;

fn origin(s: &mut Store, name: &str, priority: i32) -> i64 {
    s.upsert_origin(&OriginSpec {
        kind: KIND_MOD,
        name: name.into(),
        root: name.into(),
        parent: None,
        priority,
        derived_by: None,
        derived_state: None,
    })
    .unwrap()
}

fn lit(slots: &[&str]) -> Shader {
    Shader::Lighting(
        lighting(),
        slots.iter().map(|p| canon_texture(p.as_bytes())).collect(),
    )
}

fn put(s: &mut Store, path: &str, o: i64, block: u32, shader: Shader, skinned: bool) -> i64 {
    let scan = scan_of(vec![ShapeRow {
        block,
        head: head(skinned),
        shader,
        alpha: None,
    }]);
    s.upsert_copy(path, o, 1, 1, CopyResult::Scanned(scan), None)
        .unwrap()
}

/// a: vanilla diffuse + normal (wins). b: already-PBR paths, skinned (wins). c: `myreserve` (not on a component
/// boundary). d: a PGPatcher-derived winner over its source copy in A. e: A's copy loses to B's. f: the texture on
/// an effect shader. g: `axb`, a trap for an unescaped LIKE `_`.
fn query_store() -> (tempfile::TempDir, Store, HashMap<&'static str, i64>) {
    let (d, mut s, a) = store();
    let b = origin(&mut s, "B", 1);
    let pgp = origin(&mut s, "PGPatcher output", 2);
    let mut ids = HashMap::new();
    let vanilla = [r"textures\xmead\reserve.dds", r"textures\xmead\reserve_n.dds"];
    let pbr = [r"textures\pbr\xmead\reserve.dds", r"textures\pbr\xmead\reserve_msn.dds"];
    ids.insert("a", put(&mut s, "meshes/x/a.nif", a, 3, lit(&vanilla), false));
    ids.insert("b", put(&mut s, "meshes/x/b.nif", a, 1, lit(&pbr), true));
    ids.insert("c", put(&mut s, "meshes/x/c.nif", a, 0, lit(&[r"textures\other\myreserve.dds"]), false));
    ids.insert("d_src", put(&mut s, "meshes/x/d.nif", a, 5, lit(&[vanilla[0]]), false));
    ids.insert("d", put(&mut s, "meshes/x/d.nif", pgp, 7, lit(&[pbr[0]]), false));
    ids.insert("e_lose", put(&mut s, "meshes/x/e.nif", a, 0, lit(&[vanilla[0]]), false));
    ids.insert("e_win", put(&mut s, "meshes/x/e.nif", b, 0, lit(&[r"textures\other\rock.dds"]), false));
    ids.insert("f", put(&mut s, "meshes/x/f.nif", a, 0, Shader::Effect(effect(vanilla[0].as_bytes())), false));
    ids.insert("g", put(&mut s, "meshes/x/g.nif", a, 0, lit(&[r"textures\axb.dds"]), false));
    s.set_derived(
        &[DeriveRow {
            copy_id: ids["d"],
            is_derived: true,
            source_copy_id: Some(ids["d_src"]),
            derive_check: Some(0),
        }],
        &HashMap::new(),
    )
    .unwrap();
    let names = ["a", "b", "c", "d", "e", "f", "g"];
    let paths: Vec<String> = names.iter().map(|n| format!("meshes/x/{n}.nif")).collect();
    let pid = s
        .path_ids(&paths.iter().map(String::as_str).collect::<Vec<_>>())
        .unwrap();
    let winner = |n: &str| match n {
        "e" => ids["e_win"],
        other => ids[other],
    };
    let game: HashMap<i64, (i64, u32)> = names
        .iter()
        .zip(&pid)
        .map(|(n, p)| (*p, (winner(n), 1)))
        .collect();
    let source: HashMap<i64, i64> = [(pid[3], ids["d_src"])].into_iter().collect();
    s.set_winners(&game, &source).unwrap();
    (d, s, ids)
}

fn nifs(rows: &[modscan::store::MatchRow]) -> Vec<&str> {
    rows.iter().map(|r| r.nif_path.as_str()).collect()
}

#[test]
fn suffix_on_the_diffuse_matches_whole_components_of_winners_and_pbr_paths() {
    let (_d, s, ids) = query_store();
    let key = TexKey::Suffix { slot: 0, stem: r"XMead\Reserve".into() };
    let rows = s.nifs_using_texture(&key, 50).unwrap();
    // Non-skinned first, then fewest shapes, then path; b (skinned) last.
    assert_eq!(nifs(&rows), ["meshes/x/a.nif", "meshes/x/d.nif", "meshes/x/b.nif"]);
    assert_eq!((rows[0].copy_id, rows[0].load_copy_id, rows[0].shape_block), (ids["a"], ids["a"], 3));
    assert!(!rows[0].derived);
    // A derived winner is matched and loaded through its source copy.
    assert_eq!((rows[1].copy_id, rows[1].load_copy_id, rows[1].shape_block), (ids["d"], ids["d_src"], 5));
    assert!(rows[1].derived);
    assert!(rows[2].skinned);
    assert_eq!(rows[0].origin, "A");
    // `myreserve` is not on a component boundary, the loser and the effect shape are out, the limit holds.
    let all = s
        .nifs_using_texture(&TexKey::Suffix { slot: 0, stem: "reserve".into() }, 50)
        .unwrap();
    assert_eq!(nifs(&all), ["meshes/x/a.nif", "meshes/x/d.nif", "meshes/x/b.nif"]);
    assert_eq!(
        s.nifs_using_texture(&TexKey::Suffix { slot: 0, stem: "reserve".into() }, 2)
            .unwrap()
            .len(),
        2
    );
    // `_` is not a wildcard.
    assert!(s
        .nifs_using_texture(&TexKey::Suffix { slot: 0, stem: "a_b".into() }, 50)
        .unwrap()
        .is_empty());
}

#[test]
fn suffix_on_the_normal_strips_n_and_msn_on_the_shape_side_only() {
    let (_d, s, _) = query_store();
    let rows = s
        .nifs_using_texture(&TexKey::Suffix { slot: 1, stem: r"xmead\reserve".into() }, 50)
        .unwrap();
    assert_eq!(nifs(&rows), ["meshes/x/a.nif", "meshes/x/b.nif"]);
    // PGPatcher never strips `_n` from the JSON value (01 §1.3).
    assert!(s
        .nifs_using_texture(&TexKey::Suffix { slot: 1, stem: r"xmead\reserve_n".into() }, 50)
        .unwrap()
        .is_empty());
}

#[test]
fn contains_reads_the_extension_less_pbr_stripped_diffuse() {
    let (_d, s, _) = query_store();
    let rows = s
        .nifs_using_texture(&TexKey::Contains { slot: 0, frag: "XMEAD".into() }, 50)
        .unwrap();
    assert_eq!(nifs(&rows), ["meshes/x/a.nif", "meshes/x/d.nif", "meshes/x/b.nif"]);
    // `textures\pbr\` is read as `textures\`, and `.dds` is gone.
    for frag in ["pbr", ".dds"] {
        assert!(s
            .nifs_using_texture(&TexKey::Contains { slot: 0, frag: frag.into() }, 50)
            .unwrap()
            .is_empty(), "{frag}");
    }
}

#[test]
fn exact_compares_the_full_path_with_textures_added() {
    let (_d, s, _) = query_store();
    for path in [r"textures\xmead\reserve.dds", r"xmead\RESERVE.dds"] {
        let rows = s
            .nifs_using_texture(&TexKey::Exact { slot: 0, path: path.into() }, 50)
            .unwrap();
        assert_eq!(nifs(&rows), ["meshes/x/a.nif", "meshes/x/d.nif"], "{path}");
    }
    let rows = s
        .nifs_using_texture(&TexKey::Exact { slot: 0, path: r"textures\pbr\xmead\reserve.dds".into() }, 50)
        .unwrap();
    assert_eq!(nifs(&rows), ["meshes/x/b.nif"], "the derived copy's own rows are never used");
}

#[test]
fn open_read_only_never_creates_rebuilds_or_writes() {
    let d = tempfile::tempdir().unwrap();
    let missing = d.path().join("nope").join("scan.db");
    assert!(Store::open_read_only(&missing).is_err());
    assert!(!missing.parent().unwrap().exists(), "no folder is created");
    let old = d.path().join("old.db");
    {
        let c = rusqlite::Connection::open(&old).unwrap();
        c.execute_batch("CREATE TABLE shape (a INTEGER); INSERT INTO shape VALUES (1); PRAGMA user_version = 2;")
            .unwrap();
    }
    assert!(matches!(
        Store::open_read_only(&old),
        Err(modscan::ScanError::SchemaVersion { found: 2, want: 3 })
    ));
    let n: i64 = rusqlite::Connection::open(&old)
        .unwrap()
        .query_row("SELECT count(*) FROM shape", [], |r| r.get(0))
        .unwrap();
    assert_eq!(n, 1, "another version is left alone");
    let (q, _rw, _) = query_store();
    let ro = Store::open_read_only(&q.path().join("scan.db")).unwrap();
    let rows = ro
        .nifs_using_texture(&TexKey::Exact { slot: 0, path: r"textures\xmead\reserve.dds".into() }, 10)
        .unwrap();
    assert_eq!(rows.len(), 2);
    assert!(ro.conn().execute_batch("CREATE TABLE x (a INTEGER)").is_err());
}

#[test]
fn the_mesh_query_reports_database_errors() {
    let (_d, s, _) = query_store();
    s.conn().execute_batch("DROP TABLE nif_winner;").unwrap();
    assert!(s
        .nifs_using_texture(&TexKey::Suffix { slot: 0, stem: "reserve".into() }, 5)
        .is_err());
}
```

- [ ] **Step 2: Run the tests and confirm they fail**

Run: `cargo test -p modscan --test store`
Expected: compile errors: `unresolved import modscan::store::TexKey`, `no function or associated item named open_read_only`, `no variant SchemaVersion`.

- [ ] **Step 3: Implement**

`crates/modscan/src/lib.rs`: add this variant at the end of `ScanError` (after `Db`, line 24):
```rust
    /// `Store::open_read_only` found another schema version; only a scan (read-write open) rebuilds it.
    #[error("scan database version {found}, this build reads {want}: scan again in the Meshes view")]
    SchemaVersion { found: i64, want: i64 },
```

`crates/modscan/src/store.rs`, after `RawSlot` (line 199):
```rust
/// What [`Store::nifs_using_texture`] looks for in one texture-set slot (spec §2.4, 01 §1.3). `slot` is 0-based
/// (PGPatcher `slotN` − 1). Values come in PGPatcher form (backslashes, any case); the query lowercases them and turns
/// `\` into `/`, the form `path.p` stores (`canon_texture`: lower case, `/`, `textures/…`, extension kept).
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum TexKey {
    /// The slot path, without its extension, with a leading `textures/pbr/` read as `textures/` (and on slot 1 a
    /// trailing `_msn`/`_n` dropped), ends with `/` + `stem`, so on whole path components. `stem`: no extension, no
    /// leading `textures\`. `match_diffuse` (slot 0) and `match_normal` (slot 1).
    Suffix { slot: u32, stem: String },
    /// That same shape-side form contains `frag` (`path_contains`, slot 0).
    Contains { slot: u32, frag: String },
    /// The slot path equals `path` (with its extension; `textures\` is added when missing). `matchN`.
    Exact { slot: u32, path: String },
}

/// One Lighting shape of a game-winning NIF whose slot matches a [`TexKey`]. The SQL is a candidate filter: the
/// panel re-checks every shape with `pgpsim::shape_matches` on its raw slot strings.
#[derive(Debug, Clone, PartialEq)]
pub struct MatchRow {
    /// The copy the game loads.
    pub copy_id: i64,
    /// Canonical mesh path (`meshes/…`, lower case, `/`).
    pub nif_path: String,
    /// The mod of the copy to load.
    pub origin: String,
    /// Block, name, skinning and LE flag in the copy to load.
    pub shape_block: u32,
    pub shape_name: Option<String>,
    pub le_mesh: bool,
    pub skinned: bool,
    /// The game winner is a PGPatcher output copy.
    pub derived: bool,
    /// The copy to read: the winner, or for a derived winner its source (`nif_winner_src`, else `source_copy_id`).
    /// The simulation applies the patch itself, so a derived copy would be patched twice (02 §5.2).
    pub load_copy_id: i64,
}
```

After `keep_raw` (line 212):
```rust
/// A key value in the stored form: trimmed, `/` separators, lower case, no leading `/`.
fn key_form(s: &str) -> String {
    s.trim()
        .replace('\\', "/")
        .to_ascii_lowercase()
        .trim_start_matches('/')
        .to_string()
}

/// PGPatcher's shape-side form of a stored slot path (01 §1.3): extension stripped, a leading `textures/pbr/` read
/// as `textures/`, and on the normal slot a trailing `_msn`/`_n` stripped.
fn pgp_form(norm: &str, normal: bool) -> String {
    let base = match norm.rfind('.') {
        Some(i) if !norm[i..].contains('/') => &norm[..i],
        _ => norm,
    };
    let mut s = match base.strip_prefix("textures/pbr/") {
        Some(rest) => format!("textures/{rest}"),
        None => base.to_string(),
    };
    if normal {
        for suf in ["_msn", "_n"] {
            if let Some(x) = s.strip_suffix(suf) {
                s = x.to_string();
                break;
            }
        }
    }
    s
}

/// `s` for a `LIKE … ESCAPE '\'` pattern: `%` and `_` are literal (stems such as `rock01_d` contain `_`).
fn like_escape(s: &str) -> String {
    let mut o = String::with_capacity(s.len());
    for c in s.chars() {
        if matches!(c, '\\' | '%' | '_') {
            o.push('\\');
        }
        o.push(c);
    }
    o
}
```

After `open` (line 590), inside `impl Store`:
```rust
    /// A connection that never writes: no folder or file is created, no WAL switch, and another schema version is
    /// [`ScanError::SchemaVersion`] instead of a rebuild (a scan may be writing the database right now).
    pub fn open_read_only(path: &Path) -> Result<Store, ScanError> {
        let conn = Connection::open_with_flags(
            path,
            rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY | rusqlite::OpenFlags::SQLITE_OPEN_NO_MUTEX,
        )?;
        conn.busy_timeout(std::time::Duration::from_secs(5))?;
        let found: i64 = conn.query_row("PRAGMA user_version", [], |r| r.get(0))?;
        if found != SCHEMA_VERSION {
            return Err(ScanError::SchemaVersion {
                found,
                want: SCHEMA_VERSION,
            });
        }
        Ok(Store { conn })
    }
```

After `users_of_texture` (line 1103), inside `impl Store`:
```rust
    /// Lighting shapes of game-winning NIFs whose slot matches `key` (spec §2.4), best first: non-skinned, fewest
    /// shapes in the NIF, then path and block. At most `limit` rows. A derived winner is matched on its source
    /// copy's shapes. `Suffix`/`Contains` cannot use the path index (a scan of `path`); the caller runs this off the
    /// UI thread.
    pub fn nifs_using_texture(&self, key: &TexKey, limit: usize) -> Result<Vec<MatchRow>, ScanError> {
        let (slot, cond, arg, check): (u32, &str, String, Box<dyn Fn(&str) -> bool>) = match key {
            TexKey::Suffix { slot, stem } => {
                let stem = key_form(stem);
                if stem.is_empty() {
                    return Ok(vec![]);
                }
                let pattern = format!("%/{}%", like_escape(&stem));
                let want = format!("/{stem}");
                let normal = *slot == 1;
                (
                    *slot,
                    "tp.p LIKE ?2 ESCAPE '\\'",
                    pattern,
                    Box::new(move |p: &str| pgp_form(p, normal).ends_with(&want)),
                )
            }
            TexKey::Contains { slot, frag } => {
                let frag = key_form(frag);
                if frag.is_empty() {
                    return Ok(vec![]);
                }
                let pattern = format!("%{}%", like_escape(&frag));
                let normal = *slot == 1;
                (
                    *slot,
                    "tp.p LIKE ?2 ESCAPE '\\'",
                    pattern,
                    Box::new(move |p: &str| pgp_form(p, normal).contains(&frag)),
                )
            }
            TexKey::Exact { slot, path } => {
                let p = key_form(path);
                let p = if p.starts_with("textures/") {
                    p
                } else {
                    format!("textures/{p}")
                };
                (*slot, "tp.p = ?2", p, Box::new(|_: &str| true))
            }
        };
        let sql = format!(
            "SELECT wc.id, np.p, co.name, s.block, s.name, c.le_mesh, s.skinned, wc.is_derived, c.id, tp.p,
                    (SELECT count(*) FROM shape s2 WHERE s2.copy_id = c.id) AS n_shapes
             FROM path tp
             JOIN tex_use tu ON tu.tex_path_id = tp.id AND tu.slot = ?1
             JOIN shape s ON s.texset_id = tu.texset_id AND s.shader_kind = 1
             JOIN nif_copy c ON c.id = s.copy_id
             JOIN origin co ON co.id = c.origin_id AND co.enabled = 1
             JOIN nif_winner w ON w.path_id = c.path_id
             JOIN nif_copy wc ON wc.id = w.copy_id
             JOIN origin wo ON wo.id = wc.origin_id AND wo.enabled = 1
             LEFT JOIN nif_winner_src ws ON ws.path_id = c.path_id
             JOIN path np ON np.id = c.path_id
             WHERE {cond}
               AND c.id = CASE WHEN wc.is_derived = 1 THEN coalesce(ws.copy_id, wc.source_copy_id, wc.id)
                               ELSE wc.id END
             ORDER BY s.skinned, n_shapes, np.p, s.block"
        );
        let mut st = self.conn.prepare(&sql)?;
        let mut rows = st.query(params![slot as i64, arg])?;
        let mut out = vec![];
        while let Some(r) = rows.next()? {
            let tex: String = r.get(9)?;
            if !check(&tex) {
                continue;
            }
            out.push(MatchRow {
                copy_id: r.get(0)?,
                nif_path: r.get(1)?,
                origin: r.get(2)?,
                shape_block: r.get::<_, i64>(3)? as u32,
                shape_name: r.get(4)?,
                le_mesh: r.get(5)?,
                skinned: r.get(6)?,
                derived: r.get(7)?,
                load_copy_id: r.get(8)?,
            });
            if out.len() >= limit {
                break;
            }
        }
        Ok(out)
    }
```

- [ ] **Step 4: Run the tests and confirm they pass**

Run: `cargo test -p modscan --test store`
Expected: all pass, including the 6 new tests.

- [ ] **Step 5: Gates**

Run: `cargo fmt --all && cargo clippy --workspace --all-targets -- -D warnings && cargo test --workspace`
Expected: clean. `gui/src/model/meshes.rs:487-491` matches `ScanError` with a catch-all, so the new variant compiles there.

- [ ] **Step 6: Commit**

```bash
git add crates/modscan/src/lib.rs crates/modscan/src/store.rs crates/modscan/tests/store.rs
git commit -m "modscan: nifs_using_texture (PGPatcher match predicates) and open_read_only" -m "Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

---

### Task 12: gui `model/viewport3d.rs`: `FileSource::Memory`, parsed and built-in content, material edits

**Files:**
- Modify: `crates/gui/src/model/viewport3d.rs`: imports `8-17`; `FileSource` `54-73`; new `Content` after `MeshSpec` (line 84); `V3Cmd` `142-156`; `V3Reply` `158-190`; `Viewport3d` fields `197-235`; `new` `239-273`; `busy` `305-308`; `set_mesh` `451-472`; `request_frame` `490-537`; `handle` `568-621`; `read_source` `687-718`; `load` `740-765`; `render_loop` `767-884`
- Test: `crates/gui/tests/viewport3d.rs` (append at the end)

**Interfaces:**
- Consumes (Tasks 6, 8, 9): `render3d::{ParsedNif, ShapeId, ShapeInfo, EditKind}`, `render3d::builtin::Builtin`, `Renderer::{load_parsed, load_builtin, set_material, shapes}`, `nif::material::MaterialState`.
- Produces:
```rust
#[derive(Clone)] pub enum FileSource { Loose(PathBuf), Bsa { archive: PathBuf, norm: String },
                                       Memory { id: u64, bytes: Arc<Vec<u8>> } }   // PartialEq by id; Debug prints len
#[derive(Debug, Clone)] pub enum Content {
    Mesh(Arc<MeshSpec>),
    Parsed { key: u64, nif: Arc<ParsedNif>, textures: Arc<HashMap<String, FileSource>> },
    Builtin { key: u64, shape: Builtin, material: MaterialState, textures: Arc<HashMap<String, FileSource>> } }
impl Content { pub fn le_mesh(&self) -> bool }
pub enum V3Cmd { …, LoadParsed { id: u64, nif: Arc<ParsedNif>, textures: Arc<HashMap<String, FileSource>> },
                 LoadBuiltin { id: u64, shape: Builtin, material: MaterialState, textures: Arc<HashMap<String, FileSource>> },
                 SetMaterial { id: ShapeId, material: MaterialState }, … }
pub enum V3Reply { …, Shapes(Vec<ShapeInfo>), MaterialSet { id: ShapeId, result: Result<EditKind, String>, ms: f64 } }
impl Viewport3d {
    pub shapes: Vec<ShapeInfo>; pub last_edit: Option<(EditKind, f64)>;
    pub fn set_content(&mut self, c: Option<Content>, keep_view: bool);
    pub fn set_material(&mut self, id: ShapeId, material: MaterialState) -> bool;
    pub fn content(&self) -> Option<&Content>;
}
pub fn read_file(src: &FileSource) -> Result<Vec<u8>, String>;
```

- [ ] **Step 1: Write the failing tests** (append to `crates/gui/tests/viewport3d.rs`)

```rust
// ---- P1-A Task 12: Memory sources, parsed and built-in content, material edits ----

use nif::material::MaterialState;
use render3d::builtin::Builtin;
use render3d::{EditKind, ShapeId, ShapeInfo, ShapeStatus};
use std::collections::HashMap;
use std::sync::Arc;

fn mem(id: u64, b: &[u8]) -> v3::FileSource {
    v3::FileSource::Memory {
        id,
        bytes: Arc::new(b.to_vec()),
    }
}

fn confirmed() -> (
    tempfile::TempDir,
    Viewport3d,
    std::sync::mpsc::Receiver<v3::V3Cmd>,
) {
    let d = tempfile::tempdir().unwrap();
    let saved = fp(d.path(), "CS", "a");
    let mut v = Viewport3d::new(Some(saved.clone()));
    v.on_located(Ok(Some(saved)));
    let cmds = v.attach_test_thread();
    (d, v, cmds)
}

fn parsed(key: u64, nif: &Arc<render3d::ParsedNif>, tex: &[(&str, v3::FileSource)]) -> v3::Content {
    v3::Content::Parsed {
        key,
        nif: nif.clone(),
        textures: Arc::new(
            tex.iter()
                .map(|(k, s)| (k.to_string(), s.clone()))
                .collect(),
        ),
    }
}

#[test]
fn memory_sources_compare_by_id_print_their_length_and_serve_their_bytes() {
    assert_eq!(mem(1, b"DDS a"), mem(1, b"DDS b"));
    assert_ne!(mem(1, b"DDS a"), mem(2, b"DDS a"));
    assert_ne!(mem(1, b"x"), v3::FileSource::Loose(PathBuf::from("x")));
    let dbg = format!("{:?}", mem(7, &[0u8; 4096]));
    assert!(dbg.contains("4096") && dbg.len() < 64, "{dbg}");
    assert_eq!(v3::read_file(&mem(3, b"DDS bytes")).unwrap(), b"DDS bytes");
}

#[test]
fn parsed_content_loads_once_and_material_edits_follow_the_load() {
    let (_d, mut v, cmds) = confirmed();
    let nif = Arc::new(render3d::ParsedNif::default());
    v.set_content(Some(parsed(1, &nif, &[("textures/pbr/a.dds", mem(1, b"DDS "))])), false);
    assert!(
        !v.set_material(ShapeId(3), MaterialState::builtin_default()),
        "nothing sent yet: the caller puts the material into the content"
    );
    v.request_frame(64, 64);
    let Ok(v3::V3Cmd::LoadParsed { id, textures, .. }) = cmds.try_recv() else {
        panic!("LoadParsed first")
    };
    assert!(textures.contains_key("textures/pbr/a.dds"));
    // Queued behind the load: the thread applies it after the load.
    let mut m = MaterialState::builtin_default();
    m.params.glossiness = 12.0;
    assert!(v.set_material(ShapeId(3), m.clone()));
    assert!(matches!(
        cmds.try_recv(),
        Ok(v3::V3Cmd::SetMaterial { id: ShapeId(3), material }) if material == m
    ));
    assert!(v.busy());
    loaded(&mut v, id, render3d::LoadReport { shapes: 1, ..Default::default() });
    v.handle(v3::V3Reply::Shapes(vec![ShapeInfo {
        id: ShapeId(3),
        name: "Bottle".into(),
        material: m.clone(),
        status: ShapeStatus::Drawing,
    }]));
    assert_eq!(v.shapes.len(), 1);
    v.handle(v3::V3Reply::MaterialSet {
        id: ShapeId(3),
        result: Ok(EditKind::Cbuffer),
        ms: 0.2,
    });
    assert_eq!(v.last_edit, Some((EditKind::Cbuffer, 0.2)));
    assert!(!v.busy());
    v.request_frame(64, 64);
    assert!(matches!(cmds.try_recv(), Ok(v3::V3Cmd::Render { .. })));
    // The same content (same key, same Memory ids) is no reload.
    v.set_content(
        Some(parsed(1, &nif, &[("textures/pbr/a.dds", mem(1, b"other bytes, same id"))])),
        false,
    );
    v.request_frame(64, 64);
    assert!(cmds.try_recv().is_err());
    assert!(matches!(v.content(), Some(v3::Content::Parsed { key: 1, .. })));
}

#[test]
fn a_keep_view_reload_keeps_frame_and_camera_and_a_new_model_resets_them() {
    let (_d, mut v, cmds) = confirmed();
    let nif = Arc::new(render3d::ParsedNif::default());
    v.set_content(Some(parsed(1, &nif, &[])), false);
    v.request_frame(64, 64);
    let Ok(v3::V3Cmd::LoadParsed { id, .. }) = cmds.try_recv() else {
        panic!("load")
    };
    loaded(&mut v, id, render3d::LoadReport { shapes: 1, ..Default::default() });
    v.frame = Some(Arc::new(v3::Frame { width: 1, height: 1, rgba: vec![9; 4], id: 50 }));
    v.orbit.drag(40.0, 0.0);
    let yaw = v.orbit.yaw;
    // Re-encoded textures: the same model, a new key and new bytes.
    v.set_content(Some(parsed(2, &nif, &[("textures/a.dds", mem(9, b"DDS "))])), true);
    assert!(v.frame.is_some(), "the last frame stays until the reload lands");
    assert_eq!(v.orbit.yaw, yaw);
    v.request_frame(64, 64);
    assert!(matches!(cmds.try_recv(), Ok(v3::V3Cmd::LoadParsed { .. })));
    // A built-in shape is another model.
    v.set_content(
        Some(v3::Content::Builtin {
            key: 3,
            shape: Builtin::Sphere,
            material: MaterialState::builtin_default(),
            textures: Arc::new(HashMap::new()),
        }),
        false,
    );
    assert!(v.frame.is_none());
    assert_ne!(v.orbit.yaw, yaw);
    v.request_frame(64, 64);
    assert!(matches!(
        cmds.try_recv(),
        Ok(v3::V3Cmd::LoadBuiltin { shape: Builtin::Sphere, .. })
    ));
}

#[test]
fn an_le_parsed_nif_is_never_loaded() {
    let (_d, mut v, cmds) = confirmed();
    let nif = Arc::new(render3d::ParsedNif {
        le_mesh: true,
        ..Default::default()
    });
    v.set_content(Some(parsed(1, &nif, &[])), false);
    v.request_frame(64, 64);
    assert!(cmds.try_recv().is_err());
    assert!(v.report.as_ref().is_some_and(|r| r.le_mesh));
    assert!(!v.busy());
}
```

- [ ] **Step 2: Run the tests and confirm they fail**

Run: `cargo test -p sdc-gui --test viewport3d`
Expected: compile errors: `no variant named Memory`, `no function read_file`, `no method set_content`, `no variant LoadParsed`.

- [ ] **Step 3: Implement** (`crates/gui/src/model/viewport3d.rs`)

Imports (replace lines 8-17):
```rust
use crate::model::meshes::ScanTarget;
use crate::worker::{spawn_guarded, WorkerMsg};
use nif::material::MaterialState;
use render3d::builtin::Builtin;
use render3d::locate::{self, Change, Fingerprint};
use render3d::scene::{Camera, Scene, AUTO_DISTANCE, AUTO_PITCH, AUTO_YAW};
use render3d::{EditKind, FrameTiming, LoadReport, ParsedNif, Renderer, ShapeId, ShapeInfo};
use std::cell::RefCell;
use std::collections::{BTreeMap, HashMap};
use std::path::{Path, PathBuf};
use std::sync::mpsc::{channel, Receiver, Sender};
use std::sync::Arc;
```

`FileSource` (replace lines 54-73):
```rust
/// Where a file's bytes are: a loose file, an entry of a BSA, or bytes made in memory (a set-preview encode).
#[derive(Clone)]
pub enum FileSource {
    Loose(PathBuf),
    Bsa { archive: PathBuf, norm: String },
    /// Equal by `id` only: every new encode gets a new id, and comparing DDS blobs on each `set_content` would be
    /// costly (02 §2.2).
    Memory { id: u64, bytes: Arc<Vec<u8>> },
}

impl PartialEq for FileSource {
    fn eq(&self, o: &FileSource) -> bool {
        match (self, o) {
            (FileSource::Loose(a), FileSource::Loose(b)) => a == b,
            (
                FileSource::Bsa { archive: a, norm: n },
                FileSource::Bsa { archive: b, norm: m },
            ) => a == b && n == m,
            (FileSource::Memory { id: a, .. }, FileSource::Memory { id: b, .. }) => a == b,
            _ => false,
        }
    }
}

impl std::fmt::Debug for FileSource {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            FileSource::Loose(p) => f.debug_tuple("Loose").field(p).finish(),
            FileSource::Bsa { archive, norm } => f
                .debug_struct("Bsa")
                .field("archive", archive)
                .field("norm", norm)
                .finish(),
            FileSource::Memory { id, bytes } => f
                .debug_struct("Memory")
                .field("id", id)
                .field("len", &bytes.len())
                .finish(),
        }
    }
}

impl FileSource {
    /// From a store location: (origin kind, origin root) and the canonical path.
    pub fn from_store(kind: i32, root: &str, norm: &str) -> FileSource {
        if kind == modscan::store::KIND_BSA {
            FileSource::Bsa {
                archive: root.into(),
                norm: norm.into(),
            }
        } else {
            FileSource::Loose(Path::new(root).join(norm.replace('/', "\\")))
        }
    }
}
```

`Content` (insert after `MeshSpec`, after line 84):
```rust
/// What the panel draws: a NIF read through a [`MeshSpec`] (the Meshes view), a parsed NIF whose shape materials
/// the caller set (the set preview's simulated PGPatcher patch, P1-A), or a built-in shape.
#[derive(Debug, Clone)]
pub enum Content {
    Mesh(Arc<MeshSpec>),
    Parsed {
        /// Unique per (mesh, materials, textures): a new key is a new load.
        key: u64,
        nif: Arc<ParsedNif>,
        textures: Arc<HashMap<String, FileSource>>,
    },
    Builtin {
        key: u64,
        shape: Builtin,
        material: MaterialState,
        textures: Arc<HashMap<String, FileSource>>,
    },
}

impl PartialEq for Content {
    fn eq(&self, o: &Content) -> bool {
        match (self, o) {
            (Content::Mesh(a), Content::Mesh(b)) => a == b,
            (
                Content::Parsed { key: a, nif: n, textures: t },
                Content::Parsed { key: b, nif: m, textures: u },
            ) => a == b && Arc::ptr_eq(n, m) && t == u,
            (
                Content::Builtin { key: a, shape: s, material: x, textures: t },
                Content::Builtin { key: b, shape: r, material: y, textures: u },
            ) => a == b && s == r && x == y && t == u,
            _ => false,
        }
    }
}

impl Content {
    /// BSVER 83: the panel shows the LE text and nothing is loaded (00 §6.5).
    pub fn le_mesh(&self) -> bool {
        match self {
            Content::Mesh(m) => m.le_mesh,
            Content::Parsed { nif, .. } => nif.le_mesh,
            Content::Builtin { .. } => false,
        }
    }

    fn load_cmd(&self, id: u64) -> V3Cmd {
        match self.clone() {
            Content::Mesh(spec) => V3Cmd::Load { id, spec },
            Content::Parsed { nif, textures, .. } => V3Cmd::LoadParsed { id, nif, textures },
            Content::Builtin {
                shape,
                material,
                textures,
                ..
            } => V3Cmd::LoadBuiltin {
                id,
                shape,
                material,
                textures,
            },
        }
    }

    fn textures(&self) -> &HashMap<String, FileSource> {
        match self {
            Content::Mesh(s) => &s.textures,
            Content::Parsed { textures, .. } | Content::Builtin { textures, .. } => textures,
        }
    }
}
```

`V3Cmd` (replace lines 142-156):
```rust
/// Commands to the render thread.
pub enum V3Cmd {
    SetShaders(Fingerprint),
    SetFeatures(bool),
    Load {
        id: u64,
        spec: Arc<MeshSpec>,
    },
    /// A parsed NIF drawn with the materials in its shapes (the set preview).
    LoadParsed {
        id: u64,
        nif: Arc<ParsedNif>,
        textures: Arc<HashMap<String, FileSource>>,
    },
    LoadBuiltin {
        id: u64,
        shape: Builtin,
        material: MaterialState,
        textures: Arc<HashMap<String, FileSource>>,
    },
    /// One shape's new material on the loaded content (a JSON-only edit); applied after the load it follows and
    /// dropped by a later load in the same batch.
    SetMaterial {
        id: ShapeId,
        material: MaterialState,
    },
    Render {
        id: u64,
        camera: Camera,
        w: u32,
        h: u32,
    },
}
```

`V3Reply`: add these two variants after `RenderError { … }` (line 180):
```rust
    /// The shapes of the loaded `Parsed`/`Builtin` content: after each load and after each batch of edits.
    Shapes(Vec<ShapeInfo>),
    /// One `SetMaterial` applied: its kind (`Cbuffer`/`Textures` compile nothing) and render-thread time, or why
    /// it failed (the old draw stays).
    MaterialSet {
        id: ShapeId,
        result: Result<EditKind, String>,
        ms: f64,
    },
```

`Viewport3d` fields: replace line 217 `mesh: Option<Arc<MeshSpec>>,` with `content: Option<Content>,`. After `pub bounds: …` (line 216) add:
```rust
    /// Shapes of the loaded `Parsed`/`Builtin` content (empty for a `Mesh`).
    pub shapes: Vec<ShapeInfo>,
    /// The last material edit: its kind and render-thread time in ms.
    pub last_edit: Option<(EditKind, f64)>,
```
After `render_done: u64,` (line 223) add:
```rust
    edits_sent: u64,
    edits_done: u64,
```

`new` (lines 239-273): replace `mesh: None,` with `content: None,` and add `shapes: vec![], last_edit: None, edits_sent: 0, edits_done: 0,` beside the other initialisers.

`busy` (replace lines 305-308):
```rust
    /// A load, a render, a material edit or a locate is in flight.
    pub fn busy(&self) -> bool {
        self.locating
            || self.pending_load.is_some()
            || self.render_done < self.render_sent
            || self.edits_done < self.edits_sent
    }
```

`set_mesh` (replace lines 451-472 with these four methods):
```rust
    /// The mesh the Meshes view shows (`None`: nothing selected). Loaded on the next `request_frame`.
    pub fn set_mesh(&mut self, spec: Option<MeshSpec>) {
        let same = match (&self.content, &spec) {
            (Some(Content::Mesh(a)), Some(b)) => **a == *b,
            (None, None) => true,
            _ => false,
        };
        if !same {
            self.set_content(spec.map(|s| Content::Mesh(Arc::new(s))), false);
        }
    }

    /// What to draw (`None`: nothing), loaded on the next `request_frame`. `keep_view`: the same model with new
    /// data (re-encoded textures, P1-A): the camera, the bounds and the last frame stay until the reload lands.
    pub fn set_content(&mut self, c: Option<Content>, keep_view: bool) {
        if self.content == c {
            return;
        }
        self.content = c;
        let le = self.content.as_ref().is_some_and(Content::le_mesh);
        self.need_load = self.content.is_some() && !le;
        self.loaded = None;
        self.pending_load = None;
        // Renders and edits in flight are for the old content; the thread drops those queued behind a new load
        // without a reply.
        self.render_done = self.render_sent;
        self.edits_done = self.edits_sent;
        self.shapes.clear();
        self.render_error = None;
        self.last_sent = None;
        if keep_view && !le {
            return;
        }
        self.report = le.then(|| LoadReport {
            le_mesh: true,
            ..Default::default()
        });
        self.bounds = None;
        self.frame = None;
        self.orbit.reset();
    }

    /// A JSON-only edit (spec §2.3): one shape's material on the current `Parsed`/`Builtin` content, applied by the
    /// thread after the load it follows. False when no load of this content has been sent yet (the caller then puts
    /// the material into a new content instead).
    pub fn set_material(&mut self, id: ShapeId, material: MaterialState) -> bool {
        if !matches!(
            self.content,
            Some(Content::Parsed { .. } | Content::Builtin { .. })
        ) {
            return false;
        }
        if self.need_load || (self.loaded.is_none() && self.pending_load.is_none()) {
            return false;
        }
        let Some(t) = &self.thread else {
            return false;
        };
        if t.send(V3Cmd::SetMaterial { id, material }).is_err() {
            return false;
        }
        self.edits_sent += 1;
        // The next request_frame draws the edit.
        self.last_sent = None;
        true
    }

    pub fn content(&self) -> Option<&Content> {
        self.content.as_ref()
    }
```

`request_frame` (replace lines 490-515, i.e. from the doc comment through the `need_load` block's `return; }`; the rest of the function from `let (Some(loaded), None) = …` (line 516) stays):
```rust
    /// The panel wants a `w × h` frame of the current content and orbit: loads and renders only what changed.
    pub fn request_frame(&mut self, w: u32, h: u32) {
        if self.state == V3State::NotFound || matches!(self.state, V3State::Error(_)) {
            return;
        }
        if self.content.as_ref().is_some_and(Content::le_mesh) {
            return;
        }
        if self.content.is_none() || w == 0 || h == 0 || !self.confirmed || !self.ensure_thread() {
            return;
        }
        let Some(thread) = self.thread.clone() else {
            return;
        };
        if self.need_load {
            self.need_load = false;
            let id = self.id();
            let cmd = self
                .content
                .as_ref()
                .expect("content checked above")
                .load_cmd(id);
            if thread.send(cmd).is_ok() {
                // A queued Render is dropped by the thread when this load reaches it, with no reply.
                self.render_done = self.render_sent;
                self.pending_load = Some(id);
                self.state = V3State::Rendering;
            }
            return;
        }
```

`handle`: add these arms before `V3Reply::Located { generation, result } if …` (line 613):
```rust
            V3Reply::Shapes(s) => {
                // Only for the loaded content: a reply for content replaced since is dropped.
                if self.pending_load.is_none() && self.loaded.is_some() {
                    self.shapes = s;
                }
            }
            V3Reply::MaterialSet { result, ms, .. } => {
                self.edits_done = (self.edits_done + 1).min(self.edits_sent);
                match result {
                    Ok(kind) => self.last_edit = Some((kind, ms)),
                    Err(e) => self.render_error = Some(e),
                }
            }
```

`read_source`: add the arm `FileSource::Memory { bytes, .. } => Ok(bytes.to_vec()),` after the `Loose` arm (line 692), and after the function (line 718) add:
```rust
/// The bytes of one file (a fresh BSA index per call): for workers that read a single NIF.
pub fn read_file(src: &FileSource) -> Result<Vec<u8>, String> {
    read_source(src, &RefCell::new(HashMap::new()))
}
```

`load` (replace lines 740-765):
```rust
/// The texture resolver over `map` (canonical path → bytes); read errors are collected in `errors`.
fn resolver<'a>(
    map: &'a HashMap<String, FileSource>,
    bsas: &'a RefCell<HashMap<PathBuf, Option<BsaIndex>>>,
    errors: &'a RefCell<Vec<String>>,
) -> impl Fn(&str) -> Option<Vec<u8>> + 'a {
    move |norm: &str| {
        let src = map.get(norm)?;
        match read_source(src, bsas) {
            Ok(b) => Some(b),
            Err(e) => {
                errors.borrow_mut().push(e);
                None
            }
        }
    }
}

/// Loads `c`, then applies the material edits made since its load again (a reload after a shaders, features or
/// device change keeps them).
fn load(r: &mut Renderer, c: &Content, edits: &BTreeMap<ShapeId, MaterialState>) -> LoadReport {
    let bsas = RefCell::new(HashMap::new());
    let errors = RefCell::new(vec![]);
    let textures = resolver(c.textures(), &bsas, &errors);
    let mut rep = match c {
        Content::Mesh(spec) => match read_source(&spec.nif, &bsas) {
            Ok(nif) => r.load(&nif, &textures),
            Err(e) => {
                return LoadReport {
                    errors: vec![format!("NIF: {e}")],
                    ..Default::default()
                }
            }
        },
        Content::Parsed { nif, .. } => r.load_parsed(nif, &textures),
        Content::Builtin { shape, material, .. } => {
            r.load_builtin(*shape, material.clone(), &textures)
        }
    };
    for (id, m) in edits {
        if let Err(e) = r.set_material(*id, m.clone(), &textures) {
            rep.errors.push(e);
        }
    }
    rep.errors.extend(errors.into_inner());
    rep
}

/// One material edit on the loaded content.
fn edit(r: &mut Renderer, c: &Content, id: ShapeId, m: MaterialState) -> Result<EditKind, String> {
    let bsas = RefCell::new(HashMap::new());
    let errors = RefCell::new(vec![]);
    let textures = resolver(c.textures(), &bsas, &errors);
    r.set_material(id, m, &textures)
}
```

`render_loop` (replace lines 767-884):
```rust
/// The render thread: owns the `Renderer`; batches queued commands, applies material edits after the load they
/// follow, and draws only the latest camera.
fn render_loop(
    rx: Receiver<V3Cmd>,
    tx: &Sender<WorkerMsg>,
    shaders: Fingerprint,
    cache: PathBuf,
    features: bool,
) {
    let send = |r: V3Reply| {
        let _ = tx.send(WorkerMsg::Viewport3d(r));
    };
    let make = |shaders: Fingerprint, features: bool| -> Result<Renderer, String> {
        let mut r = Renderer::new(shaders, cache.clone())?;
        apply_features(&mut r, features);
        Ok(r)
    };
    let loaded = |r: &Renderer, id: u64, c: &Content, report: LoadReport| {
        send(V3Reply::Loaded {
            id,
            report,
            bounds: r.bounds(),
        });
        if !matches!(c, Content::Mesh(_)) {
            send(V3Reply::Shapes(r.shapes()));
        }
    };
    let mut features = features;
    let mut r = match make(shaders, features) {
        Ok(r) => r,
        Err(e) => return send(V3Reply::Failed(e)),
    };
    send(V3Reply::Ready {
        adapter: r.gpu().adapter.clone(),
        warp: r.gpu().warp,
    });
    // After a device loss: a new renderer (maybe WARP, same shaders and features), `Ready`, the last content
    // loaded again with its edits, and its `Loaded`. Err when no device can be made at all.
    let rebuild = |r: &mut Renderer,
                   last: &Option<(u64, Content)>,
                   edits: &BTreeMap<ShapeId, MaterialState>,
                   features: bool|
     -> Result<(), String> {
        *r = make(r.shaders().clone(), features)?;
        send(V3Reply::Ready {
            adapter: r.gpu().adapter.clone(),
            warp: r.gpu().warp,
        });
        if let Some((id, c)) = last {
            let report = load(r, c, edits);
            loaded(r, *id, c, report);
        }
        Ok(())
    };
    let mut last: Option<(u64, Content)> = None;
    // Material edits applied to `last` since its load.
    let mut edits: BTreeMap<ShapeId, MaterialState> = BTreeMap::new();
    while let Ok(first) = rx.recv() {
        let mut reload = false;
        let mut render = None;
        let mut pending: Vec<(ShapeId, MaterialState)> = vec![];
        for c in std::iter::once(first).chain(rx.try_iter()) {
            let content = match c {
                V3Cmd::SetShaders(f) => {
                    r.set_shaders(f);
                    reload = true;
                    None
                }
                V3Cmd::SetFeatures(on) => {
                    features = on;
                    apply_features(&mut r, on);
                    reload = true;
                    None
                }
                V3Cmd::Load { id, spec } => Some((id, Content::Mesh(spec))),
                V3Cmd::LoadParsed { id, nif, textures } => Some((
                    id,
                    Content::Parsed {
                        key: id,
                        nif,
                        textures,
                    },
                )),
                V3Cmd::LoadBuiltin {
                    id,
                    shape,
                    material,
                    textures,
                } => Some((
                    id,
                    Content::Builtin {
                        key: id,
                        shape,
                        material,
                        textures,
                    },
                )),
                V3Cmd::SetMaterial { id, material } => {
                    pending.push((id, material));
                    None
                }
                V3Cmd::Render { id, camera, w, h } => {
                    render = Some((id, camera, w, h));
                    None
                }
            };
            if let Some(c) = content {
                // A new load drops the queued render and the edits made for the old content (the model reset its
                // counters when it replaced the content).
                last = Some(c);
                edits.clear();
                pending.clear();
                render = None;
                reload = true;
            }
        }
        if reload {
            if let Some((id, c)) = &last {
                let report = load(&mut r, c, &edits);
                if r.gpu().is_removed() {
                    // Lost during the load: rebuild and load again now; with no shapes no Render would follow to
                    // trigger the rebuild.
                    if let Err(e) = rebuild(&mut r, &last, &edits, features) {
                        return send(V3Reply::Failed(e));
                    }
                } else {
                    loaded(&r, *id, c, report);
                }
            }
        }
        if !pending.is_empty() {
            if let Some((_, c)) = &last {
                for (id, m) in pending {
                    let t0 = std::time::Instant::now();
                    let result = edit(&mut r, c, id, m.clone());
                    if result.is_ok() {
                        edits.insert(id, m);
                    }
                    send(V3Reply::MaterialSet {
                        id,
                        result,
                        ms: t0.elapsed().as_secs_f64() * 1000.0,
                    });
                }
                send(V3Reply::Shapes(r.shapes()));
            }
        }
        let Some((id, camera, w, h)) = render else {
            continue;
        };
        let scene = Scene {
            camera,
            ..Scene::neutral()
        };
        let mut result = r.render(&scene, w, h);
        if let Err(e) = &result {
            if e.starts_with(render3d::device::DEVICE_REMOVED) {
                // Device lost (TDR, driver update): a new device (maybe WARP), the content again, one retry.
                if let Err(e) = rebuild(&mut r, &last, &edits, features) {
                    return send(V3Reply::Failed(e));
                }
                result = r.render(&scene, w, h);
            }
        }
        match result {
            Ok(rgba) => send(V3Reply::Frame {
                frame: Arc::new(Frame {
                    width: w,
                    height: h,
                    rgba,
                    id,
                }),
                timing: r.last_timing(),
            }),
            Err(message) => send(V3Reply::RenderError { id, message }),
        }
    }
}
```

- [ ] **Step 4: Run the tests and confirm they pass**

Run: `cargo test -p sdc-gui --test viewport3d && cargo test -p sdc-gui --test meshes_view`
Expected: all pass (the Meshes view still goes through `set_mesh` → `Content::Mesh`, and every existing `V3Cmd::Load { .. }` match still holds).

- [ ] **Step 5: Gates**

Run: `cargo fmt --all && cargo clippy --workspace --all-targets -- -D warnings && cargo test --workspace`
Expected: clean.

- [ ] **Step 6: Commit**

```bash
git add crates/gui/src/model/viewport3d.rs crates/gui/tests/viewport3d.rs
git commit -m "gui: viewport3d Memory sources, parsed/built-in content and SetMaterial edits" -m "Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

---

### Task 13: gui `model/set_preview.rs` part 1: per-member encodes → DDS bytes, fresh/stale

**Files:**
- Create: `crates/gui/src/model/set_preview.rs`
- Modify (shared hook, own commit): `crates/gui/src/model/mod.rs`: insert `pub mod set_preview;` before line 12 (`pub mod sets;`)
- Test: `crates/gui/tests/set_preview_model.rs` (new)

**Interfaces:**
- Consumes (all pub, read-only): `model::sets::{SetView (sets.rs:99), SlotKind (sets.rs:18) with slot_number/suffix/label (sets.rs:45,57,70), SlotFill (sets.rs:86)}`; `model::items::{TextureItem, ItemKind, ItemStatus}` (`items.rs:17,24,45`); `AppModel::item` (`model/mod.rs:953`); `model::preview::DEBOUNCE` (`preview.rs:18`); `pgp::writer::EntryInput` (`writer.rs:29`); `pgp::prefix::output_rel` (`prefix.rs:217`); `engine::session::{Session::preview (session.rs:264), pack_key (session.rs:114)}`; `engine::dds::{dds_bytes, resolve_header, DdsDesc}` (`dds.rs:152, 51, 77`); `engine::manifest::sha256_json` (`manifest.rs:338`); `engine::api::CancelToken` (`api.rs:225`, `Default`); `nif::path::canon_texture` (`path.rs:50`); `FileSource::Memory` (Task 12).
- Produces:
```rust
pub const DEFAULT_CAP: u32 = 2048;
pub const WAITS_FOR_EXPORT: &str;
pub fn cap_text(cap: u32) -> String;
pub fn canon(p: &str) -> String;
pub fn plain_path(kind: SlotKind) -> String;
pub enum Source { Job(Box<ResolvedJob>), Dds(PathBuf), Blocked(&'static str) }
pub fn source_of(item: &TextureItem) -> Source;
pub fn signature(item: &TextureItem) -> String;
#[derive(Debug, Clone, PartialEq)] pub struct MemberSlot { pub kind: SlotKind, pub item: String, pub job_key: String,
                                                          pub status: String, pub sig: String, pub paths: Vec<String> }
pub fn member_slots(m: &AppModel, v: &SetView, entry: Option<&EntryInput>) -> Vec<MemberSlot>;
#[derive(Debug)] pub struct Encoded { pub id: u64, pub dds: Arc<Vec<u8>>, pub ms: f64 }
#[derive(Debug, Clone)] pub enum EncState { NotEncoded, Stale(Option<Arc<Encoded>>), Encoding(Option<Arc<Encoded>>),
                                            Fresh(Arc<Encoded>), Failed(String), Blocked(&'static str) }
pub struct Member { pub slot: MemberSlot, pub state: EncState, /* private: source, due, token, cancel */ }
pub struct Encoder { pub members: BTreeMap<SlotKind, Member>, pub cap: u32, pub paused: bool, pub error: Option<String>, … }
impl Encoder {
    pub fn new(cap: u32) -> Encoder;
    pub fn reset(&mut self);
    pub fn sync(&mut self, m: &AppModel, slots: Vec<MemberSlot>, now: Instant, immediate: bool);
    pub fn pump(&mut self, session: Option<&Arc<Session>>, now: Instant, export_running: bool);
    pub fn poll(&mut self) -> bool;
    pub fn settled(&self) -> bool;
    pub fn busy(&self) -> bool;
    pub fn produces(&self, norm: &str) -> bool;
    pub fn memory_sources(&self) -> HashMap<String, FileSource>;
}
pub fn encode(session: &Session, source: &Source, cap: u32, cancel: &CancelToken) -> Result<Vec<u8>, String>;
```

- [ ] **Step 1: Write the failing tests** (`crates/gui/tests/set_preview_model.rs`)

```rust
//! The set preview's model (P1-A Tasks 13–14): encodes, fingerprint and change classification, query keys,
//! PGPatcher simulation per shape, and the orchestration without a GPU.
mod support;
use sdc_gui::model::items::{ItemKind, ItemStatus};
use sdc_gui::model::pbr_export::json_entries;
use sdc_gui::model::preview::DEBOUNCE;
use sdc_gui::model::set_preview::*;
use sdc_gui::model::sets::{SetView, SlotKind};
use sdc_gui::model::viewport3d::FileSource;
use sdc_gui::model::AppModel;
use sdc_gui::pgp::writer::EntryInput;
use std::time::{Duration, Instant};
use support::*;

fn rock(m: &AppModel) -> (SetView, EntryInput) {
    json_entries(m)
        .into_iter()
        .find(|(v, _)| v.id == "landscape/rock01")
        .expect("the fixture's PBR set")
}

fn settle(e: &mut Encoder, m: &AppModel) {
    let t = Instant::now();
    while !e.settled() && t.elapsed() < Duration::from_secs(60) {
        e.pump(m.session.as_ref(), Instant::now(), false);
        e.poll();
        std::thread::sleep(Duration::from_millis(5));
    }
    assert!(e.settled(), "the encodes did not finish");
}

#[test]
fn member_slots_serve_the_set_outputs_under_their_canonical_paths() {
    let d = tempfile::tempdir().unwrap();
    let m = pbr_opened(d.path());
    let (v, e) = rock(&m);
    let slots = member_slots(&m, &v, Some(&e));
    let path = |k: SlotKind| slots.iter().find(|s| s.kind == k).map(|s| s.paths.clone());
    assert_eq!(path(SlotKind::Diffuse), Some(vec!["textures/pbr/landscape/rock01.dds".to_string()]));
    assert_eq!(path(SlotKind::Normal), Some(vec!["textures/pbr/landscape/rock01_n.dds".to_string()]));
    // Without the PGPatcher entry (D3): the plain paths.
    let plain = member_slots(&m, &v, None);
    assert!(plain
        .iter()
        .any(|s| s.kind == SlotKind::Diffuse && s.paths == vec![plain_path(SlotKind::Diffuse)]));
}

#[test]
fn encodes_become_dds_bytes_served_as_memory_sources() {
    let d = tempfile::tempdir().unwrap();
    let m = pbr_opened(d.path());
    let (v, e) = rock(&m);
    let mut enc = Encoder::new(DEFAULT_CAP);
    enc.sync(&m, member_slots(&m, &v, Some(&e)), Instant::now(), true);
    assert!(!enc.settled());
    settle(&mut enc, &m);
    let st = &enc.members[&SlotKind::Diffuse].state;
    let EncState::Fresh(out) = st else {
        panic!("{st:?}")
    };
    assert_eq!(&out.dds[..4], b"DDS ");
    let f = engine::dds::read_dds(&out.dds).unwrap();
    assert_eq!((f.width, f.height), (64, 64));
    match &enc.memory_sources()["textures/pbr/landscape/rock01.dds"] {
        FileSource::Memory { id, .. } => assert_eq!(*id, out.id),
        s => panic!("{s:?}"),
    }
    assert!(enc.produces("textures/pbr/landscape/rock01_n.dds"));
}

#[test]
fn a_changed_member_goes_stale_keeps_its_bytes_and_re_encodes_after_the_debounce() {
    let d = tempfile::tempdir().unwrap();
    let m = pbr_opened(d.path());
    let (v, e) = rock(&m);
    let mut enc = Encoder::new(DEFAULT_CAP);
    let mut slots = member_slots(&m, &v, Some(&e));
    enc.sync(&m, slots.clone(), Instant::now(), true);
    settle(&mut enc, &m);
    let first = enc.members[&SlotKind::Diffuse].state.shown().unwrap().id;
    for s in &mut slots {
        if s.kind == SlotKind::Diffuse {
            s.sig.push_str(":edited");
        }
    }
    let now = Instant::now();
    enc.sync(&m, slots.clone(), now, false);
    let st = &enc.members[&SlotKind::Diffuse].state;
    assert!(matches!(st, EncState::Stale(Some(e)) if e.id == first), "{st:?}");
    assert!(
        matches!(enc.members[&SlotKind::Normal].state, EncState::Fresh(_)),
        "an unchanged member keeps its bytes"
    );
    enc.pump(m.session.as_ref(), now, false);
    assert!(matches!(enc.members[&SlotKind::Diffuse].state, EncState::Stale(_)), "waits for the debounce");
    enc.pump(m.session.as_ref(), now + DEBOUNCE, false);
    assert!(matches!(enc.members[&SlotKind::Diffuse].state, EncState::Encoding(Some(_))));
    settle(&mut enc, &m);
    let second = enc.members[&SlotKind::Diffuse].state.shown().unwrap().id;
    assert_ne!(second, first);
    // A path-only change keeps the bytes and encodes nothing.
    for s in &mut slots {
        s.paths.push(format!("textures/extra/{:?}.dds", s.kind).to_lowercase());
    }
    enc.sync(&m, slots, Instant::now(), false);
    assert!(enc.settled());
    assert!(enc.memory_sources().contains_key("textures/extra/diffuse.dds"));
    assert_eq!(enc.members[&SlotKind::Diffuse].state.shown().unwrap().id, second);
}

#[test]
fn nothing_encodes_while_an_export_runs() {
    let d = tempfile::tempdir().unwrap();
    let m = pbr_opened(d.path());
    let (v, e) = rock(&m);
    let mut enc = Encoder::new(DEFAULT_CAP);
    enc.sync(&m, member_slots(&m, &v, Some(&e)), Instant::now(), true);
    enc.pump(m.session.as_ref(), Instant::now(), true);
    assert!(enc.paused);
    assert!(enc
        .members
        .values()
        .all(|x| matches!(x.state, EncState::NotEncoded)));
    assert!(!enc.busy(), "no repaint loop while paused");
    settle(&mut enc, &m);
    assert!(!enc.paused);
}

#[test]
fn blocked_items_never_encode_and_dds_inputs_use_their_own_bytes() {
    let d = tempfile::tempdir().unwrap();
    let m = pbr_opened(d.path());
    let mut item = m
        .items
        .iter()
        .find(|i| i.job.is_some() && i.status == ItemStatus::Ready)
        .unwrap()
        .clone();
    assert!(matches!(source_of(&item), Source::Job(_)));
    item.status = ItemStatus::Error(vec![]);
    assert!(matches!(source_of(&item), Source::Blocked(_)));
    item.status = ItemStatus::PackedInto("x".into());
    assert!(matches!(source_of(&item), Source::Blocked(_)));
    item.status = ItemStatus::Ready;
    item.kind = ItemKind::Dds;
    let input = item.input.clone();
    assert!(matches!(source_of(&item), Source::Dds(p) if p == input));
}
```

- [ ] **Step 2: Run the tests and confirm they fail**

Run: `cargo test -p sdc-gui --test set_preview_model`
Expected: compile error: `unresolved import sdc_gui::model::set_preview`.

- [ ] **Step 3: Implement** `crates/gui/src/model/set_preview.rs`

```rust
//! The live set preview (P1-A, spec §2.3, 02): the Textures view's own 3D panel, showing the selected set on a game
//! mesh patched in memory as PGPatcher 1.2.0 would patch it (`pgpsim`), with this session's encodes of the set's
//! textures. No egui types; `ui::set_preview` draws it.
//!
//! Part 1: per-member preview encodes → DDS bytes served as `FileSource::Memory`, fresh/stale per member, paused
//! during an export. Results come back on this module's own channel, so `worker.rs` needs no new message.
use crate::model::items::{ItemKind, ItemStatus, TextureItem};
use crate::model::preview::DEBOUNCE;
use crate::model::sets::{SetView, SlotFill, SlotKind};
use crate::model::viewport3d::FileSource;
use crate::model::AppModel;
use crate::pgp::prefix::output_rel;
use crate::pgp::writer::EntryInput;
use engine::api::CancelToken;
use engine::dds::{dds_bytes, resolve_header, DdsDesc};
use engine::manifest::sha256_json;
use engine::policy::ResolvedJob;
use engine::session::{pack_key, Session};
use std::collections::{BTreeMap, HashMap};
use std::path::PathBuf;
use std::sync::mpsc::{channel, Receiver, Sender};
use std::sync::Arc;
use std::time::Instant;

/// D5: the preview's encode size cap (`[render3d] set_preview_max` in `gui.toml`, 0 = this).
pub const DEFAULT_CAP: u32 = 2048;
pub const WAITS_FOR_EXPORT: &str = "Preview encoding waits for the export to finish";
const BLOCKED_ERROR: &str = "the texture has an error";
const BLOCKED_NOTHING: &str = "nothing to encode";

pub fn cap_text(cap: u32) -> String {
    format!("Preview at {cap} px, export writes full size")
}

/// The canonical render path (`textures/…`, lower case, `/`) of a texture path in any form.
pub fn canon(p: &str) -> String {
    nif::path::canon_texture(p.as_bytes()).norm
}

/// Where a set the simulation cannot preview (D3) puts its textures: `textures/sdcpreview/<suffix>.dds`.
pub fn plain_path(kind: SlotKind) -> String {
    let s = kind.suffix();
    format!("textures/sdcpreview/{}.dds", if s.is_empty() { "d" } else { s })
}

/// What a slot member can give the preview.
#[derive(Debug, Clone)]
pub enum Source {
    Job(Box<ResolvedJob>),
    /// A DDS input: its own bytes (no encode, no cap).
    Dds(PathBuf),
    Blocked(&'static str),
}

/// 02 §2.3, mirroring `preview::classify`: a job to encode (`NeedsDecision` too: the suggested settings), a DDS
/// input's bytes, or nothing (an error, skipped, failed, or a raw map packed into another output).
pub fn source_of(item: &TextureItem) -> Source {
    if item.kind == ItemKind::Dds {
        return Source::Dds(item.input.clone());
    }
    if matches!(item.status, ItemStatus::Error(_)) {
        return Source::Blocked(BLOCKED_ERROR);
    }
    match &item.job {
        Some(job)
            if !matches!(
                item.status,
                ItemStatus::Skipped(_) | ItemStatus::Failed(_) | ItemStatus::PackedInto(_)
            ) =>
        {
            Source::Job(Box::new(job.clone()))
        }
        _ => Source::Blocked(BLOCKED_NOTHING),
    }
}

/// What makes the member's bytes: the profile plus the pack key or the source stamp (`preview.rs`'s private
/// `signature`, re-implemented from pub engine items, 02 §2.4); for a DDS input its path and stamp.
pub fn signature(item: &TextureItem) -> String {
    match &item.job {
        Some(job) if item.kind != ItemKind::Dds => {
            let p = sha256_json(&job.profile).unwrap_or_default();
            match &job.pack {
                Some(pk) => format!("{p}:{}", pack_key(pk).unwrap_or_default()),
                None => format!("{p}:{:?}", job.source),
            }
        }
        _ => format!("{:?}:{:?}:{:?}", item.kind, item.input, item.stamp),
    }
}

/// One filled slot of the selected set: its kind, the member item, what its bytes depend on, and every canonical
/// path the preview serves those bytes under.
#[derive(Debug, Clone, PartialEq)]
pub struct MemberSlot {
    pub kind: SlotKind,
    pub item: String,
    pub job_key: String,
    /// `ItemStatus` as text: a decision changes what is encoded.
    pub status: String,
    pub sig: String,
    pub paths: Vec<String>,
}

/// The set's filled slots (02 §1.1). With the PGPatcher entry (CS and a PBR set): the slot's path in
/// `entry.slots[slotN]` when sdc names it there (a borrowed slot, or a file another set owns, `pbr_export::slots_of`),
/// and for the set's own texture `<prefix>_<suffix>.dds`. A user's own `slotN` setting points at some other file
/// and is not served. Without the entry (D3): [`plain_path`]. Coat and fuzz share slot 7: both claim an explicit
/// slot-7 path, and the later one wins in [`Encoder::memory_sources`].
pub fn member_slots(m: &AppModel, v: &SetView, entry: Option<&EntryInput>) -> Vec<MemberSlot> {
    let mut out = vec![];
    for (kind, fill) in &v.slots {
        let (key, own) = match fill {
            SlotFill::Own { item } => (item, true),
            SlotFill::Borrowed { item, .. } => (item, false),
            SlotFill::Empty => continue,
        };
        let Some(item) = m.item(key) else {
            continue;
        };
        let mut paths = vec![];
        match entry {
            None => paths.push(plain_path(*kind)),
            Some(e) => {
                let n = kind.slot_number();
                let user = v
                    .settings
                    .get(&format!("slot{n}"))
                    .is_some_and(|x| x.is_string());
                if let Some(p) = e.slots.get(&n).filter(|p| !user && !p.is_empty()) {
                    paths.push(canon(p));
                }
                if let (true, Some(pre)) = (own, &v.prefix) {
                    let p = canon(&output_rel(pre, kind.suffix()).to_string_lossy());
                    if !paths.contains(&p) {
                        paths.push(p);
                    }
                }
            }
        }
        out.push(MemberSlot {
            kind: *kind,
            item: key.clone(),
            job_key: item.job_key.clone(),
            status: format!("{:?}", item.status),
            sig: signature(item),
            paths,
        });
    }
    out
}

/// A finished preview encode: a whole DDS (legacy or DX10 header, as export writes it) and its memory id.
#[derive(Debug)]
pub struct Encoded {
    pub id: u64,
    pub dds: Arc<Vec<u8>>,
    pub ms: f64,
}

#[derive(Debug, Clone)]
pub enum EncState {
    NotEncoded,
    /// Changed: waits for the debounce (or the export); the previous bytes stay on show.
    Stale(Option<Arc<Encoded>>),
    Encoding(Option<Arc<Encoded>>),
    Fresh(Arc<Encoded>),
    Failed(String),
    Blocked(&'static str),
}

impl EncState {
    /// The bytes on show: fresh ones, or the previous ones while stale or encoding.
    pub fn shown(&self) -> Option<&Arc<Encoded>> {
        match self {
            EncState::Stale(e) | EncState::Encoding(e) => e.as_ref(),
            EncState::Fresh(e) => Some(e),
            _ => None,
        }
    }
    /// Nothing more happens until the member changes.
    pub fn settled(&self) -> bool {
        matches!(
            self,
            EncState::Fresh(_) | EncState::Failed(_) | EncState::Blocked(_)
        )
    }
    pub fn label(&self) -> String {
        match self {
            EncState::NotEncoded => "not encoded yet".into(),
            EncState::Stale(_) => "stale".into(),
            EncState::Encoding(_) => "encoding…".into(),
            EncState::Fresh(e) => format!("fresh ({:.0} ms)", e.ms),
            EncState::Failed(m) => format!("failed: {m}"),
            EncState::Blocked(w) => format!("not previewed: {w}"),
        }
    }
}

#[derive(Debug)]
pub struct Member {
    pub slot: MemberSlot,
    pub state: EncState,
    source: Source,
    due: Option<Instant>,
    token: u64,
    cancel: Option<CancelToken>,
}

struct EncDone {
    kind: SlotKind,
    token: u64,
    result: Result<(Vec<u8>, f64), String>,
}

/// The selected set's encodes, one per filled slot.
pub struct Encoder {
    pub members: BTreeMap<SlotKind, Member>,
    pub cap: u32,
    /// An export is running: nothing starts (02 §2.3).
    pub paused: bool,
    pub error: Option<String>,
    next_token: u64,
    next_id: u64,
    tx: Sender<EncDone>,
    rx: Receiver<EncDone>,
}

/// A worker thread whose panic becomes `panicked(text)`: `worker::spawn_guarded` only takes `WorkerMsg`.
pub(crate) fn spawn<M: Send + 'static>(
    tx: Sender<M>,
    f: impl FnOnce() -> M + Send + 'static,
    panicked: impl FnOnce(String) -> M + Send + 'static,
) {
    std::thread::Builder::new()
        .name("sdc-gui-set-preview".into())
        .spawn(move || {
            let msg = std::panic::catch_unwind(std::panic::AssertUnwindSafe(f))
                .unwrap_or_else(|p| panicked(panic_text(&*p)));
            let _ = tx.send(msg);
        })
        .expect("spawn a worker thread");
}

fn panic_text(p: &(dyn std::any::Any + Send)) -> String {
    p.downcast_ref::<&str>()
        .map(|s| s.to_string())
        .or_else(|| p.downcast_ref::<String>().cloned())
        .unwrap_or_else(|| "panic".into())
}

pub(crate) fn ms(t0: Instant) -> f64 {
    t0.elapsed().as_secs_f64() * 1000.0
}

/// One member's bytes: the preview encode within `cap`, made into a DDS exactly as `Session::preview` builds it
/// (`session.rs:283-290`), or a DDS input's own file.
pub fn encode(
    session: &Session,
    source: &Source,
    cap: u32,
    cancel: &CancelToken,
) -> Result<Vec<u8>, String> {
    match source {
        Source::Job(job) => {
            let r = session
                .preview(job, cap, cancel)
                .map_err(|e| format!("{e:?}"))?;
            let (width, height) = r.source.dims[r.source.first_level];
            let desc = DdsDesc {
                width,
                height,
                format: r.format,
                srgb: r.srgb,
                header: resolve_header(r.format, r.srgb, job.profile.header),
            };
            dds_bytes(&desc, &r.levels).map_err(|e| e.to_string())
        }
        Source::Dds(p) => std::fs::read(p).map_err(|e| format!("{}: {e}", p.display())),
        Source::Blocked(w) => Err(w.to_string()),
    }
}

fn same_bytes(a: &MemberSlot, b: &MemberSlot) -> bool {
    a.item == b.item && a.job_key == b.job_key && a.status == b.status && a.sig == b.sig
}

impl Encoder {
    pub fn new(cap: u32) -> Encoder {
        let (tx, rx) = channel();
        Encoder {
            members: BTreeMap::new(),
            cap,
            paused: false,
            error: None,
            next_token: 0,
            next_id: 0,
            tx,
            rx,
        }
    }

    /// Another set: cancel and forget every member.
    pub fn reset(&mut self) {
        for m in self.members.values() {
            if let Some(c) = &m.cancel {
                c.cancel();
            }
        }
        self.members.clear();
        self.error = None;
    }

    /// The set's members now. A new member, or one whose item, status or signature changed, is encoded again: at
    /// once when `immediate` (a new selection), else after [`DEBOUNCE`] (an edit, 02 §2.4), with its old bytes still
    /// on show. A path-only change keeps the bytes. Members that left are cancelled.
    pub fn sync(&mut self, m: &AppModel, slots: Vec<MemberSlot>, now: Instant, immediate: bool) {
        let keep: Vec<SlotKind> = slots.iter().map(|s| s.kind).collect();
        self.members.retain(|k, mem| {
            let stay = keep.contains(k);
            if !stay {
                if let Some(c) = &mem.cancel {
                    c.cancel();
                }
            }
            stay
        });
        let due = if immediate { now } else { now + DEBOUNCE };
        for slot in slots {
            let source = m
                .item(&slot.item)
                .map(source_of)
                .unwrap_or(Source::Blocked(BLOCKED_NOTHING));
            let blocked = match &source {
                Source::Blocked(w) => Some(*w),
                _ => None,
            };
            match self.members.get_mut(&slot.kind) {
                Some(mem) if same_bytes(&mem.slot, &slot) => mem.slot = slot,
                Some(mem) => {
                    if let Some(c) = mem.cancel.take() {
                        c.cancel();
                    }
                    mem.state = match blocked {
                        Some(w) => EncState::Blocked(w),
                        None => EncState::Stale(mem.state.shown().cloned()),
                    };
                    mem.due = blocked.is_none().then_some(due);
                    mem.source = source;
                    mem.slot = slot;
                }
                None => {
                    let kind = slot.kind;
                    self.members.insert(
                        kind,
                        Member {
                            slot,
                            state: blocked.map_or(EncState::NotEncoded, EncState::Blocked),
                            due: blocked.is_none().then_some(due),
                            source,
                            token: 0,
                            cancel: None,
                        },
                    );
                }
            }
        }
    }

    /// Starts every due encode, one thread each (a set has at most 9 members). Nothing starts while an export
    /// runs or without a session.
    pub fn pump(&mut self, session: Option<&Arc<Session>>, now: Instant, export_running: bool) {
        self.paused = export_running;
        let Some(session) = session else {
            return;
        };
        if export_running {
            return;
        }
        for (kind, mem) in self.members.iter_mut() {
            if mem.due.is_none_or(|d| d > now) {
                continue;
            }
            mem.due = None;
            self.next_token += 1;
            let token = self.next_token;
            mem.token = token;
            let cancel = CancelToken::default();
            mem.cancel = Some(cancel.clone());
            mem.state = EncState::Encoding(mem.state.shown().cloned());
            let (session, source, cap, kind) = (session.clone(), mem.source.clone(), self.cap, *kind);
            spawn(
                self.tx.clone(),
                move || {
                    let t0 = Instant::now();
                    let result = encode(&session, &source, cap, &cancel).map(|b| (b, ms(t0)));
                    EncDone { kind, token, result }
                },
                move |e| EncDone {
                    kind,
                    token,
                    result: Err(format!("internal error in a preview encode: {e}")),
                },
            );
        }
    }

    /// Drains finished encodes. True when a member's bytes changed or it failed.
    pub fn poll(&mut self) -> bool {
        let mut changed = false;
        while let Ok(EncDone { kind, token, result }) = self.rx.try_recv() {
            let Some(mem) = self.members.get_mut(&kind) else {
                continue;
            };
            // Superseded (a newer edit, possibly still in its debounce).
            if mem.token != token || mem.due.is_some() {
                continue;
            }
            mem.cancel = None;
            mem.state = match result {
                Ok((dds, ms)) => {
                    self.next_id += 1;
                    EncState::Fresh(Arc::new(Encoded {
                        id: self.next_id,
                        dds: Arc::new(dds),
                        ms,
                    }))
                }
                Err(e) => EncState::Failed(e),
            };
            changed = true;
        }
        changed
    }

    pub fn settled(&self) -> bool {
        self.members.values().all(|m| m.state.settled())
    }

    /// Work is waiting or running (false while paused: the export keeps the app repainting).
    pub fn busy(&self) -> bool {
        !self.paused && !self.settled()
    }

    /// Whether sdc will produce `norm` for this set (a member that can be encoded serves it): PGPatcher's
    /// existence check (01 §1.4) passes for it.
    pub fn produces(&self, norm: &str) -> bool {
        self.members.values().any(|m| {
            !matches!(m.state, EncState::Blocked(_)) && m.slot.paths.iter().any(|p| p == norm)
        })
    }

    /// Canonical path → the member's bytes on show.
    pub fn memory_sources(&self) -> HashMap<String, FileSource> {
        let mut out = HashMap::new();
        for m in self.members.values() {
            if let Some(e) = m.state.shown() {
                for p in &m.slot.paths {
                    out.insert(
                        p.clone(),
                        FileSource::Memory {
                            id: e.id,
                            bytes: e.dds.clone(),
                        },
                    );
                }
            }
        }
        out
    }
}
```

- [ ] **Step 4: Run the tests and confirm they pass**

Run: `cargo test -p sdc-gui --test set_preview_model`
Expected: 5 passed. (Compiling needs the `pub mod` line from Step 5; add it first locally, then commit it separately.)

- [ ] **Step 5: Shared hook, own commit** (`crates/gui/src/model/mod.rs`, insert before line 12 `pub mod sets;`):
```rust
pub mod set_preview;
```
Gates: `cargo fmt --all && cargo clippy --workspace --all-targets -- -D warnings && cargo test --workspace`. Expected: clean.
```bash
git add crates/gui/src/model/mod.rs
git commit -m "gui: declare model::set_preview (shared hook, one line)" -m "Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

- [ ] **Step 6: Commit the module**

```bash
git add crates/gui/src/model/set_preview.rs crates/gui/tests/set_preview_model.rs
git commit -m "gui: set preview encodes: per-member DDS bytes, fresh/stale, paused during export" -m "Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

---

### Task 14: gui `model/set_preview.rs` part 2: fingerprint, change classification, mesh candidates, simulation

**Files:**
- Modify (own commit): `crates/gui/Cargo.toml`: after line 35 (`vfs = { path = "../vfs" }`) add `pgpsim = { path = "../pgpsim" }`
- Modify: `crates/gui/src/model/set_preview.rs`: merge the new `use` lines into the top import block; append everything else at the end of the file
- Test: `crates/gui/tests/set_preview_model.rs` (append)

**Interfaces:**
- Consumes: `pbr_export::json_entries` (`pbr_export.rs:169`), `sets::set_views` (`sets.rs:224`), `pgp::writer::{entry_node, document, Node::to_value}` (`writer.rs:216, 220, 17`), `model::Selection` (`model/mod.rs:42`), `TextureItem.set_key` (`items.rs:52`), `AppModel.{selection, session, export.running}` (`mod.rs:139,129`; `export.rs:57`), `settings::MeshesSettings` (`settings.rs:89`), `meshes::ScanTarget` (`meshes.rs:162`); Task 11 `Store::{open_read_only, nifs_using_texture, copy_location (store.rs:838), texture_location (:824), shapes_of (:1009)}`, `MatchRow`, `TexKey`; Task 12 `Content`, `Viewport3d::{set_content, set_material, content}`, `read_file`; contract `pgpsim::{prepare, shape_matches, simulate, SimInput, SimResult, Badge}`, `render3d::{parse, ParsedNif, ShapeId}`, `render3d::builtin::Builtin`, `MaterialState::{builtin_default, set_slot_raw, is_pbr}`.
- Produces (contract: `SetPreview`, `poll(&mut self, m: &AppModel)`, `busy(&self) -> bool`):
```rust
pub const NEEDS_CS, JSON_WINS, NO_SCAN, NO_MATCH, NO_BUILTIN_MATCH, OFF_FEATURES, PANEL_TITLE: &str;
pub const THROTTLE: Duration; pub const QUERY_LIMIT: usize; pub const BUILTINS: [(Builtin, &str); 3];
pub fn skip_text(reason: &str) -> String; pub fn badge_text(key: &str) -> String; pub fn count_text(n: usize) -> String;
pub fn builtin_name(b: Builtin) -> &'static str;
pub fn followed_set(m: &AppModel) -> Option<String>;
pub fn target_of(s: &MeshesSettings) -> Option<ScanTarget>;
#[derive(Debug, Clone, PartialEq)] pub struct SetFingerprint { pub set_key: String, pub label: String,
    pub entry: Option<EntryInput>, pub prefix: Option<String>, pub members: Vec<MemberSlot> }
pub fn fingerprint(m: &AppModel, set_key: &str) -> Option<SetFingerprint>;
pub fn tex_key(entry: &Value, default: &Value) -> Option<(TexKey, Option<String>)>;
#[derive(Debug, Clone, Default, PartialEq, Eq)] pub struct Delta { pub set: bool, pub encode: bool, pub requery: bool, pub reload: bool, pub material: bool }
pub fn classify(old: Option<&SetFingerprint>, new: &SetFingerprint, default: &Value) -> Delta;
#[derive(Debug, Clone, PartialEq)] pub struct Candidate { pub load_copy_id: i64, pub copy_id: i64, pub nif_path: String,
    pub origin: String, pub le_mesh: bool, pub derived: bool, pub shapes: Vec<(u32, Option<String>)> }
pub fn group_rows(rows: &[MatchRow], filter: Option<&str>, check: &dyn Fn(i64, &str, u32) -> bool) -> Vec<Candidate>;
#[derive(Debug, Clone, PartialEq)] pub enum Candidates { NotAsked, NoStore(String), Querying, Ready(Vec<Candidate>), Error(String) }
#[derive(Debug, Clone, PartialEq)] pub enum Outcome { Patched { badges: Vec<&'static str> }, Skip(String), NoMatch, Plain }
#[derive(Debug, Clone, PartialEq)] pub struct ShapeRow { pub id: ShapeId, pub name: String, pub outcome: Outcome, pub material: MaterialState }
pub fn simulate_shapes(entry: &Value, default: &Value, nif_path: &str, shapes: &[(ShapeId, String, MaterialState)],
                       exists: &dyn Fn(&str) -> bool) -> Vec<ShapeRow>;
pub fn builtin_source(entry: &Value, default: &Value) -> Option<(MaterialState, String)>;
pub fn plain_material(members: &[MemberSlot]) -> MaterialState;
#[derive(Debug, Clone, Copy, PartialEq, Eq)] pub enum MeshChoice { Auto, Nif(i64), Builtin(Builtin) }
#[derive(Debug, Clone, PartialEq)] pub enum Pick { Nif(Candidate), Builtin(Builtin) }
#[derive(Debug, Clone, Default, PartialEq)] pub struct Timings { pub query_ms: Option<f64>, pub nif_ms: Option<f64>, pub push_ms: Option<f64> }
pub struct SetPreview { pub v3: Viewport3d, pub enc: Encoder, pub enabled: bool, pub store_path: Option<PathBuf>,
    pub target: Option<ScanTarget>, pub fp: Option<SetFingerprint>, pub candidates: Candidates,
    pub choice: HashMap<String, MeshChoice>, pub rows: Vec<ShapeRow>, pub notices: Vec<String>, pub timings: Timings,
    pub shown: bool, pub input: bool, /* private state */ }
impl Default for SetPreview;   // enabled: false, no store, no shaders, no IO
impl SetPreview {
    pub fn new(shaders: Option<render3d::locate::Fingerprint>, target: Option<ScanTarget>, store_path: Option<PathBuf>, cap: u32) -> SetPreview;
    pub fn poll(&mut self, m: &AppModel); pub fn poll_at(&mut self, m: &AppModel, now: Instant);
    pub fn busy(&self) -> bool; pub fn relocate(&mut self); pub fn pick(&self) -> Pick; pub fn choose(&mut self, c: MeshChoice);
    pub fn exists(&self, pgp_path: &str) -> bool;   // the SimInput.exists closure: sdc output OR any VFS winner (loose/BSA)
}
```

- [ ] **Step 1: Add the dependency (own commit)**

`crates/gui/Cargo.toml`, after line 35 (`vfs = { path = "../vfs" }`):
```toml
pgpsim = { path = "../pgpsim" }
```
Run: `cargo build -p sdc-gui`. Expected: builds. Then:
```bash
git add crates/gui/Cargo.toml Cargo.lock
git commit -m "gui: depend on pgpsim (add-only)" -m "Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

- [ ] **Step 2: Write the failing tests** (append to `crates/gui/tests/set_preview_model.rs`)

```rust
// ---- Task 14: fingerprint, classification, query keys, simulation, orchestration ----

use modscan::store::{MatchRow, TexKey};
use nif::material::MaterialState;
use render3d::builtin::Builtin;
use render3d::ShapeId;
use sdc_gui::model::viewport3d::Content;
use sdc_gui::pgp::writer::entry_node;
use serde_json::{json, Value};
use std::path::{Path, PathBuf};

fn default_block() -> Value {
    sdc_gui::pgp::writer::document(&[]).to_value()["default"].clone()
}

fn entry(pairs: &[(&str, Value)]) -> EntryInput {
    EntryInput {
        settings: pairs.iter().map(|(k, v)| (k.to_string(), v.clone())).collect(),
        slots: Default::default(),
        derived_match: r"x\rock".into(),
    }
}

fn value(pairs: &[(&str, Value)]) -> Value {
    entry_node(&entry(pairs)).to_value()
}

fn fp_of(entry: Option<EntryInput>, sig: &str) -> SetFingerprint {
    SetFingerprint {
        set_key: "k".into(),
        label: "x/rock".into(),
        entry,
        prefix: Some(r"textures\pbr\x\rock".into()),
        members: vec![MemberSlot {
            kind: SlotKind::Diffuse,
            item: "x/rock.png".into(),
            job_key: "textures/pbr/x/rock.dds".into(),
            status: "Ready".into(),
            sig: sig.into(),
            paths: vec!["textures/pbr/x/rock.dds".into()],
        }],
    }
}

#[test]
fn a_material_key_is_json_only_and_a_texture_change_re_encodes() {
    let d = default_block();
    let a = fp_of(Some(entry(&[])), "s1");
    let rough = fp_of(Some(entry(&[("roughness_scale", json!(0.5))])), "s1");
    assert_eq!(classify(Some(&a), &rough, &d), Delta { material: true, ..Default::default() });
    let tex = fp_of(Some(entry(&[])), "s2");
    assert_eq!(classify(Some(&a), &tex, &d), Delta { encode: true, ..Default::default() });
    assert_eq!(classify(Some(&a), &a.clone(), &d), Delta::default());
}

#[test]
fn match_keys_requery_paths_reload_and_another_set_or_d3_is_everything() {
    let d = default_block();
    let a = fp_of(Some(entry(&[])), "s1");
    let moved = fp_of(Some(entry(&[("match_diffuse", json!(r"y\rock"))])), "s1");
    let dm = classify(Some(&a), &moved, &d);
    assert!(dm.requery && dm.reload && !dm.material, "{dm:?}");
    let mut slot = fp_of(Some(entry(&[])), "s1");
    slot.entry.as_mut().unwrap().slots.insert(3, r"effects\white.dds".into());
    assert_eq!(classify(Some(&a), &slot, &d), Delta { reload: true, ..Default::default() });
    let mut other = a.clone();
    other.set_key = "other".into();
    assert_eq!(classify(Some(&a), &other, &d), Delta { set: true, ..Default::default() });
    assert!(classify(Some(&a), &fp_of(None, "s1"), &d).set, "simulated → plain (D3)");
    assert!(classify(None, &a, &d).set);
}

#[test]
fn the_fixture_fingerprint_follows_edits_and_the_target() {
    let dir = tempfile::tempdir().unwrap();
    let mut m = pbr_opened(dir.path());
    let key = rock(&m).0.key;
    let d = default_block();
    let before = fingerprint(&m, &key).unwrap();
    assert!(before.entry.is_some());
    m.set_json_value("landscape/rock01", "roughness_scale", Some(json!(0.25)));
    let after = fingerprint(&m, &key).unwrap();
    assert_eq!(classify(Some(&before), &after, &d), Delta { material: true, ..Default::default() });
    m.set_target(engine::config::Target::Vanilla);
    assert!(m.wait_idle(Duration::from_secs(60)));
    let d3 = fingerprint(&m, &key).unwrap();
    assert!(d3.entry.is_none(), "json_entries is CS-only");
    assert!(classify(Some(&after), &d3, &d).set);
}

#[test]
fn the_query_key_follows_the_match_mode() {
    let d = default_block();
    let key = |pairs: &[(&str, Value)]| tex_key(&value(pairs), &d).unwrap();
    assert_eq!(key(&[]), (TexKey::Suffix { slot: 0, stem: r"x\rock".into() }, None));
    assert_eq!(key(&[("match_diffuse", json!(r"x\rock_d.dds"))]).0, TexKey::Suffix { slot: 0, stem: r"x\rock_d".into() });
    assert_eq!(key(&[("match_normal", json!(r"x\rock"))]).0, TexKey::Suffix { slot: 1, stem: r"x\rock".into() });
    assert_eq!(key(&[("path_contains", json!("rock"))]).0, TexKey::Contains { slot: 0, frag: "rock".into() });
    assert_eq!(key(&[("match3", json!(r"x\rock_g.dds"))]).0, TexKey::Exact { slot: 2, path: r"x\rock_g.dds".into() });
    assert_eq!(key(&[("nif_filter", json!("armor"))]).1.as_deref(), Some("armor"));
}

#[test]
fn shapes_are_patched_skipped_or_left_alone() {
    let d = default_block();
    let e = value(&[]);
    let mut hit = MaterialState::builtin_default();
    hit.set_slot_raw(0, r"textures\x\rock.dds");
    let mut other = MaterialState::builtin_default();
    other.set_slot_raw(0, r"textures\x\stone.dds");
    let shapes = vec![
        (ShapeId(1), "Rock".to_string(), hit.clone()),
        (ShapeId(2), "Stone".to_string(), other.clone()),
    ];
    let rows = simulate_shapes(&e, &d, r"meshes\x\rock.nif", &shapes, &|_| true);
    assert!(matches!(rows[0].outcome, Outcome::Patched { .. }), "{:?}", rows[0]);
    assert_eq!(rows[0].material.slots[0].norm, "textures/pbr/x/rock.dds");
    assert!(rows[0].material.is_pbr());
    assert_eq!(rows[1].outcome, Outcome::NoMatch);
    assert_eq!(rows[1].material, other);
    // A patched slot that neither sdc nor the game provides: PGPatcher skips the shape; it keeps its source.
    let rows = simulate_shapes(&e, &d, r"meshes\x\rock.nif", &shapes, &|p| !p.ends_with("_rmaos.dds"));
    let Outcome::Skip(why) = &rows[0].outcome else {
        panic!("{:?}", rows[0])
    };
    assert!(why.contains("rmaos"), "{why}");
    assert!(rows[0].text().contains("will be skipped"));
    assert_eq!(rows[0].material, hit);
}

#[test]
fn a_built_in_shape_stands_in_for_every_match_mode() {
    let d = default_block();
    let cases: [&[(&str, Value)]; 5] = [
        &[],
        &[("match_normal", json!(r"x\rock"))],
        &[("path_contains", json!("rock"))],
        &[("match1", json!(r"x\rock.dds"))],
        &[("nif_filter", json!(r"armor\iron"))],
    ];
    for pairs in cases {
        let e = value(pairs);
        let (src, nif) = builtin_source(&e, &d).unwrap_or_else(|| panic!("{pairs:?}"));
        let rows = simulate_shapes(&e, &d, &nif, &[(ShapeId(0), "b".into(), src)], &|_| true);
        assert!(matches!(rows[0].outcome, Outcome::Patched { .. }), "{pairs:?}: {:?}", rows[0]);
    }
}

#[test]
fn plain_textures_go_to_their_vanilla_slots() {
    let mut members = fp_of(None, "s").members;
    members.push(MemberSlot {
        kind: SlotKind::Normal,
        paths: vec![plain_path(SlotKind::Normal)],
        ..members[0].clone()
    });
    members[0].paths = vec![plain_path(SlotKind::Diffuse)];
    let m = plain_material(&members);
    assert_eq!(m.slots[0].norm, plain_path(SlotKind::Diffuse));
    assert_eq!(m.slots[1].norm, plain_path(SlotKind::Normal));
    assert!(!m.is_pbr());
}

fn row(copy: i64, load: i64, path: &str, block: u32, derived: bool) -> MatchRow {
    MatchRow {
        copy_id: copy,
        nif_path: path.into(),
        origin: "Mod".into(),
        shape_block: block,
        shape_name: Some(format!("S{block}")),
        le_mesh: false,
        skinned: false,
        derived,
        load_copy_id: load,
    }
}

#[test]
fn rows_group_by_nif_and_keep_only_shapes_pgpatcher_matches() {
    let rows = vec![
        row(1, 1, "meshes/a/rock.nif", 3, false),
        row(1, 1, "meshes/a/rock.nif", 4, false),
        row(2, 9, "meshes/armor/iron.nif", 0, true),
        row(5, 5, "meshes/b/gone.nif", 0, false),
    ];
    let check = |copy: i64, nif: &str, block: u32| {
        assert!(nif.starts_with(r"meshes\"), "PGPatcher form: {nif}");
        block != 4 && copy != 5
    };
    let c = group_rows(&rows, None, &check);
    assert_eq!(c.iter().map(|c| c.load_copy_id).collect::<Vec<_>>(), [1, 9]);
    assert_eq!(c[0].nif_path, r"meshes\a\rock.nif");
    assert_eq!(c[0].shapes, vec![(3, Some("S3".to_string()))]);
    assert!(c[1].derived && c[1].copy_id == 2);
    let f = group_rows(&rows, Some(r"ARMOR\"), &check);
    assert_eq!(f.iter().map(|c| c.load_copy_id).collect::<Vec<_>>(), [9]);
}

/// Polls like the app does (panel drawn, input every frame) until nothing is in flight.
fn run(s: &mut SetPreview, m: &AppModel) {
    let t = Instant::now();
    loop {
        s.shown = true;
        s.input = true;
        s.poll(m);
        if !s.busy() {
            break;
        }
        assert!(t.elapsed() < Duration::from_secs(90), "the preview did not settle");
        std::thread::sleep(Duration::from_millis(5));
    }
}

fn shown_ids(s: &SetPreview) -> Vec<u64> {
    s.enc
        .members
        .values()
        .filter_map(|x| x.state.shown().map(|e| e.id))
        .collect()
}

#[test]
fn without_a_scan_db_the_set_shows_patched_on_the_sphere_and_json_edits_encode_nothing() {
    let dir = tempfile::tempdir().unwrap();
    let mut m = pbr_opened(dir.path());
    let key = rock(&m).0.key;
    m.select_set(&key);
    let mut s = SetPreview::new(None, None, None, DEFAULT_CAP);
    run(&mut s, &m);
    assert_eq!(s.candidates, Candidates::NoStore(NO_SCAN.into()));
    let Some(Content::Builtin { shape: Builtin::Sphere, material, textures, .. }) = s.v3.content().cloned() else {
        panic!("{:?}", s.v3.content())
    };
    assert!(material.is_pbr());
    assert!(textures.contains_key("textures/pbr/landscape/rock01.dds"));
    assert!(matches!(s.rows[0].outcome, Outcome::Patched { .. }), "{:?}", s.rows);
    let ids = shown_ids(&s);
    m.set_json_value("landscape/rock01", "roughness_scale", Some(json!(0.25)));
    run(&mut s, &m);
    let Some(Content::Builtin { material: after, .. }) = s.v3.content().cloned() else {
        panic!()
    };
    assert_ne!(material, after, "the material follows the edit");
    assert_eq!(shown_ids(&s), ids, "a JSON-only edit encodes nothing");
}

fn lighting_shape(block: u32, slots: &[&str]) -> nif::scan::ShapeRow {
    use nif::props::{FieldOffsets, Lighting, LightingParams, LightingTrailer};
    use nif::shape_head::{ShapeHead, ShapeKind};
    nif::scan::ShapeRow {
        block,
        head: ShapeHead {
            kind: ShapeKind::BSTriShape,
            name: Some("Rock".into()),
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

/// A scan DB with one winning NIF (not on disk) whose block 3 uses `slots`.
pub fn scan_db(dir: &Path, slots: &[&str]) -> PathBuf {
    use modscan::store::{CopyResult, OriginSpec, Store, KIND_MOD};
    let p = dir.join("scan.db");
    let mut s = Store::open(&p).unwrap();
    s.begin_scan().unwrap();
    let o = s
        .upsert_origin(&OriginSpec {
            kind: KIND_MOD,
            name: "Rocks".into(),
            root: dir.join("mods").join("Rocks").to_string_lossy().into_owned(),
            parent: None,
            priority: 0,
            derived_by: None,
            derived_state: None,
        })
        .unwrap();
    let scan = nif::scan::NifScan {
        bsver: 100,
        le_mesh: false,
        shapes: vec![lighting_shape(3, slots)],
        problems: vec![],
    };
    let path = "meshes/landscape/rocks/rock01.nif";
    let id = s
        .upsert_copy(path, o, 1, 1, CopyResult::Scanned(scan), None)
        .unwrap();
    let pid = s.path_ids(&[path]).unwrap()[0];
    s.set_winners(&[(pid, (id, 1))].into_iter().collect(), &Default::default())
        .unwrap();
    p
}

/// Part P's binding clarification: `exists` is asked for every non-empty slot, vanilla ones included. A texture the
/// game has only inside a BSA must count, or a shape that keeps a vanilla slot would be skipped.
#[test]
fn exists_accepts_sdc_outputs_and_any_vfs_winner_including_bsa_only_textures() {
    use modscan::store::{OriginSpec, Store, KIND_BSA};
    let dir = tempfile::tempdir().unwrap();
    let mut m = pbr_opened(dir.path());
    let key = rock(&m).0.key;
    m.select_set(&key);
    let db = scan_db(dir.path(), &[r"textures\landscape\rock01.dds"]);
    {
        let mut s = Store::open(&db).unwrap();
        let bsa = s
            .upsert_origin(&OriginSpec {
                kind: KIND_BSA,
                name: "Skyrim - Textures0.bsa".into(),
                root: dir.path().join("Skyrim - Textures0.bsa").to_string_lossy().into_owned(),
                parent: None,
                priority: 0,
                derived_by: None,
                derived_state: None,
            })
            .unwrap();
        let pid = s.path_ids(&["textures/clutter/vanilla_m.dds"]).unwrap()[0];
        s.set_textures(&[(pid, bsa, 1, 1)], &[(pid, (bsa, 1))].into_iter().collect())
            .unwrap();
    }
    let mut s = SetPreview::new(None, None, Some(db), DEFAULT_CAP);
    run(&mut s, &m);
    assert!(s.exists(r"textures\clutter\vanilla_m.dds"), "a BSA-only vanilla slot exists");
    assert!(s.exists(r"Textures\Clutter\Vanilla_M.DDS"), "any case, PGPatcher form");
    assert!(s.exists(r"textures\pbr\landscape\rock01_n.dds"), "sdc's own output");
    assert!(!s.exists(r"textures\clutter\nothing.dds"));
}

#[test]
fn a_scanned_mesh_is_listed_and_an_unreadable_one_falls_back_to_the_sphere() {
    let dir = tempfile::tempdir().unwrap();
    let mut m = pbr_opened(dir.path());
    let key = rock(&m).0.key;
    m.select_set(&key);
    let db = scan_db(dir.path(), &[r"textures\landscape\rock01.dds"]);
    let mut s = SetPreview::new(None, None, Some(db), DEFAULT_CAP);
    run(&mut s, &m);
    let Candidates::Ready(list) = &s.candidates else {
        panic!("{:?}", s.candidates)
    };
    assert_eq!(list.len(), 1);
    assert_eq!(list[0].nif_path, r"meshes\landscape\rocks\rock01.nif");
    assert_eq!(list[0].shapes, vec![(3, Some("Rock".to_string()))]);
    assert!(s.timings.query_ms.is_some());
    // The NIF is not on disk: the sphere stands in and the reason is shown.
    assert!(matches!(s.v3.content(), Some(Content::Builtin { shape: Builtin::Sphere, .. })));
    assert!(s.notices.iter().any(|n| n.contains("rock01.nif")), "{:?}", s.notices);
    // The choice is remembered per set, for the session.
    s.choose(MeshChoice::Builtin(Builtin::Cube));
    run(&mut s, &m);
    assert!(matches!(s.v3.content(), Some(Content::Builtin { shape: Builtin::Cube, .. })));
    assert_eq!(s.choice.get(&key), Some(&MeshChoice::Builtin(Builtin::Cube)));
}
```

- [ ] **Step 3: Run the tests and confirm they fail**

Run: `cargo test -p sdc-gui --test set_preview_model`
Expected: compile errors: `cannot find type SetFingerprint`, `cannot find function classify`, `cannot find struct SetPreview`.

- [ ] **Step 4: Implement** (`crates/gui/src/model/set_preview.rs`)

Merge into the top import block:
```rust
use crate::model::meshes::ScanTarget;
use crate::model::pbr_export::json_entries;
use crate::model::sets::set_views;
use crate::model::viewport3d::{read_file, Content, Viewport3d};
use crate::model::Selection;
use crate::pgp::writer::{document, entry_node};
use crate::settings::MeshesSettings;
use modscan::store::{MatchRow, ShapeView, Store, TexKey};
use nif::material::MaterialState;
use nif::path::TexPath;
use pgpsim::{Badge, SimInput, SimResult};
use render3d::builtin::Builtin;
use render3d::{ParsedNif, ShapeId};
use serde_json::Value;
use std::cell::{Cell, RefCell};
use std::collections::HashSet;
use std::path::Path;
use std::time::Duration;
```

Append at the end of the file:
```rust
// ---- Part 2: fingerprint, change classification, mesh candidates, PGPatcher simulation, the panel's model ----

pub const PANEL_TITLE: &str = "Set preview (3D)";
pub const NEEDS_CS: &str = "PGPatcher preview needs the CS target and a PBR set";
pub const JSON_WINS: &str = "Assumes your sdc JSON wins over other mods' PGPatcher configs.";
pub const NO_SCAN: &str = "Scan your mods in the Meshes view to preview on a real mesh";
pub const NO_MATCH: &str = "No scanned mesh uses this diffuse";
pub const NO_BUILTIN_MATCH: &str =
    "A built-in shape cannot match this entry's match keys: it shows the plain textures; pick a game mesh";
/// D4: the CS features the preview leaves off (03 §3.2 "off" rows), under the single CS toggle.
pub const OFF_FEATURES: &str = "Off in this preview: Light Limit Fix, Skylighting, Screen-Space Shadows, \
Subsurface Scattering, Dynamic Cubemaps, Image-Based Lighting, Physical Sky, Wetness, Cloud Shadows, terrain effects, \
water, fog, Volumetric Shadows";
/// How often the fingerprint is checked when no input arrived (02 §3.2).
pub const THROTTLE: Duration = Duration::from_millis(200);
/// Rows asked of the scan DB; `nif_filter` and `shape_matches` thin them out afterwards.
pub const QUERY_LIMIT: usize = 500;
pub const BUILTINS: [(Builtin, &str); 3] = [
    (Builtin::Sphere, "Sphere"),
    (Builtin::Plane, "Plane"),
    (Builtin::Cube, "Cube"),
];

pub fn builtin_name(b: Builtin) -> &'static str {
    BUILTINS
        .iter()
        .find(|(x, _)| *x == b)
        .map_or("Built-in", |(_, n)| n)
}

pub fn skip_text(reason: &str) -> String {
    format!("will be skipped: {reason}")
}

pub fn badge_text(key: &str) -> String {
    format!("not previewed: {key}")
}

pub fn count_text(n: usize) -> String {
    if n == 1 {
        "1 mesh uses this diffuse".into()
    } else {
        format!("{n} meshes use this diffuse")
    }
}

/// The set the panel follows: the selected set, or the set of the selected texture (spec §8, 02 Q5).
pub fn followed_set(m: &AppModel) -> Option<String> {
    match &m.selection {
        Selection::Set(k) => Some(k.clone()),
        Selection::Item(k) => m.item(k).and_then(|i| i.set_key.clone()),
        Selection::None => None,
    }
}

/// The scan target `[meshes]` names, read as `MeshesModel::activate_with` reads it (`meshes.rs:304-311`): the
/// Meshes view may never have been opened, so its `target` may still be `None`.
pub fn target_of(s: &MeshesSettings) -> Option<ScanTarget> {
    match (&s.instance, &s.profile, &s.folder) {
        (Some(root), Some(profile), _) => Some(ScanTarget::Mo2 {
            root: root.clone(),
            profile: profile.clone(),
        }),
        (_, _, Some(f)) => Some(ScanTarget::Folder(f.clone())),
        _ => None,
    }
}

/// Everything the preview depends on, for one set (02 §3.2).
#[derive(Debug, Clone, PartialEq)]
pub struct SetFingerprint {
    pub set_key: String,
    pub label: String,
    /// The PGPatcher entry; `None` when the simulation cannot run (not CS, or not a PBR set: D3).
    pub entry: Option<EntryInput>,
    pub prefix: Option<String>,
    pub members: Vec<MemberSlot>,
}

/// `None` when the set is gone.
pub fn fingerprint(m: &AppModel, set_key: &str) -> Option<SetFingerprint> {
    let (v, entry) = match json_entries(m).into_iter().find(|(v, _)| v.key == set_key) {
        Some((v, e)) => (v, Some(e)),
        None => (set_views(m).into_iter().find(|v| v.key == set_key)?, None),
    };
    let members = member_slots(m, &v, entry.as_ref());
    Some(SetFingerprint {
        set_key: set_key.into(),
        label: v.label.clone(),
        entry,
        prefix: v.prefix.clone(),
        members,
    })
}

fn strip_ext(v: &str) -> String {
    match v.rfind('.') {
        Some(i) if !v[i..].contains(['\\', '/']) => v[..i].to_string(),
        _ => v.to_string(),
    }
}

/// The scan-DB key and `nif_filter` of an entry, read from the entry as PGPatcher reads it (`pgpsim::prepare`:
/// default merged, `texture` → `match_diffuse`). The first key set wins, in `prefix::match_mode`'s order:
/// `match_normal`, `path_contains`, `match1..8`, `match_diffuse`. `None`: no match key at all.
pub fn tex_key(entry: &Value, default: &Value) -> Option<(TexKey, Option<String>)> {
    let p = pgpsim::prepare(entry, default);
    let s = |k: &str| {
        p.get(k)
            .and_then(Value::as_str)
            .filter(|v| !v.trim_matches('\\').trim().is_empty())
            .map(str::to_string)
    };
    let stem = |v: String| strip_ext(v.trim_start_matches('\\'));
    let key = if let Some(v) = s("match_normal") {
        TexKey::Suffix { slot: 1, stem: stem(v) }
    } else if let Some(v) = s("path_contains") {
        TexKey::Contains { slot: 0, frag: v }
    } else if let Some((n, v)) = (1..=8u32).find_map(|n| s(&format!("match{n}")).map(|v| (n, v))) {
        TexKey::Exact { slot: n - 1, path: v }
    } else if let Some(v) = s("match_diffuse") {
        TexKey::Suffix { slot: 0, stem: stem(v) }
    } else {
        return None;
    };
    Some((key, s("nif_filter")))
}

/// What a fingerprint change means (02 §3.2 table, spec §2.3).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Delta {
    /// Another set, or the set switched between simulated and plain (D3): everything again.
    pub set: bool,
    /// A member's bytes change: encode it again, then reload (P1-A has no hot-swap).
    pub encode: bool,
    /// The match key or `nif_filter` changed: query the meshes again (and reload).
    pub requery: bool,
    /// Texture paths changed (prefix, slot paths): rebuild the texture map and reload.
    pub reload: bool,
    /// Only material keys changed: simulate again, `SetMaterial` for the shapes whose material changed.
    pub material: bool,
}

pub fn classify(old: Option<&SetFingerprint>, new: &SetFingerprint, default: &Value) -> Delta {
    let Some(old) = old.filter(|o| o.set_key == new.set_key && o.entry.is_some() == new.entry.is_some())
    else {
        return Delta {
            set: true,
            ..Default::default()
        };
    };
    let bytes = |f: &SetFingerprint| {
        f.members
            .iter()
            .map(|s| (s.kind, s.item.clone(), s.job_key.clone(), s.status.clone(), s.sig.clone()))
            .collect::<Vec<_>>()
    };
    let paths = |f: &SetFingerprint| {
        f.members
            .iter()
            .map(|s| (s.kind, s.paths.clone()))
            .collect::<Vec<_>>()
    };
    let mut d = Delta {
        encode: bytes(old) != bytes(new),
        reload: paths(old) != paths(new) || old.prefix != new.prefix,
        ..Default::default()
    };
    if let (Some(a), Some(b)) = (&old.entry, &new.entry) {
        let (va, vb) = (entry_node(a).to_value(), entry_node(b).to_value());
        if tex_key(&va, default) != tex_key(&vb, default) {
            d.requery = true;
            d.reload = true;
        }
        if a.slots != b.slots {
            d.reload = true;
        }
        d.material = va != vb && !d.reload;
    }
    d
}

/// One game NIF whose shapes match the entry (02 §5.3), best first.
#[derive(Debug, Clone, PartialEq)]
pub struct Candidate {
    pub load_copy_id: i64,
    /// The game winner (another copy when PGPatcher's output wins in game).
    pub copy_id: i64,
    /// PGPatcher form, as `pgpsim` takes it: `meshes\…`.
    pub nif_path: String,
    pub origin: String,
    pub le_mesh: bool,
    pub derived: bool,
    /// (block, name) of the matching shapes, re-checked with `pgpsim::shape_matches`.
    pub shapes: Vec<(u32, Option<String>)>,
}

impl Candidate {
    pub fn label(&self) -> String {
        format!(
            "{} · {} · {} matching shape{}{}{}",
            self.nif_path,
            self.origin,
            self.shapes.len(),
            if self.shapes.len() == 1 { "" } else { "s" },
            if self.le_mesh { " · LE, not drawn" } else { "" },
            if self.derived { " · patched by PGPatcher in game" } else { "" },
        )
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum Candidates {
    NotAsked,
    NoStore(String),
    Querying,
    Ready(Vec<Candidate>),
    Error(String),
}

/// Rows → one candidate per copy to load, in row order. `filter` is `nif_filter` (`icontains` on the `meshes\…`
/// path, 01 §1.3); `check(load copy, nif path, block)` is the `shape_matches` re-check.
pub fn group_rows(
    rows: &[MatchRow],
    filter: Option<&str>,
    check: &dyn Fn(i64, &str, u32) -> bool,
) -> Vec<Candidate> {
    let filter = filter.map(str::to_ascii_lowercase);
    let mut out: Vec<Candidate> = vec![];
    for r in rows {
        let nif = r.nif_path.replace('/', "\\");
        if filter
            .as_deref()
            .is_some_and(|f| !nif.to_ascii_lowercase().contains(f))
        {
            continue;
        }
        if !check(r.load_copy_id, &nif, r.shape_block) {
            continue;
        }
        match out.iter_mut().find(|c| c.load_copy_id == r.load_copy_id) {
            Some(c) => c.shapes.push((r.shape_block, r.shape_name.clone())),
            None => out.push(Candidate {
                load_copy_id: r.load_copy_id,
                copy_id: r.copy_id,
                nif_path: nif,
                origin: r.origin.clone(),
                le_mesh: r.le_mesh,
                derived: r.derived,
                shapes: vec![(r.shape_block, r.shape_name.clone())],
            }),
        }
    }
    out
}

/// A stored shape's slots as PGPatcher reads them: the raw string when the store kept it, else the canonical path
/// with backslashes (the same string up to case, which PGPatcher folds).
fn view_slots(v: &ShapeView) -> Vec<TexPath> {
    v.textures
        .iter()
        .zip(&v.raw)
        .map(|(norm, raw)| match (raw, norm) {
            (Some(r), _) => nif::path::canon_texture(r.raw.as_bytes()),
            (None, Some(n)) => nif::path::canon_texture(n.replace('/', "\\").as_bytes()),
            (None, None) => nif::path::canon_texture(b""),
        })
        .collect()
}

/// The worker side of a query: its own read-only connection (a `Connection` is not `Sync`; a scan may be writing).
fn query(path: &Path, key: &TexKey, filter: Option<&str>, prepared: &Value) -> Result<Vec<Candidate>, String> {
    let store = Store::open_read_only(path).map_err(|e| e.to_string())?;
    let rows = store
        .nifs_using_texture(key, QUERY_LIMIT)
        .map_err(|e| e.to_string())?;
    let cache: RefCell<HashMap<i64, Vec<ShapeView>>> = RefCell::new(HashMap::new());
    let check = |copy: i64, nif: &str, block: u32| -> bool {
        let mut c = cache.borrow_mut();
        let shapes = c
            .entry(copy)
            .or_insert_with(|| store.shapes_of(copy).unwrap_or_default());
        shapes
            .iter()
            .find(|s| s.block == block)
            .is_some_and(|s| pgpsim::shape_matches(prepared, nif, &view_slots(s)))
    };
    Ok(group_rows(&rows, filter, &check))
}

/// A game NIF read and parsed for the preview: source materials, the game's copy of every texture its shapes name,
/// and its path in PGPatcher form.
#[derive(Debug, Clone)]
pub struct LoadedNif {
    pub load_copy_id: i64,
    pub nif_path: String,
    pub parsed: ParsedNif,
    pub vfs: HashMap<String, FileSource>,
    pub ms: f64,
}

fn load_nif(path: &Path, c: &Candidate) -> Result<LoadedNif, String> {
    let t0 = Instant::now();
    let store = Store::open_read_only(path).map_err(|e| e.to_string())?;
    let (p, kind, root, _) = store
        .copy_location(c.load_copy_id)
        .map_err(|e| e.to_string())?
        .ok_or_else(|| format!("{}: no longer in the scan", c.nif_path))?;
    let bytes = read_file(&FileSource::from_store(kind, &root, &p))?;
    let parsed = render3d::parse(&bytes);
    let mut vfs = HashMap::new();
    for t in parsed.shapes.iter().flat_map(|s| s.material.slots.iter()) {
        if t.norm.is_empty() || vfs.contains_key(&t.norm) {
            continue;
        }
        if let Some((k, r)) = store.texture_location(&t.norm).map_err(|e| e.to_string())? {
            vfs.insert(t.norm.clone(), FileSource::from_store(k, &r, &t.norm));
        }
    }
    Ok(LoadedNif {
        load_copy_id: c.load_copy_id,
        nif_path: c.nif_path.clone(),
        parsed,
        vfs,
        ms: ms(t0),
    })
}

/// What PGPatcher does to one shape of the scene.
#[derive(Debug, Clone, PartialEq)]
pub enum Outcome {
    /// Keys the preview cannot show (`smooth_angle`, `auto_uv`, …): the "not previewed" badges.
    Patched { badges: Vec<&'static str> },
    /// PGPatcher would skip the shape for this file (01 §1.4); the shape keeps its source material (red badge).
    Skip(String),
    NoMatch,
    /// No simulation (D3, or a built-in shape no slot of which can match the entry).
    Plain,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ShapeRow {
    pub id: ShapeId,
    pub name: String,
    pub outcome: Outcome,
    /// What the renderer draws: the patched material, or the source one.
    pub material: MaterialState,
}

impl ShapeRow {
    pub fn text(&self) -> String {
        match &self.outcome {
            Outcome::Patched { .. } => format!("{}: patched as PGPatcher would", self.name),
            Outcome::Skip(r) => format!("{}: {}", self.name, skip_text(r)),
            Outcome::NoMatch => format!("{}: not matched, original textures", self.name),
            Outcome::Plain => format!("{}: plain textures, no PGPatcher simulation", self.name),
        }
    }
}

/// sdc's one entry on each shape (`pgpsim::simulate`). `exists` gets PGPatcher-form paths.
pub fn simulate_shapes(
    entry: &Value,
    default: &Value,
    nif_path: &str,
    shapes: &[(ShapeId, String, MaterialState)],
    exists: &dyn Fn(&str) -> bool,
) -> Vec<ShapeRow> {
    shapes
        .iter()
        .map(|(id, name, source)| {
            let r = pgpsim::simulate(&SimInput {
                entry,
                default,
                nif_path,
                source,
                exists,
            });
            let (outcome, material) = match r {
                SimResult::Patched { material, badges, .. } => (
                    Outcome::Patched {
                        badges: badges
                            .iter()
                            .map(|b| match b {
                                Badge::NotPreviewed(k) => *k,
                            })
                            .collect(),
                    },
                    material,
                ),
                SimResult::Skip(why) => (Outcome::Skip(why), source.clone()),
                SimResult::NoMatch => (Outcome::NoMatch, source.clone()),
            };
            ShapeRow {
                id: *id,
                name: name.clone(),
                outcome,
                material,
            }
        })
        .collect()
}

/// A built-in shape as a shape the entry matches: `builtin_default` with the slot its match key reads set to a
/// path that key matches, and a mesh path that passes `nif_filter`. `None` when no such stand-in matches.
pub fn builtin_source(entry: &Value, default: &Value) -> Option<(MaterialState, String)> {
    let (key, filter) = tex_key(entry, default)?;
    let mut m = MaterialState::builtin_default();
    match &key {
        TexKey::Suffix { slot: 0, stem } => m.set_slot_raw(0, &format!("textures\\{stem}.dds")),
        TexKey::Suffix { slot, stem } => m.set_slot_raw(*slot as usize, &format!("textures\\{stem}_n.dds")),
        TexKey::Contains { slot, frag } => m.set_slot_raw(
            *slot as usize,
            &format!("textures\\{}.dds", frag.trim_matches('\\')),
        ),
        TexKey::Exact { slot, path } => {
            let p = if path.to_ascii_lowercase().starts_with("textures\\") {
                path.clone()
            } else {
                format!("textures\\{path}")
            };
            m.set_slot_raw(*slot as usize, &p)
        }
    }
    let nif = match &filter {
        Some(f) => format!("meshes\\{}\\sdcbuiltin.nif", f.trim_matches('\\')),
        None => "meshes\\sdcbuiltin.nif".to_string(),
    };
    let prepared = pgpsim::prepare(entry, default);
    pgpsim::shape_matches(&prepared, &nif, &m.slots).then_some((m, nif))
}

/// No simulation: `builtin_default` with each member's first path at its vanilla slot (diffuse 0, normal 1,
/// glow/skin 2, height 3).
pub fn plain_material(members: &[MemberSlot]) -> MaterialState {
    let mut m = MaterialState::builtin_default();
    for s in members {
        let slot = match s.kind {
            SlotKind::Diffuse => 0,
            SlotKind::Normal => 1,
            SlotKind::Emissive | SlotKind::SkinSk => 2,
            SlotKind::Height => 3,
            _ => continue,
        };
        if let Some(p) = s.paths.first() {
            m.set_slot_raw(slot, &p.replace('/', "\\"));
        }
    }
    m
}

/// The mesh per set (spec §2.3): the best candidate, a picked NIF, or a built-in shape.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MeshChoice {
    Auto,
    Nif(i64),
    Builtin(Builtin),
}

/// What is drawn now.
#[derive(Debug, Clone, PartialEq)]
pub enum Pick {
    Nif(Candidate),
    Builtin(Builtin),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Shown {
    Nif(i64),
    Builtin(Builtin),
}

/// Wall times for the verification (Task 17); encode times are on each member, edit times in `v3.last_edit`.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Timings {
    pub query_ms: Option<f64>,
    pub nif_ms: Option<f64>,
    /// Simulation of every shape plus building the scene.
    pub push_ms: Option<f64>,
}

enum SpMsg {
    Queried {
        token: u64,
        result: Result<(Vec<Candidate>, f64), String>,
    },
    NifLoaded {
        token: u64,
        result: Result<LoadedNif, String>,
    },
}

pub struct SetPreview {
    /// Its own viewport (D2: a second D3D device; the shader cache on disk is shared).
    pub v3: Viewport3d,
    pub enc: Encoder,
    /// Off in `SetPreview::default()` (so `ui::draw`'s test state keeps the existing layout); on from `new`.
    pub enabled: bool,
    pub store_path: Option<PathBuf>,
    /// What the CS locate searches.
    pub target: Option<ScanTarget>,
    pub fp: Option<SetFingerprint>,
    pub candidates: Candidates,
    /// The mesh per set key, this session only (spec §8).
    pub choice: HashMap<String, MeshChoice>,
    pub rows: Vec<ShapeRow>,
    pub notices: Vec<String>,
    pub timings: Timings,
    /// Set by the panel each frame it draws; `poll` does nothing heavy otherwise.
    pub shown: bool,
    /// Set by the panel on a frame with input: the fingerprint is checked now instead of after `THROTTLE`.
    pub input: bool,
    default: Value,
    entry: Option<Value>,
    nif: Option<Arc<LoadedNif>>,
    failed: HashSet<i64>,
    on_screen: Option<Shown>,
    sent: BTreeMap<ShapeId, MaterialState>,
    want_scene: bool,
    querying: Option<u64>,
    loading: Option<(u64, i64)>,
    last_check: Option<Instant>,
    located: bool,
    next: u64,
    store: RefCell<Option<Store>>,
    store_tried: Cell<bool>,
    vfs_cache: RefCell<HashMap<String, Option<FileSource>>>,
    tx: Sender<SpMsg>,
    rx: Receiver<SpMsg>,
}

impl Default for SetPreview {
    /// No IO and no threads: no store path, no saved shaders, disabled.
    fn default() -> SetPreview {
        let mut s = SetPreview::new(None, None, None, DEFAULT_CAP);
        s.enabled = false;
        s
    }
}

impl SetPreview {
    /// `shaders`: `[render3d] shaders` from `gui.toml`; `store_path`: the scan DB. Opens nothing until shown.
    pub fn new(
        shaders: Option<render3d::locate::Fingerprint>,
        target: Option<ScanTarget>,
        store_path: Option<PathBuf>,
        cap: u32,
    ) -> SetPreview {
        let (tx, rx) = channel();
        SetPreview {
            v3: Viewport3d::new(shaders),
            enc: Encoder::new(cap),
            enabled: true,
            store_path,
            target,
            fp: None,
            candidates: Candidates::NotAsked,
            choice: HashMap::new(),
            rows: vec![],
            notices: vec![],
            timings: Timings::default(),
            shown: false,
            input: false,
            default: document(&[]).to_value()["default"].clone(),
            entry: None,
            nif: None,
            failed: HashSet::new(),
            on_screen: None,
            sent: BTreeMap::new(),
            want_scene: false,
            querying: None,
            loading: None,
            last_check: None,
            located: false,
            next: 0,
            store: RefCell::new(None),
            store_tried: Cell::new(false),
            vfs_cache: RefCell::new(HashMap::new()),
            tx,
            rx,
        }
    }

    /// Encodes, queries, NIF reads, the viewport or a scene to send are in flight.
    pub fn busy(&self) -> bool {
        self.v3.busy()
            || self.enc.busy()
            || self.querying.is_some()
            || self.loading.is_some()
            || (self.want_scene && !self.enc.paused)
    }

    /// Window focus: look for CS again (as `MeshesModel::relocate`), once the panel has been shown.
    pub fn relocate(&mut self) {
        if self.located {
            self.v3.locate(self.target.clone());
        }
    }

    pub fn poll(&mut self, m: &AppModel) {
        self.poll_at(m, Instant::now())
    }

    /// `poll` at a given time (tests drive the debounce and the throttle). Call once per frame.
    pub fn poll_at(&mut self, m: &AppModel, now: Instant) {
        self.v3.poll();
        if self.enc.poll() {
            self.want_scene = true;
        }
        self.drain();
        let shown = std::mem::take(&mut self.shown);
        let input = std::mem::take(&mut self.input);
        if !shown || !self.enabled {
            return;
        }
        if !self.located {
            self.located = true;
            self.v3.locate(self.target.clone());
        }
        let key = followed_set(m);
        let due = input
            || key.as_ref() != self.fp.as_ref().map(|f| &f.set_key)
            || self
                .last_check
                .is_none_or(|t| now.duration_since(t) >= THROTTLE);
        if due {
            self.last_check = Some(now);
            if let Some(k) = key {
                self.check(m, &k, now);
            }
        }
        self.enc
            .pump(m.session.as_ref(), now, m.export.running.is_some());
        if self.want_scene && self.enc.settled() && self.querying.is_none() {
            self.push_scene();
        }
    }

    fn check(&mut self, m: &AppModel, key: &str, now: Instant) {
        let Some(fp) = fingerprint(m, key) else {
            return;
        };
        let d = classify(self.fp.as_ref(), &fp, &self.default);
        if d == Delta::default() {
            return;
        }
        if d.set {
            self.enc.reset();
            self.rows.clear();
            self.notices.clear();
            self.nif = None;
            self.failed.clear();
            self.sent.clear();
            self.candidates = Candidates::NotAsked;
            self.querying = None;
            self.loading = None;
            self.on_screen = None;
        }
        self.entry = fp.entry.as_ref().map(|e| entry_node(e).to_value());
        self.enc.sync(m, fp.members.clone(), now, d.set);
        self.fp = Some(fp);
        if d.set || d.requery {
            self.requery();
        }
        if d.set || d.encode || d.reload {
            self.want_scene = true;
        } else if d.material {
            self.apply_materials();
        }
    }

    fn requery(&mut self) {
        // A scan may have run since: reopen lazily, forget cached lookups.
        self.store_tried.set(false);
        *self.store.borrow_mut() = None;
        self.vfs_cache.borrow_mut().clear();
        self.nif = None;
        self.failed.clear();
        self.querying = None;
        let Some(entry) = self.entry.clone() else {
            self.candidates = Candidates::NotAsked;
            return;
        };
        let Some(path) = self.store_path.clone().filter(|p| p.is_file()) else {
            self.candidates = Candidates::NoStore(NO_SCAN.into());
            return;
        };
        let Some((key, filter)) = tex_key(&entry, &self.default) else {
            self.candidates = Candidates::Ready(vec![]);
            return;
        };
        self.next += 1;
        let token = self.next;
        self.querying = Some(token);
        self.candidates = Candidates::Querying;
        let prepared = pgpsim::prepare(&entry, &self.default);
        spawn(
            self.tx.clone(),
            move || {
                let t0 = Instant::now();
                let result = query(&path, &key, filter.as_deref(), &prepared).map(|c| (c, ms(t0)));
                SpMsg::Queried { token, result }
            },
            move |e| SpMsg::Queried {
                token,
                result: Err(format!("internal error in the mesh query: {e}")),
            },
        );
    }

    fn drain(&mut self) {
        while let Ok(msg) = self.rx.try_recv() {
            match msg {
                SpMsg::Queried { token, result } if self.querying == Some(token) => {
                    self.querying = None;
                    match result {
                        Ok((c, t)) => {
                            self.timings.query_ms = Some(t);
                            self.candidates = Candidates::Ready(c);
                        }
                        Err(e) => self.candidates = Candidates::Error(e),
                    }
                    self.want_scene = true;
                }
                SpMsg::NifLoaded { token, result } if self.loading.is_some_and(|(t, _)| t == token) => {
                    let (_, copy) = self.loading.take().expect("checked above");
                    match result {
                        Ok(n) => {
                            self.timings.nif_ms = Some(n.ms);
                            for e in n.parsed.errors.iter().chain(&n.parsed.warnings) {
                                self.notices.push(format!("{}: {e}", n.nif_path));
                            }
                            self.nif = Some(Arc::new(n));
                        }
                        Err(e) => {
                            self.failed.insert(copy);
                            self.notices.push(e);
                        }
                    }
                    self.want_scene = true;
                }
                _ => {}
            }
        }
    }

    /// The mesh drawn now: the set's choice when usable, else the best usable candidate, else the sphere.
    pub fn pick(&self) -> Pick {
        let Some(fp) = &self.fp else {
            return Pick::Builtin(Builtin::Sphere);
        };
        let choice = self
            .choice
            .get(&fp.set_key)
            .copied()
            .unwrap_or(MeshChoice::Auto);
        let list: &[Candidate] = match (&self.candidates, fp.entry.is_some()) {
            (Candidates::Ready(v), true) => v,
            _ => &[],
        };
        let usable = |c: &&Candidate| !c.le_mesh && !self.failed.contains(&c.load_copy_id);
        let best = || list.iter().find(usable);
        let found = match choice {
            MeshChoice::Builtin(b) => return Pick::Builtin(b),
            MeshChoice::Nif(id) => list
                .iter()
                .filter(usable)
                .find(|c| c.load_copy_id == id)
                .or_else(best),
            MeshChoice::Auto => best(),
        };
        found.map_or(Pick::Builtin(Builtin::Sphere), |c| Pick::Nif(c.clone()))
    }

    pub fn choose(&mut self, c: MeshChoice) {
        if let Some(fp) = &self.fp {
            self.choice.insert(fp.set_key.clone(), c);
            self.want_scene = true;
        }
    }

    /// The game's copy of `norm` (UI-thread read-only connection, opened on first use; lookups cached).
    fn vfs_lookup(&self, norm: &str) -> Option<FileSource> {
        if let Some(hit) = self.vfs_cache.borrow().get(norm) {
            return hit.clone();
        }
        if !self.store_tried.get() {
            self.store_tried.set(true);
            if let Some(p) = self.store_path.as_ref().filter(|p| p.is_file()) {
                *self.store.borrow_mut() = Store::open_read_only(p).ok();
            }
        }
        let found = self
            .store
            .borrow()
            .as_ref()
            .and_then(|s| s.texture_location(norm).ok().flatten())
            .map(|(k, r)| FileSource::from_store(k, &r, norm));
        self.vfs_cache
            .borrow_mut()
            .insert(norm.to_string(), found.clone());
        found
    }

    /// PGPatcher's existence check (01 §1.4, TP:348-375). pgpsim calls it for every non-empty slot after its slot
    /// pass, including untouched vanilla slots, `slotN` paths and the cubemap, in PGPatcher form (backslashes, lower
    /// case). True when sdc produces the path (a member that can be encoded) or the game has a winning copy of it,
    /// loose or in a BSA (`Store::texture_location` over `tex_winner`, which holds BSA origins too). Without this,
    /// a shape that keeps a vanilla slot would come back as `Skip`.
    pub fn exists(&self, pgp_path: &str) -> bool {
        let n = canon(pgp_path);
        self.enc.produces(&n) || self.vfs_lookup(&n).is_some()
    }

    /// Adds the game's copy of every slot texture of `materials` that `map` lacks (a path PGPatcher wrote that the
    /// source NIF did not name: another mod's `_rmaos`, a `slotN`).
    fn add_vfs(&self, map: &mut HashMap<String, FileSource>, materials: &[&MaterialState]) {
        for t in materials.iter().flat_map(|m| m.slots.iter()) {
            if t.norm.is_empty() || map.contains_key(&t.norm) {
                continue;
            }
            if let Some(src) = self.vfs_lookup(&t.norm) {
                map.insert(t.norm.clone(), src);
            }
        }
    }

    /// The built-in shape's material and row: the entry simulated on a matching stand-in, or the plain textures.
    fn builtin_scene(&mut self) -> (MaterialState, ShapeRow) {
        let name = "Built-in shape".to_string();
        if let Some(entry) = self.entry.clone() {
            if let Some((src, nif)) = builtin_source(&entry, &self.default) {
                let mut rows = simulate_shapes(
                    &entry,
                    &self.default,
                    &nif,
                    &[(ShapeId(0), name, src)],
                    &|p| self.exists(p),
                );
                let row = rows.remove(0);
                return (row.material.clone(), row);
            }
            if !self.notices.iter().any(|n| n == NO_BUILTIN_MATCH) {
                self.notices.push(NO_BUILTIN_MATCH.into());
            }
        }
        let members = self.fp.as_ref().map(|f| f.members.clone()).unwrap_or_default();
        let m = plain_material(&members);
        (
            m.clone(),
            ShapeRow {
                id: ShapeId(0),
                name,
                outcome: Outcome::Plain,
                material: m,
            },
        )
    }

    fn nif_rows(&self, nif: &LoadedNif, entry: &Value) -> Vec<ShapeRow> {
        let shapes: Vec<(ShapeId, String, MaterialState)> = nif
            .parsed
            .shapes
            .iter()
            .map(|s| (s.id, s.name.clone(), s.material.clone()))
            .collect();
        simulate_shapes(entry, &self.default, &nif.nif_path, &shapes, &|p| self.exists(p))
    }

    /// Builds the scene and hands it to the viewport: every shape simulated, its material written into the parsed
    /// NIF before `LoadParsed` (no first-load edit burst), textures = the game's copies overlaid by the set's
    /// encodes. A NIF not read yet is read first (off the UI thread); the push repeats when it arrives.
    fn push_scene(&mut self) {
        let t0 = Instant::now();
        self.want_scene = false;
        let memory = self.enc.memory_sources();
        match self.pick() {
            Pick::Builtin(b) => {
                let (material, row) = self.builtin_scene();
                let mut textures = memory;
                self.add_vfs(&mut textures, &[&material]);
                let same = self.on_screen == Some(Shown::Builtin(b));
                self.next += 1;
                self.v3.set_content(
                    Some(Content::Builtin {
                        key: self.next,
                        shape: b,
                        material: material.clone(),
                        textures: Arc::new(textures),
                    }),
                    same,
                );
                self.on_screen = Some(Shown::Builtin(b));
                self.sent = BTreeMap::from([(ShapeId(0), material)]);
                self.rows = vec![row];
            }
            Pick::Nif(c) => {
                let Some(nif) = self
                    .nif
                    .clone()
                    .filter(|n| n.load_copy_id == c.load_copy_id)
                else {
                    self.start_nif(&c);
                    return;
                };
                let Some(entry) = self.entry.clone() else {
                    return;
                };
                let rows = self.nif_rows(&nif, &entry);
                let mut parsed = nif.parsed.clone();
                for (s, r) in parsed.shapes.iter_mut().zip(&rows) {
                    s.material = r.material.clone();
                }
                let mut textures = nif.vfs.clone();
                self.add_vfs(&mut textures, &rows.iter().map(|r| &r.material).collect::<Vec<_>>());
                textures.extend(memory);
                let same = self.on_screen == Some(Shown::Nif(c.load_copy_id));
                self.next += 1;
                self.v3.set_content(
                    Some(Content::Parsed {
                        key: self.next,
                        nif: Arc::new(parsed),
                        textures: Arc::new(textures),
                    }),
                    same,
                );
                self.on_screen = Some(Shown::Nif(c.load_copy_id));
                self.sent = rows.iter().map(|r| (r.id, r.material.clone())).collect();
                self.rows = rows;
            }
        }
        self.timings.push_ms = Some(ms(t0));
    }

    fn start_nif(&mut self, c: &Candidate) {
        if self.loading.is_some_and(|(_, id)| id == c.load_copy_id) {
            return;
        }
        let Some(path) = self.store_path.clone() else {
            return;
        };
        self.next += 1;
        let token = self.next;
        self.loading = Some((token, c.load_copy_id));
        let c = c.clone();
        spawn(
            self.tx.clone(),
            move || SpMsg::NifLoaded {
                token,
                result: load_nif(&path, &c),
            },
            move |e| SpMsg::NifLoaded {
                token,
                result: Err(format!("internal error reading the NIF: {e}")),
            },
        );
    }

    /// A JSON-only change (spec §2.3): simulate again on the stored source materials and send `SetMaterial` for
    /// the shapes whose material changed; a full push when the viewport has not sent a load of this scene yet.
    fn apply_materials(&mut self) {
        let Some(entry) = self.entry.clone() else {
            return;
        };
        let rows = match self.on_screen {
            Some(Shown::Builtin(_)) => vec![self.builtin_scene().1],
            Some(Shown::Nif(id)) => match self.nif.clone().filter(|n| n.load_copy_id == id) {
                Some(nif) => self.nif_rows(&nif, &entry),
                None => {
                    self.want_scene = true;
                    return;
                }
            },
            None => {
                self.want_scene = true;
                return;
            }
        };
        for r in &rows {
            if self.sent.get(&r.id) == Some(&r.material) {
                continue;
            }
            if self.v3.set_material(r.id, r.material.clone()) {
                self.sent.insert(r.id, r.material.clone());
            } else {
                self.want_scene = true;
            }
        }
        self.rows = rows;
    }
}
```

- [ ] **Step 5: Run the tests and confirm they pass**

Run: `cargo test -p sdc-gui --test set_preview_model`
Expected: 17 passed.

- [ ] **Step 6: Gates**

Run: `cargo fmt --all && cargo clippy --workspace --all-targets -- -D warnings && cargo test --workspace`
Expected: clean.

- [ ] **Step 7: Commit**

```bash
git add crates/gui/src/model/set_preview.rs crates/gui/tests/set_preview_model.rs
git commit -m "gui: set preview model: fingerprint, change classes, mesh candidates, PGPatcher simulation" -m "Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

---

### Task 15: gui `ui/set_preview.rs` panel + kittests

**Files:**
- Create: `crates/gui/src/ui/set_preview.rs`
- Modify (shared hook, own commit): `crates/gui/src/ui/mod.rs`: insert `pub mod set_preview;` after line 10 (`pub mod set_editor;`)
- Test: `crates/gui/tests/set_preview_ui.rs` (new)

**Interfaces:**
- Consumes: everything Task 14 produces; `ui::viewport3d::show(ui, &mut Viewport3d)` (`ui/viewport3d.rs:23`); `SlotKind::label` (`sets.rs:70`).
- Produces: `pub fn draw(ui: &mut egui::Ui, m: &mut AppModel, s: &mut SetPreview)` (contract), `pub fn wanted(m: &AppModel) -> bool`.

- [ ] **Step 1: Write the failing kittests** (`crates/gui/tests/set_preview_ui.rs`)

```rust
//! The set preview panel (P1-A Task 15): its states without a GPU (no render thread starts, the shaders are never
//! confirmed): no scan DB, plain textures (D3), no matching mesh → built-in, the Skip badge, the JSON line.
mod support;
use egui_kittest::kittest::Queryable;
use egui_kittest::Harness;
use nif::material::MaterialState;
use render3d::builtin::Builtin;
use render3d::ShapeId;
use sdc_gui::model::pbr_export::json_entries;
use sdc_gui::model::set_preview::*;
use sdc_gui::model::viewport3d::Content;
use sdc_gui::model::AppModel;
use std::time::{Duration, Instant};
use support::*;

type H = Harness<'static, (AppModel, SetPreview)>;

fn harness(m: AppModel, s: SetPreview) -> H {
    let h = Harness::builder()
        .with_size(eframe::egui::vec2(520.0, 1000.0))
        .build_ui_state(
            |ui, (m, s): &mut (AppModel, SetPreview)| sdc_gui::ui::set_preview::draw(ui, m, s),
            (m, s),
        );
    sdc_gui::fonts::install_system_fonts(&h.ctx).expect("system fonts");
    h
}

/// Draw and poll until nothing is in flight (what the app does every frame).
fn settle(h: &mut H) {
    let t = Instant::now();
    loop {
        h.run();
        {
            let (m, s) = h.state_mut();
            s.poll(&*m);
        }
        if !h.state().1.busy() {
            h.run();
            return;
        }
        assert!(t.elapsed() < Duration::from_secs(90), "the panel did not settle");
        std::thread::sleep(Duration::from_millis(5));
    }
}

fn rock_selected(d: &std::path::Path) -> AppModel {
    let mut m = pbr_opened(d);
    let key = json_entries(&m)
        .into_iter()
        .find(|(v, _)| v.id == "landscape/rock01")
        .unwrap()
        .0
        .key;
    m.select_set(&key);
    m
}

#[test]
fn without_a_scan_db_the_panel_says_so_and_shows_the_simulated_sphere() {
    let d = tempfile::tempdir().unwrap();
    let mut h = harness(rock_selected(d.path()), SetPreview::new(None, None, None, DEFAULT_CAP));
    settle(&mut h);
    h.get_by_label(PANEL_TITLE);
    h.get_by_label(NO_SCAN);
    h.get_by_label(JSON_WINS);
    h.get_by_label(&cap_text(2048));
    h.get_by_label_contains("Diffuse: fresh");
    h.get_by_label("Built-in shape: patched as PGPatcher would");
    h.get_by_label_contains("Off in this preview");
    for (_, name) in BUILTINS {
        h.get_by_label(name);
    }
    assert!(h.query_by_label(NEEDS_CS).is_none());
}

#[test]
fn a_set_the_simulation_cannot_preview_shows_plain_textures_and_the_notice() {
    let d = tempfile::tempdir().unwrap();
    let mut m = rock_selected(d.path());
    m.set_target(engine::config::Target::Vanilla);
    assert!(m.wait_idle(Duration::from_secs(60)));
    let mut h = harness(m, SetPreview::new(None, None, None, DEFAULT_CAP));
    settle(&mut h);
    h.get_by_label(NEEDS_CS);
    assert!(h.query_by_label(JSON_WINS).is_none());
    h.get_by_label("Built-in shape: plain textures, no PGPatcher simulation");
    let content = h.state().1.v3.content().cloned();
    assert!(
        matches!(&content, Some(Content::Builtin { material, .. }) if !material.is_pbr()),
        "{content:?}"
    );
}

#[test]
fn no_matching_mesh_falls_back_to_a_built_in_shape_the_user_can_change() {
    let d = tempfile::tempdir().unwrap();
    let db = d.path().join("scan.db");
    drop(modscan::store::Store::open(&db).unwrap()); // an empty v3 database
    let mut h = harness(
        rock_selected(d.path()),
        SetPreview::new(None, None, Some(db), DEFAULT_CAP),
    );
    settle(&mut h);
    h.get_by_label(NO_MATCH);
    assert!(matches!(
        h.state().1.v3.content(),
        Some(Content::Builtin { shape: Builtin::Sphere, .. })
    ));
    h.get_by_label("Cube").click();
    settle(&mut h);
    assert!(matches!(
        h.state().1.v3.content(),
        Some(Content::Builtin { shape: Builtin::Cube, .. })
    ));
}

#[test]
fn a_skipped_shape_gets_the_red_line_and_patched_shapes_list_their_badges() {
    let d = tempfile::tempdir().unwrap();
    let mut h = harness(rock_selected(d.path()), SetPreview::new(None, None, None, DEFAULT_CAP));
    settle(&mut h);
    h.state_mut().1.rows = vec![
        ShapeRow {
            id: ShapeId(4),
            name: "Bottle".into(),
            outcome: Outcome::Skip(r"textures\pbr\xmead\reserve_rmaos.dds missing".into()),
            material: MaterialState::builtin_default(),
        },
        ShapeRow {
            id: ShapeId(5),
            name: "Label".into(),
            outcome: Outcome::Patched { badges: vec!["smooth_angle"] },
            material: MaterialState::builtin_default(),
        },
    ];
    h.run();
    h.get_by_label(r"Bottle: will be skipped: textures\pbr\xmead\reserve_rmaos.dds missing");
    h.get_by_label("Label: patched as PGPatcher would");
    h.get_by_label(&badge_text("smooth_angle"));
}

#[test]
fn nothing_but_a_hint_without_a_followed_set() {
    let d = tempfile::tempdir().unwrap();
    let mut m = pbr_opened(d.path());
    m.selection = sdc_gui::model::Selection::None;
    let mut h = harness(m, SetPreview::new(None, None, None, DEFAULT_CAP));
    h.run();
    h.get_by_label("Select a set to preview it in 3D.");
    assert!(!sdc_gui::ui::set_preview::wanted(&h.state().0));
}
```

- [ ] **Step 2: Run the tests and confirm they fail**

Run: `cargo test -p sdc-gui --test set_preview_ui`
Expected: compile error: `could not find set_preview in ui`.

- [ ] **Step 3: Implement** `crates/gui/src/ui/set_preview.rs`

```rust
//! The set preview panel (P1-A, spec §2.3): the right side of the Textures view while a set, or a texture of a set,
//! is selected. Draws `SetPreview`; the frame, the CS toggle and the problem lists are the shared
//! `ui::viewport3d::show`. No logic beyond mapping input to model calls.
use crate::model::set_preview::{
    badge_text, builtin_name, cap_text, count_text, followed_set, Candidates, EncState, MeshChoice, Outcome, Pick,
    SetPreview, BUILTINS, JSON_WINS, NEEDS_CS, NO_MATCH, OFF_FEATURES, PANEL_TITLE, WAITS_FOR_EXPORT,
};
use crate::model::AppModel;
use eframe::egui;

const AMBER: egui::Color32 = egui::Color32::from_rgb(230, 160, 30);
const RED: egui::Color32 = egui::Color32::from_rgb(210, 60, 60);
/// Height of the shape list before the frame.
const LIST_MAX: f32 = 160.0;

/// The panel is wanted while a set, or a texture of a set, is selected (it stays beside the expanded view).
pub fn wanted(m: &AppModel) -> bool {
    followed_set(m).is_some()
}

pub fn draw(ui: &mut egui::Ui, m: &mut AppModel, s: &mut SetPreview) {
    s.shown = true;
    if ui
        .ctx()
        .input(|i| !i.events.is_empty() || i.pointer.any_down())
    {
        s.input = true;
    }
    ui.heading(PANEL_TITLE);
    if followed_set(m).is_none() {
        ui.label("Select a set to preview it in 3D.");
        return;
    }
    let Some(fp) = s.fp.clone() else {
        ui.label("Preparing the preview…");
        return;
    };
    ui.label(egui::RichText::new(&fp.label).strong());
    if fp.entry.is_some() {
        ui.label(JSON_WINS);
    } else {
        ui.colored_label(AMBER, NEEDS_CS);
    }
    ui.label(egui::RichText::new(cap_text(s.enc.cap)).small());
    if s.enc.paused {
        ui.colored_label(AMBER, WAITS_FOR_EXPORT);
    }
    mesh_picker(ui, s, fp.entry.is_some());
    members(ui, s);
    shapes(ui, s);
    for n in &s.notices {
        ui.colored_label(AMBER, n);
    }
    if let Some(e) = &s.enc.error {
        ui.colored_label(RED, e);
    }
    ui.label(egui::RichText::new(OFF_FEATURES).small());
    timings(ui, s);
    ui.separator();
    crate::ui::viewport3d::show(ui, &mut s.v3);
    if s.busy() {
        ui.ctx()
            .request_repaint_after(std::time::Duration::from_millis(50));
    }
}

fn mesh_picker(ui: &mut egui::Ui, s: &mut SetPreview, simulated: bool) {
    let pick = s.pick();
    let mut chosen = None;
    ui.horizontal_wrapped(|ui| {
        ui.label("Mesh:");
        if let Candidates::Ready(list) = &s.candidates {
            if !list.is_empty() {
                let current = match &pick {
                    Pick::Nif(c) => c.label(),
                    Pick::Builtin(b) => builtin_name(*b).to_string(),
                };
                egui::ComboBox::from_id_salt("set-3d-mesh")
                    .selected_text(current)
                    .show_ui(ui, |ui| {
                        for c in list {
                            let sel = matches!(&pick, Pick::Nif(p) if p.load_copy_id == c.load_copy_id);
                            let r = ui
                                .add_enabled_ui(!c.le_mesh, |ui| ui.selectable_label(sel, c.label()))
                                .inner;
                            if r.clicked() {
                                chosen = Some(MeshChoice::Nif(c.load_copy_id));
                            }
                        }
                    });
            }
        }
        for (b, name) in BUILTINS {
            let sel = matches!(pick, Pick::Builtin(x) if x == b);
            if ui.selectable_label(sel, name).clicked() {
                chosen = Some(MeshChoice::Builtin(b));
            }
        }
    });
    if let Some(c) = chosen {
        s.choose(c);
    }
    match &s.candidates {
        Candidates::NoStore(t) => {
            ui.label(t);
        }
        Candidates::Querying => {
            ui.label("Looking for meshes that use this diffuse…");
        }
        Candidates::Ready(v) if v.is_empty() && simulated => {
            ui.label(NO_MATCH);
        }
        Candidates::Ready(v) if !v.is_empty() => {
            ui.label(count_text(v.len()));
        }
        Candidates::Error(e) => {
            ui.colored_label(RED, e);
        }
        _ => {}
    }
}

fn members(ui: &mut egui::Ui, s: &SetPreview) {
    ui.horizontal_wrapped(|ui| {
        for m in s.enc.members.values() {
            let text = format!("{}: {}", m.slot.kind.label(), m.state.label());
            match &m.state {
                EncState::Failed(_) => ui.colored_label(RED, text),
                EncState::Stale(_) | EncState::Blocked(_) => ui.colored_label(AMBER, text),
                _ => ui.label(text),
            };
        }
    });
}

fn shapes(ui: &mut egui::Ui, s: &SetPreview) {
    if s.rows.is_empty() {
        return;
    }
    egui::ScrollArea::vertical()
        .id_salt("set-3d-shapes")
        .max_height(LIST_MAX)
        .show(ui, |ui| {
            for r in &s.rows {
                match &r.outcome {
                    Outcome::Skip(_) => {
                        ui.colored_label(RED, r.text());
                    }
                    Outcome::Patched { badges } => {
                        ui.label(r.text());
                        for b in badges {
                            ui.colored_label(AMBER, badge_text(b));
                        }
                    }
                    Outcome::NoMatch | Outcome::Plain => {
                        ui.label(r.text());
                    }
                }
            }
        });
}

fn timings(ui: &mut egui::Ui, s: &SetPreview) {
    egui::CollapsingHeader::new("Timings")
        .id_salt("set-3d-timings")
        .show(ui, |ui| {
            let f = |x: Option<f64>| x.map_or("–".to_string(), |v| format!("{v:.1} ms"));
            ui.label(format!("Mesh query: {}", f(s.timings.query_ms)));
            ui.label(format!("NIF read + parse: {}", f(s.timings.nif_ms)));
            ui.label(format!("Simulate + scene: {}", f(s.timings.push_ms)));
            for m in s.enc.members.values() {
                if let Some(e) = m.state.shown() {
                    ui.label(format!("Encode {}: {:.0} ms", m.slot.kind.label(), e.ms));
                }
            }
            if let Some((k, t)) = s.v3.last_edit {
                ui.label(format!("Last material edit: {k:?}, {t:.2} ms"));
            }
        });
}
```

- [ ] **Step 4: Shared hook, own commit** (`crates/gui/src/ui/mod.rs`, insert after line 10 `pub mod set_editor;`):
```rust
pub mod set_preview;
```
Gates: `cargo fmt --all && cargo clippy --workspace --all-targets -- -D warnings && cargo test --workspace`. Expected: clean.
```bash
git add crates/gui/src/ui/mod.rs
git commit -m "gui: declare ui::set_preview (shared hook, one line)" -m "Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

- [ ] **Step 5: Run the tests and confirm they pass**

Run: `cargo test -p sdc-gui --test set_preview_ui`
Expected: 5 passed.

- [ ] **Step 6: Gates**

Run: `cargo fmt --all && cargo clippy --workspace --all-targets -- -D warnings && cargo test --workspace`
Expected: clean.

- [ ] **Step 7: Commit**

```bash
git add crates/gui/src/ui/set_preview.rs crates/gui/tests/set_preview_ui.rs
git commit -m "gui: set preview panel: mesh picker, encode chips, PGPatcher shape list, notices" -m "Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

---

### Task 16: shared hooks: `settings.rs`, `ui/mod.rs`, `app.rs`

**Files:**
- Modify (shared): `crates/gui/src/settings.rs:79-84` (`Render3dSettings`: one field)
- Modify: `crates/gui/src/model/set_preview.rs` (append `from_settings`)
- Modify (shared): `crates/gui/src/ui/mod.rs`: one `State` field after line 34; one panel block before line 73
- Modify (shared): `crates/gui/src/app.rs`: lines 39-54 (`new`), 143-146 (`focus_regained`), 203, 222, 225
- Test: `crates/gui/tests/set_preview_ui.rs` (append)

**Interfaces:**
- Consumes: Task 14/15 items; `GuiSettings` (`settings.rs:56`); `meshes::default_store_path` (`meshes.rs:178`); `Viewport3d::take_shaders_changed` (`viewport3d.rs:311`); `AppModel::save_settings` (`model/mod.rs:280`).
- Produces: `Render3dSettings::set_preview_max: u32` (0 = 2048); `SetPreview::from_settings(&GuiSettings) -> SetPreview`; `ui::State::set3d: SetPreview`; the `Panel::right("set-3d")` in `draw_with`; the app polls, persists, repaints and relocates it.

- [ ] **Step 1: Write the failing tests** (append to `crates/gui/tests/set_preview_ui.rs`)

```rust
// ---- Task 16: hooks ----

#[test]
fn the_cap_setting_defaults_to_2048_and_round_trips() {
    let d = tempfile::tempdir().unwrap();
    let mut s = sdc_gui::settings::GuiSettings::default();
    assert_eq!(s.render3d.set_preview_max, 0);
    assert_eq!(SetPreview::from_settings(&s).enc.cap, DEFAULT_CAP);
    s.render3d.set_preview_max = 1024;
    assert_eq!(SetPreview::from_settings(&s).enc.cap, 1024);
    assert!(SetPreview::from_settings(&s).enabled);
    let p = d.path().join("gui.toml");
    s.save(&p).unwrap();
    assert_eq!(sdc_gui::settings::GuiSettings::load(&p), (s, None));
}

#[test]
fn the_textures_view_shows_the_panel_for_a_set_only_when_the_app_enables_it() {
    use sdc_gui::ui::State;
    let d = tempfile::tempdir().unwrap();
    let m = rock_selected(d.path());
    let state = State {
        set3d: SetPreview::new(None, None, None, DEFAULT_CAP),
        ..State::default()
    };
    let mut h = Harness::builder()
        .with_size(eframe::egui::vec2(1800.0, 900.0))
        .build_ui_state(
            |ui, (m, s): &mut (AppModel, State)| sdc_gui::ui::draw_with(ui, m, s),
            (m, state),
        );
    sdc_gui::fonts::install_system_fonts(&h.ctx).expect("system fonts");
    h.run();
    h.get_by_label(PANEL_TITLE);
    h.state_mut().0.selection = sdc_gui::model::Selection::None;
    h.run();
    assert!(h.query_by_label(PANEL_TITLE).is_none());
    // `ui::draw` (every existing kittest) uses `State::default()`: the panel stays off.
    let mut h = Harness::builder()
        .with_size(eframe::egui::vec2(1400.0, 900.0))
        .build_ui_state(
            |ui, m: &mut AppModel| sdc_gui::ui::draw(ui, m),
            rock_selected(d.path()),
        );
    sdc_gui::fonts::install_system_fonts(&h.ctx).expect("system fonts");
    h.run();
    assert!(h.query_by_label(PANEL_TITLE).is_none());
}
```

- [ ] **Step 2: Run the tests and confirm they fail**

Run: `cargo test -p sdc-gui --test set_preview_ui`
Expected: compile errors: `no field set_preview_max`, `no function from_settings`, `struct State has no field set3d`.

- [ ] **Step 3: Implement the setting** (`crates/gui/src/settings.rs`, inside `Render3dSettings` after line 83):
```rust
    /// The set preview's encode cap in pixels (D5); 0 = 2048. Edited in `gui.toml` for now.
    pub set_preview_max: u32,
```
Append to `crates/gui/src/model/set_preview.rs`:
```rust
impl SetPreview {
    /// The app's instance: the saved shaders, the `[meshes]` target, the scan DB path and the D5 cap. No IO.
    pub fn from_settings(s: &crate::settings::GuiSettings) -> SetPreview {
        let cap = match s.render3d.set_preview_max {
            0 => DEFAULT_CAP,
            n => n.clamp(256, 8192),
        };
        SetPreview::new(
            s.render3d.shaders.clone(),
            target_of(&s.meshes),
            crate::model::meshes::default_store_path(),
            cap,
        )
    }
}
```
Run: `cargo test -p sdc-gui --test set_preview_ui the_cap_setting`. Expected: pass. Gates (`cargo fmt --all && cargo clippy --workspace --all-targets -- -D warnings && cargo test --workspace`; the other new test still fails to compile, so run this commit's gates with that test temporarily left out of the file, or add the test in Step 6), then:
```bash
git add crates/gui/src/settings.rs crates/gui/src/model/set_preview.rs
git commit -m "gui: [render3d] set_preview_max setting (D5 cap, add-only)" -m "Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

- [ ] **Step 4: Implement the UI hook** (`crates/gui/src/ui/mod.rs`)

In `State`, after line 34 (`pub gpu: preview::GpuStatus,`):
```rust
    /// The set preview (P1-A). `Default` is disabled and does no IO; `SdcApp::new` builds the enabled one.
    pub set3d: crate::model::set_preview::SetPreview,
```
In `draw_with`, insert before line 73 (`egui::CentralPanel::default().show(…)`):
```rust
    if state.set3d.enabled && set_preview::wanted(model) {
        egui::Panel::right("set-3d")
            .default_size(380.0)
            .show(ui, |ui| set_preview::draw(ui, model, &mut state.set3d));
    }
```

- [ ] **Step 5: Implement the app hooks** (`crates/gui/src/app.rs`)

(a) In `SdcApp::new`, insert before line 46 (`SdcApp {`):
```rust
        let set3d = crate::model::set_preview::SetPreview::from_settings(&model.settings);
```
and replace line 49 (`state: crate::ui::State::default(),`) with:
```rust
            state: crate::ui::State {
                set3d,
                ..crate::ui::State::default()
            },
```
(b) In `focus_regained`, after line 145 (`self.meshes.relocate();`):
```rust
        self.state.set3d.relocate();
```
(c) After line 203 (`self.meshes.poll();`):
```rust
        self.state.set3d.poll(&self.model);
```
(d) After line 222 (the `}` that closes the `if meshes_view_active … else …`):
```rust
        if let Some(fp) = self.state.set3d.v3.take_shaders_changed() {
            self.model.settings.render3d.shaders = Some(fp);
            self.model.save_settings();
        }
```
(e) Line 225: append `|| self.state.set3d.busy()` to the repaint condition:
```rust
        if self.model.busy()
            || self.meshes.scanning()
            || self.meshes.v3.busy()
            || self.state.set3d.busy()
        {
```

- [ ] **Step 6: Run the tests and confirm they pass**

Run: `cargo test -p sdc-gui --test set_preview_ui && cargo test -p sdc-gui --test set_editor_ui && cargo test -p sdc-gui --test ui_smoke`
Expected: all pass; the existing kittests are unchanged (the panel is off in `TEST_STATE`).

- [ ] **Step 7: Gates**

Run: `cargo fmt --all && cargo clippy --workspace --all-targets -- -D warnings && cargo test --workspace`
Expected: clean, golden and `gui_repro` green.

- [ ] **Step 8: Commit (the shared-hook commit)**

```bash
git add crates/gui/src/ui/mod.rs crates/gui/src/app.rs crates/gui/tests/set_preview_ui.rs
git commit -m "gui: hook the set preview into the Textures view and the app loop (shared files, add-only)" -m "Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

---

### Task 17: end-to-end verification on owner data (manual checklist + recorded timings)

**Files:**
- Modify: this plan file, section "P1-A verification" at the end (record the results)
- No code. Owner data (`C:\_mount\moddingvol\Acherus`, CS install) is read-only: nothing is written there.

**Interfaces:** Consumes the whole P1-A build. Produces the recorded numbers and pass/fail per check.

- [ ] **Step 1: Prepare**
  - `cargo build -p sdc-gui --release` in `F:\ai\dds\converter-nif-p1`.
  - Start `target\release\sdc-gui.exe` from MO2 (the Acherus instance), so `[meshes]` and the CS locate resolve.
  - In the Meshes view, make sure a v3 scan of the Acherus profile exists (rescan if the database predates Task 11's build; the scan DB is sdc's own `%LOCALAPPDATA%\sdc\scan.db`).
  - Count the shader cache: `(Get-ChildItem "$env:LOCALAPPDATA\sdc\shader-cache" -Recurse -File).Count`. Record it as **C0**.
  - Open Task Manager → Details, add the "Dedicated GPU memory" and "Commit size" columns, and note both for `sdc-gui.exe` as **M0**.

- [ ] **Step 2: First demo (spec §1)**
  - Open the source folder of Xocco3's Mead Replacer PBR textures. In the Sets column, select the set whose diffuse is `reserve_bc` (01 §5.2 "plain" row). Confirm the target is CS.
  - Expect the "Set preview (3D)" panel on the right with the "Assumes your sdc JSON wins…" line and "Preview at 2048 px, export writes full size".
  - Expect the mesh line to list `meshes\xmead\reserve.nif` and pick it (not a built-in), with the count line "N meshes use this diffuse".
  - Expect the shape list to say "…: patched as PGPatcher would" for the matching shapes (the others "not matched, original textures") and no red "will be skipped" line.
  - Expect the frame to show the bottle with the converter's encodes.
  - Record **M1** (memory once the frame is up) and **C1** (cache count).

- [ ] **Step 3: Cbuffer edit, zero compiles (success criterion 3)**
  - In the set editor's PGPatcher fields, drag `roughness_scale` from 1.0 to 0.3 and back.
  - Expect the highlight to change on the next frame while dragging, with no frame blank.
  - Expect "Timings → Last material edit" to read `Cbuffer` with a time in ms.
  - Expect the cache count after the drag to equal C1, and every member chip to stay "fresh" with no "encoding…".
  - Repeat with an emissive colour or `specular_level`. Expect the same.

- [ ] **Step 4: Texture change (P1-A reload)**
  - Change a member's setting that re-encodes it (for example the normal map's format in its settings).
  - Expect its chip to go "stale", then "encoding…", then "fresh". Expect one reload with the camera kept and the last frame shown until the new one.

- [ ] **Step 5: Panel states on real data**
  - **Built-ins:** Sphere, Plane and Cube each draw the patched material. Switch to another set and back: the choice is remembered.
  - **Skip:** temporarily remove the RMAOS member from the set (undo afterwards). Expect "will be skipped: …_rmaos.dds missing" in red unless the game already has that file. The shape keeps its original look.
  - **D3:** switch the target to Vanilla. Expect the "PGPatcher preview needs the CS target and a PBR set" line and the plain textures on the sphere. Switch back.
  - **Export pause:** start an export. Expect "Preview encoding waits for the export to finish" during an edit that re-encodes.
  - **Scan while querying:** start a Meshes-view rescan, then select another set. Expect the panel to query without error (the read-only connection under WAL); at most a clear database error that recovers on the next selection. Expect no schema rebuild.
  - **LE:** if a candidate is an LE mesh, it is listed as "LE, not drawn" and cannot be picked.

- [ ] **Step 6: Record timings** (from the panel's "Timings" section and Task Manager) in the "P1-A verification" section:

| Measure | Value |
|---|---|
| Mesh query (`reserve_bc`, suffix key) | ms |
| Mesh query, a `path_contains` set (worst case, full `path` scan) | ms |
| NIF read + parse (`reserve.nif`) | ms |
| Simulate + scene | ms |
| Encode per member at 2048 px (diffuse, normal, RMAOS, …) | ms each |
| Last material edit (`Cbuffer`) | ms |
| Shader cache files C0 / C1 / after Step 3 | counts |
| Second device: M1 − M0 (dedicated GPU, commit) | MB |
| The same with the Meshes view's 3D panel also open | MB |

  If the second device costs more than about 150 MB, record it as the D2 tripwire for P2 (hoist one `Viewport3d`, 02 §4.3).

- [ ] **Step 7: Commit the record**

```bash
git add docs/superpowers/plans/<this plan file>.md
git commit -m "docs: P1-A verification results (reserve_bc demo, timings, second-device memory)" -m "Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

---

## Appendix: cross-part decisions from drafting

### Part R (nif::material, render3d)
#### CONTRACT ISSUES

None that change a name, type or signature. Readings taken where the contract or the research is silent (the stitcher may want them in the plan header):

1. **`pbr_flags` branch key.** 03 §1.3 writes "MLP(type 11) → TwoLayer"; 01 §3.1 (the cited CS source) keys the remap on **SLSF2.24**, not on the shader type. Part R follows 01 §3.1. The MLP *floats* are still read only for type 11 (01 §3.1), so a type-0 shape with SLSF2.24 gets TwoLayer with zero coat parameters.
2. **Subsurface is independent of Fuzz/Glint** in the last branch (01 §3.1 pseudocode: "Subsurface if SLSF2.26; Fuzz if SLSF2.25, else Glint if SLSF2.12"; 01 §3.4's exclusion list never excludes Subsurface from Fuzz/Glint). Pinned by a table row (`Rim+Soft → Subsurface|Fuzz`).
3. **`MaterialState::builtin_default` fields the contract does not list:** `uv_offset (0,0)`, `emissive_color (0,0,0)`, `emissive_multiple 1.0`, `texture_clamp_mode 3` (WRAP_S_WRAP_T, nifly default), `refraction_strength 0`.
4. **`set_material` on a compile failure returns `Ok(EditKind::Permutation)`** with `status = Failed(e)`; `Err` is only "unknown id". A change of `hidden` alone classifies as `Cbuffer`.
5. **`ParsedNif.skipped` vs `warnings`/`errors`:** `skipped` lists every scanned shape that is not in `shapes`, with its reason; the same text also goes into `warnings` (not a Lighting shape) or `errors` (geometry/`validate` failure), so `load_parsed` copies `warnings`/`errors` into the `LoadReport` unchanged and the P0 report lines survive.

#### Notes for the stitcher

- **Homes of contract types:** `EditKind`, `Bound`, `pbr_flags`, `classify` in `render3d::material`; `ShapeStatus`, `ShapeInfo` in `render3d` (lib.rs); `Builtin`, `builtin_mesh` in `render3d::builtin`. Crate-root re-exports: `render3d::{parse, ParsedNif, ParsedShape, ShapeId, Bound, pbr_flags, classify, EditKind, ShapeStatus, ShapeInfo, Builtin, builtin_mesh}`.
- **New pub items beyond the contract:** `nif::material::{SLSF1_SPECULAR, SLSF1_RECEIVE_SHADOWS, SLSF1_CAST_SHADOWS, SLSF2_ZBUFFER_WRITE, SLSF2_TRUE_PBR}`; `render3d::material::{perm_material, remapped_glint, write_material, pack_specular, pbr_params, PBR_* flag consts}`; `CbWriter::{zero, resolve, set_at, set_u32_at}` and `cbuffer::Field`; `scene::{VIEW_NAMES, LIGHT_NAMES, ViewValues, LightValues, SceneFields, write_static}`; `scene::FEATURE_FLOATS/FEATURE_UINTS` become `pub`; `draw::FrameInputs`; `Renderer::{last_uploads, set_reference_writer (doc(hidden))}`; `builtin::BUILTIN_SIZE`.
- **Removed:** `draw::{MaterialInputs, write_material, pack_specular, pbr_params}` (moved to `material.rs`; nothing outside render3d used them, checked with grep). `Material::from_shape` stays (unused but pub).
- **Behaviour changes callers can see:** (a) `GLINT` under PBR now comes from the remapped Glint flag, not SLSF2.21 (spec §2.2) — `tests/compile.rs:77-83` is rewritten in Task 7. (b) `PBRFlags` is written (P0 never wrote it) and `PBRParams2`/`MultiLayerParallaxData` only in CS's branches. (c) PBR `EmitColor` per D1. (d) `LoadReport.errors` order: parse-time geometry errors now come before build errors (P0 interleaved them per shape); the set of lines is unchanged.
- **Textures in P1-A:** `Renderer` keeps resolved textures per load (cleared by every `load*`). `set_material` reuses them by path and asks the resolver again for paths that were missing. A re-encoded texture under an unchanged path is **not** picked up by `set_material`; part G must reload (`LoadParsed`/`LoadBuiltin`) until P1-B's hot-swap.
- **`load_builtin`** names the shape `"Sphere"` / `"Plane"` / `"Cube"` (`format!("{b:?}")`), id `ShapeId(0)`, and forces `skinned = false`, `has_vertex_colors = false`. The plane faces +Y (towards `Camera::Auto`).
- **Stand-in shader** (`crates/render3d/tests/fixtures/standin/Lighting.hlsl`) gains two feature hooks in Task 8b: `STANDIN_FLAT` (pixels = 0.25·diffuse + EmitColor + 0.5 green if `PBRFlags` ≠ 0) and `STANDIN_TBN` (pixels = N·L with the normal map in the vertex tangent frame). The `#error` without `DO_ALPHA_TEST`/`STANDIN_OPAQUE_OK` is kept, so `tests/standin.rs` is untouched.
- **Part G requirements (checked):** crate-root re-exports `ShapeInfo`, `ShapeStatus` (defined in lib.rs), `EditKind`, `ShapeId`, `ParsedNif`, `ParsedShape`, `parse`, `Builtin` (also `render3d::builtin::Builtin`); `Builtin` derives Copy, Clone, Debug, PartialEq, Eq, Hash; `ParsedNif` derives Debug, Clone, Default; every `ParsedShape` field is pub.
- **Part P requirement (checked):** `has_vertex_colors = true` on a mesh with no colour stream compiles `VC` and reads white: `mesh::stream_for` already fills `[255; 4]` (`mesh.rs:119-122`); Task 8b test `vertex_colours_without_a_colour_stream_read_white` pins it (stand-in VS reads `COLOR0` under `VC`).
- **Shared test helpers:** `crates/render3d/tests/common/mod.rs` (Task 8a) — `fixture`, `renderer`, `FLAT`, `channel`, `covered`, `cache_files`, `dds_rgba8`.
- **Tangent convention (measured, Task 9):** on `TestNifFile_Furniture_Col_SE.nif`, `Mesh.tangents` ∥ +dP/dv (mean cos 1.000), `Mesh.bitangents` ∥ +dP/du (1.000), cross terms 0.002 / 0.004, `cross(tangent, bitangent)·N` mean 0.941; every fixture triangle (136 + 2846) has `cross(p1−p0, p2−p0)·n > 0`. Measured by the writer of this part with a scratch program against the `nif` crate before writing; Task 9's test re-derives it.
- **Review-focus candidates for the header:** (1) a PBR edit that only flips Rim/Soft/Fit_Slope must stay a cbuffer edit except where it toggles `GLINT` (Task 8b tests both); (2) a compile failure must never blank the panel (Task 8b `failing_permutation_keeps_old_draw`); (3) hide → unhide restores the identical frame (Task 8b); (4) an unchanged frame uploads nothing, yet pixels equal the by-name writer after camera, light, edit and resize (Task 10).

Gates before every commit (contract): `cargo fmt --all && cargo clippy --workspace --all-targets -- -D warnings && cargo test --workspace`. All new render3d tests run on WARP with the repo-owned stand-in; none is `#[ignore]`.

---

### Part P (pgpsim)
#### CONTRACT ISSUES

1. **Clarification, not a rename: `SimInput.exists` is called on more paths than its doc comment says.** The contract says it is called with `textures\pbr\…\x_n.dds` paths. PGPatcher's §1.4 check (TP:348-375) tests **every non-empty slot after the slot pass**: the 9 slots, including untouched source slots (slot 8, a kept glow slot, locked slots), `slotN` overrides (e.g. `textures\black.dds`) and, for `pbr:false`, the unchanged source paths. pgpsim follows PGPatcher. So the GUI's `exists` (Tasks 13/14) must answer for vanilla/BSA-packed paths too (VFS winners, loose **and** BSA), or every shape that keeps a vanilla slot comes back `Skip("… missing")`. The doc comment in `lib.rs` below states the wider scope. `cubemap` is written verbatim (not lower-cased) into the slot; `exists` still receives it lower-cased.
2. Nothing else. All names and types are used as written in `00-contract.md`.

#### Notes for the stitcher

**Order and dependencies.** Tasks 2→3→4→5, strictly in that order. Task 2 needs Task 1 merged: it uses `nif::material::{MaterialState, MaterialState::builtin_default, MaterialState::set_slot_raw}`, and `MaterialState::from_lighting` in Task 5. Task 2 makes three commits: (a) the crate scaffold, (b) the shared-file commit (root `Cargo.toml` member line + `Cargo.lock`), (c) the TDD work. (b) cannot come first: it would point at an uncommitted directory.

**Decisions taken here. Each is cited in the code, and the other parts must know them:**
- `matched_path` → `None` = no rule hit; `Some("")` = an entry matched but PGPatcher turns PBR off (`pbr:false`). `simulate` treats `Some("")` as a match: scalars apply, there are no slot/flag/type writes, and there is a `Badge::NotPreviewed("pbr")`. The result is `Patched { matched_path: "" }`. The GUI (Task 14) must not read an empty `matched_path` as "no match".
- `nif_filter` is checked only in `shape_matches`. `matched_path` has no `nif_path` parameter in the contract, so it ignores the filter. `simulate`/`simulate_file` call `shape_matches` first. `shape_matches` takes the **prepared** entry (`prepare(entry, default)`), as in the contract. Spec §2.1's older `(entry, default, …)` form is superseded. Task 14's candidate re-check must call `prepare` first.
- Matching uses `slots[i].raw.to_ascii_lowercase()`, never `norm`. `norm` has `/` and an inserted `textures/`, which would break `matchN` equality and the `textures\pbr\` strip.
- After any PBR write, all 9 slots are set from PGPatcher's lower-cased copy via `set_slot_raw` ([Inference] PGPatcher's `setTextureSet` writes back its lower-cased working set; the §5.1 validator compares slots lower-cased). With no PBR write (`pbr:false`, `delete` first), the source `TexPath`s are kept untouched.
- **MLP floats persist like nifly's in-memory fields.** Their working copy is seeded from the source `LightingTrailer::MultiLayer`, else nifly's defaults (0, 1, (1, 1), envmap strength 1.0; nifly `Shaders.hpp:699-702`). It is threaded through all entries of a file. The trailer is written at the end: `MultiLayer{..}` for final type 11, `EnvMap{scale}` for final type 1, the source trailer if the type is unchanged, else `None`.
- `vertex_colors` sets `has_vertex_colors` **and** SLSF2.5, as the C++ touches the shape attribute too. `vertex_colors:true` on a mesh without a colour stream therefore gives `has_vertex_colors = true`. render3d (part R) must treat a missing colour stream as white.
- `delete:true` → `Patched { material: <state so far, hidden: true>, .. }`. Processing stops at that entry, and the §1.4 check is skipped for the whole file (TP:338-345).
- 1.2.0 `canApply`: a source with SLSF1.21 (FaceGen RGB tint) → `Skip("PGPatcher 1.2.0 skips shapes with SLSF1.21 (FaceGen RGB tint)")`. The GRASS-use half of `canApply` is plugin-side and not modelled (spec §3).
- Wrong JSON types (01 §2.4: 1.2.0 throws inside the per-shape patch) → `Skip("PGPatcher 1.2.0 would fail on \"key\": expected …")`. This over-approximates: a wrong-typed known key is rejected even on a code path PGPatcher would not reach. Two exceptions follow the C++ exactly: a `glint`/`fuzz` value that is not an object still enables glint/fuzz with default parameters (the C++ only calls `contains` on it), and a colour array shorter than required is ignored.
- Badges (`Badge::NotPreviewed(key)`), deduplicated, in this order: `smooth_angle`, `auto_uv`, `vertex_color_lum_mult`, `vertex_color_sat_mult` (the last two only when the shape has vertex colours after `vertex_colors` is applied, since PGPatcher no-ops them otherwise), `pbr` (only when not `true`), `env_mapping`, `env_map_scale`, `env_map_scale_mult`. `pbr:false` and `env_mapping*` are still **simulated** on the NIF fields (type 1 + SLSF1.7 + SLSF2.27 + envmap scale). The badge tells the user the preview is not trustworthy for them.
- `simulate_file`'s `matched_path` = the matched path of the last entry applied.
- **Finding against 01 §1.3 [Inference] "forward slashes are not converted".** `getTexBase` builds `path.parent_path() / path.stem()` with MSVC `std::filesystem` (NU `getTexBase`). That treats `/` as a separator and joins with `\`, so the **last** separator of a `match_*` value (and of a NIF slot) comes back as `\`. `"landscape/rock01_d"` therefore matches `textures\landscape\rock01_d.dds`; `"a/landscape/rock01_d"` still does not match `…\a\landscape\…`. Both cases are pinned in Task 2's tests. The rest of 01 stands.

**Where this port deviates from the Python validator `pgp_tp_validate.py` (and follows 01 / the C++ instead):**
1. It has the §1.4 missing-texture rejection; Python has none.
2. Booleans are `as_bool() == Some(true)`, not Python's truthiness (`fl()`).
3. `vertex_colors` also sets the shape's vertex-colour attribute (`has_vertex_colors`).
4. There is type validation (above); Python has none.
5. Extension/separator handling follows `std::filesystem` (above).
6. It has the SLSF1.21 `canApply` skip.
7. Badges exist.

The golden cases agree with Python on every compared field.

**Not modelled (out of P1 scope, spec §3 / 01 §4):** cross-file arbitration (§1.5 sort); the pre-patched `_rmaos` pseudo-match (§1.4 last bullet, only relevant across files); alternate-texture TXSTs; GRASS uses; `PreFixMeshLighting` (owner has it off); geometry ops (`smooth_angle`, `auto_uv`, vertex-colour multipliers: badged).

**Review Focus candidates (for the plan header):**
1. A user-imported JSON with a wrong-typed key (`"emissive": "yes"`) → `Skip` with the key name, never a panic (Task 4 test `wrong_json_type_is_a_skip`).
2. A source NIF with non-ASCII or odd slot strings → matching and the prefix cut never panic (Task 3 case "non-ASCII never panics").
3. A shape whose untouched vanilla slot is missing from the VFS → `Skip`, not a silently degraded render (Task 4 `missing_untouched_source_slot_is_a_skip`). The GUI's `exists` must cover BSAs (CONTRACT ISSUES 1).
4. A PGPatcher config that is a bare array (no `default`) → `default = Value::Null` works (Task 2 `prepare` with Null; Task 5 golden uses two bare-array files).
5. Several sdc entries matching one shape → last one wins, and `emissive/parallax/subsurface:false` from `default` resets earlier ones (Task 5 `later_entry_wins_and_resets_defaults`).

**Global constraints to copy into the plan header:** as in `00-contract.md`. pgpsim adds no workspace dependency (it uses `nif` by path and `serde_json.workspace = true`).

---

### Part G (modscan, gui)
#### CONTRACT ISSUES

1. **Where `ShapeInfo`, `ShapeStatus`, `EditKind` and `Builtin` live.** The contract lists them under "render3d `Renderer` public API" with no module, and its `pub use` line re-exports only the `parse` items. Part G imports `render3d::{EditKind, ParsedNif, ParsedShape, ShapeId, ShapeInfo, ShapeStatus}` from the crate root and `render3d::builtin::Builtin` from its module. If part R puts the first three in a module, it must re-export them at the root (or the stitcher changes the `use` lines in Tasks 12–15).
2. **Assumed derives** (contract comments, used by G): `Builtin: Copy + Eq + Debug`, `ParsedNif: Default + Clone + Debug` with pub `le_mesh` and `shapes`, `ParsedShape` with pub `id`, `name`, `material`, `MaterialState: PartialEq + Clone + Debug`, `pgpsim::SimInput` with pub fields, `pgpsim::Badge::NotPreviewed(&'static str)` as its only variant.
3. **`MatchRow` field meaning (a clarification, not a change).** `copy_id` = the game-winning copy. `shape_block`, `shape_name`, `skinned` and `le_mesh` describe the **copy to load** (`load_copy_id`). For a PGPatcher-derived winner, the query matches on the source copy's own shape rows. Block numbers can differ between a derived copy and its source, so the derived copy's rows are never used.
4. **`TexKey` value form (decided here, as the contract asked).** `slot` is 0-based (PGPatcher `slotN` − 1). Values are passed in PGPatcher form (backslashes, any case, as `pgpsim::prepare` leaves them); `nifs_using_texture` lowercases them and converts `\` to `/`, because `path.p` holds `canon_texture` output (`textures/…`, lower case, `/`, extension kept; `nif/src/path.rs:50-77`). Spec §2.4's `textures\pbr\`→`textures\` rewrite and `_n`/`_msn` strip run on the stored path in Rust, after a superset SQL filter.

No other conflicts with the contract.

#### Notes for the stitcher

- **G adds beyond the contract (allowed, G-owned):** `FileSource::Memory` gets a hand-written `PartialEq`/`Debug`; `viewport3d::Content`, `Viewport3d::{set_content, set_material, content}`, `viewport3d::read_file`, `V3Reply::MaterialSet` (proves "zero compiles" in Task 17), `ScanError::SchemaVersion` (modscan, nif ground), `SetPreview::poll_at` (tests drive the time), `SetPreview::from_settings`, `Render3dSettings::set_preview_max`.
- **Exact `V3Cmd` shapes:** `LoadParsed { id: u64, nif: Arc<ParsedNif>, textures: Arc<HashMap<String, FileSource>> }`, `LoadBuiltin { id: u64, shape: Builtin, material: MaterialState, textures: Arc<…> }`, `SetMaterial { id: ShapeId, material: MaterialState }`. `V3Reply::Shapes(Vec<ShapeInfo>)` is sent after every Parsed/Builtin load and after every batch of material edits.
- **No `worker.rs` edit.** `worker::spawn_guarded` only takes `Sender<WorkerMsg>`, so `model/set_preview.rs` has its own small `catch_unwind` thread helper and its own channels. CONCURRENCY §3's `worker.rs` hook is not used.
- **Shared-file edits** (each its own commit): `crates/gui/src/model/mod.rs` gets `pub mod set_preview;` (Task 13, step 5); `crates/gui/src/ui/mod.rs` gets `pub mod set_preview;` (Task 15, step 4); `crates/gui/Cargo.toml` gets `pgpsim = { path = "../pgpsim" }` (Task 14, step 1); and in Task 16, `settings.rs` (one field), `ui/mod.rs` (one `State` field and one panel block) and `app.rs` (five small additions). `crates/gui/Cargo.toml` is not on CONCURRENCY's shared list, but it is sdc-adjacent, so the edit is add-only and kept apart.
- **Existing kittests are protected.** `ui::State` derives `Default`. `SetPreview::default()` has `enabled: false`, no store path and no shaders, so every existing test that draws through `ui::draw` (its `TEST_STATE`) keeps today's layout and never opens the developer's `%LOCALAPPDATA%\sdc\scan.db`. `SdcApp::new` builds the enabled instance with `SetPreview::from_settings`.
- **D5 cap.** The cap is `DEFAULT_CAP = 2048` in Task 13. Task 16 makes it the `gui.toml` setting `[render3d] set_preview_max` (0 = 2048). It is not offered in the settings panel (`ui/advanced.rs` is sdc's file); a follow-up can add it there.
- **P1-A texture changes reload.** A texture change re-encodes the member, then sends `LoadParsed`/`LoadBuiltin` with new `Memory` ids (`set_content(.., keep_view = true)` keeps the camera and the last frame). Hot-swap is P1-B.
- **Known limits, said in the code comments:** coat and fuzz share slot 7 (both members claim the explicit slot-7 path; the later one wins in the memory map). A JSON-only edit that makes PGPatcher name a new *VFS* texture (not a set member) draws that slot with the renderer default until the next reload. DDS inputs are served at full size (no cap).
- **`exists` (part P's binding clarification is applied):** `SetPreview::exists` is pub. It returns true for sdc's encodable outputs or for any VFS winner, loose or BSA (`Store::texture_location`), because pgpsim asks about every non-empty slot. Task 14 tests it with a BSA-only vanilla texture.
- **Plan header:** copy the contract's "Global constraints". Task 17's "Verification" section goes at the end of the stitched plan file.

---
