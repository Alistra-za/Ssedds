# Tessalith user guide (M3a, M3b)

`tessalith.exe` is Tessalith, the graphical front end of the Skyrim DDS converter (the command line is `sdc`). It runs the same engine as `sdc convert`, so a folder you decide and export here converts to the same files from the command line. This guide covers the converter GUI (M3a) and the set editor with the PGPatcher JSON (M3b); texture edits (M3c) come later.

## Opening a folder

Choose **Open folder…** (pick one from **Recent**, or drag a folder onto the window; a dropped file opens the folder that holds it). The GUI reads the folder recursively, analyses every image and plans the conversion. Nothing is written to your textures. If the folder holds an `sdc.toml`, it is loaded; if that file cannot be parsed, or the engine rejects it (an unknown field or `allow` code in a `[files]` entry, an unknown sub-type, a value out of range), the folder is planned without it, a banner shows the error, and the GUI stays read-only (editing and **Export** are off, and the file is never written) until you fix the file and press **Reload** in the banner.

The **Mode** menu picks the target (`cs`, `vanilla` or `cpm`). It is saved in `sdc.toml` as `target`. Hover the menu or an entry for what each target means.

**Dialog keys.** **Esc** is a dialog's Cancel (or Close), and so is a click outside it. The rename dialogs, **Copy settings** and the undecided-textures list ignore a click outside, so a stray click cannot lose what you typed or chose. **Enter** takes the safe default: Save in the unsaved-changes dialogs, Close, OK, Keep running, Keep it, and Cancel in the Reset all and Delete set confirms. In the "changed on disk" dialog Enter closes it without touching the file or your edits; it never overwrites. **Proceed anyway** is never taken by Enter.

## The three columns

- **Textures** (left): one tile per file, with a thumbnail, its name and badges. Filters: *Needs decision*, *Has warnings*, *Failed*, *Not in a set*; the search box filters by name. A raw map that feeds a packed output shows "packed into ...".
- **Centre**: the settings of the selected texture, grouped (Type, Colour, Alpha, Mipmaps, Compression). Every value shows where it came from (preset, config, analysis, your `[files]` entry). Selecting a set shows its recipes instead.
  - *Compression* shows the `FMT-HDR-SOURCE` reason for an emissive (why it is BC6H or BC7 sRGB).
  - The format dropdown also lists the uncompressed formats (`RGBA8`, `R8`, `RG8`, and `RGBA16F` only under `cs`); Effort and Perceptual are locked for them.
  - *Encoder* (Compression) is **Built-in** or **NVTT Production** (M5). NVTT is a quality option for BC7 and BC6H only, 40-60x slower; the row is locked for BC1-BC5 ("NVTT encodes BC7 and BC6H only"), hidden for uncompressed formats, and locked with "NVTT not found" while the NVTT status in Advanced says so (a texture already on NVTT can still go back). Changing the format away from BC7/BC6H returns the texture to the built-in encoder. A failed nvcompress fails that file (`ENC-NVTT-FAILED`); there is no silent fallback.
  - A DDS file shows its header check, a *Tag on patch* choice and **Patch header…**, then **Patch**, which writes a patched copy to the export folder; the DDS itself is never changed.
- **Sets** (right): textures grouped into material sets, with the recipe status (run, conflict, disabled, failed) of each packed output.

The columns can be collapsed to thin strips (**Collapse columns**). The **Advanced** drawer at the right edge holds the preset inspector (with the loaded user presets), batch settings (worker threads, memory budget, BC1 decoder), the NVTT path and status (**Choose nvcompress.exe…**, or **Automatic**: `PATH`, then the NVIDIA install folder; NVTT is never bundled; the status shows the version found and whether it is the tested 3.2.5, and is re-checked when the window regains focus), packing fill values, the preview settings and a list of unmatched `[files]` entries.

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

**Enable parallax** (complex material `_m` only) sits in the Alpha group. Ticked, the `_m` alpha is kept as the parallax height. Unticked, the alpha is written as 0 at every mip: complex material stays on and parallax is off. It starts ticked when the source alpha varies and unticked when the alpha is constant (solid 255, mid grey, 0) or missing (DECISIONS AH2). A source without an alpha channel cannot turn it on; the box is greyed out and its hover text says why, and ↺ clears a value set on the texture itself. The row is hidden when the alpha is removed (it writes 255). The same box is under the slot cards in the set editor, on the `_m`'s entry, and the bulk panel lists it for a selection of complex `_m` textures.

An edited file is marked **●** until you save with **Save** or **Ctrl+S**. Export saves first. If `sdc.toml` changes on disk while you have unsaved edits, saving asks whether to reload it or overwrite it. If it changes while you have no unsaved edits, the GUI reloads it when the window regains focus or when you save or export. Closing the window or opening another folder with unsaved edits asks first. So does closing or opening a folder while an export is running: the dialog offers **Cancel export and continue** (the export stops, then the window closes or the folder opens) or **Keep running**.

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
- *Reset all settings for N selected…*: removes their `sdc.toml` entries after asking. Acknowledged warnings are kept. A texture filling a locked slot is skipped, and the report says so.
- For sets, *Select members of kind*, which turns the selection into those textures.
- *Show in Explorer* (one texture selected): opens Explorer with the source file selected. It is greyed out, with the reason, when the file is gone.

Every change of this kind is one undo step, and the folder is planned once. A read-only `sdc.toml` takes no bulk change. The Advanced drawer still edits one texture at a time.

**Slot menus.** Right-clicking a slot card in the set editor, or a member thumbnail on a pill, opens a menu for that slot:
- The slot commands: *From this set*, *Path* and *Clear*. Each is one undo step.
- *From another set*: links the slot to a map of another set. The link writes that map's output path when it has one (a TruePBR set's packed output), otherwise its `textures\...` game path. A map with neither is greyed out, so a link never points at nothing. A link needs the same slot kind, so the shared slots (glow / skin_sk, coat / fuzz) never offer the other kind.
- *Lock* / *Unlock*: a locked slot (shown with a lock icon) keeps its fill. It refuses a drag in or out and the slot commands. Bulk edits skip it with the reason *slot locked*, or *slot locked in <set>* when the fill is borrowed from another set. Automatic re-typing leaves it alone; you can still change the texture's sub-type by hand. The lock is saved in `sdc-project.toml` and is never written to the PGPatcher JSON.
- *Show in Explorer*: the source file of the texture that fills the slot, selected in Explorer. A set pill's own menu has a *Show in Explorer* submenu that lists its members.

**Encoding follows type.** When a texture's sub-type changes (a set shader, a slot command, a drag, or a sub-type edit), a format and tag you set on it that the new type does not allow are reset, so the new type's default applies; a note after the status line says which (hover it when there are several). Settings that are still valid are kept, and a texture in a locked slot is left alone and named in the note. One undo restores both the change and the reset.

## Renaming

Right-click a texture or a set (or press **F2**) for the rename dialog. It has two modes:
- **Inputs** read files under corrected names, for a source saved with the wrong name (a normal map without `_n`, or a file in the wrong folder). The name is written to `sdc.toml` as `[inputs."<file>"] as = "<new path>"`; **the file on disk is never renamed**, and `sdc convert` reads the same alias. The tile shows *read as <name>*. The file keeps its own settings and set membership, whatever name it is read as. *Clear input alias* in the tile menu removes it.
- **Outputs** give a set's outputs a new base name. Every output keeps its folder and its suffix (`steel.dds`, `steel_n.dds`, `steel_rmaos.dds`). On a TruePBR set (Community Shaders) this writes the set's PGPatcher **Rename**, so the JSON follows; on any other set it is saved in `sdc-project.toml`. Nothing on disk is renamed, and outputs already exported under the old name stay where they are.

The dialog changes only the base name (the part before `_n`, `_normal` and so on): *Find* and *Replace* (tick *Regex* for a pattern, `$1` in *Replace* for a group), *Prefix*, *Suffix* and *Case*. Type in a row's *New* cell to set that row by hand, or untick it to leave it out. The table shows each new name, the kind its name now reads as, and the output.

Apply is greyed out while a row is red: two files or sets would end up with one name, a name Windows cannot use, a changed file extension, or an output name ending in a map suffix. Amber rows are warnings: a file whose name now reads as another kind of map, or an output that scanned meshes still use under its old name (renaming it leaves those meshes pointing at a file nothing writes). Packed outputs and textures in a locked slot are skipped in Inputs mode. Apply is one undo step.

## The preview

Double-click a tile (or a set member), or select it and press **Enter** or **Space**, to expand it into a preview with a Source side and an Encoded side. **Esc** or a double-click in a viewport collapses it again. In the preview, **[** and **]** step to the previous and next texture in the order the Textures column shows, **1** to **6** pick the channel buttons from left to right, and **PageUp** and **PageDown** step through the mips.

- **Live encode** is **off by default**. Encoding happens when you press **Encode now**; switch Live encode on to encode after every edit (a short pause after each edit, then the encode starts). A result that no longer matches the settings is badged *stale* until it is replaced.
- Each previewed mip is byte-identical to the same mip of the DDS that Export writes, except for a texture on NVTT: its preview uses the built-in encoder and says "Preview uses the built-in encoder; the export uses NVTT."
- The preview encodes only mips whose larger side is at most the **preview size limit**: **4096 by default**, settable to 1024, 2048, 4096 or 8192, and 16384 only after you tick *Allow 16384 previews* in Advanced. A GPU that cannot hold a texture that large lowers the limit and the preview says so.
- Toolbar: channel isolation (RGBA and an alpha mask), the mip slider, sRGB / UNORM display, *Side by side* or *Split* layout, a *Heatmap* of the difference to the source, *Fit* and *1:1*. Scroll zooms and dragging pans. Readouts under the viewports show the PSNR, alpha and normal-length checks for the mip on show.
- A DDS file in the folder previews as stored (there is no source side). Uncompressed DDS files preview from the CPU decode; BC2 and signed BC6H show "cannot be previewed" (N11 ended in M6).
- With no usable GPU the preview is off and says "No usable GPU: the preview is off; everything else works"; deciding, editing and Export still work.

## Keyboard

Press **F1** (or **?**) for the list of shortcuts, grouped by where they work. Press **Ctrl+K** for the command palette: type part of an action's, a texture's or a set's name (the letters in order are enough; names that start with what you typed come first, then names that contain it). Up and Down move, Enter runs the action or selects the texture or set as a click does and scrolls its column to it, Esc closes. The palette offers the actions that make sense where you are (Open folder, Save, Export, Undo, Redo, Rename, the Textures | Meshes view, the set's 2D | 3D view, Live encode, the PGPatcher JSON editor and more) and lists at most 200 results.

While a text field has the keyboard (the search box, a name, a number), the shortcuts leave it alone: only **Ctrl+K** opens the palette, and **Esc** leaves the field. Pressing **Ctrl+S** there says "Ctrl+S: leave the text field first" in the status line. A key does nothing while its action is off (Ctrl+S with nothing to save, Ctrl+Z with nothing to undo). No shortcut acts while a dialog, a menu or a box is open; a dialog has its own Enter and Esc. In the Textures grid the arrow keys follow the order on show (search and filters applied): Left and Right step one texture, Up and Down one row. Shift with an arrow selects the range from the active texture, as a Shift-click does.

| Area | Keys | Action |
|---|---|---|
| Global | Ctrl+K | Command palette |
| Global | F1 or ? | Keyboard shortcuts |
| Global | Ctrl+S | Save |
| Global | Ctrl+Z | Undo |
| Global | Ctrl+Y or Ctrl+Shift+Z | Redo |
| Textures grid | Ctrl+A | Select every visible texture (every set when a set is active) |
| Textures grid | Esc | Clear the selection |
| Textures grid | Left | Select the texture to the left |
| Textures grid | Right | Select the texture to the right |
| Textures grid | Up | Select the texture above |
| Textures grid | Down | Select the texture below |
| Textures grid | Shift+Left | Extend the selection to the left |
| Textures grid | Shift+Right | Extend the selection to the right |
| Textures grid | Shift+Up | Extend the selection up a row |
| Textures grid | Shift+Down | Extend the selection down a row |
| Textures grid | Enter or Space | Open the expanded preview of the active texture |
| Preview | Esc | Close the preview |
| Preview | [ | Previous texture |
| Preview | ] | Next texture |
| Preview | 1 | Show RGBA |
| Preview | 2 | Show R |
| Preview | 3 | Show G |
| Preview | 4 | Show B |
| Preview | 5 | Show A |
| Preview | 6 | Show the α-mask |
| Preview | PageUp | Larger mip |
| Preview | PageDown | Smaller mip |
| Sets | F2 | Rename the outputs of the selected sets, or the inputs of the selected textures |
| Dialogs | Enter | Press the dialog's safe default (never Proceed anyway) |
| Dialogs | Esc | Cancel the dialog |

## Export

**Export** converts the folder into the export folder (the **Export folder** button; by default `sdc-out` inside the opened folder). It cannot write into the folder it reads.

- If some textures are undecided, a dialog offers **Proceed anyway** or **Cancel**. Proceeding exports only the decided textures and skips the rest; it never decides for you.
- If a source file changed on disk since it was planned, the textures concerned show "changed on disk" and the Textures column offers **Reload**. Reload plans again, re-reading only the changed files. Export checks first and plans again by itself, so the export uses current files. The GUI also checks whenever its window regains focus.
- The **drawer** at the bottom opens when Export starts. While the export runs its line reads *done/total · elapsed · files/s · ETA*: files per second is the average of the last 10 seconds, and the ETA appears once 3 files have finished or 2 seconds have passed. **Cancel** stops the batch; **Save report…** writes the report; **Copy CLI command** copies the matching `sdc` command; **Open export folder** opens the export folder in Explorer (greyed out, with the reason, until Export has created it); **Close** hides the drawer.
  - The *Files* tab lists each file with its state and handles thousands of files: only the rows on screen are drawn. **Failed only** (failed, or failed validation) and **Has warnings** narrow it, and together they show the files that match both. Click a row to select that texture. **Show in Explorer** on a row selects the written output once it exists, otherwise the source file.
  - The *Warnings* tab lists every warning on its own row: the code, the severity, then the message (wrapped; a very long one is cut at three lines and shows whole on hover) and the file, which you can click. **By code** groups them: one row per code with its count; click a code to list its files.
- **Copy CLI command** copies the `sdc convert` command that reproduces this export from the command line (same folder, output, BC1 decoder and thread count; an NVTT path chosen in Advanced is not included: add `--nvtt <path>`). Because Export saved `sdc.toml` first, that command gives the same manifest records and the same DDS bytes.

## Sets and the PGPatcher JSON

The **Sets** column is editable. It changes which textures belong to which material set, and it can write the PGPatcher JSON that tells PGPatcher how to use each PBR set.

### Editing sets

- **Drag a texture** from the Textures column onto a set to move it there, or onto empty space to start a new set. Dropping onto a slot that already holds a texture asks **Replace** or **Cancel**.
- **Share** lets a set borrow a texture from another set; **Duplicate** copies it into the set. Both ask before replacing a filled slot. A shared `_rmaos` is never variance-baked, because the sets sharing it have different normal maps.
- **Reorder** sets by dragging the **≡** handle.
- **Right-click** a set or a member for its menu (move, share, duplicate, rename, delete). Rename is a submenu: **Rename…** (the set's name), **Rename outputs…** (F2; the files the set writes) and **Rename inputs…** (read the source files under other names), each with a hover line, and each opens a dialog. **Delete set** asks first ("Delete set 'name' (N members)? Its textures stay; only the set is removed.") and is one undo step. Only a set you created can be deleted; an automatic set would regroup at once. A set whose files are all gone still shows, as missing, so you can delete its entry.
- **Badges** on a set show what needs attention: a missing file, a borrowed slot, a blocking problem for export.
- **Undo** and **Redo** in the header (also **Ctrl+Z** and **Ctrl+Y**, in the Textures and Meshes views; 200 steps) are greyed out with nothing to undo. Hovering one shows the step, for example "Undo: Format". After one, the status line says "Undid: Format." or "Redid: Format." until your next edit.

Set edits are saved in `sdc-project.toml`, next to `sdc.toml`. It holds the set order, created sets, moves, borrows, the PGPatcher settings and each set's shader choice, and nothing else. If it cannot be parsed, the sets and the JSON go read-only (a banner says why) while `sdc.toml` editing carries on.

### The set editor

Select a set to edit it. Each texture slot is a card, in PGPatcher slot order: Diffuse, Normal, Emissive, Height, RMAOS and so on. Only the slots the set's shader can use are shown. A filled slot the shader cannot use stays, with an amber outline, and its title says why on hover. A card has a **preset dropdown** (it fills the PGPatcher settings for that kind of material), and **Slot commands** to move, share, duplicate or clear the slot. **Opaque** sits on the Diffuse card. A complex material `_m` (packed cpm or loose) has no card; its **Enable parallax** box sits under the cards, with a packed `_m`'s fill rows. The Diffuse and Normal cards have **Match on this** (see Matching below). A `_coat` map fills the Subsurface card, which then reads **Coat colour + strength**: its alpha is the coat strength and is kept.

The set's **Shader** dropdown lists the shaders the current mode offers. The picked one reads "(auto)" when Tessalith chose it, and "(fallback)" when the shader you asked for does not exist under the current mode and `default` stands in. Choosing **Auto** clears your choice. Warnings (a missing or unused map, an unavailable shader) show on the set's ⚠ badge. A scanned mod's meshes decide the auto pick unless a PBR or skin member, a complex `_m` or a `_p` does.

### PGPatcher settings

- Features that cannot go together are **greyed**, with the reason on hover: hair excludes multilayer, glint, fuzz, emissive, parallax and subsurface; multilayer excludes subsurface; multilayer, glint and fuzz exclude each other; fuzz and coat normal exclude each other, because they share one slot. A switch that is already on stays enabled so you can turn it off.
- A feature that is on without its texture blocks export.
- **Reset** returns a field, or the whole set, to its default.
- PGPatcher's own `default` block is fixed: you cannot edit defaults. Each entry writes `emissive`, `parallax` and `subsurface` only when on, and omits the three numbers when unset or equal to the fixed value.
- Numbers cannot go below 0, and strengths, weights and opacities stay within 0–1. A typed value outside the range is clamped, and the box says "clamped to ≥ 0" or "clamped to 0–1". A value hand-edited in `sdc-project.toml` shows as it is, with the same note, and the JSON gets it clamped. The glint log density has no limit.
- **Matching** lists the exact texture paths the entry matches (PGPatcher's `match1`–`match8`). The entry applies when any of them matches. The diffuse path is on by default. The **+** button adds a row for slot 3–8. A slot that holds a texture starts from the set's path for it; any other slot starts empty and says "this row matches nothing until a path is entered" (an empty row is not written to the JSON). **×** removes a row. A set that matches nothing is left out of the JSON, and Export asks first.
- **Advanced TruePBR settings** (collapsed) holds path contains, mesh path contains, rename, UV, vertex colour and lock keys. **Rename** takes a new file name only (no folders): the outputs keep the matched texture's folder under `textures\pbr\` and take the new name, and Tessalith writes that full path to the JSON so PGPatcher uses it whichever match hits. If matches point at different textures and Rename is empty, a warning says the output prefix depends on which one hits first: edit the match paths so they point at the same texture.
- **Colour fields.** Every colour input (the TruePBR colours, the 3D View background) has a swatch that opens a colour square and a `#RRGGBB` box (the `#` is optional, case is ignored). The box commits on Enter or focus loss; invalid text gets a red outline and is not applied. The 0-255 number boxes stay beside the TruePBR swatch. The background is a single grey, so a colour you pick there is stored as its grey equivalent.

### NiAlphaProperty

The TruePBR card has a collapsible **NiAlphaProperty** section for the schema's `alphaprop_*` keys (they set the alpha blending and alpha testing of the shape in the NIF). **Override flags** writes `alphaprop_flags`; the controls under it (Enable blending, Source and Destination blend mode, Enable testing, Test function, No sorter) compose the number as NifSkope does, and "= 4844" shows it with an **edit raw** box (0-65535; anything else is outlined red and not written). Switching it on starts at 4844: testing on, Greater, Src Alpha / Inv Src Alpha, blending off. **Alpha test threshold** writes `alphaprop_thresh` (0-255, starts at 128). **Create NiAlphaProperty if missing** gives shapes without one a new property; without it the flags and threshold only change shapes that already have one, and Export warns about that. **Delete NiAlphaProperty** removes the property and wins over the other alpha keys of the entry (they are greyed, and Export warns if any is set). A key that is off is not written. Each key that is set has a **reset** button. A hand-edited `4844.0` reads as 4844; a negative, fractional or too large value shows an amber "stored … is not valid" note (the key counts as set, and the first edit replaces it). The JSON editor lists the same keys.

### The JSON

After a complete export, **Export** writes `<export>\PBRNIFPatcher\<name>.json`. Only sets that exported fully are written, and the file name must be a single plain file name. If the file already exists and is not the one sdc wrote last time, you are asked before it is overwritten; a byte-identical file is never prompted. With a read-only `sdc-project.toml` the JSON is still written, but the next export asks before overwriting.

### Check by hand

The test harness cannot simulate drags. Reordering sets by dragging the **≡** handle and the drag-and-drop flows (texture onto a set, onto empty space, onto a filled slot) were verified by code review only; try them yourself.

## RNM normal layers

A set's `_bump`, `_bump2`, `_detailnorm`, `_detailnorm2` and `_bent` files (and their masks) are not textures of their own: they blend into the set's `_n` (or `_cnr`) before the mips are made. Layer files are not listed in the Textures column or as tiles, so the only place to see and change them is the normal.

- Select the normal: its settings have an **RNM** section with a switch for the whole blend, one strength slider per layer, the layer order and, for `_bent`, the AO options. The normal (or `_cnr`) card gets a **blue outline** while RNM is active (layers present and enabled); a slot the shader cannot use keeps its amber outline instead.
- When the set has both an `_n` and a `_cnr`, each layer has a **target** switch (*Normal*, *CNR* or *Both*). It is written as `rnm.target` on the layer file's `[files]` entry.
- The `_bent` AO line shows the AO source (a raw AO map, or the B channel of `_rmaos`/`_rfaos`) as an inverted thumbnail, with a *use AO* tick. The note under it says Tessalith inverts the AO for you: supply normal AO (white = open) and do not invert it yourself.
- Set-level notes (no base for a layer, a layer ignored on an eye set, an orphan mask) appear under the set grid.

## Complex grass

A complex-grass texture is one image twice as tall as it is wide: diffuse and alpha on top, normal and specular below. Pick **complex grass** as its sub-type (under *Packed and data*), or let sdc tag it after a Meshes-page scan that included plugins: a texture used by a GRAS (grass) record, 1:2, with a valid detection corner and a normal-map bottom half is tagged automatically. The vanilla target does not list it (the vanilla shader has no complex grass). Warnings name what is wrong: `GRASS-SHAPE`, `GRASS-TOP-OPAQUE`, `GRASS-NORMALS`, `GRASS-DETECT-SOURCE`, `GRASS-VANILLA`. The CLI never auto-tags: if you later run `sdc convert` on the same folder, tag the grass files in `sdc.toml` (`[files]`) or the CLI re-encodes them as diffuse.

## Where settings live

GUI-only settings (recent folders, export folder, preview limit, Live encode, worker threads, memory budget, BC1 decoder, NVTT path) are in `%APPDATA%\sdc\gui.toml`; they are never part of `sdc.toml`. The BC1 decoder model is read once when the GUI starts: after changing it the GUI shows "restart Tessalith to apply". The user config of the command line (`%APPDATA%\sdc\config.toml`) also applies to the GUI. User presets in `%APPDATA%\sdc\presets\*.toml` are loaded by the GUI and the command line alike; a file that fails to load is skipped. The preset inspector in the Advanced drawer lists the user presets that loaded, with their files. There is no preset picker yet: select a user preset in `sdc.toml` (`preset = "<id>"`) or the user config.

**What Tessalith remembers.** The window (its size, its position and whether it is maximised), the width of each column and panel you dragged, the Textures or Meshes view, the Textures filter toggles, the preview channel and layout, and the 3D View controls (background, sun, point light, radius) come back at the next start. They are saved in `gui.toml` a couple of seconds after you change them, and when you close the window. The zoom, the selection and the open folder are not remembered. If the saved window would be off every screen (a monitor was unplugged), it opens centred on the main screen instead; if only its title bar would be off the screen, it moves down so you can still drag it. Starting and closing without changing anything does not rewrite `gui.toml`. A value in `gui.toml` that makes no sense is ignored on its own; the rest of the file still applies.

Tessalith ships no fonts. It loads the Windows system fonts (Segoe UI, Segoe UI Symbol, Segoe UI Emoji, Consolas) at start-up and refuses to start, with a message, if they are missing.

## When something goes wrong

- A background job that crashes shows an "internal error in ..." dialog; the window keeps running.
- A failed plan or export shows its error in a dialog and in the status line.
