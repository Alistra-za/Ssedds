# Audit: GUI drawing side (`crates/gui/src/ui`, `crates/gui/src/render`) and the 3D renderer (`crates/render3d`)

Scope read: every file under `crates/render3d/src` (all 42 `unsafe` sites traced), `crates/gui/src/render/*`, and the
hot UI files (`ui/mod.rs`, `viewport3d.rs`, `preview.rs`, `set_preview.rs`, `columns.rs`, `sets_column.rs`, `actions.rs`,
`meshes.rs`, `thumbs.rs`, `dialogs.rs` (part), `set_editor.rs` (part), `settings_panel.rs` (part), `slot_commands.rs`,
`rnm_section.rs`, `bulk_panel.rs`, `cm_card.rs`, `skin_card.rs`, `color.rs`, `path_picker.rs`, `header.rs`, `drawer.rs`
(part)). The render thread in `model/viewport3d.rs` was traced for device loss and compile failure handling.

**Unsafe audit result (render3d, 42 sites): no invariant violation found.** Each D3D11 call's pointer arguments are locals
or slices that outlive the call; `D3D11_SUBRESOURCE_DATA` pitches match what the DDS parser laid the levels out with
(`engine::dds::read_dds` computes every level range with the same `level_size`, and `load_dds_as` bounds-checks each
face/mip slice before handing D3D the pointer); the readback reads `w*8` bytes per row inside `RowPitch` and `Unmap`s on
every path; cbuffer `UpdateSubresource` writes exactly the buffer's `ByteWidth` (`CbWriter::new` pads to 16 and the
descriptor uses the same length); `D3DReflect` indices are bounded by the shader desc; the HDRI upload's `half` buffer is
`w*h*4` halves because `decode_rgbe` pushes exactly `w` pixels per row. The debug-view injection (`debug.rs`) refuses
(does not panic) when the user's `Lighting.hlsl` lacks the single `return psout;` or any of the four declarations, and a
block that then fails to compile is a per-shape `Failed { showing_previous: true }`, not a crash. Device removal is
detected (`Gpu::fail` prefixes `DEVICE_REMOVED`), the render thread rebuilds once (WARP fallback) and reloads with the
recorded edits and swaps. `wait_idle`'s 200 µs sleep is fine: the workspace is Rust ≥ 1.88, whose Windows `sleep` uses
high-resolution timers.

The findings below are therefore about scale and small bugs, not memory safety.

---

1. **The Sets column lays out every set pill every frame and walks `model.sets` once per pill** (`crates/gui/src/ui/sets_column.rs:140-205`, `:578-592`)
   - **What this is:** The Sets column draws its pills inside `egui::ScrollArea::vertical().show(...)`, which lays out
     every pill whether or not it is on screen (egui is immediate mode: layout runs each repaint, only painting is
     clipped). Inside each pill, the recipe lines are found with `model.sets.iter().filter(|s| s.key == v.key)`, a
     linear scan of all sets.
   - **Problem:** A project with 800 sets (a few thousand textures) repaints 800 pills × (a `ComboBox`, 3-6 badges, the
     member thumbnails, a context menu, a warnings `Vec` collected per pill) ≈ 10k widgets, plus 800 × 800 = 640k
     `String` compares for the recipe lookup, on every repaint. While the user orbits the 3D set preview (60
     repaints/s) or drags a slider, the UI thread spends most of its 16 ms budget in a column that did not change. The
     Textures column was virtualised with `show_rows` for exactly this reason; the Sets column was not.
   - **Fix:** Carry the recipes on the `SetView` (it is built from `m.sets` in `build_views`, so no lookup is needed), and
     virtualise the pills: remember each pill's height from the last frame (`rects` already holds them; keep them in
     `DropState` or `ui.data`) and, for a pill whose remembered rect does not intersect `ui.clip_rect()`, `ui.allocate_space`
     that height instead of drawing it. The drag-and-drop targets and `reveal` scroll keep working because the rects stay
     stable.
   - **If we skip it:** The Textures view gets slower in proportion to the set count; with 1000+ sets every drag in the
     app stutters even though nothing in the Sets column changed.
   - **Group:** Should fix. **Kind:** scale / speed.
   - **Confidence:** medium (mechanism certain from the code; the millisecond figures are estimated, not profiled; grepped
     `show_rows|show_viewport` in `ui/`: used by columns, drawer, meshes, actions, never by sets_column).

2. **The set editor and the JSON editor run `delete_hits` over every set on every frame** (`crates/gui/src/ui/set_editor.rs:123-141`, `crates/gui/src/ui/json_editor.rs:154`; the callee `crates/gui/src/model/sections.rs:108-150`)
   - **What this is:** `delete_hits(model)` decides which PGPatcher *delete* sections hit which sets. It is not cached:
     for every written delete section it loops over every `SetView`, builds that set's slot paths, and for each match
     rule lowercases/`replace`s path strings. The set editor calls it every frame for the selected set, then throws away
     every hit but that set's; the JSON editor window calls it every frame too.
   - **Problem:** A modder with 5 delete sections and 800 sets: 5 × 800 set visits, each allocating the slot-path list
     and several lowercased copies ≈ 20-30k string allocations per frame while a set is selected (and double that
     with the JSON editor open). A hover over the set editor becomes a visibly slower frame; nothing in the result
     changes between frames.
   - **Fix:** Cache it like `views`/`export_blocks` are (a `hits: Option<Arc<Vec<DeleteHit>>>` slot in `ViewCache`,
     keyed by `view_key()`), or in `set_editor::draw` compute only the selected set (`delete_hits_for(m, &v)` that takes
     the one view). The former is two lines in `ViewCache` plus the same get-or-build shape `export_blocks` uses.
   - **If we skip it:** Fine without delete sections; with them the set editor's per-frame cost grows with the number
     of sets.
   - **Group:** Should fix. **Kind:** scale / speed.
   - **Confidence:** medium-high (read both call sites and the callee; `sections.is_empty()` short-circuits, so only
     projects using delete sections pay).

3. **No test loads a multi-mip or block-compressed DDS through `load_dds_as`** (`crates/render3d/src/textures.rs:360-404`; helpers `crates/render3d/tests/common/mod.rs:53-110`)
   - **What this is:** `load_dds_as` turns a DDS into a D3D texture: one `D3D11_SUBRESOURCE_DATA` per face × mip with
     `pSysMem = data[face*slice_bytes + r.start..]` and `SysMemPitch = w.div_ceil(4) * block_bytes` for BC formats.
     This is the arithmetic every real game texture (BC1/3/5/7 with a full mip chain, cube env maps with mips) goes
     through. The only DDS helpers in the test tree build single-mip 32-bit RGBA8 files (`dds_rgba8`,
     `dds_rgba8_dx10`) and a 1×1 single-mip cube (`dds_cube_rgba8`).
   - **Problem:** D3D does not validate pitch or data length against `pSysMem`: a wrong face offset or a pitch computed
     in texels instead of blocks would upload garbage (or read past the buffer) with no error, and nothing in the test
     suite would notice. Today's code is right by reading, but a change to `slice_bytes`, the `row` closure or the
     level order would ship silently.
   - **Fix:** One test: a hand-made 8×8 BC1 DDS with 4 mips where each mip is a flat colour (a flat BC1 block is 8
     bytes: both endpoints the colour, indices 0), loaded with `load_dds`, then `GetResource` + `GetDesc` to assert
     `MipLevels == 4` and `Format == 71`, and a `Renderer` frame on the Plane at a size that samples mip 0 (full
     resolution) and one small enough to pick mip 3, asserting the two frame colours. The same helper with the cube
     flag and 3 mips covers the face offsets.
   - **If we skip it:** The one upload path real textures use stays unverified.
   - **Group:** Should fix. **Kind:** missing test.
   - **Confidence:** high (grepped `MipLevels|mips|dds_` across `render3d/tests`: no multi-mip or BC fixture).

4. **The Slot commands path box refills itself whenever the user empties it** (`crates/gui/src/ui/slot_commands.rs:88-92`)
   - **What this is:** The dialog's per-slot text buffer `st.text[i]` is pre-filled from the stored path with
     `if st.text[i].is_empty() { st.text[i] = s.clone() }` — evaluated every frame, not once when the dialog opens.
   - **Problem:** Slot 2 stores `textures\a\b_n.dds`. The user clicks the box, presses Ctrl+A then Backspace to type a
     new path: the buffer is empty for one frame, the next frame puts the old path back, and the user types onto the
     end of the old path. Only a select-all-and-type (never passing through empty) works.
   - **Fix:** Pre-fill when the dialog opens (where `st.open` is set: `run()` and the "Slot commands…" button) or keep a
     `filled: [bool; 8]` next to `text` and fill once. Three lines moved.
   - **If we skip it:** A small but confusing edit bug in a dialog the owner uses.
   - **Group:** Nice to have. **Kind:** bug.
   - **Confidence:** high (read the whole function; `Clear` stores `""` so an empty stored path does not refill).

5. **A failed HDRI backdrop is rebuilt (two shader compiles) on every HDRI frame** (`crates/render3d/src/lib.rs:1954-1956`, `crates/render3d/src/background.rs:94-170`)
   - **What this is:** `render()` does `if background == Hdri && self.backdrop.is_none() { self.backdrop = Some(Backdrop::new(..)?) }`.
     `Backdrop::new` decodes nothing new (the HDRI is memoised) but compiles the VS and PS with `D3DCompile` and creates
     a 16 MB texture. On `Err` the `?` returns before any draw and `backdrop` stays `None`.
   - **Problem:** If creating the backdrop fails for a reason that is not device removal (the embedded shader refused by
     an old `d3dcompiler_47`, or the 2048×1024 RGBA16F immutable texture refused by a starved WARP device), every
     frame with the Studio HDRI selected recompiles both shaders (tens of ms), fails again, and shows "Render error"
     instead of the model. The GUI's `last_sent == want` throttle limits it to one attempt per camera change, so an
     orbit drag is a stream of failed compiles and no picture until the user finds the Solid radio.
   - **Fix:** Memoise the failure: `backdrop: Option<Result<Backdrop, String>>`; on `Err` skip the backdrop pass, clear to
     black and push the error once into a warning the GUI shows, so the model still draws.
   - **If we skip it:** Rare, but when it happens the 3D view is unusable with HDRI on and the log fills with compile
     lines.
   - **Group:** Nice to have. **Kind:** bug / speed.
   - **Confidence:** medium on likelihood, high on the behaviour (traced `render()` and the GUI `request_frame` throttle).

6. **A poisoned RNM thumbnail lock would panic the UI every frame** (`crates/gui/src/ui/rnm_section.rs:40`)
   - **What this is:** `thumb()` locks a shared `Arc<Mutex<ThumbCache>>` kept in egui memory with `.lock().unwrap()`
     while uploading a layer thumbnail. Every other lock in the crate uses `unwrap_or_else(PoisonError::into_inner)`.
   - **Problem:** If `ctx.load_texture` or `ColorImage::from_rgba_unmultiplied` panics once inside the lock (a thumbnail
     whose `rgba.len() != w*h*4` trips egui's assert), the panic is caught by eframe's frame handling, but the mutex is
     now poisoned and the next frame's `unwrap()` panics again on the same line: the RNM section (and with it the settings
     panel of that texture) never draws again until restart.
   - **Fix:** `cache.lock().unwrap_or_else(std::sync::PoisonError::into_inner)` as elsewhere.
   - **If we skip it:** One rare panic becomes a permanent one for the session.
   - **Group:** Nice to have. **Kind:** risk.
   - **Confidence:** high on the mechanism; the triggering panic itself is unlikely (thumbnails come from the engine).

7. **The in-memory shader memo `Renderer::compiled` never evicts** (`crates/render3d/src/lib.rs:295-297`, `:1099`, `:1129-1131`)
   - **What this is:** Every compiled VS/PS pair (bytecode + reflection, roughly 100-200 KB for CS's `Lighting.hlsl`) is
     kept in a `HashMap` for the life of the renderer so a device rebuild or `set_shaders` never recompiles. Only panic
     results are removed.
   - **Problem:** Each distinct permutation the user touches adds an entry: every debug view (5) × every material
     permutation of every set previewed (specular on/off, alpha test, skinned, PBR flags) × every CS settings/features
     toggle. A long session clicking through a few hundred sets with the debug views on accumulates hundreds of pairs:
     tens of MB that are never freed, in a process that also holds decoded textures.
   - **Fix:** Cap it: drop the oldest entries past, say, 64 pairs (the disk cache under `cache_dir` still serves them in
     ~1 ms, see `lookup`), or clear `compiled` in `set_shaders` as `programs` already is.
   - **If we skip it:** Slow memory growth proportional to how many permutations one session visits; not a leak per
     frame.
   - **Group:** Nice to have. **Kind:** scale.
   - **Confidence:** medium (sizes estimated; growth path traced through `apply_compiled`).

8. **Dead or test-only public code in scope** (`crates/gui/src/ui/columns.rs:38-41`, `crates/render3d/src/permutation.rs:5-41`, `crates/render3d/src/lib.rs:629-633`)
   - **What this is:** `columns::strip` has no caller (`strip_button` is the one used). `Material::from_shape` and
     `desc_has_vertex_colors` are called only from `render3d/tests` (production goes through `material::perm_material`).
     `Renderer::set_encode_srgb` is set only by tests; production derives `encode_srgb` in `apply_settings_defines`.
   - **Problem:** Three entry points a reader must trace before learning they do nothing in the app; `from_shape`
     duplicates the field mapping in `perm_material`, so the two must change together.
   - **Fix:** Delete `strip`; move `from_shape`/`desc_has_vertex_colors` into the test that uses them (or build the
     `Material` through `perm_material(&MaterialState::from_lighting(..))`); mark `set_encode_srgb` `#[doc(hidden)]` like
     the other test hooks or drop it from the tests.
   - **If we skip it:** Nothing breaks; ~40 lines of noise.
   - **Group:** Nice to have. **Kind:** lean.
   - **Confidence:** high (grepped the whole tree including tests and docs for each name).

---

Smaller ones left out (6): the render thread keeps every swapped DDS's bytes (`TexState.swaps`, both variants of every
member) until the next load so it can replay them after a device loss, on top of the model's own copies; `apply_in_place`
re-asks the resolver for every RFAOS candidate on every texture edit because missing entries are evicted first;
`Renderer::render` recreates the three target textures on every 1-px size change (a problem line appearing above the
frame re-renders); `set_editor::draw` and `pgp_fields::context` clone the whole `Vec<SetView>` per frame (declared
`shortcut:` in `model/sets.rs`, so not counted); `columns::tile` builds the hover tooltip string for every visible tile
whether hovered or not; `viewport3d::image` stretches the previous frame to the new rect for a frame after a resize.

Not read: `ui/json_editor.rs` (read ~200 of 1118 lines), `ui/pgp_fields.rs` (~120 of 907), `ui/presets.rs`,
`ui/advanced.rs`, `ui/cs_pbr.rs`, `ui/input_name.rs` (~80 of 444), `ui/rename_dialog.rs`, `ui/messages.rs`,
`ui/plugin_refs.rs` (head only), `ui/dialogs.rs` (~200 of 455), `ui/drawer.rs` (~150 of 358), `ui/settings_panel.rs`
(~250 of 1050); the GUI and render3d test files were surveyed by test name and helper code, not read line by line;
`model/viewport3d.rs` and `model/set_preview.rs` only where the render thread and the frame request were traced.

Lean: ~40 lines of dead/test-only code (finding 8); `scene::Scene::write` + `ShapeDraw::update_by_name` (~110 lines) exist
only as the pixel-identity oracle for the resolved-field writer and could move behind `#[cfg(test)]`-style gating if
the oracle test is kept; no dependency in scope is replaceable (`half`, `sha2`, `hex`, `walkdir`, `arboard` all earn
their place).
