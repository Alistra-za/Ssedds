# P1 spec: live set preview on CS shaders (3D preview, phase 1)

Date: 2026-10-11. Status: **draft for owner review**. Nothing built.
Inputs: `01-pgpatcher-truepbr-rules.md` (01), `02-set-editor-integration.md` (02), `03-renderer-p1-design.md` (03). This spec decides, assigns and slices. Details stay in those docs and are cited by section.
sdc-owned symbols are cited by name (checked present and `pub` on `nif/main @ 1d357be`: `pbr_export::json_entries`, `pgp::writer::{EntryInput, entry_node, Node::to_value}`, `sets::set_views`, `pgp::prefix::{MatchMode, match_mode, output_rel, derive_match_diffuse}`).

---

## Decisions needed from you

| # | Question | My recommendation |
|---|---|---|
| D1 | **Emissive colour.** PGPatcher marks `emissive:true` shapes "external emittance", so in game the engine supplies the glow colour. Our one capture shows 0 on an emissive PBR draw (01 Q1). What should the preview use? | The NIF's `emissive_color × emissive_scale`, labelled "approximate". Change it after capture C1 (below). |
| D2 | **Second GPU device.** The set panel gets its own 3D viewport (and its own D3D device, tens of MB) instead of sharing the Meshes view's (02 §4.3). | Yes, for P1. Share one viewport later if the cost shows up. |
| D3 | **Sets the simulation can't preview.** PGPatcher simulation needs the CS target and a PBR diffuse (`json_entries` is empty otherwise). | Show the panel anyway with the set's plain textures on the mesh and a line "PGPatcher preview needs the CS target and a PBR set". |
| D4 | **Community Shaders features.** One on/off toggle (P0) or a per-feature checklist? | Keep the single toggle; under it, list the features that are off in the preview (screen-space/world ones) as plain text. |
| D5 | **Preview texture size.** Each slot is encoded for the preview at a capped size. | 2048 px default, a setting; the panel says "preview at 2048 px, export writes full size". |

Also needed: **approval for shared-file edits** (§6) and **three captures** (§7).

---

## 1. What P1 delivers

**Goal.** Select a PBR set in the Textures view → a 3D panel shows a real game mesh that uses that set's diffuse, patched in memory exactly as PGPatcher 1.2.0 would patch it, rendered with your installed CS 1.9 `Lighting.hlsl`. Dragging a material slider updates the image on the next frame. Re-encoding a texture swaps it in without reloading the mesh.

**First demo (end of plan P1-A):** pick `reserve_bc` (Xocco3's Mead, 01 §5.2 "plain" row) in the set editor → the panel shows `meshes\xmead\reserve.nif` with the converter's encoded textures; drag `roughness_scale` → the highlight changes on the next frame, with no recompile.

**Success criteria.**
1. For every 01 §5.2 golden case, the simulator's patched NIF fields equal the real `ParallaxGen_Output` shape (same field list as 01 §5.1).
2. The packed `PBRFlags` for the capture's eye-moisture shape is 428 (03 §1.3).
3. A cbuffer-only edit (roughness, colours, UV scale, emissive scale) shows on the next frame with zero compiles.
4. A shader-changing edit never blanks or freezes the panel: the old image stays until the new shader is ready, and a compile failure keeps the old image with the error listed.
5. The panel shows which shapes PGPatcher will patch, and says plainly when PGPatcher would skip one ("will be skipped: `_rmaos` missing").

---

## 2. Architecture (who owns what)

```
set editor (sdc, read-only)            pgpsim (NEW crate, pure)               render3d (ours)
───────────────────────────            ──────────────────────────             ───────────────
json_entries → EntryInput ──Value──▶   match + patch                   ──▶    MaterialState per ShapeId
set_views → slot items                 in:  entry Value, default block,       CS load remap (pbr_flags,
items[..].job → encode (ours)           source MaterialState + slots,         PBRParams, MLPD, GLINT)
                                         nif path, shape block                 cbuffer / texture / permutation
                                       out: Patched{MaterialState, slots,      edits, background compile,
                                             badges} | Skip(reason)            hot-swap, CS settings
```

### 2.1 New crate `crates/pgpsim` (owner of the PGPatcher simulation)
Nobody owned it in the research (02 §5.5 and 03 risk 8 each pointed at the other). It is a **new pure crate on nif-team ground**: no GPU, no egui, no file IO. `gui/pgp` stays untouched (off-limits).

- **Boundary:** pgpsim stops at "the NIF as PGPatcher leaves it". It produces a `MaterialState` (03 §1.2), which carries the 9 slot paths. `MaterialState` lives in the `nif` crate (new `nif::material`, pure data over `nif::props`), so pgpsim and render3d share it without pgpsim depending on a GPU crate. render3d owns everything CS does at load (01 §3: flag remap, `PBRFlags`, `PBRParams1/2`, `MultiLayerParallaxData`, bound registers).
- **Contract:**
  ```rust
  pub struct SimInput<'a> { entry: &'a Value, default: &'a Value, nif_path: &'a str,
                            shape_block: u32, source: &'a MaterialState }
  pub enum SimResult { NoMatch,
                       Patched { material: MaterialState, matched_path: String, badges: Vec<Badge> },
                       Skip(String) }
  pub fn simulate(i: &SimInput) -> SimResult;              // one entry (sdc's case)
  pub fn simulate_file(entries: &[Value], default: &Value, ..) -> SimResult;  // 01 §1.5 "last wins"
  pub fn shape_matches(entry: &Value, default: &Value, nif_path: &str, slots: &[String]) -> bool;
  ```
- **Rules:** 01 §1.1 (default merge, `texture`→`match_diffuse`, backslash prefix), §1.3 (match + `matchedPath` + `rename`), §2.1–2.3 (order of operations, always-on flag clears, leaks, slot table, scalar table, 0→11 MLP defaults (0,1,1,1)), 1.2.0 semantics.
- **Missing textures (ruling):** follow PGPatcher (01 §1.4), not 02 §2.3. If any non-empty patched slot cannot be resolved (not an sdc slot item that can be encoded, not a VFS winner), the result is `Skip("<path> missing")`: the shape renders on its **source** material with a red badge.
- **Badges, "not previewed":** `smooth_angle`, `auto_uv`, `vertex_color_lum_mult` / `_sat_mult`, and 1.2.0-only keys (`pbr:false`, `env_mapping*`).
- **Supersedes** 02 §5.5's `ShapeOverride` / `load_with`: the panel calls `simulate`, then `Renderer::set_material(id, material)`.

### 2.2 render3d
03 as written (§1–§8), with these rulings:
- **GLINT define (closes 03 Q5):** under PBR, `permutation::defines` emits `GLINT` from the CS-remapped Glint flag (SLSF2.12 Fit_Slope, no Soft, no Back, no MLP; 01 §3.1), **not** from SLSF2.21 as P0 does. SLSF2.21 is cleared by CS at load. Capture C2 confirms; it is not a gate.
- **Coat/fuzz/glint shader type (closes 03 Q4):** PGPatcher writes type 11 for multilayer, glint and fuzz (01 §2.2), so the MLP trailer is loaded. Type 11 still draws technique 0 under PBR (01 §3.1).
- **`PBRParams2` and `MultiLayerParallaxData`:** written only in the branches CS writes them (TwoLayer/Subsurface; TwoLayer/Fuzz/Glint), zero otherwise (01 §4 item 4; 03 T-E2).
- **Root node transform (03 Q6):** applied, as NifSkope does.
- **Compile pool (03 Q8):** 2 workers, below-normal priority, not a setting.
- **`pointLightMult` (03 Q1):** keep the capture's π while the preview has no point lights.

### 2.3 GUI (Textures view)
02 as written, with:
- **Placement:** right-side resizable panel in the Textures view (02 §4.2 A), shown when a set (or a member item) is selected. New files `ui/set_preview.rs`, `model/set_preview.rs`.
- **Encodes:** own per-member `Session::preview` jobs, DDS rebuilt with `engine::dds`, served through `FileSource::Memory` (02 §2.2); fresh/stale per member (02 §2.4); paused during export.
- **Change detection:** per-frame fingerprint of the selected set's `EntryInput` + member signatures, throttled to input frames or 200 ms (02 §3.2). Texture change → re-encode → `SwapTexture`; JSON-only change → `simulate` → `SetMaterial`. No `AppModel::revision()` request unless profiling asks for it.
- **Mesh picker:** game NIFs whose shapes match the entry, best first; built-in Sphere/Plane/Cube always available and the default when none match or the NIF is LE. The choice is remembered per set for the session only.
- **The panel states, in plain words:** "Assumes your sdc JSON wins over other mods' PGPatcher configs."

### 2.4 Mesh query (modscan)
`Store::nifs_using_texture` (02 §5.2) with the **predicate per `MatchMode` from 01 §1.3** (replaces 02's equality):

| MatchMode | Predicate on the shape's slot path (lowercased, extension stripped, leading `textures\pbr\` → `textures\`) |
|---|---|
| `Diffuse` | slot 0 ends with `\` + `lower(match_diffuse)`, on whole path components |
| `Normal` | slot 1, after also stripping `_n` / `_msn`, ends with `\` + `lower(match_normal)` |
| `PathContains` | slot 0 contains `lower(value)` |
| `Slot(n)` | slot n-1 (with `.dds`) equals `textures\` + value |

Plus `nif_filter` as a substring of the mesh path. Winners only; for a PGPatcher-derived winner, use its source copy (02 §5.2). A suffix match cannot use the path index; it runs off the UI thread on key change, and its timing is recorded in the plan's verification. The final shape list is re-checked with `pgpsim::shape_matches`, so the SQL is only a candidate filter. Add `Store::open_read_only` so the panel can never trigger a schema rebuild.

---

## 3. Out of scope for P1 (said in the UI where it matters)

- **Cross-mod arbitration.** Whether another mod's PGPatcher JSON, Complex Material or parallax match beats sdc's entry (01 §1.5) needs every `PBRNifPatcher` JSON, including those in BSAs. P1 assumes sdc's JSON wins. Candidate for a later "conflict check" built on the scan DB.
- **Plugin side.** Unused NIFs (PGPatcher patches only meshes with a plugin use), alternate-texture TXSTs, `PBRTextureSets`, MATO, `_pgpatcher_dups`.
- **Other PGPatcher patchers** (Parallax, Complex Material, post-patchers): they compete with TruePBR, never combine with it (01 §1.6).
- **Screen-space and world CS features** (03 §3.2 "off" rows) and Dynamic Cubemaps. Listed in the panel.
- **Geometry edits:** `smooth_angle`, `auto_uv`, vertex-colour multipliers (badged).
- Writing NIFs. LE meshes (warn, don't render, as in P0).

---

## 4. Slicing: three plans, each shippable

Total ≈ 27–31 developer-days (03's 18–21 plus pgpsim ≈ 4 and GUI integration ≈ 5–6). One plan won't hold that, so three plans on one branch `nif/b-p1` (own worktree), each merged into `nif/main` on your OK.

| Plan | Contents | Ships as | Size |
|---|---|---|---|
| **P1-A: live set preview** | render3d P1.0 (parse/build split, `ShapeId`, `MaterialState`, `PBRFlags` fix, per-frame cost) + P1.2 (cbuffer/texture edits, built-in shapes; shader-changing edits still compile synchronously) · `pgpsim` with 01 §5.3 steps 1–2 (unit fixtures + golden tests, opt-in with `SDC_ACHERUS`) · modscan query · GUI panel, encodes, `Memory` source, fingerprint, mesh picker | **The first demo** (§1). Correct TruePBR features. Shader-type changes stutter 0.1–0.4 s | ≈ 14–16 d |
| **P1-B: no stalls** | render3d P1.3 (background compile pool, keep-old-draw, content-addressed cache + pruning) + P1.4 (hot-swap, source/encoded A/B toggle) + P1.1 (parent NiNode transforms) | No freezes; instant A/B; multi-node NIFs placed right | ≈ 8–9 d |
| **P1-C: your CS settings** | render3d P1.5 (read `SettingsUser/Default.json` + `Info.ini` through MO2, overwrite wins; feature defines for the 6 material-local features, each behind its installed-CS test; FeatureData from settings; re-read on focus) | Preview matches your LL, Fresnel, hair and skin settings | ≈ 4–5 d |

P1-B and P1-C both depend only on P1-A and can run one after the other in either order. P0's `load` stays as a compatibility wrapper so the Meshes view keeps working at every step.

---

## 5. Testing

- **pgpsim:** table tests for every rule in 01 §1.3/§2 (suffix-on-component matching, `texture` vs `match_diffuse`, `textures\pbr\` strip, `matchN` with `.dds`, `rename`, locks, `slotN` last, multilayer > glint > fuzz, leaks, 0→11 defaults, multi-entry last-wins, missing-texture skip). Golden tests on the nine 01 §5.2 cases, `#[ignore]` unless `SDC_ACHERUS` is set.
- **render3d:** 03 §8 table as written (WARP + repo-owned stand-in shader), plus `pbr_flags` for every 01 §5.2 golden case (e.g. the coat cave = TwoLayer | InterlayerParallax | CoatNormal | ColoredCoat + resolved Has* bits).
- **GUI:** kittest for the panel states (no scan DB, non-PBR set, no match → built-in, Skip badge); model tests for fingerprint classification (texture vs JSON-only change) and the `Memory` source.
- **Owner round trip (after P1-A, optional):** export a JSON from sdc into a test mod, run PGPatcher 1.2.0, compare the output NIF with the simulation (01 §5.3 step 5). This is the only test that also checks sdc's writer.

---

## 6. Shared-file edits (need your OK; small separate commits per CONCURRENCY §3)

1. `crates/gui/src/ui/mod.rs`: `pub mod set_preview;`, one `State` field, one `Panel::right` block in `draw_with`.
2. `crates/gui/src/model/mod.rs`: `pub mod set_preview;` (one line).
3. `crates/gui/src/app.rs`: 4 small additions (seed shaders path, poll, persist shader path changes, repaint while busy) and pass the Meshes scan target for the first locate (02 §6.2.3). Your earlier `app.rs` OK was for plan B, so I'm asking again.

No sdc-owned file changes. **One linter gap to hand to the sdc team (not ours to fix):** `check_rel_path` doesn't reject a leading `textures\` in `match_diffuse`/`match_normal`, and such a value never matches in PGPatcher (01 §1.3, Q7). The preview will show "matches no shape", which surfaces it anyway.

---

## 7. Captures to request (one session, Performance Overlay visible so `Defines:` markers land)

| ID | Mesh in game | Closes |
|---|---|---|
| C1 | An emissive PBR mesh: `meshes\clutter\eyeofmagnus\eyeofmagnus01.nif` or `meshes\dlc01\traps\lighttrap\lighttrap01.nif` | D1 / 01 Q1: what `EmitColor` the engine sends vs the NIF's emissive colour × scale |
| C2 | A glint mesh: Sovngarde stairs `sovstairspiece01.nif` (01 §5.2) | GLINT define source and glint packing (01 Q4, 03 Q5) |
| C3 | A fuzz mesh: vampire robes `vampirerobesfkha_1.nif` or `smalltreetapestry.nif` (01 §5.2) | Fuzz packing (01 Q4); define names `VANILLA_FRESNEL`, `ENABLE_LL` (03 Q2) |

None blocks P1-A; each turns an inferred value into a confirmed one. The RenderDoc scripts in `tools\rdc-scripts\p04_*` already extract these values.

---

## 8. Closed by this spec

| Question | Answer |
|---|---|
| 02 Q1 match semantics | 01 §1.3 → §2.4 table |
| 02 Q2 JSON → shader mapping | 01 §3.5 |
| 02 Q5 follow member item | Yes; panel stays visible beside the expanded-texture view |
| 02 Q7 persist mesh choice | Session only in P1 |
| 03 Q4 type 11 | Yes for multilayer/glint/fuzz |
| 03 Q5 GLINT | From the remapped Glint flag (§2.2); C2 confirms |
| 03 Q6 root transform | Applied |
| 03 Q8 pool | 2 workers, below normal |
| 03 Q9 shared frame cbuffers | Not in P1 |
| 01 Q5 leaked Fit_Slope | Preview shows it as CS does (glint, zero params); no sdc lint in P1 |
| 01 Q6 missing `specular_level` | sdc always writes it; preview shows the result as CS would, no warning |

Still open after your decisions: 01 Q2/Q3 (the 9 unreproduced residuals; not in sdc's path), 03 Q1 (`pointLightMult`, only with point lights), 03 Q3 (skin packing, verified in P1-C by the installed-CS test).
