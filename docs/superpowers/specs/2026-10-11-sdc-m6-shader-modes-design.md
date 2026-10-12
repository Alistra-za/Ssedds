# sdc M6 Track A — two output modes and a per-set shader

**Status:** owner-approved design, 2026-10-11 (brainstorm, section by section).
**Parent spec:** `docs/superpowers/specs/2026-10-08-skyrim-dds-converter-design.md`.
**Roadmap source:** phase plan, M6 "Owner direction (2026-10-10): two output modes", plus the red-only owner answer (env mask, `_p` and advanced-skin `_sk` read red only).
**Decisions note:** DECISIONS Note **S** (Q is reserved for the NIF team; R exists).

## 1. Intent

- A mod author picks an output **mode** and, per texture set, the **shader** the mesh uses. sdc then knows which maps belong in the set, what each map's defaults are, and what is extra or missing.
- Success:
  - every set shows a shader that is correct for its maps without the user touching it;
  - changing it changes the per-map defaults predictably;
  - mismatches are visible as warnings, never silent.

## 2. Owner answers (verbatim choices)

| Question | Answer |
|---|---|
| Is "mode" the existing `target`? | **Yes.** `target = cs` is Community Shaders mode, `target = vanilla` is Vanilla/ENB mode. No new axis. |
| Shader list | **As proposed** (§3). |
| Hair alpha | **Cutout + coverage.** |
| Pick and enforce | **Auto-pick, warn only.** Extra maps still export, with a warning. |
| Missing required map | **Warn, write nothing.** No generated placeholder textures. |
| Section 1 (model, auto-pick, effects, GUI) | Approved. |
| Section 2 (red-only BC4, CLI, codes, tests) | Approved. |

`target = cpm` is unchanged by this design: it keeps its current behaviour and gets the Vanilla/ENB shader list.

## 3. Shaders

Engine enum `Shader` (serde snake_case ids in brackets).

| Shader | CS | Vanilla/ENB | Required maps | Optional maps |
|---|---|---|---|---|
| PBR `pbr` | ✅ **default** | — | albedo, `_n`, `_rmaos` | emissive, subsurface, coat (`_cnr`, `_s`), fuzz, `_p` |
| Advanced Skin `advanced_skin` | ✅ | — | `skin_albedo`, `_n`/`_msn`, `skin_rfaos` | `skin_sk`, `skin_wet` |
| Default `default` | ✅ | ✅ **default** | diffuse, `_n` | `_b`/`_bl` backlight |
| Environment Map `envmap` | ✅ | ✅ | diffuse, `_n` | `_m` (env mask), `_e` |
| Complex Material `complex_material` | ✅ | ✅ | diffuse, `_n`, `_m` (complex) | `_e` |
| Glow `glow` | ✅ | ✅ | diffuse, `_n`, `_g` | — |
| Parallax `parallax` | ✅ | ✅ | diffuse, `_n`, `_p` | — |
| Skin `skin` | ✅ | ✅ | diffuse, `_n` or `_msn` | `_sk`, `_s` (only with `_msn`) |
| Hair `hair` | ✅ | ✅ | diffuse, `_n` | — |
| Eye `eye` | ✅ | ✅ | diffuse, `_n` (model-space) | `_sk`, `_m`, `_e` |
| Multilayer Parallax `multilayer` | ✅ | ✅ | diffuse, `_n`, `_i` | `_m`, `_e`, `_b` |
| Landscape `landscape` | ✅ | ✅ | diffuse, `_n` | — |

Excluded: LOD types, Tree Anim, Snow/Sparkle Snow, Cloud, Face Tint (FaceGen data is not a texture set), FO4 types.

- **Map identity:** a map is identified by its member sub-type (and, for the GUI, its slot kind), not by its suffix alone.
- **Environment Map:** a missing `_m` is not a missing required map. The game uses `_n` alpha as the mask.
- **Table location:** the per-shader table lives in one engine module (`shader.rs`, name at planner's discretion) as data. The GUI and CLI read it; neither duplicates it.

## 4. Choosing the shader

1. **Explicit choice wins.** It comes from `[sets."<base>"] shader = "<id>"` in `sdc-project.toml` (GUI), or `--set set:<base>.shader=<id>` (CLI). The planner verifies how sets are addressed in `--set` today and adapts the syntax if a set scope already exists under another name. The ruling goes in Note S.
2. **Auto-pick.** First match wins:
   1. any `skin_*` member, or an Advanced skin set per R-skin-1 → `advanced_skin`
   2. any PBR member (`_rmaos`, `pbr_*` sub-types) → `pbr`
   3. an `_m` member whose sub-type is `complex_material` → `complex_material`
   4. an `_m` member → `envmap`
   5. `_p` → `parallax`
   6. `_g` → `glow`
   7. `_i` → `multilayer`
   8. `_sk` → `skin`
   9. a path component `hair` → `hair`
   10. a path component `eyes` → `eye`
   11. a landscape diffuse or path (the existing landscape rule) → `landscape`
   12. otherwise → `default` (owner, 2026-10-11 plan review: a set with no PBR member never auto-picks `pbr`, even on CS, so plain sets raise no false missing-map warnings). The mode default (`pbr` on CS) is what the dropdown offers first and what an explicit "mode default" means; auto-pick only reaches `pbr` through rule 2.
3. **Unavailable in the mode** (e.g. `pbr` under vanilla, explicit or auto): fall back to the mode's default and raise `SHADER-UNAVAILABLE`.
4. **Provenance:** the result carries a provenance (`set`, `auto:<rule>`, `fallback`) shown by `plan`, `inspect`, the manifest and the GUI.

## 5. What the shader changes

- **Member defaults.** A shader-level default layer sits **above** the sub-type's built-in profile and suggestion, and **below** presets' explicit per-file fields, config, rules and `--set`. The planner places it exactly in the existing precedence chain and records the order in Note S.
  - `hair`: diffuse alpha policy `cutout`, coverage `on`.
  - `eye`: the `_n` member is model-space (`msn` handling, no tangent-space ops).
  - `landscape`: diffuse sub-type `landscape_diffuse` (alpha = snow mask, data).
  - `multilayer`: diffuse alpha policy `data` (owner, 2026-10-11: the alpha is the layer mask, kept exact). `_i` (inner layer) becomes a set suffix suggesting `diffuse` (owner: yes).
  - `envmap`: `_m` is single-channel, format **BC4** (§6).
  - `parallax`: `_p` is single-channel, format **BC4** (§6).
  - Every other shader: the current defaults, unchanged.
- **Extra maps:** a member the shader does not list still converts and exports. `SHADER-EXTRA-MAP` (Warning) names the member and the shader.
- **Missing required maps:** `SHADER-MISSING-MAP` (Warning). Nothing is generated.
- **PGPatcher JSON:** unchanged. It is written for `pbr` and `advanced_skin` sets only, exactly the sets that are PBR today (P5/C1).

## 6. Single-channel maps

| Map | Shader | Default format |
|---|---|---|
| `_m` env mask | `envmap` (and `eye`) | **BC4 UNORM** (red) — new |
| `_p` height | `parallax` | **BC4 UNORM** (red) — new |
| `skin_sk` | `advanced_skin` | BC4 UNORM — unchanged (R-skin-1) |
| `_sk` | `skin`, `eye` (vanilla skin tint) | unchanged, RGB kept |
| `_m` complex | `complex_material` | unchanged (RGB + alpha height) |

A user may still set any available format.

**BC4 on non-grey content:** this reads red only. `CHAN-NOT-GRAY` already reports it.

## 7. GUI

- **Set card:** each set card gets a **Shader** dropdown with the current mode's shaders. The auto-picked value shows "(auto)". Choosing a value writes `shader` to the set in `sdc-project.toml`, and choosing "Auto" removes it.
- **Warnings:** extra and missing map warnings appear on the set card, the same way the existing set warnings do (hover popup).
- **Switching mode** re-resolves every set. Sets whose shader is unavailable show the fallback and the warning.
- **Not in this track:** the Mipmaps Filter dropdown (Track C).

## 8. Codes

| Code | Severity | When |
|---|---|---|
| `SHADER-AUTO` | Info | a set's shader was auto-picked (states the rule) |
| `SHADER-UNAVAILABLE` | Warning | the explicit or auto shader is not in the mode's list; it fell back |
| `SHADER-EXTRA-MAP` | Warning | a member is not used by the shader |
| `SHADER-MISSING-MAP` | Warning | a required map for the shader is absent |

The catalogue count test is updated: merged count + 4.

## 9. Testing

- One auto-pick test per rule in §4.2, including the rule order. Examples:
  - `_m` complex beats `_p`;
  - PBR beats `_p`.
- Explicit choice via project TOML and via `--set`.
- Unavailable fallback: `pbr` under vanilla gives `default` plus `SHADER-UNAVAILABLE`.
- Shader defaults:
  - hair diffuse → cutout + coverage;
  - eye `_n` → model-space;
  - env `_m` and parallax `_p` → BC4;
  - an explicit `--set format` beats the shader default.
- Extra and missing map warnings.
- PGPatcher JSON is unchanged for PBR and Advanced Skin, and absent for the others.
- One golden case per shader for `plan` output.
- GUI model tests:
  - the dropdown lists match the mode;
  - choosing and clearing a shader round-trips in `sdc-project.toml`.

## 10. Versioning

The default BC4 for env `_m` and `_p`, and the hair default, change outputs. Bump the workspace minor version in the final task, so `if-changed` re-encodes.

## 11. Effect on Track D (presets)

The roadmap item "offer or force BC4 for single-channel env mask / red-only `_p`/`_sk`" is delivered here (§6). Track D keeps the rest: the Compact and Fast-preview built-ins, the `presets` verbs, `--preset-file`, migrations, rule tables and the carried preset minors.

## 12. Plan-review amendments (owner, 2026-10-11)

- Rule 12 auto-picks `default`, not the mode default (§4.2).
- Multilayer diffuse alpha is `data`. `_i` joins sets (§5).
- **`_coat` suffix:** a new suffix alias for the PBR coat colour map, the map that `_s` carries today when coat is used. Owner: "_coat (for the coat diffuse)". It is a suffix alias only: same sub-type, same slot, same output name rules as the existing coat colour member. The planner confirms which sub-type that is and records it in Note S.
- **Mesh-reported shader first (owner, 2026-10-11).** When the scanned meshes (the GUI's scan database) report a shader for a set's diffuse, auto-pick uses their strict majority after rules 1–2 (advanced skin, PBR) and before rules 3–12, which stay the fallback; provenance `auto:mesh`. An explicit choice still wins. PBR is never read from a NIF: the TruePBR flag marks PGPatcher's output, and a shipped mesh of a PBR set says Default. The CLI has no mesh evidence. Details: plan S-shader-10. The owner's later answer put complex `_m` and `_p` before the mesh majority as well (see DECISIONS S-shader-10/11).
