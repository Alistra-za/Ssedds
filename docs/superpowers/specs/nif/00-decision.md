# 00 — NIF texture sets + 3D preview: combined decision document

Date: 2026-10-10. Status: **research done, nothing built; all owner choices answered 2026-10-11; 3 deferred (§6)**.
Inputs: [01-nif-reader.md](01-nif-reader.md), [02-mo2-resolution.md](02-mo2-resolution.md),
[03-renderer-architecture.md](03-renderer-architecture.md), [04-cs-shader-inputs-and-renderdoc.md](04-cs-shader-inputs-and-renderdoc.md).
Shader type → slot → suffix tables: `research_notes\Skyrim DDS converter project plan\skyrim_conventions.md` §8 (not repeated here).
Nothing in `converter\` or `bench\` was modified; all prototypes live in the session scratchpad.

---

## 1. TL;DR

| | Feature 1: texture sets from NIFs | Feature 2: 3D preview |
|---|---|---|
| Recommendation | Hand-written pure-Rust `nif` crate (layer 0 = seek-only scanner) + a VFS crate (loose + own BSA v105 reader on `lz4_flex`) + MO2 resolver that reads MO2's files statically + SQLite cache | Native D3D11 (`windows` 0.62.2, already in the lock) compiling CS's **unchanged** `Lighting.hlsl` with fxc, rendered into a shared texture shown in egui |
| Evidence | 21,505 vanilla+CC NIFs scanned: block-size array valid in 100 %, 2 % of bytes read, ~90 µs/NIF in Python | fxc compiled 4 real CS permutations unchanged, 0 errors, 0.13–0.66 s each; DXC rejects the source (80 errors) |
| Fallback | Nifly FFI (GPLv3, OK licence) if the hand reader stalls — a working, PGPatcher-proven library beats nif.xml codegen (scored 3.50 vs 3.35) as an emergency path | Hand-port to WGSL in an egui wgpu callback |
| First usable result | Scan the user's MO2 setup into a table, single-mod view | Forward-shaded armour on screen with stub lighting |
| Estimate | ~2–3 weeks (reader 1 wk, VFS+BSA 0.5–1 wk, MO2 + DB + UI 1 wk) | P0 1–2 wk, P1 live edit 2 wk, P2 parity 4–6 wk |

Measured on the owner's setup (`G:\Acherus`, MO2 2.5.2 portable): 69k loose NIF copies / 52k unique paths, 21k NIFs in mod BSAs, 25k in vanilla+CC BSAs → ~90–100k unique NIF paths. FaceGen is 13–22 %. Estimated DB ≈ 35–40 MB without FaceGen. Estimated full scan 10–30 s warm (not measured in Rust).

---

## 2. How the two features fit together (the contract)

```
            ┌───────────── Feature 1 ─────────────┐        ┌──────── Feature 2 ────────┐
 MO2 files → mo2 resolver ─┐                        │        │                           │
 loose/BSA → vfs ─────────┼→ scan (nif layer 0) → scan.db ──┼→ pick NIF/shape ─┐        │
                           │       NifShapeRow      │        │                  ▼        │
                           └────────────────────────┘        │ nif layer 1 (geometry)    │
                                                             │ + vfs (textures)          │
                                                             │ → render3d (D3D11 + CS)   │
                                                             └───────────────────────────┘
```

**Contract rules (both plans must honour):**
1. Rows are **per shape**, not per NIF: `NifShapeRow` from 01 §Q8 (shape block, kind, shader kind/type, SLSF1/2, slots[9], effect texture strings, alpha flags+threshold, `LightingParams`/`EffectParams`). Feature 2 edits exactly these fields live.
2. `nif` takes `&[u8]` only. Loose files, BSA entries and test fixtures all go through the same parser. The VFS lives outside `nif`.
3. Layer 1 (geometry) is **additive** to layer 0 (same `cursor`/`header`/`block` modules), so the scanner is never thrown away.
4. Keep `block index + field offset` for every editable field so a later "write back to NIF" can patch fixed-size fields in place.
5. **One canonical path form**, matching the converter engine's existing `game_path` (`converter\crates\engine\src\discover.rs:38`: ASCII-lowercase, **forward slash**, starting at `textures/`). So scan.db texture paths join directly to converter jobs (compare without the extension: the engine keeps the source extension, NIFs name `.dds`). NIF meshes use the same form from `meshes/`. Keep the raw NIF string alongside. (01 proposed backslashes, 02 forward slashes; the engine decides it.)

**Side-by-side development:** the renderer needs only "a NIF's bytes + its textures". Feature 2 P0 can start on nifly's GPLv3 test fixtures and a hard-coded folder while Feature 1's MO2/DB work runs in parallel. The only shared dependency is the `nif` crate's `cursor/header/block` core, built first (~3–4 days).

---

## 3. Feature 1 — plan

### 3.1 Crates (proposed; new workspace members, nothing existing changes)
- `nif` — 01 §Recommendation module layout. Layer 0 now: header + size-checked block index + full decoders of the small blocks (lighting/effect shader, texture set, alpha) + shape heads (incl. particle systems — without them 23 % of effect shaders had no owner).
- `vfs` — loose-file walk + own BSA v105 reader (`lz4_flex`, MIT, pure Rust). `ba2` 3.0.1 (0BSD) supports v105 but pulls C/C++ builds; `bsa` crate is stale.
- `mo2` — instance detection + static resolution (02 §1–§2).
- Scan store — SQLite via `rusqlite` (`bundled`), schema in 02 §6, at `%LOCALAPPDATA%\sdc\scan.db`.

### 3.2 MO2 behaviour ("launch through MO2 once")
1. First launch from MO2: detect `usvfs_x64.dll` in our own process (backup: parent is `ModOrganizer.exe`). Find the instance: `portable.txt`/`ModOrganizer.ini` next to the exe, else the `CurrentInstance` registry value. Persist instance + profile. Confirm with the user in a dialog (covers the `-i` / `moshortcut://` gap).
2. Every scan after that, no MO2 needed: re-read `ModOrganizer.ini`, `modlist.txt` (stored **reversed**: first line = highest priority), `plugins.txt`, profile INIs. Rules: overwrite always wins, game `Data` always loses, loose beats BSA, BSA-vs-BSA by plugin load order (**game order**, not MO2's archive-parsing order, which differs) [Claim: from MO2 code + forum posts; UESP could not be fetched].
3. Change detection: hash of the small config files → re-rank only (no file walk). File (size, mtime) → re-parse only changed copies.
4. Optional "Verify with MO2": a small MO2 Python tool plugin that dumps `getFileOrigins` to JSON for a diff. Not needed for normal use.
5. No MO2: "Open mod folder…" / "Open Data folder…" with a single origin.
6. FaceGen skipped by prefix: `meshes\actors\character\facegendata\` and `textures\actors\character\facegendata\`.

### 3.3 GUI (single-mod view, 02 §7)
Pick a mod (by priority, separators as headers) → its NIFs with a badge each: **Wins / Overridden by X (+N) / Overrides / Inside Foo.bsa, loses to loose Y** → select a NIF → shapes with shader type, flags and slot paths, each texture resolved to its winning origin (missing-everywhere warning) → reverse lookup "who uses this texture".

### 3.4 Open items for Feature 1 (cheap to settle)
- Block-size reliability on **third-party** NIFs (only Bethesda/CC files were tested). Run the prototype over the owner's loose mod NIFs — a read-only 2-minute check.
- LE (BSVER 83) evidence is thin (7 files). Same check over LE mod meshes if any are present.
- One mod BSA failed the quick parser (BA2 renamed or v104?) — identify.
- Engine plugin↔BSA naming strictness; `SkyrimCustom.ini` merge of archive lists.

---

## 4. Feature 2 — plan

### 4.1 Options weighed (full matrix: 03 §4)

| Option | Accuracy vs CS | First pixel | CS-update maintenance | Verdict |
|---|---|---|---|---|
| **c. Native D3D11, CS HLSL unchanged, shared texture → wgpu** | Same source, compiler, profile and register layout as the game | ~1–2 wk | Load the user's installed `Data\Shaders`; only the define table + stubs change | **Recommended** |
| a. Hand-port to WGSL in an egui callback | Port drift | ~2–3 wk (one permutation) | Re-port every CS update | Fallback only |
| b. DXC → SPIR-V → naga, or wgpu HLSL passthrough | Needs patched source | Blocked | Re-patch | Rejected (tested: 80 DXC errors; passthrough uses wgpu's root signature) |
| d. Sidecar process | Same as c | ~3 wk | Same as c | Keep c's API message-shaped so it can move out later (crash isolation) |
| e. Reuse BodySlide / NifSkope / TruePBR-Manager / engine crate | Different shaders | — | — | Rejected. TruePBR-Manager is CC BY-NC (incompatible with GPL) — design reference only |

### 4.2 What "accurate" requires (04)
- **Objects are deferred in CS.** `Lighting.hlsl` writes 8 render targets; specular, reflections and SSGI are added in `DeferredCompositeCS`, fog later in `ISSAOComposite`. Parity target = the 8 targets + the composite output (pre-fog, pre-tonemap), not the backbuffer.
- **Compile-time vs runtime bits.** Shader type, SKINNED, MODELSPACENORMALS, VC, PROJECTED_UV, ANISO_LIGHTING, DO_ALPHA_TEST, TRUE_PBR, DEFERRED, GLINT pick the permutation (→ background fxc + cache). Specular/Soft/Rim/Back, Glowmap, CharacterLight are runtime bits in cb4 (→ instant). Material values are cbuffer writes (→ instant).
- **Gamma.** CS defaults (Linear Lighting off): vanilla materials lit in "Skyrim gamma" (pow 1.6), raw texels. TruePBR expects sRGB-decoded albedo/emissive, works linear × 0.65.
- **The NIF-flag → descriptor table lives in the game exe**, not CS. Must be confirmed by capture.
- **Stub state for the "golden profile":** interior cell, one directional light, DALC ambient from six colours, no shadows, IBL off, LightLimitFix off (point lights fall back to the 7-slot arrays), a list of CS features disabled at boot (04 §3.2).

### 4.3 Phases
- **P0 (1–2 wk):** `render3d` crate; D3D11 device on wgpu's adapter LUID; import via `texture_from_d3d11_shared_handle` (Vulkan, sdc's default backend) or `OpenSharedHandle`+`texture_from_raw` (DX12); CPU-copy fallback. Forward permutation, stubbed state, NIF vertex data bound as-is.
- **P1 (2 wk):** live material edit UI, shader-type change → background recompile, **hot-swap sdc's encoded BCn output onto the mesh** (A/B source vs encoded), camera/light/ambient controls, render only on change.
- **P2 (4–6 wk):** DEFERRED + 8 targets + composite port; RenderDoc golden test (§5).
- **Perf target:** 60 fps at 1600×900, 50k-tri skinned armour, 8 × 4K BC7. Estimated <2 ms GPU (RTX 3070 class), <6 ms (GTX 1660 / RX 6600). **Not benchmarked.**

### 4.4 Risks (03 §6)
Shared-handle import unsupported on a driver (→ DX12 path / CPU copy) · hybrid laptops picking different GPUs (→ create D3D11 on wgpu's LUID) · wrong descriptor-to-define table (→ read CS's `Defines:` markers from captures) · CS cbuffer layouts changing (→ build stubs from shader reflection) · a bad permutation TDR takes down sdc (→ sidecar later).

---

## 5. RenderDoc: settle by capture, not assumption

RenderDoc 1.43 is already at `F:\ai\dds\tools\RenderDoc_1.43_64` (03 missed it; 04 uses it). Protocol = 04 §5: golden CS settings profile, `QASmoke`-style interior, one object per shader type with a **unique diffuse** each, three captures (golden / golden + 1 point light / as shipped). Four new scripts extend `s8_stage1/2.py` (enumerate draws → dump formats + cbuffers → save resources + 8 targets → DebugPixel). Not written yet; they go in `tools\rdc-scripts\` only when you say so.

**Capture questions, ranked by what they unblock:**
1. **Per-slot SRV view formats (`_SRGB` or not)** for vanilla `_d` vs PBR albedo. This also decides the **converter's** sRGB policy today, not just the preview.
2. NIF-flag → descriptor bits (does type 1 envmap also need SLSF1 bit 7? glow need SLSF2 bit 6? what sets VC / DoAlphaTest?).
3. Input layout / vertex format the game binds for SSE BSTriShape, and which NIF array becomes BINORMAL (tangent/bitangent naming is swapped in NIFs).
4. cb5 SharedData / cb12 PerFrame values interior vs exterior → stub presets.
5. How much `DeferredCompositeCS` changes a static object → is forward P0 "close enough" as a first preview?
6. Unskinned MSN lighting space; bone buffer layout; render-target formats and blend state.

**Pin the CS version first.** The S8 capture's installed CS differs from the local source (smaller PerMaterial cbuffer, extra PhysSky textures).

---

## 6. Owner choices needed (ranked)

1. **DECIDED (owner, 2026-10-11): target the installed CS build** at `C:\_mount\moddingvol\Acherus\mods\CommunityShaders_AIO-2026-10-03T06-46Z` (MO2 meta `version=d2026.10.11.0`). The local `skyrim-community-shaders-dev` clone is older and is reference only. Checked: its `Lighting.hlsl` differs from the clone (1,206 diff lines) and it ships `PhysicalSky`, which explains the PhysSky SRVs seen in the S8 capture. fxc 10.0.26100 compiled its `Lighting.hlsl` unchanged: PS skinned+MSN+DEFERRED 107 ms, PS TRUE_PBR+DEFERRED 391 ms, VS skinned 118 ms, 0 errors (outputs in the session scratchpad `r3d_installed\`). New captures must use this build; the S8 capture may predate it.
2. **DECIDED (owner, 2026-10-11): default = CS as installed** (every feature the user's CS build ships and has enabled, incl. Advanced Skin and Hair Specular). A toggle turns CS features off, down to vanilla-in-CS. The capture protocol still captures both states so each mode has a golden reference.
3. **DEFERRED (owner, 2026-10-11): Dynamic Cubemaps parity is not a priority.** Weigh its cost later (cubemap stand-in + the composite's reflectance path). Until then the preview runs with DC off and says so in the UI.
4. **DECIDED (owner, 2026-10-11): load `Shaders\` from the user's installed CS, conditional on handling changing paths smoothly.** MO2 mod folders change name on update (this build's folder carries a date: `CommunityShaders_AIO-2026-10-03T06-46Z`), so a stored absolute path will go stale. Design:
   - **Locate, don't store blindly.** With an MO2 instance (Feature 1), resolve the winning `shaders/lighting.hlsl` through the same VFS winner table and take its mod folder as the shaders root. Without MO2, the user picks the CS mod folder or `Data` once.
   - **Remember** the root, the owning mod name and a hash of the shader sources (`%APPDATA%\sdc\gui.toml`, GUI-only like other GUI settings).
   - **Check on every start and on window focus**: root missing, owning mod changed, or source hash changed.
   - **Popup when it changed:** "Community Shaders moved/updated: *old* → *new*. Use the new location?" with **Use new** (default, when exactly one candidate was found), **Choose folder…**, **Keep old** (only if it still exists). A hash-only change (CS updated in place) needs no popup: show a one-line notice and recompile in the background; compiled permutations are cached by source hash, so stale ones are never reused.
   - **Not found:** the 3D preview shows "Community Shaders not found" with **Choose folder…**; the rest of the GUI works.
   - For this owner today the root is `C:\_mount\moddingvol\Acherus\mods\CommunityShaders_AIO-2026-10-03T06-46Z`. No vendored copy.
5. **DECIDED (owner, 2026-10-11): LE meshes (BSVER 83, NiTriShape-era) are not supported for rendering.** Detect them from the header, show a warning ("LE mesh: optimise it for SSE, e.g. with Cathedral Assets Optimizer"), and skip rendering. [Inference] The texture-set scan can still list their slots, because the property blocks have the same layout at BSVER 83 and 100 (01 §Q2) — this costs nothing and lets the warning name the mesh's textures; flag the row `le_mesh`. Layer 1 (geometry) needs only the SSE BSTriShape path; NiTriShapeData decoding is dropped.

---

## 7. Recommended order

1. Read-only check: run the throwaway scanner over the owner's loose mod NIFs (block-size reliability on third-party files). ~5 min.
2. ~~Owner choices~~ — answered 2026-10-11 (§6); Dynamic Cubemaps cost to be weighed later.
3. RenderDoc session per 04 §5 (captures Q1–Q3 first).
4. Write the specs (one per feature) from this document, then implementation plans.
5. Build `nif` core → split: Feature 1 (vfs, mo2, db, UI) ‖ Feature 2 P0.

---

## 8. Plans (written 2026-10-11)

- [2026-10-11-plan-A-nif-scan.md](2026-10-11-plan-A-nif-scan.md): `nif` layer 0, `vfs` (loose + BSA v105), `modscan` (MO2 order, BSA rank, PGPatcher failsafe, SQLite), Meshes view. 12 tasks.
- [2026-10-11-plan-B-3d-preview-p0.md](2026-10-11-plan-B-3d-preview-p0.md): RenderDoc stages A/B, `render3d` (locate + popup, include expander, D3DCompile + reflection cache, layer-1 geometry, D3D11 forward draw), 3D viewport, gated zero-copy interop. B1–B9.
- Inputs added after §6: [05-pgpatcher-interaction.md](05-pgpatcher-interaction.md) (PGPatcher failsafe: needed), [06-third-party-nif-check.md](06-third-party-nif-check.md) (87,266 mod NIFs, 0 size failures).
