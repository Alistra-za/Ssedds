# P1-B "No Stalls" Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** The 3D preview never freezes: shader compiles run in the background with the old image kept until the new program is ready, compiled shaders are cached by content and pruned, a re-encoded texture swaps into the loaded mesh without a reload, the Encoded/Source switch flips instantly once both variants exist, and multi-node NIFs are placed as NifSkope places them.

**Architecture:** `nif` gains a shared NiAVObject head reader and a node graph (`world` = root…parent × local). `render3d` gains a content-addressed `ProgramKey` over the expanded shader source, a pruned disk cache, a 2-worker below-normal `CompilePool` with `take_jobs`/`apply_compiled`, `ShapeStatus::{Pending, Failed}` with `showing_previous`, and per-path `TexEntry` holding Source and Encoded textures with `replace_texture`/`set_texture_variant`. The GUI render thread owns the pool and feeds results back into its own command queue; the set preview sends `SwapTexture`/`SetTextureVariant` instead of full scene pushes when only bytes change.

**Tech Stack:** Rust workspace; D3D11 via the `windows` crate (WARP in tests; two extra `windows` features in `crates/render3d/Cargo.toml`, no new crates); D3DCompile; std threads + channels; egui/eframe + egui_kittest; SQLite scan DB untouched.

**Spec:** `F:\ai\dds\nifparseref\plan\p1\00-p1-spec.md` (approved 2026-10-11), §4 row "P1-B: no stalls" and §2.2 rulings. Design: `03-renderer-p1-design.md` §1.3–§1.4, §2, §4, §5, §8 (same folder). Interface contract the parts were written against: `plan-parts-b\00-contract.md`. P1-A records: plan `2026-10-11-plan-P1A-live-set-preview.md`, ledger `F:\ai\dds\converter-nif-p1\.superpowers\sdd\2026-10-11-plan-P1A-3d-preview-live-set\progress.md`.

## Global Constraints

- Branch `nif/b-p1b` from `nif/main @ 8159bdf` (= `main`, P1-A merged), own worktree `F:\ai\dds\converter-nif-p1b`. Read `F:\ai\dds\nifparseref\plan\CONCURRENCY.md` first. Never touch `F:\ai\dds\converter`. Never push. Never merge into `main`.
- Off-limits (read pub items only): `crates/engine/`, `crates/cli/`, `crates/encoders-sys/`, `crates/gui/src/pgp/`, `crates/gui/src/sets_doc.rs`, `crates/gui/src/model/sets.rs`, `crates/gui/src/model/pbr_export.rs`, `schema/`, `docs/DECISIONS.md`, golden and `gui_repro` tests.
- No shared-file edits are planned (`Cargo.toml`, `Cargo.lock`, `crates/gui/Cargo.toml`, `crates/gui/src/ui/mod.rs`, `crates/gui/src/model/mod.rs`, `crates/gui/src/app.rs`, `crates/gui/src/settings.rs`). `crates/render3d/Cargo.toml` (nif-team crate) gains two `windows` features (LibraryLoader, Threading); `Cargo.lock` must stay unchanged — if it changes, stop and report.
- No new crate dependencies. The pool is std threads + channels.
- Spec §2.2: root node transform applied (as NifSkope); compile pool = 2 workers, below-normal priority, not a setting.
- Cache: `%LOCALAPPDATA%\sdc\shader-cache`; prune caps 512 MiB, entries untouched for 90 days, `.tmp-*` and orphan `.cso` older than 1 day; a missing or torn pair is a miss.
- Gates before every commit: `cargo fmt --all && cargo clippy --workspace --all-targets -- -D warnings && cargo test --workspace`. Commit messages end with `Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>`.
- Tests run on WARP + the repo stand-in `crates/render3d/tests/fixtures/standin/Lighting.hlsl`. Owner-data tests are `#[ignore]` + `SDC_ACHERUS` and panic without it.
- `Renderer::load` and every existing caller/test keep synchronous behaviour (`set_async_compile(false)` default + `settle()`); only the GUI render thread turns async on.
- Timing log (owner request, Task 9): JSON Lines at `%LOCALAPPDATA%\sdc\logs\preview-timings-*.jsonl`, 10 files kept, 20 MB cap per file, background writer drops events when its queue is full; `SDC_PREVIEW_LOG=0` disables it. Tests never write to the real log folder.
- Ruling (controller, 2026-10-11): `crates/gui/src/main.rs` may gain exactly the 2 lines Task 9 names (`let _perf = render3d::perf_log::start();`), add-only, in its own small commit, like a shared file. Why: the fallback (start from `Viewport3d`) would make gui tests write into the owner's real log folder. Cost if wrong: a 2-line revert.
- Review routing (owner): per-task reviews on Codex GPT-6-Astra (low/medium/high by risk); Opus only for the final whole-branch review.

## Review Focus

1. **A shader change never freezes or blanks the preview:** while a new program compiles the old draw keeps drawing (frame byte-identical), and a compile failure keeps the old pixels with the error listed. Pinned by Task 5 `permutation_edit_keeps_old_draw_until_compiled` and `failing_permutation_keeps_old_draw`, and Task 6 `a_reload_with_pending_shapes_holds_the_old_image_until_they_all_resolve`.
2. **Late or out-of-order compile results never land on the wrong shape and never leave the app busy or repainting.** Pinned by Task 5 `out_of_order_results` / `a_result_for_a_replaced_load_only_warms_the_memo` and Task 6 `a_dead_render_thread_does_not_leave_pending_shapes_busy` plus the two real-thread "idle stops repainting" tests.
3. **The cache key is the compiled bytes' identity:** the same sources under two roots share an entry, editing an included `.hlsli` changes the key, and pruning never breaks a reader (torn pair = miss). Pinned by Task 3 `key_ignores_the_root_path` / `editing_an_included_file_changes_the_key` and Task 4 `a_pruned_entry_is_recompiled`.
4. **A hot-swap never corrupts the binding:** a texture of the wrong kind keeps the old SRV and warns; flipping A/B back is byte-identical; swapping an emissive map in/out flips HasEmissive; a swap reaches shapes that are Pending or Failed but still showing their previous draw. Pinned by Task 7 T-H2, T-H3, T-H4 and the reconciliation test R1 below.
5. **Multi-node NIFs place shapes as NifSkope does; unknown node types are reported, not fatal.** Pinned by Task 1 `world_composes_parent_chain` and Task 2 DeepGraph/Furniture goldens and `unknown_node_type_unparents_its_shapes_with_a_warning`.

## Task order and dependencies

Tasks run in this order: **1 → 2 → 3 → 4 → 5 → 6 → 7 → 8 → 9 → 10**.
- 1 → 2 (transforms). 3 → 4, 5. 5 → 6. 7 → 8. 7 is written against `8159bdf` and runs after 5: follow its "Anchors earlier tasks may shift" notes and the reconciliation below. 8 runs after 6 (both edit `crates/gui/src/model/viewport3d.rs`). 9 (timing log) after 8: it hooks code from Tasks 3–8. 10 (owner checklist, numbers read from the Task 9 log) last.
- Where a task edits a file an earlier task changed, follow the quoted text anchors, not line numbers.

## Cross-part reconciliation (binding; the parts were drafted in parallel)

- **R1 (Task 7 after Task 5):** Task 7's swap walk filters draws with `s.status == ShapeStatus::Drawing`. After Task 5, shapes in `Pending { showing_previous: true }` and `Failed { showing_previous: true, .. }` still draw their previous `ShapeDraw`, so a swap must rebind those too: select every shape with `s.draw.is_some()` that is not `Hidden`. Add a Task 7 test: a shape whose permutation edit is withheld (Pending, previous draw kept) receives a swap → its pixels change.
- **R2 (Task 2 before Task 5):** Task 5's code marks `// Task 2:` at `model: mesh.transform`. Task 2 has already landed, so Task 5 must keep Task 2's `world` there, not reintroduce `mesh.transform`.
- **R3 (Task 8 after Task 6):** Task 6 replaces the device-rebuild path with `Renderer::rebuild_device_on(gpu)` (keeps `compiled`) and adds `V3Reply::Refresh`, `V3Cmd::Quit` and a narrowed `held_frame`. Task 8's `rebuild` closure change (`load_all(r, c, edits, tex)`) is applied to whatever rebuild path Task 6 left, so a device rebuild replays the swap log after `rebuild_device_on`. Task 8 must not re-widen `held_frame`.
- **R4 (ShapeStatus):** after Task 5 every match on `ShapeStatus` uses `Failed { error, showing_previous }` and handles `Pending { .. }`; Task 7/8 code that matches statuses uses those forms.

---


# Part T: parent NiNode transforms (Tasks 1-2)

> Code base: worktree `F:\ai\dds\converter-nif-p1` @ `8159bdf`; executed later on `nif/b-p1b` (`F:\ai\dds\converter-nif-p1b`). Binding names: `00-contract.md`. Design: `03-renderer-p1-design.md` §4, tests T-T1/T-T2. Spec ruling: the root node's transform IS applied (as NifSkope does).
> Line numbers are hints; follow the text anchors.

## CONTRACT ISSUES
- `read_av_head` takes `(h: &Header, c: &mut Cursor)`, not just the cursor: `AvHead.name: Option<String>` needs the header's string table (the contract left the parameter list open).
- "unknown node types -> problems" cannot be detected inside `read_graph`: a block type outside `NODE_TYPES` is indistinguishable from any other non-node block. It is reported in render3d `parse` instead: every kept shape that has no parent entry (and is not block 0) gets the warning `"<name>: not under any known node; placed by its own transform"`. `read_graph` problems are: child ref out of range, child already has a parent (first wins), child would make a cycle, node unreadable (truncated), node deeper than 256.
- 03 §4 sketched `NODE_TYPES: &[&[u8]]`; the contract's `&[&str]` is used (compare with `t.as_bytes() == header.kind(i)`).
- DeepGraph bounds: exact bound values need a full vertex decode that could not be done by hand while writing this plan. Task 2 pins exact golden *matrices* (computed independently in Python from the fixture's node blocks), exact bounds for the Furniture fixture (pure translation, so derivable), and for DeepGraph a structural bounds check against independent recomputations (local-only and reverse-order) that fails on a wrong composition order.

## SHARED-FILE EDITS NEEDED
None. (`crates/nif/src/lib.rs` is nif-team code, not a shared file; render3d edits stay in its own crate; no `Cargo.toml` changes.)

## Verified against the code at 8159bdf
- `crates/nif/src/geom/bstrishape.rs`: `read_shape(h: &Header, b: &[u8], block: usize) -> Result<Mesh, NifError>` reads the NiObjectNET + NiAVObject prefix inline (text anchor: comment `// NiObjectNET` through the `let mut m = Mesh {` statement), with `block_cursor(h, b, block)?` from `crate::geom::skin_partition` (`pub(crate) fn`, module is `pub`).
- `crates/nif/src/cursor.rs`: `Cursor::{new, pos, remaining, skip, u8, u16, u32, i32, u64, f32, f32s::<N>}`, errors are `NifError`.
- `crates/nif/src/header.rs`: `Header { bsver, block_types, type_index, sizes, strings, data_start, offsets }`, `Header::parse(&[u8])`, `kind(block) -> &[u8]`, `string(idx: i32) -> Option<&[u8]>`, `offsets()`.
- `crates/nif/src/text.rs`: `pub fn lossy(&[u8]) -> String`.
- `crates/nif/src/geom/mod.rs`: `mod bstrishape; pub mod skin_partition; mod vertex_desc;`; `Mesh.transform` = local transform, `Default` = identity.
- `crates/nif/tests/builder/mod.rs`: `Nif::{new(bsver), string, block(kind, body) -> i32, bytes}` and the `W` byte writer (`i32/u32/f/fs/...`), used as `mod builder; use builder::{Nif, W};` by `tests/header.rs`, `props.rs`, `scan.rs`.
- `crates/render3d/src/parse.rs`: `ParsedShape { id, name, mesh: Arc<Mesh>, material }`, constructed only in `parse.rs` and `lib.rs::load_builtin`; `parse(&[u8]) -> ParsedNif`; `ParsedNif.warnings`. `lib.rs` re-exports `parse::{parse, ParsedNif, ParsedShape, ShapeId}`.
- `crates/render3d/src/lib.rs`: private `ShapeState { name, mesh, material, draw, status }`; `Renderer::build_draw(&mut self, name, mesh, m, textures, rep)` with two call sites (`load_parsed`, and the rebuild branch of `set_material`) and the line `model: mesh.transform,` inside it; `compute_bounds` reads `s.mesh.transform`; `Renderer::bounds()`; `ShapeDraw.model: Mat3x4` (draw.rs) feeds `View::world(&self.model)` (scene.rs).
- Fixtures, computed with an independent Python parser while writing this plan: DeepGraph has 26 node blocks and 27 shapes, every shape has a parent chain to block 0, no multi-parent or out-of-range child refs; Furniture: node 0 identity, node 3 "Furniture" identity rotation + translation `(0, 85.12416076660156, -6.657e-13)`, children 9 and 12 (the two shapes); Skinned_SE / Skinned_Dynamic_SE / Optimize_LE_to_SE: identity root with children 1, 2 (translated bones) and 3, 9 (shapes).
- Existing tests that compare fixed pixels on Furniture/DeepGraph: none found (read `draw.rs`, `edit.rs`, `frame.rs`, `load.rs`, `standin.rs`, `builtin.rs`, `lights.rs`). They compare renderer A vs the reference writer B, before vs after an edit, coverage thresholds, `last_uploads` counts, all relative to `bounds()` auto-framing. Byte-identical by construction: built-in shapes (`load_builtin` sets `world = mesh.transform`) and the Skinned fixtures (shapes hang off the identity root; the skinned VS ignores World). Furniture and DeepGraph frames change on purpose (shapes move); Task 2 Step 6 runs the whole render3d suite to prove the relative assertions still hold. Not run while writing this plan (read-only).

---

## Task 1: `nif::geom::av` (shared AvHead reader) and `nif::graph`

**Files:**
- Create: `crates/nif/src/geom/av.rs`, `crates/nif/src/graph.rs`
- Create: `crates/nif/tests/av.rs`, `crates/nif/tests/graph.rs`, `crates/nif/tests/fixtures/nifxml_inherit.txt`
- Modify: `crates/nif/src/geom/mod.rs`, `crates/nif/src/geom/bstrishape.rs`, `crates/nif/src/lib.rs`

**Interfaces:**
- Consumes: `Cursor`, `Header`, `NifError`, `text::lossy`, `geom::skin_partition::block_cursor`.
- Produces (binding, see contract):
  ```rust
  // nif::geom::av  (re-exported as nif::geom::{AvHead, read_av_head})
  pub struct AvHead { pub name: Option<String>, pub transform: [[f32; 4]; 3] }
  pub fn read_av_head(h: &Header, c: &mut Cursor) -> Result<AvHead, NifError>;
  // nif::graph
  pub struct NodeGraph { pub parent: HashMap<u32,u32>, pub local: HashMap<u32,[[f32;4];3]>, pub problems: Vec<String> }
  pub fn read_graph(header: &Header, bytes: &[u8]) -> NodeGraph;
  pub fn world(g: &NodeGraph, shape_block: u32, shape_local: &[[f32;4];3]) -> [[f32;4];3];
  pub const NODE_TYPES: &[&str];
  ```
  Extra public items (not in the contract, additive): `graph::{Mat3x4, IDENTITY, MAX_DEPTH}`.

**Decision (`node_types_match_nifxml_closure`):** nif.xml lives outside the repo, so the unit test re-derives the closure from a committed 555-line `name inherit` list (`tests/fixtures/nifxml_inherit.txt`, about 19 KB) that one Python command regenerates from nif.xml; a nif.xml update shows up as a diff of that file and a failing test until `NODE_TYPES` is updated.

### Part 1a: AvHead reader, bstrishape uses it (no behaviour change)

- [ ] **Step 1: failing test.** Create `crates/nif/tests/av.rs`:

```rust
// NiObjectNET + NiAVObject prefix reader shared by bstrishape.rs and graph.rs
mod builder;
use builder::{Nif, W};
use nif::cursor::Cursor;
use nif::geom::{read_av_head, AvHead};
use nif::header::Header;

#[test]
fn av_head_reads_name_transform_and_stops_before_the_subclass_fields() {
    let mut f = Nif::new(100);
    let name = f.string("Root");
    // name, 2 extra-data refs, controller, flags, t, R (row triples), scale, collision | then NiNode: 0 children
    let body = W::default()
        .i32(name)
        .u32(2)
        .i32(7)
        .i32(8)
        .i32(-1)
        .u32(14)
        .fs(&[1.0, 2.0, 3.0])
        .fs(&[0.0, 1.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0])
        .f(2.0)
        .i32(-1)
        .u32(0)
        .0;
    f.block("NiNode", body);
    let bytes = f.bytes();
    let h = Header::parse(&bytes).unwrap();
    let mut c = Cursor::new(&bytes, h.offsets()[0]);
    let av = read_av_head(&h, &mut c).unwrap();
    assert_eq!(
        av,
        AvHead {
            name: Some("Root".into()),
            transform: [
                [0.0, 2.0, 0.0, 1.0],
                [2.0, 0.0, 0.0, 2.0],
                [0.0, 0.0, 2.0, 3.0]
            ],
        }
    );
    assert_eq!(c.u32().unwrap(), 0, "cursor is at the NiNode child count");
}

#[test]
fn av_head_truncation_is_an_error() {
    let mut f = Nif::new(100);
    f.block("NiNode", W::default().i32(-1).u32(0).0);
    let bytes = f.bytes();
    let h = Header::parse(&bytes).unwrap();
    let end = h.offsets()[0] + h.sizes[0] as usize;
    let mut c = Cursor::new(&bytes[..end], h.offsets()[0]);
    assert!(read_av_head(&h, &mut c).is_err());
}
```

- [ ] **Step 2: run it, expect failure.** `cargo test -p nif --test av` -> compile error `unresolved imports nif::geom::read_av_head, nif::geom::AvHead`.

- [ ] **Step 3: implement.** Create `crates/nif/src/geom/av.rs`:

```rust
//! NiObjectNET + NiAVObject prefix at BSVER 100 (nif.xml NiAVObject 3440-3460): name, extra-data refs,
//! controller, flags, translation, rotation, scale, collision object. Shared by the shape reader and the node
//! graph. SSE has no property list (that field is `NI_BS_LTE_FO3`).
use crate::cursor::Cursor;
use crate::header::Header;
use crate::text::lossy;
use crate::NifError;

/// `transform` has the `Mesh.transform` convention: row r is `[s·R[r][0], s·R[r][1], s·R[r][2], t[r]]`.
#[derive(Debug, Clone, PartialEq)]
pub struct AvHead {
    pub name: Option<String>,
    pub transform: [[f32; 4]; 3],
}

/// Reads from the start of a block body and leaves the cursor at the first field of the concrete type.
pub fn read_av_head(h: &Header, c: &mut Cursor) -> Result<AvHead, NifError> {
    let name = h.string(c.i32()?).map(lossy).filter(|s| !s.is_empty());
    let n = c.u32()? as usize;
    c.skip(n.saturating_mul(4))?; // extra data refs
    c.i32()?; // controller
    c.u32()?; // flags (u32 above BSVER 26)
    let t = c.f32s::<3>()?;
    let r = c.f32s::<9>()?;
    let s = c.f32()?;
    c.i32()?; // collision object
    Ok(AvHead {
        name,
        transform: [0, 1, 2].map(|i| [r[3 * i] * s, r[3 * i + 1] * s, r[3 * i + 2] * s, t[i]]),
    })
}
```

In `crates/nif/src/geom/mod.rs` replace the three `mod` lines and the two `pub use` lines with:

```rust
pub mod av;
mod bstrishape;
pub mod skin_partition;
mod vertex_desc;

pub use av::{read_av_head, AvHead};
pub use bstrishape::read_shape;
pub use vertex_desc::{Attr, VertexDesc};
```

In `crates/nif/src/geom/bstrishape.rs` add `use super::av::read_av_head;` next to the other `use super::...` lines, and replace this span (from the comment `// NiObjectNET` through the end of the `let mut m = Mesh { ... };` statement):

```rust
    // NiObjectNET
    c.i32()?; // name
    let n = c.u32()? as usize;
    c.skip(n.saturating_mul(4))?; // extra data refs
    c.i32()?; // controller
    c.u32()?; // NiAVObject (nif.xml 3440-3460) flags
    let t = c.f32s::<3>()?;
    let r = c.f32s::<9>()?;
    let s = c.f32()?;
    c.i32()?; // collision
    let mut m = Mesh {
        transform: [0, 1, 2].map(|i| [r[3 * i] * s, r[3 * i + 1] * s, r[3 * i + 2] * s, t[i]]),
        ..Mesh::default()
    };
```

with:

```rust
    // NiObjectNET + NiAVObject (nif.xml 3440-3460)
    let av = read_av_head(h, &mut c)?;
    let mut m = Mesh {
        transform: av.transform,
        ..Mesh::default()
    };
```

- [ ] **Step 4: run, expect pass; existing nif tests stay green.** `cargo test -p nif` -> all pass (notably `tests/geom.rs`, which pins the shape transforms).

- [ ] **Step 5: commit.**
```
cargo fmt --all && cargo clippy --workspace --all-targets -- -D warnings && cargo test --workspace
git add crates/nif/src/geom/av.rs crates/nif/src/geom/mod.rs crates/nif/src/geom/bstrishape.rs crates/nif/tests/av.rs
git commit -m "nif: shared NiAVObject prefix reader (read_av_head), bstrishape uses it

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

### Part 1b: `nif::graph`

`NODE_TYPES` below is the transitive `inherit="NiNode"` closure of `F:\ai\dds\nifparseref\nifxml-master\nif.xml` (31 types, computed 2026-10-11, sorted).

- [ ] **Step 6: create the nif.xml inheritance list the unit test reads.** From the worktree root (`python3 -I`; nif.xml is outside the repo):

```
python3 -I -c "import xml.etree.ElementTree as E; r=E.parse(r'F:/ai/dds/nifparseref/nifxml-master/nif.xml').getroot(); open(r'crates/nif/tests/fixtures/nifxml_inherit.txt','w',newline='\n').write(''.join(f\"{n.get('name')} {n.get('inherit') or '-'}\n\" for n in r.iter('niobject')))"
```
Expect 555 lines; the first three are `NiObject -`, `Ni3dsAlphaAnimator NiObject`, `Ni3dsAnimationNode NiObject`.

- [ ] **Step 7: failing tests.** Add `pub mod graph;` to `crates/nif/src/lib.rs` after `pub mod geom;`. Create `crates/nif/src/graph.rs` containing only this test module:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    /// 03 T-T1. Root: scale 2 + translate (10,0,0). A: rotate 90° about z (x -> y) + translate (0,5,0).
    /// Shape: translate (1,0,0). World = Root · A · Shape: shape point (0,0,0) -> (1,0,0) -> (0,1,0)+(0,5,0)
    /// = (0,6,0) -> (2·0+10, 2·6, 0) = (10,12,0).
    #[test]
    fn world_composes_parent_chain() {
        let root = [[2.0, 0.0, 0.0, 10.0], [0.0, 2.0, 0.0, 0.0], [0.0, 0.0, 2.0, 0.0]];
        let a = [[0.0, -1.0, 0.0, 0.0], [1.0, 0.0, 0.0, 5.0], [0.0, 0.0, 1.0, 0.0]];
        let shape = [[1.0, 0.0, 0.0, 1.0], [0.0, 1.0, 0.0, 0.0], [0.0, 0.0, 1.0, 0.0]];
        let g = NodeGraph {
            parent: HashMap::from([(1, 0), (2, 1)]),
            local: HashMap::from([(0, root), (1, a)]),
            problems: vec![],
        };
        let want = [[0.0, -2.0, 0.0, 10.0], [2.0, 0.0, 0.0, 12.0], [0.0, 0.0, 2.0, 0.0]];
        assert_eq!(world(&g, 2, &shape), want);
        // the wrong (child-first) order is a different matrix, so the test pins the order
        assert_ne!(mul(&mul(&shape, &a), &root), want);
        // a block with no parent keeps its own transform; the root node is included for its children
        assert_eq!(world(&g, 7, &shape), shape);
        assert_eq!(world(&g, 1, &IDENTITY), mul(&root, &a));
    }

    #[test]
    fn world_terminates_on_a_hand_built_cycle() {
        let t = [[1.0, 0.0, 0.0, 1.0], [0.0, 1.0, 0.0, 0.0], [0.0, 0.0, 1.0, 0.0]];
        let g = NodeGraph {
            parent: HashMap::from([(1, 2), (2, 1)]),
            local: HashMap::from([(1, t), (2, t)]),
            problems: vec![],
        };
        assert_eq!(world(&g, 1, &IDENTITY)[0][3], MAX_DEPTH as f32);
    }

    /// 03 T-T1. The list is the closure of `inherit="NiNode"` over the committed nif.xml inheritance list
    /// (`tests/fixtures/nifxml_inherit.txt`, regenerated from nif.xml with the command in the P1-B plan).
    #[test]
    fn node_types_match_nifxml_closure() {
        let mut inherit: HashMap<&str, &str> = HashMap::new();
        for l in include_str!("../tests/fixtures/nifxml_inherit.txt").lines() {
            let (n, p) = l.split_once(' ').unwrap();
            inherit.insert(n, p);
        }
        let is_node = |mut n: &str| loop {
            if n == "NiNode" {
                return true;
            }
            match inherit.get(n) {
                Some(p) => n = p,
                None => return false,
            }
        };
        let mut want: Vec<&str> = inherit.keys().copied().filter(|n| is_node(n)).collect();
        want.sort_unstable();
        assert_eq!(want, NODE_TYPES, "regenerate NODE_TYPES from nif.xml");
    }
}
```

Create `crates/nif/tests/graph.rs`:

```rust
// read_graph on synthetic NIFs: links, problems, caps. Never an Err.
mod builder;
use builder::{Nif, W};
use nif::graph::{read_graph, world};
use nif::header::Header;

const ID: [[f32; 4]; 3] = [[1.0, 0.0, 0.0, 0.0], [0.0, 1.0, 0.0, 0.0], [0.0, 0.0, 1.0, 0.0]];

/// A NiNode-shaped body (BSVER 100): av prefix with translation `t` and identity rotation, children, 0 effects.
fn node(t: [f32; 3], kids: &[i32]) -> Vec<u8> {
    let mut w = W::default()
        .i32(-1)
        .u32(0)
        .i32(-1)
        .u32(14)
        .fs(&t)
        .fs(&[1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0])
        .f(1.0)
        .i32(-1)
        .u32(kids.len() as u32);
    for k in kids {
        w = w.i32(*k);
    }
    w.u32(0).0
}

fn graph(f: &Nif) -> nif::graph::NodeGraph {
    let bytes = f.bytes();
    read_graph(&Header::parse(&bytes).unwrap(), &bytes)
}

#[test]
fn links_children_and_reads_locals_for_every_node_subclass() {
    let mut f = Nif::new(100);
    f.block("BSFadeNode", node([1.0, 2.0, 3.0], &[1, 2]));
    f.block("NiNode", node([0.0, 5.0, 0.0], &[3]));
    f.block("BSOrderedNode", node([0.0, 0.0, 0.0], &[]));
    f.block("BSTriShape", vec![]); // not a node: never read
    let g = graph(&f);
    assert!(g.problems.is_empty(), "{:?}", g.problems);
    assert_eq!(g.parent.len(), 3);
    assert_eq!((g.parent[&1], g.parent[&2], g.parent[&3]), (0, 0, 1));
    assert_eq!(
        g.local[&1],
        [[1.0, 0.0, 0.0, 0.0], [0.0, 1.0, 0.0, 5.0], [0.0, 0.0, 1.0, 0.0]]
    );
    assert_eq!(g.local[&0][2][3], 3.0);
    assert!(!g.local.contains_key(&3));
    assert_eq!(world(&g, 3, &ID)[1][3], 7.0, "5 (NiNode) + 2 (root)");
}

#[test]
fn unknown_type_children_stay_unparented_without_a_problem() {
    let mut f = Nif::new(100);
    f.block("BSFadeNode", node([0.0; 3], &[1]));
    f.block("FooNode", node([0.0; 3], &[2]));
    f.block("BSTriShape", vec![]);
    let g = graph(&f);
    assert!(g.problems.is_empty(), "{:?}", g.problems);
    assert_eq!(g.parent.len(), 1);
    assert!(!g.parent.contains_key(&2));
}

#[test]
fn bad_refs_are_problems_not_errors() {
    let mut f = Nif::new(100);
    f.block("NiNode", node([0.0; 3], &[5, 1, -1])); // 5 is out of range, -1 is none
    f.block("NiNode", node([0.0; 3], &[]));
    f.block("NiNode", node([0.0; 3], &[1])); // block 1 already has parent 0
    let g = graph(&f);
    assert_eq!(g.parent.len(), 1);
    assert_eq!(g.parent[&1], 0, "first parent wins");
    assert_eq!(
        g.problems,
        [
            "block 0: child ref 5 out of range",
            "block 2: child 1 already has parent 0"
        ]
    );
}

#[test]
fn cycles_are_cut_and_reported() {
    let mut f = Nif::new(100);
    f.block("NiNode", node([0.0; 3], &[1, 2]));
    f.block("NiNode", node([0.0; 3], &[0])); // 0 -> 1 -> 0
    f.block("NiNode", node([0.0; 3], &[2])); // itself
    let g = graph(&f);
    assert_eq!(g.parent.len(), 2);
    assert_eq!(
        g.problems,
        [
            "block 1: child 0 would make a cycle",
            "block 2: child 2 would make a cycle"
        ]
    );
    assert_eq!(world(&g, 2, &ID), ID);
}

#[test]
fn a_truncated_node_is_a_problem_and_a_huge_child_count_does_not_allocate() {
    let mut f = Nif::new(100);
    f.block("NiNode", node([0.0; 3], &[1]));
    f.block("NiNode", W::default().i32(-1).u32(0).0); // ends inside the av prefix
    let mut huge = node([0.0; 3], &[]);
    let n = huge.len();
    huge[n - 8..n - 4].copy_from_slice(&u32::MAX.to_le_bytes()); // child count, before the effects count
    f.block("NiNode", huge);
    let g = graph(&f);
    assert_eq!(g.problems.len(), 2, "{:?}", g.problems);
    assert!(g.problems[0].starts_with("block 1: "), "{:?}", g.problems);
    assert!(g.problems[1].starts_with("block 2: "), "{:?}", g.problems);
    assert_eq!(g.parent[&1], 0);
}

#[test]
fn depth_over_256_is_reported_once_and_world_stops_at_256_ancestors() {
    let mut f = Nif::new(100);
    for i in 0..300i32 {
        let kids: &[i32] = if i < 299 { &[i + 1] } else { &[] };
        f.block("NiNode", node([1.0, 0.0, 0.0], kids));
    }
    let g = graph(&f);
    assert_eq!(g.problems.len(), 1, "{:?}", g.problems);
    assert!(g.problems[0].starts_with("block 257: "), "{:?}", g.problems);
    assert_eq!(world(&g, 299, &ID)[0][3], 256.0);
}
```

- [ ] **Step 8: run, expect failure.** `cargo test -p nif` -> `graph.rs` does not compile: `cannot find NodeGraph / world / read_graph / NODE_TYPES / mul / IDENTITY / MAX_DEPTH`.

- [ ] **Step 9: implement.** In `crates/nif/src/graph.rs`, insert this above the `#[cfg(test)]` line:

```rust
//! Parent NiNode transforms for the 3D preview (03 §4). Only each node's children list is read: subclass
//! fields come after NiNode's children and effects lists, so `NiObjectNET + NiAVObject + u32 n + n refs` is
//! valid for every type in [`NODE_TYPES`]. Nothing here fails a file: problems are strings, a bad link is
//! skipped.
use crate::geom::av::read_av_head;
use crate::geom::skin_partition::block_cursor;
use crate::header::Header;
use crate::NifError;
use std::collections::HashMap;

pub type Mat3x4 = [[f32; 4]; 3];
pub const IDENTITY: Mat3x4 = [[1.0, 0.0, 0.0, 0.0], [0.0, 1.0, 0.0, 0.0], [0.0, 0.0, 1.0, 0.0]];
/// Longest parent chain `world` composes (the nearest 256 ancestors) and `read_graph` accepts without a problem.
pub const MAX_DEPTH: usize = 256;

/// `NiNode` and every type that inherits from it, transitively (nif.xml `inherit="NiNode"` closure, sorted).
/// `node_types_match_nifxml_closure` re-derives it.
pub const NODE_TYPES: &[&str] = &[
    "AvoidNode",
    "BSBlastNode",
    "BSDamageStage",
    "BSDebrisNode",
    "BSDistantObjectInstancedNode",
    "BSFadeNode",
    "BSLeafAnimNode",
    "BSMasterParticleSystem",
    "BSMultiBoundNode",
    "BSOrderedNode",
    "BSRangeNode",
    "BSTreeNode",
    "BSValueNode",
    "CsNiNode",
    "FxButton",
    "FxRadioButton",
    "FxWidget",
    "JPSJigsawNode",
    "NiBSAnimationNode",
    "NiBSParticleNode",
    "NiBillboardNode",
    "NiBone",
    "NiCollisionSwitch",
    "NiLODNode",
    "NiNode",
    "NiRoom",
    "NiRoomGroup",
    "NiSortAdjustNode",
    "NiSwitchNode",
    "NiWall",
    "RootCollisionNode",
];

#[derive(Debug, Clone, Default, PartialEq)]
pub struct NodeGraph {
    /// Child block -> parent node block (first parent wins).
    pub parent: HashMap<u32, u32>,
    /// Node block -> local transform (`Mesh.transform` convention).
    pub local: HashMap<u32, Mat3x4>,
    /// Out-of-range child refs, extra parents, cycles, unreadable nodes, depth over 256; one line each.
    pub problems: Vec<String>,
}

/// `a · b` for 3×4 affine matrices (the implicit fourth row is 0 0 0 1).
fn mul(a: &Mat3x4, b: &Mat3x4) -> Mat3x4 {
    let mut r = [[0.0f32; 4]; 3];
    for (i, row) in r.iter_mut().enumerate() {
        for (j, v) in row.iter_mut().enumerate() {
            let t = if j == 3 { a[i][3] } else { 0.0 };
            *v = (0..3).map(|k| a[i][k] * b[k][j]).sum::<f32>() + t;
        }
    }
    r
}

fn children(h: &Header, b: &[u8], block: usize) -> Result<(Mat3x4, Vec<i32>), NifError> {
    let mut c = block_cursor(h, b, block)?;
    let av = read_av_head(h, &mut c)?;
    let n = c.u32()? as usize;
    if n.saturating_mul(4) > c.remaining() {
        return Err(NifError::Truncated {
            at: c.pos(),
            need: n.saturating_mul(4),
        });
    }
    let kids = (0..n).map(|_| c.i32()).collect::<Result<Vec<_>, _>>()?;
    Ok((av.transform, kids))
}

/// True when `target` is `from` or one of its ancestors (looks at most `MAX_DEPTH + 1` links up).
// shortcut: a cycle closed only through a chain deeper than 256 is not cut here; `world` still stops after 256 links.
fn reaches(parent: &HashMap<u32, u32>, from: u32, target: u32) -> bool {
    let mut cur = from;
    for _ in 0..=MAX_DEPTH {
        if cur == target {
            return true;
        }
        match parent.get(&cur) {
            Some(&p) => cur = p,
            None => return false,
        }
    }
    false
}

fn depth(parent: &HashMap<u32, u32>, mut b: u32) -> usize {
    let mut d = 0;
    while let Some(&p) = parent.get(&b) {
        d += 1;
        b = p;
        if d > MAX_DEPTH + 1 {
            break;
        }
    }
    d
}

/// Never fails. Walks every block whose type is in [`NODE_TYPES`], in block order.
pub fn read_graph(header: &Header, bytes: &[u8]) -> NodeGraph {
    let mut g = NodeGraph::default();
    let total = header.sizes.len();
    for i in 0..total {
        let kind = header.kind(i);
        if !NODE_TYPES.iter().any(|t| t.as_bytes() == kind) {
            continue;
        }
        let (local, kids) = match children(header, bytes, i) {
            Ok(x) => x,
            Err(e) => {
                g.problems.push(format!("block {i}: {e}"));
                continue;
            }
        };
        let node = i as u32;
        g.local.insert(node, local);
        for k in kids {
            if k == -1 {
                continue;
            }
            let Some(c) = usize::try_from(k).ok().filter(|&c| c < total) else {
                g.problems.push(format!("block {i}: child ref {k} out of range"));
                continue;
            };
            let c = c as u32;
            if let Some(p) = g.parent.get(&c) {
                g.problems
                    .push(format!("block {i}: child {c} already has parent {p}"));
            } else if reaches(&g.parent, node, c) {
                g.problems
                    .push(format!("block {i}: child {c} would make a cycle"));
            } else {
                g.parent.insert(c, node);
            }
        }
    }
    let mut keys: Vec<u32> = g.parent.keys().copied().collect();
    keys.sort_unstable();
    for b in keys {
        if depth(&g.parent, b) == MAX_DEPTH + 1 {
            g.problems.push(format!(
                "block {b}: deeper than {MAX_DEPTH} nodes; the ancestors above {MAX_DEPTH} are ignored"
            ));
        }
    }
    g
}

/// The shape's world transform: root … parent chain × `shape_local`, root node included (as NifSkope does).
/// A block with no parent entry keeps `shape_local`. Composes at most [`MAX_DEPTH`] ancestors, so a cycle
/// built by hand still terminates.
pub fn world(g: &NodeGraph, shape_block: u32, shape_local: &Mat3x4) -> Mat3x4 {
    let (mut m, mut cur) = (*shape_local, shape_block);
    for _ in 0..MAX_DEPTH {
        let Some(&p) = g.parent.get(&cur) else { break };
        if let Some(l) = g.local.get(&p) {
            m = mul(l, &m);
        }
        cur = p;
    }
    m
}
```

- [ ] **Step 10: run, expect pass.** `cargo test -p nif` -> `world_composes_parent_chain`, `world_terminates_on_a_hand_built_cycle`, `node_types_match_nifxml_closure`, all of `tests/graph.rs` and `tests/av.rs` pass, plus every pre-existing nif test.

- [ ] **Step 11: commit.**
```
cargo fmt --all && cargo clippy --workspace --all-targets -- -D warnings && cargo test --workspace
git add crates/nif/src/graph.rs crates/nif/src/lib.rs crates/nif/tests/graph.rs crates/nif/tests/fixtures/nifxml_inherit.txt
git commit -m "nif: node graph (parent chain, local transforms, NODE_TYPES from nif.xml closure)

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

---

## Task 2: render3d applies the parent chain (`ParsedShape.world`); bounds and draw use it

**Files:**
- Create: `crates/render3d/tests/transforms.rs`
- Modify: `crates/render3d/src/parse.rs`, `crates/render3d/src/lib.rs`

**Interfaces:**
- Consumes: `nif::graph::{read_graph, world}` (Task 1), `nif::header::Header`.
- Produces (binding): `ParsedShape.world: [[f32; 4]; 3]`; `Mesh.transform` stays local. `Renderer::bounds()` and `ShapeDraw.model` use `world`. New parse warnings: `"NIF: <graph problem>"` and `"<name>: not under any known node; placed by its own transform"`. Private: `ShapeState.world`, `build_draw(.., world, ..)`.

- [ ] **Step 1: failing tests.** Create `crates/render3d/tests/transforms.rs` (P1.1; 03 T-T2):

```rust
//! Parent NiNode transforms (P1.1): ParsedShape.world, the bounds and the draw's model matrix.
#![allow(clippy::needless_range_loop)]
mod common;
use common::{fixture, renderer, FLAT};
use nif::graph::{read_graph, world};
use nif::header::Header;
use render3d::{parse, ParsedNif, ParsedShape, ShapeId};

type M = [[f32; 4]; 3];
const ID: M = [[1.0, 0.0, 0.0, 0.0], [0.0, 1.0, 0.0, 0.0], [0.0, 0.0, 1.0, 0.0]];

fn mul(a: &M, b: &M) -> M {
    let mut r = [[0.0f32; 4]; 3];
    for (i, row) in r.iter_mut().enumerate() {
        for (j, v) in row.iter_mut().enumerate() {
            let t = if j == 3 { a[i][3] } else { 0.0 };
            *v = (0..3).map(|k| a[i][k] * b[k][j]).sum::<f32>() + t;
        }
    }
    r
}

/// Linear part within `lin`, translation column within `tr`.
fn close(a: &M, b: &M, lin: f32, tr: f32) {
    for i in 0..3 {
        for j in 0..4 {
            let tol = if j == 3 { tr } else { lin };
            assert!((a[i][j] - b[i][j]).abs() <= tol, "[{i}][{j}] {a:?} vs {b:?}");
        }
    }
}

fn graph_of(bytes: &[u8]) -> nif::graph::NodeGraph {
    read_graph(&Header::parse(bytes).unwrap(), bytes)
}

/// Bounding sphere of `positions × m(shape)` over the shapes (centre = box centre, the renderer's rule).
fn sphere(p: &ParsedNif, m: impl Fn(&ParsedShape) -> M) -> ([f32; 3], f32) {
    let (mut lo, mut hi) = ([f32::MAX; 3], [f32::MIN; 3]);
    let mut pts = vec![];
    for s in &p.shapes {
        assert!(!s.material.skinned, "{}", s.name);
        let t = m(s);
        for v in &s.mesh.positions {
            let q = [0, 1, 2].map(|r| t[r][0] * v[0] + t[r][1] * v[1] + t[r][2] * v[2] + t[r][3]);
            for k in 0..3 {
                lo[k] = lo[k].min(q[k]);
                hi[k] = hi[k].max(q[k]);
            }
            pts.push(q);
        }
    }
    let c = [0, 1, 2].map(|k| (lo[k] + hi[k]) / 2.0);
    let r = pts
        .iter()
        .map(|p| ((p[0] - c[0]).powi(2) + (p[1] - c[1]).powi(2) + (p[2] - c[2]).powi(2)).sqrt())
        .fold(0.0f32, f32::max);
    (c, r)
}

fn dist(a: [f32; 3], b: [f32; 3]) -> f32 {
    (0..3).map(|k| (a[k] - b[k]).powi(2)).sum::<f32>().sqrt()
}

#[test]
fn deep_graph_every_lighting_shape_chains_to_block_zero() {
    let b = fixture("TestNifFile_DeepGraph_SE.nif");
    let p = parse(&b);
    let g = graph_of(&b);
    assert!(g.problems.is_empty(), "{:?}", g.problems);
    assert_eq!(p.shapes.len(), 27);
    assert!(
        p.warnings.iter().all(|w| !w.contains("not under any known node")),
        "{:?}",
        p.warnings
    );
    for s in &p.shapes {
        let (mut cur, mut hops) = (s.id.0, 0);
        while let Some(&up) = g.parent.get(&cur) {
            cur = up;
            hops += 1;
            assert!(hops <= 256, "{}", s.name);
        }
        assert!(cur == 0 && hops > 0, "{} must hang under block 0", s.name);
    }
}

/// Golden: world matrix of node 77 (SkullPeg01, the parent of shape 143) = root · … · node 77, computed outside
/// Rust from the fixture's 14 node blocks in float64. The rotations cancel to the identity; the translation is
/// (48.22637, -0.15830, 327.96471). Composing in the wrong order gives translation (5.8, 227.7, 360.1).
const W77: M = [
    [1.0, 0.0, 0.0, 48.226_37],
    [0.0, 1.0, 0.0, -0.158_298_83],
    [0.0, 0.0, 1.0, 327.964_7],
];

#[test]
fn deep_graph_skull_peg_world_matches_the_chain_golden() {
    let b = fixture("TestNifFile_DeepGraph_SE.nif");
    let p = parse(&b);
    let g = graph_of(&b);
    assert_eq!(g.parent[&143], 77);
    close(&world(&g, 143, &ID), &W77, 1e-4, 0.02);
    let s = p.shapes.iter().find(|s| s.id == ShapeId(143)).unwrap();
    close(&s.world, &mul(&W77, &s.mesh.transform), 1e-3, 0.05);
    assert_ne!(s.world, s.mesh.transform, "the local transform alone is the P1-A placement");
}

#[test]
fn deep_graph_bounds_follow_the_world_transforms() {
    let b = fixture("TestNifFile_DeepGraph_SE.nif");
    let p = parse(&b);
    let g = graph_of(&b);
    let cache = tempfile::tempdir().unwrap();
    let mut r = renderer(cache.path(), FLAT);
    let rep = r.load(&b, &|_| None);
    assert_eq!(rep.shapes, 27, "{rep:?}");
    let (c, rad) = r.bounds().unwrap();
    let (wc, wr) = sphere(&p, |s| s.world);
    let (lc, _) = sphere(&p, |s| s.mesh.transform);
    // the chain composed child-first instead of root-first: what a wrong composition order would give
    let (rc, _) = sphere(&p, |s| {
        let (mut m, mut cur) = (s.mesh.transform, s.id.0);
        while let Some(&up) = g.parent.get(&cur) {
            m = mul(&m, &g.local[&up]);
            cur = up;
        }
        m
    });
    assert!(dist(c, wc) < 0.01, "centre {c:?} vs {wc:?}");
    assert!((rad - wr).abs() < 0.01 * wr.max(1.0), "{rad} vs {wr}");
    assert!(dist(c, lc) > 1.0, "bounds must differ from the local-only P1-A bounds");
    assert!(dist(c, rc) > 1.0, "bounds must differ from the reverse-order composition");
}

/// Furniture: node 3 "Furniture" is the only non-identity node, a pure +85.12416 translation in y above shapes 9, 12.
#[test]
fn furniture_shapes_sit_under_the_raised_node() {
    let b = fixture("TestNifFile_Furniture_Col_SE.nif");
    let p = parse(&b);
    let g = graph_of(&b);
    assert_eq!((g.parent[&9], g.parent[&12], g.parent[&3]), (3, 3, 0));
    assert_eq!(g.local[&3][1][3], 85.124_16);
    assert_eq!(p.shapes.len(), 2);
    for s in &p.shapes {
        let mut want = s.mesh.transform;
        want[1][3] += 85.124_16;
        close(&s.world, &want, 1e-6, 1e-3);
    }
    // bounds: pure translation, so the centre moves by 85.12416 in y and the radius does not change
    let cache = tempfile::tempdir().unwrap();
    let mut r = renderer(cache.path(), FLAT);
    assert_eq!(r.load(&b, &|_| None).shapes, 2);
    let (c, rad) = r.bounds().unwrap();
    let (lc, lr) = sphere(&p, |s| s.mesh.transform);
    assert!((c[1] - lc[1] - 85.124_16).abs() < 0.01, "{c:?} vs local {lc:?}");
    assert!((c[0] - lc[0]).abs() < 0.01 && (c[2] - lc[2]).abs() < 0.01);
    assert!((rad - lr).abs() < 0.01 * lr.max(1.0), "{rad} vs {lr}");
}

/// Identity node chains leave world == local exactly, so these fixtures draw as before (the skinned VS ignores
/// World anyway).
#[test]
fn identity_chains_leave_world_equal_to_local() {
    let mut n = 0;
    for f in [
        "TestNifFile_Skinned_SE.nif",
        "TestNifFile_Skinned_Dynamic_SE.nif",
        "TestNifFile_Optimize_LE_to_SE.nif",
    ] {
        for s in parse(&fixture(f)).shapes {
            assert_eq!(s.world, s.mesh.transform, "{f} {}", s.name);
            n += 1;
        }
    }
    assert!(n > 0);
}

/// Unknown node types are reported, not fatal: rename NiNode to NiNodX (same length) so block 3 is no longer a
/// known node; its shapes lose their parent, keep their own transform and are warned about.
#[test]
fn unknown_node_type_unparents_its_shapes_with_a_warning() {
    let mut b = fixture("TestNifFile_Furniture_Col_SE.nif");
    let pat = *b"\x06\0\0\0NiNode";
    let at: Vec<usize> = (0..b.len() - pat.len())
        .filter(|&i| b[i..i + pat.len()] == pat)
        .collect();
    assert_eq!(at.len(), 1, "the type table names NiNode once");
    b[at[0] + pat.len() - 1] = b'X';
    let p = parse(&b);
    assert!(p.errors.is_empty(), "{:?}", p.errors);
    assert_eq!(p.shapes.len(), 2);
    for s in &p.shapes {
        assert_eq!(s.world, s.mesh.transform, "{}", s.name);
    }
    let orphan: Vec<&String> = p
        .warnings
        .iter()
        .filter(|w| w.contains("not under any known node"))
        .collect();
    assert_eq!(
        orphan,
        [
            "cylinder_1: not under any known node; placed by its own transform",
            "cylinder_2: not under any known node; placed by its own transform"
        ]
    );
}
```

- [ ] **Step 2: run, expect failure.** `cargo test -p render3d --test transforms` -> compile error `no field world on type &ParsedShape`.

- [ ] **Step 3: parse fills `world`.** In `crates/render3d/src/parse.rs`:

(a) In the module doc, replace the sentence `P1-A places each shape by its own transform (`Mesh.transform`) only; parent NiNode transforms are P1-B.` (it wraps over two `//!` lines) with: `A shape's `world` is the root..parent NiNode chain x its own transform (`Mesh.transform`, local); a block with no parent entry keeps its own transform.`

(b) In `ParsedShape`, after `pub mesh: Arc<nif::geom::Mesh>,` add:

```rust
    /// Root … parent chain × `mesh.transform` (3×4 row-major, the `Mesh.transform` convention). The draw's model
    /// matrix and the bounds use it. Skinned shapes carry it too, but their VS ignores World.
    pub world: [[f32; 4]; 3],
```

(c) After the `let header = match nif::header::Header::parse(nif_bytes) { ... };` statement add:

```rust
    let graph = nif::graph::read_graph(&header, nif_bytes);
    out.warnings
        .extend(graph.problems.iter().map(|p| format!("NIF: {p}")));
```

(d) Immediately before `out.shapes.push(ParsedShape {` add:

```rust
        if row.block != 0 && !graph.parent.contains_key(&row.block) {
            out.warnings.push(format!(
                "{label}: not under any known node; placed by its own transform"
            ));
        }
        let world = nif::graph::world(&graph, row.block, &mesh.transform);
```

and add `world,` after `name: label,` in the struct literal.

- [ ] **Step 4: the renderer uses it.** In `crates/render3d/src/lib.rs`:

(a) `ShapeState` (anchor `struct ShapeState {`): add `world: [[f32; 4]; 3],` after `mesh: Arc<nif::geom::Mesh>,`.

(b) `build_draw`: add a parameter `world: &[[f32; 4]; 3],` after `mesh: &nif::geom::Mesh,`; in its returned `ShapeDraw { ... }` replace `model: mesh.transform,` with `model: *world,`.

(c) `load_parsed`: change `self.build_draw(&s.name, &s.mesh, &s.material, textures, &mut rep)` to `self.build_draw(&s.name, &s.mesh, &s.world, &s.material, textures, &mut rep)`; in the `ShapeState { name: s.name.clone(), mesh: s.mesh.clone(), ...` literal add `world: s.world,` after the `mesh:` line.

(d) `set_material`, rebuild branch: change

```rust
            let (name, mesh) = {
                let s = &self.shapes[&id];
                (s.name.clone(), s.mesh.clone())
            };
            match self.build_draw(&name, &mesh, &m, textures, &mut scratch) {
```

to

```rust
            let (name, mesh, world) = {
                let s = &self.shapes[&id];
                (s.name.clone(), s.mesh.clone(), s.world)
            };
            match self.build_draw(&name, &mesh, &world, &m, textures, &mut scratch) {
```

(e) `load_builtin`: before `let parsed = ParsedNif {` add `let mesh = builtin_mesh(b);` and make the shape literal

```rust
            shapes: vec![ParsedShape {
                id: ShapeId(0),
                name: format!("{b:?}"),
                world: mesh.transform,
                mesh: Arc::new(mesh),
                material,
            }],
```

(f) `compute_bounds`: replace `let t = &s.mesh.transform;` with `let t = &s.world;`.

(g) Doc comment of `Renderer::bounds`: replace `positions × transform` ... `Parent NiNode transforms are not applied in P1-A.` with: `positions × world` (the parent NiNode chain × the shape's own transform; skinned shapes: positions as-is, since the skinned VS ignores World and P0 bones are identity). Material edits do not move it.`

- [ ] **Step 5: run, expect pass.** `cargo test -p render3d --test transforms` -> 6 tests pass. If `deep_graph_every_lighting_shape_chains_to_block_zero` fails on the warnings assertion, print `p.warnings`: a real problem line is the diagnosis, not a test bug (the fixture was checked problem-free with an independent parser).

- [ ] **Step 6: whole suite (the "existing tests stay green" proof).** `cargo test -p render3d`, then the full gates. Expected: all green. Byte-identical: built-ins and the Skinned fixtures (asserted structurally by `identity_chains_leave_world_equal_to_local`). Furniture and DeepGraph frames move on purpose, and every existing assertion on them is relative (see "Verified" above), so none should need an edit. If one fails because it asserted something absolute about placement, stop and report which one instead of editing it.

- [ ] **Step 7: commit.**
```
cargo fmt --all && cargo clippy --workspace --all-targets -- -D warnings && cargo test --workspace
git add crates/render3d/src/parse.rs crates/render3d/src/lib.rs crates/render3d/tests/transforms.rs
git commit -m "render3d: place shapes by their parent NiNode chain (ParsedShape.world); bounds and draw use it

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```


---

# Part C: compile cache and background compile (Tasks 3-6)

Contract: `00-contract.md`. Code base: worktree `F:\ai\dds\converter-nif-p1` @ `8159bdf` (execution happens in `F:\ai\dds\converter-nif-p1b`, branch `nif/b-p1b`). Every name below was checked against that commit; line numbers are hints, text anchors rule.

## SHARED-FILE EDITS NEEDED

None. (`crates/render3d/Cargo.toml` gets two `windows` features; render3d is a nif-team crate, not a shared file. `Cargo.lock` does not change: features of the `windows` crate add no dependency.)

## CONTRACT ISSUES

1. `ProgramKey` and `CompiledPair` do not exist at `8159bdf` (the key is a private `fn key(..) -> String` in `compile.rs`; the pair is the `vs`/`ps` fields of `draw::Program`). Both are created in `compile.rs`. `program_key` returns a per-stage `ProgramKey` (the disk cache stays per stage, `<key>.cso/.json`); the pair key used by `CompileJob`/`CompileDone`/`compiled`/`programs` is `ProgramKey::pair(&vs_key, &ps_key)`.
2. The contract says `load_parsed` pushes jobs and only `load` runs them. About 40 existing call sites (`tests/edit.rs`, `builtin.rs`, `frame.rs`, `lights.rs`, `load.rs`, gui) call `load_parsed` / `load_builtin` / `set_material` and expect a settled result. Added `Renderer::set_async_compile(bool)` (default `false`): in sync mode those three finish by running their own queued jobs (`Renderer::settle`), so every existing test is unchanged; the GUI render thread turns async on. `load` always settles, as the contract says.
3. `apply_compiled -> bool` is implemented as "a shape's draw or status changed" (a superset of "on screen changed"): `Pending -> Failed` with no previous draw changes no pixel but the panel must be told.
4. The GUI rebuilds a lost device by making a new `Renderer`, which would drop `compiled`. Added `Renderer::rebuild_device_on(&mut self, gpu: Gpu)`, which keeps `compiled`, features and mode (T-E7 tests it).
5. Two additions the contract does not list, both in `model/viewport3d.rs`: `V3Reply::Refresh` (a compile-triggered frame must not advance `render_done`, and must carry an id that differs from every model-issued id or the UI skips the upload) and `V3Cmd::Quit` (the pool sink holds a sender to the loop's own queue, so `recv()` never errors when the model drops its sender any more; `Drop for Viewport3d` sends `Quit`).
6. Textures: `apply_compiled` has no resolver, so every slot that maps to a register (`textures::register_for_slot`) is resolved at load/edit time, not only the slots the PS reflects. `missing_textures` can therefore list a mapped slot the PS does not read (before: only PS-read slots). No existing assertion depends on the old subset (checked: `standin.rs:53` is an `any`, `builtin.rs:322` is `is_empty` on a fully-provided material).
7. Task 2 (part T) changes the model matrix and bounds from `mesh.transform` to the shape's `world`. Where Task 5's code shows `model: mesh.transform` / `s.mesh.transform` it is marked `// Task 2:`; keep whatever Task 2 wrote there (a `world` field on `ShapeState` passed into `build_draw`). Nothing else in Task 5 depends on the transform.
8. The first `SourceSet::get` (cold memo) expands `Lighting.hlsl` on the thread that asks for the key (the render thread): the key needs the hash before a job exists. It is memoised, a few ms to tens of ms once per shaders folder. The compiles themselves are on the workers.
9. `LoadReport.compile_errors` is empty for an async load (failures arrive in `Shapes`), as design §1.4 says; sync loads fill it as before.

---

## Task 3: `source.rs` (`SourceSet`, `ExpandedSource`) and the content-addressed `ProgramKey`

**Files:**
- Create: `crates/render3d/src/source.rs`
- Modify: `crates/render3d/src/includes.rs` (`#line` names root-relative)
- Modify: `crates/render3d/src/compile.rs` (`ProgramKey`, `program_key`, `compiler_id`, `compile`/`cached` signatures; old `key()` removed)
- Modify: `crates/render3d/src/lib.rs` (`pub mod source`, `Renderer.sources`, `programs` keyed by `ProgramKey`, `program()`, `set_shaders`, `permutation_key` removed)
- Modify: `crates/render3d/Cargo.toml` (+ feature `Win32_System_LibraryLoader`)
- Modify: `crates/render3d/tests/compile.rs` (call helper)
- Test: `crates/render3d/tests/source.rs` (new)

**Interfaces:**
- Consumes: `includes::expand`, `permutation::Defines`, the existing `read_cache` / `write_cache`.
- Produces:
  ```rust
  // source.rs
  pub struct ExpandedSource { pub text: Arc<str>, pub sha256: [u8; 32], pub files: Vec<PathBuf> }
  pub struct SourceSet { /* root, memo */ }
  impl SourceSet { pub fn new(root: PathBuf) -> SourceSet; pub fn get(&self, rel: &Path) -> Result<Arc<ExpandedSource>, String>; }
  // compile.rs
  #[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)] pub struct ProgramKey(pub String);
  impl ProgramKey { pub fn pair(vs: &ProgramKey, ps: &ProgramKey) -> ProgramKey }
  pub fn program_key(src: &ExpandedSource, entry: &str, target: &str, defines: &[(String, String)]) -> ProgramKey;
  pub fn compiler_id() -> &'static str;      // loaded d3dcompiler_47.dll file version "10.1.x.y"
  pub fn cached(src: &ExpandedSource, d: &Defines, target: &str, cache_dir: &Path) -> Option<Compiled>;   // disk hit only
  pub fn compile(src: &ExpandedSource, d: &Defines, target: &str, cache_dir: &Path) -> Result<Compiled, String>;  // was (file, root, d, target, cache_dir, root_hash)
  ```
- Why `compiler_id` uses the file version: `GetModuleHandleW("d3dcompiler_47.dll")` + `GetModuleFileNameW` finds the DLL the process actually loaded (an app-local copy beats System32), `GetFileVersionInfoW` / `VerQueryValueW` read `VS_FIXEDFILEINFO`. All of it is in the `windows` crate already used; the only change is the `Win32_System_LibraryLoader` feature (`Win32_Storage_FileSystem`, which holds the version-info calls, is already on). If the query fails the id is `"unknown"` (never cached to disk differently; the key still changes with the sources), and the test below fails loudly on a machine where it happens.

The expansion must not contain the root path, or two folders with identical sources would hash differently. `includes::go` writes `#line N "<absolute path>"` today; it now writes the path relative to the first search root when it is under it.

- [ ] **Step 1: Write the failing tests** — create `crates/render3d/tests/source.rs`:

```rust
//! 03 T-C1: the shader cache key names the bytes that were compiled, not where the folder is.
use render3d::compile::{compiler_id, program_key, ProgramKey};
use render3d::source::SourceSet;
use std::fs;
use std::path::Path;
use std::sync::Arc;

fn tree(root: &Path, a: &str) {
    fs::create_dir_all(root.join("Common")).unwrap();
    fs::write(
        root.join("Lighting.hlsl"),
        "#include \"Common/A.hlsli\"\nfloat4 main() : SV_Target { return A; }\n",
    )
    .unwrap();
    fs::write(
        root.join("Common/A.hlsli"),
        format!("static const float4 A = {a};\n"),
    )
    .unwrap();
}

fn src(root: &Path) -> Arc<render3d::source::ExpandedSource> {
    SourceSet::new(root.to_path_buf())
        .get(Path::new("Lighting.hlsl"))
        .unwrap()
}

fn key(root: &Path, defines: &[(&str, &str)]) -> ProgramKey {
    let d: Vec<(String, String)> = defines
        .iter()
        .map(|(k, v)| (k.to_string(), v.to_string()))
        .collect();
    program_key(&src(root), "main", "ps_5_0", &d)
}

#[test]
fn key_ignores_the_root_path() {
    let (a, b) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
    tree(a.path(), "1.0");
    tree(b.path(), "1.0");
    assert_ne!(a.path(), b.path());
    assert_eq!(src(a.path()).sha256, src(b.path()).sha256);
    assert_eq!(key(a.path(), &[("X", "1")]), key(b.path(), &[("X", "1")]));
    let shown = a.path().display().to_string().replace('\\', "/");
    assert!(
        !src(a.path()).text.contains(&shown),
        "the expansion names files relative to the root"
    );
    assert!(src(a.path()).text.contains("#line 1 \"Common/A.hlsli\""));
    assert_eq!(src(a.path()).files.len(), 2);
}

#[test]
fn editing_an_included_file_changes_the_key() {
    let d = tempfile::tempdir().unwrap();
    tree(d.path(), "1.0");
    let before = key(d.path(), &[]);
    fs::write(
        d.path().join("Common/A.hlsli"),
        "static const float4 A = 0.5;\n",
    )
    .unwrap();
    assert_ne!(key(d.path(), &[]), before);
}

#[test]
fn define_order_does_not_matter_but_values_entry_and_target_do() {
    let d = tempfile::tempdir().unwrap();
    tree(d.path(), "1.0");
    let ab = key(d.path(), &[("A", "1"), ("B", "2")]);
    assert_eq!(ab, key(d.path(), &[("B", "2"), ("A", "1")]));
    assert_ne!(ab, key(d.path(), &[("A", "1"), ("B", "3")]));
    assert_ne!(ab, key(d.path(), &[("A", "1")]));
    let s = src(d.path());
    let defs = vec![("A".to_string(), "1".to_string())];
    let ps = program_key(&s, "main", "ps_5_0", &defs);
    assert_ne!(ps, program_key(&s, "main", "vs_5_0", &defs));
    assert_ne!(ps, program_key(&s, "other", "ps_5_0", &defs));
    let pair = ProgramKey::pair(&ps, &program_key(&s, "main", "vs_5_0", &defs));
    assert_ne!(pair, ps);
    assert_eq!(pair.0.len(), 64);
}

#[test]
fn the_expansion_is_memoised_until_the_set_is_replaced() {
    let d = tempfile::tempdir().unwrap();
    tree(d.path(), "1.0");
    let set = SourceSet::new(d.path().to_path_buf());
    let first = set.get(Path::new("Lighting.hlsl")).unwrap();
    fs::write(d.path().join("Common/A.hlsli"), "static const float4 A = 2.0;\n").unwrap();
    let again = set.get(Path::new("Lighting.hlsl")).unwrap();
    assert!(Arc::ptr_eq(&first, &again), "memo hit: no second expansion");
    let fresh = SourceSet::new(d.path().to_path_buf())
        .get(Path::new("Lighting.hlsl"))
        .unwrap();
    assert_ne!(fresh.sha256, first.sha256, "a new set sees the edit");
}

#[test]
fn a_missing_entry_file_is_an_error() {
    let d = tempfile::tempdir().unwrap();
    let e = SourceSet::new(d.path().to_path_buf()).get(Path::new("Nope.hlsl"));
    assert!(e.is_err());
}

/// The id is the loaded d3dcompiler_47.dll's file version; "unknown" means the query failed.
#[test]
fn compiler_id_is_the_loaded_dlls_file_version() {
    let id = compiler_id();
    assert_ne!(id, "unknown");
    let parts: Vec<&str> = id.split('.').collect();
    assert_eq!(parts.len(), 4, "{id}");
    assert!(parts.iter().all(|p| p.parse::<u32>().is_ok()), "{id}");
}
```

- [ ] **Step 2: Run to verify RED**

Run: `cargo test -p render3d --test source`
Expected: FAIL to compile: `unresolved import render3d::source` (and `compiler_id`, `program_key`, `ProgramKey` not found in `render3d::compile`).

- [ ] **Step 3: `Cargo.toml`** — in `crates/render3d/Cargo.toml` change the `windows` line's feature list by appending `"Win32_System_LibraryLoader"` (keep the others):

```toml
windows = { version = "0.62.2", features = ["Win32_Graphics_Direct3D", "Win32_Graphics_Direct3D11", "Win32_Graphics_Dxgi", "Win32_Graphics_Dxgi_Common", "Win32_Graphics_Direct3D_Fxc", "Win32_Foundation", "Win32_Storage_FileSystem", "Win32_System_LibraryLoader"] }
```

- [ ] **Step 4: `includes.rs`** — root-relative `#line` names. Replace the helper and its three uses.

Old text anchors → new:

```rust
fn slash(p: &Path) -> String {
    p.display()
        .to_string()
        .replace(std::path::MAIN_SEPARATOR, "/")
}
```
becomes
```rust
fn slash(p: &Path) -> String {
    p.display()
        .to_string()
        .replace(std::path::MAIN_SEPARATOR, "/")
}

/// The name written into `#line`: relative to `root` when `p` is under it, so the expansion (and the hash of
/// it) is the same wherever the folder sits on disk.
fn shown(p: &Path, root: Option<&Path>) -> String {
    slash(root.and_then(|r| p.strip_prefix(r).ok()).unwrap_or(p))
}
```
In `fn go`, after `out.files.push(file.to_path_buf());` replace `let fname = slash(file);` with:

```rust
    let root = search.first().map(PathBuf::as_path);
    let fname = shown(file, root);
```
and replace the two uses `slash(&found)` with `shown(&found, root)`:

```rust
                out.text
                    .push_str(&format!("#line 1 \"{}\"\n", shown(&found, root)));
```
(`slash` stays: `shown` uses it.) `tests/includes.rs` keeps passing: it asserts `e.text.contains("#line 1 \"")` only.

- [ ] **Step 5: `source.rs`** — create `crates/render3d/src/source.rs`:

```rust
//! The include-expanded entry file (`Lighting.hlsl`) of one CS `Shaders` folder, expanded once and memoised,
//! with the SHA-256 of the expansion: the content half of the shader cache key (03 §5). The expansion does not
//! depend on the defines (the expander follows every `#include`, also inside `#if 0`), so one `ExpandedSource`
//! serves every permutation. The memo lives as long as the `SourceSet`: `Renderer::set_shaders` (a moved or
//! updated CS, which the GUI sends for `Change::Updated` too) makes a new one.
use crate::includes::expand;
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, PoisonError};

/// One entry file with every include inlined.
pub struct ExpandedSource {
    pub text: Arc<str>,
    /// SHA-256 of `text`. `#line` names are root-relative, so this does not depend on where the folder is.
    pub sha256: [u8; 32],
    /// The entry file first, then every included file found, as absolute paths.
    pub files: Vec<PathBuf>,
}

pub struct SourceSet {
    root: PathBuf,
    memo: Mutex<HashMap<PathBuf, Arc<ExpandedSource>>>,
}

impl SourceSet {
    pub fn new(root: PathBuf) -> SourceSet {
        SourceSet {
            root,
            memo: Mutex::new(HashMap::new()),
        }
    }

    /// `rel` (root-relative, e.g. `Lighting.hlsl`) expanded, from the memo after the first call. Errors are
    /// text (`path: io error`), not memoised.
    pub fn get(&self, rel: &Path) -> Result<Arc<ExpandedSource>, String> {
        let mut memo = self.memo.lock().unwrap_or_else(PoisonError::into_inner);
        if let Some(e) = memo.get(rel) {
            return Ok(e.clone());
        }
        let path = self.root.join(rel);
        let e = expand(&path, std::slice::from_ref(&self.root))
            .map_err(|e| format!("{}: {e}", path.display()))?;
        let sha256: [u8; 32] = Sha256::digest(e.text.as_bytes()).into();
        let out = Arc::new(ExpandedSource {
            text: e.text.into(),
            sha256,
            files: e.files,
        });
        memo.insert(rel.to_path_buf(), out.clone());
        Ok(out)
    }
}
```

- [ ] **Step 6: `compile.rs`** — new key, compiler id, new `compile` / `cached`.

(a) Header and imports. Replace the first doc line `//! D3DCompile of the include-expanded CS source, reflection into plain data, and an on-disk cache keyed by` and the following line with:

```rust
//! D3DCompile of the include-expanded CS source, reflection into plain data, and an on-disk cache keyed by the
//! bytes that were compiled (expanded source hash, entry, target, defines, flags, reflection version, compiler
//! version): see [`program_key`].
```
Replace `use crate::includes::expand;` with `use crate::source::ExpandedSource;`. Replace `use std::path::Path;` with `use std::path::Path;\nuse std::sync::OnceLock;`.

(b) Replace the whole `fn key(...)` (from `fn key(file: &Path, root: &Path, d: &Defines, target: &str, root_hash: &str) -> String {` through its closing brace) with:

```rust
/// Names one compiled stage by what was compiled. The root path and the saved fingerprint are not part of it:
/// two CS folders with identical sources share entries, and a stale fingerprint cannot file a new compile
/// under an old name.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct ProgramKey(pub String);

impl ProgramKey {
    /// The key of a VS + PS pair (the unit the renderer caches in memory).
    pub fn pair(vs: &ProgramKey, ps: &ProgramKey) -> ProgramKey {
        let mut h = Sha256::new();
        h.update(vs.0.as_bytes());
        h.update([0]);
        h.update(ps.0.as_bytes());
        ProgramKey(hex::encode(h.finalize()))
    }
}

/// `sha256(expanded.sha256 ‖ entry ‖ target ‖ sorted defines ‖ FLAGS ‖ REFLECT_VERSION ‖ compiler_id)`.
pub fn program_key(
    src: &ExpandedSource,
    entry: &str,
    target: &str,
    defines: &[(String, String)],
) -> ProgramKey {
    let mut sorted: Vec<&(String, String)> = defines.iter().collect();
    sorted.sort();
    let mut h = Sha256::new();
    h.update(src.sha256);
    h.update([0]);
    h.update(entry);
    h.update([0]);
    h.update(target);
    h.update([0]);
    for (k, v) in sorted {
        h.update(format!("{k}={v};"));
    }
    h.update([0]);
    h.update(FLAGS.to_le_bytes());
    h.update(REFLECT_VERSION.to_le_bytes());
    h.update(compiler_id());
    ProgramKey(hex::encode(h.finalize()))
}

/// The file version (`10.1.x.y`) of the d3dcompiler_47.dll this process loaded: a Windows update replaces the
/// DLL and its output can change, so it is part of the key. `"unknown"` when the query fails.
pub fn compiler_id() -> &'static str {
    static ID: OnceLock<String> = OnceLock::new();
    ID.get_or_init(|| dll_version().unwrap_or_else(|| "unknown".to_string()))
}

fn dll_version() -> Option<String> {
    use windows::core::{w, PCWSTR};
    use windows::Win32::Storage::FileSystem::{
        GetFileVersionInfoSizeW, GetFileVersionInfoW, VerQueryValueW, VS_FIXEDFILEINFO,
    };
    use windows::Win32::System::LibraryLoader::{
        GetModuleFileNameW, GetModuleHandleW, LoadLibraryW,
    };
    // SAFETY (whole block): `path` is zero-initialised and longer than the returned length, so it is NUL
    // terminated; `buf` is as long as the size the API asked for; `VerQueryValueW` points into `buf`, which
    // outlives the unaligned read of the fixed-size struct.
    unsafe {
        let module = GetModuleHandleW(w!("d3dcompiler_47.dll"))
            .or_else(|_| LoadLibraryW(w!("d3dcompiler_47.dll")))
            .ok()?;
        let mut path = [0u16; 520];
        let n = GetModuleFileNameW(Some(module), &mut path) as usize;
        if n == 0 || n >= path.len() {
            return None;
        }
        let name = PCWSTR(path.as_ptr());
        let size = GetFileVersionInfoSizeW(name, None);
        if size == 0 {
            return None;
        }
        let mut buf = vec![0u8; size as usize];
        GetFileVersionInfoW(name, None, size, buf.as_mut_ptr().cast()).ok()?;
        let mut p: *mut core::ffi::c_void = std::ptr::null_mut();
        let mut len = 0u32;
        let ok = VerQueryValueW(buf.as_ptr().cast(), w!("\\"), &mut p, &mut len);
        if !ok.as_bool() || p.is_null() || (len as usize) < std::mem::size_of::<VS_FIXEDFILEINFO>()
        {
            return None;
        }
        let info = p.cast::<VS_FIXEDFILEINFO>().read_unaligned();
        let (ms, ls) = (info.dwFileVersionMS, info.dwFileVersionLS);
        Some(format!("{}.{}.{}.{}", ms >> 16, ms & 0xffff, ls >> 16, ls & 0xffff))
    }
}
```

(c) Replace the whole `pub fn compile(...)` signature and its first lines up to (not including) `let names =` with the following, and adjust the body as shown:

Old:
```rust
pub fn compile(
    file: &Path,
    root: &Path,
    d: &Defines,
    target: &str,
    cache_dir: &Path,
    root_hash: &str,
) -> Result<Compiled, String> {
    let k = key(file, root, d, target, root_hash);
    let (cso, js) = (
        cache_dir.join(format!("{k}.cso")),
        cache_dir.join(format!("{k}.json")),
    );
    if let Some(hit) = read_cache(&cso, &js) {
        return Ok(hit);
    }
    let src =
        expand(file, &[root.to_path_buf()]).map_err(|e| format!("{}: {e}", file.display()))?;
    let names =
```
New:
```rust
/// The cache entry paths of `k`: (`.cso`, `.json`).
fn entry_paths(cache_dir: &Path, k: &ProgramKey) -> (std::path::PathBuf, std::path::PathBuf) {
    (
        cache_dir.join(format!("{}.cso", k.0)),
        cache_dir.join(format!("{}.json", k.0)),
    )
}

/// A disk-cache hit for this stage, if there is one (a missing, truncated or torn pair is a miss).
pub fn cached(src: &ExpandedSource, d: &Defines, target: &str, cache_dir: &Path) -> Option<Compiled> {
    let k = program_key(src, "main", target, &d.0);
    let (cso, js) = entry_paths(cache_dir, &k);
    read_cache(&cso, &js)
}

/// Compiles `src` (entry `main`) for `target` with `d`, or returns the disk-cache entry.
pub fn compile(
    src: &ExpandedSource,
    d: &Defines,
    target: &str,
    cache_dir: &Path,
) -> Result<Compiled, String> {
    if let Some(hit) = cached(src, d, target, cache_dir) {
        return Ok(hit);
    }
    let k = program_key(src, "main", target, &d.0);
    let (cso, js) = entry_paths(cache_dir, &k);
    let names =
```
Then in the same function replace the line
```rust
    let src_name = cstr(&file.display().to_string().replace('\\', "/"))?;
```
with
```rust
    let src_name = cstr(
        &src.files
            .first()
            .map_or_else(|| "shader.hlsl".to_string(), |p| p.display().to_string())
            .replace('\\', "/"),
    )?;
```
(`src.text.as_ptr()` / `src.text.len()` in the `D3DCompile` call keep working on `Arc<str>`.)

- [ ] **Step 7: `lib.rs`** (Task 3 part; Task 5 reworks these again)

- After `pub mod includes;` add `pub mod source;`.
- Replace `use compile::{compile, SrvKind};` with `use compile::{compile, program_key, ProgramKey, SrvKind};`.
- Replace `use std::path::PathBuf;` with `use std::path::{Path, PathBuf};` and add `use source::SourceSet;` after the `use scene::...` line.
- In `struct Renderer`: replace `programs: HashMap<String, Result<Program, String>>,` with
  ```rust
      /// One expanded `Lighting.hlsl` per shaders folder (03 §5); dropped with the folder by `set_shaders`.
      sources: SourceSet,
      programs: HashMap<ProgramKey, Result<Program, String>>,
  ```
- Delete `fn permutation_key(...)` (the 5-line fn above `impl Renderer`).
- In `with_gpu`'s `Ok(Renderer { ... })`, before the line `shaders,` insert `sources: SourceSet::new(shaders.root.clone()),`.
- Replace `set_shaders`:
  ```rust
      pub fn set_shaders(&mut self, shaders: Fingerprint) {
          self.sources = SourceSet::new(shaders.root.clone());
          self.shaders = shaders;
          self.programs.clear();
      }
  ```
  and fix its doc comment: `/// Replaces the shaders (a moved or updated CS): the expansion memo and every compiled program are dropped. The loaded model still uses the old programs until the next [`Renderer::load`].`
- Replace the whole `fn program(&mut self, m: &Material)` with:
  ```rust
      fn program(&mut self, m: &Material) -> Result<Program, String> {
          let f: Vec<&str> = self.features.iter().map(String::as_str).collect();
          let vd = defines(m, Stage::Vertex, &f, false);
          let pd = defines(m, Stage::Pixel, &f, false);
          let src = self.sources.get(Path::new("Lighting.hlsl"))?;
          let key = ProgramKey::pair(
              &program_key(&src, "main", "vs_5_0", &vd.0),
              &program_key(&src, "main", "ps_5_0", &pd.0),
          );
          if let Some(p) = self.programs.get(&key) {
              return p.clone();
          }
          let cache = &self.cache_dir;
          let built = compile(&src, &vd, "vs_5_0", cache)
              .map_err(|e| format!("vs: {e}"))
              .and_then(|vs| {
                  compile(&src, &pd, "ps_5_0", cache)
                      .map_err(|e| format!("ps: {e}"))
                      .map(|ps| (vs, ps))
              })
              .and_then(|(vs, ps)| create_program(&self.gpu, vs, ps));
          self.programs.insert(key, built.clone());
          built
      }
  ```

- [ ] **Step 8: `tests/compile.rs`** — the old 6-argument `compile` becomes a local helper so no call site changes. Replace `use render3d::compile::compile;` (line 1) and the `use render3d::permutation::{...}` line with:

```rust
use render3d::compile::Compiled;
use render3d::permutation::{defines, runtime_bits, Defines, Material, Stage};
use render3d::source::SourceSet;
use std::path::Path;

/// The pre-P1-B call shape (file, root, defines, target, cache, hash): expand `file` under `root`, then
/// `render3d::compile::compile`. The hash argument is ignored: the key is the content now.
fn compile(
    file: &Path,
    root: &Path,
    d: &Defines,
    target: &str,
    cache: &Path,
    _hash: &str,
) -> Result<Compiled, String> {
    let rel = file.strip_prefix(root).unwrap_or(file);
    let src = SourceSet::new(root.to_path_buf()).get(rel)?;
    render3d::compile::compile(&src, d, target, cache)
}
```
(Call sites pass `&Path::new(..)`, `&PathBuf` and `&Default::default()`; all coerce.)

- [ ] **Step 9: Run to verify GREEN**

Run: `cargo test -p render3d --test source --test compile --test includes --test standin --test edit`
Expected: PASS. `compile::tiny_shader_compiles_and_reflects` still sees exactly 2 files in its cache dir; `cache_key_uses_path_relative_to_root` still sees different bytecode for `a/x.hlsl` and `b/x.hlsl` (the content differs, which is what the key now sees).

- [ ] **Step 10: Gates**

Run: `cargo fmt --all && cargo clippy --workspace --all-targets -- -D warnings && cargo test --workspace`
Expected: all green. The first run after this task recompiles every shader (old cache entries are orphans; Task 4's prune removes them after 90 days / over the cap).

- [ ] **Step 11: Commit**

```bash
git add crates/render3d/Cargo.toml crates/render3d/src/source.rs crates/render3d/src/includes.rs crates/render3d/src/compile.rs crates/render3d/src/lib.rs crates/render3d/tests/source.rs crates/render3d/tests/compile.rs
git commit -m "feat(render3d): content-addressed shader cache key (expanded source hash + compiler version), SourceSet memo" -m "Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

---

## Task 4: `cache.rs`: hit touches the entry, startup prune

**Files:**
- Create: `crates/render3d/src/cache.rs`
- Modify: `crates/render3d/src/lib.rs` (`pub mod cache;`, `Renderer::new` starts the prune)
- Modify: `crates/render3d/src/compile.rs` (`cached` touches on a hit)
- Test: `crates/render3d/tests/cache.rs` (new)

**Interfaces:**
- Consumes: Task 3 `cached` / `compile` / `SourceSet`.
- Produces:
  ```rust
  pub fn touch(json: &Path);
  pub struct PruneLimits { pub max_bytes: u64, pub max_age_days: u32, pub stale_tmp_days: u32 }   // Default: 512 MiB, 90, 1
  #[derive(Debug, Clone, Default, PartialEq, Eq)] pub struct PruneReport { pub removed: usize, pub bytes_after: u64 }   // removed = files deleted
  pub fn prune(dir: &Path, limits: &PruneLimits, now: SystemTime) -> PruneReport;
  pub fn prune_in_background(dir: PathBuf);   // once per process, own thread
  ```
- Rules (03 §5): in this order, `.tmp-*` older than `stale_tmp_days`; a `.cso` without a `.json` older than `stale_tmp_days` (a writer may sit between its two renames, so a young orphan stays); entries whose `.json` mtime is older than `max_age_days`; then oldest entries first until the total of what is left is `<= max_bytes`. An entry is removed `.json` first, then `.cso`, so a concurrent reader sees a plain miss, never a torn pair (`read_cache` already treats any mismatch as a miss). Only the top level of the directory is looked at; other files are left alone and not counted.
- "Called once on a background thread at renderer start": `Renderer::new` (the constructor the GUI uses; tests use `with_gpu` and are not affected) calls `cache::prune_in_background(cache_dir)`, which spawns one thread per process (`std::sync::Once`). It runs on the render thread's start, never the UI thread.

- [ ] **Step 1: Write the failing tests** — create `crates/render3d/tests/cache.rs`:

```rust
//! 03 T-C2: pruning is pure filesystem work (tempdir, explicit `now`); a hit refreshes the entry's age.
use render3d::cache::{prune, touch, PruneLimits};
use render3d::compile::{cached, compile};
use render3d::permutation::Defines;
use render3d::source::SourceSet;
use std::fs;
use std::path::Path;
use std::time::{Duration, SystemTime};

const DAY: u64 = 86_400;

fn ago(now: SystemTime, days: u64) -> SystemTime {
    now - Duration::from_secs(days * DAY)
}

fn put(dir: &Path, name: &str, len: usize, mtime: SystemTime) {
    let p = dir.join(name);
    fs::write(&p, vec![7u8; len]).unwrap();
    fs::OpenOptions::new()
        .write(true)
        .open(&p)
        .unwrap()
        .set_modified(mtime)
        .unwrap();
}

/// A cache entry: 1000-byte `.cso` + 100-byte `.json`.
fn entry(dir: &Path, key: &str, mtime: SystemTime) {
    put(dir, &format!("{key}.cso"), 1000, mtime);
    put(dir, &format!("{key}.json"), 100, mtime);
}

fn names(dir: &Path) -> Vec<String> {
    let mut v: Vec<String> = fs::read_dir(dir)
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    v.sort();
    v
}

#[test]
fn over_the_cap_the_oldest_entries_go_first() {
    let d = tempfile::tempdir().unwrap();
    let now = SystemTime::now();
    for (i, key) in ["a", "b", "c", "d"].iter().enumerate() {
        entry(d.path(), key, ago(now, 4 - i as u64)); // a oldest (4 days), d newest (1 day)
    }
    let limits = PruneLimits {
        max_bytes: 2500,
        ..PruneLimits::default()
    };
    let r = prune(d.path(), &limits, now);
    assert_eq!(names(d.path()), ["c.cso", "c.json", "d.cso", "d.json"]);
    assert_eq!((r.removed, r.bytes_after), (4, 2200));
    // Already under the cap: nothing more.
    assert_eq!(prune(d.path(), &limits, now).removed, 0);
}

#[test]
fn entries_untouched_for_90_days_expire() {
    let d = tempfile::tempdir().unwrap();
    let now = SystemTime::now();
    entry(d.path(), "old", ago(now, 91));
    entry(d.path(), "fresh", ago(now, 89));
    let r = prune(d.path(), &PruneLimits::default(), now);
    assert_eq!(names(d.path()), ["fresh.cso", "fresh.json"]);
    assert_eq!((r.removed, r.bytes_after), (2, 1100));
}

#[test]
fn stale_temp_files_and_orphan_cso_go_after_a_day_unrelated_files_stay() {
    let d = tempfile::tempdir().unwrap();
    let now = SystemTime::now();
    put(d.path(), ".tmp-1-0", 50, ago(now, 2));
    put(d.path(), ".tmp-1-1", 50, now);
    put(d.path(), "orphan_old.cso", 500, ago(now, 2));
    put(d.path(), "orphan_young.cso", 500, now); // a writer may be between its renames
    put(d.path(), "notes.txt", 10, ago(now, 400));
    let r = prune(d.path(), &PruneLimits::default(), now);
    assert_eq!(
        names(d.path()),
        [".tmp-1-1", "notes.txt", "orphan_young.cso"]
    );
    assert_eq!(r.removed, 2);
    assert_eq!(r.bytes_after, 50 + 500, "young tmp + young orphan; notes.txt is not counted");
    // A missing directory is not an error.
    assert_eq!(
        prune(&d.path().join("gone"), &PruneLimits::default(), now).removed,
        0
    );
}

fn tiny() -> (SourceSet, Defines) {
    let dir = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures");
    (
        SourceSet::new(dir.into()),
        Defines(vec![("PSHADER".into(), "1".into())]),
    )
}

#[test]
fn a_hit_refreshes_the_entry_age() {
    let cache = tempfile::tempdir().unwrap();
    let (set, d) = tiny();
    let src = set.get(Path::new("tiny.hlsl")).unwrap();
    compile(&src, &d, "ps_5_0", cache.path()).unwrap();
    let json = fs::read_dir(cache.path())
        .unwrap()
        .map(|e| e.unwrap().path())
        .find(|p| p.extension().is_some_and(|x| x == "json"))
        .unwrap();
    let old = SystemTime::now() - Duration::from_secs(30 * DAY);
    fs::OpenOptions::new()
        .write(true)
        .open(&json)
        .unwrap()
        .set_modified(old)
        .unwrap();
    assert!(cached(&src, &d, "ps_5_0", cache.path()).is_some());
    let age = SystemTime::now()
        .duration_since(fs::metadata(&json).unwrap().modified().unwrap())
        .unwrap();
    assert!(age < Duration::from_secs(60), "touched on the hit: {age:?}");
    // `touch` on its own, and on a path that is not there.
    fs::OpenOptions::new()
        .write(true)
        .open(&json)
        .unwrap()
        .set_modified(old)
        .unwrap();
    touch(&json);
    assert!(fs::metadata(&json).unwrap().modified().unwrap() > old);
    touch(&cache.path().join("missing.json"));
}

/// Pruning an entry away is a plain miss: the next compile writes it again.
#[test]
fn a_pruned_entry_is_recompiled() {
    let cache = tempfile::tempdir().unwrap();
    let (set, d) = tiny();
    let src = set.get(Path::new("tiny.hlsl")).unwrap();
    let first = compile(&src, &d, "ps_5_0", cache.path()).unwrap();
    let limits = PruneLimits {
        max_bytes: 0,
        ..PruneLimits::default()
    };
    let r = prune(cache.path(), &limits, SystemTime::now());
    assert_eq!((r.removed, r.bytes_after), (2, 0));
    assert!(cached(&src, &d, "ps_5_0", cache.path()).is_none());
    let again = compile(&src, &d, "ps_5_0", cache.path()).unwrap();
    assert_eq!(again.bytecode, first.bytecode);
    assert!(cached(&src, &d, "ps_5_0", cache.path()).is_some());
}

/// The only test in this binary that starts the process-wide startup prune.
#[test]
fn the_startup_prune_runs_off_the_calling_thread() {
    let d = tempfile::tempdir().unwrap();
    let stale = d.path().join(".tmp-9-9");
    put(
        d.path(),
        ".tmp-9-9",
        10,
        SystemTime::now() - Duration::from_secs(3 * DAY),
    );
    render3d::cache::prune_in_background(d.path().to_path_buf());
    let t = std::time::Instant::now();
    while stale.exists() && t.elapsed() < Duration::from_secs(10) {
        std::thread::sleep(Duration::from_millis(10));
    }
    assert!(!stale.exists());
}
```

- [ ] **Step 2: Run to verify RED**

Run: `cargo test -p render3d --test cache`
Expected: FAIL to compile: `unresolved import render3d::cache`.

- [ ] **Step 3: `cache.rs`** — create `crates/render3d/src/cache.rs`:

```rust
//! Housekeeping of the on-disk shader cache (`<key>.cso` + `<key>.json` per stage, written by `compile`): a hit
//! refreshes the entry's age, and one background pass at renderer start removes what is stale or over the size
//! cap (03 §5). Deleting while another process reads is safe: a missing or torn pair is a miss and recompiles.
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

/// Refreshes the modified time of a cache entry's `.json` (the entry's age; access time is not used because
/// NTFS often has last-access updates off). Best effort.
pub fn touch(json: &Path) {
    if let Ok(f) = std::fs::OpenOptions::new().write(true).open(json) {
        let _ = f.set_modified(SystemTime::now());
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PruneLimits {
    pub max_bytes: u64,
    pub max_age_days: u32,
    pub stale_tmp_days: u32,
}

impl Default for PruneLimits {
    fn default() -> PruneLimits {
        PruneLimits {
            max_bytes: 512 * 1024 * 1024,
            max_age_days: 90,
            stale_tmp_days: 1,
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PruneReport {
    /// Files deleted.
    pub removed: usize,
    /// Bytes of the cache files left (entries, young temp files and young orphans).
    pub bytes_after: u64,
}

/// One entry: `.json` first, then `.cso`, so a reader sees a plain miss. False when the `.json` stayed.
fn drop_entry(files: &[PathBuf], removed: &mut usize) -> bool {
    let Some((first, rest)) = files.split_first() else {
        return false;
    };
    if std::fs::remove_file(first).is_err() {
        return false;
    }
    *removed += 1;
    for f in rest {
        if std::fs::remove_file(f).is_ok() {
            *removed += 1;
        }
    }
    true
}

/// Removes, in this order: `.tmp-*` older than `stale_tmp_days`; a `.cso` with no `.json` older than
/// `stale_tmp_days`; entries whose `.json` is older than `max_age_days`; then the oldest entries until the files
/// left total at most `max_bytes`. Top level of `dir` only; files of other names are left alone.
pub fn prune(dir: &Path, limits: &PruneLimits, now: SystemTime) -> PruneReport {
    let Ok(rd) = std::fs::read_dir(dir) else {
        return PruneReport::default();
    };
    let age = |t: SystemTime| now.duration_since(t).unwrap_or(Duration::ZERO);
    let days = |d: u32| Duration::from_secs(u64::from(d) * 86_400);
    type Part = (PathBuf, u64, SystemTime);
    let mut parts: HashMap<String, [Option<Part>; 2]> = HashMap::new(); // stem -> [json, cso]
    let (mut removed, mut total) = (0usize, 0u64);
    for e in rd.flatten() {
        let Ok(md) = e.metadata() else { continue };
        if !md.is_file() {
            continue;
        }
        let (path, name) = (e.path(), e.file_name().to_string_lossy().into_owned());
        let (len, modified) = (md.len(), md.modified().unwrap_or(now));
        if name.starts_with(".tmp-") {
            if age(modified) > days(limits.stale_tmp_days) && std::fs::remove_file(&path).is_ok() {
                removed += 1;
            } else {
                total += len;
            }
        } else if let Some(stem) = name.strip_suffix(".json") {
            parts.entry(stem.to_string()).or_default()[0] = Some((path, len, modified));
        } else if let Some(stem) = name.strip_suffix(".cso") {
            parts.entry(stem.to_string()).or_default()[1] = Some((path, len, modified));
        }
    }
    // (json mtime, bytes, [json, cso] paths)
    let mut live: Vec<(SystemTime, u64, Vec<PathBuf>)> = vec![];
    for [json, cso] in parts.into_values() {
        match (json, cso) {
            (Some(j), c) => {
                let mut files = vec![j.0];
                let mut bytes = j.1;
                if let Some(c) = c {
                    files.push(c.0);
                    bytes += c.1;
                }
                if age(j.2) > days(limits.max_age_days) && drop_entry(&files, &mut removed) {
                    continue;
                }
                total += bytes;
                live.push((j.2, bytes, files));
            }
            (None, Some(c)) => {
                if age(c.2) > days(limits.stale_tmp_days) && std::fs::remove_file(&c.0).is_ok() {
                    removed += 1;
                } else {
                    total += c.1;
                }
            }
            (None, None) => {}
        }
    }
    live.sort_by_key(|e| e.0);
    for (_, bytes, files) in live {
        if total <= limits.max_bytes {
            break;
        }
        if drop_entry(&files, &mut removed) {
            total -= bytes;
        }
    }
    PruneReport {
        removed,
        bytes_after: total,
    }
}

/// One prune per process, on its own thread, with the default limits and the real clock. Called by
/// `Renderer::new`; never run on the UI thread.
pub fn prune_in_background(dir: PathBuf) {
    static ONCE: std::sync::Once = std::sync::Once::new();
    ONCE.call_once(|| {
        let _ = std::thread::Builder::new()
            .name("shader-cache-prune".into())
            .spawn(move || {
                prune(&dir, &PruneLimits::default(), SystemTime::now());
            });
    });
}
```

- [ ] **Step 4: Wire it**

`compile.rs`, replace `cached` (Task 3) with:

```rust
/// A disk-cache hit for this stage, if there is one (a missing, truncated or torn pair is a miss). A hit
/// refreshes the entry's age (`cache::touch`).
pub fn cached(src: &ExpandedSource, d: &Defines, target: &str, cache_dir: &Path) -> Option<Compiled> {
    let k = program_key(src, "main", target, &d.0);
    let (cso, js) = entry_paths(cache_dir, &k);
    let hit = read_cache(&cso, &js)?;
    crate::cache::touch(&js);
    Some(hit)
}
```
`lib.rs`: add `pub mod cache;` before `pub mod cbuffer;`, and in `Renderer::new` replace the body with:

```rust
    pub fn new(shaders: Fingerprint, cache_dir: PathBuf) -> Result<Renderer, String> {
        cache::prune_in_background(cache_dir.clone());
        Renderer::with_gpu(Gpu::new(None)?, shaders, cache_dir)
    }
```

- [ ] **Step 5: Run to verify GREEN**

Run: `cargo test -p render3d --test cache --test compile --test source`
Expected: PASS.

- [ ] **Step 6: Gates**

Run: `cargo fmt --all && cargo clippy --workspace --all-targets -- -D warnings && cargo test --workspace`
Expected: all green.

- [ ] **Step 7: Commit**

```bash
git add crates/render3d/src/cache.rs crates/render3d/src/lib.rs crates/render3d/src/compile.rs crates/render3d/tests/cache.rs
git commit -m "feat(render3d): shader cache pruning (90 days, 512 MB cap, stale temp/orphan files), hit refreshes the entry age" -m "Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

---

## Task 5: `pool.rs`, async `Renderer`, `ShapeStatus::{Pending, Failed}`

> **Reconciliation R2 (binding):** Task 2 has already landed: where this task's code says `// Task 2:` at `model: mesh.transform`, keep Task 2's `world`; do not reintroduce `mesh.transform`.

**Files:**
- Create: `crates/render3d/src/pool.rs`
- Modify: `crates/render3d/src/compile.rs` (`CompiledPair`)
- Modify: `crates/render3d/src/lib.rs` (the main change; anchors below)
- Modify: `crates/render3d/Cargo.toml` (+ feature `Win32_System_Threading`)
- Modify (mechanical, so every commit builds): `crates/render3d/tests/{edit.rs,load.rs,frame.rs}`, `crates/gui/src/ui/set_preview.rs`, `crates/gui/tests/{viewport3d.rs,set_preview_ui.rs}`
- Test: `crates/render3d/tests/async_compile.rs`, `crates/render3d/tests/pool.rs` (new)

**Interfaces:**
- Consumes: Tasks 3–4 (`ProgramKey`, `program_key`, `cached`, `compile`, `SourceSet`, `ExpandedSource`).
- Produces (contract, plus the additions of CONTRACT ISSUES 2 and 4):
  ```rust
  // compile.rs
  #[derive(Debug, Clone)] pub struct CompiledPair { pub vs: Compiled, pub ps: Compiled }
  // pool.rs
  pub const WORKERS: usize = 2;
  pub struct CompileJob { pub key: ProgramKey, pub src: Arc<ExpandedSource>, pub vs: Defines, pub ps: Defines, pub cache_dir: PathBuf }
  #[derive(Debug, Clone)] pub struct CompileDone { pub key: ProgramKey, pub result: Result<Arc<CompiledPair>, String> }
  pub fn run_job(job: CompileJob) -> CompileDone;               // vs then ps; a panic becomes Err
  pub struct CompilePool;  impl CompilePool { pub fn new(threads: usize, sink: impl Fn(CompileDone) + Send + Sync + 'static) -> CompilePool; pub fn submit(&self, job: CompileJob); }
  // lib.rs
  pub enum ShapeStatus { Drawing, Hidden, Pending { showing_previous: bool }, Failed { error: String, showing_previous: bool } }
  LoadReport { …, pub pending: usize }                           // shapes = Drawing + Pending
  impl Renderer {
      pub fn take_jobs(&mut self) -> Vec<CompileJob>;
      pub fn apply_compiled(&mut self, done: CompileDone) -> bool;
      pub fn set_async_compile(&mut self, on: bool);              // default false: sync mode settles inside load_parsed/load_builtin/set_material
      pub fn settle(&mut self);                                   // run every queued job on this thread and apply it
      pub fn rebuild_device_on(&mut self, gpu: Gpu) -> Result<(), String>;   // keeps `compiled`, features, mode; drops the loaded model
  }
  ```
- Behaviour, in plain words:
  - A shape wants a program (`Wanted`: pair key, expanded source, VS/PS defines). Lookup order: `programs` (device objects), `compiled` (plain data from an earlier job or disk hit: `create_program` now), the disk cache (both stages hit: apply now, ~1 ms), else a job is queued once (`inflight` dedupes) and the shape is `Pending { showing_previous: draw.is_some() }`.
  - The old `ShapeDraw` keeps drawing with its old cbuffers until the swap (03 §1.3). `swap` builds a whole new draw and replaces the old only on success; on error the old draw stays and the status is `Failed { showing_previous: true }`.
  - Results are keyed by `ProgramKey`, never by shape: `apply_compiled` always memoises into `compiled`, and applies the result to every shape whose `wanted.key` equals it. A late result for a shape that moved on only warms the memo.
  - `set_shaders` keeps `compiled` (content-addressed, still valid) and drops `programs` and the source memo.
- Textures: `begin` first resolves every mapped slot through the caller's resolver into `tex_cache` (CONTRACT ISSUE 6); building a draw later reads `tex_cache` only.
- Priority: workers call `SetThreadPriority(GetCurrentThread(), THREAD_PRIORITY_BELOW_NORMAL)`; needs the `Win32_System_Threading` feature.

All `ShapeStatus::Failed(String)` sites (changed in this task): `crates/render3d/src/lib.rs` (enum; `load_parsed` ×2; `set_material` ×1; all rewritten below), `crates/render3d/tests/edit.rs` (`failing_permutation_keeps_old_draw`), `crates/render3d/tests/load.rs` (`load_lists_every_lighting_shape_by_block`), `crates/render3d/tests/frame.rs` (the `Failed(_)` match near the end), `crates/gui/src/ui/set_preview.rs` (`shapes()`), `crates/gui/tests/viewport3d.rs` (two `ShapeStatus::Failed(` constructions: `a_failed_edit_on_the_only_shape_keeps_the_camera_live`, the keep-view reload test), `crates/gui/tests/set_preview_ui.rs` (`a_shader_that_failed_to_compile_is_listed_per_shape`). `gui/src/model/viewport3d.rs` only compares with `==` / `!=` and needs no change here. Matches on `Drawing`/`Hidden` elsewhere (`builtin.rs`, `load.rs`) are unaffected.

- [ ] **Step 1: Write the failing tests**

Create `crates/render3d/tests/async_compile.rs`:

```rust
//! 03 T-E3..T-E7 and the load-time behaviour of the background compile. The test is the pool: no threads, it
//! takes the jobs, runs them with `run_job`, and can withhold or reorder the results.
mod common;
use common::{cache_files, channel, covered, fixture, renderer, FLAT};
use nif::material::MaterialState;
use nif::props::Alpha;
use render3d::device::Gpu;
use render3d::scene::Scene;
use render3d::{parse, run_job, Builtin, EditKind, Renderer, ShapeId, ShapeStatus};
use std::collections::HashSet;
use std::path::Path;

const CYL1: ShapeId = ShapeId(9);
const CYL2: ShapeId = ShapeId(12);

fn none(_: &str) -> Option<Vec<u8>> {
    None
}

fn material(r: &Renderer, id: ShapeId) -> MaterialState {
    r.shapes().into_iter().find(|s| s.id == id).unwrap().material
}

fn status(r: &Renderer, id: ShapeId) -> ShapeStatus {
    r.shapes().into_iter().find(|s| s.id == id).unwrap().status
}

/// Everything queued so far, compiled on this thread and applied. Returns how many jobs there were.
fn drain(r: &mut Renderer) -> usize {
    let jobs = r.take_jobs();
    let n = jobs.len();
    for j in jobs {
        r.apply_compiled(run_job(j));
    }
    n
}

fn furniture(cache: &Path) -> Renderer {
    let mut r = renderer(cache, FLAT);
    r.set_async_compile(true);
    let rep = r.load_parsed(&parse(&fixture("TestNifFile_Furniture_Col_SE.nif")), &none);
    assert_eq!((rep.shapes, rep.pending), (2, 2), "{rep:?}");
    assert!(rep.compile_errors.is_empty() && rep.errors.is_empty(), "{rep:?}");
    assert_eq!(
        status(&r, CYL1),
        ShapeStatus::Pending {
            showing_previous: false
        }
    );
    assert!(drain(&mut r) >= 1);
    assert_eq!(status(&r, CYL1), ShapeStatus::Drawing);
    assert_eq!(status(&r, CYL2), ShapeStatus::Drawing);
    r
}

/// Vanilla → TruePBR with Rim (Subsurface): a permutation edit whose pixels differ (the stand-in writes 0.5
/// green when PBRFlags ≠ 0).
fn pbr_subsurface(mut m: MaterialState) -> MaterialState {
    m.slsf2 &= !((1 << 12) | (1 << 24) | (1 << 25) | (1 << 26) | (1 << 27));
    m.slsf2 |= (1 << 23) | (1 << 26);
    m.params.emissive_color = [0.0; 3];
    m
}

fn alpha_test(mut m: MaterialState) -> MaterialState {
    m.alpha = Some(Alpha {
        flags: 0x200,
        threshold: 0,
    });
    m
}

#[test]
fn a_first_load_queues_one_job_per_program_and_draws_the_clear_colour_meanwhile() {
    let cache = tempfile::tempdir().unwrap();
    let mut r = renderer(cache.path(), FLAT);
    r.set_async_compile(true);
    let rep = r.load_parsed(&parse(&fixture("TestNifFile_Furniture_Col_SE.nif")), &none);
    assert_eq!((rep.shapes, rep.pending), (2, 2), "pending counts as drawable");
    assert!(r.bounds().is_some(), "framing works before any program lands");
    let jobs = r.take_jobs();
    assert!(!jobs.is_empty());
    let keys: HashSet<_> = jobs.iter().map(|j| j.key.clone()).collect();
    assert_eq!(keys.len(), jobs.len(), "one job per distinct program");
    assert!(r.take_jobs().is_empty());
    assert_eq!(covered(&r.render(&Scene::neutral(), 64, 64).unwrap()), 0);
    for j in jobs {
        assert!(r.apply_compiled(run_job(j)));
    }
    assert!(covered(&r.render(&Scene::neutral(), 64, 64).unwrap()) > 0);
}

/// `load` is the compatibility wrapper: it settles even in async mode, with the P0 report.
#[test]
fn load_settles_even_when_the_renderer_is_async() {
    let cache = tempfile::tempdir().unwrap();
    let mut r = renderer(cache.path(), &["STANDIN_FLAT"]);
    r.set_async_compile(true);
    let rep = r.load(&fixture("TestNifFile_DeepGraph_SE.nif"), &none);
    assert_eq!(rep.pending, 0);
    assert_eq!(rep.compile_errors.len(), 1, "{:?}", rep.compile_errors);
    assert!(rep.compile_errors[0].starts_with("SkullPeg01:19: ps:"));
    assert!(r.take_jobs().is_empty());
}

#[test]
fn a_disk_cache_hit_is_applied_at_load_with_no_job() {
    let cache = tempfile::tempdir().unwrap();
    furniture(cache.path()); // writes the entries
    let mut r = renderer(cache.path(), FLAT);
    r.set_async_compile(true);
    let rep = r.load_parsed(&parse(&fixture("TestNifFile_Furniture_Col_SE.nif")), &none);
    assert_eq!((rep.shapes, rep.pending), (2, 0), "{rep:?}");
    assert!(r.take_jobs().is_empty());
    assert_eq!(status(&r, CYL1), ShapeStatus::Drawing);
}

/// 03 T-E3
#[test]
fn permutation_edit_keeps_old_draw_until_compiled() {
    let cache = tempfile::tempdir().unwrap();
    let mut r = furniture(cache.path());
    let scene = Scene::neutral();
    let before = r.render(&scene, 64, 64).unwrap();
    let m = pbr_subsurface(material(&r, CYL1));
    assert_eq!(
        r.set_material(CYL1, m.clone(), &none),
        Ok(EditKind::Permutation)
    );
    assert_eq!(
        status(&r, CYL1),
        ShapeStatus::Pending {
            showing_previous: true
        }
    );
    assert_eq!(status(&r, CYL2), ShapeStatus::Drawing);
    assert_eq!(material(&r, CYL1), m, "the wanted material is reported");
    let jobs = r.take_jobs();
    assert_eq!(jobs.len(), 1);
    assert_eq!(
        r.render(&scene, 64, 64).unwrap(),
        before,
        "byte-identical while the program compiles"
    );
    // A second edit for the same program queues nothing and is what lands.
    let mut m2 = m.clone();
    m2.params.glossiness = 7.0;
    assert_eq!(r.set_material(CYL1, m2.clone(), &none), Ok(EditKind::Cbuffer));
    assert!(r.take_jobs().is_empty(), "in flight already");
    assert_eq!(
        status(&r, CYL1),
        ShapeStatus::Pending {
            showing_previous: true
        }
    );
    let files = cache_files(cache.path());
    let done = run_job(jobs.into_iter().next().unwrap());
    assert!(done.result.is_ok());
    assert!(r.apply_compiled(done));
    assert_eq!(status(&r, CYL1), ShapeStatus::Drawing);
    assert_eq!(material(&r, CYL1), m2);
    assert!(cache_files(cache.path()) > files, "the worker wrote the cache");
    let after = r.render(&scene, 64, 64).unwrap();
    assert!(
        channel(&after, 1) > channel(&before, 1) + 1000,
        "green {} → {}",
        channel(&before, 1),
        channel(&after, 1)
    );
}

/// 03 T-E4 / Review Focus #3
#[test]
fn failing_permutation_keeps_old_draw() {
    let cache = tempfile::tempdir().unwrap();
    // No STANDIN_OPAQUE_OK: any permutation without DO_ALPHA_TEST fails to compile.
    let mut r = renderer(cache.path(), &["STANDIN_FLAT"]);
    r.set_async_compile(true);
    let rep = r.load_parsed(&parse(&fixture("TestNifFile_DeepGraph_SE.nif")), &none);
    assert!(rep.compile_errors.is_empty(), "failures arrive later: {rep:?}");
    drain(&mut r);
    let peg = ShapeId(103);
    assert_eq!(status(&r, peg), ShapeStatus::Drawing);
    assert!(matches!(
        status(&r, ShapeId(143)),
        ShapeStatus::Failed {
            showing_previous: false,
            ..
        }
    ));
    let scene = Scene::neutral();
    let before = r.render(&scene, 96, 96).unwrap();
    let good = material(&r, peg);
    let mut bad = good.clone();
    bad.alpha = None;
    assert_eq!(
        r.set_material(peg, bad.clone(), &none),
        Ok(EditKind::Permutation)
    );
    assert_eq!(
        status(&r, peg),
        ShapeStatus::Pending {
            showing_previous: true
        }
    );
    assert_eq!(r.render(&scene, 96, 96).unwrap(), before);
    drain(&mut r);
    assert!(
        matches!(status(&r, peg), ShapeStatus::Failed { error, showing_previous: true } if error.starts_with("ps:"))
    );
    assert_eq!(material(&r, peg), bad);
    assert_eq!(r.render(&scene, 96, 96).unwrap(), before, "old pixels stay");
    assert_eq!(status(&r, ShapeId(107)), ShapeStatus::Drawing, "others unaffected");
    // Back to the good material: its program is still in memory, so the swap is immediate.
    assert_eq!(
        r.set_material(peg, good, &none),
        Ok(EditKind::Permutation)
    );
    assert_eq!(status(&r, peg), ShapeStatus::Drawing);
    assert!(r.take_jobs().is_empty());
    assert_eq!(r.render(&scene, 96, 96).unwrap(), before);
}

/// 03 T-E5: vertex colours on and off change the VS inputs, so the swap rebuilds the input layout.
#[test]
fn swap_rebuilds_input_layout() {
    let cache = tempfile::tempdir().unwrap();
    let mut r = furniture(cache.path());
    let scene = Scene::neutral();
    let before = r.render(&scene, 64, 64).unwrap();
    let off = material(&r, CYL1);
    let mut on = off.clone();
    on.has_vertex_colors = true;
    on.slsf2 |= 1 << 5; // Vertex_Colors → VC (VS)
    assert_eq!(r.set_material(CYL1, on, &none), Ok(EditKind::Permutation));
    assert_eq!(
        status(&r, CYL1),
        ShapeStatus::Pending {
            showing_previous: true
        }
    );
    assert_eq!(drain(&mut r), 1);
    assert_eq!(status(&r, CYL1), ShapeStatus::Drawing);
    assert!(!r.gpu().is_removed());
    assert_eq!(r.render(&scene, 64, 64).unwrap(), before, "white vertex colour");
    // And back: the first program is in memory, no job, drawn at once.
    r.set_material(CYL1, off, &none).unwrap();
    assert_eq!(status(&r, CYL1), ShapeStatus::Drawing);
    assert!(r.take_jobs().is_empty());
    assert_eq!(r.render(&scene, 64, 64).unwrap(), before);
}

/// 03 T-E6: edits A then B; B's program lands first, A's later. B stays on screen, A is only memoised.
#[test]
fn out_of_order_results() {
    let cache = tempfile::tempdir().unwrap();
    let mut r = furniture(cache.path());
    let scene = Scene::neutral();
    let base = material(&r, CYL1);
    let a = alpha_test(base.clone());
    let b = pbr_subsurface(base);
    r.set_material(CYL1, a.clone(), &none).unwrap();
    let job_a = r.take_jobs().into_iter().next().unwrap();
    r.set_material(CYL1, b.clone(), &none).unwrap();
    let job_b = r.take_jobs().into_iter().next().unwrap();
    assert_ne!(job_a.key, job_b.key);
    assert!(r.apply_compiled(run_job(job_b)), "B is wanted");
    assert_eq!(status(&r, CYL1), ShapeStatus::Drawing);
    assert_eq!(material(&r, CYL1), b);
    let on_screen = r.render(&scene, 64, 64).unwrap();
    assert!(!r.apply_compiled(run_job(job_a)), "A is stale: memoised only");
    assert_eq!(status(&r, CYL1), ShapeStatus::Drawing);
    assert_eq!(r.render(&scene, 64, 64).unwrap(), on_screen);
    // The memo works: asking for A now needs no compile.
    r.set_material(CYL1, a, &none).unwrap();
    assert_eq!(status(&r, CYL1), ShapeStatus::Drawing);
    assert!(r.take_jobs().is_empty());
}

/// A result for content the user already left is harmless: it applies to no shape of the new content.
#[test]
fn a_result_for_a_replaced_load_only_warms_the_memo() {
    let cache = tempfile::tempdir().unwrap();
    let mut r = renderer(cache.path(), FLAT);
    r.set_async_compile(true);
    r.load_parsed(&parse(&fixture("TestNifFile_Furniture_Col_SE.nif")), &none);
    let old_jobs = r.take_jobs();
    let mut pbr = pbr_subsurface(MaterialState::builtin_default());
    pbr.skinned = false;
    let rep = r.load_builtin(Builtin::Sphere, pbr, &none);
    assert_eq!((rep.shapes, rep.pending), (1, 1));
    let new_jobs = r.take_jobs();
    assert_eq!(new_jobs.len(), 1);
    for j in old_jobs {
        assert!(!r.apply_compiled(run_job(j)), "nobody waits for it");
    }
    assert_eq!(
        status(&r, ShapeId(0)),
        ShapeStatus::Pending {
            showing_previous: false
        }
    );
    assert!(r.apply_compiled(run_job(new_jobs.into_iter().next().unwrap())));
    assert_eq!(status(&r, ShapeId(0)), ShapeStatus::Drawing);
}

/// 03 T-E7: a rebuilt device reuses `compiled`: nothing is compiled or read from disk again.
#[test]
fn device_rebuild_reuses_compiled() {
    let cache = tempfile::tempdir().unwrap();
    let mut r = furniture(cache.path());
    let scene = Scene::neutral();
    let before = r.render(&scene, 64, 64).unwrap();
    for e in std::fs::read_dir(cache.path()).unwrap() {
        std::fs::remove_file(e.unwrap().path()).unwrap();
    }
    r.rebuild_device_on(Gpu::new_warp().unwrap()).unwrap();
    assert!(r.shapes().is_empty(), "the loaded model is gone: load again");
    let rep = r.load_parsed(&parse(&fixture("TestNifFile_Furniture_Col_SE.nif")), &none);
    assert_eq!((rep.shapes, rep.pending), (2, 0), "{rep:?}");
    assert!(r.take_jobs().is_empty());
    assert_eq!(cache_files(cache.path()), 0, "no compile, no disk entry");
    assert_eq!(r.render(&scene, 64, 64).unwrap(), before);
    assert_eq!(r.features(), common::FLAT, "features survive the rebuild");
}
```

Create `crates/render3d/tests/pool.rs`:

```rust
//! The compile pool: real worker threads, below-normal priority, results through the sink.
mod common;
use common::{fixture, renderer, FLAT};
use render3d::{parse, CompilePool, ShapeStatus};
use std::time::Duration;
use windows::Win32::System::Threading::{GetCurrentThread, GetThreadPriority};

#[test]
fn the_pool_compiles_off_thread_at_below_normal_priority() {
    let cache = tempfile::tempdir().unwrap();
    let mut r = renderer(cache.path(), FLAT);
    r.set_async_compile(true);
    r.load_parsed(
        &parse(&fixture("TestNifFile_Furniture_Col_SE.nif")),
        &|_| None,
    );
    let jobs = r.take_jobs();
    let n = jobs.len();
    assert!(n >= 1);
    let (tx, rx) = std::sync::mpsc::channel();
    // The sink runs on the worker thread: read that thread's priority there.
    let pool = CompilePool::new(render3d::pool::WORKERS, move |d| {
        // SAFETY: the pseudo-handle of the calling thread is always valid.
        let prio = unsafe { GetThreadPriority(GetCurrentThread()) };
        let _ = tx.send((d, prio, std::thread::current().id()));
    });
    for j in jobs {
        pool.submit(j);
    }
    for _ in 0..n {
        let (done, prio, tid) = rx.recv_timeout(Duration::from_secs(60)).expect("a result");
        assert_eq!(prio, -1, "THREAD_PRIORITY_BELOW_NORMAL");
        assert_ne!(tid, std::thread::current().id());
        assert!(done.result.is_ok(), "{:?}", done.result);
        assert!(r.apply_compiled(done));
    }
    assert!(r.shapes().iter().all(|s| s.status == ShapeStatus::Drawing));
}

#[test]
fn a_failing_job_is_delivered_as_an_error() {
    let cache = tempfile::tempdir().unwrap();
    let set = render3d::source::SourceSet::new(
        concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures").into(),
    );
    // `tiny.hlsl` is a pixel shader (`SV_Position` in, `SV_Target` out): as vs_5_0 it does not compile.
    let src = set.get(std::path::Path::new("tiny.hlsl")).unwrap();
    let job = render3d::CompileJob {
        key: render3d::compile::ProgramKey("k".into()),
        src,
        vs: render3d::permutation::Defines::default(),
        ps: render3d::permutation::Defines::default(),
        cache_dir: cache.path().to_path_buf(),
    };
    let done = render3d::run_job(job);
    assert_eq!(done.key.0, "k");
    assert!(
        done.result.as_ref().is_err_and(|e| e.starts_with("vs:")),
        "{:?}",
        done.result
    );
}
```

The panic path itself (`catch_unwind` in `run_job`) is a three-line wrapper and is not unit-tested.

- [ ] **Step 2: Run to verify RED**

Run: `cargo test -p render3d --test async_compile --test pool`
Expected: FAIL to compile (`no method set_async_compile`, `unresolved import render3d::run_job`, `CompilePool`, `pending` field, `ShapeStatus::Pending`).

- [ ] **Step 3: `Cargo.toml`** — append `"Win32_System_Threading"` to the same `windows` feature list (now `…, "Win32_System_LibraryLoader", "Win32_System_Threading"]`).

- [ ] **Step 4: `compile.rs`** — add after `pub struct Compiled { .. }`:

```rust
/// The vertex and pixel stage of one permutation: plain data, `Send`, what a worker returns.
#[derive(Debug, Clone)]
pub struct CompiledPair {
    pub vs: Compiled,
    pub ps: Compiled,
}
```

- [ ] **Step 5: `pool.rs`** — create `crates/render3d/src/pool.rs`:

```rust
//! The background shader compile (03 §1.4): plain-data jobs in, plain-data results out. Only plain data crosses
//! threads (`ExpandedSource`, `Defines`, `Compiled` are `Send`); the render thread owns every device object.
use crate::compile::{compile, CompiledPair, ProgramKey};
use crate::permutation::Defines;
use crate::source::ExpandedSource;
use std::path::PathBuf;
use std::sync::mpsc::{channel, Sender};
use std::sync::{Arc, Mutex, PoisonError};

/// Spec §2.2: two workers, below-normal priority, not a setting (the encoder shares the CPU).
pub const WORKERS: usize = 2;

pub struct CompileJob {
    /// `ProgramKey::pair` of the two stage keys.
    pub key: ProgramKey,
    pub src: Arc<ExpandedSource>,
    pub vs: Defines,
    pub ps: Defines,
    pub cache_dir: PathBuf,
}

#[derive(Debug, Clone)]
pub struct CompileDone {
    pub key: ProgramKey,
    pub result: Result<Arc<CompiledPair>, String>,
}

fn pair(job: &CompileJob) -> Result<CompiledPair, String> {
    let vs = compile(&job.src, &job.vs, "vs_5_0", &job.cache_dir).map_err(|e| format!("vs: {e}"))?;
    let ps = compile(&job.src, &job.ps, "ps_5_0", &job.cache_dir).map_err(|e| format!("ps: {e}"))?;
    Ok(CompiledPair { vs, ps })
}

/// Compiles (or reads from the disk cache) both stages. A panic inside the compiler becomes an error result,
/// so a job is never lost and a shape never stays `Pending` for ever.
pub fn run_job(job: CompileJob) -> CompileDone {
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| pair(&job)))
        .unwrap_or_else(|_| Err("internal error: the shader compiler panicked".to_string()));
    CompileDone {
        key: job.key,
        result: result.map(Arc::new),
    }
}

fn lower_priority() {
    use windows::Win32::System::Threading::{
        GetCurrentThread, SetThreadPriority, THREAD_PRIORITY_BELOW_NORMAL,
    };
    // SAFETY: the pseudo-handle of the calling thread is always valid.
    let _ = unsafe { SetThreadPriority(GetCurrentThread(), THREAD_PRIORITY_BELOW_NORMAL) };
}

/// `threads` workers at below-normal priority. Every finished job goes to `sink` on the worker thread (the GUI's
/// sink sends it into the render loop's own queue). Dropping the pool lets idle workers exit; a job already
/// running finishes first.
pub struct CompilePool {
    tx: Sender<CompileJob>,
}

impl CompilePool {
    pub fn new(threads: usize, sink: impl Fn(CompileDone) + Send + Sync + 'static) -> CompilePool {
        let (tx, rx) = channel::<CompileJob>();
        let rx = Arc::new(Mutex::new(rx));
        let sink = Arc::new(sink);
        for n in 0..threads.max(1) {
            let (rx, sink) = (rx.clone(), sink.clone());
            let _ = std::thread::Builder::new()
                .name(format!("shader-compile-{n}"))
                .spawn(move || {
                    lower_priority();
                    loop {
                        // The lock is held while waiting: the other workers queue behind it, which is fine.
                        let job = rx.lock().unwrap_or_else(PoisonError::into_inner).recv();
                        let Ok(job) = job else { break };
                        sink(run_job(job));
                    }
                });
        }
        CompilePool { tx }
    }

    pub fn submit(&self, job: CompileJob) {
        let _ = self.tx.send(job);
    }
}
```

- [ ] **Step 6: `lib.rs`** — the main edit. Apply in this order; each block is "replace X with Y" by text anchor.

(a) Modules / imports. After `pub mod permutation;` add `pub mod pool;`. After `pub use parse::{parse, ParsedNif, ParsedShape, ShapeId};` add `pub use pool::{run_job, CompileDone, CompileJob, CompilePool};`. Replace the import lines

```rust
use compile::{compile, program_key, ProgramKey, SrvKind};
...
use permutation::{defines, Material, Stage};
...
use std::collections::{BTreeMap, HashMap};
use std::path::{Path, PathBuf};
use source::SourceSet;
```
with
```rust
use compile::{cached, program_key, CompiledPair, ProgramKey, SrvKind};
...
use permutation::{defines, Defines, Stage};
...
use source::{ExpandedSource, SourceSet};
use std::collections::{BTreeMap, HashMap, HashSet};
use std::path::{Path, PathBuf};
```
(keep every other `use` line as is; `compile` and `Material` are no longer used in lib.rs.)

(b) `LoadReport`: replace the `shapes` field and add `pending`:

```rust
    /// Shapes that are drawing or will draw (waiting for a program counts: an async first load of an uncached
    /// mesh has none drawing yet, and the GUI must still send a camera, 03 §1.4).
    pub shapes: usize,
    /// Of `shapes`, how many are waiting for a background compile.
    pub pending: usize,
```

(c) `ShapeStatus`: replace the whole enum with

```rust
/// What a loaded shape is doing on screen.
#[derive(Debug, Clone, PartialEq)]
pub enum ShapeStatus {
    /// The current material is on screen.
    Drawing,
    /// `MaterialState::hidden` (PGPatcher `delete`): not drawn.
    Hidden,
    /// Its shader program is compiling in the background. `showing_previous`: the draw of an earlier material
    /// is still on screen, with its old constants, until the program lands.
    Pending { showing_previous: bool },
    /// The current material could not be built (compile, mesh or GPU error). `showing_previous`: the draw of an
    /// earlier material stays on screen.
    Failed {
        error: String,
        showing_previous: bool,
    },
}
```

(d) Replace `struct ShapeState` and `enum BuildError` + its `impl` (through the closing brace of `impl BuildError`) with:

```rust
/// One loaded shape: the CPU mesh (kept so an edit rebuilds the draw without re-parsing), the material last
/// loaded or set, and what is on screen (possibly built from an earlier material, see [`ShapeStatus`]).
struct ShapeState {
    name: String,
    mesh: Arc<nif::geom::Mesh>,
    material: MaterialState,
    draw: Option<ShapeDraw>,
    status: ShapeStatus,
    /// The program a `Pending` shape waits for.
    wanted: Option<Wanted>,
    /// `Failed` because the program did not compile (a `LoadReport::compile_errors` line), not for a mesh or
    /// GPU error (an `errors` line).
    compile_failed: bool,
}

/// A permutation as the compile pool needs it: its pair key, the shared expanded source, both define sets.
struct Wanted {
    key: ProgramKey,
    src: Arc<ExpandedSource>,
    vs: Defines,
    ps: Defines,
}

impl Wanted {
    fn job(&self, cache_dir: &Path) -> CompileJob {
        CompileJob {
            key: self.key.clone(),
            src: self.src.clone(),
            vs: self.vs.clone(),
            ps: self.ps.clone(),
            cache_dir: cache_dir.to_path_buf(),
        }
    }
}

enum Lookup {
    Ready(Program),
    Failed(String),
    Waiting,
}

fn ready(r: &Result<Program, String>) -> Lookup {
    match r {
        Ok(p) => Lookup::Ready(p.clone()),
        Err(e) => Lookup::Failed(e.clone()),
    }
}
```

(e) `struct Renderer`: replace the `programs` field (Task 3 version) with

```rust
    /// One expanded `Lighting.hlsl` per shaders folder (03 §5); replaced by `set_shaders`.
    sources: SourceSet,
    /// Device objects by pair key; dropped by `set_shaders` and by a device rebuild.
    programs: HashMap<ProgramKey, Result<Program, String>>,
    /// Plain-data results by pair key (a worker's answer or a disk hit). Content-addressed, so they survive
    /// `set_shaders` and a device rebuild: a rebuilt device compiles nothing.
    compiled: HashMap<ProgramKey, Result<Arc<CompiledPair>, String>>,
    /// Keys whose job was queued and has not been applied yet.
    inflight: HashSet<ProgramKey>,
    /// Queued, not yet taken by the host (`take_jobs`).
    jobs: Vec<CompileJob>,
    /// Hosts that drive a pool turn this on; the default settles every load and edit before returning.
    async_compile: bool,
```
(`sources` is already there from Task 3: keep one copy.) In `with_gpu`'s struct literal add, next to `programs: HashMap::new(),`:
```rust
            compiled: HashMap::new(),
            inflight: HashSet::new(),
            jobs: vec![],
            async_compile: false,
```

(f) `set_shaders` (Task 3 version) stays. Delete the whole `fn program(&mut self, m: &Material)`.

(g) Replace `fn bind_textures` header and its `texture` call. New header:

```rust
    /// NIF slot → register for every slot the PS reads (taken from `tex_cache`, which `resolve_textures` filled),
    /// then a 1×1 default for every reflected register left without one. Returns `(register, view)` for every
    /// reflected SRV and the registers bound to a real texture.
    fn bind_textures(
        &mut self,
        name: &str,
        program: &Program,
        m: &MaterialState,
        rep: &mut LoadReport,
    ) -> Result<(Vec<(u32, ID3D11ShaderResourceView)>, Bound), String> {
```
and inside, replace `match self.texture(&path.norm, textures, rep) {` with `match self.tex_cache.get(&path.norm).cloned().flatten() {`.

(h) Replace `fn build_draw` (whole fn) with the following, and add the new methods right after it:

```rust
    /// Resolves every slot of `m` that maps to a register through `textures` into `tex_cache` (once per path
    /// per load). Done at load/edit time because `apply_compiled` has no resolver.
    fn resolve_textures(
        &mut self,
        m: &MaterialState,
        textures: &dyn Fn(&str) -> Option<Vec<u8>>,
        rep: &mut LoadReport,
    ) {
        let (pbr, msn) = (m.is_pbr(), m.slsf1 & (1 << 12) != 0);
        for (slot, path) in m.slots.iter().enumerate() {
            if !path.norm.is_empty()
                && textures::register_for_slot(m.shader_type, pbr, msn, slot).is_some()
            {
                self.texture(&path.norm, textures, rep);
            }
        }
    }

    /// Builds one shape's draw for material `m` on `program`: textures (already in `tex_cache`), vertex streams
    /// against the VS inputs, zero cbuffers from the reflection, then the material.
    fn build_draw(
        &mut self,
        name: &str,
        mesh: &nif::geom::Mesh,
        m: &MaterialState,
        program: &Program,
        rep: &mut LoadReport,
    ) -> Result<ShapeDraw, String> {
        let (srvs, bound) = self.bind_textures(name, program, m, rep)?;
        let gpu_mesh = mesh::build(
            &self.gpu,
            mesh,
            &program.vs.reflection.inputs,
            &program.vs.bytecode,
        )?;
        let (mut vs_cbs, mut ps_cbs) = match (
            cbuffers(&self.gpu, &program.vs.reflection),
            cbuffers(&self.gpu, &program.ps.reflection),
        ) {
            (Ok(v), Ok(p)) => (v, p),
            (Err(e), _) | (_, Err(e)) => return Err(e),
        };
        let ll = self.linear_lighting();
        for cb in vs_cbs.iter_mut().chain(ps_cbs.iter_mut()) {
            write_material(&mut cb.writer, m, &bound, ll);
            scene::write_static(&mut cb.writer);
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
            model: mesh.transform, // Task 2: the shape's `world`
            blend: m.blend(),
            skinned: m.skinned,
            bound,
            stale: true,
        })
    }

    /// The program `m` needs: both stage keys over the shared expansion of `Lighting.hlsl`.
    fn wanted(&self, m: &MaterialState) -> Result<Wanted, String> {
        let pm = perm_material(m);
        let f: Vec<&str> = self.features.iter().map(String::as_str).collect();
        let vs = defines(&pm, Stage::Vertex, &f, false);
        let ps = defines(&pm, Stage::Pixel, &f, false);
        let src = self.sources.get(Path::new("Lighting.hlsl"))?;
        let key = ProgramKey::pair(
            &program_key(&src, "main", "vs_5_0", &vs.0),
            &program_key(&src, "main", "ps_5_0", &ps.0),
        );
        Ok(Wanted { key, src, vs, ps })
    }

    /// The program if it is at hand without compiling: device objects, a memoised result (made into device
    /// objects now), or a disk-cache hit for both stages (~1 ms). Otherwise `Waiting`.
    fn lookup(&mut self, w: &Wanted) -> Lookup {
        if let Some(p) = self.programs.get(&w.key) {
            return ready(p);
        }
        if !self.compiled.contains_key(&w.key) {
            let dir = &self.cache_dir;
            if let (Some(vs), Some(ps)) = (
                cached(&w.src, &w.vs, "vs_5_0", dir),
                cached(&w.src, &w.ps, "ps_5_0", dir),
            ) {
                self.compiled
                    .insert(w.key.clone(), Ok(Arc::new(CompiledPair { vs, ps })));
            }
        }
        match self.compiled.get(&w.key) {
            None => Lookup::Waiting,
            Some(Err(e)) => Lookup::Failed(e.clone()),
            Some(Ok(pair)) => {
                let built = create_program(&self.gpu, pair.vs.clone(), pair.ps.clone());
                self.programs.insert(w.key.clone(), built.clone());
                ready(&built)
            }
        }
    }

    fn fail(&mut self, id: ShapeId, error: String, compile: bool) {
        if let Some(s) = self.shapes.get_mut(&id) {
            s.status = ShapeStatus::Failed {
                error,
                showing_previous: s.draw.is_some(),
            };
            s.wanted = None;
            s.compile_failed = compile;
        }
    }

    /// Replaces shape `id`'s draw with one built for `m` on `p`, only on success (03 §1.3 `swap_program`):
    /// on error the old draw stays and the status becomes `Failed { showing_previous: true }`.
    fn swap(&mut self, id: ShapeId, p: &Program, m: &MaterialState, rep: &mut LoadReport) {
        let (name, mesh) = match self.shapes.get(&id) {
            Some(s) => (s.name.clone(), s.mesh.clone()),
            None => return,
        };
        match self.build_draw(&name, &mesh, m, p, rep) {
            Ok(d) => {
                if let Some(s) = self.shapes.get_mut(&id) {
                    s.draw = Some(d);
                    s.status = ShapeStatus::Drawing;
                    s.wanted = None;
                    s.compile_failed = false;
                }
            }
            Err(e) => self.fail(id, e, false),
        }
    }

    /// Starts building shape `id` for `m` (already stored as its material): swaps now when the program is at
    /// hand, else queues one job (deduplicated) and marks the shape `Pending`. The old draw, if any, stays.
    fn begin(
        &mut self,
        id: ShapeId,
        m: &MaterialState,
        textures: &dyn Fn(&str) -> Option<Vec<u8>>,
        rep: &mut LoadReport,
    ) {
        self.resolve_textures(m, textures, rep);
        let wanted = match self.wanted(m) {
            Ok(w) => w,
            Err(e) => return self.fail(id, e, true),
        };
        match self.lookup(&wanted) {
            Lookup::Ready(p) => self.swap(id, &p, m, rep),
            Lookup::Failed(e) => self.fail(id, e, true),
            Lookup::Waiting => {
                if self.inflight.insert(wanted.key.clone()) {
                    self.jobs.push(wanted.job(&self.cache_dir));
                }
                if let Some(s) = self.shapes.get_mut(&id) {
                    s.status = ShapeStatus::Pending {
                        showing_previous: s.draw.is_some(),
                    };
                    s.wanted = Some(wanted);
                    s.compile_failed = false;
                }
            }
        }
    }

    /// Hosts that run their own pool: the jobs queued by loads and edits since the last call.
    pub fn take_jobs(&mut self) -> Vec<CompileJob> {
        std::mem::take(&mut self.jobs)
    }

    /// A finished job. Always memoised under its key; applied to every shape that waits for that key (a late
    /// result for a shape that moved on only warms the memo). True when a shape's draw or status changed (the
    /// host re-renders and refreshes its shape list).
    pub fn apply_compiled(&mut self, done: CompileDone) -> bool {
        let CompileDone { key, result } = done;
        self.inflight.remove(&key);
        self.compiled.insert(key.clone(), result);
        let waiting: Vec<ShapeId> = self
            .shapes
            .iter()
            .filter(|(_, s)| s.wanted.as_ref().is_some_and(|w| w.key == key))
            .map(|(id, _)| *id)
            .collect();
        for id in &waiting {
            let Some(w) = self.shapes.get_mut(id).and_then(|s| s.wanted.take()) else {
                continue;
            };
            let m = self.shapes[id].material.clone();
            match self.lookup(&w) {
                Lookup::Ready(p) => self.swap(*id, &p, &m, &mut LoadReport::default()),
                Lookup::Failed(e) => self.fail(*id, e, true),
                Lookup::Waiting => self.fail(*id, "the program is missing after its compile".into(), true),
            }
        }
        if !waiting.is_empty() {
            self.sort_draw_order();
        }
        !waiting.is_empty()
    }

    /// Runs every queued job on this thread and applies it: the synchronous mode (tests, the Meshes view's
    /// `load`).
    pub fn settle(&mut self) {
        loop {
            let jobs = self.take_jobs();
            if jobs.is_empty() {
                break;
            }
            for j in jobs {
                self.apply_compiled(run_job(j));
            }
        }
    }

    /// Turns the pool-driven mode on or off (default off). On: `load_parsed`, `load_builtin` and
    /// `set_material` return with `Pending` shapes and jobs for `take_jobs`; off: they settle before returning.
    pub fn set_async_compile(&mut self, on: bool) {
        self.async_compile = on;
    }

    /// After a device loss: a fresh device (the caller makes it, WARP if need be) with the same shaders, cache
    /// dir, features, sRGB mode and `compiled` results. Everything that lived on the old device is gone,
    /// including the loaded model: load it again, and every program comes from `compiled` with no compile.
    pub fn rebuild_device_on(&mut self, gpu: Gpu) -> Result<(), String> {
        let mut n = Renderer::with_gpu(gpu, self.shaders.clone(), self.cache_dir.clone())?;
        n.features = std::mem::take(&mut self.features);
        n.encode_srgb = self.encode_srgb;
        n.async_compile = self.async_compile;
        n.reference_writer = self.reference_writer;
        n.compiled = std::mem::take(&mut self.compiled);
        *self = n;
        Ok(())
    }
```
Also add `pub use pool::...` already done in (a).

(i) Replace `pub fn load`, `pub fn load_parsed` (both whole fns, through the end of `load_parsed`) with:

```rust
    /// Parses the NIF and loads it, then settles: `load_parsed` in sync mode whatever the renderer's mode.
    /// `textures` resolves a canonical texture path (`textures/…`, lower case, `/`) to DDS bytes.
    pub fn load(
        &mut self,
        nif_bytes: &[u8],
        textures: &dyn Fn(&str) -> Option<Vec<u8>>,
    ) -> LoadReport {
        self.load_inner(&parse(nif_bytes), textures, true)
    }

    /// Replaces the loaded model with `parsed`: geometry and textures now, one program per distinct
    /// permutation. A program that is at hand (in memory or a disk-cache hit) is applied now; the others are
    /// queued (`take_jobs`) and their shapes are `Pending` with no draw. In sync mode (the default) the queue
    /// is run before returning, so no shape is `Pending` and `LoadReport::compile_errors` is filled.
    pub fn load_parsed(
        &mut self,
        parsed: &ParsedNif,
        textures: &dyn Fn(&str) -> Option<Vec<u8>>,
    ) -> LoadReport {
        self.load_inner(parsed, textures, !self.async_compile)
    }

    fn load_inner(
        &mut self,
        parsed: &ParsedNif,
        textures: &dyn Fn(&str) -> Option<Vec<u8>>,
        sync: bool,
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
            let hidden = s.material.hidden;
            self.shapes.insert(
                s.id,
                ShapeState {
                    name: s.name.clone(),
                    mesh: s.mesh.clone(),
                    material: s.material.clone(),
                    draw: None,
                    status: if hidden {
                        ShapeStatus::Hidden
                    } else {
                        ShapeStatus::Pending {
                            showing_previous: false,
                        }
                    },
                    wanted: None,
                    compile_failed: false,
                },
            );
            if !hidden {
                self.begin(s.id, &s.material, textures, &mut rep);
            }
        }
        if sync {
            self.settle();
        }
        for s in self.shapes.values() {
            if let ShapeStatus::Failed { error, .. } = &s.status {
                let line = format!("{}: {error}", s.name);
                if s.compile_failed {
                    rep.compile_errors.push(line);
                } else {
                    rep.errors.push(line);
                }
            }
        }
        self.bounds = self.compute_bounds();
        self.warn_missing_fields(&mut rep);
        self.sort_draw_order();
        rep.pending = self
            .shapes
            .values()
            .filter(|s| matches!(s.status, ShapeStatus::Pending { .. }))
            .count();
        rep.shapes = rep.pending
            + self
                .shapes
                .values()
                .filter(|s| s.status == ShapeStatus::Drawing)
                .count();
        rep
    }
```
`load_builtin` is unchanged (it calls `self.load_parsed`).

(j) Replace the whole `set_material` and `apply_in_place` with:

```rust
    /// Replaces shape `id`'s material and applies it the cheapest way [`classify`] allows (03 §1.3):
    /// - `Cbuffer`: every cbuffer of the draw back to zero, then the material; on the next frame, no compile;
    /// - `Textures`: the slots resolved again through `textures` and rebound, then the `Cbuffer` path (the PBR
    ///   flags and `EnvmapData.y` depend on what is bound);
    /// - `Permutation`: the new program at hand is swapped in now; otherwise a job is queued (`take_jobs`) and
    ///   the shape is `Pending { showing_previous: true }`. The old draw keeps drawing, with its old
    ///   constants, until the program lands. In sync mode the queue is run before returning.
    ///
    /// A shape that is not `Drawing` (pending, failed, hidden) is always rebuilt. On any build error the
    /// previous draw stays on screen and the status is `Failed { showing_previous: true }`; the result is still
    /// `Ok(kind)`. A hidden material stops drawing the shape and keeps its draw for later. Paths the resolver
    /// did not return before are asked for again. `Err` only for an id that is not loaded.
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
                // A program still compiling for it is memoised when it lands, not applied.
                state.wanted = None;
                // Not updated while hidden: whatever returns it to the draw order must rewrite its fields.
                if let Some(d) = state.draw.as_mut() {
                    d.stale = true;
                }
            }
            self.sort_draw_order();
            return Ok(kind);
        }
        if kind == EditKind::Same && !rebuild {
            return Ok(kind);
        }
        self.tex_cache.retain(|_, t| t.is_some());
        let mut scratch = LoadReport::default();
        if rebuild {
            if let Some(s) = self.shapes.get_mut(&id) {
                s.material = m.clone();
            }
            self.begin(id, &m, textures, &mut scratch);
        } else {
            let outcome = self.apply_in_place(id, kind, &m, textures, &mut scratch);
            if let Some(state) = self.shapes.get_mut(&id) {
                state.status = match outcome {
                    Ok(()) => ShapeStatus::Drawing,
                    Err(error) => ShapeStatus::Failed {
                        error,
                        showing_previous: state.draw.is_some(),
                    },
                };
                state.material = m;
            }
        }
        self.sort_draw_order();
        if !self.async_compile {
            self.settle();
        }
        Ok(kind)
    }

    /// A `Cbuffer` or `Textures` edit on the shape's existing draw (same permutation, so the program is at
    /// hand).
    fn apply_in_place(
        &mut self,
        id: ShapeId,
        kind: EditKind,
        m: &MaterialState,
        textures: &dyn Fn(&str) -> Option<Vec<u8>>,
        rep: &mut LoadReport,
    ) -> Result<(), String> {
        if kind == EditKind::Textures {
            self.resolve_textures(m, textures, rep);
            let name = self.shapes[&id].name.clone();
            let w = self.wanted(m)?;
            let program = match self.lookup(&w) {
                Lookup::Ready(p) => p,
                Lookup::Failed(e) => return Err(e),
                Lookup::Waiting => return Err("the program is not compiled".into()),
            };
            let (srvs, bound) = self.bind_textures(&name, &program, m, rep)?;
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

(k) `compute_bounds`: shapes still waiting for a program count (so a first async load frames the mesh before any program lands). Replace
`for s in self.shapes.values().filter(|s| s.draw.is_some()) {` with

```rust
        for s in self
            .shapes
            .values()
            .filter(|s| s.draw.is_some() || matches!(s.status, ShapeStatus::Pending { .. }))
        {
```
(`let t = &s.mesh.transform;` and the rest stay; Task 2: its `world`.) Bounds are computed once at load and not recomputed by `apply_compiled`, so the camera does not jump when programs land; in sync mode `Pending` is gone by then and the result is identical to before.

(l) `Renderer::program`'s doc/uses: nothing else calls it now (`build_draw` and `apply_in_place` were its only callers). Remove any leftover `use`.

- [ ] **Step 7: Mechanical fixes of the old `Failed(String)` sites**

`crates/render3d/tests/edit.rs`, in `failing_permutation_keeps_old_draw` replace
```rust
        matches!(status(&r, peg), ShapeStatus::Failed(e) if e.starts_with("ps:")),
```
with
```rust
        matches!(status(&r, peg), ShapeStatus::Failed { error, showing_previous: true } if error.starts_with("ps:")),
```
`crates/render3d/tests/load.rs`, replace `matches!(&skull.status, ShapeStatus::Failed(e) if e.starts_with("ps:"))` with `matches!(&skull.status, ShapeStatus::Failed { error, .. } if error.starts_with("ps:"))`.
`crates/render3d/tests/frame.rs`, replace `render3d::ShapeStatus::Failed(_)` with `render3d::ShapeStatus::Failed { .. }`.
`crates/gui/src/ui/set_preview.rs`, in `fn shapes`, replace `if let ShapeStatus::Failed(e) = &i.status {` with `if let ShapeStatus::Failed { error: e, .. } = &i.status {` (Task 6 rewrites this block).
`crates/gui/tests/viewport3d.rs`: `v.handle(v3::V3Reply::Shapes(vec![info(ShapeStatus::Failed(\n        "compile".into(),\n    ))]));` becomes `info(ShapeStatus::Failed { error: "compile".into(), showing_previous: true })`; the keep-view reload test's `info(ShapeStatus::Failed(\n        "x".into(),\n    ))` becomes `info(ShapeStatus::Failed { error: "x".into(), showing_previous: false })`.
`crates/gui/tests/set_preview_ui.rs`: `status: ShapeStatus::Failed("X3000 bad register".into()),` becomes `status: ShapeStatus::Failed { error: "X3000 bad register".into(), showing_previous: false },`.

- [ ] **Step 8: Run to verify GREEN**

Run: `cargo test -p render3d`
Expected: PASS, including every pre-existing test untouched in behaviour (`edit`, `load`, `builtin`, `frame`, `lights`, `standin`, `compile`) plus `async_compile` (9 tests) and `pool` (2 tests).

- [ ] **Step 9: Gates**

Run: `cargo fmt --all && cargo clippy --workspace --all-targets -- -D warnings && cargo test --workspace`
Expected: all green (the gui crate compiles with the mechanical fixes of step 7).

- [ ] **Step 10: Commit**

```bash
git add crates/render3d/Cargo.toml crates/render3d/src/pool.rs crates/render3d/src/compile.rs crates/render3d/src/lib.rs crates/render3d/tests/async_compile.rs crates/render3d/tests/pool.rs crates/render3d/tests/edit.rs crates/render3d/tests/load.rs crates/render3d/tests/frame.rs crates/gui/src/ui/set_preview.rs crates/gui/tests/viewport3d.rs crates/gui/tests/set_preview_ui.rs
git commit -m "feat(render3d): background compile pool, Pending/Failed with showing_previous, old draw kept until the swap" -m "Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

---

## Task 6: gui `model/viewport3d.rs`: the render thread owns the pool; re-render on `Compiled`; pending is busy

**Files:**
- Modify: `crates/gui/src/model/viewport3d.rs` (anchors below)
- Modify: `crates/gui/src/ui/set_preview.rs` (`shapes()`: Pending lines, kept-look suffix)
- Test: `crates/gui/tests/viewport3d.rs` (append), `crates/gui/tests/set_preview_ui.rs` (append)

**Interfaces:**
- Consumes: Task 5 (`CompilePool`, `CompileDone`, `Renderer::{set_async_compile, take_jobs, apply_compiled, rebuild_device_on}`, `LoadReport.pending`, `ShapeStatus::{Pending, Failed}`, `render3d::pool::WORKERS`).
- Produces:
  ```rust
  pub enum V3Cmd { …, Compiled(render3d::CompileDone), Quit }
  pub enum V3Reply { …, Refresh { frame: Arc<Frame>, timing: FrameTiming } }   // a compile-triggered re-render; never touches render_done
  impl Viewport3d { pub fn compiling(&self) -> usize }                       // Pending shapes (until the first Shapes reply: report.pending)
  impl Drop for Viewport3d                                                    // sends Quit
  ```
- Decisions:
  1. **`held_frame` stays, narrowed.** A reload (`LoadParsed` of the same model with a new key, or a shaders/features reload) goes through `Renderer::load_parsed`, which discards the old draws: there is no `showing_previous` to keep drawing, so the only thing that keeps the picture is the model's own pixel frame. `held_frame` is now set when a reload lands with `report.pending > 0` (or compile errors) and a frame exists, and cleared by a `Shapes` reply that has a Drawing shape and no `Pending { showing_previous: false }` shape: the old image stays until the whole new content is ready, instead of being replaced by a half-built one. `Refresh` frames are ignored while it holds. Edits (`SetMaterial`) are different: their shapes are `Pending/Failed { showing_previous: true }`, the renderer itself keeps drawing the old draws, so the model keeps rendering camera moves as usual; `held_frame` is not involved.
  2. `renderable()` is unchanged: `Pending` is not `Hidden`, so it counts (that is the "first-load camera" rule for shapes; `report.shapes` counts pending for the report-only case).
  3. **`busy()` includes `compiling() > 0`**, so the existing 300 ms `show_spinner` shows for a long compile and the existing 16 ms/50 ms repaint loops run only while something is pending. Idle is exact: every pending shape is resolved by a `Compiled` that is always followed by a `Shapes` reply when anything changed; `compiling()` is 0 when the render thread is gone (`thread == None`), so a dead worker cannot leave the panel busy; a stale `Compiled` (content the user left) changes no shape, sends nothing and costs no repaint.
  4. The pool's sink sends `V3Cmd::Compiled` into the loop's own queue (one queue: a burst of results plus camera drags is one render). A refresh renders the last `(camera, w, h, look)` the loop saw and replies `Refresh` with an id from a private counter starting at `1 << 62` (never equal to a model-issued id, so the UI uploads it; kept out of `render_done` so the busy accounting is untouched).
  5. The Meshes view's `Content::Mesh` keeps the synchronous `Renderer::load` (the loop is a worker thread; a compile there never blocks the UI), so Mesh content never has `Pending` shapes and gets no `Shapes` replies, as before.

- [ ] **Step 1: Write the failing tests** — append to `crates/gui/tests/viewport3d.rs` (all helpers used exist in the file: `confirmed`, `parsed`, `loaded`, `info`, `mem`, `fp`):

```rust
// ---- P1-B Task 6: background compiles ----

fn load_with(v: &mut Viewport3d, cmds: &std::sync::mpsc::Receiver<v3::V3Cmd>, key: u64, keep: bool, shapes: usize, pending: usize) -> u64 {
    let nif = Arc::new(render3d::ParsedNif::default());
    v.set_content(Some(parsed(key, &nif, &[])), keep);
    v.request_frame(64, 64);
    let Ok(v3::V3Cmd::LoadParsed { id, .. }) = cmds.try_recv() else {
        panic!("load")
    };
    loaded(
        v,
        id,
        render3d::LoadReport {
            shapes,
            pending,
            ..Default::default()
        },
    );
    id
}

fn frame_of(id: u64, px: u8) -> Arc<v3::Frame> {
    Arc::new(v3::Frame {
        width: 1,
        height: 1,
        rgba: vec![px; 4],
        id,
    })
}

fn pending(previous: bool) -> ShapeStatus {
    ShapeStatus::Pending {
        showing_previous: previous,
    }
}

/// A first load of an uncached mesh has no drawing shape yet; the camera must still go out so the render
/// thread has something to redraw with when the programs land (03 §1.4).
#[test]
fn a_first_load_with_only_pending_shapes_still_gets_a_camera() {
    let (_d, mut v, cmds) = confirmed();
    load_with(&mut v, &cmds, 1, false, 1, 1);
    v.request_frame(64, 64);
    assert!(
        matches!(cmds.try_recv(), Ok(v3::V3Cmd::Render { .. })),
        "report.shapes counts the pending shape"
    );
    v.handle(v3::V3Reply::Shapes(vec![info(pending(false))]));
    v.orbit.drag(5.0, 0.0);
    v.request_frame(64, 64);
    assert!(matches!(cmds.try_recv(), Ok(v3::V3Cmd::Render { .. })));
}

#[test]
fn pending_shapes_are_busy_so_a_long_compile_shows_the_spinner_and_idle_does_not() {
    use std::time::{Duration, Instant};
    let (_d, mut v, cmds) = confirmed();
    load_with(&mut v, &cmds, 1, false, 1, 1);
    assert!(v.busy(), "report.pending before the first Shapes reply");
    assert_eq!(v.compiling(), 1);
    v.handle(v3::V3Reply::Shapes(vec![info(pending(false))]));
    assert!(v.busy());
    let t0 = Instant::now();
    assert!(!v.show_spinner(t0));
    assert!(!v.show_spinner(t0 + Duration::from_millis(299)));
    assert!(v.show_spinner(t0 + v3::SPINNER_DELAY), "a compile longer than 300 ms spins");
    v.handle(v3::V3Reply::Shapes(vec![info(ShapeStatus::Drawing)]));
    assert_eq!(v.compiling(), 0);
    assert!(!v.busy());
    assert!(!v.show_spinner(t0 + Duration::from_secs(5)));
    assert!(!v.poll(), "idle: nothing arrives, nothing to repaint for");
}

#[test]
fn a_dead_render_thread_does_not_leave_pending_shapes_busy() {
    let (_d, mut v, cmds) = confirmed();
    load_with(&mut v, &cmds, 1, false, 1, 1);
    v.handle(v3::V3Reply::Shapes(vec![info(pending(false))]));
    assert!(v.busy());
    v.handle(v3::V3Reply::Failed("gone".into()));
    assert!(!v.busy());
}

#[test]
fn a_refresh_frame_replaces_the_image_without_touching_the_render_accounting() {
    let (_d, mut v, cmds) = confirmed();
    load_with(&mut v, &cmds, 1, false, 1, 1);
    v.handle(v3::V3Reply::Shapes(vec![info(pending(false))]));
    v.request_frame(64, 64);
    let Ok(v3::V3Cmd::Render { id: rid, .. }) = cmds.try_recv() else {
        panic!("render")
    };
    v.handle(v3::V3Reply::Frame {
        frame: frame_of(rid, 1),
        timing: Default::default(),
    });
    v.handle(v3::V3Reply::Shapes(vec![info(ShapeStatus::Drawing)]));
    v.handle(v3::V3Reply::Refresh {
        frame: frame_of((1 << 62) + 1, 7),
        timing: Default::default(),
    });
    assert_eq!(v.frame.as_ref().unwrap().rgba, vec![7; 4]);
    // A real render sent now is still in flight: the refresh id did not advance render_done.
    v.orbit.drag(5.0, 0.0);
    v.request_frame(64, 64);
    assert!(v.busy());
}

/// A reload whose programs are all new has no old draws to keep (load_parsed discards them): the old image
/// stays until the new content is ready, not replaced by a half-built one.
#[test]
fn a_reload_with_pending_shapes_holds_the_old_image_until_they_all_resolve() {
    let (_d, mut v, cmds) = confirmed();
    load_with(&mut v, &cmds, 1, false, 1, 0);
    v.handle(v3::V3Reply::Shapes(vec![info(ShapeStatus::Drawing)]));
    v.frame = Some(frame_of(50, 9));
    load_with(&mut v, &cmds, 2, true, 2, 2);
    v.handle(v3::V3Reply::Shapes(vec![info(pending(false)), info(pending(false))]));
    assert_eq!(v.frame.as_ref().unwrap().rgba, vec![9; 4], "the old image stays");
    v.orbit.drag(5.0, 0.0);
    v.request_frame(64, 64);
    assert!(cmds.try_recv().is_err(), "no Render over the held image");
    // One shape lands, one still has no program: still held, refreshes ignored.
    v.handle(v3::V3Reply::Shapes(vec![info(ShapeStatus::Drawing), info(pending(false))]));
    v.handle(v3::V3Reply::Refresh {
        frame: frame_of((1 << 62) + 1, 7),
        timing: Default::default(),
    });
    assert_eq!(v.frame.as_ref().unwrap().rgba, vec![9; 4]);
    v.request_frame(64, 64);
    assert!(cmds.try_recv().is_err());
    // Everything resolved: released, the camera goes out, refreshes are taken again.
    v.handle(v3::V3Reply::Shapes(vec![info(ShapeStatus::Drawing), info(ShapeStatus::Drawing)]));
    v.request_frame(64, 64);
    assert!(matches!(cmds.try_recv(), Ok(v3::V3Cmd::Render { .. })));
    v.handle(v3::V3Reply::Refresh {
        frame: frame_of((1 << 62) + 2, 5),
        timing: Default::default(),
    });
    assert_eq!(v.frame.as_ref().unwrap().rgba, vec![5; 4]);
}

/// An edit's shapes keep their old draw in the renderer, so camera moves keep rendering while it compiles.
#[test]
fn an_edit_waiting_for_its_shader_keeps_the_camera_live_on_the_old_draw() {
    let (_d, mut v, cmds) = confirmed();
    let id = load_with(&mut v, &cmds, 1, false, 1, 0);
    v.handle(v3::V3Reply::Shapes(vec![info(ShapeStatus::Drawing)]));
    assert!(v.set_material(ShapeId(0), MaterialState::builtin_default()));
    let _ = cmds.try_recv();
    v.handle(v3::V3Reply::MaterialSet {
        content: id,
        id: ShapeId(0),
        result: Ok(EditKind::Permutation),
        ms: 1.0,
    });
    v.handle(v3::V3Reply::Shapes(vec![info(pending(true))]));
    v.orbit.drag(10.0, 0.0);
    v.request_frame(64, 64);
    let Ok(v3::V3Cmd::Render { id: rid, .. }) = cmds.try_recv() else {
        panic!("render over the old draw")
    };
    v.handle(v3::V3Reply::Frame {
        frame: frame_of(rid, 3),
        timing: Default::default(),
    });
    assert!(v.busy(), "the compile is still running");
    v.handle(v3::V3Reply::Shapes(vec![info(ShapeStatus::Drawing)]));
    assert!(!v.busy());
}

#[test]
fn dropping_the_viewport_tells_the_render_thread_to_quit() {
    let (_d, v, cmds) = confirmed();
    drop(v);
    assert!(matches!(cmds.try_iter().last(), Some(v3::V3Cmd::Quit)));
}

// ---- P1-B Task 6: a real render thread, the repo stand-in shader, the real pool ----

fn standin_root() -> PathBuf {
    PathBuf::from(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../render3d/tests/fixtures/standin"
    ))
}

fn real_viewport(cache: &Path, material: MaterialState) -> Viewport3d {
    let f = render3d::locate::fingerprint(&standin_root(), None).unwrap();
    let mut v = Viewport3d::new(Some(f.clone()));
    v.on_located(Ok(Some(f)));
    v.cache_dir = Some(cache.to_path_buf());
    v.set_content(
        Some(v3::Content::Builtin {
            key: 1,
            shape: Builtin::Sphere,
            material,
            textures: Arc::new(HashMap::new()),
        }),
        false,
    );
    v
}

/// Drives the UI-thread side until nothing is in flight (the way the app's frames do).
fn run_idle(v: &mut Viewport3d) {
    let t = std::time::Instant::now();
    loop {
        v.poll();
        v.request_frame(64, 64);
        if !v.busy() && !v.shapes.is_empty() {
            return;
        }
        assert!(t.elapsed().as_secs() < 60, "never went idle: {:?}", v.shapes);
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
}

#[test]
fn a_real_load_compiles_in_the_background_then_draws_and_goes_idle() {
    let cache = tempfile::tempdir().unwrap();
    let mut m = MaterialState::builtin_default();
    m.alpha = Some(nif::props::Alpha {
        flags: 0x200,
        threshold: 0,
    }); // DO_ALPHA_TEST: the stand-in's pixel permutation compiles
    let mut v = real_viewport(cache.path(), m);
    run_idle(&mut v);
    assert_eq!(v.shapes[0].status, ShapeStatus::Drawing);
    // The refresh frame follows the Shapes reply on the same queue; wait for the lit image.
    let lit = |v: &Viewport3d| {
        v.frame.as_ref().is_some_and(|f| {
            f.rgba.chunks(4).any(|p| p[0] as u32 + p[1] as u32 + p[2] as u32 > 0)
        })
    };
    let t = std::time::Instant::now();
    while !lit(&v) && t.elapsed().as_secs() < 10 {
        v.poll();
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
    assert!(lit(&v), "the compile-triggered refresh drew the sphere");
    // Idle stays idle: no endless repaints.
    std::thread::sleep(std::time::Duration::from_millis(100));
    v.poll();
    for _ in 0..5 {
        std::thread::sleep(std::time::Duration::from_millis(20));
        assert!(!v.poll());
        assert!(!v.busy());
    }
}

#[test]
fn a_real_compile_failure_is_listed_and_the_panel_goes_idle() {
    let cache = tempfile::tempdir().unwrap();
    let mut m = MaterialState::builtin_default();
    m.alpha = None; // the stand-in's pixel permutation without DO_ALPHA_TEST does not compile
    let mut v = real_viewport(cache.path(), m);
    run_idle(&mut v);
    assert!(matches!(
        &v.shapes[0].status,
        ShapeStatus::Failed { error, showing_previous: false } if error.starts_with("ps:")
    ));
    assert!(v.frame.is_some(), "the background frame was shown meanwhile");
    assert!(!v.busy());
}
```
(`confirmed`, `parsed`, `info`, `loaded`, `mem` exist earlier in the file; `Builtin`, `HashMap`, `Arc`, `MaterialState`, `EditKind`, `ShapeId`, `ShapeInfo`, `ShapeStatus`, `PathBuf`, `Path` are already imported at the top or in the "Task 12" `use` block.)

Append to `crates/gui/tests/set_preview_ui.rs`:

```rust
#[test]
fn a_shader_still_compiling_is_listed_and_a_kept_look_is_said() {
    let (_d, mut h) = plain_rock();
    h.state_mut().1.v3.shapes = vec![
        ShapeInfo {
            id: ShapeId(1),
            name: "Bottle".into(),
            material: MaterialState::builtin_default(),
            status: ShapeStatus::Pending {
                showing_previous: true,
            },
        },
        ShapeInfo {
            id: ShapeId(2),
            name: "Cork".into(),
            material: MaterialState::builtin_default(),
            status: ShapeStatus::Failed {
                error: "X3000 bad register".into(),
                showing_previous: true,
            },
        },
    ];
    h.run_steps(4);
    h.get_by_label("Bottle: compiling…");
    h.get_by_label("Cork: shader failed: X3000 bad register (previous look kept)");
}
```

- [ ] **Step 2: Run to verify RED**

Run: `cargo test -p sdc-gui --test viewport3d --test set_preview_ui`
Expected: FAIL to compile (`no variant V3Reply::Refresh`, `V3Cmd::Quit`, `no method compiling`).

- [ ] **Step 3: `model/viewport3d.rs`: commands and replies.** Imports: change `use render3d::{\n    EditKind, FrameTiming, LoadReport, ParsedNif, Renderer, ShapeId, ShapeInfo, ShapeStatus,\n};` to include the pool and the device:

```rust
use render3d::device::Gpu;
use render3d::{
    CompileDone, CompilePool, EditKind, FrameTiming, LoadReport, ParsedNif, Renderer, ShapeId,
    ShapeInfo, ShapeStatus,
};
```
In `pub enum V3Cmd` append after the `Render { .. }` variant:

```rust
    /// A finished background compile, sent by the pool into the render loop's own queue.
    Compiled(CompileDone),
    /// The model is gone: end the loop (the pool's sink holds a sender to the loop's queue, so the channel no
    /// longer closes by itself).
    Quit,
```
In `pub enum V3Reply` append after `Frame { .. }`:

```rust
    /// A frame the render thread made on its own because a compile landed (same camera as the last `Render`).
    /// Its id comes from a private range, so the UI uploads it; it never counts as an answered `Render`.
    Refresh {
        frame: Arc<Frame>,
        timing: FrameTiming,
    },
```

- [ ] **Step 4: `model/viewport3d.rs`: the model.**

(a) `busy()` and `compiling()` — replace `busy` with:

```rust
    /// A load, a render, a material edit, a locate or a background compile is in flight.
    pub fn busy(&self) -> bool {
        self.locating
            || self.pending_load.is_some()
            || self.render_done < self.render_sent
            || self.edits_done < self.edits_sent
            || self.compiling() > 0
    }

    /// Shapes waiting for a shader program: the latest `Shapes` reply, or the load report until the first one.
    /// Zero when the render thread is gone, so a dead worker cannot keep the panel busy.
    pub fn compiling(&self) -> usize {
        if self.thread.is_none() {
            return 0;
        }
        if self.shapes.is_empty() {
            return self.report.as_ref().map_or(0, |r| r.pending);
        }
        self.shapes
            .iter()
            .filter(|s| matches!(s.status, ShapeStatus::Pending { .. }))
            .count()
    }
```
(b) `Drop` — add after the `impl Viewport3d { .. }` block that ends before `pub fn chosen_root`:

```rust
impl Drop for Viewport3d {
    fn drop(&mut self) {
        if let Some(t) = &self.thread {
            let _ = t.send(V3Cmd::Quit);
        }
    }
}
```
(c) `ensure_thread`: replace the spawn with

```rust
        let (ctx, crx) = channel();
        let features = self.features_on;
        let me = ctx.clone();
        spawn_guarded(self.tx.clone(), "viewport3d", move |tx| {
            render_loop(crx, me, tx, shaders, cache, features)
        });
        self.thread = Some(ctx);
```
(d) `handle`, `Loaded` arm: replace the `reload_failed` block (from `let reload_failed = ...` through `self.held_frame = ...;`) with

```rust
                    let reload = self.loaded == Some(id) || self.kept_view;
                    let reload_failed = reload && !report.compile_errors.is_empty();
                    // Programs still compiling after a reload: load_parsed dropped the old draws, so the old
                    // image is all that is left to show (held until the new content is ready).
                    let reload_pending = reload && report.pending > 0;
                    self.pending_load = None;
                    self.loaded = Some(id);
                    self.held_frame = (reload_failed || reload_pending) && self.frame.is_some();
```
and update the comment above it to: `// A reload of the same mesh (shaders or features changed) keeps its id. When its permutations fail or are still compiling, the last frame stays (Review Focus #3).` Keep the following `if (report.le_mesh || report.shapes == 0) && !reload_failed { self.frame = None; }` as is.

(e) `handle`, add after the `Frame` arm:

```rust
            V3Reply::Refresh { frame, timing } => {
                // Not an answered Render: render_done is untouched. Ignored while the old image is held.
                if self.loaded.is_some() && self.pending_load.is_none() && !self.held_frame {
                    self.render_error = None;
                    self.frame = Some(frame);
                    self.timing = Some(timing);
                }
            }
```
(f) `handle`, `Shapes` arm: replace its inner `if s.iter().any(..) { self.held_frame = false; }` with

```rust
                    let first_time = |x: &ShapeInfo| {
                        matches!(x.status, ShapeStatus::Pending { showing_previous: false })
                    };
                    if s.iter().any(|x| x.status == ShapeStatus::Drawing) && !s.iter().any(first_time) {
                        self.held_frame = false;
                    }
```
(g) `field doc` of `held_frame`: replace its doc comment with `/// A reload left shapes with no draw (failed, or still compiling): the previous image stays and no render goes out until they have resolved (a render would blank or half-fill it).`

- [ ] **Step 5: `model/viewport3d.rs`: the render loop** — replace everything from the doc comment `/// The render thread: owns the `Renderer`; batches queued commands, ...` to the end of the file with:

```rust
/// The render thread: owns the `Renderer` and the compile pool; batches queued commands, applies material edits
/// after the load they follow, and draws only the latest camera. Compile results arrive on the same queue
/// (`V3Cmd::Compiled`, sent by the pool's sink), so a burst of them and a camera drag still collapse into one
/// render. A result that changes a shape makes a `Shapes` reply and, with no `Render` in the batch, a `Refresh`
/// frame from the last camera.
fn render_loop(
    rx: Receiver<V3Cmd>,
    self_tx: Sender<V3Cmd>,
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
        r.set_async_compile(true);
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
    let mut r = match make(shaders, features) {
        Ok(r) => r,
        Err(e) => return send(V3Reply::Failed(e)),
    };
    send(V3Reply::Ready {
        adapter: r.gpu().adapter.clone(),
        warp: r.gpu().warp,
    });
    let pool = CompilePool::new(render3d::pool::WORKERS, move |d| {
        let _ = self_tx.send(V3Cmd::Compiled(d));
    });
    // Jobs queued by a load or an edit go to the workers.
    let pump = |r: &mut Renderer| {
        for job in r.take_jobs() {
            pool.submit(job);
        }
    };
    // After a device loss: a new device (maybe WARP) under the same renderer state (shaders, features and the
    // compiled programs are kept, so nothing recompiles), `Ready`, the last content loaded again with its
    // edits, and its `Loaded`. Err when no device can be made at all.
    let rebuild = |r: &mut Renderer,
                   last: &Option<(u64, Content)>,
                   edits: &BTreeMap<ShapeId, MaterialState>|
     -> Result<(), String> {
        r.rebuild_device_on(Gpu::new(None)?)?;
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
    // The last camera, size and look drawn: what a compile-triggered refresh renders with.
    let mut view: Option<(Camera, u32, u32, Look)> = None;
    let mut refresh_id: u64 = 1 << 62;
    while let Ok(first) = rx.recv() {
        let mut reload = false;
        let mut render = None;
        let mut compiled_changed = false;
        let mut pending: Vec<(u64, ShapeId, MaterialState)> = vec![];
        for c in std::iter::once(first).chain(rx.try_iter()) {
            let content = match c {
                V3Cmd::Quit => return,
                V3Cmd::Compiled(done) => {
                    compiled_changed |= r.apply_compiled(done);
                    None
                }
                V3Cmd::SetShaders(f) => {
                    r.set_shaders(f);
                    reload = true;
                    None
                }
                V3Cmd::SetFeatures(on) => {
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
                V3Cmd::SetMaterial {
                    content,
                    id,
                    material,
                } => {
                    pending.push((content, id, material));
                    None
                }
                V3Cmd::Render {
                    id,
                    camera,
                    w,
                    h,
                    look,
                } => {
                    render = Some((id, camera, w, h, look));
                    None
                }
            };
            if let Some(c) = content {
                // A new load drops the queued render and the edits made for the old content (the model reset its
                // counters when it replaced the content). Results already applied or still in flight for the old
                // content are keyed by program, not by shape: they only warm the memo.
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
                    if let Err(e) = rebuild(&mut r, &last, &edits) {
                        return send(V3Reply::Failed(e));
                    }
                } else {
                    loaded(&r, *id, c, report);
                }
            }
        }
        let pending_was_empty = pending.is_empty();
        if !pending.is_empty() {
            if let Some((lid, c)) = &last {
                for (content, id, m) in pending {
                    if content != *lid {
                        continue;
                    }
                    let t0 = std::time::Instant::now();
                    let result = edit(&mut r, c, id, m.clone());
                    if result.is_ok() {
                        edits.insert(id, m);
                    }
                    send(V3Reply::MaterialSet {
                        content,
                        id,
                        result,
                        ms: t0.elapsed().as_secs_f64() * 1000.0,
                    });
                }
                send(V3Reply::Shapes(r.shapes()));
            }
        }
        pump(&mut r);
        // A landed program changed a shape and nothing above reported it: tell the panel.
        if compiled_changed
            && !reload
            && pending_was_empty
            && !matches!(last, Some((_, Content::Mesh(_))))
        {
            send(V3Reply::Shapes(r.shapes()));
        }
        let (id, camera, w, h, look, refresh) = match (render, view) {
            (Some((id, camera, w, h, look)), _) => (id, camera, w, h, look, false),
            (None, Some((camera, w, h, look))) if compiled_changed => {
                refresh_id += 1;
                (refresh_id, camera, w, h, look, true)
            }
            _ => continue,
        };
        view = Some((camera, w, h, look));
        let scene = look.scene(camera, r.bounds());
        let mut result = r.render(&scene, w, h);
        if let Err(e) = &result {
            if e.starts_with(render3d::device::DEVICE_REMOVED) {
                // Device lost (TDR, driver update): a new device (maybe WARP), the content again, one retry.
                if let Err(e) = rebuild(&mut r, &last, &edits) {
                    return send(V3Reply::Failed(e));
                }
                pump(&mut r);
                result = r.render(&scene, w, h);
            }
        }
        match result {
            Ok(rgba) => {
                let frame = Arc::new(Frame {
                    width: w,
                    height: h,
                    rgba,
                    id,
                });
                let timing = r.last_timing();
                if refresh {
                    send(V3Reply::Refresh { frame, timing });
                } else {
                    send(V3Reply::Frame { frame, timing });
                }
            }
            // A failed refresh is dropped: the next real Render reports the error with an id the model knows.
            Err(message) if !refresh => send(V3Reply::RenderError { id, message }),
            Err(_) => {}
        }
    }
}
```
`fn load` (the free fn above) is unchanged: `Content::Mesh` keeps `r.load(..)` (settled), `Parsed`/`Builtin` use `load_parsed`/`load_builtin`, which in the loop's async mode leave `Pending` shapes and queue jobs for `pump`.

- [ ] **Step 6: `ui/set_preview.rs`** — in `fn shapes`, replace the first loop (Task 5's mechanical version) with

```rust
    // Spec §1 criterion 4: a shader that failed to compile is named here, and one still compiling says so.
    for i in &s.v3.shapes {
        match &i.status {
            ShapeStatus::Failed {
                error,
                showing_previous,
            } => {
                let kept = if *showing_previous {
                    " (previous look kept)"
                } else {
                    ""
                };
                ui.colored_label(RED, format!("{}: shader failed: {error}{kept}", i.name));
            }
            ShapeStatus::Pending { .. } => {
                ui.label(format!("{}: compiling…", i.name));
            }
            ShapeStatus::Drawing | ShapeStatus::Hidden => {}
        }
    }
```

- [ ] **Step 7: Run to verify GREEN**

Run: `cargo test -p sdc-gui --test viewport3d --test set_preview_ui --test set_preview_model`
Expected: PASS, including the two real-render-thread tests (they use the repo stand-in shader and WARP/the default adapter; no owner data).

- [ ] **Step 8: Gates**

Run: `cargo fmt --all && cargo clippy --workspace --all-targets -- -D warnings && cargo test --workspace`
Expected: all green. Check clippy for the unused `features` assignment: the loop no longer stores `features` after the first `make` (the renderer keeps them through `rebuild_device_on`).

- [ ] **Step 9: Commit**

```bash
git add crates/gui/src/model/viewport3d.rs crates/gui/src/ui/set_preview.rs crates/gui/tests/viewport3d.rs crates/gui/tests/set_preview_ui.rs
git commit -m "feat(gui): render thread owns the compile pool; re-render on Compiled; pending shapes are busy, listed in the set preview" -m "Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

---

## Review focus pinned by this part

- T-E3 `permutation_edit_keeps_old_draw_until_compiled` and T-E4 `failing_permutation_keeps_old_draw` (render3d): byte-identical frames while Pending, Failed keeps the old pixels, others unaffected.
- T-E6 `out_of_order_results` and `a_result_for_a_replaced_load_only_warms_the_memo`: results are keyed by program, never applied to the wrong shape.
- T-C1 `key_ignores_the_root_path` / `editing_an_included_file_changes_the_key`; T-C2 `over_the_cap_the_oldest_entries_go_first`, `a_hit_refreshes_the_entry_age`, `a_pruned_entry_is_recompiled` (a pruned or torn pair is a miss).
- gui: `a_reload_with_pending_shapes_holds_the_old_image_until_they_all_resolve`, `pending_shapes_are_busy_…`, `a_dead_render_thread_does_not_leave_pending_shapes_busy`, and the two real-thread tests (no endless repaints once idle).

## Known limits (record in the ledger, not fixed here)

- In async mode a kind-mismatch warning ("not a Cube texture … default bound") raised while a late program lands is dropped (`apply_compiled` has no `LoadReport`); sync loads report it as before.
- `warn_missing_fields` runs at load only, over the draws that exist then; in an async first load it sees none until programs land (the check returns early on zero draws).
- A panic inside the compiler becomes an `Err` that is memoised for the session (no retry until the shaders change or the app restarts).
- Old cache entries from before Task 3 are orphans until Task 4's prune removes them (90 days or over the cap).


---

# Part H: texture hot-swap, instant A/B, owner verification (Tasks 7-9)

Written against `nif/main @ 8159bdf` (worktree `F:\ai\dds\converter-nif-p1`). Line numbers are hints; the quoted text anchors rule. Executed on `nif/b-p1b` after Tasks 1–6 (7 needs 5; 8 needs 6 and 7; 9 is last).

## CONTRACT ISSUES

1. **Extra public surface, additive only** (the contract fixes the minimum; these are needed to make "Encoded ↔ Variant::Encoded" true for the first load): `Renderer::set_load_variant(Variant)` (which variant the load resolver's bytes count as), `V3Cmd::SetLoadVariant(Variant)`, `Viewport3d::{swap_texture, set_texture_variant, set_load_variant}`, `Viewport3d.swap_errors`, `V3Reply::TextureSwap`, `Encoder::{ab_active, scene_dirty}`. Without the load hint the resolver's bytes (current-mode bytes) would sit in the `Source` slot even in Encoded mode, and the first flip would overwrite the picture on screen.
2. `Variant` also derives `Hash, PartialOrd, Ord` (the render thread keeps its swap log in a `BTreeMap`). Superset of the contract's derive list.
3. `replace_texture(path, &[], v)` drops variant `v`'s copy for **both** variants (contract: "empty dds + Encoded"). The set preview must drop a stale inactive `Source` copy too.
4. `replace_texture` is atomic on a kind mismatch: `Err` and **nothing** is stored (contract: "keeps the old SRV and returns a warning/Err"). A variant that is the entry's only texture becomes active (a swap into a path that was missing at load shows at once); otherwise a swap into the inactive variant only stores.
5. The contract's decision "after the active variant is Fresh, the inactive one encodes in the background" is refined: at the **first flip** the old mode's Fresh bytes are kept as the inactive copy (no encode needed); the background encode of the inactive mode is only needed when that copy is missing or was invalidated (source change, settings change, cap change, flip while still encoding). Tests pin both.
6. Design 03 §2 step 4 checks the kind against `srv_kind_at(ps_reflection, reg)`; a `ShapeDraw` does not keep its reflection and Task 5 reshapes `build_draw`, so the check reads the kind from the bound view's dimension (`textures::view_kind`). Equivalent: `bind_textures` only binds views whose kind equals the reflected one and defaults are made by reflected kind.
7. Task 9: in a TruePBR set a **shader-type** change alone does not change the define set (`permutation::defines` drops ENVMAP/MLP under TRUE_PBR), so it is not a permutation edit. The checklist forces recompiles with the `glint` entry key (adds `GLINT`) and the viewport's "Community Shaders features" toggle (changes `ENABLE_LL` for every shape).

## SHARED-FILE EDITS NEEDED

None. Files touched: `crates/render3d/{src/lib.rs, src/textures.rs, tests/hotswap.rs (new), tests/common/mod.rs, tests/fixtures/standin/Lighting.hlsl}`, `crates/gui/{src/model/viewport3d.rs, src/model/set_preview.rs, tests/viewport3d.rs, tests/set_preview_model.rs, tests/set_preview_ui.rs}`.

## Anchors earlier tasks may shift

- **Task 5 (render3d `lib.rs`)** reshapes `ShapeStatus` (`Failed(String)` → `Failed { error, showing_previous }`, adds `Pending { .. }`), `build_draw`/`program`, `set_material`, `load_parsed`. Task 7 only relies on: the `tex_cache` field, `fn texture`, the line `self.tex_cache.retain(|_, t| t.is_some());` in `set_material`, `self.tex_cache.clear()` in `load_parsed`, `ShapeState { name, material, draw, status }`, `ShapeStatus::Drawing`, `Renderer::default_srv`, `linear_lighting`, `ShapeDraw::{srvs, bound, rewrite_material}`. If Task 5 renamed one of these, adjust the quoted text, not the logic. Task 5's `bind_textures` must keep calling `self.texture(..)` for every slot read (it does in 8159bdf).
- **Task 6 (gui `viewport3d.rs`)** adds `V3Cmd::Compiled`, a pool in `render_loop`, may replace `held_frame`, and edits `V3Reply::Loaded`/`Shapes` handling. Task 8 anchors on: the `V3Cmd` enum's existing variants (`SetMaterial`, `Render`), `V3Reply::MaterialSet`, the `Viewport3d` struct's `edits_done` field and its initialiser, `ensure_thread` (`self.thread = Some(ctx);`), `request_frame`'s `if self.need_load {` branch (`.load_cmd(id);`), `fn handle`'s `V3Reply::MaterialSet {` arm, `fn load(r: &mut Renderer, c: &Content, edits: ..)`, the render loop's `for c in std::iter::once(first).chain(rx.try_iter())` command match, `let mut pending: Vec<(u64, ShapeId, MaterialState)> = vec![];`, `if !pending.is_empty() {`, and the `rebuild` closure. None of these should move; if Task 6 changed the `rebuild`/`load(..)` call sites, give the new `load_all` the same arguments they pass today.
- **Task 6 (`set_preview.rs` panel)** only touches `ui/set_preview.rs`; Task 8's `set_preview.rs` edits are in `Encoder`, `SetPreview::{new, set_mode, poll_at, push_scene}` and new private fns.

---

## Task 7: render3d texture hot-swap (`TexEntry`, `Variant`, `replace_texture`, `set_texture_variant`)

> **Reconciliation R1 (binding):** this task was drafted against 8159bdf, before Task 5. The swap walk below filters on `s.status == ShapeStatus::Drawing`; after Task 5 select every shape with `s.draw.is_some()` that is not `Hidden` (Pending/Failed with `showing_previous` still draw their previous `ShapeDraw`). Add a test: a shape whose permutation edit is withheld (Pending, previous draw kept) receives a swap and its pixels change. Status matches use Task 5's `Failed { error, showing_previous }` / `Pending { .. }` forms (R4).

**Files:**
- Modify: `crates/render3d/src/lib.rs`, `crates/render3d/src/textures.rs`
- Modify (test support): `crates/render3d/tests/common/mod.rs` (two DDS builders), `crates/render3d/tests/fixtures/standin/Lighting.hlsl` (an emissive register)
- Create: `crates/render3d/tests/hotswap.rs`

**Interfaces:**
```rust
// render3d (lib.rs)
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Default)]
pub enum Variant { #[default] Source, Encoded }
impl Variant { pub fn other(self) -> Variant }
impl Renderer {
    /// Which variant the resolver's bytes of the NEXT loads count as (default Source). Sticky.
    pub fn set_load_variant(&mut self, v: Variant);
    /// New texture + SRV from `dds` (empty = drop that variant's copy); rebinds every draw that reads `path`
    /// through the entry's active variant; re-runs the material write where the set of real textures changed.
    /// Err (nothing changed) on a bad DDS or a kind the register does not read.
    pub fn replace_texture(&mut self, path: &str, dds: &[u8], v: Variant) -> Result<Vec<(ShapeId, u32)>, String>;
    /// Makes `v` the bound variant. True = a binding changed (false: already active, no such copy, no draw reads it).
    pub fn set_texture_variant(&mut self, path: &str, v: Variant) -> bool;
    #[doc(hidden)] pub fn draw_identity(&self, id: ShapeId) -> Option<usize>;   // test hook: the index buffer's COM pointer
}
// textures.rs
#[derive(Clone)] pub struct Texture { .. }            // gains Clone
pub fn view_kind(v: &ID3D11ShaderResourceView) -> Option<SrvKind>;
```
Behaviour pinned by the tests:
- A swap never reloads the mesh: `draw_identity` is unchanged, no shader is compiled.
- `TexEntry { source, encoded, active }` per canonical path replaces the old `Option<(srv, kind)>` cache. A texture the resolver returns at load is stored under `load_variant` (default `Source`) and is the active one; a path the resolver did not return is an entry with both copies `None` (a load asks once, `set_material` asks again — as today).
- A shape that is not `Drawing` (hidden, failed, pending, no draw) is not touched: its old draw keeps the old cbuffers; its next build reads the entry.

- [ ] **Step 1: Test support (compiles today, changes no pixel)**

Append to `crates/render3d/tests/common/mod.rs`:

```rust
/// An uncompressed DX10-header DDS, one mip: `R8G8B8A8_UNORM` (28) or `R8G8B8A8_UNORM_SRGB` (29). The same texels in
/// the two formats sample differently on the GPU (the sRGB view linearises), which is what the A/B shows.
pub fn dds_rgba8_dx10(w: u32, h: u32, texels: &[[u8; 4]], srgb: bool) -> Vec<u8> {
    assert_eq!(texels.len(), (w * h) as usize);
    let mut b = vec![0u8; 148];
    let mut put = |at: usize, v: u32| b[at..at + 4].copy_from_slice(&v.to_le_bytes());
    put(0, u32::from_le_bytes(*b"DDS "));
    put(4, 124);
    put(8, 0x1 | 0x2 | 0x4 | 0x8 | 0x1000); // CAPS | HEIGHT | WIDTH | PITCH | PIXELFORMAT
    put(12, h);
    put(16, w);
    put(20, w * 4);
    put(28, 1);
    put(76, 32);
    put(80, 0x4); // DDPF_FOURCC
    put(84, u32::from_le_bytes(*b"DX10"));
    put(108, 0x1000); // DDSCAPS_TEXTURE
    put(128, if srgb { 29 } else { 28 });
    put(132, 3); // D3D10_RESOURCE_DIMENSION_TEXTURE2D
    put(140, 1); // array size
    b.extend(texels.iter().flatten());
    b
}

/// A 1×1 legacy cube map (all six faces `texel`): a kind a 2D register must refuse.
pub fn dds_cube_rgba8(texel: [u8; 4]) -> Vec<u8> {
    let mut b = dds_rgba8(1, 1, &[texel]);
    b[108..112].copy_from_slice(&0x1008u32.to_le_bytes()); // TEXTURE | COMPLEX
    b[112..116].copy_from_slice(&(0x200u32 | 0xFC00).to_le_bytes()); // CUBEMAP | all six faces
    for _ in 0..5 {
        b.extend(texel);
    }
    b
}
```

In `crates/render3d/tests/fixtures/standin/Lighting.hlsl` (PS section), replace

```hlsl
Texture2D<float4> DiffuseMap : register(t0);
Texture2D<float4> NormalMap : register(t1);
SamplerState DiffuseSampler : register(s0);
```
with
```hlsl
Texture2D<float4> DiffuseMap : register(t0);
Texture2D<float4> NormalMap : register(t1);
// t6 is where render3d binds a TruePBR emissive map (slot 2). STANDIN_FLAT reads it behind a test that is never true
// (EmitColor.w is never written), so the register is reflected and bindable but no pixel changes: the hot-swap tests
// see HasEmissive flip through the PBRFlags green channel.
Texture2D<float4> EmissiveMap : register(t6);
SamplerState DiffuseSampler : register(s0);
```
and replace the `STANDIN_FLAT` return
```hlsl
    return float4(0.25 * base.rgb * i.Color.rgb + EmitColor.rgb + float3(0.0, flagGreen, 0.0), base.a * 0.5);
```
with
```hlsl
    float3 emissive = EmitColor.a < -1.0 ? EmissiveMap.SampleLevel(DiffuseSampler, i.Uv, 0).rgb : float3(0.0, 0.0, 0.0);
    return float4(0.25 * base.rgb * i.Color.rgb + EmitColor.rgb + emissive + float3(0.0, flagGreen, 0.0), base.a * 0.5);
```
Add to the header comment under `STANDIN_FLAT`: `plus an unread t6 emissive map (see EmissiveMap)`.

- [ ] **Step 2: Run the existing render3d suite: nothing changed**

Run: `cargo test -p render3d`
Expected: PASS (the new register is black by default and never sampled). If a test fails because it counts SRVs of the stand-in, update that count (`grep -rn "srvs" crates/render3d/tests` showed only `tiny.hlsl`, which is a different fixture).

- [ ] **Step 3: Write the failing tests** — create `crates/render3d/tests/hotswap.rs`:

```rust
//! Texture hot-swap and the Source/Encoded variants (03 §2, tests T-H1..T-H4) on WARP with the stand-in shader.
//! `STANDIN_FLAT`: pixels = 0.25·diffuse + EmitColor (+ 0.5 green when PBRFlags ≠ 0).
mod common;
use common::{
    cache_files, channel, dds_cube_rgba8, dds_rgba8, dds_rgba8_dx10, fixture, renderer, FLAT,
};
use nif::material::MaterialState;
use render3d::scene::Scene;
use render3d::{EditKind, Renderer, ShapeId, Variant};
use std::path::Path;

const CYL1: ShapeId = ShapeId(9);
const PATH: &str = "textures/standin/swap.dds";
const EMIT: &str = "textures/standin/emit.dds";

fn material(r: &Renderer) -> MaterialState {
    r.shapes()
        .into_iter()
        .find(|s| s.id == CYL1)
        .unwrap()
        .material
}

fn frame(r: &mut Renderer) -> Vec<u8> {
    r.render(&Scene::neutral(), 64, 64).unwrap()
}

/// The furniture NIF with cylinder_1's diffuse naming `PATH`, resolved to `bytes` (loaded as the Source variant).
fn with_texture(cache: &Path, bytes: Vec<u8>) -> Renderer {
    let mut r = renderer(cache, FLAT);
    let rep = r.load(&fixture("TestNifFile_Furniture_Col_SE.nif"), &|_| None);
    assert_eq!(rep.shapes, 2, "{rep:?}");
    let mut m = material(&r);
    m.set_slot_raw(0, "Textures\\Standin\\Swap.dds");
    let resolve = |p: &str| (p == PATH).then(|| bytes.clone());
    assert_eq!(r.set_material(CYL1, m, &resolve), Ok(EditKind::Textures));
    r
}

/// T-H1: new bytes for a bound path show on the next frame; the mesh buffers are the same objects and nothing is
/// compiled.
#[test]
fn hot_swap_replaces_the_srv_without_a_reload() {
    let cache = tempfile::tempdir().unwrap();
    let mut r = with_texture(cache.path(), dds_rgba8(1, 1, &[[255, 0, 0, 255]]));
    let before = frame(&mut r);
    let files = cache_files(cache.path());
    let mesh = r.draw_identity(CYL1).expect("a draw");
    let touched = r
        .replace_texture(PATH, &dds_rgba8(1, 1, &[[0, 255, 0, 255]]), Variant::Source)
        .unwrap();
    assert_eq!(touched, vec![(CYL1, 0)]);
    let after = frame(&mut r);
    assert!(
        channel(&after, 1) > channel(&before, 1) + 1000,
        "green rose: {} → {}",
        channel(&before, 1),
        channel(&after, 1)
    );
    assert!(channel(&after, 0) < channel(&before, 0), "red fell");
    assert_eq!(r.draw_identity(CYL1), Some(mesh), "no reload: same buffers");
    assert_eq!(cache_files(cache.path()), files, "nothing was compiled");
    // A path no shape reads is stored, touches nothing, and cannot be flipped to a copy it lacks.
    let unused = "textures/standin/unused.dds";
    let px = dds_rgba8(1, 1, &[[1, 2, 3, 255]]);
    assert!(r.replace_texture(unused, &px, Variant::Source).unwrap().is_empty());
    assert!(!r.set_texture_variant(unused, Variant::Encoded));
}

/// T-H2: a cube into a 2D slot, or bytes that do not decode, change nothing.
#[test]
fn hot_swap_kind_mismatch_keeps_the_old_binding() {
    let cache = tempfile::tempdir().unwrap();
    let mut r = with_texture(cache.path(), dds_rgba8(1, 1, &[[255, 0, 0, 255]]));
    let before = frame(&mut r);
    let e = r
        .replace_texture(PATH, &dds_cube_rgba8([0, 0, 255, 255]), Variant::Source)
        .unwrap_err();
    assert!(e.contains("Cube"), "{e}");
    assert_eq!(frame(&mut r), before, "the old texture stays");
    assert!(r
        .replace_texture(PATH, b"not a dds", Variant::Source)
        .is_err());
    assert_eq!(frame(&mut r), before);
    // Nothing was stored for the refused cube: there is no second copy to flip to.
    assert!(!r.set_texture_variant(PATH, Variant::Encoded));
}

/// T-H3: the same texels as UNORM (Source) and UNORM_SRGB (Encoded): the sRGB view linearises, so Encoded is
/// darker; flipping back is byte-identical; an inactive copy changes nothing.
#[test]
fn ab_toggle_is_instant_and_the_srgb_header_shows() {
    let cache = tempfile::tempdir().unwrap();
    let grey = [[128, 128, 128, 255]];
    let mut r = with_texture(cache.path(), dds_rgba8(1, 1, &grey));
    let source = frame(&mut r);
    let touched = r
        .replace_texture(PATH, &dds_rgba8_dx10(1, 1, &grey, true), Variant::Encoded)
        .unwrap();
    assert!(touched.is_empty(), "stored, not shown: Source is active");
    assert_eq!(frame(&mut r), source, "an inactive copy changes nothing");
    assert!(r.set_texture_variant(PATH, Variant::Encoded));
    let encoded = frame(&mut r);
    assert!(
        channel(&encoded, 0) < channel(&source, 0),
        "sRGB-tagged grey is darker: {} vs {}",
        channel(&encoded, 0),
        channel(&source, 0)
    );
    assert!(!r.set_texture_variant(PATH, Variant::Encoded), "already active");
    assert!(r.set_texture_variant(PATH, Variant::Source));
    assert_eq!(frame(&mut r), source, "flipping back is byte-identical");
    // Dropping the inactive copy keeps what is shown; dropping the active one falls back to the other.
    r.replace_texture(PATH, &[], Variant::Encoded).unwrap();
    assert_eq!(frame(&mut r), source);
    assert!(!r.set_texture_variant(PATH, Variant::Encoded), "dropped");
}

/// T-H4: a TruePBR emissive map (slot 2 → t6) swapped in sets HasEmissive (the stand-in's green channel) and
/// dropping it clears the flag again.
#[test]
fn swapping_an_emissive_map_in_and_out_flips_has_emissive() {
    let cache = tempfile::tempdir().unwrap();
    let mut r = renderer(cache.path(), FLAT);
    r.load(&fixture("TestNifFile_Furniture_Col_SE.nif"), &|_| None);
    let mut m = material(&r);
    m.slsf2 &= !((1 << 12) | (1 << 24) | (1 << 25) | (1 << 26) | (1 << 27));
    m.slsf2 |= 1 << 23; // TruePBR
    m.params.emissive_color = [0.0; 3]; // D1 would add the NIF emissive colour under PBR
    m.set_slot_raw(2, "Textures\\Standin\\Emit.dds");
    assert_eq!(
        r.set_material(CYL1, m, &|_| None),
        Ok(EditKind::Permutation)
    );
    let absent = frame(&mut r);
    let white = dds_rgba8(1, 1, &[[255, 255, 255, 255]]);
    let touched = r.replace_texture(EMIT, &white, Variant::Encoded).unwrap();
    assert_eq!(touched, vec![(CYL1, 6)]);
    let present = frame(&mut r);
    assert!(
        channel(&present, 1) > channel(&absent, 1) + 1000,
        "green (HasEmissive): {} → {}",
        channel(&absent, 1),
        channel(&present, 1)
    );
    assert_eq!(channel(&present, 2), channel(&absent, 2), "only the flag moved");
    assert_eq!(
        r.replace_texture(EMIT, &[], Variant::Encoded).unwrap(),
        vec![(CYL1, 6)]
    );
    assert_eq!(frame(&mut r), absent, "the flag is clear again");
}

/// The resolver's bytes count as `set_load_variant`'s variant, so a later flip can reach the other copy.
#[test]
fn the_load_variant_labels_what_the_resolver_returned() {
    let cache = tempfile::tempdir().unwrap();
    let mut r = renderer(cache.path(), FLAT);
    r.set_load_variant(Variant::Encoded);
    r.load(&fixture("TestNifFile_Furniture_Col_SE.nif"), &|_| None);
    let mut m = material(&r);
    m.set_slot_raw(0, "Textures\\Standin\\Swap.dds");
    let red = dds_rgba8(1, 1, &[[255, 0, 0, 255]]);
    let resolve = |p: &str| (p == PATH).then(|| red.clone());
    r.set_material(CYL1, m, &resolve).unwrap();
    let shown = frame(&mut r);
    let green = dds_rgba8(1, 1, &[[0, 255, 0, 255]]);
    assert!(
        r.replace_texture(PATH, &green, Variant::Source)
            .unwrap()
            .is_empty(),
        "the resolver's red is the Encoded copy and stays shown"
    );
    assert_eq!(frame(&mut r), shown);
    assert!(r.set_texture_variant(PATH, Variant::Source));
    assert!(channel(&frame(&mut r), 1) > channel(&shown, 1) + 1000);
}
```

- [ ] **Step 4: Run to see RED**

Run: `cargo test -p render3d --test hotswap`
Expected: does not compile (`Variant`, `replace_texture`, `set_texture_variant`, `set_load_variant`, `draw_identity` not found in `render3d`).

- [ ] **Step 5: `textures.rs`**

Add `#[derive(Clone)]` above `pub struct Texture {` (its fields are a COM view, a `Copy` kind and a `Copy` format), and add after `load_dds`:

```rust
/// The kind of a bound view, from its dimension. A `ShapeDraw` keeps no reflection, and `bind_textures` only binds a
/// view whose kind equals the reflected one (defaults are made by reflected kind), so this is the kind the register
/// reads; the hot-swap checks a new texture against it.
pub fn view_kind(v: &ID3D11ShaderResourceView) -> Option<SrvKind> {
    let mut d = D3D11_SHADER_RESOURCE_VIEW_DESC::default();
    // SAFETY: `d` is a live local the call fills in; `v` is a live view.
    unsafe { v.GetDesc(&mut d) };
    match d.ViewDimension {
        x if x == D3D11_SRV_DIMENSION_TEXTURE2D => Some(SrvKind::Tex2D),
        x if x == D3D11_SRV_DIMENSION_TEXTURE2DARRAY => Some(SrvKind::Tex2DArray),
        x if x == D3D11_SRV_DIMENSION_TEXTURECUBE => Some(SrvKind::Cube),
        _ => None,
    }
}
```

- [ ] **Step 6: `lib.rs`: the entry type and the cache**

Replace the `TexSlot` block

```rust
/// A texture resolved once per load: its view and kind, or `None` (the resolver had no bytes, or they did
/// not decode).
type TexSlot = Option<(ID3D11ShaderResourceView, SrvKind)>;
```
with
```rust
/// A texture resolved once per load: its view and kind, or `None` (the resolver had no bytes, or they did
/// not decode).
type TexSlot = Option<(ID3D11ShaderResourceView, SrvKind)>;

/// Which of a path's two textures the draws use: the one the host's resolver returned at load (or a `Source`
/// swap), or the converter's encode of it. The set preview maps `PreviewMode` to this one to one (P1-B A/B, 03 §2).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Default)]
pub enum Variant {
    #[default]
    Source,
    Encoded,
}

impl Variant {
    pub fn other(self) -> Variant {
        match self {
            Variant::Source => Variant::Encoded,
            Variant::Encoded => Variant::Source,
        }
    }
}

/// Both textures of one canonical path (`textures/…`) and the one the draws use. GPU textures are immutable, so a
/// swap makes a new texture and SRV and rebinds. Both `None` = the resolver had nothing (kept so one load asks once).
#[derive(Clone, Default)]
struct TexEntry {
    source: Option<textures::Texture>,
    encoded: Option<textures::Texture>,
    active: Variant,
}

impl TexEntry {
    fn slot(&self, v: Variant) -> &Option<textures::Texture> {
        match v {
            Variant::Source => &self.source,
            Variant::Encoded => &self.encoded,
        }
    }

    fn slot_mut(&mut self, v: Variant) -> &mut Option<textures::Texture> {
        match v {
            Variant::Source => &mut self.source,
            Variant::Encoded => &mut self.encoded,
        }
    }

    fn current(&self) -> Option<&textures::Texture> {
        self.slot(self.active).as_ref()
    }

    fn any(&self) -> bool {
        self.source.is_some() || self.encoded.is_some()
    }
}
```

In `pub struct Renderer`, replace
```rust
    /// Textures resolved since the last load, by canonical path.
    tex_cache: HashMap<String, TexSlot>,
```
with
```rust
    /// Textures resolved or swapped in since the last load, by canonical path.
    tex_cache: HashMap<String, TexEntry>,
    /// The variant the resolver's bytes of the next loads count as ([`Renderer::set_load_variant`]).
    load_variant: Variant,
```
In `with_gpu`'s initialiser replace `tex_cache: HashMap::new(),` with
```rust
            tex_cache: HashMap::new(),
            load_variant: Variant::Source,
```
Replace the whole `fn texture` with:

```rust
    /// The active texture at canonical `norm`, resolved and decoded once per load (stored as the `load_variant`
    /// copy). A path the resolver does not return is listed in `rep.missing_textures`; undecodable bytes are an
    /// `rep.errors` line.
    fn texture(
        &mut self,
        norm: &str,
        textures: &dyn Fn(&str) -> Option<Vec<u8>>,
        rep: &mut LoadReport,
    ) -> TexSlot {
        if let Some(e) = self.tex_cache.get(norm) {
            return e.current().map(|t| (t.srv.clone(), t.kind));
        }
        let v = self.load_variant;
        let mut entry = TexEntry {
            active: v,
            ..TexEntry::default()
        };
        match textures(norm) {
            None => {
                if !rep.missing_textures.iter().any(|m| m == norm) {
                    rep.missing_textures.push(norm.to_string());
                }
            }
            Some(bytes) => match textures::load_dds(&self.gpu, &bytes) {
                Ok(t) => *entry.slot_mut(v) = Some(t),
                Err(e) => rep.errors.push(format!("{norm}: {e}")),
            },
        }
        let slot = entry.current().map(|t| (t.srv.clone(), t.kind));
        self.tex_cache.insert(norm.to_string(), entry);
        slot
    }
```
In `set_material` replace `self.tex_cache.retain(|_, t| t.is_some());` with `self.tex_cache.retain(|_, e| e.any());` (swapped-in textures stay; paths the resolver lacked are asked again).

Add, after `apply_in_place` and before `compute_bounds`:

```rust
    /// Which variant the resolver's bytes count as for the loads that follow (default `Source`). The set preview
    /// sets it to its current mode before a scene push, so the loaded textures are that mode's copy and the other
    /// mode's can be swapped in beside it.
    pub fn set_load_variant(&mut self, v: Variant) {
        self.load_variant = v;
    }

    /// Hot-swaps one texture (03 §2): a new immutable texture + SRV from `dds` is stored as variant `v` of `path`
    /// (empty `dds`: that copy is dropped). A variant that is the entry's only texture becomes the active one; a
    /// dropped active copy falls back to the other. When the bound texture changes, every `Drawing` shape that
    /// reads `path` gets the new view, and its material is written again where the set of real textures changed
    /// (`PBRFlags` HasEmissive/…, `EnvmapData.y`). No mesh, shader or cbuffer layout is touched.
    ///
    /// Returns the `(shape, register)` pairs rebound. `Err`, with nothing stored or changed, for a DDS that does
    /// not decode or a texture of another kind than a register reads (a cube into a 2D slot).
    pub fn replace_texture(
        &mut self,
        path: &str,
        dds: &[u8],
        v: Variant,
    ) -> Result<Vec<(ShapeId, u32)>, String> {
        let fresh = if dds.is_empty() {
            None
        } else {
            Some(textures::load_dds(&self.gpu, dds).map_err(|e| format!("{path}: {e}"))?)
        };
        let mut entry = self.tex_cache.get(path).cloned().unwrap_or_default();
        let was = entry.active;
        let dropped = fresh.is_none();
        *entry.slot_mut(v) = fresh;
        if !dropped && entry.slot(v.other()).is_none() {
            entry.active = v;
        } else if dropped && entry.active == v {
            entry.active = v.other();
        }
        if entry.active == v || entry.active != was {
            self.rebind(path, entry)
        } else {
            self.tex_cache.insert(path.to_string(), entry);
            Ok(vec![])
        }
    }

    /// Makes variant `v` of `path` the bound one (the A/B flip): a new view per bound register, no upload, no
    /// decode. False when `v` is already active, `path` has no `v` copy, no `Drawing` shape reads `path`, or the
    /// copy's kind does not fit (then nothing changes).
    pub fn set_texture_variant(&mut self, path: &str, v: Variant) -> bool {
        let Some(mut entry) = self.tex_cache.get(path).cloned() else {
            return false;
        };
        if entry.active == v || entry.slot(v).is_none() {
            return false;
        }
        entry.active = v;
        self.rebind(path, entry)
            .is_ok_and(|touched| !touched.is_empty())
    }

    /// `(shape, register, kind)` for every register a `Drawing` shape reads `path` through (the same slot →
    /// register rule as the load). Hidden, failed and pending shapes are left alone: their old draw keeps its old
    /// cbuffers, and their next build reads the entry.
    fn texture_targets(&self, path: &str) -> Vec<(ShapeId, u32, SrvKind)> {
        let mut out: Vec<(ShapeId, u32, SrvKind)> = vec![];
        for (id, s) in &self.shapes {
            let Some(d) = s.draw.as_ref().filter(|_| s.status == ShapeStatus::Drawing) else {
                continue;
            };
            let m = &s.material;
            let msn = m.slsf1 & (1 << 12) != 0;
            for (slot, p) in m.slots.iter().enumerate() {
                if p.norm != path {
                    continue;
                }
                let Some(reg) = textures::register_for_slot(m.shader_type, m.is_pbr(), msn, slot)
                else {
                    continue;
                };
                let Some(view) = d.srvs.iter().find(|(r, _)| *r == reg).map(|(_, v)| v) else {
                    continue;
                };
                let Some(kind) = textures::view_kind(view) else {
                    continue;
                };
                if !out.iter().any(|(i, r, _)| i == id && *r == reg) {
                    out.push((*id, reg, kind));
                }
            }
        }
        out
    }

    /// Stores `entry` for `path` and points every draw that reads it at the entry's active texture (a 1×1 default
    /// when it has none). All kinds are checked before anything changes.
    fn rebind(&mut self, path: &str, entry: TexEntry) -> Result<Vec<(ShapeId, u32)>, String> {
        let mut plan = vec![];
        for (id, reg, kind) in self.texture_targets(path) {
            let (shader_type, pbr, name) = {
                let s = &self.shapes[&id];
                (s.material.shader_type, s.material.is_pbr(), s.name.clone())
            };
            let real = match entry.current() {
                Some(t) if t.kind != kind => {
                    return Err(format!(
                        "{path} is a {:?} texture but {name} reads t{reg} as {kind:?}",
                        t.kind
                    ))
                }
                Some(t) => Some(t.srv.clone()),
                None => None,
            };
            let srv = match &real {
                Some(v) => v.clone(),
                None => self.default_srv(kind, textures::default_for(reg, shader_type, pbr))?,
            };
            plan.push((id, reg, srv, real.is_some()));
        }
        self.tex_cache.insert(path.to_string(), entry);
        let ll = self.linear_lighting();
        let mut touched = vec![];
        for (id, reg, srv, real) in plan {
            let Some(s) = self.shapes.get_mut(&id) else {
                continue;
            };
            let material = s.material.clone();
            let Some(d) = s.draw.as_mut() else {
                continue;
            };
            if let Some(slot) = d.srvs.iter_mut().find(|(r, _)| *r == reg) {
                slot.1 = srv;
            }
            let was = d.bound.regs.contains(&reg);
            if real {
                d.bound.regs.insert(reg);
            } else {
                d.bound.regs.remove(&reg);
            }
            if was != real {
                d.rewrite_material(&material, ll);
            }
            touched.push((id, reg));
        }
        Ok(touched)
    }

    /// Test hook: the COM pointer of the shape's index buffer. A hot-swap must leave it alone (no mesh reload).
    #[doc(hidden)]
    pub fn draw_identity(&self, id: ShapeId) -> Option<usize> {
        use windows::core::Interface;
        self.shapes
            .get(&id)?
            .draw
            .as_ref()
            .map(|d| d.mesh.index.as_raw() as usize)
    }
```
`Variant` is `pub` in `lib.rs`, so it is `render3d::Variant` with no re-export.

- [ ] **Step 7: GREEN**

Run: `cargo test -p render3d --test hotswap`
Expected: 5 pass. If `T-H4` fails with "green did not rise", check that the TRUE_PBR permutation of the stand-in reflects t6 (`EmissiveMap` must be used in the `STANDIN_FLAT` branch) and that `texture_targets` found register 6 (`replace_texture` returned `[(CYL1, 6)]`).
If `D3D11_SHADER_RESOURCE_VIEW_DESC::default()` is rejected, use `unsafe { std::mem::zeroed() }` with the same SAFETY note.

- [ ] **Step 8: Whole crate, then the gates**

Run: `cargo test -p render3d` (every existing test must still pass: `edit.rs` texture tests, `material.rs`, `frame.rs`).
Run: `cargo fmt --all && cargo clippy --workspace --all-targets -- -D warnings && cargo test --workspace`

- [ ] **Step 9: Commit**

```bash
git add crates/render3d
git commit -m "render3d: texture hot-swap (TexEntry, Variant, replace_texture, set_texture_variant)" -m "A swap makes a new texture and SRV and rebinds; kinds are checked before anything changes; HasEmissive and EnvmapData follow presence; A/B flips are SRV swaps. The stand-in gains an unread t6 emissive map so the flag is visible." -m "Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

---

## Task 8: gui: `SwapTexture` / `SetTextureVariant`, swaps on encode completion, instant A/B

> **Reconciliation R3 (binding):** Task 6 replaced the device-rebuild path with `Renderer::rebuild_device_on(gpu)` and added `V3Reply::Refresh`, `V3Cmd::Quit` and a narrowed `held_frame`. Apply this task's `rebuild`/`load_all(r, c, edits, tex)` change to the rebuild path Task 6 left, so a device rebuild replays the swap log after `rebuild_device_on`; do not re-widen `held_frame`.

**How it works (read first).**
- The render thread owns the textures. The set preview owns a **mirror** of what the renderer holds: `tex_sent: (path, PreviewMode) → Encoded.id` and `tex_active: path → PreviewMode`. Every scene push (`push_scene`) rebuilds the mirror from the bytes it served (`memory_sources`) and tells the renderer to count the resolver's bytes as the current mode's variant (`SetLoadVariant`).
- `sync_textures()` diffs the encoder against the mirror each polled frame and sends what is missing: `SwapTexture` for the shown mode's Fresh bytes, and once A/B is in use for the other mode's bytes (never as a lone copy: it would become the active one), `SwapTexture` with empty bytes to drop a stale inactive copy, then `SetTextureVariant` for the shown mode. If the viewport cannot take a **required** command (no load of this scene sent yet), the caller falls back to today's scene push (`want_scene`). Failed encodes still push the scene (the game's copy must come back). Slot-path changes are untouched (P1-A final-review I3: `Delta.reload`/`encode` → `want_scene` → full push).
- `Encoder` keeps, per member, an `alt`: the other mode's Fresh bytes at the current cap and signature. A/B is "in use" after the first effective `set_mode` (`ab`). The first flip moves the old Fresh bytes into `alt` and re-encodes as today; once the shown mode is Fresh and every member has settled, a missing `alt` encodes in the background. A flip with `alt` present swaps the two states: no encode, no scene push, `SetTextureVariant` only. A source/settings/cap change clears `alt` (the generation rules already used for the main encode: token zeroed + cancel; `alt_token` is checked the same way).
- The render loop logs the swaps and flips since the content's own load and applies them again after every reload (shaders/features/device change), like `edits`.

**Files:**
- Modify: `crates/gui/src/model/viewport3d.rs` (8a), `crates/gui/src/model/set_preview.rs` (8b, 8c)
- Modify (tests): `crates/gui/tests/viewport3d.rs`, `crates/gui/tests/set_preview_model.rs`, `crates/gui/tests/set_preview_ui.rs`

**Interfaces:**
```rust
// viewport3d.rs
V3Cmd::SwapTexture { path: String, bytes: Arc<Vec<u8>>, variant: render3d::Variant }
V3Cmd::SetTextureVariant { path: String, variant: render3d::Variant }
V3Cmd::SetLoadVariant(render3d::Variant)                                   // extra: see CONTRACT ISSUES 1
V3Reply::TextureSwap { content: u64, path: String, result: Result<(), String> }
impl Viewport3d {
    pub fn swap_texture(&mut self, path: String, bytes: Arc<Vec<u8>>, variant: Variant) -> bool;
    pub fn set_texture_variant(&mut self, path: String, variant: Variant) -> bool;
    pub fn set_load_variant(&mut self, v: Variant);
    pub swap_errors: Vec<String>;            // drained into the set preview's notices
}
// set_preview.rs
impl PreviewMode { pub fn other(self) -> PreviewMode }
pub fn variant_of(m: PreviewMode) -> render3d::Variant;
impl Encoder { pub fn ab_active(&self) -> bool; pub scene_dirty: bool;
               #[doc(hidden)] pub fn has_alt(&self, kind: SlotKind) -> bool;
               #[doc(hidden)] pub fn alt_token_of(&self, kind: SlotKind) -> u64; }
```

### 8a: viewport commands, render-loop handling, replay

- [ ] **Step 1: Failing tests** — append to `crates/gui/tests/viewport3d.rs` (it already has `confirmed()`, `parsed()`, `mem()`, `loaded()` helpers and `use std::collections::HashMap; use std::sync::Arc;` above them):

```rust
// ---- P1-B Task 8a: texture swaps ----

#[test]
fn texture_swaps_follow_the_load_re_render_and_report_errors() {
    use render3d::Variant;
    let (_d, mut v, cmds) = confirmed();
    let nif = Arc::new(render3d::ParsedNif::default());
    v.set_content(Some(parsed(1, &nif, &[])), false);
    let p = "textures/a.dds";
    let bytes = Arc::new(vec![1u8, 2, 3]);
    assert!(
        !v.swap_texture(p.into(), bytes.clone(), Variant::Encoded),
        "no load sent yet: the caller pushes the scene instead"
    );
    assert!(!v.set_texture_variant(p.into(), Variant::Encoded));
    v.request_frame(64, 64);
    let Ok(v3::V3Cmd::LoadParsed { id, .. }) = cmds.try_recv() else {
        panic!("LoadParsed first (the default load variant is never announced)")
    };
    assert!(v.swap_texture(p.into(), bytes.clone(), Variant::Encoded));
    assert!(v.set_texture_variant(p.into(), Variant::Encoded));
    assert!(matches!(
        cmds.try_recv(),
        Ok(v3::V3Cmd::SwapTexture { path, variant: Variant::Encoded, bytes: b }) if path == p && b == bytes
    ));
    assert!(matches!(
        cmds.try_recv(),
        Ok(v3::V3Cmd::SetTextureVariant { path, variant: Variant::Encoded }) if path == p
    ));
    loaded(
        &mut v,
        id,
        render3d::LoadReport {
            shapes: 1,
            ..Default::default()
        },
    );
    v.request_frame(64, 64);
    assert!(matches!(cmds.try_recv(), Ok(v3::V3Cmd::Render { .. })));
    v.request_frame(64, 64);
    assert!(cmds.try_recv().is_err(), "nothing changed: nothing sent");
    assert!(v.swap_texture(p.into(), bytes, Variant::Source));
    cmds.try_recv().unwrap();
    v.request_frame(64, 64);
    assert!(
        matches!(cmds.try_recv(), Ok(v3::V3Cmd::Render { .. })),
        "a swap draws on the next request"
    );
    // A failed swap is listed; a reply for content replaced since is dropped.
    v.handle(v3::V3Reply::TextureSwap {
        content: id,
        path: p.into(),
        result: Err("boom".into()),
    });
    assert_eq!(v.swap_errors, vec!["textures/a.dds: boom".to_string()]);
    v.handle(v3::V3Reply::TextureSwap {
        content: id + 99,
        path: p.into(),
        result: Err("stale".into()),
    });
    assert_eq!(v.swap_errors.len(), 1);
}

#[test]
fn only_parsed_and_builtin_content_takes_swaps_and_the_load_variant_is_announced_once() {
    use render3d::Variant;
    let (_d, mut v, cmds) = confirmed();
    v.set_mesh(Some(spec(1, false)));
    v.request_frame(64, 64);
    cmds.try_recv().unwrap(); // Load
    assert!(!v.swap_texture("textures/a.dds".into(), Arc::new(vec![1]), Variant::Source));
    let nif = Arc::new(render3d::ParsedNif::default());
    v.set_load_variant(Variant::Encoded);
    v.set_content(Some(parsed(2, &nif, &[])), false);
    v.request_frame(64, 64);
    assert!(matches!(
        cmds.try_recv(),
        Ok(v3::V3Cmd::SetLoadVariant(Variant::Encoded))
    ));
    assert!(matches!(cmds.try_recv(), Ok(v3::V3Cmd::LoadParsed { .. })));
    // Unchanged for the next load: not announced again.
    v.set_content(Some(parsed(3, &nif, &[])), false);
    v.request_frame(64, 64);
    assert!(matches!(cmds.try_recv(), Ok(v3::V3Cmd::LoadParsed { .. })));
}

/// A real render thread on WARP with the repo stand-in: a swap shows in the next frame and is applied again after
/// a reload (here: the features toggle), which loads the content's own bytes again.
#[test]
fn a_texture_swap_reaches_the_render_thread_and_survives_a_reload() {
    use nif::material::MaterialState;
    use render3d::Variant;
    use sdc_gui::model::set_preview::uncompressed_dds;
    const PATH: &str = "textures/ab/t.dds";
    let standin = PathBuf::from(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../render3d/tests/fixtures/standin"
    ));
    let fp = render3d::locate::fingerprint(&standin, None).unwrap();
    let cache = tempfile::tempdir().unwrap();
    let mut v = Viewport3d::new(Some(fp.clone()));
    v.cache_dir = Some(cache.path().to_path_buf());
    v.on_located(Ok(Some(fp)));
    let tex = |c: [f32; 4]| {
        uncompressed_dds(
            &[engine::image::Rgba32F {
                width: 1,
                height: 1,
                px: vec![c],
            }],
            false,
            false,
        )
        .unwrap()
    };
    let mut material = MaterialState::builtin_default();
    material.set_slot_raw(0, PATH);
    // DO_ALPHA_TEST: the stand-in refuses the other permutations.
    material.alpha = Some(nif::props::Alpha {
        flags: 0x200,
        threshold: 0,
    });
    let red = v3::FileSource::Memory {
        id: 1,
        bytes: Arc::new(tex([1.0, 0.0, 0.0, 1.0])),
    };
    v.set_content(
        Some(v3::Content::Builtin {
            key: 1,
            shape: render3d::builtin::Builtin::Sphere,
            material,
            textures: Arc::new(HashMap::from([(PATH.to_string(), red)])),
        }),
        false,
    );
    let settle = |v: &mut Viewport3d| {
        let t = std::time::Instant::now();
        loop {
            v.request_frame(64, 64);
            v.poll();
            if !v.busy() {
                break;
            }
            assert!(t.elapsed().as_secs() < 60, "the viewport did not settle");
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
    };
    let chan = |v: &Viewport3d, c: usize| -> u64 {
        let f = v.frame.as_ref().expect("a frame");
        f.rgba.chunks(4).map(|p| p[c] as u64).sum()
    };
    settle(&mut v);
    assert!(v.render_error.is_none(), "{:?}", v.render_error);
    assert!(
        chan(&v, 0) > chan(&v, 1) + 1000,
        "red texture: {} vs {}",
        chan(&v, 0),
        chan(&v, 1)
    );
    let green = Arc::new(tex([0.0, 1.0, 0.0, 1.0]));
    assert!(v.swap_texture(PATH.into(), green, Variant::Source));
    settle(&mut v);
    assert!(
        chan(&v, 1) > chan(&v, 0) + 1000,
        "green after the swap: {} vs {}",
        chan(&v, 1),
        chan(&v, 0)
    );
    assert!(v.swap_errors.is_empty(), "{:?}", v.swap_errors);
    v.set_features(false);
    settle(&mut v);
    assert!(
        chan(&v, 1) > chan(&v, 0) + 1000,
        "the swap is applied again after the reload"
    );
}
```

Run: `cargo test -p sdc-gui --test viewport3d texture` → does not compile (`swap_texture`, `V3Cmd::SwapTexture`, … missing).

- [ ] **Step 2: `viewport3d.rs`: imports, commands, reply**

Add `Variant` to the `use render3d::{...}` list (`EditKind, FrameTiming, LoadReport, ParsedNif, Renderer, ShapeId, ShapeInfo, ShapeStatus, Variant`). In `enum V3Cmd`, after the `SetMaterial { .. }` variant add:

```rust
    /// New DDS bytes for a texture path as one variant (`Renderer::replace_texture`); empty bytes drop that copy.
    /// Applied after the load it follows; a later load in the same batch drops it.
    SwapTexture {
        path: String,
        bytes: Arc<Vec<u8>>,
        variant: Variant,
    },
    /// Makes one variant of a path the bound one (`Renderer::set_texture_variant`): the instant A/B flip.
    SetTextureVariant { path: String, variant: Variant },
    /// The variant the next loads' resolver bytes count as (`Renderer::set_load_variant`); sticky.
    SetLoadVariant(Variant),
```
In `enum V3Reply`, after `MaterialSet { .. }` add:

```rust
    /// One `SwapTexture` applied (or why it was not: the old texture stays).
    TextureSwap {
        content: u64,
        path: String,
        result: Result<(), String>,
    },
```

- [ ] **Step 3: `viewport3d.rs`: the model side**

In `pub struct Viewport3d`, after `pub last_edit: Option<(EditKind, f64)>,` add:

```rust
    /// Texture swaps the render thread refused (a bad DDS, a kind the slot does not read); the set preview lists them.
    pub swap_errors: Vec<String>,
    /// The variant the next load's resolver bytes count as, and the one the render thread was last told.
    load_variant: Variant,
    load_variant_sent: Variant,
```
In `Viewport3d::new`, after `last_edit: None,` add `swap_errors: vec![], load_variant: Variant::Source, load_variant_sent: Variant::Source,`.

In `ensure_thread`, right before `self.thread = Some(ctx);` add `self.load_variant_sent = Variant::Source; // a new thread starts at the default`.

In `request_frame`, in the `if self.need_load {` branch, immediately before `if thread.send(cmd).is_ok() {` add:

```rust
            if self.load_variant != self.load_variant_sent
                && thread
                    .send(V3Cmd::SetLoadVariant(self.load_variant))
                    .is_ok()
            {
                self.load_variant_sent = self.load_variant;
            }
```
After `pub fn set_material(..)` add:

```rust
    /// The render thread, once a load of the current `Parsed`/`Builtin` content has been sent: the guard of every
    /// edit that follows a load (`set_material`, texture swaps).
    fn content_thread(&self) -> Option<Sender<V3Cmd>> {
        if !matches!(
            self.content,
            Some(Content::Parsed { .. } | Content::Builtin { .. })
        ) {
            return None;
        }
        if self.need_load || (self.loaded.is_none() && self.pending_load.is_none()) {
            return None;
        }
        self.thread.clone()
    }

    /// New DDS bytes for `path` as `variant` on the current content (empty bytes drop that copy), applied by the
    /// thread after the load it follows. False when no load of this content has been sent yet: the caller then puts
    /// the bytes into a new content instead.
    pub fn swap_texture(&mut self, path: String, bytes: Arc<Vec<u8>>, variant: Variant) -> bool {
        self.send_texture_cmd(V3Cmd::SwapTexture {
            path,
            bytes,
            variant,
        })
    }

    /// Shows `variant` of `path` (both copies are already on the thread). False as [`Viewport3d::swap_texture`].
    pub fn set_texture_variant(&mut self, path: String, variant: Variant) -> bool {
        self.send_texture_cmd(V3Cmd::SetTextureVariant { path, variant })
    }

    fn send_texture_cmd(&mut self, cmd: V3Cmd) -> bool {
        let Some(t) = self.content_thread() else {
            return false;
        };
        if t.send(cmd).is_err() {
            return false;
        }
        // The next request_frame draws it.
        self.last_sent = None;
        true
    }

    /// The variant the next load's resolver bytes count as (the set preview's current mode). Announced to the
    /// thread with the next load, and only when it differs from what the thread has.
    pub fn set_load_variant(&mut self, v: Variant) {
        self.load_variant = v;
    }
```
In `handle`, after the `V3Reply::MaterialSet { .. } => { .. }` arm add:

```rust
            V3Reply::TextureSwap {
                content,
                path,
                result,
            } => {
                if self.loaded.or(self.pending_load) != Some(content) {
                    return;
                }
                if let Err(e) = result {
                    self.swap_errors.push(format!("{path}: {e}"));
                }
            }
```

- [ ] **Step 4: `viewport3d.rs`: the render loop**

Add above `fn render_loop`:

```rust
/// The render thread's texture state (P1-B A/B): the variant the load resolver's bytes count as, and the swaps and
/// flips made since the content's own load. A reload (shaders, features or device change) loads the content's own
/// bytes again, so they are applied again, exactly as `edits` are.
#[derive(Default)]
struct TexState {
    load: Variant,
    swaps: BTreeMap<(String, Variant), Arc<Vec<u8>>>,
    active: BTreeMap<String, Variant>,
}

impl TexState {
    fn clear_log(&mut self) {
        self.swaps.clear();
        self.active.clear();
    }

    /// Swaps first, then flips (the order the set preview sends them). A failure here would have failed the
    /// original swap too, which was reported then.
    fn replay(&self, r: &mut Renderer) {
        for ((path, v), bytes) in &self.swaps {
            let _ = r.replace_texture(path, bytes, *v);
        }
        for (path, v) in &self.active {
            r.set_texture_variant(path, *v);
        }
    }
}

enum TexCmd {
    Swap {
        path: String,
        bytes: Arc<Vec<u8>>,
        variant: Variant,
    },
    Flip {
        path: String,
        variant: Variant,
    },
}

/// [`load`] with the texture state: the load variant first, the logged swaps and flips after.
fn load_all(
    r: &mut Renderer,
    c: &Content,
    edits: &BTreeMap<ShapeId, MaterialState>,
    tex: &TexState,
) -> LoadReport {
    r.set_load_variant(tex.load);
    let rep = load(r, c, edits);
    tex.replay(r);
    rep
}
```
In `render_loop`:
- the `rebuild` closure: add a parameter `tex: &TexState` after `edits` and call `load_all(r, c, edits, tex)` instead of `load(r, c, edits)`; update its two callers (`rebuild(&mut r, &last, &edits, features)` → `rebuild(&mut r, &last, &edits, &tex, features)`), keeping the parameter order of your closure consistent.
- after `let mut edits: BTreeMap<ShapeId, MaterialState> = BTreeMap::new();` add `let mut tex = TexState::default();`.
- after `let mut pending: Vec<(u64, ShapeId, MaterialState)> = vec![];` add `let mut tex_cmds: Vec<TexCmd> = vec![];`.
- in the command `match c {`, after the `V3Cmd::SetMaterial { .. } => { .. }` arm add:

```rust
                V3Cmd::SetLoadVariant(v) => {
                    tex.load = v;
                    None
                }
                V3Cmd::SwapTexture {
                    path,
                    bytes,
                    variant,
                } => {
                    tex_cmds.push(TexCmd::Swap {
                        path,
                        bytes,
                        variant,
                    });
                    None
                }
                V3Cmd::SetTextureVariant { path, variant } => {
                    tex_cmds.push(TexCmd::Flip { path, variant });
                    None
                }
```
- in `if let Some(c) = content {`, next to `edits.clear(); pending.clear();` add `tex_cmds.clear(); tex.clear_log();`.
- in the `if reload {` block replace `let report = load(&mut r, c, &edits);` with `let report = load_all(&mut r, c, &edits, &tex);`.
- after the `if !pending.is_empty() { .. }` block, before `let Some((id, camera, w, h, look)) = render else {`, add:

```rust
        if !tex_cmds.is_empty() {
            if let Some((lid, _)) = &last {
                for t in tex_cmds {
                    match t {
                        TexCmd::Swap {
                            path,
                            bytes,
                            variant,
                        } => {
                            let result = r.replace_texture(&path, &bytes, variant).map(|_| ());
                            if result.is_ok() {
                                tex.swaps.insert((path.clone(), variant), bytes);
                            }
                            send(V3Reply::TextureSwap {
                                content: *lid,
                                path,
                                result,
                            });
                        }
                        TexCmd::Flip { path, variant } => {
                            r.set_texture_variant(&path, variant);
                            tex.active.insert(path, variant);
                        }
                    }
                }
            }
        }
```

- [ ] **Step 5: GREEN for 8a**

Run: `cargo test -p sdc-gui --test viewport3d`
Expected: PASS, including the three new tests and every existing one (the strict `cmds.try_recv()` tests still see `LoadParsed` first because the default load variant is never announced).
Run the gates: `cargo fmt --all && cargo clippy --workspace --all-targets -- -D warnings && cargo test --workspace`

- [ ] **Step 6: Commit**

```bash
git add crates/gui/src/model/viewport3d.rs crates/gui/tests/viewport3d.rs
git commit -m "gui: V3Cmd::SwapTexture/SetTextureVariant/SetLoadVariant, applied by the render thread and after reloads" -m "Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

### 8b: the encoder keeps both variants (A/B)

- [ ] **Step 1: Failing tests** — append to `crates/gui/tests/set_preview_model.rs` (after the last test; `started()`, `fake()`, `rock()`, `settle()` exist above). Encoder-level:

```rust
// ---- P1-B Task 8b: A/B variants in the encoder ----

fn started_ab() -> (tempfile::TempDir, AppModel, Encoder) {
    let (d, m, mut enc, slots) = started();
    let t = Instant::now();
    enc.sync(&m, slots, t, true);
    enc.pump(m.session.as_ref(), t, false);
    for k in enc.members.keys().copied().collect::<Vec<_>>() {
        let tok = enc.token_of(k);
        assert_ne!(tok, 0);
        enc.inject(k, tok, DEFAULT_CAP, fake());
    }
    assert!(enc.poll());
    assert!(enc.settled());
    (d, m, enc)
}

fn kinds_of(e: &Encoder) -> Vec<SlotKind> {
    e.members.keys().copied().collect()
}

#[test]
fn the_first_flip_keeps_the_old_bytes_for_the_way_back_and_the_second_flip_is_free() {
    let (_d, m, mut enc) = started_ab();
    assert!(!enc.ab_active());
    let old: Vec<u64> = enc.members.values().map(|x| x.state.shown().unwrap().id).collect();
    let t = Instant::now();
    enc.set_mode(PreviewMode::Source, t);
    assert!(enc.ab_active());
    for k in kinds_of(&enc) {
        assert!(enc.has_alt(k), "the old Fresh bytes are the other copy");
        assert!(matches!(enc.members[&k].state, EncState::Stale(Some(_))), "as before the change");
    }
    // The re-encode of the new mode as today.
    enc.pump(m.session.as_ref(), t, false);
    for k in kinds_of(&enc) {
        let tok = enc.token_of(k);
        assert_ne!(tok, 0);
        enc.inject(k, tok, DEFAULT_CAP, fake());
    }
    assert!(enc.poll());
    for k in kinds_of(&enc) {
        assert!(matches!(enc.members[&k].state, EncState::Fresh(_)));
        assert_eq!(enc.alt_token_of(k), 0, "nothing to encode: the other copy exists");
    }
    // Back: no encode, the old bytes are on show again, and the Source bytes are the other copy.
    let shown_source: Vec<u64> = enc.members.values().map(|x| x.state.shown().unwrap().id).collect();
    enc.set_mode(PreviewMode::Encoded, t);
    enc.pump(m.session.as_ref(), t, false);
    let back: Vec<u64> = enc.members.values().map(|x| x.state.shown().unwrap().id).collect();
    assert_eq!(back, old);
    for k in kinds_of(&enc) {
        assert_eq!(enc.token_of(k), 0, "a flip with both variants starts no encode");
        assert_eq!(enc.alt_token_of(k), 0);
        assert!(enc.has_alt(k));
        assert!(matches!(enc.members[&k].state, EncState::Fresh(_)));
    }
    enc.set_mode(PreviewMode::Source, t);
    let again: Vec<u64> = enc.members.values().map(|x| x.state.shown().unwrap().id).collect();
    assert_eq!(again, shown_source);
}

#[test]
fn a_cap_change_drops_both_variants_and_the_other_one_encodes_after_the_shown_one_is_fresh() {
    let (_d, m, mut enc) = started_ab();
    let t = Instant::now();
    enc.set_mode(PreviewMode::Source, t); // A/B in use; Encoded kept as the other copy
    enc.set_cap(1024, t);
    for k in kinds_of(&enc) {
        assert!(!enc.has_alt(k), "the other copy was made at the old cap");
        assert_eq!(enc.alt_token_of(k), 0);
    }
    enc.pump(m.session.as_ref(), t, false);
    for k in kinds_of(&enc) {
        let tok = enc.token_of(k);
        assert_ne!(tok, 0);
        assert_eq!(enc.alt_token_of(k), 0, "the shown mode first");
        enc.inject(k, tok, 1024, fake());
    }
    assert!(enc.poll());
    enc.pump(m.session.as_ref(), t, false);
    for k in kinds_of(&enc) {
        assert_ne!(enc.alt_token_of(k), 0, "now the inactive mode encodes");
    }
    // Its result becomes the other copy; the shown bytes do not move and nothing is reported as changed.
    let shown: Vec<u64> = enc.members.values().map(|x| x.state.shown().unwrap().id).collect();
    for k in kinds_of(&enc) {
        let at = enc.alt_token_of(k);
        enc.inject_as(k, at, 1024, PreviewMode::Encoded, fake());
    }
    assert!(!enc.poll(), "an inactive copy is not a change of what is on show");
    for k in kinds_of(&enc) {
        assert!(enc.has_alt(k));
        assert_eq!(enc.alt_token_of(k), 0);
    }
    assert_eq!(
        enc.members.values().map(|x| x.state.shown().unwrap().id).collect::<Vec<_>>(),
        shown
    );
    // A late result of the superseded other-mode encode never lands.
    let k = kinds_of(&enc)[0];
    enc.inject_as(k, 1, 1024, PreviewMode::Encoded, fake());
    assert!(!enc.poll());
}

#[test]
fn a_source_change_clears_the_other_variant_and_nothing_encodes_before_the_first_flip() {
    let (_d, m, mut enc, mut slots) = started();
    let t = Instant::now();
    enc.sync(&m, slots.clone(), t, true);
    enc.pump(m.session.as_ref(), t, false);
    for k in kinds_of(&enc) {
        let tok = enc.token_of(k);
        enc.inject(k, tok, DEFAULT_CAP, fake());
    }
    assert!(enc.poll());
    enc.pump(m.session.as_ref(), t, false);
    for k in kinds_of(&enc) {
        assert_eq!(enc.alt_token_of(k), 0, "A/B is not in use: no doubled CPU");
    }
    enc.set_mode(PreviewMode::Source, t);
    assert!(kinds_of(&enc).iter().all(|k| enc.has_alt(*k)));
    edit(&mut slots);
    enc.sync(&m, slots, t, true);
    let diffuse = SlotKind::Diffuse;
    assert!(!enc.has_alt(diffuse), "the edited member's other copy is gone");
    assert_eq!(enc.alt_token_of(diffuse), 0);
}

#[test]
fn a_flip_before_the_other_variant_exists_re_encodes_as_before() {
    let (_d, m, mut enc, slots) = started();
    let t = Instant::now();
    enc.sync(&m, slots, t, true);
    enc.pump(m.session.as_ref(), t, false);
    let t1 = enc.token_of(SlotKind::Diffuse); // still encoding the first mode
    enc.set_mode(PreviewMode::Source, t);
    assert!(!enc.has_alt(SlotKind::Diffuse), "nothing Fresh to keep");
    assert_eq!(enc.token_of(SlotKind::Diffuse), 0, "the old encode is cancelled");
    enc.pump(m.session.as_ref(), t, false);
    let t2 = enc.token_of(SlotKind::Diffuse);
    assert_ne!(t2, 0);
    assert_ne!(t2, t1);
}
```
Run: `cargo test -p sdc-gui --test set_preview_model -- ab_ flip variants` → does not compile (`ab_active`, `has_alt`, `alt_token_of`).

- [ ] **Step 2: `set_preview.rs`: types and helpers**

In `use render3d::{ParsedNif, ShapeId};` add `Variant`. After `pub const MODE_LABEL_SOURCE ...` add:

```rust
impl PreviewMode {
    pub fn other(self) -> PreviewMode {
        match self {
            PreviewMode::Encoded => PreviewMode::Source,
            PreviewMode::Source => PreviewMode::Encoded,
        }
    }
}

/// The renderer's variant for a mode (P1-B contract: `Encoded` ↔ `Variant::Encoded`, `Source` ↔ `Variant::Source`).
pub fn variant_of(m: PreviewMode) -> Variant {
    match m {
        PreviewMode::Encoded => Variant::Encoded,
        PreviewMode::Source => Variant::Source,
    }
}
```
In `pub struct Member` add after `cancel: Option<CancelToken>,`:

```rust
    /// A/B: the other mode's Fresh bytes at the current cap and signature (a DDS input: the same bytes).
    alt: Option<Arc<Encoded>>,
    /// The other mode's background encode: its generation (0 = none) and cancel, and whether it failed (not retried
    /// until the member changes).
    alt_token: u64,
    alt_cancel: Option<CancelToken>,
    alt_err: bool,
```
Add after `struct EncDone { .. }`:

```rust
/// What one encode run is for.
#[derive(Clone, Copy)]
struct Run {
    kind: SlotKind,
    token: u64,
    cap: u32,
    mode: PreviewMode,
}

fn spawn_encode(
    tx: Sender<EncDone>,
    session: Arc<Session>,
    source: Source,
    run: Run,
    cancel: CancelToken,
) {
    spawn(
        tx,
        move || {
            let t0 = Instant::now();
            let result = encode(&session, &source, run.cap, run.mode, &cancel).map(|b| (b, ms(t0)));
            EncDone {
                kind: run.kind,
                token: run.token,
                cap: run.cap,
                mode: run.mode,
                result,
            }
        },
        move |e| EncDone {
            kind: run.kind,
            token: run.token,
            cap: run.cap,
            mode: run.mode,
            result: Err(format!("internal error in a preview encode: {e}")),
        },
    );
}

impl Member {
    fn cancel_all(&self) {
        for c in [&self.cancel, &self.alt_cancel].into_iter().flatten() {
            c.cancel();
        }
    }

    /// The other mode's copy and encode are void (source, settings or cap changed).
    fn clear_alt(&mut self) {
        if let Some(c) = self.alt_cancel.take() {
            c.cancel();
        }
        self.alt = None;
        self.alt_token = 0;
        self.alt_err = false;
    }

    /// The mode flips. The other copy is Fresh: swap the two states, no work. Otherwise as before the A/B (the
    /// member goes stale, old bytes on show, encodes again), except that the old Fresh bytes are kept as the other
    /// copy and an other-mode encode already running becomes the live one.
    fn flip(&mut self, mode: PreviewMode, cap: u32, now: Instant) {
        if let Some(alt) = self.alt.take() {
            if let Some(c) = self.cancel.take() {
                c.cancel();
            }
            self.token = 0;
            self.due = None;
            let old = std::mem::replace(&mut self.state, EncState::Fresh(alt));
            self.alt = match old {
                EncState::Fresh(e) => Some(e),
                _ => None,
            };
            self.mode = mode;
            self.cap = cap;
            return;
        }
        let kept = match &self.state {
            EncState::Fresh(e) => Some(e.clone()),
            _ => None,
        };
        if let Some(c) = self.cancel.take() {
            c.cancel();
        }
        self.token = 0;
        if matches!(self.state, EncState::Blocked(_)) {
            return;
        }
        let shown = self.state.shown().cloned();
        self.cap = cap;
        self.mode = mode;
        if self.alt_token != 0 {
            self.token = std::mem::take(&mut self.alt_token);
            self.cancel = self.alt_cancel.take();
            self.due = None;
            self.state = EncState::Encoding(shown);
        } else {
            self.state = EncState::Stale(shown);
            self.due = Some(now);
        }
        self.alt = kept;
        self.alt_err = false;
    }
}
```
In `pub struct Encoder` add after `hold: bool,`:

```rust
    /// A/B is in use: the user has flipped the mode once this session (kept across sets). Until then only the
    /// shown mode encodes.
    ab: bool,
    /// A member failed since the last drain: the scene must be pushed again so the game's copy of its texture
    /// comes back (a Fresh result needs no push, it is swapped in).
    pub scene_dirty: bool,
```
In `Encoder::new` add `ab: false, scene_dirty: false,` after `hold: false,`.

- [ ] **Step 3: `set_preview.rs`: Encoder methods**

Replace `Encoder::set_mode` with:

```rust
    /// A new mode. The first change puts A/B in use. A member whose other copy is Fresh just swaps (no encode, no
    /// scene push); the rest re-encode as before and keep their old bytes as the other copy. A DDS input's bytes do
    /// not depend on the mode, so it is left alone.
    pub fn set_mode(&mut self, mode: PreviewMode, now: Instant) {
        if mode == self.mode {
            return;
        }
        self.mode = mode;
        self.ab = true;
        for mem in self.members.values_mut() {
            if matches!(mem.source, Source::Dds(_)) {
                mem.mode = mode;
                continue;
            }
            mem.flip(mode, self.cap, now);
        }
    }

    /// A/B is in use (the mode has been flipped this session).
    pub fn ab_active(&self) -> bool {
        self.ab
    }

    /// Test seam: `kind` has the other mode's Fresh bytes.
    #[doc(hidden)]
    pub fn has_alt(&self, kind: SlotKind) -> bool {
        self.members.get(&kind).is_some_and(|m| m.alt.is_some())
    }

    /// Test seam: the generation of `kind`'s other-mode background encode (0 = none).
    #[doc(hidden)]
    pub fn alt_token_of(&self, kind: SlotKind) -> u64 {
        self.members.get(&kind).map_or(0, |m| m.alt_token)
    }
```
In `fn restart`, directly after the `if let Some(c) = mem.cancel.take() { c.cancel(); }` line add `mem.clear_alt();`. (`set_cap` is now its only caller; keep the `skip_dds` parameter as is.)

In `reset` and `Drop for Encoder` replace the loops' body `if let Some(c) = &m.cancel { c.cancel(); }` by `m.cancel_all();`. In `sync`: in the `self.members.retain(..)` closure replace `if let Some(c) = &mem.cancel { c.cancel(); }` by `mem.cancel_all();`; in the changed-member arm (`Some(mem) => {` with `mem.token = 0;`) add `mem.clear_alt();` after `mem.token = 0;`; in the `None =>` arm's `Member { .. }` literal add `alt: None, alt_token: 0, alt_cancel: None, alt_err: false,`.

Replace `pump` with:

```rust
    /// Starts every due encode, one thread each (a set has at most 9 members). Nothing starts while an export
    /// runs or without a session. With A/B in use and every member settled, the missing other-mode copies encode
    /// next (a DDS input has none to make).
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
            mem.cap = self.cap;
            mem.mode = self.mode;
            let cancel = CancelToken::default();
            mem.cancel = Some(cancel.clone());
            mem.state = EncState::Encoding(mem.state.shown().cloned());
            if self.hold {
                continue;
            }
            let run = Run {
                kind: *kind,
                token,
                cap: self.cap,
                mode: self.mode,
            };
            spawn_encode(self.tx.clone(), session.clone(), mem.source.clone(), run, cancel);
        }
        if !self.ab || !self.members.values().all(|m| m.state.settled()) {
            return;
        }
        let other = self.mode.other();
        for (kind, mem) in self.members.iter_mut() {
            if mem.alt.is_some()
                || mem.alt_token != 0
                || mem.alt_err
                || !matches!(mem.state, EncState::Fresh(_))
                || matches!(mem.source, Source::Dds(_))
            {
                continue;
            }
            self.next_token += 1;
            let token = self.next_token;
            mem.alt_token = token;
            let cancel = CancelToken::default();
            mem.alt_cancel = Some(cancel.clone());
            if self.hold {
                continue;
            }
            let run = Run {
                kind: *kind,
                token,
                cap: self.cap,
                mode: other,
            };
            spawn_encode(self.tx.clone(), session.clone(), mem.source.clone(), run, cancel);
        }
    }
```
Replace `poll` with:

```rust
    /// Drains finished encodes. True when a member's bytes changed or it failed. An other-mode copy arriving is not
    /// a change of what is on show (the set preview sends it to the renderer on its own); a failure sets
    /// `scene_dirty`.
    pub fn poll(&mut self) -> bool {
        let mut changed = false;
        while let Ok(EncDone {
            kind,
            token,
            cap,
            mode,
            result,
        }) = self.rx.try_recv()
        {
            let Some(mem) = self.members.get_mut(&kind) else {
                continue;
            };
            // The other mode's background encode.
            if token != 0 && mem.alt_token == token {
                mem.alt_cancel = None;
                mem.alt_token = 0;
                match result {
                    Ok((dds, ms)) if cap == self.cap && mode != self.mode => {
                        mem.alt = Some(Arc::new(Encoded {
                            id: NEXT_ID.fetch_add(1, Ordering::Relaxed),
                            dds: Arc::new(dds),
                            ms,
                        }));
                    }
                    Ok(_) => {} // made at another cap or mode than now: the next pump encodes it again
                    Err(_) => mem.alt_err = true,
                }
                continue;
            }
            // Superseded: a newer edit, a block, a cap change (all zero or replace the token).
            if mem.token == 0 || mem.token != token {
                continue;
            }
            // The live encode ran at another cap or mode than the current one: useless, schedule it again.
            if cap != self.cap || mode != self.mode {
                if let Some(c) = mem.cancel.take() {
                    c.cancel();
                }
                mem.token = 0;
                mem.cap = self.cap;
                mem.mode = self.mode;
                mem.state = EncState::Stale(mem.state.shown().cloned());
                mem.due = Some(Instant::now());
                changed = true;
                continue;
            }
            mem.cancel = None;
            mem.token = 0;
            mem.state = match result {
                Ok((dds, ms)) => {
                    let e = Arc::new(Encoded {
                        id: NEXT_ID.fetch_add(1, Ordering::Relaxed),
                        dds: Arc::new(dds),
                        ms,
                    });
                    // A DDS input is the same file in both modes.
                    mem.alt = matches!(mem.source, Source::Dds(_)).then(|| e.clone());
                    EncState::Fresh(e)
                }
                Err(e) => {
                    self.scene_dirty = true;
                    EncState::Failed(e)
                }
            };
            changed = true;
        }
        changed
    }
```
Replace `busy` and `in_flight` with:

```rust
    /// Work is waiting or running (false while paused: the export keeps the app repainting). Includes the other
    /// mode's background encodes once A/B is in use.
    pub fn busy(&self) -> bool {
        !self.paused && (!self.settled() || self.alt_pending())
    }

    /// An other-mode copy is running or still to be started.
    fn alt_pending(&self) -> bool {
        self.ab
            && self.members.values().any(|m| {
                m.alt_token != 0
                    || (m.alt.is_none()
                        && !m.alt_err
                        && matches!(m.state, EncState::Fresh(_))
                        && !matches!(m.source, Source::Dds(_)))
            })
    }

    /// An encode is running (its completion is still to drain).
    fn in_flight(&self) -> bool {
        !self.paused
            && self
                .members
                .values()
                .any(|m| m.token != 0 || m.alt_token != 0)
    }
```

- [ ] **Step 4: GREEN for 8b**

Run: `cargo test -p sdc-gui --test set_preview_model`
Expected: the four new tests and **every existing one** pass. Watch these existing tests: `switching_mode_marks_members_stale_and_reencodes` (first flip → `Stale(Some(old))`), `a_completion_from_the_old_mode_is_rejected` (flip while Encoding: token 0, no alt), `a_no_op_cap_or_mode_change_keeps_members_fresh`, `dds_inputs_ignore_the_mode`.

- [ ] **Step 5: Commit**

```bash
git add crates/gui/src/model/set_preview.rs crates/gui/tests/set_preview_model.rs
git commit -m "gui: the set preview encoder keeps both Encoded and Source variants once A/B is used" -m "Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

### 8c: the set preview sends swaps and flips instead of a scene push

- [ ] **Step 1: Failing tests** — append to `crates/gui/tests/set_preview_model.rs` (`poll_once`, `builtin_key`, `rock`, `fake` exist above):

```rust
// ---- P1-B Task 8c: swaps instead of scene pushes ----

use render3d::Variant;
use sdc_gui::model::viewport3d::{V3Cmd, V3Reply};

struct Live {
    _d: tempfile::TempDir,
    m: AppModel,
    s: SetPreview,
    cmds: std::sync::mpsc::Receiver<V3Cmd>,
}

fn kinds(s: &SetPreview) -> Vec<SlotKind> {
    s.enc.members.keys().copied().collect()
}

/// The workers are held: the test finishes every member's pending encode with a fake DDS.
fn finish(s: &mut SetPreview, cap: u32) {
    for k in kinds(s) {
        let t = s.enc.token_of(k);
        assert_ne!(t, 0, "{k:?} has an encode to finish");
        s.enc.inject(k, t, cap, fake());
    }
}

/// A set preview on the built-in sphere, scene pushed, load sent and answered; encodes held.
fn live() -> Live {
    let dir = tempfile::tempdir().unwrap();
    let mut m = pbr_opened(dir.path());
    let key = rock(&m).0.key;
    m.select_set(&key);
    let fp = render3d::locate::Fingerprint {
        root: dir.path().to_path_buf(),
        owner_mod: Some("CS".into()),
        hash: "a".into(),
    };
    let mut s = SetPreview::new(Some(fp.clone()), None, None, DEFAULT_CAP);
    s.enc.hold_workers(true);
    poll_once(&mut s, &m); // starts the locate; the pump hands every member a held encode
    while s.v3.locating() {
        poll_once(&mut s, &m);
        std::thread::sleep(Duration::from_millis(5));
    }
    s.v3.on_located(Ok(Some(fp)));
    let cmds = s.v3.attach_test_thread();
    finish(&mut s, DEFAULT_CAP);
    poll_once(&mut s, &m); // Fresh → the scene is pushed
    s.v3.request_frame(64, 64);
    let id = cmds
        .try_iter()
        .find_map(|c| match c {
            V3Cmd::LoadBuiltin { id, .. } => Some(id),
            _ => None,
        })
        .expect("a LoadBuiltin");
    s.v3.handle(V3Reply::Loaded {
        id,
        report: render3d::LoadReport {
            shapes: 1,
            ..Default::default()
        },
        bounds: Some(([0.0; 3], 1.0)),
    });
    Live {
        _d: dir,
        m,
        s,
        cmds,
    }
}

fn drain(t: &Live) -> Vec<V3Cmd> {
    t.cmds.try_iter().collect()
}

fn n_paths(s: &SetPreview) -> usize {
    s.enc
        .members
        .values()
        .flat_map(|m| m.slot.paths.iter())
        .collect::<std::collections::HashSet<_>>()
        .len()
}

fn count(c: &[V3Cmd], f: impl Fn(&V3Cmd) -> bool) -> usize {
    c.iter().filter(|x| f(x)).count()
}

fn swaps(c: &[V3Cmd], v: Variant) -> usize {
    count(c, |x| matches!(x, V3Cmd::SwapTexture { variant, bytes, .. } if *variant == v && !bytes.is_empty()))
}

fn drops(c: &[V3Cmd], v: Variant) -> usize {
    count(c, |x| matches!(x, V3Cmd::SwapTexture { variant, bytes, .. } if *variant == v && bytes.is_empty()))
}

fn flips(c: &[V3Cmd], v: Variant) -> usize {
    count(c, |x| matches!(x, V3Cmd::SetTextureVariant { variant, .. } if *variant == v))
}

fn loads(c: &[V3Cmd]) -> usize {
    count(c, |x| {
        matches!(
            x,
            V3Cmd::Load { .. } | V3Cmd::LoadParsed { .. } | V3Cmd::LoadBuiltin { .. }
        )
    })
}

#[test]
fn an_encode_completion_on_loaded_content_swaps_the_texture_and_pushes_no_scene() {
    let mut t = live();
    drain(&t);
    let key0 = builtin_key(&t.s);
    // A new size cap re-encodes every member; the old bytes stay on show meanwhile.
    t.s.enc.set_cap(1024, Instant::now());
    poll_once(&mut t.s, &t.m);
    assert!(drain(&t).is_empty(), "nothing to send while the encodes run");
    finish(&mut t.s, 1024);
    poll_once(&mut t.s, &t.m);
    let c = drain(&t);
    assert_eq!(swaps(&c, Variant::Encoded), n_paths(&t.s));
    assert_eq!(loads(&c), 0, "no scene push");
    assert_eq!(flips(&c, Variant::Encoded), 0, "already the shown variant");
    assert_eq!(builtin_key(&t.s), key0);
    // Nothing changed since: nothing is sent again.
    poll_once(&mut t.s, &t.m);
    assert!(drain(&t).is_empty());
}

#[test]
fn a_failed_encode_still_pushes_the_scene_so_the_games_texture_comes_back() {
    let mut t = live();
    drain(&t);
    let key0 = builtin_key(&t.s);
    t.s.enc.set_cap(1024, Instant::now());
    poll_once(&mut t.s, &t.m);
    for k in kinds(&t.s) {
        let tok = t.s.enc.token_of(k);
        t.s.enc.inject(k, tok, 1024, Err("boom".into()));
    }
    poll_once(&mut t.s, &t.m);
    poll_once(&mut t.s, &t.m);
    assert_ne!(builtin_key(&t.s), key0, "the scene was pushed");
}

#[test]
fn flipping_the_mode_with_both_variants_sends_only_a_variant_flip_and_starts_no_encode() {
    let mut t = live();
    drain(&t);
    let n = n_paths(&t.s);
    // First flip: today's re-encode of the new mode; the old bytes are kept as the other copy.
    t.s.set_mode(PreviewMode::Source);
    assert!(t.s.enc.ab_active());
    assert!(drain(&t).is_empty(), "nothing Fresh to send yet");
    poll_once(&mut t.s, &t.m);
    finish(&mut t.s, DEFAULT_CAP);
    poll_once(&mut t.s, &t.m);
    let c = drain(&t);
    assert_eq!(swaps(&c, Variant::Source), n);
    assert_eq!(flips(&c, Variant::Source), n, "then show it");
    assert_eq!(swaps(&c, Variant::Encoded), 0, "the renderer already holds the Encoded copy");
    assert_eq!(loads(&c), 0);
    // Second flip: both variants exist.
    t.s.set_mode(PreviewMode::Encoded);
    let c = drain(&t);
    assert_eq!(c.len(), n, "only the flips");
    assert_eq!(flips(&c, Variant::Encoded), n);
    poll_once(&mut t.s, &t.m);
    assert!(drain(&t).is_empty());
    for k in kinds(&t.s) {
        assert_eq!(t.s.enc.token_of(k), 0, "no encode started");
        assert_eq!(t.s.enc.alt_token_of(k), 0);
    }
}

#[test]
fn a_source_change_voids_both_variants_and_the_inactive_one_follows_the_active() {
    let mut t = live();
    t.s.set_mode(PreviewMode::Source);
    poll_once(&mut t.s, &t.m);
    finish(&mut t.s, DEFAULT_CAP);
    poll_once(&mut t.s, &t.m);
    drain(&t);
    let n = n_paths(&t.s);
    t.s.enc.set_cap(1024, Instant::now()); // a settings change as the encoder sees it
    for k in kinds(&t.s) {
        assert!(!t.s.enc.has_alt(k));
    }
    poll_once(&mut t.s, &t.m); // the renderer's stale Encoded copy is dropped
    let c = drain(&t);
    assert_eq!(drops(&c, Variant::Encoded), n);
    finish(&mut t.s, 1024); // the shown (Source) mode
    poll_once(&mut t.s, &t.m);
    let c = drain(&t);
    assert_eq!(swaps(&c, Variant::Source), n);
    for k in kinds(&t.s) {
        assert_ne!(t.s.enc.alt_token_of(k), 0, "the inactive mode encodes now");
    }
    for k in kinds(&t.s) {
        let at = t.s.enc.alt_token_of(k);
        t.s.enc.inject_as(k, at, 1024, PreviewMode::Encoded, fake());
    }
    poll_once(&mut t.s, &t.m);
    let c = drain(&t);
    assert_eq!(swaps(&c, Variant::Encoded), n, "stored beside the shown one");
    assert_eq!(flips(&c, Variant::Encoded), 0, "not shown");
    assert_eq!(loads(&c), 0);
}

#[test]
fn a_flip_before_the_other_variant_exists_falls_back_to_the_re_encode() {
    let mut t = live();
    drain(&t);
    t.s.enc.set_cap(1024, Instant::now()); // everything stale: no Fresh bytes to keep
    t.s.set_mode(PreviewMode::Source);
    poll_once(&mut t.s, &t.m);
    for k in kinds(&t.s) {
        assert_ne!(t.s.enc.token_of(k), 0, "encoded as before the A/B");
        assert!(!t.s.enc.has_alt(k));
    }
    assert!(drain(&t).iter().all(|c| !matches!(c, V3Cmd::SetTextureVariant { .. })));
}
```
Run: `cargo test -p sdc-gui --test set_preview_model -- loaded_content failed_encode both_variants source_change falls_back` → the first fails (a `LoadBuiltin` is pushed, no `SwapTexture`) or does not compile if 8b is not in.

- [ ] **Step 2: `set_preview.rs`: the mirror, the sync, the push**

Add two fields to `pub struct SetPreview` after `pushed_norms: HashSet<String>,`:

```rust
    /// What the viewport's renderer holds of the encoder's bytes: (path, mode) → `Encoded.id`, and the mode each path
    /// is showing. Rebuilt by every scene push, kept in line by [`SetPreview::sync_textures`] (P1-B A/B).
    tex_sent: HashMap<(String, PreviewMode), u64>,
    tex_active: HashMap<String, PreviewMode>,
```
and in `SetPreview::new` after `pushed_norms: HashSet::new(),` add `tex_sent: HashMap::new(), tex_active: HashMap::new(),`.

Replace `SetPreview::set_mode` with:

```rust
    /// Encoded / Source (not persisted): only the encodes change, the meshes stay. With both variants encoded this
    /// is a `SetTextureVariant` per path and nothing else; otherwise the new mode encodes as before and its bytes
    /// are swapped in when they land. The viewport cannot take it yet (no load of this scene sent): push the scene.
    pub fn set_mode(&mut self, mode: PreviewMode) {
        if mode == self.enc.mode() {
            return;
        }
        self.enc.set_mode(mode, Instant::now());
        if !self.sync_textures() {
            self.want_scene = true;
        }
    }
```
In `poll_at`, replace

```rust
        if self.enc.poll() {
            self.want_scene = true;
        }
```
with
```rust
        // A finished encode on a scene that is already loaded is swapped in (`sync_textures`); the scene is pushed
        // for a first load and when an encode failed (the game's copy of the texture must come back).
        let changed = self.enc.poll();
        let failed = std::mem::take(&mut self.enc.scene_dirty);
        if changed && (failed || self.on_screen.is_none()) {
            self.want_scene = true;
        }
```
and after the block
```rust
        if self.want_scene && self.enc.settled() && self.querying.is_none() {
            self.push_scene();
        }
```
add
```rust
        if !self.want_scene && !self.sync_textures() {
            self.want_scene = true;
        }
        for e in std::mem::take(&mut self.v3.swap_errors) {
            self.notice(e);
        }
```
In `push_scene`, after `let memory = self.enc.memory_sources();` add `self.v3.set_load_variant(variant_of(self.enc.mode()));`, and immediately before `self.timings.push_ms = Some(ms(t0));` add `self.reset_tex_mirror();`.

Add to an `impl SetPreview` block (e.g. right after `apply_materials`):

```rust
    /// After a scene push the renderer holds exactly what `memory_sources` served, as the current mode's variant
    /// (`set_load_variant`), and shows it.
    fn reset_tex_mirror(&mut self) {
        self.tex_sent.clear();
        self.tex_active.clear();
        let mode = self.enc.mode();
        for m in self.enc.members.values() {
            for p in &m.slot.paths {
                // A path without bytes on show may still be loaded from the game's copy, under the same label.
                self.tex_active.insert(p.clone(), mode);
                if let Some(e) = m.state.shown() {
                    self.tex_sent.insert((p.clone(), mode), e.id);
                }
            }
        }
    }

    /// Brings the renderer's copy of the set's textures in line with the encoder without a scene push (P1-B Task 8):
    /// the shown mode's Fresh bytes as `SwapTexture`; once A/B is in use the other mode's Fresh bytes beside them
    /// (only for a path the renderer already shows: a lone copy would become the shown one) and an empty
    /// `SwapTexture` for a copy that went stale; then `SetTextureVariant` for the shown mode. False when the viewport
    /// cannot take a required command (no load of this scene has been sent yet): the caller pushes the scene.
    fn sync_textures(&mut self) -> bool {
        if self.on_screen.is_none() {
            return true;
        }
        let mode = self.enc.mode();
        let other = mode.other();
        let ab = self.enc.ab_active();
        // Later members win a shared path, as in `memory_sources`.
        let mut want: HashMap<(String, PreviewMode), Arc<Encoded>> = HashMap::new();
        let mut paths: Vec<String> = vec![];
        let mut stale: Vec<(String, PreviewMode)> = vec![];
        for m in self.enc.members.values() {
            for p in &m.slot.paths {
                paths.push(p.clone());
                if let EncState::Fresh(e) = &m.state {
                    want.insert((p.clone(), mode), e.clone());
                }
                match (&m.alt, ab) {
                    (Some(e), true) => {
                        want.insert((p.clone(), other), e.clone());
                    }
                    // A stale copy goes, unless it is the one on screen (a flip before the new mode landed keeps
                    // showing the old picture until it does).
                    _ if self.tex_sent.contains_key(&(p.clone(), other))
                        && self.tex_active.get(p) != Some(&other) =>
                    {
                        stale.push((p.clone(), other))
                    }
                    _ => {}
                }
            }
        }
        let mut flip: HashSet<String> = HashSet::new();
        for ((p, label), e) in want.iter().filter(|(k, _)| k.1 == mode) {
            if self.tex_sent.get(&(p.clone(), *label)) == Some(&e.id) {
                continue;
            }
            let is_new = !self.tex_sent.contains_key(&(p.clone(), *label));
            if !self
                .v3
                .swap_texture(p.clone(), e.dds.clone(), variant_of(*label))
            {
                return false;
            }
            self.tex_sent.insert((p.clone(), *label), e.id);
            // The renderer shows the first texture a path gets.
            self.tex_active.entry(p.clone()).or_insert(*label);
            if is_new {
                flip.insert(p.clone());
            }
        }
        for ((p, label), e) in want.iter().filter(|(k, _)| k.1 == other) {
            if !self.tex_sent.contains_key(&(p.clone(), mode))
                || self.tex_sent.get(&(p.clone(), *label)) == Some(&e.id)
            {
                continue;
            }
            // Not required: refused now, tried again on the next frame.
            if self
                .v3
                .swap_texture(p.clone(), e.dds.clone(), variant_of(*label))
            {
                self.tex_sent.insert((p.clone(), *label), e.id);
            }
        }
        for key in stale {
            if self
                .v3
                .swap_texture(key.0.clone(), Arc::new(vec![]), variant_of(key.1))
            {
                self.tex_sent.remove(&key);
            }
        }
        for p in paths {
            if !self.tex_sent.contains_key(&(p.clone(), mode)) {
                continue;
            }
            if flip.contains(&p) || self.tex_active.get(&p) != Some(&mode) {
                if !self.v3.set_texture_variant(p.clone(), variant_of(mode)) {
                    return false;
                }
                self.tex_active.insert(p, mode);
            }
        }
        true
    }
```

- [ ] **Step 3: GREEN for 8c and the existing suites**

Run: `cargo test -p sdc-gui --test set_preview_model`
Expected: PASS: the five new tests and every existing one. In particular `a_material_edit_naming_a_new_slot_path_pushes_the_scene_instead_of_set_material` (slot-path changes still push) and the real-worker `run()` tests (no render thread attached: `swap_texture` refuses, so the required commands fall back to the scene push exactly as before; optional ones are retried silently).
Run: `cargo test -p sdc-gui` (all of `set_preview_ui`, `viewport3d`, `meshes`…).

- [ ] **Step 4: UI test — the toggle still calls `set_mode`** — append to `crates/gui/tests/set_preview_ui.rs`:

```rust
#[test]
fn the_mode_toggle_still_calls_set_mode_and_puts_ab_in_use() {
    let (_d, mut h) = plain_rock();
    assert!(!h.state().1.enc.ab_active());
    h.get_by_label(MODE_LABEL_SOURCE).click();
    h.run_steps(4);
    assert_eq!(h.state().1.mode(), PreviewMode::Source);
    assert_eq!(h.state().1.enc.mode(), PreviewMode::Source);
    assert!(h.state().1.enc.ab_active(), "the first flip puts A/B in use");
    h.get_by_label(MODE_LABEL_ENCODED).click();
    h.run_steps(4);
    assert_eq!(h.state().1.mode(), PreviewMode::Encoded);
}
```
(`ui/set_preview.rs::mode_toggle` is unchanged: it already calls `s.set_mode(mode)`.)
Run: `cargo test -p sdc-gui --test set_preview_ui`

- [ ] **Step 5: Gates and commit**

Run: `cargo fmt --all && cargo clippy --workspace --all-targets -- -D warnings && cargo test --workspace`

```bash
git add crates/gui
git commit -m "gui: set preview swaps encoded textures into the loaded scene and flips Encoded/Source instantly" -m "An encode completion on loaded content sends SwapTexture instead of a scene push (a failure and a first load still push); once A/B is used both variants are kept per member and flips are SetTextureVariant only." -m "Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

---

## Task 9: preview timing log (automatic timings for the owner checklist)

Owner request: "ensure logging is added for recorded timings, i cannot reliably capture that information". The Task 10 checklist stops asking for stop-watch numbers: the app writes every timing it can know to a JSON Lines file, and the owner sends the file back.

**Where the logger lives:** `crates/render3d/src/perf_log.rs` (render3d is a nif-team crate and gui already depends on it, so `model/mod.rs` stays untouched and both the compile pool and the GUI can call the same global `emit`).

**Files:**
- Create: `crates/render3d/src/perf_log.rs`
- Modify: `crates/render3d/src/lib.rs` (`pub mod perf_log;`; `Wanted.since`; hooks in `begin`, `apply_compiled`, `lookup`), `crates/render3d/src/pool.rs` (`run_job` logs `compile`), `crates/render3d/src/cache.rs` (`prune_logged`, used by `prune_in_background`)
- Modify: `crates/gui/src/main.rs` (2 lines: `let _perf = render3d::perf_log::start();` as the first statement of `fn main`, with a one-line comment; own commit, see the Global Constraints ruling), `crates/gui/src/model/viewport3d.rs` (gap watch, marks, `load`, `first_frame`, `gpu`, `device_rebuild`, `frame_stats`), `crates/gui/src/model/set_preview.rs` (`encode`, `panel`, `extra_busy`), `crates/gui/src/ui/set_preview.rs` (log path + "Copy log path")
- Test: `crates/render3d/tests/perf_log.rs` (new: the logger, pure functions), `crates/render3d/tests/perf_hooks.rs` (new: the render3d hooks), append to `crates/gui/tests/viewport3d.rs`, `crates/gui/tests/set_preview_model.rs`, `crates/gui/tests/set_preview_ui.rs`

**Interfaces:**
- Consumes: Task 3 `ProgramKey(pub String)`, `compile::{cached, compile}`; Task 4 `cache::{prune, PruneLimits, PruneReport}`, `prune_in_background`; Task 5 `CompileJob`/`run_job`, `Renderer::{take_jobs, apply_compiled, set_async_compile}`, `Wanted`, `ShapeStatus::{Pending, Failed}`; Task 6 `render_loop`, `V3Reply::Refresh`, `rebuild`; Task 8 `Viewport3d::{swap_texture, set_texture_variant}`, `send_texture_cmd`, `V3Reply::TextureSwap`, the 8b `Encoder::poll`; P1-A `FrameTiming`, `Encoded.ms`, `V3Reply::{Ready, Frame, MaterialSet}`.
- Produces:
  ```rust
  // render3d::perf_log
  pub struct Opts { pub max_files: usize, pub max_bytes: u64, pub queue: usize }   // Default: 10, 20 MiB, 8192
  pub struct PerfLog;                                   // one writer thread; Drop writes `run_end` and joins
  impl PerfLog {
      pub fn open(dir: &Path, stamp: &str, o: &Opts) -> io::Result<PerfLog>;   // rotates, creates preview-timings-<stamp>.jsonl
      pub fn emit(&self, event: &str, fields: serde_json::Value);              // never blocks; Full => dropped += 1
      pub fn path(&self) -> &Path;
  }
  pub fn open_if_enabled(dir: &Path, stamp: &str, switch: Option<&str>) -> Option<PerfLog>;  // switch == Some("0") => None
  pub fn start() -> Option<Arc<PerfLog>>;               // %LOCALAPPDATA%\sdc\logs, SDC_PREVIEW_LOG=0 disables
  pub fn start_in(dir: &Path) -> Option<Arc<PerfLog>>;  // same, explicit dir, ignores the env var (tests)
  pub fn emit(event: &str, fields: serde_json::Value);  // global: thread-local capture first, else the live logger, else nothing
  pub fn path() -> Option<PathBuf>;                     // the live log file
  pub fn capture<R>(f: impl FnOnce() -> R) -> (R, Vec<Value>);   // #[doc(hidden)] test seam, this thread only
  pub fn stamp(t: SystemTime) -> String;                // UTC "YYYYMMDD-HHMMSS"
  pub struct GapWatch;  impl GapWatch { pub fn tick(&mut self, now: Instant, busy: bool) -> Option<f64> }
  pub struct FrameStats; impl FrameStats { pub fn push(&mut self, now: Instant, draw_ms: f64, total_ms: f64) -> Option<Value> }
  pub fn short(key: &str) -> String;  pub fn first_line(s: &str) -> String;  pub fn ms_since(t: Instant) -> f64;
  // render3d::cache
  pub fn prune_logged(dir: &Path, limits: &PruneLimits, now: SystemTime) -> PruneReport;
  // gui
  Viewport3d::poll_at(&mut self, now: Instant) -> bool;  pub panel: &'static str;  pub extra_busy: bool;
  Content::label(&self) -> String;
  ```

**Event reference (what is written; every line has `t_ms` = ms since the log opened, 3 decimals, and `event`).** Refinements of the owner design, all keeping its shape: `run_start` carries only what the library knows (`version`); the adapter, `warp` and `cs_root` come in a separate `gpu` event on every renderer `Ready` (also after a device rebuild); `shape_pending` is logged when a shape starts waiting and `shape_ready` (with `ms_pending`) when it resolves; `load` has `build_ms` only (a parsed NIF is parsed on the UI thread, a Meshes-view NIF is parsed inside `build_ms`), and a parsed/builtin content is named by its content key (`parsed:<key>`, `builtin:<shape>`, `mesh:<key>`), not by the NIF path (the content does not carry it).

| event | fields | written by |
|---|---|---|
| `run_start` | `version` | `start_in` |
| `gpu` | `adapter`, `warp`, `cs_root` | `Viewport3d::handle(Ready)` |
| `ui_gap` | `gap_ms`, `panel` (`meshes`/`set_preview`) | `Viewport3d::poll_at` |
| `compile` | `key` (12 hex), `ms`, `cache` (`hit`/`miss`), `ok`, `error?` | `run_job` (worker), `Renderer::lookup` (sync disk hit) |
| `shape_pending` | `shape`, `key`, `showing_previous` | `Renderer::begin` |
| `shape_ready` | `shape`, `ms_pending`, `ok` | `Renderer::apply_compiled` |
| `load` | `content`, `shapes`, `pending`, `build_ms` | render loop |
| `first_frame` | `ms_since_load` | `Viewport3d::handle(Frame)` |
| `edit` | `shape`, `kind` (`Same`/`Cbuffer`/`Textures`/`Permutation`), `ms_to_frame` | `handle(Frame)` |
| `texture_swap` | `path`, `variant`, `bytes`, `ms_to_frame` | `handle(Frame)` |
| `ab_flip` | `paths`, `variant`, `ms_to_frame` | `handle(Frame)` |
| `encode` | `slot`, `mode`, `ms`, `bytes`, `ok`, `error?` | `Encoder::poll` |
| `prune` | `removed`, `bytes_before`, `bytes_after`, `ms` | `cache::prune_logged` |
| `frame_stats` | `frames`, `draw_ms_p50`, `draw_ms_p95`, `total_ms_p95` (every 10 s while frames are produced) | render loop |
| `device_rebuild` | `ms` | render loop `rebuild` |
| `log_capped` | `bytes`, `dropped` (last line when the 20 MB cap is hit) | writer |
| `run_end` | `dropped` | writer, at `Drop` |

`ms_to_frame` = the UI thread sent the command to the render thread, until the first `Frame` reply that was requested after the command (id greater than the last render id sent when the command was sent; `Refresh` frames never count) reached `Viewport3d::handle`. That is what the owner sees: queue wait + apply + render + hand-over, not just the render thread's share. For an `edit` whose permutation compiles in the background it is the time to the first frame (the old image), while the compile time itself is `shape_ready.ms_pending`.

**How `ui_gap` avoids false positives.** `poll` runs once per egui frame. egui stops running frames when nothing requests a repaint, so a long gap between two polls is normal when idle. `GapWatch::tick(now, busy)` only reports a gap when the previous tick had `busy == true` (3D work in flight: `Viewport3d::busy()`, or for the set preview also an encode running, a scene push wanted, or a mesh query running; all of those keep the app repainting every frame). It also ignores gaps of 60 s or more (laptop sleep, window minimised mid-work). Known limit: a freeze that starts while the previous frame saw no work in flight (the UI thread blocks inside the very first frame of a burst) is not caught by this path; the long `edit`/`texture_swap`/`ab_flip` `ms_to_frame` of that burst shows it.

- [ ] **Step 1: Failing tests, the logger** — create `crates/render3d/tests/perf_log.rs`:

```rust
//! Task 9: the preview timing log. The writer is exercised on a temp dir with explicit stamps and options; the
//! gap watch and the frame statistics are pure (injected `Instant`s, no sleeps).
use render3d::perf_log::{self, FrameStats, GapWatch, Opts, PerfLog};
use serde_json::{json, Value};
use std::fs;
use std::path::Path;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

fn lines(p: &Path) -> Vec<Value> {
    fs::read_to_string(p)
        .unwrap()
        .lines()
        .map(|l| serde_json::from_str(l).unwrap_or_else(|e| panic!("not JSON: {l:?}: {e}")))
        .collect()
}

fn logs(dir: &Path) -> Vec<String> {
    let mut v: Vec<String> = fs::read_dir(dir)
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .filter(|n| n.starts_with("preview-timings-"))
        .collect();
    v.sort();
    v
}

#[test]
fn every_line_is_a_json_object_with_t_ms_and_event_and_the_log_ends_with_run_end() {
    let d = tempfile::tempdir().unwrap();
    let log = PerfLog::open(d.path(), "20261011-100000", &Opts::default()).unwrap();
    let path = log.path().to_path_buf();
    assert!(path.ends_with("preview-timings-20261011-100000.jsonl"));
    log.emit("compile", json!({"key": "abc", "ms": 1.5, "cache": "miss", "ok": true}));
    log.emit("edit", json!({"shape": 3, "kind": "Same", "ms_to_frame": 2.0}));
    drop(log);
    let l = lines(&path);
    assert_eq!(l.len(), 3);
    for e in &l {
        assert!(e["t_ms"].is_f64() || e["t_ms"].is_u64(), "{e}");
        assert!(e["event"].is_string());
    }
    assert_eq!((l[0]["event"].as_str(), l[0]["cache"].as_str()), (Some("compile"), Some("miss")));
    assert_eq!(l[2]["event"], "run_end");
    assert_eq!(l[2]["dropped"], 0);
    assert!(l[0]["t_ms"].as_f64() <= l[2]["t_ms"].as_f64());
}

#[test]
fn only_the_ten_newest_logs_are_kept_and_other_files_are_left_alone() {
    let d = tempfile::tempdir().unwrap();
    for n in 1..=12 {
        fs::write(d.path().join(format!("preview-timings-202610{n:02}-000000.jsonl")), "{}\n").unwrap();
    }
    fs::write(d.path().join("other.txt"), "x").unwrap();
    let log = PerfLog::open(d.path(), "20261020-000000", &Opts::default()).unwrap();
    drop(log);
    let kept = logs(d.path());
    assert_eq!(kept.len(), 10, "{kept:?}");
    assert!(!kept.contains(&"preview-timings-20261001-000000.jsonl".to_string()));
    assert!(!kept.contains(&"preview-timings-20261003-000000.jsonl".to_string()));
    assert!(kept.contains(&"preview-timings-20261004-000000.jsonl".to_string()));
    assert!(kept.contains(&"preview-timings-20261020-000000.jsonl".to_string()));
    assert!(d.path().join("other.txt").exists());
}

#[test]
fn two_runs_in_the_same_second_get_two_files() {
    let d = tempfile::tempdir().unwrap();
    let a = PerfLog::open(d.path(), "20261011-100000", &Opts::default()).unwrap();
    let b = PerfLog::open(d.path(), "20261011-100000", &Opts::default()).unwrap();
    assert_ne!(a.path(), b.path());
}

#[test]
fn a_full_file_gets_one_log_capped_line_and_nothing_after() {
    let d = tempfile::tempdir().unwrap();
    let o = Opts {
        max_bytes: 400,
        ..Opts::default()
    };
    let log = PerfLog::open(d.path(), "20261011-100000", &o).unwrap();
    let path = log.path().to_path_buf();
    for i in 0..50 {
        log.emit("edit", json!({"shape": i, "kind": "Same", "ms_to_frame": 1.0}));
    }
    drop(log);
    let l = lines(&path);
    let capped: Vec<_> = l.iter().filter(|e| e["event"] == "log_capped").collect();
    assert_eq!(capped.len(), 1, "{l:?}");
    assert_eq!(l.last().unwrap()["event"], "log_capped");
    assert!(l.len() < 12, "the cap stopped the writing: {}", l.len());
    assert!(fs::metadata(&path).unwrap().len() < 400 + 200);
}

#[test]
fn the_switch_turns_the_log_off_and_writes_nothing() {
    let d = tempfile::tempdir().unwrap();
    assert!(perf_log::open_if_enabled(d.path(), "20261011-100000", Some("0")).is_none());
    assert!(logs(d.path()).is_empty());
    assert!(perf_log::open_if_enabled(d.path(), "20261011-100001", Some("1")).is_some());
    assert!(perf_log::open_if_enabled(d.path(), "20261011-100002", None).is_some());
}

#[test]
fn the_stamp_is_utc_and_sorts_by_time() {
    let at = |s: u64| UNIX_EPOCH + Duration::from_secs(s);
    assert_eq!(perf_log::stamp(at(1_000_000_000)), "20010909-014640");
    assert_eq!(perf_log::stamp(at(1_700_000_000)), "20231114-221320");
    assert_eq!(perf_log::stamp(at(1_709_164_800)), "20240229-000000"); // leap day
    assert!(perf_log::stamp(SystemTime::now()).len() == 15);
}

#[test]
fn capture_collects_this_threads_events_and_the_global_emit_is_a_no_op_without_a_log() {
    perf_log::emit("nobody_listens", json!({}));
    let (r, ev) = perf_log::capture(|| {
        perf_log::emit("a", json!({"x": 1}));
        perf_log::emit("b", json!({}));
        7
    });
    assert_eq!(r, 7);
    assert_eq!(ev.iter().map(|e| e["event"].as_str().unwrap()).collect::<Vec<_>>(), ["a", "b"]);
    assert_eq!(ev[0]["x"], 1);
    let (_, none) = perf_log::capture(|| ());
    assert!(none.is_empty());
}

#[test]
fn helpers_shorten_keys_and_errors() {
    assert_eq!(perf_log::short("0123456789abcdef0123"), "0123456789ab");
    assert_eq!(perf_log::short("ab"), "ab");
    assert_eq!(perf_log::first_line("ps: X3000\nline 2"), "ps: X3000");
    assert_eq!(perf_log::first_line(""), "");
}

#[test]
fn a_gap_is_reported_only_when_the_previous_frame_saw_work_in_flight() {
    let t0 = Instant::now();
    let at = |ms: u64| t0 + Duration::from_millis(ms);
    let mut g = GapWatch::default();
    assert_eq!(g.tick(at(0), true), None, "the first tick has no previous one");
    assert_eq!(g.tick(at(99), true), None, "under 100 ms is a frame, not a freeze");
    assert_eq!(g.tick(at(199), true), Some(100.0), "exactly 100 ms counts");
    assert_eq!(g.tick(at(700), false), Some(501.0), "work was in flight at the previous tick");
    assert_eq!(g.tick(at(5_000), false), None, "idle: egui simply did not run frames");
    assert_eq!(g.tick(at(5_050), true), None);
    assert_eq!(g.tick(at(5_050 + 60_000), true), None, "a minute or more is sleep, not a freeze");
    assert_eq!(g.tick(at(5_050 + 60_000 + 250), false), Some(250.0));
}

#[test]
fn frame_stats_report_once_per_ten_seconds_with_percentiles() {
    let t0 = Instant::now();
    let at = |ms: u64| t0 + Duration::from_millis(ms);
    let mut s = FrameStats::default();
    for i in 0..100u64 {
        // draw 1..=100 ms, total twice that
        assert_eq!(s.push(at(i * 50), (i + 1) as f64, 2.0 * (i + 1) as f64), None, "under 10 s");
    }
    let v = s.push(at(10_000), 101.0, 202.0).expect("10 s passed");
    assert_eq!(v["frames"], 101);
    assert_eq!(v["draw_ms_p50"], 51.0);
    assert_eq!(v["draw_ms_p95"], 96.0);
    assert_eq!(v["total_ms_p95"], 192.0);
    // The window restarts at the next frame.
    assert_eq!(s.push(at(10_100), 1.0, 1.0), None);
    assert_eq!(s.push(at(19_000), 1.0, 1.0), None);
    assert!(s.push(at(20_100), 1.0, 1.0).is_some());
}
```

- [ ] **Step 2: Run to verify RED**

Run: `cargo test -p render3d --test perf_log`
Expected: FAIL to compile: `unresolved import render3d::perf_log`.

- [ ] **Step 3: `perf_log.rs`** — create `crates/render3d/src/perf_log.rs`:

```rust
//! The preview timing log: JSON Lines in `%LOCALAPPDATA%\sdc\logs\preview-timings-<UTC stamp>.jsonl`, one file
//! per app run, the 10 newest kept, 20 MiB cap per file. Events go through a bounded channel to one writer
//! thread, so neither the UI thread nor the render thread ever waits for the disk; when the channel is full the
//! event is dropped and counted (`run_end.dropped`). `SDC_PREVIEW_LOG=0` turns it off. The log is how the owner
//! reports timings (Task 10): the app measures, the owner sends the file.
use serde_json::{json, Map, Value};
use std::cell::RefCell;
use std::fs::{self, File, OpenOptions};
use std::io::{BufWriter, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::{sync_channel, Receiver, SyncSender, TrySendError};
use std::sync::{Arc, Mutex, PoisonError, Weak};
use std::thread::JoinHandle;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

const PREFIX: &str = "preview-timings-";
const EXT: &str = ".jsonl";
/// A UI frame gap at or above this, with work in flight, is a freeze worth a line.
pub const GAP_MS: f64 = 100.0;
/// Longer gaps are the machine sleeping or the window minimised, not a freeze.
const GAP_MAX_MS: f64 = 60_000.0;
const STATS_EVERY: Duration = Duration::from_secs(10);

pub struct Opts {
    pub max_files: usize,
    pub max_bytes: u64,
    pub queue: usize,
}

impl Default for Opts {
    fn default() -> Opts {
        Opts {
            max_files: 10,
            max_bytes: 20 * 1024 * 1024,
            queue: 8192,
        }
    }
}

enum Msg {
    Line(String),
    Stop,
}

pub struct PerfLog {
    start: Instant,
    path: PathBuf,
    tx: SyncSender<Msg>,
    dropped: Arc<AtomicU64>,
    writer: Option<JoinHandle<()>>,
}

fn r3(x: f64) -> f64 {
    (x * 1000.0).round() / 1000.0
}

fn object(event: &str, t_ms: f64, fields: Value) -> Value {
    let mut m = Map::new();
    m.insert("t_ms".into(), json!(r3(t_ms)));
    m.insert("event".into(), json!(event));
    if let Value::Object(f) = fields {
        m.extend(f);
    }
    Value::Object(m)
}

fn line_of(event: &str, start: Instant, fields: Value) -> String {
    let mut s =
        serde_json::to_string(&object(event, start.elapsed().as_secs_f64() * 1000.0, fields))
            .unwrap_or_default();
    s.push('\n');
    s
}

impl PerfLog {
    /// Creates `preview-timings-<stamp>.jsonl` in `dir` (a `_<n>` suffix when that name is taken) after deleting
    /// the oldest logs so that at most `max_files` remain, the new one included. Only names of this shape are
    /// ever deleted.
    pub fn open(dir: &Path, stamp: &str, o: &Opts) -> std::io::Result<PerfLog> {
        fs::create_dir_all(dir)?;
        let mut old: Vec<PathBuf> = fs::read_dir(dir)?
            .filter_map(Result::ok)
            .map(|e| e.path())
            .filter(|p| {
                p.file_name()
                    .and_then(|n| n.to_str())
                    .is_some_and(|n| n.starts_with(PREFIX) && n.ends_with(EXT))
            })
            .collect();
        old.sort();
        let excess = old.len().saturating_sub(o.max_files.saturating_sub(1));
        for p in &old[..excess] {
            let _ = fs::remove_file(p);
        }
        let (path, file) = (0..1000)
            .find_map(|n| {
                let name = if n == 0 {
                    format!("{PREFIX}{stamp}{EXT}")
                } else {
                    format!("{PREFIX}{stamp}_{n}{EXT}")
                };
                let p = dir.join(name);
                OpenOptions::new()
                    .write(true)
                    .create_new(true)
                    .open(&p)
                    .ok()
                    .map(|f| (p, f))
            })
            .ok_or_else(|| std::io::Error::other("no free log file name"))?;
        let (tx, rx) = sync_channel(o.queue);
        let dropped = Arc::new(AtomicU64::new(0));
        let start = Instant::now();
        let writer = std::thread::Builder::new().name("perf-log".into()).spawn({
            let (dropped, cap) = (dropped.clone(), o.max_bytes);
            move || write_loop(rx, BufWriter::new(file), cap, &dropped, start)
        })?;
        Ok(PerfLog {
            start,
            path,
            tx,
            dropped,
            writer: Some(writer),
        })
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Queues one event; never blocks. A full queue drops it and counts it.
    pub fn emit(&self, event: &str, fields: Value) {
        let line = line_of(event, self.start, fields);
        if let Err(TrySendError::Full(_)) = self.tx.try_send(Msg::Line(line)) {
            self.dropped.fetch_add(1, Ordering::Relaxed);
        }
    }
}

impl Drop for PerfLog {
    fn drop(&mut self) {
        let _ = self.tx.send(Msg::Stop);
        if let Some(h) = self.writer.take() {
            let _ = h.join();
        }
    }
}

/// Writes batches and flushes when the queue is empty, so a killed app still leaves its lines on disk. Past the
/// cap: one `log_capped` line, then nothing (not even `run_end`).
fn write_loop(
    rx: Receiver<Msg>,
    mut out: BufWriter<File>,
    cap: u64,
    dropped: &AtomicU64,
    start: Instant,
) {
    let mut bytes = 0u64;
    let mut capped = false;
    let mut put = |out: &mut BufWriter<File>, line: &str| {
        if capped {
            return;
        }
        if bytes + line.len() as u64 > cap {
            capped = true;
            let l = line_of(
                "log_capped",
                start,
                json!({"bytes": bytes, "dropped": dropped.load(Ordering::Relaxed)}),
            );
            let _ = out.write_all(l.as_bytes());
        } else {
            bytes += line.len() as u64;
            let _ = out.write_all(line.as_bytes());
        }
    };
    while let Ok(first) = rx.recv() {
        for m in std::iter::once(first).chain(rx.try_iter()) {
            match m {
                Msg::Line(l) => put(&mut out, &l),
                Msg::Stop => {
                    let l = line_of(
                        "run_end",
                        start,
                        json!({"dropped": dropped.load(Ordering::Relaxed)}),
                    );
                    put(&mut out, &l);
                    let _ = out.flush();
                    return;
                }
            }
        }
        let _ = out.flush();
    }
    let _ = out.flush();
}

/// `switch` is the value of `SDC_PREVIEW_LOG`; `"0"` is off, anything else (or unset) is on.
pub fn open_if_enabled(dir: &Path, stamp: &str, switch: Option<&str>) -> Option<PerfLog> {
    if switch == Some("0") {
        return None;
    }
    PerfLog::open(dir, stamp, &Opts::default()).ok()
}

static GLOBAL: Mutex<Weak<PerfLog>> = Mutex::new(Weak::new());

thread_local! {
    static SINK: RefCell<Option<Vec<Value>>> = const { RefCell::new(None) };
}

/// Opens the process log in `%LOCALAPPDATA%\sdc\logs` unless `SDC_PREVIEW_LOG=0` (or no `%LOCALAPPDATA%`).
/// The caller keeps the returned `Arc` alive for the whole run: dropping the last one writes `run_end`.
pub fn start() -> Option<Arc<PerfLog>> {
    if std::env::var("SDC_PREVIEW_LOG").as_deref() == Ok("0") {
        return None;
    }
    let dir = std::env::var_os("LOCALAPPDATA").map(|a| PathBuf::from(a).join("sdc").join("logs"))?;
    start_in(&dir)
}

/// [`start`] with an explicit directory and no environment switch. A second call while a log is alive returns it.
pub fn start_in(dir: &Path) -> Option<Arc<PerfLog>> {
    let mut g = GLOBAL.lock().unwrap_or_else(PoisonError::into_inner);
    if let Some(l) = g.upgrade() {
        return Some(l);
    }
    let l = Arc::new(open_if_enabled(dir, &stamp(SystemTime::now()), None)?);
    *g = Arc::downgrade(&l);
    drop(g);
    l.emit("run_start", json!({"version": env!("CARGO_PKG_VERSION")}));
    Some(l)
}

/// The global emit: this thread's capture if one is active, else the live log, else nothing.
pub fn emit(event: &str, fields: Value) {
    let captured = SINK.with(|s| match s.borrow_mut().as_mut() {
        Some(v) => {
            v.push(object(event, 0.0, fields.clone()));
            true
        }
        None => false,
    });
    if captured {
        return;
    }
    let live = GLOBAL.lock().unwrap_or_else(PoisonError::into_inner).upgrade();
    if let Some(l) = live {
        l.emit(event, fields);
    }
}

/// The live log file, for the panel.
pub fn path() -> Option<PathBuf> {
    let live = GLOBAL.lock().unwrap_or_else(PoisonError::into_inner).upgrade();
    live.map(|l| l.path.clone())
}

/// Test seam: everything `emit` is given on this thread while `f` runs (events from other threads are not seen).
#[doc(hidden)]
pub fn capture<R>(f: impl FnOnce() -> R) -> (R, Vec<Value>) {
    SINK.with(|s| *s.borrow_mut() = Some(vec![]));
    let r = f();
    let events = SINK.with(|s| s.borrow_mut().take()).unwrap_or_default();
    (r, events)
}

/// UTC `YYYYMMDD-HHMMSS` (civil-from-days, no time crate).
pub fn stamp(t: SystemTime) -> String {
    let s = t.duration_since(UNIX_EPOCH).map_or(0, |d| d.as_secs());
    let (days, rem) = ((s / 86_400) as i64, s % 86_400);
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + i64::from(m <= 2);
    format!(
        "{y:04}{m:02}{d:02}-{:02}{:02}{:02}",
        rem / 3600,
        rem % 3600 / 60,
        rem % 60
    )
}

/// The first 12 characters of a program key.
pub fn short(key: &str) -> String {
    key.chars().take(12).collect()
}

pub fn first_line(s: &str) -> String {
    s.lines().next().unwrap_or("").to_string()
}

pub fn ms_since(t: Instant) -> f64 {
    r3(t.elapsed().as_secs_f64() * 1000.0)
}

/// The UI-thread freeze detector: call once per egui frame with `busy` = "3D work is in flight". Returns the gap
/// in ms when this frame came 100 ms or more (and under a minute) after the previous one AND the previous frame
/// had work in flight (so egui was being asked to repaint). Idle gaps are normal and never reported.
#[derive(Debug, Default)]
pub struct GapWatch {
    last: Option<Instant>,
    expect: bool,
}

impl GapWatch {
    pub fn tick(&mut self, now: Instant, busy: bool) -> Option<f64> {
        let gap = self
            .last
            .filter(|_| self.expect)
            .map(|t| now.saturating_duration_since(t).as_micros() as f64 / 1000.0)
            .filter(|g| (GAP_MS..GAP_MAX_MS).contains(g));
        self.last = Some(now);
        self.expect = busy;
        gap
    }
}

/// Per-frame draw and total times, summarised once per `STATS_EVERY`.
#[derive(Debug, Default)]
pub struct FrameStats {
    since: Option<Instant>,
    draw: Vec<f64>,
    total: Vec<f64>,
}

fn pct(v: &mut [f64], q: f64) -> f64 {
    v.sort_by(f64::total_cmp);
    r3(v[((v.len() - 1) as f64 * q).round() as usize])
}

impl FrameStats {
    pub fn push(&mut self, now: Instant, draw_ms: f64, total_ms: f64) -> Option<Value> {
        let since = *self.since.get_or_insert(now);
        self.draw.push(draw_ms);
        self.total.push(total_ms);
        if now.saturating_duration_since(since) < STATS_EVERY {
            return None;
        }
        let v = json!({
            "frames": self.draw.len(),
            "draw_ms_p50": pct(&mut self.draw, 0.5),
            "draw_ms_p95": pct(&mut self.draw, 0.95),
            "total_ms_p95": pct(&mut self.total, 0.95),
        });
        self.draw.clear();
        self.total.clear();
        self.since = None;
        Some(v)
    }
}
```

`lib.rs`: add `pub mod perf_log;` before `pub mod permutation;`.

- [ ] **Step 4: Run to verify GREEN**

Run: `cargo test -p render3d --test perf_log`
Expected: PASS (10 tests). `frame_stats...`: percentiles of 101 values 1..=101: p50 index `round(100*0.5)=50` -> 51.0; p95 index 95 -> 96.0; total p95 = 192.0 as asserted. `a_gap_is_reported...`: ticks at 0 (expect becomes true), 99 (gap 99 <100 none, expect true), 199 (gap 100 from 99, `Some(100.0)`), 700 with busy=false (previous busy: gap 501, then expect=false), 5000 (none), 5050 busy true (expect was false: none), 65050 (expect true: gap 60 000 is not `< 60 000`: none; expect true), 65300 busy false (gap 250).

- [ ] **Step 5: Failing tests, the render3d hooks** — create `crates/render3d/tests/perf_hooks.rs` (helpers `common::{fixture, renderer, FLAT}` are the ones Task 5's `async_compile.rs` uses):

```rust
//! Task 9: the render3d side of the timing log, driven through the no-pool seam (`take_jobs` + `run_job`) on
//! this thread, so `perf_log::capture` sees every event.
mod common;
use common::{fixture, renderer, FLAT};
use render3d::cache::{prune_logged, PruneLimits};
use render3d::{parse, perf_log, run_job};
use serde_json::Value;
use std::time::{Duration, SystemTime};

fn none(_: &str) -> Option<Vec<u8>> {
    None
}

fn named<'a>(ev: &'a [Value], n: &str) -> Vec<&'a Value> {
    ev.iter().filter(|e| e["event"] == n).collect()
}

fn first_async_load(cache: &std::path::Path) {
    let mut r = renderer(cache, FLAT);
    r.set_async_compile(true);
    r.load_parsed(&parse(&fixture("TestNifFile_Furniture_Col_SE.nif")), &none);
    for j in r.take_jobs() {
        r.apply_compiled(run_job(j));
    }
}

#[test]
fn a_cold_async_load_logs_pending_compile_misses_and_ready() {
    let cache = tempfile::tempdir().unwrap();
    let ((), ev) = perf_log::capture(|| first_async_load(cache.path()));
    let pending = named(&ev, "shape_pending");
    let ready = named(&ev, "shape_ready");
    assert_eq!((pending.len(), ready.len()), (2, 2), "{ev:?}");
    for e in &ready {
        assert_eq!(e["ok"], true);
        assert!(e["ms_pending"].as_f64().unwrap() >= 0.0);
    }
    assert_eq!(pending[0]["showing_previous"], false);
    let compiles = named(&ev, "compile");
    assert!(!compiles.is_empty());
    for c in compiles {
        assert_eq!((c["cache"].as_str(), c["ok"].as_bool()), (Some("miss"), Some(true)), "{c}");
        assert_eq!(c["key"].as_str().unwrap().len(), 12);
        assert!(c["ms"].as_f64().unwrap() > 0.0);
        assert!(c.get("error").is_none());
    }
    // pending comes before ready, by shape
    let pos = |n: &str| ev.iter().position(|e| e["event"] == n).unwrap();
    assert!(pos("shape_pending") < pos("shape_ready"));
}

#[test]
fn a_warm_cache_logs_compile_hits_and_no_pending_shape() {
    let cache = tempfile::tempdir().unwrap();
    first_async_load(cache.path()); // fills the disk cache
    let ((), ev) = perf_log::capture(|| first_async_load(cache.path()));
    let compiles = named(&ev, "compile");
    assert!(!compiles.is_empty(), "{ev:?}");
    assert!(compiles.iter().all(|c| c["cache"] == "hit" && c["ok"] == true), "{ev:?}");
    assert!(named(&ev, "shape_pending").is_empty(), "a disk hit is applied at once");
}

#[test]
fn a_failing_program_logs_ok_false_with_the_first_error_line() {
    let cache = tempfile::tempdir().unwrap();
    let ((), ev) = perf_log::capture(|| {
        let mut r = renderer(cache.path(), &["STANDIN_FLAT"]);
        r.load(&fixture("TestNifFile_DeepGraph_SE.nif"), &none); // one shape's ps does not compile (Task 5)
    });
    let bad: Vec<_> = named(&ev, "compile").into_iter().filter(|c| c["ok"] == false).collect();
    assert_eq!(bad.len(), 1, "{ev:?}");
    let e = bad[0]["error"].as_str().unwrap();
    assert!(e.starts_with("ps:") && !e.contains('\n'), "{e}");
    assert_eq!(named(&ev, "shape_ready").iter().filter(|e| e["ok"] == false).count(), 1);
}

#[test]
fn a_prune_logs_what_it_removed() {
    let d = tempfile::tempdir().unwrap();
    std::fs::write(d.path().join(".tmp-stale"), [7u8; 5]).unwrap();
    std::fs::write(d.path().join("keep.json"), [1u8; 3]).unwrap();
    let later = SystemTime::now() + Duration::from_secs(3 * 86_400); // the files are 3 days old at `later`
    let (rep, ev) = perf_log::capture(|| prune_logged(d.path(), &PruneLimits::default(), later));
    let p = named(&ev, "prune");
    assert_eq!(p.len(), 1, "{ev:?}");
    assert_eq!(p[0]["removed"], rep.removed);
    assert_eq!(p[0]["bytes_before"], 8);
    assert_eq!(p[0]["bytes_after"], rep.bytes_after);
    assert!(p[0]["ms"].as_f64().unwrap() >= 0.0);
    assert!(rep.removed >= 1);
}
```

- [ ] **Step 6: Run to verify RED**

Run: `cargo test -p render3d --test perf_hooks`
Expected: FAIL to compile (`prune_logged` unresolved); once it compiles, the other three fail on empty event lists.

- [ ] **Step 7: render3d hooks.**

`cache.rs`: add (and make `prune_in_background`'s thread call it instead of `prune`):

```rust
/// Size of the top-level files of `dir` (what `prune` counts).
fn dir_bytes(dir: &Path) -> u64 {
    std::fs::read_dir(dir)
        .map(|rd| {
            rd.filter_map(Result::ok)
                .filter_map(|e| e.metadata().ok())
                .filter(|m| m.is_file())
                .map(|m| m.len())
                .sum()
        })
        .unwrap_or(0)
}

/// [`prune`] plus one `prune` line in the timing log.
pub fn prune_logged(dir: &Path, limits: &PruneLimits, now: SystemTime) -> PruneReport {
    let t0 = std::time::Instant::now();
    let before = dir_bytes(dir);
    let rep = prune(dir, limits, now);
    crate::perf_log::emit(
        "prune",
        serde_json::json!({
            "removed": rep.removed,
            "bytes_before": before,
            "bytes_after": rep.bytes_after,
            "ms": crate::perf_log::ms_since(t0),
        }),
    );
    rep
}
```
and in `prune_in_background` replace `prune(&dir, &PruneLimits::default(), SystemTime::now());` with `prune_logged(&dir, &PruneLimits::default(), SystemTime::now());`.

`pool.rs`: add `use crate::compile::{cached, Compiled}; use crate::perf_log; use serde_json::json; use std::time::Instant;` (merge with the existing `use crate::compile::{compile, CompiledPair, ProgramKey};`), and replace `fn pair` and `pub fn run_job` with:

```rust
/// One stage: a disk-cache hit, or a compile. The flag says which.
fn stage(job: &CompileJob, d: &Defines, target: &str) -> (Result<Compiled, String>, bool) {
    match cached(&job.src, d, target, &job.cache_dir) {
        Some(c) => (Ok(c), true),
        None => (compile(&job.src, d, target, &job.cache_dir), false),
    }
}

/// Both stages; the flag is true only when both came from the disk cache.
fn pair(job: &CompileJob) -> (Result<CompiledPair, String>, bool) {
    let (vs, vs_hit) = stage(job, &job.vs, "vs_5_0");
    let vs = match vs {
        Ok(v) => v,
        Err(e) => return (Err(format!("vs: {e}")), false),
    };
    let (ps, ps_hit) = stage(job, &job.ps, "ps_5_0");
    match ps {
        Ok(ps) => (Ok(CompiledPair { vs, ps }), vs_hit && ps_hit),
        Err(e) => (Err(format!("ps: {e}")), false),
    }
}

/// Compiles (or reads from the disk cache) both stages and logs one `compile` line. A panic inside the compiler
/// becomes an error result, so a job is never lost and a shape never stays `Pending` for ever.
pub fn run_job(job: CompileJob) -> CompileDone {
    let t0 = Instant::now();
    let (result, hit) = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| pair(&job)))
        .unwrap_or_else(|_| {
            (
                Err("internal error: the shader compiler panicked".to_string()),
                false,
            )
        });
    let mut f = json!({
        "key": perf_log::short(&job.key.0),
        "ms": perf_log::ms_since(t0),
        "cache": if hit { "hit" } else { "miss" },
        "ok": result.is_ok(),
    });
    if let Err(e) = &result {
        f["error"] = json!(perf_log::first_line(e));
    }
    perf_log::emit("compile", f);
    CompileDone {
        key: job.key,
        result: result.map(Arc::new),
    }
}
```

`lib.rs`:
- `struct Wanted`: add `since: std::time::Instant,`; in the function that builds a `Wanted` (`Renderer::wanted`, the one `begin` calls) add `since: std::time::Instant::now(),` to the literal.
- `begin`, `Lookup::Waiting` arm: before `if self.inflight.insert(...)` add
  ```rust
                perf_log::emit(
                    "shape_pending",
                    serde_json::json!({
                        "shape": id.0,
                        "key": perf_log::short(&wanted.key.0),
                        "showing_previous": self.shapes.get(&id).is_some_and(|s| s.draw.is_some()),
                    }),
                );
  ```
- `apply_compiled`, in `for id in &waiting {`: after `let Some(w) = ... else { continue; };` add `let since = w.since;`, and after the `match self.lookup(&w) { .. }` block add
  ```rust
            perf_log::emit(
                "shape_ready",
                serde_json::json!({
                    "shape": id.0,
                    "ms_pending": perf_log::ms_since(since),
                    "ok": self.shapes.get(id).is_some_and(|s| s.status == ShapeStatus::Drawing),
                }),
            );
  ```
- `lookup`, the disk-hit branch: replace
  ```rust
            let dir = &self.cache_dir;
            if let (Some(vs), Some(ps)) = (
  ```
  with
  ```rust
            let dir = &self.cache_dir;
            let t0 = std::time::Instant::now();
            if let (Some(vs), Some(ps)) = (
  ```
  and as the first statement inside that `if let { .. }` body add
  ```rust
                perf_log::emit(
                    "compile",
                    serde_json::json!({"key": perf_log::short(&w.key.0), "ms": perf_log::ms_since(t0), "cache": "hit", "ok": true}),
                );
  ```
  (`ShapeId`'s inner field is used as `id.0`; if it is not a plain integer type, use `u64::from(id.0)`.)

- [ ] **Step 8: Run to verify GREEN**

Run: `cargo test -p render3d --test perf_hooks --test perf_log` then `cargo test -p render3d`
Expected: PASS; the existing suite unchanged (no sink and no live log: `emit` is a no-op).

- [ ] **Step 9: Commit render3d**

```bash
git add crates/render3d/src/perf_log.rs crates/render3d/src/lib.rs crates/render3d/src/pool.rs crates/render3d/src/cache.rs crates/render3d/tests/perf_log.rs crates/render3d/tests/perf_hooks.rs
git commit -m "feat(render3d): preview timing log (JSON Lines writer, compile/pending/prune events)" -m "Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

- [ ] **Step 10: Failing tests, the GUI hooks** — append to `crates/gui/tests/viewport3d.rs` (the Task 8 helpers `confirmed()`, `parsed()`, `loaded()` and `use std::sync::Arc;` are above; add `use render3d::perf_log; use std::time::{Duration, Instant};` if missing):

```rust
// ---- P1-B Task 9: timing log events ----

fn frame_reply(v: &mut Viewport3d, id: u64) {
    v.handle(v3::V3Reply::Frame {
        frame: Arc::new(v3::Frame {
            width: 2,
            height: 2,
            rgba: vec![0; 16],
            id,
        }),
        timing: Default::default(),
    });
}

fn named<'a>(ev: &'a [serde_json::Value], n: &str) -> Vec<&'a serde_json::Value> {
    ev.iter().filter(|e| e["event"] == n).collect()
}

#[test]
fn ui_gap_is_logged_only_when_work_was_in_flight() {
    let (_d, mut v, cmds) = confirmed();
    let nif = Arc::new(render3d::ParsedNif::default());
    v.set_content(Some(parsed(1, &nif, &[])), false);
    v.request_frame(64, 64);
    let Ok(v3::V3Cmd::LoadParsed { id, .. }) = cmds.try_recv() else {
        panic!("LoadParsed")
    };
    assert!(v.busy());
    let t0 = Instant::now();
    let at = |ms: u64| t0 + Duration::from_millis(ms);
    let ((), ev) = perf_log::capture(|| {
        v.poll_at(at(0));
        v.poll_at(at(150)); // busy before: a freeze
        loaded(
            &mut v,
            id,
            render3d::LoadReport {
                shapes: 1,
                ..Default::default()
            },
        );
        assert!(!v.busy());
        v.poll_at(at(200)); // short gap
        v.poll_at(at(10_000)); // the previous frame found nothing in flight: idle
    });
    let gaps = named(&ev, "ui_gap");
    assert_eq!(gaps.len(), 1, "{ev:?}");
    assert_eq!(gaps[0]["gap_ms"], 150.0);
    assert_eq!(gaps[0]["panel"], "meshes");
}

#[test]
fn the_load_edit_swap_and_flip_lines_are_written_when_the_frame_that_shows_them_arrives() {
    use render3d::{EditKind, ShapeId, Variant};
    let (_d, mut v, cmds) = confirmed();
    let nif = Arc::new(render3d::ParsedNif::default());
    v.set_content(Some(parsed(1, &nif, &[])), false);
    let ((), ev) = perf_log::capture(|| {
        v.request_frame(64, 64);
        let Ok(v3::V3Cmd::LoadParsed { id, .. }) = cmds.try_recv() else {
            panic!("LoadParsed")
        };
        loaded(
            &mut v,
            id,
            render3d::LoadReport {
                shapes: 1,
                ..Default::default()
            },
        );
        v.request_frame(64, 64);
        let Ok(v3::V3Cmd::Render { id: r1, .. }) = cmds.try_recv() else {
            panic!("Render")
        };
        frame_reply(&mut v, r1);
    });
    let ff = named(&ev, "first_frame");
    assert_eq!(ff.len(), 1, "{ev:?}");
    assert!(ff[0]["ms_since_load"].as_f64().unwrap() >= 0.0);
    // Commands sent after that frame was requested.
    let r1 = v.render_sent_for_test();
    let cid = v.loaded_for_test().unwrap();
    let ((), early) = perf_log::capture(|| {
        assert!(v.set_material(ShapeId(0), Default::default()));
        assert!(v.swap_texture("textures/a.dds".into(), Arc::new(vec![1, 2, 3]), Variant::Encoded));
        assert!(v.set_texture_variant("textures/a.dds".into(), Variant::Source));
        assert!(v.set_texture_variant("textures/b.dds".into(), Variant::Source));
        // The edit's kind arrives with the thread's MaterialSet reply.
        v.handle(v3::V3Reply::MaterialSet {
            content: cid,
            id: ShapeId(0),
            result: Ok(EditKind::Permutation),
            ms: 1.0,
        });
        // A frame requested before the commands cannot show them.
        frame_reply(&mut v, r1);
    });
    assert!(named(&early, "edit").is_empty() && named(&early, "texture_swap").is_empty(), "{early:?}");
    let ((), late) = perf_log::capture(|| {
        v.request_frame(64, 64);
        let Ok(v3::V3Cmd::Render { id: r2, .. }) = cmds.try_recv() else {
            panic!("a Render for the commands")
        };
        // Refresh frames (rendered after a compile result) never answer a command.
        v.handle(v3::V3Reply::Refresh {
            frame: Arc::new(v3::Frame { width: 2, height: 2, rgba: vec![0; 16], id: 1 << 62 }),
            timing: Default::default(),
        });
        frame_reply(&mut v, r2);
    });
    let edit = named(&late, "edit");
    let swap = named(&late, "texture_swap");
    let flip = named(&late, "ab_flip");
    assert_eq!((edit.len(), swap.len(), flip.len()), (1, 1, 1), "{late:?}");
    assert_eq!((edit[0]["shape"].as_u64(), edit[0]["kind"].as_str()), (Some(0), Some("Permutation")));
    assert_eq!(swap[0]["path"], "textures/a.dds");
    assert_eq!((swap[0]["variant"].as_str(), swap[0]["bytes"].as_u64()), (Some("Encoded"), Some(3)));
    assert_eq!((flip[0]["paths"].as_u64(), flip[0]["variant"].as_str()), (Some(2), Some("Source")));
    for e in [edit[0], swap[0], flip[0]] {
        assert!(e["ms_to_frame"].as_f64().unwrap() >= 0.0, "{e}");
    }
}
```

The test uses two tiny `#[doc(hidden)]` accessors, `render_sent_for_test()` (returns `render_sent`) and `loaded_for_test()` (returns `loaded`), added in Step 12 next to `attach_test_thread`.

Also append, in the same file, the real-thread test (WARP + stand-in, modelled on Task 8's `a_texture_swap_reaches_the_render_thread_and_survives_a_reload`, same fixture setup; the render thread's events go to the live log, so this test starts one in a temp dir):

```rust
/// The render thread's lines (`gpu`, `compile`, `load`, `frame_stats` aside) reach the log file.
#[test]
fn a_real_render_thread_writes_gpu_compile_load_and_first_frame_lines() {
    use nif::material::MaterialState;
    let log_dir = tempfile::tempdir().unwrap();
    let log = perf_log::start_in(log_dir.path()).expect("a log");
    let path = log.path().to_path_buf();
    let standin = PathBuf::from(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../render3d/tests/fixtures/standin"
    ));
    let fp = render3d::locate::fingerprint(&standin, None).unwrap();
    let cache = tempfile::tempdir().unwrap();
    let mut v = Viewport3d::new(Some(fp.clone()));
    v.cache_dir = Some(cache.path().to_path_buf());
    v.on_located(Ok(Some(fp)));
    let mut material = MaterialState::builtin_default();
    material.alpha = Some(nif::props::Alpha { flags: 0x200, threshold: 0 }); // the stand-in refuses the other permutations
    v.set_content(
        Some(v3::Content::Builtin {
            key: 7,
            shape: render3d::builtin::Builtin::Sphere,
            material,
            textures: Arc::new(std::collections::HashMap::new()),
        }),
        false,
    );
    let t = Instant::now();
    loop {
        v.request_frame(64, 64);
        v.poll();
        if !v.busy() {
            break;
        }
        assert!(t.elapsed().as_secs() < 60, "the viewport did not settle");
        std::thread::yield_now();
    }
    assert!(v.frame.is_some());
    drop(log);
    // A pool worker may still hold a temporary handle for an instant: wait for `run_end`.
    let t = Instant::now();
    let text = loop {
        let text = std::fs::read_to_string(&path).unwrap();
        if text.contains("\"run_end\"") || t.elapsed().as_secs() > 10 {
            break text;
        }
        std::thread::yield_now();
    };
    let ev: Vec<serde_json::Value> = text.lines().map(|l| serde_json::from_str(l).unwrap()).collect();
    for want in ["run_start", "gpu", "compile", "load", "first_frame"] {
        assert!(ev.iter().any(|e| e["event"] == want), "no {want} line in {text}");
    }
    let load = ev.iter().find(|e| e["event"] == "load").unwrap();
    assert_eq!(load["content"], "builtin:Sphere");
    assert_eq!(load["shapes"], 1);
}
```

And in `crates/gui/tests/set_preview_model.rs` (helpers `started()`, `fake()`, `DEFAULT_CAP` exist; add `use render3d::perf_log;` if missing):

```rust
// ---- P1-B Task 9: encode lines ----

#[test]
fn an_encode_completion_is_logged_with_its_slot_mode_time_and_size() {
    let (_d, m, mut enc, slots) = started();
    let t = Instant::now();
    enc.sync(&m, slots, t, true);
    enc.pump(m.session.as_ref(), t, false);
    let k = *enc.members.keys().next().unwrap();
    let tok = enc.token_of(k);
    let ((), ev) = perf_log::capture(|| {
        enc.inject(k, tok, DEFAULT_CAP, fake());
        assert!(enc.poll());
    });
    let e: Vec<_> = ev.iter().filter(|e| e["event"] == "encode").collect();
    assert_eq!(e.len(), 1, "{ev:?}");
    assert_eq!((e[0]["ms"].as_f64(), e[0]["bytes"].as_u64(), e[0]["ok"].as_bool()), (Some(1.0), Some(8), Some(true)));
    assert_eq!(e[0]["mode"], "Encoded");
    assert!(e[0]["slot"].is_string());
}

#[test]
fn the_set_preview_tags_its_viewport_for_the_gap_watch() {
    assert_eq!(SetPreview::new(None, None, None, DEFAULT_CAP).v3.panel, "set_preview");
}
```

And in `crates/gui/tests/set_preview_ui.rs`:

```rust
#[test]
fn the_timings_section_shows_the_log_path_and_a_copy_button() {
    let log_dir = tempfile::tempdir().unwrap();
    let _log = render3d::perf_log::start_in(log_dir.path()).expect("a log");
    let (_d, mut h) = plain_rock();
    h.get_by_label("Timings").click();
    h.run();
    h.get_by_label_contains("Timing log:");
    h.get_by_label("Copy log path");
}
```

- [ ] **Step 11: Run to verify RED**

Run: `cargo test -p sdc-gui --test viewport3d ui_gap --test set_preview_model encode_completion`
Expected: FAIL to compile (`poll_at`, `panel`, `perf_log` use in gui missing); run each `--test` separately if cargo rejects several.

- [ ] **Step 12: GUI hooks.**

`crates/gui/src/main.rs`: the two lines from the top of this task, first statement of `main`.

`crates/gui/src/model/viewport3d.rs` (anchors by text):

1. Imports: add `use render3d::perf_log::{self, FrameStats, GapWatch};` and `use serde_json::json;`.

2. After `impl PartialEq for Content { .. }` add:
```rust
impl Content {
    /// What a `load` log line calls this content.
    pub fn label(&self) -> String {
        match self {
            Content::Mesh(s) => format!("mesh:{}", s.key),
            Content::Parsed { key, .. } => format!("parsed:{key}"),
            Content::Builtin { shape, .. } => format!("builtin:{shape:?}"),
        }
    }
}

/// A command whose effect shows in a later frame (timing log): sent at `at`, answered by the first `Frame` with
/// an id above `after` (the last render id sent when the command was sent).
struct Mark {
    at: Instant,
    after: u64,
    what: MarkKind,
}

enum MarkKind {
    Edit { shape: u32, kind: Option<EditKind> },
    Swap { path: String, variant: Variant, bytes: usize },
    Flip { variant: Variant, paths: usize },
}
```
(`Variant` is already imported by Task 8; `ShapeId.0` is used as `u32` below, adapt the type to `ShapeId`'s field.)

3. `pub struct Viewport3d`: add after `pub adapter: Option<(String, bool)>,`:
```rust
    /// Which panel owns this viewport, for the `ui_gap` line.
    pub panel: &'static str,
    /// The owner of the viewport has other work in flight that keeps the app repainting (set preview: encodes,
    /// a wanted scene push); counted by the gap watch.
    pub extra_busy: bool,
```
and private fields after `rx: Receiver<WorkerMsg>,`: `gap: GapWatch, marks: Vec<Mark>, load_at: Option<Instant>,`. In `Viewport3d::new`: `panel: "meshes", extra_busy: false, gap: GapWatch::default(), marks: vec![], load_at: None,`.

4. `poll`: rename the body to `pub fn poll_at(&mut self, now: Instant) -> bool` and add above it
```rust
    pub fn poll(&mut self) -> bool {
        self.poll_at(Instant::now())
    }
```
First statements of `poll_at`:
```rust
        if let Some(gap) = self.gap.tick(now, self.busy() || self.extra_busy) {
            perf_log::emit("ui_gap", json!({"gap_ms": gap, "panel": self.panel}));
        }
```
Add the two test accessors next to `attach_test_thread`:
```rust
    #[doc(hidden)]
    pub fn render_sent_for_test(&self) -> u64 {
        self.render_sent
    }
    #[doc(hidden)]
    pub fn loaded_for_test(&self) -> Option<u64> {
        self.loaded
    }
```

5. `request_frame`, `need_load` branch: in `if thread.send(cmd).is_ok() {` add `self.load_at = Some(Instant::now());` as first statement.

6. Marks: add methods in `impl Viewport3d`:
```rust
    fn mark(&mut self, what: MarkKind) {
        let after = self.render_sent;
        // One click flips many paths: they share one line.
        if let (MarkKind::Flip { variant, paths }, Some(Mark { after: a, what: MarkKind::Flip { variant: v, paths: n }, .. })) =
            (&what, self.marks.last_mut())
        {
            if *a == after && v == variant {
                *n += *paths;
                return;
            }
        }
        if self.marks.len() >= 64 {
            self.marks.remove(0);
        }
        self.marks.push(Mark { at: Instant::now(), after, what });
    }

    /// A frame with id `id` reached the model: log every command it can show.
    fn answer_marks(&mut self, id: u64) {
        let (done, keep): (Vec<Mark>, Vec<Mark>) =
            std::mem::take(&mut self.marks).into_iter().partition(|m| m.after < id);
        self.marks = keep;
        for m in done {
            let ms = perf_log::ms_since(m.at);
            match m.what {
                MarkKind::Edit { shape, kind } => perf_log::emit(
                    "edit",
                    json!({"shape": shape, "kind": kind.map_or("unknown".into(), |k| format!("{k:?}")), "ms_to_frame": ms}),
                ),
                MarkKind::Swap { path, variant, bytes } => perf_log::emit(
                    "texture_swap",
                    json!({"path": path, "variant": format!("{variant:?}"), "bytes": bytes, "ms_to_frame": ms}),
                ),
                MarkKind::Flip { variant, paths } => perf_log::emit(
                    "ab_flip",
                    json!({"paths": paths, "variant": format!("{variant:?}"), "ms_to_frame": ms}),
                ),
            }
        }
    }
```
In `set_material`, after `self.edits_sent += 1;` add `self.mark(MarkKind::Edit { shape: id.0, kind: None });`. In `send_texture_cmd` (Task 8a), at the top add
```rust
        let what = match &cmd {
            V3Cmd::SwapTexture { path, bytes, variant } => {
                MarkKind::Swap { path: path.clone(), variant: *variant, bytes: bytes.len() }
            }
            V3Cmd::SetTextureVariant { variant, .. } => MarkKind::Flip { variant: *variant, paths: 1 },
            _ => return false,
        };
```
and after the `t.send(cmd).is_err()` check (before `self.last_sent = None;`) add `self.mark(what);`.

7. `handle`:
   - `V3Reply::Ready { adapter, warp }`: replace `=> self.adapter = Some((adapter, warp)),` with
     ```rust
            V3Reply::Ready { adapter, warp } => {
                perf_log::emit(
                    "gpu",
                    json!({"adapter": adapter, "warp": warp, "cs_root": self.shaders.as_ref().map(|f| f.root.display().to_string())}),
                );
                self.adapter = Some((adapter, warp));
            }
     ```
   - `V3Reply::Frame { frame, timing }`: inside `if self.loaded.is_some() && self.pending_load.is_none() {` (after `self.timing = Some(timing);`; `frame` was moved into `self.frame`, so read `id` before: add `let fid = frame.id;` as the first line of the arm) add
     ```rust
                    if let Some(t) = self.load_at.take() {
                        perf_log::emit("first_frame", json!({"ms_since_load": perf_log::ms_since(t)}));
                    }
                    self.answer_marks(fid);
     ```
   - `V3Reply::MaterialSet { content, result, ms, .. }`: add `id,` to the pattern and in the `Ok(kind) => self.last_edit = Some((kind, ms)),` arm first fill the mark:
     ```rust
                    Ok(kind) => {
                        for m in &mut self.marks {
                            if let MarkKind::Edit { shape, kind: k @ None } = &mut m.what {
                                if *shape == id.0 {
                                    *k = Some(kind);
                                }
                            }
                        }
                        self.last_edit = Some((kind, ms));
                    }
     ```
   (`EditKind` is `Copy`; if not, `.clone()`.)

8. Render loop (Task 6 text):
   - after `let mut refresh_id: u64 = 1 << 62;` add `let mut stats = FrameStats::default();`
   - in the `if reload {` block, replace `let report = load_all(&mut r, c, &edits, &tex);` with
     ```rust
                let t0 = std::time::Instant::now();
                let report = load_all(&mut r, c, &edits, &tex);
                perf_log::emit(
                    "load",
                    json!({"content": c.label(), "shapes": report.shapes, "pending": report.pending, "build_ms": perf_log::ms_since(t0)}),
                );
     ```
   - `rebuild` closure: first statement `let t0 = std::time::Instant::now();`; before its final `Ok(())` add `perf_log::emit("device_rebuild", json!({"ms": perf_log::ms_since(t0)}));`
   - after `let timing = r.last_timing();` add
     ```rust
                if let Some(s) = stats.push(
                    std::time::Instant::now(),
                    timing.draw_ms,
                    timing.draw_ms + timing.gpu_ms + timing.readback_ms,
                ) {
                    perf_log::emit("frame_stats", s);
                }
     ```

`crates/gui/src/model/set_preview.rs`:
- `Encoder::poll`, right before `mem.state = match result {` add
  ```rust
            match &result {
                Ok((dds, ms)) => perf_log::emit(
                    "encode",
                    json!({"slot": kind.label(), "mode": format!("{mode:?}"), "ms": ms, "bytes": dds.len(), "ok": true}),
                ),
                Err(e) => perf_log::emit(
                    "encode",
                    json!({"slot": kind.label(), "mode": format!("{mode:?}"), "ok": false, "error": perf_log::first_line(e)}),
                ),
            }
  ```
  (Task 8b's alt-variant completion arm gets the same statement.) Imports: `use render3d::perf_log; use serde_json::json;`.
- `SetPreview::new`: replace `v3: Viewport3d::new(shaders),` with `v3: { let mut v = Viewport3d::new(shaders); v.panel = "set_preview"; v },`.
- `poll_at`: replace `self.v3.poll();` with
  ```rust
        self.v3.extra_busy = self.enc.busy() || self.want_scene || self.loading.is_some();
        self.v3.poll_at(now);
  ```

`crates/gui/src/ui/set_preview.rs`, `fn timings`: after the `Last material edit` block add
```rust
            if let Some(p) = render3d::perf_log::path() {
                ui.horizontal(|ui| {
                    ui.label(format!("Timing log: {}", p.display()));
                    if ui.button("Copy log path").clicked() {
                        ui.ctx().copy_text(p.display().to_string());
                    }
                });
            } else {
                ui.label("Timing log: off (SDC_PREVIEW_LOG=0)");
            }
```

- [ ] **Step 13: Run to verify GREEN**

Run: `cargo test -p sdc-gui --test viewport3d --test set_preview_model --test set_preview_ui`
Expected: PASS, the existing tests unchanged (a `Viewport3d` without a live log and outside `capture` emits to nobody).

- [ ] **Step 14: Gates**

Run: `cargo fmt --all && cargo clippy --workspace --all-targets -- -D warnings && cargo test --workspace`
Expected: all green. `Cargo.lock` unchanged (`git diff --stat Cargo.lock` empty); if it changed, stop and report. Run the sdc-gui exe once by hand, open a set preview, and confirm a `preview-timings-*.jsonl` appears in `%LOCALAPPDATA%\sdc\logs` and grows while the 3D panel works (not automated: `run_end` on exit depends on eframe returning from `run_native`).

- [ ] **Step 15: Commit**

```bash
git add crates/gui/src/main.rs crates/gui/src/model/viewport3d.rs crates/gui/src/model/set_preview.rs crates/gui/src/ui/set_preview.rs crates/gui/tests/viewport3d.rs crates/gui/tests/set_preview_model.rs crates/gui/tests/set_preview_ui.rs
git commit -m "feat(gui): preview timing log hooks (ui_gap, load, edit, swap, flip, encode, frame stats) and the log path in the Timings section" -m "Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

---

---

## Task 10: end-to-end verification on owner data (manual checklist; numbers come from the timing log)

**Files:**
- Modify: this plan file (the stitched P1-B plan), section "P1-B verification" at the end (record the results)
- No code. Owner data (`C:\_mount\moddingvol\Acherus`, the CS install) is read-only. The only things written are inside sdc's own cache directory (`%LOCALAPPDATA%\sdc\shader-cache`), the timing log directory (`%LOCALAPPDATA%\sdc\logs`) and a scratch copy of the CS `Shaders` folder under `%TEMP%`.

**Interfaces:** Consumes the whole P1-B build (Tasks 1–9). Produces the pass/fail per check. **The owner does not time anything.** The app records every timing in `%LOCALAPPDATA%\sdc\logs\preview-timings-YYYYMMDD-HHMMSS.jsonl` (one file per run, UTC stamp; Task 9). The owner does the actions below, pausing about 5 seconds between steps, and sends the newest log file back; the controller reads the numbers from it with the table at the end. Screenshots are still taken where the check is about what is on screen (the pixels), never for a duration.

- [ ] **Step 1: Prepare**
  - `cargo build -p sdc-gui --release` in `F:\ai\dds\converter-nif-p1b` (the P1-B build). The P1-A exe has no log, so it is no longer needed for the "before" numbers: the sum of the `compile` times (`cache: "miss"`) of a burst is how long the P1-A path would have frozen the UI for it.
  - Start the P1-B exe from MO2 (the Acherus instance), so `[meshes]` and the CS locate resolve. Meshes view: a v3 scan of the Acherus profile must exist. Do **not** set `SDC_PREVIEW_LOG=0`. Open a set once and look at the set preview's "Timings" section: it shows "Timing log: <path>" and a "Copy log path" button; if it says "off", stop and report.
  - Shader cache before anything (still by hand, the log only sees the prune run): `Get-ChildItem "$env:LOCALAPPDATA\sdc\shader-cache" -Recurse -File | Measure-Object Length -Sum` → file count **C0** and total MB **S0**. Note the newest and oldest `LastWriteTime`.
  - Task Manager → Details, columns "Dedicated GPU memory" and "Commit size" for `sdc-gui.exe` (memory is not in the log): **M0** at idle with the Textures view open and the panel closed.

- [ ] **Step 2: No freeze on a recompile of a big NIF (success criterion "no freezes")**
  - Pick a set whose matched NIF is a skinned armour or body piece with many shapes. First choice (01 §5.2 fuzz case): Faultier's PBR Armors and Clothes, set for `meshes\dlc01\clothes\vampire\vampirerobesfkha_1.nif`. Any skinned armour or body NIF with ≥ 6 shapes will do (`femalebody_1.nif` is the P0 stress mesh). Select the set; wait until the frame is up and every member chip is "fresh". Write down the NIF and its shape count **N** (the log does not carry the NIF path).
  - Drag in the 3D panel continuously (orbit) and, at the same time, toggle the viewport's "Community Shaders features" off and on, once. That changes `ENABLE_LL` for every shape, so every shape needs a new permutation (a burst of N compiles; in a TruePBR set a shader-type edit alone would not change the define set, `permutation::defines`). Keep dragging until the picture has settled, then stop and wait 5 seconds.
  - Expect: the old image stays on screen while the new programs compile; the model keeps following the mouse; the shape list marks the shapes that are waiting for a program (the pending marker from Task 6) and clears it as each lands; the image changes shape by shape or all at once but never goes blank. The log records `shape_pending` / `shape_ready` / `compile` / `ui_gap` / `load` for it.
  - Repeat once with the set's `glint` entry key switched on (adds the `GLINT` define for the matching shapes), then wait 5 seconds: expect only those shapes to go pending (log: fewer `shape_pending` lines than N), the rest unchanged, and the `edit` line to say `kind: "Permutation"`.
  - Pass (from the log): no `ui_gap` line between the first `shape_pending` of the burst and the last `shape_ready` of it (so no UI frame was 100 ms or more late), and no blank frame (visual).

- [ ] **Step 3: A compile failure keeps the old image and lists the error**
  - Make a scratch copy of the CS shaders (never edit the owner's): `robocopy "<CS mod>\Shaders" "$env:TEMP\sdc-cs-broken\Shaders" /E`. In the copy, add this as the first line of `Lighting.hlsl`: `#if defined(PSHADER) && defined(GLINT)` / `#error sdc verification: GLINT permutation fails` / `#endif`.
  - In the viewport panel use "Choose folder…" and pick `%TEMP%\sdc-cs-broken`. Expect: the model reloads (the edited file gives new cache keys, so everything recompiles and draws again; no shape uses GLINT yet).
  - Switch the set's `glint` key on. Expect: the glint shapes go pending, then show a red failure line naming the shader error (`GLINT permutation fails`) in the shape list; **the previous image of those shapes stays on screen** (screenshot before and after: identical pixels apart from the list), the other shapes are unaffected, and the panel does not blank. Wait 5 seconds. Switch `glint` off again: the failure clears.
  - Restore: "Choose folder…" back to the real CS `Shaders` folder, then delete `%TEMP%\sdc-cs-broken`.
  - Pass: old image kept (visual), error listed (visual), no blank frame (visual); the log has a `compile` line with `ok: false` whose `error` starts with `ps:` and mentions `GLINT permutation fails`, the matching `shape_ready` lines have `ok: false`, and no `ui_gap`.

- [ ] **Step 4: Texture hot-swap and A/B (success criterion "instant A/B")**
  - Use the set from Step 2 (or `reserve_bc` from P1-A Task 17: the mesh is small and quick). Mode toggle at its default, Encoded; chips "fresh". Wait 5 seconds between each action below.
  - **Swap, not reload:** change a member's setting that re-encodes only that member (the normal map's format, say). Expect the chip "stale" → "encoding…" → "fresh" and the new look to appear **without** the shape list flickering or the camera moving, and the Timings "Last load" unchanged (no scene push). A failed encode (give the member an invalid setting) still reloads the scene and shows the game's texture. The log records an `encode` line (`ms`) and a `texture_swap` line (`ms_to_frame`), and no new `load` line for the swap.
  - **First flip:** click "Source (faster, no compression)". Expect a re-encode of the members in Source mode (it was the old behaviour; chips go stale → fresh), the picture switching to the uncompressed look when they land. From now on A/B is in use. Log: `encode` lines with `mode: "Source"`, then `texture_swap` lines with `variant: "Source"`.
  - **Background encode of the other mode:** after the Source chips are fresh, change a member setting (invalidates both) and let it settle: expect the active mode to land first and the other mode to encode in the background afterwards (no visible change in the picture). Log: an `encode` line for the active mode, then one for the other mode, only the first followed by a `texture_swap` of the shown variant.
  - **Instant flip:** with both variants encoded, click "Encoded" and "Source" alternately 5 times, waiting about a second between clicks. Expect the picture to change on the very next frame after each click and **no chip to leave "fresh"**, no "encoding…". Log: five `ab_flip` lines, and **no `encode` line** among them. Flipping Source → Encoded → Source must give pixel-identical Source frames (screenshots of the same camera, compared by eye or by file hash).
  - **Header decision visible:** pick a texture whose Encoded format has an sRGB DX10 header and whose Source copy is untagged: the two modes should differ in brightness exactly as the game would (the converter's header decision). Visual only.
  - **Memory:** with the A/B in use on the big set, read **M_ab** (dedicated GPU, commit) from Task Manager and compare with **M1** taken before the first flip. The design budget is ~340 MB worst case for 8 × 4K BC7 twice. The log has no memory numbers.

- [ ] **Step 5: Multi-node NIF placed like NifSkope (success criterion "multi-node NIFs placed right")**
  - Pick a candidate that has nested `NiNode`s with a non-identity transform on a non-root parent. Choose one by opening it in NifSkope first (Block List): it must have ≥ 3 levels of `NiNode` above at least one shape and a non-zero translation, rotation or scale on one of those nodes. Candidates from the 01 §5.2 golden list (their node structure has not been inspected, so check each in NifSkope): `meshes\dlc01\traps\lighttrap\lighttrap01.nif`, `meshes\clutter\eyeofmagnus\eyeofmagnus01.nif`, `meshes\architecture\sovengarde\sovstairspiece01.nif`; any multi-part architecture kit piece or furniture NIF with several shapes also works. The repo fixture `TestNifFile_DeepGraph_SE.nif` (27 shapes under nested nodes) is the automated stand-in (Task 2's golden).
  - In sdc: open a set whose match list includes the candidate (or the Meshes view's 3D panel) and pick it. Take a screenshot of the model from the front with the default camera. In NifSkope render the same file from the front. Compare silhouettes: every part must sit where NifSkope puts it, with no part at the origin, no stacked or overlapping pieces that NifSkope separates, and the bounds-framed camera showing the whole model.
  - Also load it in the P1-A exe (`F:\ai\dds\converter-nif-p1\target\release\sdc-gui.exe`, if present): parts that sit under a transformed parent are misplaced there (write down which). Pass: P1-B matches NifSkope for every shape; record the NIF, shape count and the offending shapes in P1-A. Visual check; no timing.
  - A NIF with an unknown node type or a broken parent chain must open with a warning in the panel, not an error; if you have one, record it.

- [ ] **Step 6: The shader cache pruned itself**
  - Before launching the P1-B exe for this step (or after closing it), create two test artefacts in `%LOCALAPPDATA%\sdc\shader-cache`: a file `.tmp-sdc-verify` and a `deadbeef.cso` with no `.json`, both with `LastWriteTime` two days ago (`(Get-Item <f>).LastWriteTime = (Get-Date).AddDays(-2)`). Record **C0'** and **S0'** (count and MB including them).
  - Launch the exe, open any set so the 3D panel creates its renderer (the prune runs on a background thread at renderer start), wait 30 s. Re-count: **C1/S1**. Expect: both artefacts gone; the total at most 512 MB; entries touched within 90 days present. If **S0'** was already under 512 MB, no real entry may disappear except those older than 90 days (list them if any). Log: one `prune` line (`removed`, `bytes_before`, `bytes_after`, `ms`); `removed` is at least 2 and `bytes_after` ≤ 536870912.
  - Hit-touches-mtime: note the `.json` `LastWriteTime` of a program the session uses (the newest cache entry before and after rendering a shape that uses it): it moves to "now" on a cache hit. Log: `compile` lines with `cache: "hit"` (the sync disk hits are logged too).
  - Content-addressed keys: copy nothing; just record whether the entry count grew when the same CS shaders are reached through "Choose folder…" at a different path (it must not: the key ignores the root path). Log: the reload after "Choose folder…" has `compile` lines that are all `cache: "hit"`, and no `shape_pending`.
  - Pass: artefacts removed, size ≤ 512 MB, the used entry touched, no growth from a moved root.

- [ ] **Step 7: Close the exe normally and send the log**
  - Close sdc-gui with the window's close button (not Task Manager), so the log ends with a `run_end` line.
  - Send the newest `%LOCALAPPDATA%\sdc\logs\preview-timings-*.jsonl` (the set preview's "Copy log path" button gives its path; the file name stamp is UTC). If several runs were needed, send each file and say which step each covers. If a `log_capped` line is present, say so (the file stopped at 20 MB).
  - Also send: the NIF names and shape counts from Steps 2 and 5, the screenshots of Steps 3 and 5, and M0 / M1 / M_ab and C0/S0 → C0'/S0' → C1/S1 from Task Manager and PowerShell.

- [ ] **Step 8: Record** in the "P1-B verification" section at the end of this plan file, filled from the log by the controller (reading rules below), in this form:

```markdown
## P1-B verification (<date>, exe <git sha>, log file <name>)

| Check | Result | Value (from the log unless marked manual) |
|---|---|---|
| Step 2 recompile, big NIF: <nif>, N shapes | pass / fail | burst of <n> compiles, ui_gap lines in the burst: <count, max gap_ms>; first shape_pending → last shape_ready <ms>; P1-A would have blocked ≈ <sum of compile ms, cache miss> ms |
| Step 2 glint key only | pass / fail | shape_pending count <k> of N; edit kind <kind>, ms_to_frame <ms> |
| Step 3 compile failure keeps old image, error listed | pass / fail | compile ok:false error "<first line>"; no ui_gap; visual: old image kept (manual) |
| Step 4 swap on re-encode, no scene push | pass / fail | encode ms <ms>; texture_swap ms_to_frame <ms>; load lines during the swap: <0> |
| Step 4 first flip (re-encode) | pass / fail | encode (Source) ms <ms> |
| Step 4 instant flip, both variants | pass / fail | ab_flip ms_to_frame ×5: <list>; encode lines among them: <0>; flip-back byte-identical yes/no (manual) |
| Step 4 background encode of the other mode | pass / fail | encode order (active, then other) yes/no |
| Step 4 GPU/commit with both variants | — | M0, M1, M_ab MB (manual) |
| Step 4 frame cost after the flip | — | frame_stats draw_ms_p95 / total_ms_p95 <ms> |
| Step 5 multi-node NIF vs NifSkope: <nif> | pass / fail | shapes wrong in P1-A: <list> (manual) |
| Step 6 cache prune | pass / fail | prune removed <n>, bytes_before → bytes_after; C0/S0 → C0'/S0' → C1/S1 (manual); touched yes/no; moved root grew yes/no, reload compile hits <n>/<n> |
| Whole run | — | ui_gap lines outside Steps 2-3 windows: <count, max>; dropped events (run_end): <n>; device_rebuild lines: <n> |

Notes / surprises:
```

- [ ] **Step 9: Commit the record**

```bash
git add <this plan file>.md
git commit -m "docs: P1-B verification results (no-freeze recompile, instant A/B, multi-node placement, cache prune)" -m "Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

### How the controller reads the log

One JSON object per line; `t_ms` = milliseconds since the app started its log. Windows are cut by the `t_ms` of the events themselves (the 5-second pauses keep bursts apart); for a burst, `shape_pending` lines are the start and `shape_ready` lines the end. Quick filter: `Get-Content <file> | ConvertFrom-Json | Where-Object event -eq ab_flip`.

| Checklist line | Event(s) | Pass threshold |
|---|---|---|
| Step 2: no freeze on a shader change | `ui_gap` (`gap_ms`) between the first `shape_pending` and last `shape_ready` of the burst | no `ui_gap` line at all in that window (every UI frame < 100 ms late); N `shape_pending` and N `shape_ready` with `ok: true` |
| Step 2: how long the change takes to land | first `shape_pending` → last `shape_ready` (`t_ms`), `shape_ready.ms_pending`, `compile.ms` | informational; compare with the sum of `compile.ms` where `cache: "miss"` = what P1-A would have frozen for |
| Step 2: edit shows at once | `edit.ms_to_frame` for `kind: "Permutation"` | < 100 ms (the old image is redrawn while the compile runs) |
| Step 2: glint only some shapes | count of `shape_pending` in that burst, `edit.kind` | fewer than N; kind `Permutation` |
| Step 3: failure listed, old image kept | `compile` with `ok: false` and `error`; `shape_ready` with `ok: false`; `ui_gap` | error text contains `GLINT permutation fails`; other shapes' `shape_ready` ok; no `ui_gap` (the old image itself is the visual check) |
| Step 4: swap without a scene push | `encode` (`ms`, `mode`, `slot`), `texture_swap` (`ms_to_frame`), `load` | a `texture_swap` per swapped path, `ms_to_frame` < 250 ms, no `load` line between the encode and the swap |
| Step 4: first flip | `encode` with `mode: "Source"` | informational (`ms`) |
| Step 4: background encode | order of `encode` lines after the settings change | active mode first (and its `texture_swap`), the other mode afterwards without a visible `texture_swap` of the shown variant |
| Step 4: instant A/B | `ab_flip.ms_to_frame`, `encode` | each of the 5 flips < 50 ms (a frame or two at 60 Hz plus the render), and no `encode` line between the first and last flip |
| Step 4: frame cost | `frame_stats` (`draw_ms_p95`, `total_ms_p95`) | informational; `total_ms_p95` should not rise after the flip |
| Step 6: prune | `prune` | `removed` ≥ 2 (the two artefacts), `bytes_after` ≤ 536870912; `ms` informational |
| Step 6: hits and moved root | `compile` `cache: "hit"`; `shape_pending` | after "Choose folder…" to the same sources: every `compile` is `hit`, no `shape_pending` |
| Always | `ui_gap`, `run_end.dropped`, `log_capped`, `device_rebuild`, `gpu` (`adapter`, `warp`) | `ui_gap` outside the Step 2 and 3 windows is worth a look (a freeze elsewhere); `dropped` 0; `warp: false` on the owner's machine; `device_rebuild` should be absent |

## P1-B verification, run 1 (2026-10-12, exe d23063a, log file preview-timings-20261012-091950.jsonl)

| Check | Result | Value (from the log unless marked manual) |
|---|---|---|
| Step 2 recompile, big NIF (owner: the first example, `vampirerobesfkha_1.nif`) | pass (N short) | Set preview burst at 623.42 s: 2 shapes (the set showed 2), `showing_previous: true`; first `shape_pending` → last `shape_ready` 731 ms; no `ui_gap` in the burst; P1-A would have blocked ≈ 1430 ms (702 + 728, cache miss). Meshes-panel CS toggle at 465.91 s: 3 shapes, 750 ms, no `ui_gap`, P1-A ≈ 731 ms. N was below the ≥ 6 the step asks for. |
| Step 2 glint key only | inconclusive | `edit` kind `Permutation`, `ms_to_frame` 31.8 ms (later toggles 3–18 ms, memo hits); both shapes take glint, so "fewer than N" cannot show at N = 2 |
| Step 3 compile failure keeps old image, error listed | not run | no `compile` line with `ok: false` in the log; no screenshot |
| Step 4 swap on re-encode, no scene push | fail as written | 100.5 / 103.3 / 106.1 s: `encode` (Encoded) ≈ 1000 ms each, each followed by a `load` (build 5–6 ms, first frame 32–33 ms, all shader hits), no `texture_swap`. The code follows plan Task 8 (line 4540: `Delta.encode` → `want_scene` → full push); the Goal line and this step expect a swap. Owner decision. |
| Step 4 first flip (re-encode) | pass | `encode` (Source) 188 / 236 / 273 ms; `texture_swap` `ms_to_frame` 12.6 / 24.1 / 16.8 ms; no `load` |
| Step 4 instant flip, both variants | not run | only the 3 `ab_flip` lines of the first flip (12.6 / 24.1 / 16.9 ms) |
| Step 4 background encode of the other mode | not run | no settings change after the first flip |
| Step 4 GPU/commit with both variants | — | not recorded (manual) |
| Step 4 frame cost after the flip | — | `frame_stats` at 280 s: `draw_ms_p95` 0.054, `total_ms_p95` 2.235 ms (2.306 before): no rise |
| Step 5 multi-node NIF vs NifSkope: `lighttrap01.nif` | pass (manual) | owner: every mesh in the correct spot; no screenshot; P1-A comparison not done |
| Step 6 cache prune | not run | `prune` removed 0, 993,512 bytes before and after (the artefacts were not created); disk cache hits seen (84.8 s) |
| Whole run | see note | 104 `ui_gap` lines, max 2481 ms, none inside a compile burst; `dropped` 0; `warp: false` (RTX 4090); no `device_rebuild`, no `log_capped` |

Notes / surprises:
- `ui_gap` metric bug: 77 of the 104 lines follow a completion event within 60 ms of the previous frame. The watch was armed before the frame drained its replies, so the owner's pause after the last reply was logged as a freeze. Fixed in 04ac290 (armed at the end of the frame, regression test). The other 27 match the same pattern after a render reply, which the log does not record; a rerun on the fixed exe confirms or refutes.
- 466.7–484.4 s: 22 reloads of mesh:132418 at about 0.5 s intervals, build ≈ 44 ms, no compiles (memo hits); matches repeated CS-feature toggles.
- Rerun needed on the fixed exe: Steps 3, 4 (instant flip, background encode), 6, and Step 2 on a set showing ≥ 6 shapes.
