# PBR Texture Sets and PBR Material Objects: design (2026-10-13)

Status: APPROVED by owner 2026-10-13, including edits to the off-limits files sets_doc.rs, model/pbr_export.rs and pgp/writer.rs (§10). Research: `01-research.md` (same folder); every Community Shaders (CS) claim below is cited there.
Code base: nif/main 54a7a1c, worktree `F:\ai\dds\converter-nif-p1`. Execution later on a new branch `nif/j-cspbr` in its own worktree.

## 1. Goal

sdc writes the two CS runtime JSON kinds next to its existing PGPatcher export:
- **PBR texture set**: `<export root>\PBRTextureSets\<EDID>.json`, one file per TXST EditorID.
- **PBR material object**: `<export root>\PBRMaterialObjects\<EDID>.json`, one file per MATO EditorID.

The set editor starts these from a built-in template library made from Faultier's PBR landscape modders resource. The 3D preview shows landscape sets on the plane only, with a preview-only tiling control (1x to 3x).

Out of scope (owner): MATO preview work beyond the existing default meshes; writing plugins (TXST/LTEX/MATO records); a landscape shader or projected-material shader path.

## 2. Owner decisions (2026-10-13)

| # | Decision |
|---|---|
| O1 | New built-in PBR template library with two kinds: texture set and material object. |
| O2 | Faultier is credited in the docs only, linking https://www.nexusmods.com/profile/Faultierfan?gameId=1704. The owner handles permission. |
| O3 | (revised 2026-10-13 after a scan of the owner's load order: 46 of 114 landscape diffuses are used by ~1,560 non-LOD meshes, e.g. dirt02 512, cavebaseground01 385, rocks01 278) The main kind stays **PBR landscape**, which writes the texture-set JSON. When statics use the set's diffuse, the same set also writes its PBRNifPatcher entry, from the same values, so the two cannot drift. |
| O4 | The library is deduplicated: 31 texture-set and 3 material-object value sets. Each template keeps the EDIDs it was used for. |
| O5 | Material objects use the existing default preview meshes (no new 3D work). Landscape sets show the plane only, with tiling up to 3x. |
| O6 | The exported file name equals the record's EDID exactly (case included). |

## 3. Data model

A set gains two optional blocks, stored in its `[[sets]]` table in `sdc-project.toml`:

```toml
[sets.cs_txst]                     # PBR texture set
edids = ["LandscapeSnow01", "LandscapeSnow01Landscape"]
template = "LandscapeSnow01"       # informational: last applied template
roughnessScale = 1.0
displacementScale = 0.2
specularLevel = 0.04
glintParameters = { enabled = true, screenSpaceScale = 1.5, logMicrofacetDensity = 24.0, microfacetRoughness = 0.7, densityRandomization = 300.0 }

[sets.cs_mato]                     # PBR material object
edids = ["SnowMaterialObject1P"]
baseColorScale = [0.98, 0.98, 1.0]
roughness = 0.9
specularLevel = 0.04
```

- Field names and defaults are CS's own (`TruePBR.h`), with no translation layer. Texture-set fields: `roughnessScale`, `displacementScale`, `specularLevel`, `subsurfaceColor`, `subsurfaceOpacity`, `coatColor`, `coatStrength`, `coatRoughness`, `coatSpecularLevel`, `innerLayerDisplacementOffset`, `fuzzColor`, `fuzzWeight`, `glintParameters{enabled, screenSpaceScale, logMicrofacetDensity, microfacetRoughness, densityRandomization}`. Material-object fields: `baseColorScale`, `roughness`, `specularLevel`, `glintParameters`.
- A field that is absent from the block is absent from the export (CS applies its default), except where §5 says it is always written.
- A set may have one block, both, or neither. Several EDIDs share one value set (O4).
- `cs_txst` and `cs_mato` become **reserved keys** in `sets_doc.rs`. Without that they would be flattened into the PGPatcher settings and leak into the PGPatcher JSON. **This edits an off-limits file (CONCURRENCY.md §3) and needs owner OK** (see §10).

## 4. Template library

- One committed data file, `crates/gui/assets/cs_pbr_templates.json`, generated once from the resource folder by a script kept in `tools/` (the script is not part of the build). It is embedded with `include_str!`, the same way presets are.
- Entry: `{ "name", "kind": "txst" | "mato", "edids": [...], "values": { <CS fields> } }`. Deduplication groups files whose values are equal after parsing (ints read as floats). The name is the most common EDID prefix in the group, or else the first EDID alphabetically. The 3 `BDS_*` material objects are included (their EDIDs are carried as-is).
- The set editor gets a **Template** picker per block. It lists templates of that block's kind with their EDIDs. Applying one is a single undo step that replaces the block's values and, if the block has no EDIDs yet, offers the template's EDIDs (it never removes EDIDs the user added).
- Credit: `docs/CREDITS.md` (new) and a line in `docs/gui.md`'s section on this feature: "PBR landscape template values by Faultier (https://www.nexusmods.com/profile/Faultierfan?gameId=1704), from Faultier's PBR landscape modders resource." No credit inside exported JSON (strict JSON, CS reads named keys only).

## 5. Export

- Runs together with the existing export (same button, same export root), as a new module `crates/gui/src/model/cs_pbr_export.rs`. The PGPatcher export is not changed.
- One file per EDID per block: `PBRTextureSets\<EDID>.json` or `PBRMaterialObjects\<EDID>.json`. The file stem is the EDID byte for byte.
- Format: one JSON object, 4-space indent (like CS's own saver), keys in CS's field order. **Every number is written with a decimal point** (`1.0`, never `1`): CS silently drops colour arrays with int literals. `specularLevel` is **always** written for material objects (CS default 1.0 is 25x the reset value).
- File writing reuses `pbr_export.rs`'s atomic tmp-write and SHA-256 hand-edit guard. If they are private, they are made `pub(crate)` with no behaviour change (**off-limits file, owner OK needed**, §10).
- Refusals (export blocked for that file, with the reason shown): an EDID that is empty, contains characters invalid in a Windows file name (`<>:"/\|?*`, control characters, trailing dot or space), or is a reserved device name; two sets binding the same EDID in the same kind (names both sets).
- A file that sdc wrote earlier for an EDID no longer bound by any set is reported, not deleted (same rule as the hand-edit guard: sdc never removes user files silently).

### 5.1 Statics (PBRNifPatcher entry from the same set)

- A PBR landscape set is still a normal CS PBR set, so the existing PGPatcher export already writes its entry. The `cs_txst` values are the single source: the PGPatcher entry takes `roughness_scale`, `specular_level`, `displacement_scale`, `subsurface`/`subsurface_color`/`subsurface_opacity`, `multilayer` + `coat_*`, and `glint` (only when `enabled`) from the block, using the mapping in research §1.5. Editing either side edits the block.
- The entry is written when the scan finds at least one winning non-LOD NIF whose shape uses the set's diffuse (new query: same join as the research scan, LOD paths excluded with `is_lod_path`). The set shows "Used by N meshes: PGPatcher entry written" and lists them on demand. With no scan, or no statics, no entry is written and the set says why. A per-set override (`statics = "auto" | "always" | "never"`, default auto) covers mods not yet scanned.
- `innerLayerDisplacementOffset`, `fuzz*` and `baseColorScale` have no PGPatcher equivalent and are not mapped. `rename` and `slotN` are not generated (Faultier's rename entries for differently named statics stay hand-made).
- This needs the PGPatcher writer to read the mapped values from the block: **`pgp/` is off-limits; owner OK needed** (§10).

## 6. EDID binding and warnings

- **Texture sets:** the EDID list offers suggestions from the scan through the existing `txsts_matching` query on the set's diffuse path. Free entry is always allowed (records outside the scan, e.g. a plugin not yet installed).
- **Material objects:** a new scan-DB query lists winning MATO records (EDID, plugin, single-pass flag) for a filterable picker; free entry is allowed.
- Warnings (shown on the block, never blocking export):
  - W1 EDID not found among scanned winning records of that kind (or no scan yet: "not checked").
  - W2 EDID matches a scanned record only case-insensitively: shows the exact form with a one-click fix (CS is case-sensitive).
  - W3 material object whose winning record is not single-pass: "CS ignores this JSON unless the record is single-pass". Records with a 48-byte DATA (no flags field) show "single-pass unknown" instead. This needs MATO `DATA` flags read in `crates/esp` (one field) and stored in the scan DB.
  - W4 texture-set block on a set whose diffuse path is not used by any TXST in the scan ("no texture set uses this texture").

## 7. 3D preview

- **Landscape** = a set with a `cs_txst` block, or a set whose path starts with `landscape\`. For it, the mesh picker is fixed to the built-in Plane (other choices hidden), and the viewport shows a **Tiling** slider from 1.0 to 3.0 (step 0.5, default 1.0).
- Tiling is preview-only and session-only. It multiplies `uv_scale` **after** the pgpsim patch (pgpsim overwrites `uv_scale` from a PBR entry). It is a constant-buffer change only (samplers already WRAP), with no shader recompile.
- The preview applies the texture-set block's `roughnessScale` and `specularLevel` on the object PBR path. Terrain multi-tile blending is not simulated.
- **Material objects:** no change. They use the existing default meshes.

## 8. Error handling

- Template file parse failure is a build-time test failure, never a runtime path.
- A malformed `cs_txst`/`cs_mato` block in a hand-edited project file: unknown keys are kept and round-tripped; a wrong-typed known key shows a set warning and is not exported; the set still loads.
- Export write errors use the existing export error report; one failed file does not stop the others.

## 9. Testing

- Writer: golden JSON for a texture set and a material object; int-valued inputs are written with a decimal point; material-object `specularLevel` present when unset; absent fields omitted.
- Round trip: project file with both blocks, unknown keys and comments survives load and save; `cs_*` keys never reach the PGPatcher JSON.
- Library: the embedded file parses; counts are 31 texture-set and 3 material-object templates; every resource JSON's values equal its template's values (test under `#[ignore]` reading the resource folder, read-only).
- EDID validation: invalid file names, duplicates across sets, case-only mismatch suggestion.
- Statics: a landscape set whose diffuse is used by a non-LOD NIF writes both files with equal values; with only LOD users, or `statics = "never"`, it writes the texture-set JSON only; int glint values map to floats; `enabled = false` writes no `glint`.
- Export: one file per EDID, stem equals EDID exactly, hand-edit guard respected, orphan report.
- Preview: landscape set gets Plane only; tiling multiplies after pgpsim (a set whose PBR entry sets `uv_scale` 2 shows 4 at 2x).
- Owner-data check under `#[ignore]` + `SDC_ACHERUS`: the MATO query and single-pass flags on the owner's load order.

## 10. Files and ownership

| File | Change | Ownership |
|---|---|---|
| `crates/gui/src/model/cs_pbr.rs` (new) | blocks, templates, validation, warnings | nif |
| `crates/gui/src/model/cs_pbr_export.rs` (new) | writer + export | nif |
| `crates/gui/src/ui/cs_pbr.rs` (new) | block UI, template picker, EDID list | nif |
| `crates/gui/assets/cs_pbr_templates.json` (new), `tools/` script | library | nif |
| `crates/esp` records.rs, `crates/modscan` store/plugin_store | MATO DATA flags, MATO query (schema bump) | nif |
| `crates/gui/src/model/set_preview.rs`, viewport3d | plane lock, tiling | nif |
| `crates/gui/src/sets_doc.rs` | reserve `cs_txst`, `cs_mato`; read/write the blocks | **off-limits: owner OK needed** |
| `crates/gui/src/model/pbr_export.rs` | make atomic write + guard `pub(crate)`; call the CS export; statics rule | **off-limits: owner OK needed** |
| `crates/gui/src/pgp/writer.rs` | entry values from the `cs_txst` block | **off-limits: owner OK needed** |
| set editor UI file, `model/mod.rs`, `ui/mod.rs` | mount the block UI | shared: add-only commits |
| `docs/CREDITS.md`, `docs/gui.md`, `docs/DECISIONS.md` new note | docs | DECISIONS needs owner OK |

## 11. Open risks

- CS folder scanning is believed non-recursive (from memory); sdc writes flat, so either way works.
- `DefaultPBRLand` lookup may need po3 Tweaks in game (unverified); sdc only writes the file.
- The 48-byte MATO records: single-pass status unknown, surfaced as W3 "unknown".
- One texture bound to several TXST EDIDs shares one value set; differing values need separate sets (by design, O4).
