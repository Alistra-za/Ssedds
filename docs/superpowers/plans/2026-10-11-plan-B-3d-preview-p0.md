# 3D NIF Preview — Capture Tooling + P0 Renderer Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

> **Not started. `F:\ai\dds\converter` is read-only until the owner says otherwise.** The new RenderDoc scripts go in `F:\ai\dds\tools\rdc-scripts\` (B1–B2); ask the owner before writing there. Plan B depends on plan A Tasks 1–6: the `nif` crate core and `vfs`. B1–B2 (captures) can run before any Rust exists.

**Goal:** Render a selected SSE NIF in sdc-gui with Community Shaders' own `Lighting.hlsl`, compiled unchanged from the user's installed CS, with stubbed lighting. A RenderDoc capture toolchain records the real game state that P1/P2 will match.

**Architecture:** A Windows-only `render3d` crate drives D3D11 through the `windows` crate (0.62.2, already in the lock).
- Includes are expanded in Rust, then `D3DCompile` from `d3dcompiler_47.dll` (ships with Windows) compiles the result.
- Cbuffer layouts come from shader reflection, so CS updates don't break hard-coded structs.
- Geometry comes from a new `nif` layer-1 module that decodes BSTriShape vertex data.
- P0 presents by CPU readback into an egui texture. Zero-copy shared-texture import into wgpu is B9, gated on a probe.

Plan P1 (live editing) and P2 (deferred + composite parity) get their own plans after the B2 capture answers Q1–Q5 of `04` §5.3.

**Tech Stack:**
- Rust: `windows` 0.62.2 with features `Win32_Graphics_Direct3D`, `Win32_Graphics_Direct3D11`, `Win32_Graphics_Dxgi`, `Win32_Graphics_Dxgi_Common`, `Win32_Graphics_Direct3D_Fxc`;
- `sha2` and `hex` (workspace) for the shader cache key;
- `engine`'s DDS reader (read-only use of its public API);
- RenderDoc 1.43 at `F:\ai\dds\tools\RenderDoc_1.43_64` with Python 3 inside `qrenderdoc.exe`.

**Spec:** (all in `docs/superpowers/specs/nif/`; paths written `converter/crates/...` below mean `crates/...` in worktree `F:iddsnverter-nif-b`) `00-decision.md` §2, §4 and §6 (owner choices 1, 2, 4 and 5), `03-renderer-architecture.md` §5 (P0) and §6, `04-cs-shader-inputs-and-renderdoc.md` §1.2, §2, §3.2 and §5.

## Global Constraints

- **CS is ground truth.** Compile the user's installed `Shaders\Lighting.hlsl` unchanged. Never patch CS source; differences are handled in defines, stubs or bindings.
- CS target build for this owner: `C:\_mount\moddingvol\Acherus\mods\CommunityShaders_AIO-2026-10-03T06-46Z` (00 §6.1).
- Shaders root: locate it, don't store it blindly (00 §6.4).
  - Re-locate on start and on window focus.
  - A changed root or owning mod → popup: **Use new** / **Choose folder…** / **Keep old** (Keep old only if the old folder still exists).
  - A hash-only change → a one-line notice plus a background recompile.
  - Not found → only the 3D panel says "Community Shaders not found" with **Choose folder…**.
- Default mode = CS as installed (its enabled features); a toggle turns CS features off (00 §6.2). For P0 that toggle only removes feature defines.
- LE meshes (BSVER 83): no rendering. Show "LE mesh: optimise it for SSE, e.g. with Cathedral Assets Optimizer." (00 §6.5).
- Dynamic Cubemaps parity is deferred (00 §6.3). The panel shows "Dynamic Cubemaps: off in preview".
- Realtime edits are cbuffer writes; shader-type/define changes recompile on a worker thread while the old permutation keeps drawing (03 §5 P1). P0 implements the cache and worker; the edit UI is P1.
- Perf target: 60 fps at 1600×900, 50k-tri skinned armour, 8× 4K BC7 (03 §5); render only on change. P0 measures and records, it doesn't optimise.
- Converter crates are not modified, except the workspace `members` line and sdc-gui's `Cargo.toml` + the Meshes view from plan A.
- Licence: GPL-3.0-or-later. CS shaders are read from the user's install at run time and are never copied into the repo or the binary.

## Review Focus

1. **The CS folder disappears or is renamed while sdc is open** (MO2 update renames `CommunityShaders_AIO-<date>`). Expected: on focus the popup appears; nothing crashes; the 3D panel keeps the last frame until the user answers. Pinned in B3 (`renamed_root_triggers_popup`, `missing_root_is_not_found`).
2. **An `#include` inside an `#if 0` block, or one that names a file that does not exist in this CS build.** Expected: the expander leaves the directive alone and the compiler decides; no hard failure on inactive code. Pinned in B4 (`missing_include_left_for_compiler`).
3. **A permutation that fails to compile** (unknown define combination, CS bug). Expected: the error text shows in the panel, the previous permutation keeps drawing, and other meshes still render. Pinned in B5 (`compile_error_is_reported_not_fatal`).
4. **A NIF with no texture-set slots, empty slot strings, or a texture missing everywhere.** Expected: the shape still renders with 1×1 defaults (white diffuse, flat normal `(0.5,0.5,1)`, black glow) and the missing paths are listed. Pinned in B7 (`missing_textures_use_defaults`).
5. **No usable D3D11 hardware device** (RDP session, broken driver). Expected: fall back to WARP with a "software renderer" notice, or show "3D preview unavailable" if WARP fails. The rest of the GUI is unaffected. Pinned in B6 (`warp_fallback_renders`).

---

## File Structure

```
F:\ai\dds\tools\rdc-scripts\p04_stageA_enumerate.py   (new; B1)
F:\ai\dds\tools\rdc-scripts\p04_stageB_state.py       (new; B2)
F:\ai\dds\tools\rdc-scripts\p04_common.py             (new; shared helpers)
converter/crates/nif/src/geom/mod.rs                  (new; B5a layer 1)
converter/crates/nif/src/geom/vertex_desc.rs
converter/crates/nif/src/geom/bstrishape.rs
converter/crates/nif/src/geom/skin_partition.rs
converter/crates/nif/tests/geom.rs
converter/crates/render3d/
  Cargo.toml
  src/lib.rs            Renderer facade (message-shaped API, 03 §5)
  src/locate.rs         CS shaders root: locate, fingerprint, change detection
  src/includes.rs       #include expander with search paths
  src/permutation.rs    NIF material → technique/defines (04 §1.2)
  src/compile.rs        D3DCompile + reflection + disk cache
  src/cbuffer.rs        reflection-driven cbuffer writer (set by field name)
  src/device.rs         D3D11 device (hardware → WARP), offscreen targets, readback
  src/mesh.rs           vertex/index buffers matching the VS input signature
  src/textures.rs       DDS → SRV, slot→register map (04 §2.3), 1×1 defaults
  src/scene.rs          stub engine state (camera, one dir light, DALC ambient)
  src/draw.rs           one draw per shape, forward permutation
  tests/*.rs
converter/crates/gui/src/ui/viewport3d.rs              (new; B8)
converter/crates/gui/src/model/viewport3d.rs           (new; B8)
```

---

### Task B1: RenderDoc stage A — enumerate Lighting draws

**Files:**
- Create: `F:\ai\dds\tools\rdc-scripts\p04_common.py`, `p04_stageA_enumerate.py`
- Output: `F:\ai\dds\tools\rdc-out\p04\<capture-stem>\drawmap.json`, `stageA.log`

**Interfaces:**
- Produces: `drawmap.json` = list of `{event, name, psId, vsId, runtimeHex, definesMarker, inferred: [defines], outputs: n, srvs: [[name, bind]], vsInputs: [semantic], t0ResourceId}` consumed by B2 and by P1/P2 planning.

- [ ] **Step 1: Write `p04_common.py`**

```python
# p04_common.py: shared helpers for the p04 RenderDoc stages (run inside qrenderdoc --python).
import os, json, re, traceback
import renderdoc as rd

def open_capture(path):
    rd.InitialiseReplay(rd.GlobalEnvironment(), [])
    cap = rd.OpenCaptureFile()
    st = cap.OpenFile(path, "", None)
    if st != rd.ResultCode.Succeeded:
        raise RuntimeError("open %s: %s" % (path, st))
    st, ctrl = cap.OpenCapture(rd.ReplayOptions(), None)
    if st != rd.ResultCode.Succeeded:
        raise RuntimeError("replay: %s" % st)
    return cap, ctrl

def walk(actions, out, markers, sf):
    """Flatten actions. Remember the last 'Draw: CS Lighting::<hex>' and 'Defines:' markers seen before each draw
    (04 §5.2: the marker group sits before the draw, not around it)."""
    for a in actions:
        name = a.GetName(sf)
        if name.startswith("Draw: CS Lighting::"):
            markers["runtime"] = name.split("::", 1)[1].strip()
        elif name.startswith("Defines:"):
            markers["defines"] = name[len("Defines:"):].strip()
        if a.flags & rd.ActionFlags.Drawcall:
            out.append((a, dict(markers)))
        walk(a.children, out, markers, sf)

def infer_defines(refl_ps, vs_inputs, n_outputs):
    names = {r.name: r.fixedBindNumber for r in refl_ps.readOnlyResources}
    d = []
    if n_outputs == 8: d.append("DEFERRED")
    if "TexSpecularSampler" in names: d.append("MODELSPACENORMALS")
    if "TexRMAOSSampler" in names: d.append("TRUE_PBR")
    if "TexLayerSampler" in names: d.append("MULTI_LAYER_PARALLAX")
    if "TexTintSampler" in names or "TexDetailSampler" in names: d.append("FACEGEN")
    if "TexEnvSampler" in names: d.append("ENVMAP|MLP|EYE")
    if any(s.startswith("BLENDWEIGHT") for s in vs_inputs): d.append("SKINNED")
    return d

def out_dir(capture):
    d = os.path.join(r"F:\ai\dds\tools\rdc-out\p04", os.path.splitext(os.path.basename(capture))[0])
    os.makedirs(d, exist_ok=True)
    return d
```

- [ ] **Step 2: Write `p04_stageA_enumerate.py`**

```python
# Stage A: list every draw whose pixel shader is a CS Lighting permutation, with markers and inferred defines.
# Run: qrenderdoc.exe --python p04_stageA_enumerate.py   (set P04_CAPTURE to the .rdc path first)
import os, sys, json, traceback
sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import renderdoc as rd
from p04_common import open_capture, walk, infer_defines, out_dir

CAP = os.environ["P04_CAPTURE"]
OUT = out_dir(CAP)
log = open(os.path.join(OUT, "stageA.log"), "w")
def p(*a): print(*a, file=log); log.flush()
try:
    cap, ctrl = open_capture(CAP)
    sf = ctrl.GetStructuredFile()
    draws = []
    walk(ctrl.GetRootActions(), draws, {}, sf)
    p("draws", len(draws))
    rows = []
    for a, m in draws:
        if "runtime" not in m:
            continue
        ctrl.SetFrameEvent(a.eventId, False)
        s = ctrl.GetPipelineState()
        ps = s.GetShaderReflection(rd.ShaderStage.Pixel)
        vs = s.GetShaderReflection(rd.ShaderStage.Vertex)
        if ps is None or vs is None:
            continue
        vs_inputs = [sig.semanticIdxName for sig in vs.inputSignature]
        outs = [o for o in s.GetOutputTargets() if o.resource != rd.ResourceId.Null()]
        ro = s.GetReadOnlyResources(rd.ShaderStage.Pixel)
        t0 = next((str(r.descriptor.resource) for r in ro if r.access.index == 0), None)
        rows.append({"event": a.eventId, "name": a.GetName(sf), "psId": str(s.GetShader(rd.ShaderStage.Pixel)),
                     "vsId": str(s.GetShader(rd.ShaderStage.Vertex)), "runtimeHex": m.get("runtime"),
                     "definesMarker": m.get("defines"), "inferred": infer_defines(ps, vs_inputs, len(outs)),
                     "outputs": len(outs), "srvs": [[r.name, r.fixedBindNumber] for r in ps.readOnlyResources],
                     "vsInputs": vs_inputs, "t0ResourceId": t0})
    json.dump(rows, open(os.path.join(OUT, "drawmap.json"), "w"), indent=1)
    p("lighting draws", len(rows))
    ctrl.Shutdown(); cap.Shutdown()
except Exception:
    p(traceback.format_exc())
p("done"); log.close()
```

- [ ] **Step 3: Run against the existing S8 capture (expect markers absent, inferred defines present)**

Run (PowerShell):
```
$env:P04_CAPTURE='G:\Acherus\overwrite\SKSE\Plugins\CommunityShaders\Captures\Skyrim_AE_1.6.1170_frame34102.rdc'
& 'F:\ai\dds\tools\RenderDoc_1.43_64\qrenderdoc.exe' --python F:\ai\dds\tools\rdc-scripts\p04_stageA_enumerate.py
```
Expected:
- `stageA.log` ends `done`.
- `drawmap.json` exists. It may be **empty** if the S8 capture was taken without Frame Annotations, because there were no `Draw: CS Lighting` markers.
- If empty, add a fallback: when no runtime markers exist, keep draws whose PS reflection has `TexColorSampler` at bind 0. Re-run; expect ≥ 2 rows including the two S8 draws (events listed in `rdc-out\stage1.log`).

- [ ] **Step 4: Commit** (tools is not a git repo: copy the scripts' SHA-256 into `rdc-out\p04\README.md` with the date instead)

---

### Task B2: RenderDoc stage B — formats, samplers, cbuffers per draw (answers Q1–Q5)

**Files:**
- Create: `F:\ai\dds\tools\rdc-scripts\p04_stageB_state.py`
- Output: `rdc-out\p04\<capture>\state_<event>.json` and `rdc-out\p04\<capture>\answers.md`

**Interfaces:**
- Consumes: `drawmap.json` (B1).
- Produces: for each event, `{srvs: [{bind, name, resourceId, format, srgb, w, h, mips, type}], samplers: [{bind, filter, addressU, addressV, mipBias, maxAniso}], ps_cbuffers: {name: {var: value}}, vs_cbuffers: {...}, rts: [format], depth: format, blend: [...], cull}`.

- [ ] **Step 1: Write the script**

```python
# Stage B: per Lighting draw, dump SRV formats (sRGB?), samplers, every cbuffer, RT formats and fixed-function state.
import os, sys, json, traceback
sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import renderdoc as rd
from p04_common import open_capture, out_dir

CAP = os.environ["P04_CAPTURE"]
OUT = out_dir(CAP)
log = open(os.path.join(OUT, "stageB.log"), "w")
def p(*a): print(*a, file=log); log.flush()

def var_value(v):
    if v.members:
        return {m.name: var_value(m) for m in v.members}
    n = v.rows * v.columns
    return list(v.value.f32v[:n]) if v.type == rd.VarType.Float else list(v.value.u32v[:n])

def cbuffers(ctrl, s, stage):
    refl = s.GetShaderReflection(stage)
    out = {}
    if refl is None:
        return out
    pipe = s.GetGraphicsPipelineObject()
    shader = s.GetShader(stage)
    entry = s.GetShaderEntryPoint(stage)
    for i, cb in enumerate(refl.constantBlocks):
        bind = s.GetConstantBlock(stage, i, 0)
        vs = ctrl.GetCBufferVariableContents(pipe, shader, stage, entry, i, bind.descriptor.resource,
                                             bind.descriptor.byteOffset, bind.descriptor.byteSize)
        out["%s(b%d)" % (cb.name, cb.fixedBindNumber)] = {v.name: var_value(v) for v in vs}
    return out

try:
    rows = json.load(open(os.path.join(OUT, "drawmap.json")))
    cap, ctrl = open_capture(CAP)
    texinfo = {str(t.resourceId): t for t in ctrl.GetTextures()}
    for r in rows:
        ctrl.SetFrameEvent(r["event"], False)
        s = ctrl.GetPipelineState()
        refl = s.GetShaderReflection(rd.ShaderStage.Pixel)
        names = {x.fixedBindNumber: x.name for x in refl.readOnlyResources}
        srvs = []
        for d in s.GetReadOnlyResources(rd.ShaderStage.Pixel):
            rid = str(d.descriptor.resource)
            t = texinfo.get(rid)
            fmt = d.descriptor.format.Name() if d.descriptor.format.Name() else (t.format.Name() if t else "?")
            srvs.append({"bind": d.access.index, "name": names.get(d.access.index), "resourceId": rid,
                         "format": fmt, "srgb": "SRGB" in fmt.upper(),
                         "w": t.width if t else 0, "h": t.height if t else 0, "mips": t.mips if t else 0,
                         "type": str(t.type) if t else "buffer"})
        samplers = []
        for d in s.GetSamplers(rd.ShaderStage.Pixel):
            sm = d.sampler
            samplers.append({"bind": d.access.index, "filter": str(sm.filter), "addressU": str(sm.addressU),
                             "addressV": str(sm.addressV), "mipBias": sm.mipBias, "maxAniso": sm.maxAnisotropy})
        state = {"event": r["event"], "srvs": srvs, "samplers": samplers,
                 "ps_cbuffers": cbuffers(ctrl, s, rd.ShaderStage.Pixel),
                 "vs_cbuffers": cbuffers(ctrl, s, rd.ShaderStage.Vertex),
                 "rts": [texinfo[str(o.resource)].format.Name() for o in s.GetOutputTargets() if str(o.resource) in texinfo],
                 "depth": (texinfo[str(s.GetDepthTarget().resource)].format.Name()
                           if str(s.GetDepthTarget().resource) in texinfo else None)}
        json.dump(state, open(os.path.join(OUT, "state_%d.json" % r["event"]), "w"), indent=1)
        p("event", r["event"], "srvs", [(x["bind"], x["format"]) for x in srvs])
    ctrl.Shutdown(); cap.Shutdown()
except Exception:
    p(traceback.format_exc())
p("done"); log.close()
```

RenderDoc's Python API names drift between versions. If an attribute fails on 1.43, check `renderdoc.pyi` in the RenderDoc folder or the S8 scripts (`s8_stage2.py` already uses `GetCBufferVariableContents`). Fix the name; keep the output schema.

- [ ] **Step 2: Run on the S8 capture**

Same command as B1 with `p04_stageB_state.py`.
Expected:
- one `state_<event>.json` per row;
- the S8 `_m` draw shows t5 `BC7_UNORM` with `srgb: false`, matching `stage1.log`. That proves the script reads view formats correctly.

- [ ] **Step 3: Owner capture session (owner runs the game; follow `04` §5.0–§5.1 exactly)**

The owner takes the three captures (golden / golden + 1 point light / as installed), using the CS build in Global Constraints with Frame Annotations on. Run B1 and B2 on each capture.

- [ ] **Step 4: Write `answers.md`**

For Q1 (sRGB per slot), Q2 (ambient), Q3 (dir light space), Q4 (runtime bits vs NIF flags) and Q5 (specular colour/strength packing), write one table each. Put the event number and JSON field next to every value. This file is the input for the P1 plan.

---

### Task B3: `render3d` crate + CS shaders root locator

**Files:**
- Modify: `converter/Cargo.toml` (member `crates/render3d`)
- Create: `crates/render3d/Cargo.toml`, `src/lib.rs`, `src/locate.rs`, `tests/locate.rs`

**Interfaces:**
- Produces:
  - `render3d::locate::Fingerprint { pub root: PathBuf, pub owner_mod: Option<String>, pub hash: String /*sha256 hex of sorted (relpath, bytes) of *.hlsl + *.hlsli*/ }`.
  - `locate::fingerprint(root: &Path, owner_mod: Option<String>) -> io::Result<Fingerprint>`.
  - `locate::find_in_mods(origins: &[(String /*mod name*/, PathBuf /*root*/, i32 /*priority*/)]) -> Option<(String, PathBuf)>`: highest-priority origin whose `Shaders/Lighting.hlsl` exists. Plan A's origin list feeds it.
  - `locate::Change { Same, Updated /*hash only*/, Moved { old: Fingerprint, new: Fingerprint }, NotFound { old: Option<Fingerprint> } }`.
  - `locate::compare(saved: Option<&Fingerprint>, found: Option<Fingerprint>) -> Change`.

- [ ] **Step 1: Write the failing tests**

```rust
// crates/render3d/tests/locate.rs
use render3d::locate::*;
use std::fs;

fn cs(root: &std::path::Path, body: &str) {
    fs::create_dir_all(root.join("Shaders/Common")).unwrap();
    fs::write(root.join("Shaders/Lighting.hlsl"), body).unwrap();
    fs::write(root.join("Shaders/Common/Color.hlsli"), "// c").unwrap();
}

#[test]
fn hash_changes_with_content_only() {
    let d = tempfile::tempdir().unwrap();
    cs(d.path(), "a");
    let a = fingerprint(&d.path().join("Shaders"), Some("CS".into())).unwrap();
    let b = fingerprint(&d.path().join("Shaders"), Some("CS".into())).unwrap();
    assert_eq!(a.hash, b.hash);
    fs::write(d.path().join("Shaders/Lighting.hlsl"), "b").unwrap();
    let c = fingerprint(&d.path().join("Shaders"), Some("CS".into())).unwrap();
    assert_ne!(a.hash, c.hash);
    assert!(matches!(compare(Some(&a), Some(c)), Change::Updated));
}

#[test]
fn renamed_root_triggers_popup() {
    let d = tempfile::tempdir().unwrap();
    let old = d.path().join("CommunityShaders_AIO-2026-10-03T06-46Z");
    cs(&old, "a");
    let saved = fingerprint(&old.join("Shaders"), Some("CommunityShaders_AIO-2026-10-03T06-46Z".into())).unwrap();
    let new = d.path().join("CommunityShaders_AIO-2026-11-01T00-00Z");
    fs::rename(&old, &new).unwrap();
    let found = find_in_mods(&[("CommunityShaders_AIO-2026-11-01T00-00Z".into(), new.clone(), 5)]).unwrap();
    let fp = fingerprint(&found.1.join("Shaders"), Some(found.0)).unwrap();
    assert!(matches!(compare(Some(&saved), Some(fp)), Change::Moved { .. }));
}

#[test]
fn missing_root_is_not_found() {
    let d = tempfile::tempdir().unwrap();
    cs(d.path(), "a");
    let saved = fingerprint(&d.path().join("Shaders"), None).unwrap();
    assert!(find_in_mods(&[("X".into(), d.path().join("nope"), 1)]).is_none());
    assert!(matches!(compare(Some(&saved), None), Change::NotFound { old: Some(_) }));
}

#[test]
fn highest_priority_provider_wins() {
    let d = tempfile::tempdir().unwrap();
    cs(&d.path().join("A"), "a");
    cs(&d.path().join("B"), "b");
    let got = find_in_mods(&[("A".into(), d.path().join("A"), 1), ("B".into(), d.path().join("B"), 9)]).unwrap();
    assert_eq!(got.0, "B");
}
```

- [ ] **Step 2: Run to verify failure**

Run: `cargo test -p render3d --test locate`
Expected: compile error.

- [ ] **Step 3: Implement**

```rust
// crates/render3d/src/locate.rs
//! Where the user's Community Shaders lives, and whether it changed (00-decision §6.4).
use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Fingerprint {
    pub root: PathBuf,
    pub owner_mod: Option<String>,
    pub hash: String,
}

pub fn fingerprint(root: &Path, owner_mod: Option<String>) -> std::io::Result<Fingerprint> {
    let mut files: Vec<PathBuf> = walkdir::WalkDir::new(root)
        .into_iter()
        .filter_map(Result::ok)
        .filter(|e| e.file_type().is_file())
        .map(|e| e.into_path())
        .filter(|p| p.extension().is_some_and(|x| x.eq_ignore_ascii_case("hlsl") || x.eq_ignore_ascii_case("hlsli")))
        .collect();
    files.sort();
    let mut h = Sha256::new();
    for f in &files {
        h.update(f.strip_prefix(root).unwrap().to_string_lossy().to_ascii_lowercase().as_bytes());
        h.update([0]);
        h.update(std::fs::read(f)?);
    }
    Ok(Fingerprint { root: root.to_path_buf(), owner_mod, hash: hex::encode(h.finalize()) })
}

pub fn find_in_mods(origins: &[(String, PathBuf, i32)]) -> Option<(String, PathBuf)> {
    origins
        .iter()
        .filter(|(_, root, _)| root.join("Shaders").join("Lighting.hlsl").is_file())
        .max_by_key(|(_, _, prio)| *prio)
        .map(|(n, r, _)| (n.clone(), r.clone()))
}

#[derive(Debug, Clone, PartialEq)]
pub enum Change {
    Same,
    Updated,
    Moved { old: Fingerprint, new: Fingerprint },
    NotFound { old: Option<Fingerprint> },
}

pub fn compare(saved: Option<&Fingerprint>, found: Option<Fingerprint>) -> Change {
    match (saved, found) {
        (_, None) => Change::NotFound { old: saved.cloned() },
        (None, Some(new)) => Change::Moved { old: Fingerprint { root: PathBuf::new(), owner_mod: None, hash: String::new() }, new },
        (Some(old), Some(new)) if old.root == new.root && old.owner_mod == new.owner_mod => {
            if old.hash == new.hash { Change::Same } else { Change::Updated }
        }
        (Some(old), Some(new)) => Change::Moved { old: old.clone(), new },
    }
}
```

`Cargo.toml` deps: `sha2`, `hex`, `walkdir`, `serde` (workspace), `thiserror`, plus `windows` (features above). Dev-deps: `tempfile`. `lib.rs`: `pub mod locate;` (other modules are added by their tasks). `#![cfg(windows)]` goes on the D3D modules only.

- [ ] **Step 4: Run to verify pass**

Run: `cargo test -p render3d --test locate`
Expected: 4 PASS.

- [ ] **Step 5: Commit**

```bash
git add Cargo.toml Cargo.lock crates/render3d
git commit -m "feat(render3d): locate the user's CS shaders and detect moves/updates"
```

---

### Task B4: `#include` expander

**Files:**
- Create: `crates/render3d/src/includes.rs`, `crates/render3d/tests/includes.rs`

**Interfaces:**
- Produces: `includes::expand(file: &Path, search: &[PathBuf]) -> io::Result<Expanded>`, where `Expanded { pub text: String, pub files: Vec<PathBuf> }`.
  - Each included file's text is spliced in with `#line 1 "<path>"` before it and `#line <n+1> "<includer>"` after it, so compiler errors point at real files.
  - Search order: the includer's directory, then `search` in order (the Shaders root first).
  - A missing include is left as the original directive (the compiler errors only if it is in an active branch).
  - Recursion depth is capped at 64.
  - Include guards are left to the compiler (CS uses `#ifndef` guards; it has no `#pragma once`, verified 2026-10-11: 0 files).

- [ ] **Step 1: Write the failing tests**

```rust
// crates/render3d/tests/includes.rs
use render3d::includes::expand;
use std::fs;

#[test]
fn expands_relative_to_root_and_includer() {
    let d = tempfile::tempdir().unwrap();
    let r = d.path();
    fs::create_dir_all(r.join("Common")).unwrap();
    fs::write(r.join("Lighting.hlsl"), "#include \"Common/A.hlsli\"\nfloat main_marker;\n").unwrap();
    fs::write(r.join("Common/A.hlsli"), "#include \"Common/B.hlsli\"\nfloat a;\n").unwrap();
    fs::write(r.join("Common/B.hlsli"), "float b;\n").unwrap();
    let e = expand(&r.join("Lighting.hlsl"), &[r.to_path_buf()]).unwrap();
    let ia = e.text.find("float a;").unwrap();
    let ib = e.text.find("float b;").unwrap();
    let im = e.text.find("float main_marker;").unwrap();
    assert!(ib < ia && ia < im);
    assert_eq!(e.files.len(), 3);
    assert!(e.text.contains("#line 1 \""));
}

#[test]
fn missing_include_left_for_compiler() {
    let d = tempfile::tempdir().unwrap();
    fs::write(d.path().join("x.hlsl"), "#if 0\n#include \"Nope/Gone.hlsli\"\n#endif\nfloat x;\n").unwrap();
    let e = expand(&d.path().join("x.hlsl"), &[d.path().to_path_buf()]).unwrap();
    assert!(e.text.contains("#include \"Nope/Gone.hlsli\""));
}

#[test]
fn include_cycle_is_capped() {
    let d = tempfile::tempdir().unwrap();
    fs::write(d.path().join("a.hlsli"), "#include \"a.hlsli\"\n").unwrap();
    assert!(expand(&d.path().join("a.hlsli"), &[d.path().to_path_buf()]).is_err());
}
```

- [ ] **Step 2: Run to verify failure**

Run: `cargo test -p render3d --test includes`
Expected: compile error.

- [ ] **Step 3: Implement**

```rust
// crates/render3d/src/includes.rs
//! Textual #include expansion so D3DCompile sees one buffer; CS includes are root-relative ("Common/x.hlsli").
use std::path::{Path, PathBuf};

pub struct Expanded {
    pub text: String,
    pub files: Vec<PathBuf>,
}

fn target(line: &str) -> Option<&str> {
    let rest = line.trim_start().strip_prefix("#include")?.trim();
    let rest = rest.strip_prefix('"').or_else(|| rest.strip_prefix('<'))?;
    rest.split(['"', '>']).next()
}

fn go(file: &Path, search: &[PathBuf], depth: u32, out: &mut Expanded) -> std::io::Result<()> {
    if depth > 64 {
        return Err(std::io::Error::other(format!("include depth > 64 at {}", file.display())));
    }
    let text = std::fs::read_to_string(file)?;
    out.files.push(file.to_path_buf());
    let fname = file.display().to_string().replace('\\', "/");
    for (n, line) in text.lines().enumerate() {
        if let Some(t) = target(line) {
            let dirs = file.parent().into_iter().map(Path::to_path_buf).chain(search.iter().cloned());
            if let Some(found) = dirs.map(|d| d.join(t)).find(|p| p.is_file()) {
                out.text.push_str(&format!("#line 1 \"{}\"\n", found.display().to_string().replace('\\', "/")));
                go(&found, search, depth + 1, out)?;
                out.text.push_str(&format!("#line {} \"{fname}\"\n", n + 2));
                continue;
            }
        }
        out.text.push_str(line);
        out.text.push('\n');
    }
    Ok(())
}

pub fn expand(file: &Path, search: &[PathBuf]) -> std::io::Result<Expanded> {
    let mut out = Expanded { text: String::new(), files: vec![] };
    go(file, search, 0, &mut out)?;
    Ok(out)
}
```

- [ ] **Step 4: Run to verify pass**

Run: `cargo test -p render3d --test includes`
Expected: 3 PASS.

- [ ] **Step 5: Commit**

```bash
git add crates/render3d
git commit -m "feat(render3d): root-relative #include expander with #line mapping"
```

---

### Task B5: Permutations, D3DCompile, reflection, cache

**Files:**
- Create: `crates/render3d/src/permutation.rs`, `src/compile.rs`, `src/cbuffer.rs`, `tests/compile.rs`, `tests/fixtures/tiny.hlsl`

**Interfaces:**
- Consumes: `includes::expand`, `nif::props::Lighting`, `nif::props::Alpha`, `nif::shape_head::ShapeHead`.
- Produces:
  - `permutation::Material { shader_type: u32, slsf1: u32, slsf2: u32, skinned: bool, alpha_test: bool, has_vertex_colors: bool }` with `Material::from_shape(&ShapeHead, &Lighting, Option<Alpha>) -> Material`.
  - `permutation::Stage { Vertex, Pixel }` (`#[derive(Clone, Copy, PartialEq)]`).
  - `permutation::Defines(pub Vec<(String, String)>)` (`#[derive(Clone, Debug, Default, PartialEq)]`) with `permutation::defines(m: &Material, stage: Stage, features: &[&str], deferred: bool) -> Defines`. This implements the 04 §1.2 table:
    - `SKINNED` ← skinned;
    - `MODELSPACENORMALS` ← SLSF1 bit 12;
    - `VC` (VS only) ← SLSF2 bit 5 and vertex colours;
    - `TRUE_PBR` (PS only) ← SLSF2 bit 23;
    - `DO_ALPHA_TEST` ← alpha test;
    - `PROJECTED_UV` ← SLSF1 bit 23;
    - `ANISO_LIGHTING` ← SLSF2 bit 21 (`GLINT` with TRUE_PBR);
    - technique from shader type: 1→`ENVMAP`, 4→`FACEGEN`, 5→`FACEGEN_RGB_TINT`, 6→`HAIR`, 3→`PARALLAX`, 11→`MULTI_LAYER_PARALLAX`+`ENVMAP`, 16→`EYE`, 2→none (Glowmap is runtime);
    - PS: no technique for 2; VS: technique cleared for 2/3/4/5/6/13/14/15;
    - always `PSHADER`/`VSHADER`;
    - every name in `features` added verbatim (CS feature defines from 04 §1.2 notes);
    - `DEFERRED` when `deferred` (false in P0).
  - `permutation::runtime_bits(m: &Material) -> u32`: the stripped bits for cb4 `PixelShaderDescriptor`. Specular = bit 9 ← SLSF1 bit 0; Soft = bit 10 ← SLSF2 bit 25; Rim = bit 11 ← SLSF2 bit 26; Back = bit 12 ← SLSF2 bit 27; Glowmap technique 2 → `2 << 24`.
  - `compile::Compiled { pub bytecode: Vec<u8>, pub reflection: Reflection }`.
  - `compile::Reflection { pub cbuffers: Vec<CbLayout>, pub srvs: Vec<(String, u32, SrvKind)>, pub samplers: Vec<(String, u32)>, pub inputs: Vec<(String /*semantic*/, u32 /*index*/, u32 /*component mask*/)> }`, with `compile::SrvKind { Tex2D, Tex2DArray, Cube, Buffer, Structured }` from `D3D11_SHADER_INPUT_BIND_DESC::Type`/`Dimension` (`TexEnvSampler` t4 is a `TextureCube`; LLF t35–t37 are structured buffers when features are on).
  - `compile::CbLayout { pub name: String, pub slot: u32, pub size: u32, pub vars: Vec<(String, u32 /*offset*/, u32 /*size*/)> }`.
  - `compile::compile(file: &Path, root: &Path, defines: &Defines, target: &str /*"vs_5_0"|"ps_5_0"*/, cache_dir: &Path, root_hash: &str) -> Result<Compiled, String>`. The cache key is `sha256(root_hash, file name, sorted defines, target, flags)` and the cached file is `<key>.cso` + `<key>.json` (reflection).
  - `cbuffer::CbWriter::new(&CbLayout) -> CbWriter`; `CbWriter::set(&mut self, var: &str, data: &[f32]) -> bool` (false if the var is not in this build; the caller logs it); `CbWriter::bytes(&self) -> &[u8]`.

- [ ] **Step 1: Write the failing tests**

```rust
// crates/render3d/tests/compile.rs
use render3d::compile::compile;
use render3d::permutation::{defines, runtime_bits, Material, Stage};

fn m(shader_type: u32, slsf1: u32, slsf2: u32, skinned: bool) -> Material {
    Material { shader_type, slsf1, slsf2, skinned, alpha_test: false, has_vertex_colors: false }
}
fn names(d: &render3d::permutation::Defines) -> Vec<&str> { d.0.iter().map(|x| x.0.as_str()).collect() }

#[test]
fn define_table() {
    let d = defines(&m(5, 1 << 12, 0, true), Stage::Pixel, &["CS_SKIN"], false);
    for want in ["PSHADER", "SKINNED", "MODELSPACENORMALS", "FACEGEN_RGB_TINT", "CS_SKIN"] {
        assert!(names(&d).contains(&want), "{want} in {:?}", names(&d));
    }
    let glow_ps = defines(&m(2, 0, 0, false), Stage::Pixel, &[], false);
    assert!(!names(&glow_ps).iter().any(|n| n.contains("GLOW")));
    let mlp = defines(&m(11, 0, 0, false), Stage::Pixel, &[], false);
    assert!(names(&mlp).contains(&"MULTI_LAYER_PARALLAX") && names(&mlp).contains(&"ENVMAP"));
    let pbr = defines(&m(0, 0, 1 << 23, false), Stage::Pixel, &[], false);
    assert!(names(&pbr).contains(&"TRUE_PBR"));
    let pbr_vs = defines(&m(0, 0, 1 << 23, false), Stage::Vertex, &[], false);
    assert!(!names(&pbr_vs).contains(&"TRUE_PBR"));
    let skin_vs = defines(&m(5, 0, 0, true), Stage::Vertex, &[], false);
    assert!(!names(&skin_vs).contains(&"FACEGEN_RGB_TINT"));
}

#[test]
fn runtime_bits_table() {
    let b = runtime_bits(&m(2, 1, (1 << 25) | (1 << 27), false));
    assert_eq!(b, (1 << 9) | (1 << 10) | (1 << 12) | (2 << 24));
}

#[test]
fn tiny_shader_compiles_and_reflects() {
    let dir = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures");
    let cache = tempfile::tempdir().unwrap();
    let d = render3d::permutation::Defines(vec![("PSHADER".into(), "1".into())]);
    let c = compile(std::path::Path::new(&format!("{dir}/tiny.hlsl")), std::path::Path::new(dir), &d, "ps_5_0", cache.path(), "h").unwrap();
    let cb = &c.reflection.cbuffers[0];
    assert_eq!(cb.name, "PerMaterial");
    assert!(cb.vars.iter().any(|v| v.0 == "Tint" && v.1 == 16));
    use render3d::compile::SrvKind;
    assert_eq!(c.reflection.srvs, vec![("TexColor".to_string(), 0, SrvKind::Tex2D), ("TexEnv".to_string(), 4, SrvKind::Cube)]);
    let again = compile(std::path::Path::new(&format!("{dir}/tiny.hlsl")), std::path::Path::new(dir), &d, "ps_5_0", cache.path(), "h").unwrap();
    assert_eq!(again.bytecode, c.bytecode);
    assert_eq!(std::fs::read_dir(cache.path()).unwrap().count(), 2);
}

#[test]
fn compile_error_is_reported_not_fatal() {
    let d = tempfile::tempdir().unwrap();
    std::fs::write(d.path().join("bad.hlsl"), "float4 main() : SV_Target { return undefined_symbol; }").unwrap();
    let e = compile(&d.path().join("bad.hlsl"), d.path(), &Default::default(), "ps_5_0", d.path(), "h").unwrap_err();
    assert!(e.contains("undefined_symbol"), "{e}");
}

#[test]
fn cbuffer_writer_sets_by_name() {
    let layout = render3d::compile::CbLayout { name: "PerMaterial".into(), slot: 1, size: 32,
        vars: vec![("Scale".into(), 0, 4), ("Tint".into(), 16, 12)] };
    let mut w = render3d::cbuffer::CbWriter::new(&layout);
    assert!(w.set("Tint", &[1.0, 0.5, 0.25]));
    assert!(!w.set("NotInThisBuild", &[1.0]));
    assert_eq!(&w.bytes()[16..20], &1.0f32.to_le_bytes());
    assert_eq!(&w.bytes()[24..28], &0.25f32.to_le_bytes());
}

/// Opt-in: compiles the user's installed CS Lighting.hlsl (Global Constraints path).
/// `SDC_CS_SHADERS=...\Shaders cargo test -p render3d -- --ignored installed_cs`
#[test]
#[ignore]
fn installed_cs() {
    let root = std::path::PathBuf::from(std::env::var("SDC_CS_SHADERS").unwrap());
    let cache = tempfile::tempdir().unwrap();
    for (st, skinned, target, stage) in [(0, true, "ps_5_0", Stage::Pixel), (5, true, "ps_5_0", Stage::Pixel),
                                         (0, true, "vs_5_0", Stage::Vertex), (1, false, "ps_5_0", Stage::Pixel)] {
        let d = defines(&m(st, 1 << 12, 0, skinned), stage, &[], false);
        let c = compile(&root.join("Lighting.hlsl"), &root, &d, target, cache.path(), "x").unwrap_or_else(|e| panic!("{st}: {e}"));
        assert!(!c.reflection.cbuffers.is_empty());
    }
}
```

```hlsl
// crates/render3d/tests/fixtures/tiny.hlsl
cbuffer PerMaterial : register(b1) { float Scale; float3 Pad0; float3 Tint; float Pad1; };
Texture2D<float4> TexColor : register(t0);
TextureCube<float4> TexEnv : register(t4);
SamplerState S : register(s0);
float4 main(float4 pos : SV_Position, float2 uv : TEXCOORD0) : SV_Target { return TexColor.Sample(S, uv) * float4(Tint * Scale, 1) + TexEnv.Sample(S, float3(uv, 1)); }
```

- [ ] **Step 2: Run to verify failure**

Run: `cargo test -p render3d --test compile`
Expected: compile error.

- [ ] **Step 3: Implement `permutation.rs` (pure Rust, the table above), `cbuffer.rs`, and `compile.rs`**

`compile.rs` core:

```rust
// crates/render3d/src/compile.rs
use crate::includes::expand;
use crate::permutation::Defines;
use sha2::{Digest, Sha256};
use std::ffi::CString;
use std::path::Path;
use windows::core::PCSTR;
use windows::Win32::Graphics::Direct3D::Fxc::{D3DCompile, D3DCOMPILE_OPTIMIZATION_LEVEL3};
use windows::Win32::Graphics::Direct3D::{ID3DBlob, D3D_SHADER_MACRO};
use windows::Win32::Graphics::Direct3D11::{ID3D11ShaderReflection, D3D11_SHADER_DESC, D3D11_SHADER_BUFFER_DESC,
    D3D11_SHADER_VARIABLE_DESC, D3D11_SHADER_INPUT_BIND_DESC, D3D11_SIGNATURE_PARAMETER_DESC};
use windows::Win32::Graphics::Direct3D::Fxc::D3DReflect;

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct CbLayout { pub name: String, pub slot: u32, pub size: u32, pub vars: Vec<(String, u32, u32)> }
#[derive(Debug, Clone, PartialEq, Default, serde::Serialize, serde::Deserialize)]
pub struct Reflection {
    pub cbuffers: Vec<CbLayout>,
    pub srvs: Vec<(String, u32, SrvKind)>,
    pub samplers: Vec<(String, u32)>,
    pub inputs: Vec<(String, u32, u32)>,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum SrvKind { Tex2D, Tex2DArray, Cube, Buffer, Structured }
pub struct Compiled { pub bytecode: Vec<u8>, pub reflection: Reflection }

const FLAGS: u32 = D3DCOMPILE_OPTIMIZATION_LEVEL3;

fn key(file: &Path, d: &Defines, target: &str, root_hash: &str) -> String {
    let mut sorted = d.0.clone();
    sorted.sort();
    let mut h = Sha256::new();
    h.update(root_hash);
    h.update(file.file_name().unwrap().to_string_lossy().as_bytes());
    for (k, v) in &sorted { h.update(format!("{k}={v};")); }
    h.update(target);
    h.update(FLAGS.to_le_bytes());
    hex::encode(h.finalize())
}

fn blob_bytes(b: &ID3DBlob) -> Vec<u8> {
    unsafe { std::slice::from_raw_parts(b.GetBufferPointer() as *const u8, b.GetBufferSize()).to_vec() }
}

pub fn compile(file: &Path, root: &Path, d: &Defines, target: &str, cache_dir: &Path, root_hash: &str) -> Result<Compiled, String> {
    let k = key(file, d, target, root_hash);
    let (cso, js) = (cache_dir.join(format!("{k}.cso")), cache_dir.join(format!("{k}.json")));
    if let (Ok(b), Ok(j)) = (std::fs::read(&cso), std::fs::read_to_string(&js)) {
        if let Ok(r) = serde_json::from_str(&j) {
            return Ok(Compiled { bytecode: b, reflection: r });
        }
    }
    let src = expand(file, &[root.to_path_buf()]).map_err(|e| e.to_string())?;
    let names: Vec<(CString, CString)> = d.0.iter().map(|(k, v)| (CString::new(k.as_str()).unwrap(), CString::new(v.as_str()).unwrap())).collect();
    let mut macros: Vec<D3D_SHADER_MACRO> = names.iter().map(|(k, v)| D3D_SHADER_MACRO { Name: PCSTR(k.as_ptr() as _), Definition: PCSTR(v.as_ptr() as _) }).collect();
    macros.push(D3D_SHADER_MACRO::default());
    let src_name = CString::new(file.display().to_string()).unwrap();
    let target_c = CString::new(target).unwrap();
    let (mut code, mut errs) = (None, None);
    let hr = unsafe {
        D3DCompile(src.text.as_ptr() as _, src.text.len(), PCSTR(src_name.as_ptr() as _), Some(macros.as_ptr()), None,
                   PCSTR(b"main\0".as_ptr()), PCSTR(target_c.as_ptr() as _), FLAGS, 0, &mut code, Some(&mut errs))
    };
    if hr.is_err() {
        let msg = errs.map(|e| String::from_utf8_lossy(&blob_bytes(&e)).into_owned()).unwrap_or_else(|| hr.unwrap_err().to_string());
        return Err(msg);
    }
    let bytecode = blob_bytes(&code.unwrap());
    let reflection = reflect(&bytecode)?;
    std::fs::create_dir_all(cache_dir).map_err(|e| e.to_string())?;
    let _ = std::fs::write(&cso, &bytecode);
    let _ = std::fs::write(&js, serde_json::to_string(&reflection).unwrap());
    Ok(Compiled { bytecode, reflection })
}
```

`reflect(&[u8]) -> Result<Reflection, String>` uses `D3DReflect` into `ID3D11ShaderReflection`:
- `GetDesc` → `ConstantBuffers`, `BoundResources`, `InputParameters`.
- Per cbuffer: `GetConstantBufferByIndex(i).GetDesc()` and `GetVariableByIndex(j).GetDesc()` → `(Name, StartOffset, Size)`, plus the slot from `GetResourceBindingDescByName(cb name).BindPoint`.
- Bound resources: `Type == D3D_SIT_TEXTURE` → srvs, with `Dimension` mapped to `SrvKind` (TEXTURECUBE → Cube, TEXTURE2DARRAY → Tex2DArray, BUFFER → Buffer, else Tex2D); `D3D_SIT_STRUCTURED` → `SrvKind::Structured`; `D3D_SIT_SAMPLER` → samplers.
- Inputs: `GetInputParameterDesc(i)` → `(SemanticName, SemanticIndex, Mask)`.

Names are `PCSTR` → `to_string()`. The executor writes it against the windows 0.62.2 signatures (check `windows-0.62.2/src/Windows/Win32/Graphics/Direct3D11/mod.rs` in the cargo registry for exact method names). The `tiny_shader_compiles_and_reflects` test pins the behaviour.

- [ ] **Step 4: Run to verify pass**

Run: `cargo test -p render3d --test compile`
Expected: 5 PASS. Then:
```
$env:SDC_CS_SHADERS='C:\_mount\moddingvol\Acherus\mods\CommunityShaders_AIO-2026-10-03T06-46Z\Shaders'
cargo test -p render3d --release -- --ignored installed_cs --nocapture
```
Expected: PASS. 00 §6.1 measured fxc at 107–391 ms per permutation; D3DCompile should be in the same range. Print the times.

- [ ] **Step 5: Commit**

```bash
git add crates/render3d
git commit -m "feat(render3d): CS permutation defines, D3DCompile with reflection and disk cache"
```

---

### Task B5a: `nif` layer 1 — BSTriShape geometry

**Files:**
- Create: `crates/nif/src/geom/mod.rs`, `vertex_desc.rs`, `bstrishape.rs`, `skin_partition.rs`, `crates/nif/tests/geom.rs`
- Modify: `crates/nif/src/lib.rs` (`pub mod geom;`)

**Interfaces:**
- Consumes: `Header`, `Cursor`, `ShapeHead` (plan A Tasks 2, 5).
- Produces:
  - `geom::VertexDesc(pub u64)` with `stride() -> usize` (low nibble × 4) and `has(attr: Attr) -> bool`. Attribute flags are the bits at 44+ per nif.xml 2077-2105: Vertex 0x1, UVs 0x2, UV2 0x4, Normals 0x8, Tangents 0x10, Colors 0x20, Skinned 0x40, LandData 0x80, EyeData 0x100, FullPrecision 0x400.
  - `geom::Mesh { pub positions: Vec<[f32;3]>, pub uvs: Vec<[f32;2]>, pub normals: Vec<[f32;3]>, pub tangents: Vec<[f32;3]>, pub bitangents: Vec<[f32;3]>, pub colors: Vec<[u8;4]>, pub bone_weights: Vec<[f32;4]>, pub bone_indices: Vec<[u8;4]>, pub eye: Vec<f32>, pub triangles: Vec<[u16;3]>, pub transform: [[f32;4];3] }`.
  - `geom::read_shape(h: &Header, b: &[u8], block: usize) -> Result<Mesh, NifError>`:
    - `BSTriShape` / `BSSubIndexTriShape` / `BSDynamicTriShape` read inline data;
    - a skinned SSE shape with `DataSize == 0` reads its vertex data from the `NiSkinPartition` reached through `Skin → NiSkinInstance/BSDismemberSkinInstance → SkinPartition` (nif.xml 2142-2172);
    - `BSDynamicTriShape` positions come from its trailing `Vertices` (float4) array;
    - LE `NiTriShape` → `Err(NotSkyrim)` is never reached because callers skip `le_mesh` (00 §6.5).
  - SSE vertex layout per 01 Recommendation: pos f32×3 (always full precision at BSVER 100), bitangent.x f32, uv f16×2, normal u8×3 + bitangent.y u8, tangent u8×3 + bitangent.z u8, colour u8×4, skin weights f16×4 + indices u8×4, eye f32. A packed byte decodes as `b / 255 * 2 - 1`.

- [ ] **Step 1: Write the failing tests**

```rust
// crates/nif/tests/geom.rs
use nif::geom::read_shape;
use nif::header::Header;
use nif::scan::scan;

fn read(f: &str) -> Vec<u8> { std::fs::read(format!("{}/tests/fixtures/{f}", env!("CARGO_MANIFEST_DIR"))).unwrap() }

#[test]
fn se_fixture_shapes_decode_consistently() {
    for f in ["TestNifFile_Furniture_Col_SE.nif", "TestNifFile_Skinned_SE.nif", "TestNifFile_Skinned_Dynamic_SE.nif"] {
        let b = read(f);
        let h = Header::parse(&b).unwrap();
        for row in scan(&b).unwrap().shapes.iter().filter(|r| r.head.vertex_desc.is_some()) {
            let m = read_shape(&h, &b, row.block as usize).unwrap_or_else(|e| panic!("{f} block {}: {e}", row.block));
            let n = m.positions.len();
            assert!(n > 0, "{f}");
            assert!(m.positions.iter().flatten().all(|v| v.is_finite()), "{f}");
            assert!(m.triangles.iter().flatten().all(|&i| (i as usize) < n), "{f}");
            if !m.normals.is_empty() {
                let len = |v: &[f32; 3]| (v[0] * v[0] + v[1] * v[1] + v[2] * v[2]).sqrt();
                let ok = m.normals.iter().filter(|v| (len(v) - 1.0).abs() < 0.05).count();
                assert!(ok * 10 >= n * 9, "{f}: normals mostly unit length");
            }
            if row.head.skinned {
                assert_eq!(m.bone_weights.len(), n, "{f}");
            }
        }
    }
}

#[test]
fn vertex_desc_stride_and_flags() {
    use nif::geom::{Attr, VertexDesc};
    let d = VertexDesc(0x0000_B000_0000_0000 | 0x7); // stride 7*4; attribute flags at bits 44+: 0xB = Vertex|UVs|Normals
    assert_eq!(d.stride(), 28);
    assert!(d.has(Attr::Vertex) && d.has(Attr::Uvs) && d.has(Attr::Normals));
    assert!(!d.has(Attr::Tangents));
}
```

The flag bit position in `vertex_desc_stride_and_flags` follows nif.xml 2077-2105 (`VertexAttribute` at bits 44–55, `>> 44`).

- [ ] **Step 2: Run to verify failure** — `cargo test -p nif --test geom` → compile error.
- [ ] **Step 3: Implement** per the interface above. Read nif.xml `BSVertexDataSSE` (2128-2141) and nifly `Geometry.cpp:449-560` while writing.
- [ ] **Step 4: Run to verify pass** — `cargo test -p nif` → all PASS.
- [ ] **Step 5: Commit** — `git commit -m "feat(nif): layer-1 BSTriShape/skin-partition geometry decode"`

---

### Task B6: D3D11 device, offscreen target, readback (hardware → WARP)

**Files:**
- Create: `crates/render3d/src/device.rs`, `tests/device.rs`

**Interfaces:**
- Produces:
  - `device::Gpu { pub device: ID3D11Device, pub ctx: ID3D11DeviceContext, pub warp: bool }`, with `Gpu::new(prefer_luid: Option<[u8; 8]>) -> Result<Gpu, String>`. Also `Gpu::new_warp() -> Result<Gpu, String>`, which forces WARP (used by the fallback test and by `new` as its last resort). It tries hardware on the adapter whose LUID matches (from DXGI enumeration), then any hardware adapter, then WARP.
  - `device::Target::new(gpu, w, h) -> Target`: RGBA16F colour + D32 depth; P0 forward writes `SV_Target0` only.
  - `Target::clear(&self, gpu, rgba: [f32; 4])` and `Target::readback_rgba8(&self, gpu) -> Vec<u8>`: tonemap is the identity clamp in P0, and the conversion is done by a fullscreen copy into an RGBA8 staging texture.

- [ ] **Step 1: Write the failing tests**

```rust
// crates/render3d/tests/device.rs
use render3d::device::{Gpu, Target};

#[test]
fn clear_and_readback() {
    let gpu = Gpu::new(None).unwrap();
    let t = Target::new(&gpu, 4, 2);
    t.clear(&gpu, [1.0, 0.5, 0.0, 1.0]);
    let px = t.readback_rgba8(&gpu);
    assert_eq!(px.len(), 4 * 2 * 4);
    assert_eq!(&px[0..4], &[255, 128, 0, 255]);
}

#[test]
fn warp_fallback_renders() {
    let gpu = render3d::device::Gpu::new_warp().unwrap();
    assert!(gpu.warp);
    let t = Target::new(&gpu, 2, 2);
    t.clear(&gpu, [0.0, 1.0, 0.0, 1.0]);
    assert_eq!(&t.readback_rgba8(&gpu)[0..4], &[0, 255, 0, 255]);
}
```

- [ ] **Step 2: Run to verify failure** — `cargo test -p render3d --test device` → compile error.
- [ ] **Step 3: Implement** with `D3D11CreateDevice`:
  - try `D3D_DRIVER_TYPE_UNKNOWN` with the matched `IDXGIAdapter1`, then `HARDWARE`, then `WARP`;
  - feature level 11_0;
  - `D3D11_CREATE_DEVICE_BGRA_SUPPORT`.

  Readback flow: copy RGBA16F into an RGBA16F staging texture, `Map`, convert half → u8 with `half` (workspace dep) and round-to-nearest. 0.5 → 128 matches the test.
- [ ] **Step 4: Run to verify pass** — `cargo test -p render3d --test device` → 2 PASS.
- [ ] **Step 5: Commit** — `git commit -m "feat(render3d): D3D11 device with WARP fallback, offscreen target, readback"`

---

### Task B7: Mesh, textures, stub scene, forward draw

**Files:**
- Create: `crates/render3d/src/mesh.rs`, `textures.rs`, `scene.rs`, `draw.rs`, `src/lib.rs` (facade), `tests/draw.rs`

**Interfaces:**
- Consumes: B3–B6, `nif::geom::Mesh`, `nif::scan::ShapeRow`, `vfs` (texture bytes), `engine`'s DDS reader for header/mips (the executor finds the public reader used by `crates/engine/tests/dds_read.rs` and uses it read-only).
- Produces:
  - `textures::register_for_slot(shader_type: u32, pbr: bool, msn: bool, slot: usize) -> Option<u32>`. Tables from 04 §2.3:

    | NIF slot | Vanilla | TruePBR |
    |---|---|---|
    | 0 | t0 | t0 |
    | 1 | t1 | t1 |
    | 2 | t6 (glow, type 2) / t12 (types 4, 5 `_sk`) | t6 |
    | 3 | t3 (parallax 3) / t4 (facegen 4) | t4 |
    | 4 | t4 (types 1, 11, 16) | — |
    | 5 | t5 | t5 |
    | 6 | t8 (type 11) / t3 (facegen 4) | t9 |
    | 7 | t2 (when MSN) / t9 (backlight) | t12 |

    Marked [Inference] in 04. The B2 `answers.md` confirms or corrects it, and a correction changes only this function and its test.
  - `textures::default_for_register(reg) -> [u8; 4]` (colour) and `textures::default_srv(gpu, kind: SrvKind, rgba: [u8; 4])`: 1×1 2D, 1×1×6 cube or a 1-element zeroed buffer/structured buffer, so every reflected SRV slot gets a resource of the right dimension. Colours: t0 white, t1 `(128,128,255,255)`, t5 black (vanilla) / `(255,0,255,255)` (RMAOS: rough 1, metal 0, AO 1, spec 1), t6 black, t14 white, all others black.
  - `scene::Scene { pub camera: Camera, pub dir_light: DirLight, pub ambient6: [[f32;3];6] }` with `Scene::neutral()`: interior-like, one light from the upper front-left, ambient from six grey levels per 04 §3.2.
  - `scene::Scene::write(&self, w: &mut CbWriter)`. It sets the names `World`, `ViewProj`, `EyePosition`, `DirLightDirection`, `DirLightColor`, `DirectionalAmbient` (3×4 built from the six colours, 04 §3.2 formula), `TexcoordOffset`. Unknown names are logged once per build (CS version drift, 03 §6).
  - `Renderer` facade (`lib.rs`), message-shaped so it can later move to a sidecar:
    - `Renderer::new(shaders: Fingerprint, cache_dir: PathBuf) -> Result<Renderer, String>`;
    - `Renderer::load(&mut self, nif_bytes: &[u8], textures: &dyn Fn(&str) -> Option<Vec<u8>>) -> LoadReport`, where `LoadReport { le_mesh: bool, shapes: usize, missing_textures: Vec<String>, compile_errors: Vec<String> }`;
    - `Renderer::render(&mut self, scene: &Scene, w: u32, h: u32) -> Vec<u8>` (RGBA8).
    - `Renderer::bounds(&self) -> Option<([f32; 3] /*centre*/, f32 /*radius*/)>`: computed in `load` from every shape's `positions × transform`. `Scene::neutral()` has `camera: Camera::Auto`, so `render` places the camera at `centre + radius × 2.5 × normalize(0.3, -1, 0.4)` looking at the centre (Skyrim is Z-up). `Camera::Orbit { .. }` from the GUI overrides it.

- [ ] **Step 1: Write the failing tests**

```rust
// crates/render3d/tests/draw.rs
use render3d::scene::Scene;
use render3d::textures::{default_for_register, register_for_slot};

#[test]
fn slot_register_tables() {
    assert_eq!(register_for_slot(0, false, false, 0), Some(0));
    assert_eq!(register_for_slot(2, false, false, 2), Some(6));
    assert_eq!(register_for_slot(5, false, true, 2), Some(12));
    assert_eq!(register_for_slot(5, false, true, 7), Some(2));
    assert_eq!(register_for_slot(1, false, false, 4), Some(4));
    assert_eq!(register_for_slot(11, false, false, 6), Some(8));
    assert_eq!(register_for_slot(0, true, false, 5), Some(5));
    assert_eq!(register_for_slot(0, true, false, 6), Some(9));
    assert_eq!(register_for_slot(0, true, false, 7), Some(12));
    assert_eq!(default_for_register(1), [128, 128, 255, 255]);
}

fn shaders() -> Option<std::path::PathBuf> { std::env::var("SDC_CS_SHADERS").ok().map(Into::into) }

/// Needs the installed CS: `SDC_CS_SHADERS=... cargo test -p render3d -- --ignored`
#[test]
#[ignore]
fn renders_fixture_nonblack_with_defaults() {
    let root = shaders().unwrap();
    let fp = render3d::locate::fingerprint(&root, None).unwrap();
    let cache = tempfile::tempdir().unwrap();
    let mut r = render3d::Renderer::new(fp, cache.path().to_path_buf()).unwrap();
    let nif = std::fs::read(concat!(env!("CARGO_MANIFEST_DIR"), "/../nif/tests/fixtures/TestNifFile_Furniture_Col_SE.nif")).unwrap();
    let rep = r.load(&nif, &|_| None);
    assert!(rep.compile_errors.is_empty(), "{:?}", rep.compile_errors);
    assert!(rep.shapes > 0);
    let px = r.render(&Scene::neutral(), 256, 256);
    let lit = px.chunks(4).filter(|p| p[0] as u32 + p[1] as u32 + p[2] as u32 > 30).count();
    assert!(lit > 256 * 256 / 50, "object should cover >2% of the frame and be lit, got {lit} px");
}

#[test]
#[ignore]
fn missing_textures_use_defaults() {
    let root = shaders().unwrap();
    let fp = render3d::locate::fingerprint(&root, None).unwrap();
    let cache = tempfile::tempdir().unwrap();
    let mut r = render3d::Renderer::new(fp, cache.path().to_path_buf()).unwrap();
    let nif = std::fs::read(concat!(env!("CARGO_MANIFEST_DIR"), "/../nif/tests/fixtures/TestNifFile_Skinned_SE.nif")).unwrap();
    let rep = r.load(&nif, &|_| None);
    assert!(!rep.missing_textures.is_empty());
    assert!(rep.compile_errors.is_empty(), "{:?}", rep.compile_errors);
}

#[test]
#[ignore]
fn le_mesh_is_skipped() {
    let root = shaders().unwrap();
    let mut r = render3d::Renderer::new(render3d::locate::fingerprint(&root, None).unwrap(), tempfile::tempdir().unwrap().keep()).unwrap();
    let nif = std::fs::read(concat!(env!("CARGO_MANIFEST_DIR"), "/../nif/tests/fixtures/TestNifFile_Animated_LE.nif")).unwrap();
    let rep = r.load(&nif, &|_| None);
    assert!(rep.le_mesh);
    assert_eq!(rep.shapes, 0);
}
```

- [ ] **Step 2: Run to verify failure** — `cargo test -p render3d --test draw` → compile error.
- [ ] **Step 3: Implement**
  - `mesh.rs`: build an input layout from `Reflection.inputs`. For each VS input semantic, create a vertex stream from `Mesh`:
    - POSITION0 = float4 (xyz + tangent.x, per 04 §2.1);
    - TEXCOORD0 = float2;
    - NORMAL0 = R8G8B8A8_UNORM (normal·0.5+0.5, w = bitangent.y packed);
    - BINORMAL0 = R8G8B8A8_UNORM (bitangent, w = bitangent.z packed);
    - COLOR0 = R8G8B8A8_UNORM;
    - BLENDWEIGHT0 = float4;
    - BLENDINDICES0 = R8G8B8A8_UINT ×3 per 04 Q13 (`765.01` scaling note; P0 uses bind pose with identity bones so indices only need to be in range);
    - TEXCOORD2 = float (eye).

    Which NIF array feeds BINORMAL0 is Q13. P0 uses NIF *bitangents* for BINORMAL0 and *tangents* for POSITION.w/NORMAL.w/BINORMAL.w. Put it in one function `pack_tbn()` with a comment pointing to B2 `answers.md`.
  - `textures.rs`: DDS → `ID3D11Texture2D` with the file's DXGI format, plus the SRV. P0 uses the file format as-is (Q1 decides `_SRGB` views in P1). Defaults are 1×1 RGBA8. Missing paths are collected.
  - `draw.rs`: per shape, with `le_mesh` skipped entirely:
    1. `defines(.., deferred = false)`;
    2. compile VS+PS (B5);
    3. CbWriters for every reflected cbuffer, zero-initialised, then `Scene::write`;
    4. `PerMaterial` from the NIF params (`TexcoordOffset` = uv offset/scale, `SpecularColor` = spec colour × strength with gloss in w, `EmitColor`);
    5. cb4 `PixelShaderDescriptor` = `runtime_bits`;
    6. bind SRVs by `register_for_slot`, `default_srv(kind, default_for_register(reg))` for every other reflected SRV (kind from `Reflection.srvs`, so cube slots get a cube), a white 1×1 for t14 (shadow mask);
    7. one linear-wrap sampler per reflected sampler;
    8. draw indexed.

    Bones (cb9/cb10) are identity 3×4 for all 240.
- [ ] **Step 4: Run** — `cargo test -p render3d` (unit) → PASS. Then with `SDC_CS_SHADERS` set: `cargo test -p render3d --release -- --ignored` → 3 PASS. If `renders_fixture_nonblack_with_defaults` renders black, check cbuffer names against reflection with RenderDoc on sdc's own device (03 §5 P2 step 5). Attach a capture of sdc's draw to the report instead of guessing.
- [ ] **Step 5: Commit** — `git commit -m "feat(render3d): forward CS Lighting draw with stub scene and default textures"`

---

### Task B8: sdc-gui 3D viewport in the Meshes view

**Files:**
- Modify: `crates/gui/Cargo.toml` (`render3d = { path = "../render3d" }`)
- Create: `crates/gui/src/model/viewport3d.rs`, `crates/gui/src/ui/viewport3d.rs`, `crates/gui/tests/viewport3d.rs`
- Modify: `crates/gui/src/ui/meshes.rs` (plan A Task 12): a "3D" tab in the right column; `crates/gui/src/app.rs`: on window focus, run the locate check; `%APPDATA%\sdc\gui.toml` gains a `[render3d] shaders = <Fingerprint>`.

**Interfaces:**
- Consumes: `render3d::{Renderer, locate, scene::Scene}`, `modscan` texture resolution (plan A Task 11) to fetch texture bytes for slots (loose read or `Bsa::read`).
- Produces:
  - `Viewport3d { state: V3State, orbit: Orbit, last_frame: Option<egui::TextureHandle>, report: Option<LoadReport>, popup: Option<locate::Change>, features_on: bool }`.
  - `V3State { NotFound, Ready, Rendering, Error(String) }`.
  - Popup texts (verbatim):
    - "Community Shaders moved/updated: {old} → {new}. Use the new location?" with [Use new] [Choose folder…] [Keep old] (Keep old only if the old root exists);
    - notice "Community Shaders updated — recompiling shaders…";
    - panel text "Community Shaders not found" with [Choose folder…];
    - LE: "LE mesh: optimise it for SSE, e.g. with Cathedral Assets Optimizer.";
    - toggle label "Community Shaders features" (on by default);
    - footer "Dynamic Cubemaps: off in preview".

- [ ] **Step 1: Write the failing UI tests**

```rust
// crates/gui/tests/viewport3d.rs
use egui_kittest::{kittest::Queryable, Harness};
use sdc_gui::model::viewport3d::{V3State, Viewport3d};

#[test]
fn not_found_offers_choose_folder() {
    let mut v = Viewport3d::with_state(V3State::NotFound);
    let mut h = Harness::new_ui(|ui| sdc_gui::ui::viewport3d::show(ui, &mut v));
    h.run();
    h.get_by_label("Community Shaders not found");
    h.get_by_label("Choose folder…");
}

#[test]
fn moved_popup_has_three_choices_when_old_exists() {
    let d = tempfile::tempdir().unwrap();
    let old = render3d::locate::Fingerprint { root: d.path().to_path_buf(), owner_mod: Some("CS_old".into()), hash: "a".into() };
    let new = render3d::locate::Fingerprint { root: d.path().join("n"), owner_mod: Some("CS_new".into()), hash: "b".into() };
    let mut v = Viewport3d::with_state(V3State::Ready);
    v.popup = Some(render3d::locate::Change::Moved { old, new });
    let mut h = Harness::new_ui(|ui| sdc_gui::ui::viewport3d::show(ui, &mut v));
    h.run();
    h.get_by_label("Use new");
    h.get_by_label("Choose folder…");
    h.get_by_label("Keep old");
}

#[test]
fn le_mesh_message_and_no_frame() {
    let mut v = Viewport3d::with_state(V3State::Ready);
    v.report = Some(render3d::LoadReport { le_mesh: true, shapes: 0, missing_textures: vec![], compile_errors: vec![] });
    let mut h = Harness::new_ui(|ui| sdc_gui::ui::viewport3d::show(ui, &mut v));
    h.run();
    h.get_by_label("LE mesh: optimise it for SSE, e.g. with Cathedral Assets Optimizer.");
}
```

- [ ] **Step 2: Run to verify failure** — `cargo test -p sdc-gui --test viewport3d` → compile error.
- [ ] **Step 3: Implement**
  - Rendering runs on a worker thread: the `Renderer` lives on that thread, and messages go through `worker.rs` like the encode preview.
  - Orbit camera: drag rotates, scroll zooms, double-click frames the bounds.
  - Re-render only on change.
  - Each frame is uploaded as an egui `ColorImage` (P0 CPU path).
  - The panel lists missing textures and compile errors under the image.
- [ ] **Step 4: Run** — `cargo test -p sdc-gui` → all PASS. Manual check on the owner's machine:
  1. Pick an "Assorted mesh fixes" NIF: it renders.
  2. Rename nothing; turn the CS features toggle off and on: it recompiles (cached the second time).
  3. Simulate an update by pointing gui.toml at a copied CS folder: the popup appears.
  4. Record fps and frame ms for a large armour in the commit message.
- [ ] **Step 5: Commit** — `git commit -m "feat(gui): 3D viewport with CS shaders, locate popup, LE skip"`

---

### Task B9: Zero-copy shared texture into wgpu (perf; gated)

**Files:**
- Modify: `crates/render3d/src/device.rs` (shared NT handle on the target), `crates/gui/src/render/mod.rs` (`wgpu_configuration()`: request `VULKAN_EXTERNAL_MEMORY_WIN32` when the adapter has it), `crates/gui/src/ui/viewport3d.rs`
- Test: `crates/gui/tests/viewport3d_interop.rs` (`#[ignore]`, needs a GPU)

**Gate:** start only if B8's manual check measured CPU readback above 4 ms per frame at 1600×900, or the owner asks. Otherwise P0 is done without it.

**Interfaces:**
- Produces:
  - `device::Target::shared_handle() -> HANDLE` (`D3D11_RESOURCE_MISC_SHARED_NTHANDLE | D3D11_RESOURCE_MISC_SHARED_KEYEDMUTEX`).
  - `viewport3d::import(handle, w, h, render_state) -> Option<egui::TextureId>`:
    - Vulkan: `wgpu::hal::vulkan::Device::texture_from_d3d11_shared_handle`;
    - DX12: `OpenSharedHandle` + `texture_from_raw`;
    - then `register_native_texture` (03 TL;DR 1, §5 P0 step 3).
  - The D3D11 device is created on the wgpu adapter's LUID (B6 `prefer_luid`).

- [ ] **Step 1: Write the failing test**: `shared_texture_round_trip`. Clear the D3D11 target to (0, 1, 0), import it, copy it to a wgpu buffer, read back. Expect green. On import failure the test asserts the CPU fallback path is used instead, and prints which backend failed.
- [ ] **Step 2: Run** — `cargo test -p sdc-gui -- --ignored shared_texture_round_trip` → FAIL (missing).
- [ ] **Step 3: Implement** with a CPU-side fence wait (03 §6 risk row "No portable cross-API GPU sync"). The keyed mutex is acquired on the D3D11 side before drawing and released after; wgpu samples after the release.
- [ ] **Step 4: Run** → PASS on the owner's RTX 4090 (Vulkan default backend). Record frame ms before and after.
- [ ] **Step 5: Commit** — `git commit -m "perf(gui): zero-copy D3D11 → wgpu shared texture for the 3D viewport"`

---

## After this plan

- **P1 plan (live editing):** written from B2 `answers.md`. It covers:
  - material edit UI (cbuffer writes);
  - the shader-type dropdown (background recompile);
  - texture hot-swap of sdc's encoded output (A/B);
  - `_SRGB` view policy per Q1;
  - the TBN source per Q13.
- **P2 plan (parity):**
  - DEFERRED + 8 RTs + `DeferredCompositeCS` port;
  - golden-test replay from stage C/D captures (04 §5.4);
  - Dynamic Cubemaps cost estimate (00 §6.3).

## Self-review notes

- **Spec coverage:**

  | Requirement | Task(s) |
  |---|---|
  | 00 §4.3 P0 | B3–B8 |
  | 00 §5 capture questions | B1–B2 (Q1–Q5 now; Q6–Q15 in the P2 stages) |
  | 00 §6.1 / 6.4 locate + popup | B3, B8 |
  | 00 §6.2 features toggle | B8 |
  | 00 §6.3 DC note | B8 |
  | 00 §6.5 LE skip | B7, B8 |
  | 03 §6 risks | WARP fallback (B6), reflection-driven cbuffers (B5), LUID (B6/B9), CPU fallback (B9) |

- **[Inference] items** that B2 can overturn are isolated to single functions:
  - `register_for_slot`;
  - `pack_tbn`;
  - the specular-colour packing in `draw.rs`.
- **Type names:** `Fingerprint`, `Change`, `Expanded`, `Material`, `Defines`, `Stage`, `Compiled`, `Reflection`, `CbLayout`, `CbWriter`, `Mesh`, `VertexDesc`, `Attr`, `Gpu`, `Target`, `Scene`, `Renderer`, `LoadReport`, `Viewport3d`, `V3State`. Each is defined in the task that first produces it.
