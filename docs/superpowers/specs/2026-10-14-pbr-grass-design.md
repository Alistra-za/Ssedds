# PBR grass material type (design)

Date: 2026-10-14. Status: design approved by the owner in chat (2026-10-14, "approved, go ahead"); this file writes it down with the rulings needed to build it. Evidence: `01-research.md` in this folder. Plan: `2026-10-14-plan-pbr-grass.md`.

Code base read: `F:\ai\dds\converter-nif-p1`, `nif/main` at `b4c228e`. Conflict check against `main` at `3be490a` (the local `main` ref; `d5bd603` is PR #9, docs only, and is older than `3be490a` for `docs/DECISIONS.md`).

## 1. Goal

Community Shaders draws PBR grass with `RunGrass.hlsl`, which reads only albedo, `_n`, `_rmaos` and an optional `_s`, plus roughness scale, specular level and subsurface. sdc gets a set shader for that: **`pbr_grass`**. A PBR set used by a GRAS record picks it automatically, the PGPatcher form shows only the keys grass reads, and sdc warns where the result will not look right in game. Output files and the JSON format do not change.

## 2. Owner decisions

| # | Decision | Source |
|---|---|---|
| D1 | New `Shader::PbrGrass`, id `pbr_grass`, label "PBR grass", CS only (like `pbr`). Required albedo, `_n`, `_rmaos`; optional `_s`. Reuses the PBR sub-types; no new `Subtype`. | chat 2026-10-14 |
| D2 | Auto-pick: a PBR set whose diffuse a GRAS record uses (scan) becomes `pbr_grass`, from a new modscan query that keeps TruePBR (SLSF2.23) shapes. An explicit `pbr` still wins. | chat 2026-10-14 |
| D3 | PGPatcher form trimmed to the keys grass uses: roughness scale, specular level, subsurface (on by default). Hidden: emissive, parallax/displacement, coat/multilayer, glint, fuzz, hair, UV scale. Writer format unchanged. | chat 2026-10-14 |
| D4 | Warnings: "PGPatcher 1.2.0 does not patch grass meshes; needs 2.1.0 or later" (sdc exports anyway, no NIF writing); the complex-grass detector as a negative check on PBR grass albedos. No Grass Lighting warning. | chat 2026-10-14; owner answers 2026-10-13 |
| D5 | Preview: the existing object PBR path with an "approximated" badge. A real `RunGrass` shader is out of scope. | chat 2026-10-14 |
| D6 | Approved edits to otherwise off-limits files: `crates/engine` (`shader.rs`, `policy.rs`, `api.rs`, `codes.rs`), `schema/`, `crates/gui/src/pgp/`, `sets_doc.rs`, `model/sets.rs`, `model/pbr_export.rs`, `docs/DECISIONS.md` (one note). Still off-limits: `crates/cli`, `crates/encoders-sys`, golden and `gui_repro` tests. | task brief |

## 3. Rulings this design makes (owner may overrule)

- **R1 Negative check needs `crates/engine/src/analyse.rs` (OWNER QUESTION).** `Analysis.grass` exists only for 1:2 images, and a PBR albedo is almost never 1:2. CS's detector reads texel (0, h−1) of any texture. The check needs one new field, `Analysis.corner_err` (`grass::unit_err` of that texel, any shape): one line in the struct and one in `analyse()`. `analyse.rs` is engine but not in D6's list. The plan does it in its own commit (Task 3) so it can be dropped; without it the negative check is not built.
- **R2 Subsurface default needs an `_s`.** `pbr_export::NEEDS` blocks export when subsurface is on and the set has no `_s`, and D1 makes `_s` optional. So "on by default" applies when the set's Subsurface slot is filled; without `_s` it stays off (no transmission is possible anyway: research §1.4, opacity 1 with no `_s` gives zero). A user's explicit off is stored as `subsurface = false` on a PBR grass set so the default does not turn it back on; "reset" brings the default back.
- **R3 Hidden keys are dropped, not just hidden.** The PBR grass view of a set's settings (`SetView.settings`, used by the form, the export checks and the writer) leaves out every hidden key. A set switched from `pbr` to `pbr_grass` therefore writes no stale `emissive` or `uv_scale`, and its emissive-without-`_g` block disappears. The keys stay in `sdc-project.toml`; switching back restores them. The JSON editor window is not filtered (it lists every key; a hidden key edited there is still dropped on export).
- **R4 "UV scale" covers `uv_scale` and `auto_uv`** (`auto_uv` only computes `uv_scale`). Hidden keys exactly: `emissive*`, `lock_emissive`, `parallax`, `displacement_scale`, `lock_parallax`, `hair`, `multilayer`, `coat_*`, `inner_uv_scale`, `lock_cnr`, `glint*`, `fuzz*`, `uv_scale`, `auto_uv`. `cubemap`, `zbuffer_write`, `smooth_angle`, vertex colours, locks of grass maps, slots and matching stay (not in D3's list).
- **R5 Evidence is a superset.** The new query `Store::gras_all_diffuse_textures` is `gras_diffuse_textures` without the SLSF2.23 filter: every GRAS slot-0 diffuse, TruePBR or not. A PBR texture for a vanilla grass NIF (to be patched later) and a pre-patched PBR grass NIF both count. The old query stays for complex grass (DECISIONS X5). No scan-DB schema change, so `SCHEMA_VERSION` stays 7.
- **R6 Warning homes and severity.** `PBR-GRASS-PGPATCHER` is a set warning (every `pbr_grass` pick, CLI included); `PBR-GRASS-DETECT` is a job warning on the albedo. Both `Warning` severity: nothing blocks.
- **R7 Shader order.** `pbr_grass` sits right after `pbr` in `Shader::ALL`, so the CS dropdown reads PBR, PBR grass, Advanced Skin, …; the CPM and vanilla lists start at index 3.
- **R8 Not built** (research §4.2 items outside D1–D5): the flat-lit, opacity ≥ 1 and hidden-key-in-JSON lints, the SLSF2.30 sphere-normal warning, the pgpsim "GRAS + 1.2.0 → skip" result, the CS texture-set/material-object blocks for grass sets (GRAS has no TXST; the block stays offered), and any sub-type or alpha default change.
- **R9 DECISIONS letter: Note AG.** `nif/main` ends at Note AD, but `main` (`3be490a`) already has AE and AF.

## 4. Behaviour

### 4.1 Shader table (`engine::shader`)

```
static PBR_GRASS: ShaderSpec = ShaderSpec {
    required: &[role("albedo", &[PbrAlbedo]), role("_n", &[PbrNormal]), role("_rmaos", &[Rmaos])],
    optional: &[role("subsurface _s", &[PbrSubsurface])],
};
```

- `available(Cs)` lists 14 shaders; CPM 11 and vanilla 10, neither with `pbr_grass` (fallback `default` with `SHADER-UNAVAILABLE`, `wanted: pbr_grass`).
- `has_member_defaults` false, `member_subtype` none: members resolve as in a `pbr` set, encoding unchanged, no version bump.
- `check_maps` gives `SHADER-EXTRA-MAP` for `_g`, `_p`, `_cnr`, `_f` members; the set editor's slot cards follow `usable_slots` (Diffuse, Normal, RMAOS, Subsurface).

### 4.2 Auto-pick

- `AutoRule::PbrGrass` (id `pbr_grass`, provenance `auto:pbr_grass`), tried inside the PBR rule: a set that would pick `pbr` picks `pbr_grass` when `PickInput.gras` is true. Advanced skin still comes first.
- `PickInput.gras` = any member's `grass::path_key(game_path)` is in `BatchRequest::gras_all_diffuses`.
- The GUI fills `gras_all_diffuses` from the scan with the other mesh evidence; the CLI sends none, so the CLI never auto-picks it (same class as DECISIONS X11). `--set set:<id>.shader=pbr_grass` works.
- Explicit choice (GUI or `--set`) wins as for every shader.

### 4.3 PGPatcher export (GUI)

- `pgp::schema::grass_ignores(key)` is the hide list (R4). `pgp::settings::grass_view(settings, has_s)` drops those keys and adds `subsurface = true` when `has_s` and the key is absent (R2).
- `model::sets::build_views` applies `grass_view` to every `pbr_grass` set; the form, `export_blocks`, `json_entries` and the writer all read that view. `pgp/writer.rs` and `sets_doc.rs` do not change; `pbr_export.rs` does not either.
- `ui::pgp_fields::panel` skips hidden keys for a `pbr_grass` set (empty groups draw nothing); unticking Subsurface there stores `false`.
- The entry is an ordinary TruePBR entry (fixed `default` block unchanged, `subsurface: true` written by the entry when on).

### 4.4 Warnings

| Code | Severity | Where | When | Text |
|---|---|---|---|---|
| `PBR-GRASS-PGPATCHER` | warning | set (`SetResolved.warnings`) | the pick is `pbr_grass` | "PGPatcher 1.2.0 does not patch grass meshes; needs 2.1.0 or later" |
| `PBR-GRASS-DETECT` | warning | job of the colour member | pick `pbr_grass`, member sub-type `pbr_albedo` or `diffuse`, `Analysis.corner_err < 0.03 + 0.01` | "bottom-left texel has \|L-1\| = {e:.3}: Community Shaders' grass shadow pass would read this albedo as complex grass, so shadows will not match; change that texel" |

The 0.01 margin is `GRASS_GUARD_MARGIN`: BC encoding moves the texel, as for complex grass (X2). Codes 90 → 92.

### 4.5 Preview

`SetFingerprint.grass` (the set's pick is `pbr_grass`) shows one amber line in the set preview: "PBR grass approximated: drawn with the object PBR shader (no wrapped diffuse, wind or grass LOD; UV scale and micro-shadows still apply). PGPatcher 1.2.0 does not patch grass meshes." Rendering is unchanged.

## 5. Error handling

- No scan DB, an old-schema DB or a failed read: `gras_all_diffuses` stays empty or keeps its last value (existing `votes_landed` rule); sets pick `pbr`.
- A `pbr_grass` choice under vanilla or CPM falls back to `default` (`SHADER-UNAVAILABLE`); a saved choice survives a target switch, as for `pbr`.
- A `pbr_grass` set with an `_g`/`_p`/`_cnr`/`_f` member: `SHADER-EXTRA-MAP`, still converted.
- Hidden keys in an imported project: dropped from the grass view, kept in the file (R3).
- 0×0 or unreadable corner: `corner_err` is infinite, so no `PBR-GRASS-DETECT`.

## 6. Testing

- Engine unit (`shader.rs`): spec roles and `SHADER-EXTRA-MAP`; auto-pick needs PBR plus GRAS evidence; explicit `pbr` wins; vanilla fallback `wanted: pbr_grass`; lists 14/11/10; `pick_notes` and `pbr_grass_albedo_note`.
- Engine integration (new `crates/engine/tests/pbr_grass.rs`): plan with `gras_all_diffuses`; the set event carries `PBR-GRASS-PGPATCHER`; a blue (unit-vector corner) albedo gets `PBR-GRASS-DETECT`, a brown one does not, an explicit `pbr` gets neither.
- Schema: `cargo test -p engine --test schemas` after regenerating with the CLI.
- modscan (`tests/gras_diffuse.rs`): the new query keeps TruePBR shapes, still ignores a STAT.
- GUI: scan DB with a TruePBR GRAS shape → `m.gras_all_diffuses` filled, `m.gras_diffuses` empty, the set picks `pbr_grass` (`tests/complex_grass.rs`); `grass_ignores`/`grass_view` (`tests/pgp.rs`); a `pbr` set with emissive and `uv_scale` switched to `pbr_grass` exports neither and loses its emissive block (`tests/pbr_export.rs`); dropdown count 14 (`tests/sets_model.rs`); fingerprint flag and badge label (`tests/set_preview_model.rs`, `tests/set_preview_ui.rs`).
- No golden or `gui_repro` test changes: the CLI never auto-picks `pbr_grass` and no preset or output changes. If one fails, stop and ask the owner.

## 7. Files and ownership

| File | Owner rule | Change | Merge-conflict risk vs `main` |
|---|---|---|---|
| `crates/engine/src/shader.rs` | approved (D6) | variant, spec, lists, `AutoRule`, `PickInput.gras`, `pick_for_set` param, two note fns | medium: converter team works in engine; additive arms |
| `crates/engine/src/api.rs` | approved | `BatchRequest::gras_all_diffuses`, `pick_for_set` call, detect note in step 6c | medium |
| `crates/engine/src/codes.rs` | approved | two codes, test counts | medium: any new code there conflicts on the last lines |
| `crates/engine/src/analyse.rs` | **owner question (R1)** | `corner_err` field | low |
| `crates/engine/src/policy.rs` | approved | no change needed | none |
| `schema/*.json` | approved | regenerated | medium: regenerate after any merge |
| `crates/modscan/src/plugin_store.rs` | ours | `gras_all_diffuse_textures` | none |
| `crates/gui/src/model/meshes.rs` | ours | `gras_all_diffuses(db)` | none |
| `crates/gui/src/worker.rs` | shared, add-only | `MeshEvidence.gras_all` | low (audit b added 5 lines) |
| `crates/gui/src/model/mod.rs` | shared, add-only | evidence plumbing (8 one-line adds) | **high** (audit b changed ~100 lines) |
| `crates/gui/src/pgp/schema.rs`, `pgp/settings.rs` | approved | `grass_ignores`, `grass_view` | low |
| `crates/gui/src/model/sets.rs` | approved | `is_pbr_grass`, overlay in `build_views` | low (audit b +5) |
| `crates/gui/src/ui/pgp_fields.rs` | sdc team's, not listed off-limits; small | hide filter, subsurface off = `false` | low (audit b +3) |
| `crates/gui/src/sets_doc.rs`, `model/pbr_export.rs`, `pgp/writer.rs` | approved | no change needed | none |
| `crates/gui/src/model/set_preview.rs`, `ui/set_preview.rs` | ours | `grass` flag, badge | none |
| `docs/gui.md` | shared doc | "PBR grass" section | low |
| `docs/DECISIONS.md` | approved | Note AG | medium (append at end) |

## 8. Open risks

- **PGPatcher 1.2.0 never patches GRAS meshes** (research §3.1): until the owner upgrades to ≥ 2.1.0, a `pbr_grass` entry does nothing in game except for meshes also used by a STAT/FLOR. The warning says so on every set.
- **Grass Lighting off** turns PBR grass into flat t0-only grass in CS; owner chose no warning.
- **Detector false positives on the encoded file:** the check reads the source texel; BC7 can move it. The 0.01 margin covers what complex grass measured; an encoded-file validator is not built.
- **Diffuse override** (research open question 3): if the GRAS form ever binds a different diffuse than NIF slot 0, the auto-pick evidence is wrong. Not seen in owner data.
- **Preview is wrong in four known ways** (Lambert, micro-shadows, UV scale, no wind); the badge says so.
- **Concurrency:** `model/mod.rs` and `codes.rs` will likely conflict when `nif/main` meets `main`; both edits are add-only lines, and `schema/` must be regenerated after that merge.
