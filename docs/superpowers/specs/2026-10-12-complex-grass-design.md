# Complex grass texture subtype — design

Status: approved by the owner 2026-10-12. Brainstormed with the owner the same day. Depends on the plugin scanner (`2026-10-12-esp-scanner-design.md`) for auto-detection only.

## 1. Intent

A new texture subtype, `complex_grass`, so the converter encodes complex-grass textures correctly and warns when one is malformed.

Owner decisions:
- Layout (owner example `F:\ai\dds\nifparseref\grassfieldtundra.png`, 2048×4096): **one image, height = 2 × width, split top/bottom**. Top half: diffuse RGB + **alpha transparency**. Bottom half: normal RGB + **specular in alpha**.
- Applies to **both the CS and ENB targets** (like `complex_material`, a member of both sets).
- Wanted: **encode correctly**, **validate**, **detect automatically — only if there are no false positives whatsoever**; complex grass is rare, so a person can tag it by hand. Preview of the halves is not wanted now.
- Engine edits are allowed for this feature (lifts the nif-branch "engine off-limits" rule for this work only).

## 2. Facts (verified 2026-10-12)

- CS `Shaders/RunGrass.hlsl` (nexus release):
  - Detection: `TexBaseSampler.Load(int3(0, height-1, 0)).xyz * 2 - 1`; complex when `abs(length - 1) < ComplexGrassThreshold` (:621-623). That is **the bottom-left texel of mip 0**.
  - Diffuse/alpha sampled at `uv.y * 0.5`; normal + specular at `0.5 + uv.y * 0.5` (:628, :644, :664).
- ENB: closed source; **assumed to use the same bottom-left texel detector** (owner ruling 2026-10-12), so the §3.6 guard applies to both targets.
- Owner's `ComplexGrassThreshold` = 0.03 (overwrite `SettingsUser.json`, "Grass Lighting").
- Owner example: bottom-left texel (127,126,255,10), length 1.0001; bottom half 100 % unit-length (|L−1| < 0.1) with alpha 10–16; top half 5.5 % unit-length, alpha 0–255.

## 3. Encoding

1. Split the source at `height / 2` into top (diffuse) and bottom (normal) halves.
2. **Top**: the existing diffuse-with-alpha path for vegetation: alpha-coverage-preserving mips (`coverage.rs`), the subtype's alpha policy is cutout/blend as the diffuse rules decide.
3. **Bottom**: normal mips renormalised per level (the existing normal path); alpha is **data** (specular), averaged, never coverage-adjusted.
4. Mips are built per half and stacked per level, so **no filter crosses the seam**. Requires height = 2 × width and power-of-two sizes; the 1:2 ratio keeps the halves separate down to the last level (1×2).
5. One output file, **BC7 UNORM** (not sRGB; the shader treats both halves as raw data in one view).
6. **Detection texel guard**: after encoding, decode the mip-0 block holding texel `(0, height−1)` and test `abs(length(rgb*2−1) − 1) < 0.03 − margin` (margin 0.01, so the file passes any user threshold ≥ 0.02). On failure, re-encode that block at the highest BC7 quality; if it still fails, the file fails with a named error (code in the `VAL-` family), never ships silently.
7. Vanilla target: no complex-grass support in the vanilla shader. Same handling as `complex_material` on vanilla: the subtype is not offered for the vanilla target; a `complex_grass` tag there produces a warning and the file encodes as `complex_grass` unchanged.

## 4. Validation (warnings unless stated)

- Not exactly 1:2 (height ≠ 2 × width) or not power of two → **error** (cannot encode per §3.4).
- Top half alpha is fully opaque (no transparency).
- Fewer than 99 % of bottom-half texels are unit-length normals (|L−1| < 0.1).
- Source bottom-left texel already fails CS's test (`|L−1| ≥ 0.03`): the game will not detect it as complex even before compression.

## 5. Auto-detection (no false positives)

Tag `complex_grass` automatically **only when all four hold**; otherwise leave the existing classification and let the user tag it:
1. The plugin scanner finds the texture as the diffuse (slot 0) of a NIF used by a **GRAS** record in the active load order.
2. Height = 2 × width.
3. The source bottom-left texel passes CS's test with the 0.03 threshold.
4. ≥ 99 % of bottom-half texels are unit-length normals.

A user tag always wins (both ways). Until the plugin scanner has landed, condition 1 cannot hold, so nothing is auto-tagged.

## 6. Testing

- Engine: synthetic 1:2 images (top coverage alpha, bottom unit normals with data alpha): per-half mips (no seam bleed: a top-half texel never contributes to a bottom-half mip texel and vice versa), coverage preserved on top, renormalised bottom, alpha untouched on bottom; BC7 UNORM output.
- Detection guard: a source whose bottom-left block compresses badly → re-encode path; an impossible one → named error.
- Validation: each §4 case.
- Auto-detect: each condition false alone → not tagged; all true → tagged; user tag overrides.
- `#[ignore]` + `SDC_ACHERUS`: the owner example encodes, the guard passes, and the decoded bottom-left texel passes the 0.03 test.

## 7. Constraints

nif/ branch in its own worktree from nif/main; never push or merge to main; shared files add-only in separate commits; **`crates/engine` editable for this feature only**; other off-limits paths unchanged (`cli`, `encoders-sys`, `gui/src/pgp`, `sets_doc.rs`, `model/sets.rs`, `model/pbr_export.rs`, `schema/` — if the subtype id must be added to `schema/`, that one addition is flagged to the owner in the plan, `docs/DECISIONS.md`, golden/gui_repro tests). Owner data read-only.
