# sdc-gui user guide (M3a, M3b)

`sdc-gui.exe` is the graphical front end of the Skyrim DDS converter. It runs the same engine as `sdc convert`, so a folder you decide and export here converts to the same files from the command line. This guide covers the converter GUI (M3a) and the set editor with the PGPatcher JSON (M3b); texture edits (M3c) come later.

## Opening a folder

Choose **Open folder…** (pick one from **Recent**, or drag a folder onto the window; a dropped file opens the folder that holds it). The GUI reads the folder recursively, analyses every image and plans the conversion. Nothing is written to your textures. If the folder holds an `sdc.toml`, it is loaded; if that file cannot be parsed, or the engine rejects it (an unknown field or `allow` code in a `[files]` entry, an unknown sub-type, a value out of range), the folder is planned without it, a banner shows the error, and the GUI stays read-only (editing and **Export** are off, and the file is never written) until you fix the file and press **Reload** in the banner.

The **Mode** menu picks the target (`cs`, `vanilla` or `cpm`). It is saved in `sdc.toml` as `target`.

## The three columns

- **Textures** (left): one tile per file, with a thumbnail, its name and badges. Filters: *Needs decision*, *Has warnings*, *Failed*, *Not in a set*; the search box filters by name. A raw map that feeds a packed output shows "packed into ...".
- **Centre**: the settings of the selected texture, grouped (Type, Colour, Alpha, Mipmaps, Compression). Every value shows where it came from (preset, config, analysis, your `[files]` entry). Selecting a set shows its recipes instead.
  - *Compression* shows the `FMT-HDR-SOURCE` reason for an emissive (why it is BC6H or BC7 sRGB).
  - The format dropdown also lists the uncompressed formats (`RGBA8`, `R8`, `RG8`, and `RGBA16F` only under `cs`); Effort and Perceptual are locked for them.
  - A DDS file shows its header check, a *Tag on patch* choice and **Patch header…**, then **Patch**, which writes a patched copy to the export folder; the DDS itself is never changed.
- **Sets** (right): textures grouped into material sets, with the recipe status (run, conflict, disabled, failed) of each packed output.

The columns can be collapsed to thin strips (**Collapse columns**). The **Advanced** drawer at the right edge holds the preset inspector (with the loaded user presets), batch settings (worker threads, memory budget, BC1 decoder), packing fill values, the preview settings and a list of unmatched `[files]` entries.

## Deciding

A texture whose name follows the conventions decides itself. A High-confidence suggestion is accepted, and its sub-type reads **(auto)**; hover it to see why. High suggestions include:

- a known suffix such as `_n`, `_rmaos` or `_rfaos`;
- an exporter term such as `_albedo`;
- a file with no suffix, as its set's colour map;
- `_n` in a PBR set.

Choose another sub-type to override it: your choice is written to `sdc.toml` and always wins. Naming that breaks the conventions (a normal map saved without `_n`) is read as a colour map, so rename the file or choose its sub-type. A texture that still needs an answer has an **amber border**: a Medium or Low sub-type suggestion, the meaning of its alpha, the green-channel convention of a normal map, or a blocking warning you must acknowledge. Select it and use the confirm controls in the centre column (for example the sub-type buttons, the *Remove / Cutout / Blend / Cutout + Blend / Data* alpha buttons, *DirectX / OpenGL* for green, or an acknowledge button for a blocking warning). Each answer is written explicitly, as a field value or as an `allow` code, so it stays in your project file.

**Alpha from the meshes.** When a scan database exists (the Meshes view), a colour map whose alpha varies takes its alpha meaning from the meshes that use it, and the alpha question is not asked:

- any mesh with alpha blending gives *Blend*;
- alpha testing gives *Cutout*, at the strictest mesh threshold;
- both give *Cutout + Blend*;
- no mesh with alpha gives *Remove*.

The Alpha row then reads *from the meshes that use this texture*. A texture with no alpha, or a fully opaque one, keeps *Remove*. Your own choice still wins. Right after a folder opens, the question may show until the scan has been read.

## Editing

Any field in the centre column can be changed. Changes go into `sdc.toml` as `[files."<path>"]` entries, keyed by the file's path relative to the folder holding `sdc.toml` (packed outputs without an extension, for example `landscape/rock01_rmaos`). The GUI writes only `target`, the `[files.*]` tables and `defaults.packing.fill.*`; comments, ordering, rules and every other table in your `sdc.toml` are left byte for byte as they were.

**Mipmaps: Filter and Edge.** The *Filter* row in the Mipmaps group picks *Box* (the default), *Tent*, *Mitchell* or *Kaiser*. Options the texture cannot use are greyed out and their hover text says why: tent, Mitchell and Kaiser are only for sRGB colour without alpha weighting (not cutout or blend), and an HDR texture (BC6H or RGBA16F) may use box or tent only. The *Edge* row under it (*Wrap*, the default, or *Clamp*) is locked while the filter is box, because only tent, Mitchell and Kaiser read past the edge. **Resize** (*None*, *Down to a power of two* or *Half size (larger side above 512)*) is the last of the *Advanced texture fields* in the Advanced drawer. *Down to a power of two* shrinks each side to the largest power of two that fits (1000×600 becomes 512×512); *Half size* halves both sides when the larger is above 512 (1024×512 becomes 512×256). A resized texture shows a `DIM-RESIZED` note. The BC4/BC5 search radius field in the same list offers *Default*, *Maximum* and *Fast (preview)*.

A texture with a non-power-of-two size gets the blocking `DIM-NPOT` warning. Its decision box offers **Resize to a power of two** next to **Acknowledge for this texture**; the button writes `resize = "pot-down"` on that file's `[files]` entry and clears the block.

**Normal base level** (normal maps only) stores mip 0 at unit length by default; choose *Keep as authored* for a normal map whose short normals are deliberate. A PBR set's normal map also bakes its detail into the set's `_rmaos`/`_rfaos` roughness (including a normal map shared into another set, which bakes into that set's own roughness); its Normal handling row then reads *decided by the texture set*. A `_cnr` bakes its detail into its own coat roughness by default, so its row reads *built-in default*.

An edited file is marked **●** until you save with **Save** or **Ctrl+S**. Export saves first. If `sdc.toml` changes on disk while you have unsaved edits, saving asks whether to reload it or overwrite it. Closing the window or opening another folder with unsaved edits asks first. So does closing or opening a folder while an export is running: the dialog offers **Cancel export and continue** (the export stops, then the window closes or the folder opens) or **Keep running**.

Per-texture `[files]` entries sit above preset and config rules and analysis, and below any `--set` on the command line, so `sdc convert <folder>` honours them.

## Bulk editing

**Selecting several.** In the Textures or Sets column:
- **Ctrl+click** adds or removes one entry.
- **Shift+click** selects the range from the last clicked entry, in the order shown (search and filters applied).
- **Ctrl+A** selects everything shown.
- **Esc** clears the selection.

Textures and sets are never selected together. The column heading counts the selection, for example *12 selected (4 hidden by filter)*: textures hidden by the search or a filter stay selected and are edited too.

**Editing them together.** With two or more textures selected, the centre shows their settings in one panel:
- A row whose values differ reads *— mixed*.
- Picking a value writes it to every selected texture that can take it. Rows you do not touch are never written.
- A row that some textures cannot take (a field locked for them, or one that does not apply, such as *Normal handling* on a diffuse) says *applies to K of N*. Hover it for the reasons.
- After each change a line reports how many textures were written and why any were skipped.
- **↺** resets the field on all of them. A reset is not blocked by a lock.

**Sets.** With two or more sets selected, a row of chips picks which of their maps to edit: *All*, *Diffuse*, *Normal _n*, *RMAOS*, and so on. Each chip shows its count, plus *applies to K of N sets* when some sets have no map of that kind.

**Right-click.** Right-clicking a selected tile or set keeps the selection; right-clicking another one selects it alone. The menu offers:
- *Edit settings for N selected*.
- *Copy settings from <texture> to N selected…*: a checklist of the active (last clicked) texture's own settings, all ticked; untick what you want to keep. Textures only; to copy between sets, use *Select members of kind* first.
- *Reset all settings for N selected…*: removes their `sdc.toml` entries after asking. Acknowledged warnings are kept.
- For sets, *Select members of kind*, which turns the selection into those textures.

Every change of this kind is one undo step, and the folder is planned once. A read-only `sdc.toml` takes no bulk change. The Advanced drawer still edits one texture at a time.

**Slot menus.** Right-clicking a slot card in the set editor, or a member thumbnail on a pill, opens a menu for that slot:
- The slot commands: *From this set*, *Path* and *Clear*. Each is one undo step.
- *From another set*: links the slot to a map of another set. The link writes that map's output path when it has one (a TruePBR set's packed output), otherwise its `textures\...` game path. A map with neither is greyed out, so a link never points at nothing. A link needs the same slot kind, so the shared slots (glow / skin_sk, coat / fuzz) never offer the other kind.
- *Lock* / *Unlock*: a locked slot (shown with a lock icon) keeps its fill. It refuses a drag in or out and the slot commands. Bulk edits skip it with the reason *slot locked*, or *slot locked in <set>* when the fill is borrowed from another set. Automatic re-typing leaves it alone; you can still change the texture's sub-type by hand. The lock is saved in `sdc-project.toml` and is never written to the PGPatcher JSON.

## The preview

Double-click a tile (or a set member) to expand it into a preview with a Source side and an Encoded side. **Esc** or a double-click in a viewport collapses it again.

- **Live encode** is **off by default**. Encoding happens when you press **Encode now**; switch Live encode on to encode after every edit (a short pause after each edit, then the encode starts). A result that no longer matches the settings is badged *stale* until it is replaced.
- Each previewed mip is byte-identical to the same mip of the DDS that Export writes.
- The preview encodes only mips whose larger side is at most the **preview size limit**: **4096 by default**, settable to 1024, 2048, 4096 or 8192, and 16384 only after you tick *Allow 16384 previews* in Advanced. A GPU that cannot hold a texture that large lowers the limit and the preview says so.
- Toolbar: channel isolation (RGBA and an alpha mask), the mip slider, sRGB / UNORM display, *Side by side* or *Split* layout, a *Heatmap* of the difference to the source, *Fit* and *1:1*. Scroll zooms and dragging pans. Readouts under the viewports show the PSNR, alpha and normal-length checks for the mip on show.
- A DDS file in the folder previews as stored (there is no source side). Uncompressed DDS files preview from the CPU decode; BC2 and signed BC6H show "cannot be previewed" (N11 ended in M6).
- With no usable GPU the preview is off and says "No usable GPU: the preview is off; everything else works"; deciding, editing and Export still work.

## Export

**Export** converts the folder into the export folder (the **Export folder** button; by default `sdc-out` inside the opened folder). It cannot write into the folder it reads.

- If some textures are undecided, a dialog offers **Proceed anyway** or **Cancel**. Proceeding exports only the decided textures and skips the rest; it never decides for you.
- If a source file changed on disk since it was planned, the textures concerned show "changed on disk" and the Textures column offers **Reload**. Reload plans again, re-reading only the changed files. Export checks first and plans again by itself, so the export uses current files. The GUI also checks whenever its window regains focus.
- The **drawer** at the bottom opens when Export starts: a *Files* tab with per-file progress (queued, running, done, up to date, skipped, failed) and a *Warnings* tab grouped by code. Click a row to select that texture. **Cancel** stops the batch; **Save report…** writes the report; **Close** hides the drawer.
- **Copy CLI command** copies the `sdc convert` command that reproduces this export from the command line (same folder, output, BC1 decoder and thread count). Because Export saved `sdc.toml` first, that command gives the same manifest records and the same DDS bytes.

## Sets and the PGPatcher JSON

The **Sets** column is editable. It changes which textures belong to which material set, and it can write the PGPatcher JSON that tells PGPatcher how to use each PBR set.

### Editing sets

- **Drag a texture** from the Textures column onto a set to move it there, or onto empty space to start a new set. Dropping onto a slot that already holds a texture asks **Replace** or **Cancel**.
- **Share** lets a set borrow a texture from another set; **Duplicate** copies it into the set. Both ask before replacing a filled slot. A shared `_rmaos` is never variance-baked, because the sets sharing it have different normal maps.
- **Reorder** sets by dragging the **≡** handle.
- **Right-click** a set or a member for its menu (move, share, duplicate, rename, delete). Rename opens a dialog. Only a set you created can be deleted; an automatic set would regroup at once. A set whose files are all gone still shows, as missing, so you can delete its entry.
- **Badges** on a set show what needs attention: a missing file, a borrowed slot, a blocking problem for export.
- **Ctrl+Z** undoes and **Ctrl+Y** redoes (200 steps).

Set edits are saved in `sdc-project.toml`, next to `sdc.toml`. It holds the set order, created sets, moves, borrows, the PGPatcher settings and each set's shader choice, and nothing else. If it cannot be parsed, the sets and the JSON go read-only (a banner says why) while `sdc.toml` editing carries on.

### The set editor

Select a set to edit it. Each texture slot is a card, in PGPatcher slot order: Diffuse, Normal, Emissive, Height, RMAOS and so on. Only the slots the set's shader can use are shown. A filled slot the shader cannot use stays, with an amber outline, and its title says why on hover. A card has a **preset dropdown** (it fills the PGPatcher settings for that kind of material), and **Slot commands** to move, share, duplicate or clear the slot. **Opaque** sits on the Diffuse card. The Diffuse and Normal cards have **Match on this** (see Matching below). A `_coat` map fills the Subsurface card, which then reads **Coat colour + strength**: its alpha is the coat strength and is kept.

The set's **Shader** dropdown lists the shaders the current mode offers. The picked one reads "(auto)" when sdc chose it, and "(fallback)" when the shader you asked for does not exist under the current mode and `default` stands in. Choosing **Auto** clears your choice. Warnings (a missing or unused map, an unavailable shader) show on the set's ⚠ badge. A scanned mod's meshes decide the auto pick unless a PBR or skin member, a complex `_m` or a `_p` does.

### PGPatcher settings

- Features that cannot go together are **greyed**, with the reason on hover: hair excludes multilayer, glint, fuzz, emissive, parallax and subsurface; multilayer excludes subsurface; multilayer, glint and fuzz exclude each other; fuzz and coat normal exclude each other, because they share one slot. A switch that is already on stays enabled so you can turn it off.
- A feature that is on without its texture blocks export.
- **Reset** returns a field, or the whole set, to its default.
- PGPatcher's own `default` block is fixed: you cannot edit defaults. Each entry writes `emissive`, `parallax` and `subsurface` only when on, and omits the three numbers when unset or equal to the fixed value.
- Numbers cannot go below 0, and strengths, weights and opacities stay within 0–1. A typed value outside the range is clamped, and the box says "clamped to ≥ 0" or "clamped to 0–1". A value hand-edited in `sdc-project.toml` shows as it is, with the same note, and the JSON gets it clamped. The glint log density has no limit.
- **Matching** lists the exact texture paths the entry matches (PGPatcher's `match1`–`match8`). The entry applies when any of them matches. The diffuse path is on by default. The **+** button adds a row for slot 3–8. A slot that holds a texture starts from the set's path for it; any other slot starts empty and says "this row matches nothing until a path is entered" (an empty row is not written to the JSON). **×** removes a row. A set that matches nothing is left out of the JSON, and Export asks first.
- **Advanced TruePBR settings** (collapsed) holds path contains, mesh path contains, rename, UV, vertex colour and lock keys. **Rename** takes a new file name only (no folders): the outputs keep the matched texture's folder under `textures\pbr\` and take the new name, and sdc writes that full path to the JSON so PGPatcher uses it whichever match hits. If matches point at different textures and Rename is empty, a warning says the output prefix depends on which one hits first: edit the match paths so they point at the same texture.

### The JSON

After a complete export, **Export** writes `<export>\PBRNIFPatcher\<name>.json`. Only sets that exported fully are written, and the file name must be a single plain file name. If the file already exists and is not the one sdc wrote last time, you are asked before it is overwritten; a byte-identical file is never prompted. With a read-only `sdc-project.toml` the JSON is still written, but the next export asks before overwriting.

### Check by hand

The test harness cannot simulate drags. Reordering sets by dragging the **≡** handle and the drag-and-drop flows (texture onto a set, onto empty space, onto a filled slot) were verified by code review only; try them yourself.

## RNM normal layers

A set's `_bump`, `_bump2`, `_detailnorm`, `_detailnorm2` and `_bent` files (and their masks) are not textures of their own: they blend into the set's `_n` (or `_cnr`) before the mips are made. Layer files are not listed in the Textures column or as tiles, so the only place to see and change them is the normal.

- Select the normal: its settings have an **RNM** section with a switch for the whole blend, one strength slider per layer, the layer order and, for `_bent`, the AO options. The normal (or `_cnr`) card gets a **blue outline** while RNM is active (layers present and enabled); a slot the shader cannot use keeps its amber outline instead.
- When the set has both an `_n` and a `_cnr`, each layer has a **target** switch (*Normal*, *CNR* or *Both*). It is written as `rnm.target` on the layer file's `[files]` entry.
- The `_bent` AO line shows the AO source (a raw AO map, or the B channel of `_rmaos`/`_rfaos`) as an inverted thumbnail, with a *use AO* tick. The note under it says sdc inverts the AO for you: supply normal AO (white = open) and do not invert it yourself.
- Set-level notes (no base for a layer, a layer ignored on an eye set, an orphan mask) appear under the set grid.

## Complex grass

A complex-grass texture is one image twice as tall as it is wide: diffuse and alpha on top, normal and specular below. Pick **complex grass** as its sub-type (under *Packed and data*), or let sdc tag it after a Meshes-page scan that included plugins: a texture used by a GRAS (grass) record, 1:2, with a valid detection corner and a normal-map bottom half is tagged automatically. The vanilla target does not list it (the vanilla shader has no complex grass). Warnings name what is wrong: `GRASS-SHAPE`, `GRASS-TOP-OPAQUE`, `GRASS-NORMALS`, `GRASS-DETECT-SOURCE`, `GRASS-VANILLA`. The CLI never auto-tags: if you later run `sdc convert` on the same folder, tag the grass files in `sdc.toml` (`[files]`) or the CLI re-encodes them as diffuse.

## Where settings live

GUI-only settings (recent folders, export folder, preview limit, Live encode, worker threads, memory budget, BC1 decoder) are in `%APPDATA%\sdc\gui.toml`; they are never part of `sdc.toml`. The BC1 decoder model is read once when the GUI starts: after changing it the GUI shows "restart sdc-gui to apply". The user config of the command line (`%APPDATA%\sdc\config.toml`) also applies to the GUI. User presets in `%APPDATA%\sdc\presets\*.toml` are loaded by the GUI and the command line alike; a file that fails to load is skipped. The preset inspector in the Advanced drawer lists the user presets that loaded, with their files. There is no preset picker yet: select a user preset in `sdc.toml` (`preset = "<id>"`) or the user config.

sdc-gui ships no fonts. It loads the Windows system fonts (Segoe UI, Segoe UI Symbol, Segoe UI Emoji, Consolas) at start-up and refuses to start, with a message, if they are missing.

## When something goes wrong

- A background job that crashes shows an "internal error in ..." dialog; the window keeps running.
- A failed plan or export shows its error in a dialog and in the status line.
