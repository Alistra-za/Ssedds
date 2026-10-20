# Changelog

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
