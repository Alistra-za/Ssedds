# sdc M6 Track E — NIF-aware alpha and fewer confirmations: design

Owner-approved in chat on 2026-10-12. Base: `main` 1b5f571 (Track D and the cloud fixes are merged).

## 1. Intent

**The owner said:**
- "a confident suffix match e.g. _rfaos _RFAOS _rmaos _RMaos should not need confirmation in the gui; if people don't follow naming conventions they should have to fix it themselves".
- "a texture set with these maps should also confirm that _n for the same texture set is a pbr normal, diffuse is pbr — etc".
- "_rfaos is its own thing, advanced skin".
- NIF-aware alpha, with these rules:
  - any mesh with alpha beats meshes without it;
  - blend on one mesh and test on another gives `cutout_blend`;
  - blend and test on the same mesh gives `cutout_blend`;
  - blend everywhere gives `blend`;
  - test everywhere gives `cutout`;
  - no mesh using alpha gives `remove`;
  - the mesh alpha-test threshold feeds the cutout threshold.

**Success:**
- Opening a folder of correctly named sets in the GUI asks nothing for the High-confidence members. A PBR set's `_rmaos`, `_n`, and bare colour member all resolve without a click.
- Medium and Low suggestions still ask.
- A diffuse with varying alpha whose meshes are in the scan database gets its alpha policy from those meshes, and the alpha question is not asked.
- The CLI's behaviour is unchanged: it still needs `--accept-suggestions`. When the CLI is given no mesh evidence, it behaves as today.

**Out of scope:**
- changing `suggest::is_pbr_set`;
- the meaning of advanced-skin members;
- a CLI way to pass mesh alpha;
- reading NIFs directly in the engine.

## 2. GUI accepts High suggestions

- `crates/gui/src/model/mod.rs` `AppModel::request` sets `req.accept = AcceptSuggestions::High` instead of `None`.
- Medium and Low suggestions stay unresolved and keep their existing confirm UI.
- Any explicit choice the user makes in the GUI still wins, as today. A user who wants to confirm everything can still change a member's sub-type by hand.
- **Visible "auto" marker:** a member whose sub-type was decided by an accepted suggestion shows "(auto)" next to its sub-type. Hovering shows the suggestion's reason, e.g. "_rmaos", "_n in a PBR set". The engine already records this provenance. The plan finds the exact field (the sub-type provenance for an accepted suggestion) and uses it. No new engine field is added unless none exists.

## 3. Bare colour member of a PBR set is High

- `crates/engine/src/suggest.rs`: the arm `Some("d") | None if pbr` gives `PbrAlbedo` at **High** when the set has a PBR marker member: `_rmaos`, `_cnr`/`_crn` or `_f`, or a raw packing input that starts the `_rmaos` recipe. The reason text is "colour member of a PBR set (the set has _rmaos/_cnr/_f)".
- A PBR set recognised only by a `pbr` path component keeps today's `colour` confidence: High for `_d`, Medium for a bare stem. A folder name is weaker evidence than a member map.
- **`_rfaos` is advanced skin, its own thing (owner).** It is not a marker for this rule. An `_rfaos` set's colour member is already `SkinAlbedo` at High through the `advanced_skin` arm, which comes before the `pbr` arm, and this track does not change that. `is_pbr_set` is unchanged.
- Already High today, and unchanged:
  - `_n` in a PBR set (`PbrNormal`);
  - `_g` in a PBR set;
  - `_s` in a PBR set;
  - `_rmaos`;
  - `_rfaos`;
  - `_sk` and `_wet` in an advanced skin set.

## 4. NIF-aware alpha

### 4.1 Evidence

- **New request field:** `BatchRequest::mesh_alpha: BTreeMap<String, Vec<MeshAlpha>>`, keyed by `shader::colour_mesh_key` (the Track A key), and empty by default. Its doc says the GUI fills it from the scan database and the CLI sends none.
- `pub struct MeshAlpha { pub blend: bool, pub test: bool, pub threshold: u8 }` holds one entry per lighting shape that uses the colour member in slot 0.
- **GUI:** `meshes::mesh_alpha(db, keys)` sits beside `meshes::mesh_votes`, with the same source rules: lighting shapes, slot 0, non-derived copies of enabled origins, TruePBR excluded.
  - It reads `alpha_flags` and `alpha_threshold` from the shape table.
  - NiAlphaProperty flag bits: blend enable is bit 0, test enable is bit 9.
  - A shape with no alpha property counts as `{blend: false, test: false}`.
  - It is raw SQL on `Store::conn()` with a `shortcut:` comment, as in Track A.
  - `refresh_mesh_votes` fills both maps in one pass.

### 4.2 Rule

`shader::mesh_alpha_policy(&[MeshAlpha]) -> Option<(AlphaPolicy, Option<u8>)>`, with the owner's table:

| Shapes using the texture | Policy | Threshold |
|---|---|---|
| no entries (no evidence) | `None`: today's behaviour | — |
| none uses blend or test | `remove` | — |
| some blend, none test | `blend` | — |
| some test, none blend | `cutout` | the largest threshold among the test shapes |
| any blend and any test, on the same shape or on different shapes | `cutout_blend` | the largest threshold among the test shapes |

"Any alpha beats no alpha" means that shapes without alpha are ignored once any shape uses alpha. There is no majority vote. Ruling: the largest threshold is the conservative cutoff; if shapes disagree, the stricter one wins. The plan records this in Note U.

### 4.3 Where it applies

- **Sub-types:** only those whose alpha comes from the mesh (`Subtype::alpha_from_mesh`: diffuse, pbr_albedo, ui_sky, skin_albedo), and only for the set's colour member.
- **Layer:** a new provenance `mesh:alpha`.
  - It sits above the built-in profile and suggestion defaults and above analysis facts.
  - It sits below explicit layers (user presets, configs, rules, `[files]`, `--set`), as the shader-default layer does (Note S-shader-3).
  - It counts as explicit, so the alpha question (`policy.rs` ~1720, `alpha_policy_explicit`) is not asked.
  - `alpha.threshold` is written with the same provenance when the rule gives one.
- **Source without alpha:** when the source image has no alpha channel, the mesh policy is ignored, because there is nothing to keep. Analysis already says `remove`.
- **No evidence:** nothing changes, and the question is asked as today.

## 5. Codes, version, docs

- **No new codes.** A mesh-decided alpha is visible through `mesh:alpha` provenance in plan, inspect, the manifest and the GUI.
- **Version:** workspace 0.5.0 → 0.6.0. Mesh-decided alpha changes outputs, so `if-changed` re-encodes.
- **DECISIONS Note U** gets one bullet per ruling. A short `docs/gui.md` note covers "(auto)" and mesh-decided alpha.

## 6. Tests

- **Model:** `request()` sets `accept = High`; a GUI test shows a PBR set's members resolved with no confirm prompt, and a Medium member still unresolved.
- **"(auto)":** a GUI test shows it renders, with the reason as hover text.
- **Suggestions:**
  - bare stem plus `_rmaos` gives High `pbr_albedo`;
  - bare stem with only a `pbr` folder stays Medium;
  - bare stem plus `_rfaos` gives High `skin_albedo` (unchanged);
  - upper-case `_RMAOS` / `_RFAOS` resolve the same as lower-case.
- **`mesh_alpha_policy`:** every row of §4.2, plus a mixed set with one shape without alpha and one with blend, which gives `blend`.
- **Planning:** a varying-alpha diffuse with mesh evidence gets the policy (provenance `mesh:alpha`) and the threshold, and is not asked the question. Without evidence, it is asked as today. An explicit `--set` alpha beats the mesh.
- **GUI DB read:** a store with shapes carrying alpha flags yields the expected `MeshAlpha` entries, and a TruePBR shape is excluded.

## 7. Review focus

1. A texture used by both a hair-style test mesh and a blended mesh in another mod must give `cutout_blend`, not a majority result.
2. A mesh-decided `cutout` on a source whose alpha is all 255 must still go through the existing constant-alpha checks, not silently write BC7 alpha.
3. The GUI's switch to accept High must not auto-accept a suggestion the user had explicitly overridden in `sdc-project.toml`.
4. A set whose colour member was renamed by the vocabulary (`x_albedo.png`) must use its canonical output stem for the mesh key, as `colour_mesh_key` does.
5. Compact's BC1 rule matches source alpha content. A mesh-decided `remove` on a varying-alpha diffuse must still end up BC1-compatible: the rule is `alpha = "none"` / `"constant255"` only. Record whether that diffuse stays BC7 under Compact, and say so in Note U.
