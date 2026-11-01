# Changelog

## Unreleased

- BC3 alpha, BC4 and BC5: better block search (cutout alpha about +3 dB); existing BC3/BC4/BC5 outputs re-encode once.
- The host plugins (chaiNNer, GIMP, Paint.NET, Photoshop) moved to branch `host-plugins` (bug fixes only, not kept in parity with the engine). `main` no longer builds them.
- BC6H (HDR) at encoder effort `slowest` and `max` now uses a peak-preserving mode: small, very bright highlights such as a sun keep their brightness (venice_sunset's peak goes from 0.22 to 0.92 of the source). Other blocks encode as `veryslow`. Effort `veryslow` (the default) and below are unchanged. `--on-existing if-changed` re-encodes `pbr_emissive` and `hdr_sky`, the two types that are BC6H by default, once after upgrading. A texture of another type set to BC6H by hand is not re-encoded; to get the new mode for it, re-export it with `--on-existing overwrite`.

## 1.1.0

- `--on-existing if-changed` now re-encodes only the texture types whose output changed in a new version. Upgrading from 1.0.0 re-encodes nothing.
- Cubemaps: a new `cubemap` texture type for `_e` environment maps and files under `textures\cubemaps\`. It reads six face files (`_posx` to `_negz`), crosses and strips, and existing cube DDS files, and writes a cube DDS with a full mip chain. It also reads 360° panoramas (`equirect`, auto-detected only at exactly 2:1), mirror balls (`mirror_ball`), angular maps (`angular`) and a single photo as a blurred approximation (`photo`); the last three are chosen with `cubemap.layout`. `cubemap.*` settings in `sdc.toml` give four styles (`vanilla_metal`, `room`, `eye`, `hd`), face turns and flips, yaw and mirror, a ground fill below an adjustable horizon, blur and colour adjustments. Colour uses Community Shaders' game curve (`pow(c, 1.6)`), not sRGB. Files under `textures\cubemaps\` that 1.0.0 wrote as 2D are now cubemaps: one shaped like a cross, strip or exact 2:1 panorama is written as a cube, and any other shape (a square one, say) is skipped as unclear until you set `cubemap.layout`. Other texture types are unchanged. See docs/wiki/Cubemaps.md.
- New `sdc cubemap-marker` writes the 1×1 dynamic-cubemap marker.
- Helper textures: raw maps, masks and RNM layers are now *helpers* that fill a channel of another output of their set. An `_opacity` map fills the colour texture's alpha, and a `_specular` map fills the alpha of a non-PBR `_n` as well as its packed channel when the set packs one. You can set a helper's role (or make it *Not a helper* to convert it alone), the channels it fills (including the `_cnr` coat roughness, `_f` fuzz weight and `_s` subsurface weight alphas), the channel it is read from and Invert: in the GUI on the helper's panel and on a packed card's new Source picker, in `sdc.toml` with `helper.role`, `helper.fills`, `helper.read` and `helper.invert` on the helper file's `[files]` entry, and with `--set file:<glob>.helper.<key>=`. In the GUI every helper is a row in a new Helpers group under the tiles (pill on by default), including the RNM layer files, which were not listed before. See docs/wiki/Texture-Types.md#helper-textures.
- **Existing projects: some outputs change on the next export.** A set with a helper that 1.0.0 ignored, beside a target whose alpha is empty (none, fully opaque or flat), now fills that alpha: a colour texture beside an `_opacity` file, and a non-PBR `_n` beside a `_specular` file. That output gains an alpha and may change format (a BC1 diffuse moves to an alpha format). A target with a real alpha keeps it. `--on-existing if-changed` re-exports only those outputs (the helper is now recorded as a source of the output); nothing else re-encodes. To keep the old output, set `helper.role = "none"` on the helper (it is then converted on its own) or `helper.fills = []` (it is then not used).
- A set whose shader is Advanced Skin (saved in the GUI project, or `--set set:<id>.shader=advanced_skin`) is now an Advanced skin set: its diffuse (inferred, or written as plain `diffuse`) becomes `skin_albedo`, and it packs `_rfaos` instead of `_rmaos` when it can build one. Existing Advanced Skin projects switch `_rmaos` to `_rfaos` on the next export; the old `_rmaos.dds` stays in the export folder until you delete it.
- New codes `CUBE-FACES` and `CUBE-NO-MIPS`. `sdc inspect` and `sdc validate` flag cubes wider than 1 pixel with one mip, a lit 1×1 marker, an sRGB cube, and 2D files named `_e` or under `textures\cubemaps\`. **`sdc validate` now fails those 2D files**, because the game reads them as cubes.
- Cubemaps in the GUI: a Cubemap section in the settings panel (Source, Orientation, Colour grade, Ground, Look), a layout question for unclear images, a six-face cross preview with face labels and source face buttons, and Export to > Make marker cube... Settings are written to `sdc.toml`, so the GUI and CLI make the same bytes.
- The GUI keeps its image analyses in `%LOCALAPPDATA%\sdc\analysis-cache` (pruned to 200 MB when the GUI starts, safe to delete), so reopening a folder reuses the analyses of unchanged files instead of decoding them again.

## 1.0.0

First official release. It covers everything since 0.8.0-alpha.2 (0.9.0 was never released).

- **Upgrading: one re-encode.** The version change makes every `--on-existing if-changed` run re-encode each recorded output once after you upgrade. The bump itself changes no pixels and no presets.
- Derived outputs (Generate from normal map, Also export) no longer re-encode when you move a project folder. The first `if-changed` run after upgrading re-encodes them once.
- Meshes page: edit NIF files in the GUI. Change shader and material fields, edit extra data, add and remove alpha properties, and see the result in the 3D preview. **Export NIF** writes the edited mesh as a loose file into your Textures export folder (or an `sdc NIF Edits` mod in MO2). See docs/gui.md.
- Meshes page redesign: shape cards update as you edit, and **Export NIF** is a footer that stays in view. The button names its work ("Export NIF · 3 edits", "Export NIF again"), and Enter or Space on it never exports or overwrites. Also new: a Clips section, heel height, a known-extra-data list and controllers on the cards.
- Animation: a clock with play, pause and scrub. A key editor with colour pickers, plus controller editing (add, remove, retarget). The In game preview animates UV scroll, emissive, glossiness, specular, alpha and alpha threshold the way the game does.
- Named clips: add, rename and remove animation clips, and **Convert to clips** turns a plain controller chain into one. Removing the last clip removes the empty manager.
- Export NIF drops the orphan blocks that your edits left behind. A mesh with no orphans exports byte-identical to before.
- Shader-flag checks: Skin Tint and FaceGen detail need their shader type. A mesh that would crash the game or render black is flagged as an error and blocks export.
- Edit history: undo, redo, revert and Keep mine / Take source behave the same everywhere. A refusal message goes away once it no longer applies.
- 3D preview matches Community Shaders' deferred path more closely: **Accurate (deferred)** in the 3D view's *View* menu, on by default. Missing textures show a fill instead of white.
- **Generate from normal map**: make height, AO and curvature maps from a normal map. The files are named `_generated` and are used as packing inputs. See docs/gui.md.
- **Also export as Vanilla / Complex Material**: from a PBR set, write a vanilla or Complex Material version beside the PBR one. `_curves`, `_curvature` and `_cavity` files now join their set as packing inputs instead of making their own set. They write nothing by themselves.
- **Artist checks**: advice for banding, very dark or bright areas, normal-map curl and stepped height or metalness maps, listed in the bottom panel (**Checks**). They never block an export.
- **BC7 weighting** dropdown replaces the Perceptual check box: Linear (default), Perceptual, Alpha ×2, ×8 or ×16. It applies to the built-in BC7 encoder only. Other formats and encoders show a warning that it is ignored. Output with the default setting is unchanged.
- The `encoder.bc7_weights` setting is also available from `--set`, `[files]` and rules.
- Fixes:
  - Mod scan: MO2 files (`modlist.txt`, `ModOrganizer.ini`, `plugins.txt`, `Skyrim.ccc`, `Skyrim.ini`) are read one line at a time. A byte-order mark or a non-UTF-8 mod name no longer breaks the scan.
  - The loose-file walk skips only the actor facegen folder, so other folders are no longer missed.
  - Scan state: a scan that is stopped, or one started while a mesh read is in flight, no longer leaves "Used by meshes" and UV coverage empty.
  - Followed links: recursive discovery follows folder links. A link loop or a dead link is reported instead of hanging or being read back as input.
  - Rename Inputs refuses two rows that would write the same output. The JSON rename box refuses device and map-suffix names.
  - Fill from names checks slot clashes inside the batch. A file dropped from Explorer onto a slot card gets the same slot 3 and 7 check as a drag inside the app.
  - Moving a file back to its set, or a set that shares an extension-less key, now splits and restores correctly.
  - The Also gate with no project open says to open a folder.
  - The JSON editor: pending match pills stay visible, follow a mode change and shift when you remove a row. Shift+F8 goes to the previous error.
  - Keyboard: Ctrl+Shift+Z with nothing to redo no longer undoes. Shift+Tab in the input-name box no longer takes the ghost suggestion. A disabled shortcut yields to an enabled one on the same keys.
  - Presets: the Delete confirmation is per preset. The RNM strength and AO-power sliders keep the value you set until the plan updates.
  - Encode now pressed during an export runs when the export ends.
  - A non-Unicode path is left out of `gui.toml` instead of failing the settings save.
  - An edit that changes no text leaves the project clean. A `[files]` lookup also finds a key without an extension.
  - The bulk Encoder row keeps its lock when NVTT is not found.
  - CLI: `patch-header --dry-run` prints "would patch: ..." and no longer prints the real patched code. A quoted dotted `[files]` key is now a config error that names the field. Before, the value was silently dropped.
  - NVTT: a level timeout or Cancel stops the whole encoder process tree, not only the launcher. The temp folder is cleaned up.
  - Images: a NaN alpha pixel no longer turns an opaque float image almost opaque. A corrupt PSD with no pixel data is refused instead of crashing. The derive env-map gate finds the colour map by its own key (`_d` kept).
  - Meshes: the Unused note says LOD textures are left out of the list.
  - Meshes: the Meshes path filter folds ASCII case only, as stored paths do. The NIF edit source ignores path-separator spelling (`/` or `\`). A new 3D load no longer waits behind a replaced load's queued compiles.

## 0.8.0-alpha.2

- GUI layout v2. The header is rebuilt, and dialogs share one frame with a fixed button order. The export Review tab is split out, the Sets column has a filter, and the Presets window and the 3D settings window have moved. See docs/gui.md.
- Set cards show their position number. **Move to…** in a set's menu opens a dialog where you type a position or a set name; while a filter is on, it is the only way to reorder.
- The Sets column draws only the cards on screen, so it stays fast with hundreds of sets. Tab and screen readers reach only the cards on screen; Ctrl+K and Move to… still reach every set.
- Motion:
  - Toasts rise and fade, and stay while the pointer is on them.
  - Export fades to the accent colour when it becomes available.
  - Progress bars glide.
  - A dragged texture shows a ghost at the pointer.
  - Reordered sets and changed slots flash.
  - The columns fade in when a folder opens.
  - Ctrl+K, F1 and path autocomplete open at once.
- **Reduce motion** under Advanced → Interface stops movement and shortens fades.
- Rename dialog: a **Whole path** toggle (Inputs) runs Find, Replace, Prefix and Suffix over the folders too, for moving many inputs at once. The clipped *New* column is gone; *Output* shows the result.
- Fewer per-frame costs in the GUI. Background-only reads (output sizes, *used by*, report save) may appear up to 250 ms later.
- New bc7e quality level `encoder.effort = "max"` (sdc patch 0002: a quantised endpoint search and a fully refined, wider
  partition ranking on top of `veryslow`). Off by default; the default output is unchanged. It needs a build whose bc7e
  objects carry the patch and is refused with `CFG-INVALID` otherwise. Numbers: docs/research/bc7e-tuning.md.

## 0.8.0-alpha.1 (first public alpha)

Tessalith converts a folder of source images to DDS for Skyrim SE/AE. The GUI (`tessalith.exe`) and the CLI (`sdc.exe`) share one engine and one per-project `sdc.toml`. This is an alpha: back up your mod folders first.

### Features

- Works out what each file is from its name and folder, groups files into material sets, packs loose maps into Skyrim's layouts (`_rmaos`, `_rfaos`, complex `_m`), builds the mips and picks the BC format and the `_SRGB`/UNORM tag.
- Targets `cs` (Community Shaders with TruePBR, the default) and `vanilla` (which also packs the complex-material `_m` and complex grass).
- Inputs: PNG, TGA, TIFF, EXR, PSD/PSB, Krita, OpenRaster. Output: BC1, BC3, BC4, BC5, BC6H, BC7.
- Coverage-preserving mips for cutout alpha, renormalised normal mips, normal-mip variance baked into TruePBR roughness, RNM blending of `_bump`, `_detailnorm` and `_bent` layers.
- TruePBR: PGPatcher JSON written per set. Complex grass. Mod scan of MO2 meshes and plugins to decide each diffuse's alpha meaning and shader.
- Every output is decoded and checked against Skyrim's rules; `sdc validate` checks DDS files from other tools.
- GUI with encoded-mip preview, pixel readout, pin and compare, and a 3D preview. Optional NVTT 3.2.5 as the BC7/BC6H encoder (not bundled).

### Changes that matter when upgrading from 0.7

Breaking:

- **The `cpm` target is gone.** The targets are `cs` and `vanilla`. `cpm` in `--target`, `sdc.toml`, the user config or a preset is an error (exit 2), and `--preset cpm-quality` no longer exists. A user preset with `target = "cpm"` is skipped with `CFG-PRESET-SKIPPED`. Use `vanilla` with the set's shader on complex material instead.
- **Vanilla gains complex grass and complex material.** Under `vanilla`, a set packs the complex `_m` only when its shader is complex material; every other vanilla set keeps the env-mask `_m`, so existing vanilla output is unchanged.
- **Plain `_normal`, `_nrm` and `_norm` default to DirectX green.** They no longer block or ask; an info line (`IN-GREEN-UNSTATED`) says DirectX was assumed. `_normalgl` still flips. For an OpenGL map set `channels.green_convention = "opengl"` or use the GUI's *Treat as OpenGL* button. JSON event output no longer has `needs: ["green_convention"]`.
- **Hair diffuse defaults to cutout + blend** (was cutout), with coverage mips on. Mesh evidence still wins.
- **A complex `_m` with no source alpha now writes alpha 0** (was 255, which Community Shaders reads as a plain env mask).
- **One PGPatcher match key per entry.** A renamed set writes `match_diffuse` + `rename` only, with no `matchN`.
- **Rules layer.** Every matching rule of a source (each preset, the user config, the project config) now applies in file order, field by field. Before, only the first matching rule of each source applied.
- **Skin sets write no PGPatcher JSON** (Advanced skin).
- **Re-encode once.** The version bump re-encodes each recorded output once under `--on-existing if-changed`. Pixel output and presets are otherwise unchanged.

### Known issues

See [docs/wiki/Known-Issues.md](docs/wiki/Known-Issues.md).
