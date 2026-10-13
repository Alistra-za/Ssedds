# sdc M6 Track F2: RNM normal layers (design)

Owner-approved in chat on 2026-10-12, section by section. Base: `main` 00db642. The reference script is `F:\ai\bake\rnm\rnm.py`; it is a reference, not an authority (owner).

## 1. Intent

**The owner said:**
- These suffixes are assigned automatically to an RNM section in the normals slot, under compression settings: `_bump _bump2 _bumpmask _bumpmask2 _detailnorm _detailnorm2 _detailnormmask _detailnormmask2 _bent _bentmask`.
- The section has an RNM toggle and strength sliders, and the layer order is fluid.
- These suffixes appear nowhere else in the UI.
- Bent normals get a toggle between rnm.py's AO-weighted blend (for a normal and a bent normal baked from the same high→low source) and treating the bent normal like a detail normal.
- `_cnr`: the base normal's own alpha (coat roughness) is removed before blending and put back unchanged.
- AO can come from `_ao` or from the AO channel of `_rmaos`/`_rfaos`. `_ao` stays visible for channel packing.
- Owner answers (2026-10-12):
  - The output is the base normal's size.
  - RNM is on when layers are found, and the normals slot gets an outline so that is obvious.
  - Strength is set per layer.
  - No base normal means a warning and no output.
  - Eye sets (model-space `_n`, the `_e` envmap) are ignored.
  - Layers feed `_n`, `_cnr` or both: plain layers default to `_n`, prefixed ones (`_cnr_bump`) to their base, and either can be shared.
  - Every mask, AO included, can be switched off.
  - AO is displayed inverted, with a note saying not to invert it yourself.

**Success:**
- A folder holding `body_n.png`, `body_detailnorm.png` and `body_detailnormmask.png` converts to one `body_n.dds` with the detail blended in. No `body_detailnorm*.dds` is written, and the GUI shows the layers only in the RNM section.
- Moving a slider re-encodes the normal.
- A `_cnr` keeps its alpha bytes exactly.

**Out of scope:**
- A separate height mode. Ruling: height and bump differ only by a scale factor, which the bump strength covers. Bump slope is computed at the bump's own resolution (§3).
- Tiled (UV-scaled) detail normals.
- RNM for model-space normals.
- Synthesising a normal with no base.
- Bulk editing (F3).

## 2. Files and suffixes

### 2.1 Vocabulary

A new vocabulary role, **normal layer**, is added to `group.rs` `VOCABULARY`. Matching is case-insensitive.

| Layer id | Layer file | Mask file | Aliases (rnm.py) |
|---|---|---|---|
| `bump` | `_bump` | `_bumpmask` | `_bump_mask` |
| `bump2` | `_bump2` | `_bumpmask2` | `_bump2_mask` |
| `detail` | `_detailnorm` | `_detailnormmask` | `_detail`, `_detail_mask` |
| `detail2` | `_detailnorm2` | `_detailnormmask2` | `_detail2`, `_detail2_mask` |
| `bent` | `_bent` | `_bentmask` | `_bent_mask` |

- `_norm` becomes an alias of `_n`.
- Every layer and mask suffix also takes a base prefix: `_n_<suffix>` or `_cnr_<suffix>`. For example `_cnr_bump`, `_cnr_bumpmask` and `_n_detailnorm2`. The prefix sets the default target (§2.3).
- `_ao` keeps its existing packing role and is also read as the bent AO source (§3.3).

### 2.2 Grouping and hiding

- Layer and mask files join their set by base name, the same way packing inputs do (`group.rs`).
- They get a new job status, **`NormalLayer`**, like `PackingInput`:
  - they are never converted on their own;
  - they get no set card and no `PackedInto` row;
  - they appear only in the RNM section.
- **Bug this fixes:** today `foo_bump.png` has no vocabulary match. It becomes its own set with a bare stem, and since Track E it is auto-accepted at High as a diffuse. A test pins the fix: the file joins `foo`'s set, gets the hidden status, and never becomes a colour output. An existing manifest may still list a stale `foo_bump` colour output. That is noted in `docs/presets-changelog.md`; sdc deletes nothing.

### 2.3 Base

- **The base** is the set's tangent-space normal: sub-type `normal`, `pbr_normal`, `coat_normal` (`_cnr`) or `landscape_normal`.
- **Never a base:**
  - `msn`;
  - any member of a set whose resolved shader is **Eye** (its `_n` resolves to `msn`, S-shader-4);
  - a cubemap, such as the eye `_e` envmap.
- **Eye set:** its layer files stay hidden and unconverted, and code `RNM-IGNORED-EYE` names them.
- **No base:** the set's layers stay hidden and unconverted, and code `RNM-NO-BASE` is raised. Nothing is synthesised.
- **Which base a layer feeds** (owner, 2026-10-12):
  - Every layer file has a **target**: `n`, `cnr` or `both`. Its mask follows it.
  - **Prefixed layers** like `_cnr_bump`, `_cnr_detailnorm` and `_cnr_bentmask` default to `cnr`. `_n_` prefixes (e.g. `_n_bump`) default to `n`.
  - **Plain layers** (`_bump`) default to `n`.
  - Any layer can be switched to any target. This allows sharing: a plain `_bump` or a `_cnr_bump` can be set to `both`.
  - A target that names a base the set doesn't have is ignored for that base. If a plain layer's default base `_n` is missing but a `_cnr` exists, the layer feeds `_cnr`.
  - **Same layer id twice in one stack** (e.g. `_bump` set to `both` plus a `_cnr_bump` in the `_cnr` stack): the file whose prefix names that base wins, and the other is left out of that stack with code `RNM-LAYER-SHADOWED`.
  - Each base has its own stack and its own `rnm.*` settings (§4). A shared layer can have a different strength in each stack.
- **Mask without its layer:** it is ignored, with code `RNM-MASK-ORPHAN`.

## 3. Blend

A new engine job, `NormalStackJob`, sits beside `PackJob` and `BakeJob`. The pipeline swaps it in where `assemble` replaces `load_image` (`pipeline.rs` ~204).

**Rejected alternatives:**
- extending the packing `Recipe`, which copies single channels while RNM needs 3-channel vector maths;
- a pre-pass that writes an intermediate file, which adds I/O and makes staleness track a temp file.

### 3.1 Steps (per base, when `rnm.enabled`)

1. Decode the base to f32 RGBA (`load.rs`). Lift off and keep its alpha.
2. Decode each layer as a tangent normal at strength `s`: `t = (2c−1)·s`, `n = normalize(t.x, t.y, |t.z| + 1 − s)`. When the mask is on, `s` is multiplied per pixel by the mask's red channel.
3. **Bump layers** (`bump`, `bump2`):
   - Read the height as the mean of RGB.
   - Compute the slope at the bump's own resolution by central difference, with `k = strength`.
   - Encode it as a normal, then resample.
   - The bump mask scales the slope exactly.
4. **Resample** every layer, mask and AO to the base size:
   - downscale with the existing area filter (`mips::area_resample`);
   - upscale with a new bilinear filter.
   - Aspect differences are allowed, because each axis resamples independently.
5. **Apply the layers in `rnm.order`.** The default is `bent, bump, bump2, detail, detail2`; layers with no file are skipped.
   - **Normal layers:** RNM (Barre-Brisebois & Hill) reorients the layer onto the result so far.
   - **`bent`, mode `ao_blend` (default):**
     - normalised lerp toward the bent normal with weight `w = strength × occlusion^ao_power × mask`;
     - `occlusion = 1 − AO`, and `ao_power` defaults to 1.5;
     - the bent default strength is 0.35.
   - **`bent`, mode `detail`:** RNM, like a detail layer.
6. Re-encode RGB. Put the original alpha back byte-for-byte when the source alpha was 8-bit, and as the exact float otherwise.
7. Hand off to the existing normal path, unchanged: `mips.normal`, `normal_base` renormalising, `_cnr` `coat_self_bake`, then compression.

### 3.2 Green channel

- The blend runs in DirectX convention, the same as the output.
- Each layer has `flip_green` (OpenGL source). Its default is the base's own green setting.

### 3.3 AO source (bent, `ao_blend`)

**Order:**
1. the set's `_ao` file;
2. otherwise the **B channel** of the set's `_rmaos`/`_rfaos` source (R roughness, G metallic or fuzz, B AO, A specular; `pack/mod.rs`);
3. otherwise a uniform weight, with code `RNM-AO-MISSING` (info).

**Rules:**
- `rnm.bent.ao = false` switches AO off, which gives a uniform weight with no code.
- AO is read as white = open, as supplied. sdc does the inversion. Analysis's existing `CHAN-AO-INVERTED` still applies to the AO file.

### 3.4 Defaults

| Layer | Strength |
|---|---|
| `bump` | 0.4 |
| `bump2` | 0.4 |
| `detail` | 0.6 |
| `detail2` | 0.3 |
| `bent` | 0.35 |

- The base is always 1.0, with no slider.
- Every mask defaults to on when its file exists.

## 4. Settings and staleness

- **Location:** the keys live on the base normal's `[files]` entry in `sdc.toml`. They are also settable by presets, rules, user config and `--set`, with the usual provenance layers.
  - `rnm.enabled`: bool, default `true`.
  - `rnm.order`: a list of layer ids. Unknown ids are refused by `check_profile`, and missing ids are appended in the default order.
  - per layer: `rnm.<layer>.strength` (f32, minimum 0; ruling: maximum 2), `rnm.<layer>.flip_green` (bool) and `rnm.<layer>.mask` (bool).
  - bent only: `rnm.bent.mode` (`ao_blend` | `detail`), `rnm.bent.ao_power` (f32, 0.1..=8) and `rnm.bent.ao` (bool).
  - On the **layer file's** own `[files]` entry: `rnm.target` (`n` | `cnr` | `both`). The default comes from the prefix (§2.3). A changed target re-encodes every base it adds or removes.
- **Staleness:**
  - Layer files, mask files and the AO source join the job's hashed source list, in order, with their roles.
  - Every `rnm.*` key joins the hashed `Profile`.
  - Any edit re-encodes the normal. Review Focus 1.
- **Reporting:**
  - `plan`, `inspect` and the manifest list each layer, its file and its settings.
  - `explain` covers the new codes.
- **New codes:** `RNM-NO-BASE`, `RNM-IGNORED-EYE`, `RNM-MASK-ORPHAN`, `RNM-AO-MISSING`, `RNM-LAYER-SHADOWED`. Catalogue 75 → 80.
- **Version:** 0.6.0 → 0.7.0. DECISIONS **Note W** gets one bullet per ruling.

## 5. GUI

### 5.1 Normals slot card

When RNM is active (layers present and enabled), the card gets an outline. Its hover text lists the layers in order.

### 5.2 RNM section

The section sits in the settings panel, under the normal's compression settings. It shows only when the set has layers.

- **Header:** "RNM" with the `rnm.enabled` toggle.
- **One row per layer, in `rnm.order`:**
  - a thumbnail and the layer name (bump, bump 2, detail, detail 2, bent);
  - a strength slider (0–2);
  - "flip green" toggle;
  - a mask thumbnail with an on/off toggle, or "no mask";
  - ▲▼ to reorder;
  - a **target** switch: "Normal" / "CNR" / "Both". It is shown only when the set has both `_n` and `_cnr`, and writes `rnm.target` on the layer file.
- **Two stacks:** a set with both `_n` and `_cnr` gets one RNM section per base, each listing the layers that feed it.
- **Bent row, extra controls:**
  - the mode switch "AO blend" / "Like a detail";
  - the AO power slider;
  - the AO source line: its name (`_ao` or `_rmaos` B), an **inverted** thumbnail (white = bent most), and an on/off toggle.
  - The note under the AO thumbnail: "sdc inverts this for you. Supply normal AO (white = open); don't invert it yourself."
- **Warnings** show in the section: `RNM-NO-BASE`, `RNM-IGNORED-EYE`, `RNM-MASK-ORPHAN` and `RNM-AO-MISSING`.
- **Hiding:** layer and mask files appear nowhere else in the GUI, including set cards, item lists and the unassigned list.

## 6. Tests

- **Vocabulary:**
  - every suffix and alias in §2.1 groups into the base's set with status `NormalLayer`;
  - case-insensitive;
  - `foo_bump.png` never becomes a colour output.
- **Base selection:**
  - `_n` only;
  - in a set with `_n` + `_cnr`:
    - plain `_bump` feeds only `_n`;
    - `_cnr_bump` feeds only `_cnr`;
    - `rnm.target = both` feeds both;
    - `_bump` set to `both` plus a `_cnr_bump` gives the `_cnr` stack only `_cnr_bump`, with `RNM-LAYER-SHADOWED`;
  - plain `_bump` with only a `_cnr` feeds `_cnr`;
  - a shared layer has independent strengths per stack;
  - an eye set gives `RNM-IGNORED-EYE`;
  - no base gives `RNM-NO-BASE`;
  - an orphan mask gives `RNM-MASK-ORPHAN`.
- **Maths:**
  - A flat layer leaves the base unchanged (within 8-bit rounding).
  - Strength 0 equals no layer.
  - A black mask equals no layer, and a white mask equals mask off.
  - The bent weight follows `0.35·(1−AO)^1.5` at AO 1, 0.5 and 0.
  - Mode `detail` equals RNM.
  - `flip_green` on an OpenGL layer matches the DirectX layer.
- **Resampling:**
  - A 2× bigger layer and a 2× smaller layer both produce base-size output.
  - A bump's slope is computed before resampling, so an 8K bump on a 2K base and on a 4K base give the same angles within tolerance.
- **Alpha:** a `_cnr` with varying alpha keeps its alpha bytes exactly after blending.
- **AO source:**
  - `_ao` beats `_rmaos` B;
  - `_rmaos` B is used when there is no `_ao`;
  - neither gives `RNM-AO-MISSING`;
  - `rnm.bent.ao = false` gives a uniform weight.
- **Staleness:** changing each `rnm.*` key, and touching a layer file, marks the output stale. An unrelated edit does not.
- **GUI:**
  - the outline renders when RNM is active;
  - the section renders rows in order, and ▲▼ reorders and writes `rnm.order`;
  - layer files appear in no item list;
  - the AO note renders.

## 7. Review focus

1. A slider, toggle or order edit that does not re-encode the normal: every `rnm.*` key must be in the hashed profile.
2. A layer file reaching the encoder as its own output, through some status path that skips `NormalLayer` (CLI single-file convert, `--only`, a GUI item list).
3. `_cnr` alpha altered by the blend or by resampling. Alpha must never pass through the vector maths.
4. Layer sizes with different aspect ratios from the base, and non-power-of-two sizes, must resample without panics or off-by-one edges.
5. Bump sign convention: the bump's "up" must match the DirectX green after `flip_green`. Raised areas must light from the same side as in the base.
