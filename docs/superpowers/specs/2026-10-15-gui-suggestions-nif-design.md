# GUI suggestions, NIF side: design spec
Status: APPROVED by owner 2026-10-15, incl. edits to the converter-team GUI files model/viewport3d.rs, ui/viewport3d.rs, settings/layout.rs, tests/viewport3d.rs, tests/pgp.rs AND tests/window_layout.rs, tests/color_fields.rs, tests/set_preview_model.rs, tests/open_in_textures.rs.

Date 2026-10-15. Spec only: no code, branch or commit.
Inputs: `01-research.md` (findings and Owner answers, binding) and `NIF-HANDOFF.md`.
Verified against: `F:\ai\dds\converter-nif-p1`, nif/main **bd495d2**, read-only. CS source: `C:\_mount\moddingvol\Acherus\mods\CommunityShaders_AIO-2026-10-03T06-46Z\Shaders\Lighting.hlsl` ("L:"), read-only.
Line numbers below are from bd495d2.

## Goal
Give the GUI consumer the seven render3d / pgpsim / nif / modscan features it asked for, and ship each one end to end: the API, its consumer change in the GUI, and tests. When this is done the GUI:
- has no per-path magenta swap;
- does not reload to remove the checker;
- has no grey-only background;
- carries no copy of the SLSF tables;
- numbers Meshes rows by PGPatcher's 3D index.

## Owner decisions (2026-10-14, binding)
| # | Decision |
|---|---|
| D1 | The NIF team edits the GUI consumer files itself: `model/viewport3d.rs`, `ui/viewport3d.rs`, `settings/layout.rs`, `tests/viewport3d.rs`, `tests/pgp.rs`. Each feature ships end to end. |
| D2 | pgpsim gets only the four `alphaprop_*` keys. A full PGPatcher 2.2 audit is a later, separate plan. |
| D3 | The missing-texture fill covers colour slots only (base colour t0, env cube t4). Every other slot stays neutral. |
| D4 | The HDRI yaw is shared with the sun: Shift+right-drag turns both. |
| D5 | The shape index bumps the modscan schema from 7 to 8. |

## Anchor check (research vs bd495d2)
Everything the research cites holds. The line shifts and the gaps it glossed over are listed here.
- `SourceSet::get` is `source.rs:36-53`, not 196-212. The file is 53 lines.
- L: has exactly one `return psout;` (L:3200), and `WorkingToUI` (L:3176) comes before it.
  - `material` L:1828, `worldNormal` L:1642/1645, `glossiness` L:1361, `vertexAO` L:2673/2689/2691.
  - All are declared at `main` scope, under `#if` arms that between them cover every permutation.
  - `HAIR`, `FACEGEN`, `FACEGEN_RGB_TINT` and `EYE` set `vertexAO = 1`.
- **One source for both stages.** `Wanted.src` (lib.rs:138-145), `CompileJob.src` (pool.rs:19-26) and `lookup()`'s two `cached()` calls (lib.rs:889-890) all share one `src`. A PS-only patched source would need a second field threaded through `pool.rs`. §1 settles this.
- **`rebuild_device_on` (lib.rs:1058-1070) copies fields by hand, then does `*self = n`.** Every new `Renderer` field must be added there.
- **The GUI builds a fresh `Renderer` in `make` (viewport3d.rs:2027-2033).** The render thread therefore re-sends sticky state on start. The existing pattern is the `SetDebug` re-send at viewport3d.rs:1363.
- **`texture()` cannot tell the real report from the RFAOS `quiet` one** (lib.rs:673-704, 791-797). §3 settles this.
- **pgpsim `json::num` turns booleans into 0/1** (json.rs:22-28). PGPatcher's `unsignedFromJSON` ignores them, so the integer keys need their own parser. `flag()` (json.rs:14) already matches `isEnabled`.
- **modscan has four shape-column SQL sites:**
  - `CREATE TABLE shape` (store.rs:23-28);
  - `pending_shape` (store.rs:106-110);
  - `SHAPE_COLS` plus the `?1..?16` list in `write_shapes` (store.rs:98-99, 597);
  - `commit_pending`'s explicit `INSERT … SELECT` (store.rs:1116-1121).
- **`Look.grey` is read in more places than the research lists:**
  - `tests/color_fields.rs:179`;
  - `tests/window_layout.rs:37, 129, 162, 334`;
  - `tests/viewport3d.rs:1014`;
  - `settings/layout.rs:105, 123`;
  - the luma collapse in `ui/viewport3d.rs:173-191`.
- The colour picker already exists: `ui/color.rs` `color_field` (a swatch plus a `#RRGGBB` box).

---

## 1. Debug views (Albedo, NormalWorld, Roughness, Metallic, AO)

### API (render3d, new `src/debug.rs`, re-exported)
```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum DebugOutput { #[default] Lit, Albedo, NormalWorld, Roughness, Metallic, Ao }

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Availability { Exact, StandIn(&'static str), None }

impl DebugOutput {
    pub const ALL: [DebugOutput; 6];
    pub fn label(self) -> &'static str;
    /// TruePBR: Exact for all. Non-PBR: Albedo/NormalWorld Exact; Roughness StandIn("1 − specular mask");
    /// Metallic None; Ao StandIn("vertex AO"), None for shader types 4, 5, 6, 16 (vertexAO is constant 1).
    pub fn availability(self, m: &MaterialState) -> Availability;
}

impl SourceSet {
    /// The expansion with the debug block inserted before its one `return psout;`, memoised separately.
    /// Err when the text lacks exactly one `return psout;` or any of `MaterialProperties material`,
    /// `worldNormal`, `glossiness`, `vertexAO`.
    pub fn get_debug(&self, rel: &Path) -> Result<Arc<ExpandedSource>, String>;
}

impl Renderer {
    /// Err with nothing changed when `!debug_supported()` and `d != Lit`. Otherwise stores `d`, then `begin_all()`.
    pub fn set_debug_output(&mut self, d: DebugOutput) -> Result<(), String>;
    pub fn debug_output(&self) -> DebugOutput;
    /// `self.sources.get_debug("Lighting.hlsl").is_ok()`.
    pub fn debug_supported(&self) -> bool;
}
```

### Behaviour
- **Inject text.** There is one define, `SDC_DEBUG=<1..5>` (Albedo … Ao). `SDC_DEBUG_PRELINEAR` is added when `encode_srgb` is on. The block below goes before the single `return psout;`:
  ```hlsl
  #if defined(SDC_DEBUG)
  {   float3 sdc;
  #  if SDC_DEBUG == 1
      sdc = material.BaseColor;                       // colour, written as is
  #  else
  #    if SDC_DEBUG == 2
      sdc = worldNormal * 0.5 + 0.5;
  #    elif SDC_DEBUG == 3
  #      if defined(TRUE_PBR)
      sdc = material.Roughness.xxx;
  #      else
      sdc = (1 - glossiness).xxx;
  #      endif
  #    elif SDC_DEBUG == 4
      sdc = material.Metallic.xxx;
  #    else
  #      if defined(TRUE_PBR)
      sdc = (material.AO * vertexAO).xxx;
  #      else
      sdc = vertexAO.xxx;
  #      endif
  #    endif
  #    if defined(SDC_DEBUG_PRELINEAR)  // the readback sRGB-encodes; pre-decode so data shows as its value
      sdc = sdc <= 0.04045 ? sdc / 12.92 : pow((saturate(sdc) + 0.055) / 1.055, 2.4);
  #    endif
  #  endif
      psout.Diffuse.xyz = sdc; }
  #endif
  ```
  - Alpha (`psout.Diffuse.w`) is untouched, so alpha test and blend behave as in Lit.
  - The block's own sRGB EOTF depends on no CS helper name.
- **Keys.** When `debug != Lit` and `get_debug` is Ok, `wanted()` (lib.rs:853) takes `src = get_debug(..)` for **both** stages and appends `("SDC_DEBUG", n)` (and `SDC_DEBUG_PRELINEAR`) to `ps` only.
  - Lit keys and the user's warm disk cache are unchanged.
  - The VS key is the same for all five views, so the cost is one extra VS compile per permutation, shared by the views.
  - `Wanted`, `CompileJob`, `pool.rs` and `program_key` need no change.
  - Rejected: a separate `ps_src` through `Wanted` → `CompileJob` → `pool.rs` → `lookup`. It saves one VS compile but costs four touched structs and functions.
- **Fallback.** After `set_shaders` the debug memo goes with the old `SourceSet`. If the new CS has no anchor, `wanted()` compiles Lit while `debug_output()` keeps the request, and `debug_supported()` turns false. Nothing fails per shape.
- **Switching.** `set_debug_output` → `begin_all()` (lib.rs:564), the same path as a define-set change. The old draw stays on screen while programs compile.
- **`rebuild_device_on`** copies `debug`.

### GUI wiring (`model/viewport3d.rs`, `ui/viewport3d.rs`)
- **`DebugView`** gains `Albedo, Normals, Roughness, Metallic, Ao` next to `Lit, UvChecker`. `fn output(self) -> DebugOutput` maps them; `UvChecker` maps to `Lit` (§4).
- **Support reply.** New `V3Reply::DebugSupport(bool)`. It is sent after `make` (beside `OffInPreview`, viewport3d.rs:2050) and after `SetShaders` (2292). The model stores it.
- **`V3Cmd::SetDebug(d)`** (handled at 2160) calls `r.set_debug_output(d.output())`. An Err goes back as the status line and the model returns to `Lit`.
- **`offered(&self) -> Vec<DebugView>`.** Lit and UvChecker are always offered. Each other view is offered when `debug_support` is true and at least one shape in `V3Reply::Shapes` has `availability != None`.
- **Radio list** (ui 155-165) iterates `offered()`. Each radio's hover text names the stand-in when any shape uses one. The "not offered yet" note goes.
- **The V key** `next()` cycles over `offered()`, not `ALL`.
- **Load replacing an offered view.** If a load leaves the current view not offered, it falls back to `Lit`.
- **Fresh renderer.** `make` runs only at thread start (2042). The existing re-send of `SetDebug` there (1363) re-applies the debug output and the checker override together. `SetShaders` (2108) keeps the renderer (`set_shaders`), and device loss uses `rebuild_device_on` (2069), which copies the new fields. No further re-send is needed.

### Errors
- No anchor: `debug_supported()` is false and the views are not offered.
- A CS compile error in debug mode takes the existing `Failed { showing_previous }` path, per shape.

### Tests
- render3d unit (`debug.rs`), no GPU:
  - inject into a minimal text with the anchors gives Ok, with the block before `return psout;`;
  - zero or two `return psout;`, or a missing name, gives Err;
  - the standin fixture (`tests/fixtures/standin/Lighting.hlsl`) gives Err;
  - an `availability` table test.
- render3d integration, standin:
  - `set_debug_output(Albedo)` is Err and `debug_output()` stays `Lit`;
  - the Lit `ProgramKey` is unchanged with the module present.
- render3d `#[ignore]`, `SDC_CS_SHADERS` (the compile.rs:278 pattern): a TruePBR quad with `roughness_scale = 1` (`PBRParams1.x`, L:1439, multiplies R) and a 1×1 RMAOS `(R=64, G=128, B=192)`. Roughness, Metallic and AO read back as 64 / 128 / 192 ±2 × vertexAO 1, with encode on and with encode off.
- GUI `tests/viewport3d.rs`:
  - 2574: `ALL` gains the five views;
  - `offered()` follows support and availability;
  - `next()` skips views that are not offered;
  - `SetDebug` sends no reload.

---

## 2. HDRI yaw (shared with the sun) and RGB solid background

### API (render3d)
```rust
pub struct Scene { …, /// Environment turn about world +Z, radians (the sun's sense); 0 = today.
    pub env_yaw: f32 }                                   // Scene::neutral(): 0.0
pub enum Background { /// Linear RGB. Solid([f32; 3]),  Hdri }
impl Background {
    pub fn grey(srgb: f32) -> Background;                // Solid([lin; 3]), unchanged meaning
    pub fn rgb(srgb: [f32; 3]) -> Background;            // per channel srgb_to_linear(clamp)
}
impl Studio { pub fn ambient6_at(&self, yaw: f32) -> [[f32; 3]; 6]; }   // AMBIENT_SCALE applied
```

### Behaviour
- **Backdrop.**
  - `Backdrop::draw(gpu, view, encode, yaw: f32)` writes `Flags.y = yaw` (background.rs:201-202).
  - The shader uses `u = frac((atan2(d.y,d.x) + YAW - Flags.y)/2π + 0.5)` (background.rs:42).
  - lib.rs:1670 passes `scene.env_yaw`.
- **Ambient.**
  - `hdri.rs` splits `ambient6` into `cells(img) -> Vec<([f32;3] dir, f64 solid, [f64;3] mean)>` (≤ 64×32) and `integrate(&cells, yaw)`, which takes the cosine lobes about the axes rotated by **−yaw**.
  - `ambient6(img) = integrate(&cells(img), 0.0)`, so the existing tests stay valid.
  - `Studio` caches `cells` once. `Studio.ambient6` stays (= `ambient6_at(0.0)`).
  - `Scene::ambient()` Hdri arm → `studio().ambient6_at(self.env_yaw)`. The cost is about 12k multiply-adds per call, so there is no memo (see the shortcut note in Risks).
- **Change detection.** `Scene != last` (lib.rs:1626) already re-uploads the light rows when `env_yaw` changes.
- **Solid colour.** The clear at lib.rs:1648-1654 goes per channel: `linear_to_srgb` per component when not encoding.

### GUI wiring
- **`Look::scene`** (viewport3d.rs:378): `env_yaw: self.sun_yaw`, and `background: Background::rgb(self.bg)`.
- **`Look::sun_yaw` doc** (370-371): drop "The HDRI does not turn". The HDRI note at ui 218-220 changes to "Shift+right drag turns the sun and the HDRI".
- **`Look.grey: f32` becomes `Look.bg: [f32; 3]`**, sRGB 0..1, default `[0.0; 3]`.
- **`LookSettings`** (layout.rs:101-115) gains `bg: Option<[f32; 3]>` (lenient) and keeps `grey` for reading only. In `clamp` (layout.rs:118), `bg` becomes `None` unless all three channels are finite, and each channel is clamped to 0..=1 (one `map` line; `fix` takes `Option<f32>`).
  - `from_settings`: `bg = s.bg.or(s.grey.map(|g| [g; 3])).unwrap_or(d.bg)`.
  - `of()` writes `bg` and `grey: None`.
- **UI** (ui 172-191): `color_field(ui, id, bg.map(unit_to_u8))` → `bg = rgb.map(u8_to_unit)`. The luma collapse and the "Grey %" slider go: the picker's square and the hex box cover them.

### Tests
- render3d:
  - `ambient6_at(0) == ambient6` (bit-equal);
  - `ambient6_at(π)` swaps +X↔−X and +Y↔−Y within 1e-4, and ±Z is unchanged;
  - the brightest backdrop azimuth at yaw π/2 and the brightest ambient axis turn the same way (the sign test);
  - `tests/background.rs:30, 45-46` and `tests/cs_features.rs:84` move to `[f32; 3]`;
  - a new test: `Background::rgb([1,0,0])` clears to red with encode on and with encode off.
- GUI:
  - `tests/viewport3d.rs:1040` → `Solid([0.0; 3])`; 1014 `l.grey` → `l.bg`;
  - add `look.scene().env_yaw == sun_yaw`;
  - `tests/window_layout.rs` 37/129/162/334 and `tests/color_fields.rs:179` move to `bg`;
  - add a test that an old `grey = 0.25` gui.toml loads as `[0.25; 3]`.

---

## 3. Missing-texture fill (colour slots only, D3)

### API
```rust
impl Renderer {
    /// Some(rgba): a slot path the resolver did not return shows `rgba` (1×1, 2D or cube as the register reads it)
    /// in colour registers only: t0, and t4 when it reflects as Cube. Data registers keep `default_for`.
    /// `Bound` is not changed (material flags stay those of a missing file). Applies from the next load / rebind.
    pub fn set_missing_texture(&mut self, fill: Option<[u8; 4]>);
}
```

### Behaviour
- **`TexEntry.missing: bool`.** `texture()` gains a `report_missing: bool` parameter (it already takes the report).
  - Slot resolves pass `true`. The `None` branch sets `entry.missing = report_missing`.
  - RFAOS guesses (lib.rs:791-797) pass `false`, so t71 never fills.
- **`bind_textures`, default loop** (lib.rs:757-767): if `fill` is set, the register is a colour register, and the slot path mapped to it has `tex_cache[..].missing`, bind `default_srv(kind, fill)` in place of `default_for`. The register is not added to `real`.
  - A colour register is `reg == 0`, or `reg == 4 && kind == SrvKind::Cube`. PBR t4 (`m_p`) and FaceGen t4 (detail) reflect as 2D and stay neutral.
- **`rebind`'s `None` branch** (lib.rs:1444) applies the same rule when a swap drops the last copy of a path that was missing.
- **`rebuild_device_on`** copies `missing_fill`.

### GUI wiring
- `make` calls `r.set_missing_texture(Some([255, 0, 255, 255]))`.
- Delete the magenta loop in `load_all` (viewport3d.rs:1967-1972) and `magenta_dds` (520-523).

### Tests
- render3d (`tests/hotswap.rs` style, standin):
  - a missing diffuse reads the fill;
  - a missing env cube (type 1, slot 4) gets a Cube SRV of the fill. The standin reflects no `TextureCube`, so this test runs `#[ignore]` under `SDC_CS_SHADERS`;
  - a missing normal keeps `(128,128,255)`, and `Bound` does not contain t1;
  - a FaceGen shape with no RFAOS file keeps t71 at `[0,0,0,0]`;
  - `None` gives today's defaults.
- GUI: drop `magenta_is_a_solid_magenta_dds` (tests/viewport3d.rs:2626).

---

## 4. `override_slot` (UV checker by slot)

### API
```rust
impl Renderer {
    /// Shows `dds` in NIF texture slot `slot` of every shape (through each shape's slot→register rule), now and for
    /// later loads, over both variants, even where the slot path is empty. `None` removes it and rebinds the real
    /// textures. Err, with nothing changed, for a DDS that does not decode. Returns the (shape, register) pairs rebound.
    pub fn override_slot(&mut self, slot: usize, dds: Option<&[u8]>) -> Result<Vec<(ShapeId, u32)>, String>;
}
```

### Behaviour
- **Storage.** `overrides: BTreeMap<usize, (Arc<[u8]>, textures::Texture)>`.
  - `load_inner` does not clear it.
  - `rebuild_device_on` re-decodes the bytes on the new device. A decode that fails there drops that override and is reported to stderr; it decoded before, so this is not expected.
- **`bind_textures`.** The slot loop skips the slots under an override. After the default loop, for each override, take `reg = register_for_slot(..)`.
  - If the register reflects with the override's kind, replace that register's entry in `srvs`. This applies even when the path is empty.
  - Otherwise add a `rep.warnings` line in the lib.rs:735 wording.
  - The override is never in `real`/`Bound`, so the flags stay those of the material.
- **Live apply.** `override_slot` does what `texture_targets`/`rebind` do, keyed by slot over the drawing and pending shapes:
  - `Some`: swap in the override SRV, skipping kind mismatches.
  - `None`: restore `tex_cache[path].current()`, or the default / missing fill, exactly as `bind_textures` would.
  - The material is not rewritten (`Bound` does not change).
- **No clobbering.** `texture_targets` (lib.rs:1392) skips (shape, register) pairs under an override, so `replace_texture` and `set_texture_variant` update the entry but not the screen. Clearing the override shows the current entry.

### GUI wiring
- `TexState.checker` stays as the sticky flag.
- `SetDebug(UvChecker)` → `r.override_slot(0, Some(&checker_dds()))`. Any other view → `r.override_slot(0, None)`, with **no reload** (drop `reload |= !on`, 2165).
- After `make`, re-apply the override when `tex.checker` is set (beside the existing re-send at 1363).
- Delete `checker_over` (1980-1988), its call in `load_all` (1974-1976), `diffuse_paths` (526-536) and the `tex.checker && …` refresh at 2281 if nothing else needs it.
- GUI tests: drop the `diffuse_paths` test (2622). Update 2585-2599 and 2699 to "no reload on Lit".

### Tests (render3d)
- The override survives `load`.
- An empty slot 0 shows the checker.
- `set_texture_variant` under an override leaves the SRV unchanged, and `None` then shows the flipped variant.
- `None` restores without a load.
- A 2D override against a cube register is a warning, not an error.

---

## 5. pgpsim `alphaprop_*` (D2)

### API (internal plus the existing public `range_warnings`)
```rust
// check.rs
const INT_KEYS: [(&str, u32); 2] = [("alphaprop_flags", 65535), ("alphaprop_thresh", 255)];
/// unsignedFromJSON: Some only for an integer, or an integral float, in 0..=max. Bools/strings/fractions → None.
pub(crate) fn uint_in(v: &Value, max: u32) -> Option<u32>;
```

### Behaviour
- **Not in `NUM_KEYS`/`BOOL_KEYS`.** These keys never cause a Skip.
- **`range_warnings`.** For an `INT_KEYS` value that is a number but not accepted by `uint_in`, add a warning: `"alphaprop_thresh is 300, outside 0–255 (PGPatcher ignores it)"`. Strings and booleans give no warning (`check_types` ignores the keys).
- **`patch.rs`**, after `zbuffer_write` (patch.rs:143-145):
  ```text
  if flag(e,"alphaprop_delete") { m.alpha = None }
  else if m.alpha.is_some() || flag(e,"alphaprop_create") {
      let old = m.alpha.unwrap_or(Alpha{flags:4844, threshold:128});
      m.alpha = Some(Alpha{ flags: uint_in(flags, 65535).map_or(old.flags, |v| v as u16),
                            threshold: uint_in(thresh, 255).map_or(old.threshold, |v| v as u8) });
  }
  ```
  - Sharing and unsharing do not apply to a per-shape preview.
- **Version.** The lib.rs:1 doc line gains "plus the 2.2.0 `alphaprop_*` keys".

### GUI
- `tests/pgp.rs:145-148`: replace the `Int` skip with probes −1 (one warning), 1 (none) and max+1 (one warning).

### Tests (pgpsim unit)
- In-range values apply.
- 300 / −1 / 1.5 / `"8"` / `true` are ignored and never Skip; the first three warn.
- delete beats create.
- create on a shape with no alpha gives 4844/128, overridden by given values.
- No create and no alpha: nothing.

---

## 6. Shape index (PGPatcher 3D index; D5)

### API
```rust
// nif/src/graph.rs
/// PGPatcher's 3D index (PGNIFUtil::shapesWith3DIdx): pre-order DFS from the root over node `children` (effects are
/// never shapes), first visit wins, iterative, depth ≤ MAX_DEPTH. Every NiShape-family block takes the next index;
/// every NiParticleSystem-family block also takes one (PGPatcher increments for it). Unreachable blocks: absent.
pub fn index3d(header: &Header, bytes: &[u8]) -> BTreeMap<u32, u32>;
// nif/src/scan.rs
pub struct NifScan { …, /// block → 3D index (graph::index3d), filled by `scan`.
    pub index3d: BTreeMap<u32, u32> }
// modscan
pub struct ShapeView { …, pub index3d: Option<u32> }
```

### Behaviour
- **Root.** Block 0 if it is in `NODE_TYPES`, else the first block that is. Children come from `children()` (graph.rs:78).
- **Shape and particle sets** come from `ShapeKind` (shape_head.rs:8-35): `NiParticleSystem` and `BSStripParticleSystem` are the particle family, the rest are shapes. A particle row gets the index it consumes.
- **Why a `NifScan` map rather than a `ShapeRow` field.** It breaks 3 struct literals instead of about 10.
- **Assumption, stated in the doc.** Property, collision and controller refs never reach a shape in Skyrim NIFs, so walking `children` alone matches nifly's `GetChildIndices` order.
- **modscan schema 7 → 8** (store.rs:11). The store is a cache and rebuilds. Add `index3d INTEGER` at:
  - `CREATE TABLE shape`;
  - `pending_shape`;
  - `SHAPE_COLS` and the `?17` placeholder in `write_shapes`;
  - the `commit_pending` column lists;
  - the `shapes_of` SELECT (index 14; `T` becomes 15).
  The column is NULL when the block is unreachable.
- **GUI.** In `ui/meshes.rs` `shape()` (729-745) the header shows `#{index3d}`, or `#– (not in scene tree)` when NULL. The hover keeps `NIF block {block}` and adds "PGPatcher 3D index". The loop counter `i` (535) goes.

### Tests
- nif unit, synthetic NIFs:
  - shapes out of block order are numbered in tree order;
  - a particle consumes an index;
  - an unreachable shape is absent;
  - a shared shape keeps its first visit;
  - a cycle is depth-capped.
- nif: a nif.xml closure test, in the style of `node_types_match_nifxml_closure` over `tests/fixtures/nifxml_inherit.txt`, checks that `ShapeKind` covers the NiShape ∪ NiParticleSystem closures that Skyrim uses.
- nif `#[ignore]`, `SDC_ACHERUS`: reproduce the logged indices of `birdsnest02.nif`, `fireshockwarhammer.nif`, `loadscreenadventure01.nif` and `rbfarmhousewindmill.nif` (01-research §6) from the owner's data, read-only.
- modscan: `shapes_of` round-trips `index3d`; a v7 database is rebuilt.
- Literal fixes:
  - `modscan/tests/store.rs` `scan_of` (76);
  - `gui/tests/complex_grass.rs:69`;
  - `gui/tests/set_preview_model.rs:777`;
  - the `ShapeView` literals in `gui/tests/meshes_view.rs:187, 1576, 1581` and `gui/tests/open_in_textures.rs:14`.

---

## 7. SLSF names in `nif::material`

### API
```rust
// nif/src/material.rs — moved verbatim from gui model/meshes.rs:1441-1512, GUI spellings kept
// (bit 8 "Receive_Shadows", nif.xml "Recieve_Shadows"; SLSF2 bit 23 "Unused01 (TruePBR)").
pub const SLSF1_NAMES: [&str; 32];
pub const SLSF2_NAMES: [&str; 32];
```

### Wiring and tests
- `gui/src/model/meshes.rs` replaces its tables with `pub use nif::material::{SLSF1_NAMES, SLSF2_NAMES};`. `ui/meshes.rs:6` and `tests/meshes_view.rs:1620` then compile unchanged.
- nif unit tests:
  - bit 8 and SLSF2 bit 23 have the documented names;
  - each existing `SLSF*_` constant (material.rs:13-22) names the bit that the table names.
- No nif.xml fixture: the table is moved as it is, and it was already taken from nif.xml.

---

## Files and ownership
| File | Items | Owner |
|---|---|---|
| `crates/render3d/src/{debug.rs (new), source.rs, lib.rs}` | 1, 3, 4 | NIF |
| `crates/render3d/src/{scene.rs, hdri.rs, background.rs}` | 2 | NIF |
| `crates/render3d/tests/{background.rs, cs_features.rs, hotswap.rs, debug.rs (new)}` | 1-4 | NIF |
| `crates/pgpsim/src/{check.rs, patch.rs, lib.rs}` | 5 | NIF |
| `crates/nif/src/{graph.rs, scan.rs, material.rs}` | 6, 7 | NIF |
| `crates/modscan/src/store.rs`, `crates/modscan/tests/store.rs` | 6 | NIF |
| `crates/gui/src/model/meshes.rs`, `ui/meshes.rs`, `tests/meshes_view.rs` | 6, 7 | NIF (CONCURRENCY.md §3, as cited by the research; that file is not in this worktree) |
| `crates/gui/tests/complex_grass.rs` | 6 (literal) | NIF (complex grass work) |
| `crates/gui/src/model/viewport3d.rs`, `ui/viewport3d.rs`, `settings/layout.rs`, `tests/viewport3d.rs`, `tests/pgp.rs` | 1-5 | Converter team; the owner approved NIF edits (D1) |
| `crates/gui/tests/{window_layout.rs, color_fields.rs, set_preview_model.rs, open_in_textures.rs}` | 2 (`grey`→`bg`), 6 (literals) | Converter team; mechanical follow-on edits of D1 features (see Q1) |

## Order
1. Item 7.
2. Item 5.
3. Item 3.
4. Item 2 (yaw and RGB background).
5. Item 4 (after 3: same binding code).
6. Item 6 (independent; can run in parallel with 3-5).
7. Item 1 (after 3 and 4).

Each item is its own `nif/` branch from nif/main and carries its GUI consumer change.

## Risks
- **CS rename (item 1).** The gate fails closed and the views go unoffered. A compile error is per shape.
- **First-use compile latency (item 1).** The first switch to a debug view costs one PS compile per permutation, plus one VS compile per permutation the first time any debug view is used.
- **Non-PBR roughness (item 1)** is a stand-in, not physical. It is labelled in the UI.
- **Yaw sign (item 2)** between backdrop and ambient is caught by the sign test.
- **Ambient per frame (item 2).** `ambient6_at` runs on every `Scene::ambient()` call, at most two calls per frame. shortcut: no memo; add a `(yaw bits, result)` memo if a profile shows it.
- **Override vs swaps (item 4).** Covered by the variant tests.
- **Schema bump (item 6).** Bumping to 8 forces one full rescan. Any other branch that also bumps from 7 must rebase onto 8.
- **Grey slider removed (item 2).** It is a UX change: the colour picker replaces it.

## Open questions (evidence cannot settle)
1. **Test-only literal and rename edits** in converter-team test files outside the D1 list (`tests/window_layout.rs`, `tests/color_fields.rs`, `tests/set_preview_model.rs`, `tests/open_in_textures.rs`): are they covered by D1 ("each feature ships end to end")?
2. **Which copies to pin for the ignored 3D-index test.** For `birdsnest02`, `fireshockwarhammer`, `loadscreenadventure01` and `rbfarmhousewindmill`, the logs name the path but not the mod or BSA that won. The test needs the exact copy PGPatcher read.
