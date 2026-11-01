# GUI guide

A map of the GUI (`tessalith.exe`): the main window with its areas numbered, each linked to the page that explains it. New to the GUI? Start with [Main window](GUI-Main-Window.md).

## Main window

![Tessalith's main window with a folder open: the header on top, the Textures and Sets columns, the settings of the selected texture, the Advanced strip at the right and the bottom panel with its Problems tab. Numbered red markers point at the areas listed below.](../img/gui/GUI-Guide/main-window.png)

The picture shows the Textures view after a folder is open, with a small made-up mod loaded. The numbers match the list. The 2D preview is not shown: it replaces the centre when you double-click a tile (see [The 2D preview](GUI-Preview.md)).

1. **Open, Undo, Redo** (header, far left). Pick a folder or a recent one, and step back and forward through your decisions. See [Header](GUI-Main-Window.md#header).
2. **Mode, MO2…, Preset** (header). Mode is the target (`cs` or `vanilla`), **MO2…** is the scan source, and **Preset** is the preset the folder converts with. The three are saved with the project. See [Header](GUI-Main-Window.md#header).
3. **Textures / Meshes tabs and the column eyes** (header, middle). Switch between the converter and the [mod scanning](Mod-Scanning.md) view, and show or hide the Textures and Sets columns. See [Header](GUI-Main-Window.md#header).
4. **Export to ▾** (header, right; **More ▾** until a folder is chosen). The folder the converted textures go to for this project. Its menu opens it, changes it and switches on the Vanilla and CM extras. See [Export to](GUI-Main-Window.md#export-to).
5. **Save** (header, right). Writes `sdc.toml` and `sdc-project.toml`. **Ctrl+S** does the same. See [Keyboard](GUI-Keyboard.md#keyboard).
6. **Export** (header, right edge). Converts the folder. If something is blocked or undecided, Review opens in the bottom panel first. See [Export](GUI-Main-Window.md#export) and [Export](#export).
7. **Texture filters** (top of the Textures column). Pills with counts (*Undecided*, *Warnings*, *Failed*, *Not in a set*), the **Filter ▾** popup, the tile size buttons and the search box. See [Textures column](GUI-Textures-Column.md#filters).
8. **Texture tiles** (Textures column). One tile per source file with its thumbnail, name and badges. An amber border means it still needs an answer; click a tile to select it, double-click for the preview. See [Textures column](GUI-Textures-Column.md) and [Deciding](GUI-Texture-Panel.md#deciding).
9. **Sets filters** (top of the Sets column). Pills for *Not ready*, *Ready* and *No JSON*, a **Filter ▾** and a search box for sets. See [Sets column](GUI-Sets-Column.md#filter).
10. **A set card** (Sets column). A set groups the textures of one material: its shader, a status dot, a warning count and a chip for each slot (D, N, P, RMAOS and so on). Drag a texture onto a card to move it there. See [Sets column](GUI-Sets-Column.md#set-cards).
11. **Settings of the selected texture** (centre). Output summary, then the groups Type, Colour, Alpha, Mipmaps and Compression, each value showing where it came from. A texture that needs an answer shows an *Undecided* box above them. Selecting a set shows its recipes instead. See [Texture settings panel](GUI-Texture-Panel.md#centre).
12. **Advanced** (right edge). The drawer with tool paths, encoder and interface settings. Click the strip to open it. See [Advanced drawer](GUI-Advanced-Drawer.md).
13. **Bottom panel tabs** (full width, bottom). **Problems** lists every warning with its fix as the row's own button (for example **Remove from set**, **Regenerate…**, **Rebuild Z**, **Open folder**, **Open report**, **Set layout ▾**, **Fix path**, **Undo this change**, and for what used to be done by hand **Open in editor** and **Reload** for an unreadable `sdc-project.toml`, **Clear read-only flag**, **Reset also.json**, **Tag as grass**, **Straight alpha**, **Remove match** and **Remove aliases from {id}**; buttons act only on their own click, never on Enter), in a sentence that says what to do (hover it for the engine's message and its code, such as `LOSS-ALPHA-DATA`; the codes are explained in [Troubleshooting](Troubleshooting.md)), **Messages** holds the notes of this session, **Files** shows an export's progress, and **Review** joins them while an export is being reviewed. Review opens at 40 % of the window's height (drag its top edge to change it; Review remembers its own height), and under 1200 points of window width its sections stack in one scrolling column. Click a tab to open or collapse the panel. A texture with a suggested type shows it as one button (**Set type: diffuse ✓**), with **▾** for the type picker (search box, suggestion on top); **Accept all suggestions (n)** at the top takes every suggestion, and **Enter** or **Space** on a focused row takes that row's. **Save** writes them.

The line under the header is the [status line](GUI-Main-Window.md#status-line-and-banner).

## Pages

- [Main window](GUI-Main-Window.md): opening a folder, the header, the columns, dialogs and the words the window uses.
- [GUI settings and recovery](GUI-Settings.md): where settings live, what is remembered, crashes.
- [Keyboard and command palette](GUI-Keyboard.md): every shortcut.
- [Textures column](GUI-Textures-Column.md): tiles, filters, sort and tile size.
- [Sets column](GUI-Sets-Column.md): set cards, the Sets filter, moving and reordering sets, slot menus.
- [Bulk editing and renaming](GUI-Bulk-and-Rename.md): selecting several, editing them together, the rename dialog and the input name box.
- [Texture settings panel](GUI-Texture-Panel.md): the settings of one texture, deciding, BC7 weighting, how edits are written.
- [Advanced drawer](GUI-Advanced-Drawer.md): batch, NVTT, preview and interface settings.
- [Helpers, RNM layers and cubemaps](GUI-Helpers-RNM-and-Cubemaps.md): the Helpers group and panel, RNM layers, the cube controls.
- [The 2D preview](GUI-Preview.md): source beside encode, readouts, lens, heatmap, pin and compare.
- [Generate from normal map](GUI-Generate-From-Normal.md): height, AO and curvature from a normal.
- [The set editor](GUI-Set-Editor.md): slot rows, packed channel sources, the shader.
- [The 3D preview](GUI-Set-Preview-3D.md): a set on a mesh with Community Shaders.
- [PGPatcher settings and the JSON](GUI-PGPatcher-JSON.md): TruePBR settings, the JSON file and the JSON editor.
- [Set materials](GUI-Set-Materials.md): Advanced skin, PBR Landscape, CS PBR records, PBR grass.

The sections below have no page of their own yet.

## Header

Moved to [Main window](GUI-Main-Window.md#header).

## The three columns

Moved to [Textures column](GUI-Textures-Column.md), [Sets column](GUI-Sets-Column.md) and [Texture settings panel](GUI-Texture-Panel.md).

## Deciding

Moved to [Texture settings panel](GUI-Texture-Panel.md#deciding).

## Editing

Moved to [Texture settings panel](GUI-Texture-Panel.md#editing).

## Bulk editing

Moved to [Bulk editing and renaming](GUI-Bulk-and-Rename.md#bulk-editing).

## Renaming

Moved to [Bulk editing and renaming](GUI-Bulk-and-Rename.md#renaming).

## The 2D preview

Moved to [The 2D preview](GUI-Preview.md).

## Export

**Export** converts the folder into the export folder. It cannot write into the folder it reads.

1. Export saves `sdc.toml` and `sdc-project.toml`.
2. If something is blocked, undecided or worth a second look, **Review** opens in the bottom panel instead of exporting (see the bottom panel, [Main window](#main-window) item 13). Decide the textures there (or **Accept all suggestions**), fix the blockers, tick the warnings, then press Review's **Export**, which exports the decided textures and skips the rest. It never decides for you and never exports by itself.
3. If a source changed on disk since it was planned, Export plans again first. The Textures column also offers **Reload** for changed files.
4. The bottom panel opens on its *Files* tab (one row per file: queued, running, converted, validation failed, up to date, skipped, failed, cancelled). Click a row to select that texture; hover it for the file's warnings, which *Problems* also lists.
5. After a complete export, the TruePBR JSON is written (see below).

While it runs the Files tab shows done/total, elapsed time, files per second and an ETA. The *Files* tab handles thousands of rows, with **Failed only** and **Has warnings** filters; **Show in Explorer** on a row selects the output. Buttons: **Cancel** stops the batch. **Open export folder** opens it in Explorer. **Save report…** writes the report as JSON Lines or Markdown. **Close** collapses the panel. **Copy CLI command** copies an `sdc convert` command for this export: the same folder (with `--recursive`), output, BC1 decoder and thread count, plus `--accept-suggestions high`, because the GUI accepts High-confidence suggestions. Before you run it, check two things it does not carry over:

- It has no `--nvtt`. If you chose an NVTT path in Advanced, add `--nvtt <path>`.
- The CLI does not read `sdc-project.toml` or the scan. Set edits from the Sets column (moves, borrows, created sets, shader choices, output names), alpha taken from scanned meshes and the complex-grass auto-tag do not apply to `sdc convert`. Decisions written to `sdc.toml` do.

If an export is blocked, Review's *Blockers* column lists what to fix; **Go to set** selects the set.

Review's footer also lists **Stale outputs (n)**: files an earlier export wrote that the project no longer writes (a renamed set's old files, a skin set's old `_f.dds` or `textures\pbr\` files, the old `_rmaos.dds` after a recipe switch, the output of a deleted source). A file still exactly as Tessalith wrote it has **Delete**, which acts on a mouse click only; a file changed since reads "edited since Tessalith wrote it: left alone" and has no button. Old JSON files Tessalith wrote (a renamed PGPatcher JSON, a Community Shaders PBR or complex-material JSON no longer written) are listed the same way, checked against the hash Tessalith recorded. The rows never block Export; while any has **Delete**, Problems shows "*n* old outputs Tessalith wrote are no longer exported" with **Review…**.

Generated maps' records and the hashes of the JSON files Tessalith writes are also saved at once in `_sdc/generated.json` beside `sdc-project.toml`, so closing without saving, discarding edits or a crash does not lose them.

### Also export as Vanilla and CM

In the cs mode one export can also write a vanilla and a complex-material (CM) version of a PBR mod.

- **Switch it on** with **Export to ▾** (**More ▾** until a folder is chosen) **→ Also export as Vanilla** or **Also export as Complex material** (checkable, no dialog; "Open a folder first" without a project), or with the ticks in Review's **Also export as** column. The column shows in the cs mode or while a tick is on; its red reason line shows only while a tick is on.
- **While a tick is on, Export always opens Review.** Review's Export writes three folders side by side: `<project> PBR` (the normal cs export, with its JSON), `<project> Vanilla` and `<project> CM`. Ticking only CM writes Vanilla too.
- **Review's column** also holds the **Export location** (per project, `[project] also_location` in `sdc-project.toml`; by default the export folder), the three folder names, **Keep metal brightness** and its strength (CM), **Also write a zip of each folder** and **Skip these sets**.
- **Install** the PBR folder or the Vanilla folder. For CM, install the CM folder on top of the Vanilla folder. With Keep metal brightness off, the CM folder holds only `_m` files (plus the complex-grass stack of each PBR grass set). Files an earlier export left in the folders are kept and listed in the report as "not written by this export"; delete them yourself.
- **Vanilla** gets a converted diffuse, a `_n` with the specular mask in alpha and, where the meshes use the environment-map shader, an env `_m`. That needs a Meshes scan; without one no env `_m` is written. A PBR Landscape set's `_p` goes into Vanilla too (Terrain Helper). Textures that are not PBR are copied from the PBR folder when they would be the same. **CM** gets one `_m` per PBR set.
- **Skip a set** with Review's **Skip these sets**, or, in 2D, **Skip vanilla/CM for this set** in the set editor's Material tab.
- **NIF edits** go into the PBR folder while a tick is on.
- **The recipe** is the **Also export recipe** section in Advanced (closed at first; Review's **Recipe settings are in Advanced** opens it). See [Project Config](Project-Config.md#export) for each value.
- **After the export** `<project> conversion-report.md` sits next to the folders: counts per folder, the recipe, a row per PBR set with a suggested NIF Glossiness and Specular Strength and whether it needs the Environment Map shader, the sets not converted, and how to install. With the zip on, `<folder>.zip` per folder and `<project> SHA256SUMS.txt`; the PBR zip includes the PGPatcher JSON. A zip too large for the ZIP format (65,535 files or more, or 4 GiB) is a Review blocker before anything is written: untick **Also write a zip of each folder** or split the export.

## Sets and the set editor

Moved to [Sets column](GUI-Sets-Column.md) and [The set editor](GUI-Set-Editor.md).

## TruePBR settings

Moved to [PGPatcher settings and the JSON](GUI-PGPatcher-JSON.md).

## Set preview (3D)

Moved to [The 3D preview](GUI-Set-Preview-3D.md).

## Cubemaps

Moved to [Helpers, RNM layers and cubemaps](GUI-Helpers-RNM-and-Cubemaps.md#cubemaps).

## RNM normal layers

Moved to [Helpers, RNM layers and cubemaps](GUI-Helpers-RNM-and-Cubemaps.md#rnm-normal-layers).

## Generate from normal map

Moved to [Generate from normal map](GUI-Generate-From-Normal.md).

## Advanced drawer

Moved to [Advanced drawer](GUI-Advanced-Drawer.md).

## Artist checks

Six checks look at the numbers Tessalith measures while it plans and warn about maps that will probably look wrong in game. They only warn: no check blocks an export, asks a question or changes "ready" (no amber dot on a tile or set, and the header **Export** button does not change). Each has a switch in **Advanced → Checks**, on by default; a switch hides its results at once.

| Check | Flags |
|---|---|
| Value range | Albedo too dark or too bright, metal that is not black or white, extreme roughness, a dark AO channel, dark metal, a mostly bright emissive background. |
| Plastic look | A vanilla `_n` with no alpha or an almost all-white alpha, so the specular is flat and full. Soft by design. |
| Low detail | A top mip that adds almost nothing over mip 1. Measured during Export. |
| Green direction | A normal that looks OpenGL when read as DirectX (an Info hint, never an auto-flip) or whose blue points the wrong way. |
| Parallax | A height map, judged against the set's neutral height (0.5, or 1.0 with a multilayer coat), that uses little range, clips, is mostly white or is too steep; landscape sets whose displacement scales differ by more than 2x. |
| UV coverage | UV islands closer than 8 texels for more than 2 % of used texels, so colour bleeds at a distance. Needs a Meshes scan. |

- **Problems rows.** Each warning is a row in the bottom panel's Problems tab, after the ordinary warnings, with a grey **check** dot (a check is never amber and never changes "ready"). A **Checks** pill narrows the list to them; the Problems tab's number counts every row shown.
- **Short normals.** A normal far from unit length gets one `CHAN-NORMAL-LENGTH` row with a **Rebuild Z** button (sets `channels.reconstruct_z`); it is greyed out with the reason when `sdc.toml` is read-only.
- **ⓘ badge.** A set card shows **ⓘ n** beside **⚠ n**. Hover it for the list, click it to open Problems on the set. The green-direction guess is a check row with **Treat as OpenGL**; it also shows on the badge and in the texture's own settings panel.
- **End of export.** One Info line counts them, for example "Checks: 3 warnings on 1 texture and 1 set; see Problems".
- **Range overlay.** The preview's **Range** toggle paints the texels that break the value-range rule magenta on the Source side. It is drawn for every texture but works only when the texture has a per-texel range warning (otherwise it is greyed out and the hover says why), and it is also greyed out while a pin is compared.
- **UV coverage overlay.** The **UV coverage** toggle darkens the texels no mesh uses. It reads the Meshes scan, so scan first: it is greyed out with the reason until a scan has finished (an empty scan counts as none). **Check all UV coverage** in Advanced → Checks reads every colour texture of the folder, with a progress bar and a **Stop** button.
- **Half size.** With **Measure whether full size adds anything** on, Export measures every texture and always writes it in full. Review then asks, before the next export, about each texture whose top mip added almost nothing: **Half size** saves `drop_top_mip = true` when you press Export and writes it from mip 1; **Full size** writes it as it is and saves nothing, so it is asked again next time. Export waits until every row is answered; Close and Esc write nothing. **Advanced → Half size** sets the same field by hand, and *Copy settings* lists it as a tick.
- **Toksvig gloss bake (opt-in).** Setting a `_n`'s *Normal handling* to **Renormalise and bake**, in a set with a complex-material `_m`, also bakes the normal's variance into that `_m`'s gloss. Off by default.

## Presets

**Presets…** (the last entry of the Preset combo in the header, or *Open the preset creator* in the command palette) opens the preset creator as a tab of the centre column. While Presets or the JSON editor is open there, tabs over the centre (**Selection**, **Presets**, **TruePBR JSON**) pick what it shows; clicking a texture or set brings **Selection** back. **Pop out** shows the view in a window of its own, for a second monitor; **Dock** puts it back in the centre; **Close** closes it. It edits your own presets, the files in `%APPDATA%\sdc\presets\` that the command line also loads.

- **The top bar.** Pick a preset. The built-in ones are read-only and show no error; of the editing buttons they offer only **Duplicate**, which makes an editable copy (named `<id>-copy`, without the original's suffix aliases); **New** starts an empty preset on the current target's quality preset. **Name** is the preset's id and file name (lower-case letters, digits and `-`); **Base** is the preset it builds on; **Target** is cs, vanilla, or "from the base". **Save preset**, **Revert**, **Reset preset** (drops every per-sub-type and suffix-branch change, so the base shows through) and **Delete…** (after a confirmation) are for your own presets only. The line beside them says whether the preset loads, or why not, in the engine's own words. **TOML preview** shows exactly what Save writes. Picking another preset, New or Duplicate while you have unsaved changes asks first. Preset files in the folder that did not load are listed at the top of the view with the reason, and **Open folder** opens the folder.
- **The sub-types.** One row per sub-type, in two families: **PBR / Community Shaders** (the sub-types only the cs target has: PBR, coat, Advanced skin, flow, HDR sky; and complex grass), then **Vanilla** (everything vanilla also has, including glow, envmask and complex_material). Complex grass is under PBR / Community Shaders: both targets have it, but it is a Community Shaders feature. Inside a family the rows are grouped Colour, Normal, Data and Special, where it has them. A row's title shows its format; open it for:
  - **Suffixes**: one branch per suffix that names the sub-type: the built-in suffixes, then your aliases for them (`_bumpy` for `_n`, say). The row's own settings below are the "no suffix" default; a branch changes them for the files with that suffix only. Open a branch (its title shows its format and how many fields it overrides, e.g. `_d — BC7 · sRGB (overrides 2)`) for the same Compression, Resize & mips, Alpha, Channel remap and Packing fills as the row, each hinting what the files get without the branch ("inherits"), with **↺** to drop the change. The title's format is what a file with that suffix gets from the whole preset, its other rules and, for an alias, the canonical suffix's branch included; "(+ rules by alpha)" marks a branch that a rule on alpha content may also change. A branch with no changes writes nothing. A branch is saved as one rule: `match = { suffix = "d", subtypes = ["diffuse"] }` for a built-in suffix, `match = { glob = "*_<alias>.*", subtypes = [...] }` for an alias. Every matching rule applies, in order, each changing only the fields it sets, so branches are written after your other rules (built-in suffixes, then aliases): a branch changes only its own fields on top of them, and an alias branch adds to its canonical suffix's branch. Rules the creator did not write are listed under **Other rules** (read-only) and kept as they are. A `[files]` value or `--set` still beats a branch. **Add alias** adds the alias and opens its empty branch; **Remove alias** removes it with its branch. The rmaos row shows the built-in ORM → RMAOS packing as a read-only `_orm` branch (it needs no preset). A new alias is checked as you type, with the engine's message when it is refused (not lower-case letters and digits, a built-in suffix, an exporter term such as `orm`, or an alias another preset already declares).
  - **Compression**: format (only the formats and tags the engine allows for the sub-type and target), encoder and effort.
  - **Resize & mips**: resize, mip levels, mip filter, and coverage for the cutout policies.
  - **Alpha**: policy, and the threshold for cutout; shown where the format keeps alpha.
  - **Channel remap** (data sub-types): the channel swizzle, the source of R, G, B and A (r, g, b, a, 0 or 1), saved as `channels.swizzle`. A remap for an alias's files is the Channel remap of that alias's branch.
  - **Packing fills** (data sub-types): the values for packed channels without a source map (roughness, metallic, AO, specular, height, 0-255).
- Each field shows the base's value as a hint (in a branch, the sub-type's). A field you changed has **↺**, which removes your change. A sub-type's category, colour space and the other built-in rules are never offered.
- **Saving.** The preset is loaded through the same loader as the presets folder before the file is written; a preset that would not load is never written. Your preset goes back to its own file, edited in place so its comments and layout stay; a new one goes to `<name>.toml`. After a save the folder is read again, the preset is listed in the header's **Preset** menu and the folder is planned again, without a restart. If `sdc.toml` names a preset that has since been deleted, the folder is still planned with the fallback (your config's preset, else the Mode's default), but the export is blocked: Problems shows a red row "Preset <id> is not installed" with **Choose preset…**, which opens the **Preset** menu. `sdc convert` stops on the same file with exit code 2.

## Where settings live

Moved to [GUI settings](GUI-Settings.md#where-settings-live).

## When something goes wrong

Moved to [GUI settings](GUI-Settings.md#when-something-goes-wrong).

## Keyboard shortcuts

Moved to [Keyboard and command palette](GUI-Keyboard.md#keyboard).
