# GUI guide

Tessalith (`tessalith.exe`) runs the same engine as `sdc convert`. A folder you decide and export in the GUI converts to the same files from the command line, because every decision is saved in the folder's `sdc.toml`.

The window has two views, picked in the header: the Textures view (converter and set editor) and the Meshes view (mod scanning, covered in [Mod scanning](Mod-Scanning.md)).

## Header

| Control | What it does |
|---|---|
| **Open folder…** | Pick a source folder. You can also drag a folder onto the window; a dropped file opens the folder that holds it. |
| **Recent** | The last ten folders. |
| **Mode** | The target: `cs`, `vanilla` or `cpm`. Saved in `sdc.toml` as `target`. |
| **MO2…** | The scan source: the MO2 instance (**Choose…**), its profiles, **Scan a folder…** and **Scan** / **Cancel**. Hover for the instance and profile. |
| **Preset** | The preset the folder converts with: the Mode's default, or a built-in or user preset of this Mode. Saved in `sdc.toml` as `preset`. |
| **Presets…** | The preset creator (see [Presets](#presets)). |
| **Textures view** / **Meshes view** | Switch views. |
| **Textures column**, **Sets column** | Show or hide each column. |
| **Export** | Convert the folder. |
| **Messages** (Ctrl+M) | The last 500 notices, newest first; the button counts the ones you have not seen. |
| **Undo** / **Redo** | Greyed out with nothing to undo; hover for the step ("Undo: Format"). |
| **Save** (Ctrl+S) | Write `sdc.toml` and `sdc-project.toml`. |
| **Export folder: …** | Where converted textures go. By default `sdc-out` inside the opened folder. |
| **JSON:** | The name of the TruePBR JSON file. By default the folder name. |
| **Editor…** | Open the TruePBR JSON editor window. |

The window title shows the folder, with a dot when there are unsaved edits. Under the header is a status line, and a banner when something needs attention (for example an `sdc.toml` that failed to load, with a **Reload** button). The **MO2…** header button opens the scan source popup: the MO2 instance with **Choose…**, its profiles, **Scan a folder…** and **Scan** / **Cancel**; the button's hover text names the instance and profile. On the Textures view, a scan row under the header shows the scan's progress, status and errors when there are any.

When you open a folder, the GUI reads it recursively, analyses every image and plans the conversion. Nothing is written to your textures. If `sdc.toml` cannot be parsed or the engine rejects it (an unknown field, an unknown `allow` code, an unknown sub-type; most out-of-range values are reported per file as `CFG-INVALID` instead), the folder is planned without it, the banner shows the error, and the GUI stays read-only until you fix the file and press **Reload**.

While a folder opens, the status bar shows "Analysing 1,234 / 5,000 files…" with a bar and **Cancel**, which re-opens the folder you had before. Thumbnails appear when the whole plan lands. File and folder dialogs do not freeze the window; only one is open at a time. **Ctrl+K** opens a command palette and **F1** the shortcut sheet (see [Keyboard shortcuts](#keyboard-shortcuts)). Toasts report routine results; anything that needs a decision stays a dialog. The window size and position, column widths, view, Textures filters and 3D view settings come back at the next start.

## The three columns

From left to right: Textures, Sets, then the centre. The right edge holds the Advanced drawer.

### Textures (left)

One tile per file, with a thumbnail, the name and badges. Filters: the toggles *Needs decision*, *Has warnings*, *Failed* and *Not in a set*; combos for *Sub-type*, *Output format*, *Encoder* and *Set membership*; and a search box. They combine, and **Clear filters** resets them. *Sort* orders by Name, Source size or Status, and a slider sets the tile size. A raw map that feeds a packed output shows "Packed into …". The **◀** on the heading collapses the column to a strip; **▶** expands it.

### Centre

The settings of the selected texture, in groups: Type, Colour, Alpha, Mipmaps, Compression. Each value shows where it came from, for example "from sdc.toml", "suggested", or "from the meshes that use this texture". Selecting a set shows its recipes instead.

| Row | Choices |
|---|---|
| Sub-type | Grouped as PBR (Community Shaders), Advanced skin (Community Shaders), Colour maps, Normal maps, Grayscale maps, Packed and data, HDR maps, Other. Only sub-types the current Mode allows are offered, plus the texture's current sub-type. |
| Content | sRGB-encoded colour, or Linear data. |
| DDS tag | _SRGB or UNORM (linear), limited to what the format and Mode allow. |
| Alpha | Remove, Cutout, Blend, Cutout + Blend, Data. |
| Threshold, Comparison | Slider 1 to 254; Greater than, or Greater than or equal. |
| Coverage, Exact opaque alpha | Auto / On / Off; on or off. |
| Normal handling, Normal base level | Renormalise or Renormalise and bake; Renormalise to unit length or Keep as authored. |
| Mipmaps: Levels, Filter, Edge | Full chain or None (base level only); Box, Tent, Mitchell, Kaiser; Wrap or Clamp. |
| Format, Effort, Encoder, Perceptual | The block formats plus RGBA8, R8, RG8 and (under `cs`) RGBA16F; bc7e effort; Built-in or NVTT Production; perceptual metric. |

Options a texture cannot use are greyed out, with the reason on hover. For example, Tent, Mitchell and Kaiser are only for sRGB colour without alpha weighting; HDR textures may use Box or Tent; Edge is locked while the filter is Box; Effort and Perceptual are locked for uncompressed formats; the Encoder row is locked for BC1 to BC5 and hidden for uncompressed formats.

A texture's settings start with its output size ("Output: BC7 · 2048x2048 · 12 mips · 5.3 MiB"), and the status line adds a folder total (a VRAM estimate). After a mod scan, **Used by meshes (N)** lists the meshes that use the texture, with their mod and priority. A complex `_m` has an **Enable parallax** box: ticked keeps its alpha as the height, unticked writes alpha 0.

For an emissive, the Compression group shows the `FMT-HDR-SOURCE` reason (why it is BC6H or BC7 sRGB).

A DDS file in the folder shows its header check, a *Tag on patch* choice and **Patch header…**, then **Patch**, which writes a patched copy to the export folder. The DDS itself is never changed.

### Sets (between Textures and the centre)

Textures grouped into material sets, with the status of each packed output (run, conflict, disabled, failed). See [Sets and the set editor](#sets-and-the-set-editor).

## Deciding

A texture whose name follows the conventions decides itself: High-confidence suggestions are accepted, and the sub-type reads **(auto)**. Hover it to see why. See [Texture types](Texture-Types.md) for the naming rules.

A texture that still needs an answer has an amber border. The centre column then shows a *Needs a decision* box with the question:

| Question | Controls |
|---|---|
| What kind of texture is this? | A sub-type dropdown and **Confirm**. |
| What does the alpha channel mean? | *Remove*, *Cutout*, *Blend*, *Cutout + Blend*, *Data*. |
| Which way does this normal map's green channel point? | *DirectX* or *OpenGL*. |
| A blocking warning | **Acknowledge for this texture**. For `DIM-NPOT` there is also **Resize to a power of two**, which writes `resize = "pot-down"`. |

Every answer is written to `sdc.toml`, as a field value or as an `allow` code on the file's `[files]` entry. A choice you make always wins over a suggestion.

### Alpha from the meshes

After a mod scan (see [Mod scanning](Mod-Scanning.md)), a colour map whose alpha varies takes its alpha meaning from the meshes that use it, and the alpha question is not asked:

| Meshes using the texture | Result |
|---|---|
| Any mesh with alpha blending | Blend |
| Alpha testing | Cutout, at the strictest mesh threshold |
| Both | Cutout + Blend |
| No mesh with alpha | Remove |

The Alpha row then reads "from the meshes that use this texture". Your own choice still wins. Right after a folder opens, the question may show until the scan has been read.

## Editing

Change any field in the centre column. The change goes into `sdc.toml` as a `[files."<path>"]` entry, keyed by the file's path relative to the folder that holds `sdc.toml`. Packed outputs are keyed without an extension, for example `landscape/rock01_rmaos`.

The GUI writes only `target`, the `[files.*]` tables, `[inputs.*]` aliases and `defaults.packing.fill.*`. Comments, ordering, rules and every other table are left as they were.

- Unsaved edits put a dot in the window title until you save. Tiles carry no mark. Export saves first.
- If `sdc.toml` changes on disk while you have unsaved edits, saving asks whether to reload it or overwrite it. Without unsaved edits, the GUI reloads it when the window regains focus or when you export.
- Closing the window or opening another folder with unsaved edits asks first. During an export, the dialog offers **Cancel export and continue** or **Keep running**.
- **Ctrl+Z** undoes and **Ctrl+Y** redoes, up to 200 steps.

When a texture's sub-type changes (by a set shader, a slot command, a drag, or a sub-type edit), a format and tag you set that the new type does not allow are reset to the new type's default. A note after the status line says which. Textures in pinned slots are left alone and named in the note. One undo restores both.

**Resize** is in the Advanced drawer under *Advanced texture fields*: *None*, *Down to a power of two* (1000x600 becomes 512x512), or *Half size (larger side above 512)* (1024x512 becomes 512x256).

## Bulk editing

### Selecting several

In the Textures or Sets column:

| Input | Effect |
|---|---|
| Ctrl+click | Add or remove one entry. |
| Shift+click | Select the range from the last clicked entry, in the order shown. |
| Ctrl+A | Select everything shown. |
| Esc | Clear the selection. |

Textures and sets are never selected together. The column heading counts the selection, for example "12 selected (4 hidden by filter)". Textures hidden by the search or a filter stay selected and are edited too.

### Editing them together

With two or more textures selected, the centre shows one panel for all of them:

- A row whose values differ reads "mixed".
- Picking a value writes it to every selected texture that can take it. Rows you do not touch are never written.
- A row some textures cannot take says "applies to K of N". Hover it for the reasons.
- After each change, a line reports how many textures were written and why any were skipped.
- **↺** resets the field on all of them.

With two or more sets selected, a row of chips picks which of their maps to edit: *All*, *Diffuse*, *Normal _n*, *RMAOS* and so on, each with a count.

### Right-click menu

Right-clicking a selected tile or set keeps the selection; right-clicking another one selects it alone. The menu offers:

- *Edit settings for N selected*.
- *Copy settings from <texture> to N selected…*: a checklist of the active texture's own settings. Untick what you want to keep.
- *Reset all settings for N selected…*: removes their `sdc.toml` entries after asking. Acknowledged warnings are kept.
- For sets, *Select members of kind*, which turns the selection into those textures.

Each bulk change is one undo step. A read-only `sdc.toml` takes no bulk change.

## Renaming

Right-click a texture or a set, or press **F2**, for the rename dialog. It has two modes.

| Mode | What it does |
|---|---|
| **Inputs** | Reads files under corrected names, for a source saved with the wrong name. Writes `[inputs."<file>"] as = "<new path>"` to `sdc.toml`. The file on disk is never renamed, and `sdc convert` reads the same alias. The tile shows "read as <name>". |
| **Outputs** | Gives a set's outputs a new base name. Each output keeps its folder and suffix (`steel.dds`, `steel_n.dds`, `steel_rmaos.dds`). On a TruePBR set this writes the set's PGPatcher Rename; on other sets it is saved in `sdc-project.toml`. |

The dialog changes only the base name (the part before `_n`, `_normal` and so on). Tools: *Find* and *Replace* (tick *Regex* for a pattern, `$1` for a group), *Prefix*, *Suffix* and *Case*. Type in a row's *New* cell to set it by hand, or untick a row to leave it out.

Apply is greyed out while any row is red: two names collide, a name Windows cannot use, a changed extension, or an output name ending in a map suffix. Amber rows are warnings: a file whose name now reads as another kind of map, or an output that scanned meshes still use under its old name.

### Input name box

On the set editor heading, the **input name** box shows the path the set's textures are read as, and you can type a new one. While you type, the best known texture path (every path the last mod scan saw in a mesh's diffuse slot, plus this project's own diffuse paths) shows in gray after the text: **Tab** takes it up to the next `/`, Tab again keeps going, and **Right arrow** at the end takes all of it. **Enter**, or clicking away, gives every member of the set an `[inputs]` alias with its own suffix and extension, as one undo step; **Esc** puts the old name back. The set's TruePBR settings, pins and borrows follow it. A name that cannot be used (outside the project's folder, a name another file already has, a pinned slot) gets a red outline, with the reason on hover, and is never applied.

The folder part of the name is locked: it is dimmed and you edit only the file name. The thin rail along the bottom of the box has a stop at each `/`; drag it (or press **Ctrl+Left / Ctrl+Right** in the box) to lock more or less of the path. The **▾** at the right lists the known paths under the locked part, with a filter box (all words must appear); click one to use it.

## The 2D preview

Double-click a tile (or a set member) to expand it into a preview with a Source side and an Encoded side. **Esc** or a double-click in a viewport closes it.

- **Live encode** is off by default. Press **Encode now** to encode. With Live encode on, the preview re-encodes a moment after each edit. A result that no longer matches the settings is marked stale.
- Each previewed mip is byte-identical to the same mip of the DDS that Export writes. The exception is a texture on NVTT: its preview uses the built-in encoder and says so.
- Only mips whose larger side is at most the preview size limit are encoded. The limit is 4096 by default, settable to 1024, 2048, 4096 or 8192, and 16384 after you tick *Allow 16384 previews* in Advanced. A GPU that cannot hold a texture that large lowers the limit and says so.
- Toolbar: channel isolation (RGBA, R, G, B, A and α-mask), mip slider, sRGB / UNORM display, *Side by side* or *Split*, a *Heatmap* of the difference to the source, *Fit* and *1:1*. Scroll zooms, drag pans. HDR formats are tone-mapped for display.
- Readouts under the viewports show PSNR, alpha and normal-length checks for the mip on show.
- **Pixel readout:** point at the picture to see the source and encoded value of the texel under the cursor (0 to 255 and floats), the difference per channel, and for a normal map both normals and the angle between them.
- **Lens** (L) magnifies 8x under the cursor. **Tile 3x3** (T) repeats the picture to check seams. HDR formats get an **Exposure** slider. The heatmap has a colour legend and a full-scale slider.
- **Pin and compare** (P): *Pin this result* keeps the finished encode with its settings. Change a setting or the encoder, encode again, and *Compare: Source | Pinned* shows the pin beside the new result, with PSNR for both, the readout difference and a pinned-vs-current heatmap. There is one pin; closing the preview, changing texture or opening a folder drops it.
- A DDS file previews as stored (no source side). BC2 and signed BC6H cannot be previewed.
- Without a usable GPU the preview is off, and everything else still works.

## Export

**Export** converts the folder into the export folder. It cannot write into the folder it reads.

1. Export saves `sdc.toml` and `sdc-project.toml`.
2. If some textures are undecided, the *Undecided textures* dialog lets you decide them there, **Accept all suggestions**, **Proceed anyway** (export only the decided textures), or **Cancel**. It never decides for you.
3. If a source changed on disk since it was planned, Export plans again first. The Textures column also offers **Reload** for changed files.
4. The bottom drawer opens with a *Files* tab (one row per file: queued, running, converted, validation failed, up to date, skipped, failed, cancelled) and a *Warnings* tab grouped by code. Click a row to select that texture.
5. After a complete export, the TruePBR JSON is written (see below).

While it runs the drawer shows done/total, elapsed time, files per second and an ETA. The *Files* tab handles thousands of rows, with **Failed only** and **Has warnings** filters; the *Warnings* tab can group **By code**; **Show in Explorer** on a row selects the output. Drawer buttons: **Cancel** stops the batch. **Open export folder** opens it in Explorer. **Save report…** writes the report as JSON Lines or Markdown. **Close** hides the drawer. **Copy CLI command** copies an `sdc convert` command for this export: the same folder (with `--recursive`), output, BC1 decoder and thread count. Before you run it, check three things it does not carry over:

- It has no `--accept-suggestions high`, while the GUI accepts High-confidence suggestions. Add the flag, or files decided only by a suggestion are reported as unresolved.
- It has no `--nvtt`. If you chose an NVTT path in Advanced, add `--nvtt <path>`.
- The CLI does not read `sdc-project.toml` or the scan. Set edits from the Sets column (moves, borrows, created sets, shader choices, output names), alpha taken from scanned meshes and the complex-grass auto-tag do not apply to `sdc convert`. Decisions written to `sdc.toml` do.

If an export is blocked, the *Export is blocked* dialog lists what to fix; click an entry to select its set.

## Sets and the set editor

### Editing sets

- **Drag a texture** from the Textures column onto a set to move it there, or onto empty space to start a new set. Dropping onto a filled slot asks **Replace** or **Cancel**.
- **Share** lets a set borrow another set's finished output. **Duplicate** copies the source into the set. A shared `_rmaos` is never variance-baked.
- **≡** drags a set to reorder.
- **Right-click** a set for its menu: a **Rename** submenu (**Rename…** for the set's name, **Rename outputs…** (F2), **Rename inputs…**), **Edit match…**, **Remove member**, **Delete set**, **Move up** and **Move down**. **Delete set** asks first, naming the set and its member count. Only sets you created can be deleted; an automatic set would regroup from the file names at once. Right-clicking a member that fills a slot opens that slot's menu instead. A set whose files are all gone still shows, as missing, so you can delete its entry.
- **Fill from names…** (set menu) lists files in the set's folder named like its diffuse plus a known suffix, and moves the ticked ones in as one undo step.
- Dropping a file from Explorer on a slot card fills that slot when the file is a texture of the open folder and its sub-type fits; files are never copied.
- Badges show what needs attention: a missing file, a borrowed slot, a blocking problem. A PBR set under `cs` shows "PBR → JSON".

Set edits are saved in `sdc-project.toml` next to `sdc.toml`: set order, created sets, moves, borrows, shader choices, output names, slot pins and TruePBR settings. If that file cannot be parsed, sets and the JSON go read-only while `sdc.toml` editing carries on.

### The set editor

Select a set to edit it. The heading has the input name box and a **2D | 3D** switch (remembered in `gui.toml`, default 2D) that swaps the centre pane in place; in 3D the settings fold in sections above the view.

A set whose diffuse is an **Advanced skin** gets an **Advanced skin (Community Shaders)** card instead of the TruePBR box: 20 per-material skin settings, each "not set" or an explicit value saved in `sdc-project.toml`. Such a set has no PGPatcher JSON entry, and its files go to their normal paths. Export does not write the skin settings to Community Shaders yet.

In 2D, each texture slot is a card, in PGPatcher slot order: Diffuse, Normal, Emissive _g (or Skin _sk), Height, RMAOS, Coat _cnr, Fuzz _f, Subsurface. The cubemap slot has no card; set it with a slot command. Only the slots the set's shader uses are shown. A filled slot the shader cannot use keeps an amber outline, with the reason on hover.

| Card control | What it does |
|---|---|
| **Slot commands…** | *From this set*, *Path*, *Clear*, and *Pin slot N* (the sdc pin). Each is one undo step. |
| Right-click | The slot commands plus *From another set*, which links the slot to another set's map, *Pin slot (sdc)* / *Unpin slot (sdc)*, and *Keep original texture (PGPatcher lock)*. |
| **Opaque** (Diffuse) | Strips alpha: the diffuse is fully opaque. |
| **Match on this** (Diffuse, Normal) | PGPatcher applies the entry to meshes with this exact texture in this slot. |
| RMAOS / RFAOS (skin) | On a packed output's card: the recipe and per-channel fills. **Reset** removes a fill. |

A pinned slot keeps its fill. It refuses drags in or out and the slot commands, and bulk edits skip it. The pin is saved in `sdc-project.toml` and never written to the JSON.

*Keep original texture (PGPatcher lock)* turns on the slot's PGPatcher lock key (`lock_normal` and so on, the same key as in Texture locks): PGPatcher keeps the mesh's own texture in that slot. The card shows a big red ✕ with "The original texture in this spot is used" (with a scan, its hover lists the meshes' textures in that slot), and the member thumbnail a small red ✕. The file is still exported. A slot command on the same slot overrides the lock in PGPatcher, and a warning says so. The cubemap is not offered: `lock_cubemap` clears it.

A `_coat` map fills the Subsurface card, which then reads "Coat colour + strength". When RNM normal layers are active, the normal card has a blue outline; see [RNM layers](#rnm-normal-layers).

**Shader** lists the shaders the current Mode offers. The pick reads "(auto)" when Tessalith chose it and "(fallback)" when the shader you asked for does not exist under this Mode. **Auto** clears your choice. Shader warnings show on the set's badge. See [Texture types](Texture-Types.md#set-shaders).

**Used by plugins** lists the plugin texture sets (TXST) and alternate textures that use this set, from the last scan, with **Refresh**.

## TruePBR settings

A PBR set under `cs` has a TruePBR box with the settings PGPatcher reads. **Open the TruePBR JSON editor…** at its top opens the editor window.

| Group | Settings |
|---|---|
| Material | Specular level, Roughness scale. |
| Features | Emissive, Parallax, Hair, Multilayer (coat), Glint, Fuzz, and a **Subsurface** selector: Off, Standard or Foliage. |
| Emissive settings | Emissive scale, Emissive colour. |
| Parallax settings | Displacement scale. |
| Subsurface settings | Subsurface colour and opacity, for Standard and Foliage. |
| Coat settings | Under the Multilayer checkbox while it is on: coat colour, specular level, roughness, strength, diffuse, normal, parallax and inner UV scale. With Multilayer off the values are kept but not written. |
| Glint settings, Fuzz settings | The glint parameters; Fuzz colour and weight. |
| Matching | The exact texture paths the entry matches (PGPatcher's `match1` to `match8`). |
| Advanced TruePBR settings | Path contains, Mesh path contains, Rename; UV and cubemap (Smooth angle, Auto UV, UV scale, Cubemap path); Vertex colours and depth; Texture locks. |
| NiAlphaProperty | The schema's `alphaprop_*` keys: override flags (blending, blend modes, testing, test function, no sorter, or a raw number), alpha test threshold, create if missing, delete. A key that is off is not written. |

Rules:

- Features that cannot go together are greyed, with the reason on hover. Hair excludes multilayer, glint, fuzz, emissive, parallax and subsurface; multilayer excludes subsurface; multilayer, glint and fuzz exclude each other; fuzz and coat normal share a slot. A switch already on stays enabled so you can turn it off.
- Switches turn on by themselves from the set's textures and read "(auto)": a `_cnr` turns on Multilayer and Coat normal, an `_f` Fuzz, a `_p` Parallax, a `_g` Emissive, an `_s` Subsurface. Untick one to store an explicit off. A hair diffuse defaults to cutout + blend alpha.
- Every colour has a swatch with a colour square and a `#RRGGBB` box.
- A feature that is on without its texture blocks export.
- Each field's **reset** button returns it to its default. There is no reset for a whole set.
- Numbers cannot go below 0; strengths, weights and opacities stay within 0 to 1. A typed value outside the range is clamped and the box says so. The glint log density has no limit.
- PGPatcher's own `default` block is fixed and not editable.
- **Matching**: the diffuse path is on by default. **+** adds a row for slot 3 to 8; **×** removes one. A row with no path matches nothing and is not written. A set that matches nothing is left out of the JSON, and Export asks first.
- **Rename** takes a new file name only. Outputs keep the matched texture's folder under `textures\pbr\` and take the new name, and the matched diffuse is written as `match_diffuse`, the key PGPatcher renames from. A renamed entry is written as `match_diffuse` + `rename` only, with no `match1`-`match8` ("renamed: matches by diffuse (PGPatcher rename needs match_diffuse)"). A renamed set takes no extra match; extra rows from before the rename, and any path contains, are not written, and Export asks about them.

### The JSON file

After a complete export, Export writes `<export folder>\PBRNIFPatcher\<name>.json`, with `<name>` from the **JSON:** field. Only sets that exported fully are written. If the file exists and is not the one Tessalith wrote last time, you are asked before it is overwritten; a byte-identical file is never prompted. The JSON is written in `cs` mode only.

### The TruePBR JSON editor

Open it with **Editor…** in the header or from a set's TruePBR box. It is a window of its own, titled "TruePBR JSON — Tessalith".

- Each PBR set's entry is a pill with its title, output prefix and **Show** (selects the set in the main window), then its keys as small pills with the same controls as the TruePBR box.
- **+ key** adds any schema key. Keys Tessalith does not know are shown in amber and kept as written; they can be removed.
- A texture match pill picks how it matches: *exact · slot N* (`matchN`) or *path contains*. The **▾** picker lists the scan's textures for that slot, or every known texture path. The *mesh path contains* pill (`nif_filter`) has a picker of folders that hold scanned meshes. Without a scan, the pickers list only the project's own paths.
- **+ Delete section** adds an entry that belongs to no set and makes PGPatcher delete the shapes it matches, for example a vanilla glow shape the PBR base shape replaces. Sections are saved as `[[pgp_sections]]` in `sdc-project.toml` (keys `match1` to `match8`, `path_contains`, `nif_filter`) and written with `delete: true` after every set's entry. A section with no texture match is not written.
- A delete section that hits one of your sets (by exact match on the set's texture, or `path_contains` in its diffuse path) strikes that set's pill through, outlines it amber and warns. The set editor shows the same warning with a button to the editor.
- The header reads "N errors, M warnings". Errors block Export and show as a red line under their entry; warnings are what Export asks about. **Next error** / **Prev** (F8 / Shift+F8) jump between entries with errors, and a search box filters entries by set name or path.
- **JSON text** shows the file as it would be written now, with **Copy**.

Edits in the window and in the TruePBR box are the same edits; each shows the other's at once, and undo is shared.

## Set preview (3D)

Switch the set editor to **3D** to see the set on a mesh, rendered with Direct3D 11 and your own Community Shaders shader files. The preview does no work while it is hidden.

| Control | What it does |
|---|---|
| Encoded (exact export) / Source (faster, no compression) | Show the bytes Export would write, or the prepared source without compression. |
| **Mesh:** | *Auto* (a scanned game mesh that uses the diffuse) or a built-in shape: Sphere, Plane, Cube, Material ball. |
| **Shader folder…** | Point at your Community Shaders mod folder or its `Shaders` folder when it is not found automatically ("Community Shaders not found", **Choose folder…**). |
| Community Shaders features | On by default. Off compiles the shaders without CS feature defines. |
| Background | Solid or Studio HDRI (Poly Haven, CC0). For Solid, a colour swatch with a hex field and a *Grey* slider (0 to 100 %) set the grey level; a non-grey colour is turned into its grey. |
| Sun brightness, Point light brightness and radius | Lighting. |
| **Reset view** | Reset the camera. |

Moving around: drag to orbit, scroll to zoom, middle-drag or Shift+drag to pan, Shift+right-drag to turn the sun, double-click or Home to recentre. **View** has a debug view (Lit or UV checker; V steps through them) and **Reset view**. Missing textures show in magenta, listed under the frame with **Open in Textures** when the file is in the folder. **Copy image** (Ctrl+Shift+C) copies the frame to the clipboard.

Notes:

- The PGPatcher simulation needs the `cs` target and a PBR set. Other sets show their plain textures.
- To preview on a real mesh, scan your mods first. "No scanned mesh uses this diffuse" means the scan found none. Meshes from Skyrim LE are not drawn; a built-in shape is shown instead. A built-in shape cannot match an entry's match keys, so it shows the plain textures.
- The preview assumes your JSON wins over other mods' PGPatcher configs.
- Large textures are previewed at a capped size ("Preview at N px, export writes full size").
- These Community Shaders features are off in the preview: Light Limit Fix, Skylighting, Screen-Space Shadows, Subsurface Scattering, Dynamic Cubemaps, Image-Based Lighting, Physical Sky, Wetness, Cloud Shadows, terrain effects, water, fog, Volumetric Shadows.
- A timing log is written while the preview runs. Set the environment variable `SDC_PREVIEW_LOG=0` to turn it off.

## RNM normal layers

A set's `_bump`, `_bump2`, `_detailnorm`, `_detailnorm2` and `_bent` files (and their masks) blend into its `_n` (or `_cnr`) before the mips are made. They are not shown as tiles; select the normal to see them.

- The normal's settings have an **RNM** section: a switch for the whole blend, a strength slider per layer, the layer order, and for `_bent` the AO options (*AO blend* or *Like a detail*, *AO power*, *use AO*).
- When the set has both `_n` and `_cnr`, each layer has a target switch: *Normal*, *CNR* or *Both*. It is written as `rnm.target` on the layer file's `[files]` entry.
- The `_bent` AO line shows the AO source (a raw AO map, or the B channel of `_rmaos`/`_rfaos`) as an inverted thumbnail. Supply normal AO (white = open); Tessalith inverts it.
- Set-level notes (no base for a layer, a layer ignored on an eye set, an orphan mask) appear under the set grid.

## Advanced drawer

The drawer at the right edge holds settings that are not per-texture, plus a few rarely used texture fields.

| Section | Contents |
|---|---|
| Preset inspector | The batch preset, the user presets that loaded (with their files), and how the selected texture's settings were resolved. *Show only differences* hides values equal to the base preset. Built-in presets are read-only. |
| Batch | Worker threads, Memory budget (MiB), BC1 decoder (Ideal, Ideal (round 4), NVIDIA, AMD). The BC1 decoder is read at start, so a change shows "restart Tessalith to apply". |
| NVTT | NVTT path, **Choose nvcompress.exe…**, **Automatic** (`PATH`, then the NVIDIA install folder). The status shows the version found and whether it is the tested 3.2.5, and is re-checked when the window regains focus. |
| Project-wide fills (legacy) | One line, shown only while `sdc.toml` holds fills in `defaults.packing.fill.*`. **Clear** removes them; set fills on each packed output's card instead. |
| Preview | Largest previewed side, Allow 16384 previews, Live encode. |
| Mipmaps | Levels for the selected texture. |
| Advanced texture fields | Perceptual, BC1 transparent black, BC4/BC5 search radius (Default, Maximum, Fast (preview)), rgbcx level, DDS header (Auto, DX10 extended header, Legacy FourCC), Resize. |
| Unmatched [files] entries | `[files]` entries that name no file under the current target, with **Remove**. |

## Presets

**Presets…** in the header (or *Open the preset creator* in the command palette) opens the preset creator, a window you can move and resize; where it was is remembered in `gui.toml`. It edits your own presets, the files in `%APPDATA%\sdc\presets\` that the command line also loads.

- **The top bar.** Pick a preset. The built-in ones are read-only: **Duplicate** makes an editable copy (named `<id>-copy`, without the original's suffix aliases), and **New** starts an empty preset on the current target's quality preset. **Name** is the preset's id and file name (lower-case letters, digits and `-`); **Base** is the preset it builds on; **Target** is cs, vanilla, cpm, or "from the base". **Save preset**, **Revert**, **Reset preset** (drops every per-sub-type change, so the base shows through) and **Delete…** (your presets only, after a confirmation). The line beside them says whether the preset loads, or why not, in the engine's own words. **TOML preview** shows exactly what Save writes. Picking another preset, New or Duplicate while you have unsaved changes asks first. Preset files in the folder that did not load are listed at the top of the window with the reason, and **Open folder** opens the folder.
- **The sub-types.** One row per sub-type, grouped Colour, Normal, Data and Special. A row's title shows its format; open it for:
  - **Suffixes**: the built-in suffixes that name the sub-type, and your aliases for them (`_bumpy` for `_n`, say). A new alias is checked as you type, with the engine's message when it is refused (not lower-case letters and digits, a built-in suffix, an exporter term such as `orm`, or an alias another preset already declares).
  - **Compression**: format (only the formats and tags the engine allows for the sub-type and target), encoder and effort.
  - **Resize & mips**: resize, mip levels, mip filter, and coverage for the cutout policies.
  - **Alpha**: policy, and the threshold for cutout; shown where the format keeps alpha.
  - **Channel remap** (data sub-types): a remap is a new suffix alias whose files get a channel swizzle; pick the source of R, G, B and A (r, g, b, a, 0 or 1). It is saved as the alias plus a rule matching `*_<alias>.*` that sets `channels.swizzle`. The RMAOS row shows the built-in ORM → RMAOS packing as an example (read-only; it needs no preset).
  - **Packing fills** (data sub-types): the values for packed channels without a source map (roughness, metallic, AO, specular, height, 0-255).
- Each field shows the base's value as a hint. A field you changed has **↺**, which removes your change. A sub-type's category, colour space and the other built-in rules are never offered.
- **Saving.** The preset is loaded through the same loader as the presets folder before the file is written; a preset that would not load is never written. Your preset goes back to its own file, edited in place so its comments and layout stay; a new one goes to `<name>.toml`. After a save the folder is read again, the preset is listed in the header's **Preset** menu and the folder is planned again, without a restart. If `sdc.toml` names a preset that has since been deleted, the folder is planned with the fallback (your config's preset, else the Mode's default) and a banner says "preset <id> not found: using <fallback>".

## Where settings live

| File | Contents |
|---|---|
| `<folder>\sdc.toml` | Target and per-file decisions. Shared with the CLI. |
| `<folder>\sdc-project.toml` | Sets, shaders, slot pins, output names, TruePBR settings, JSON name, delete sections. GUI only. |
| `%APPDATA%\sdc\gui.toml` | Window and app settings: export folder, recent folders (10), preview limit, 16384 previews, Live encode, BC1 decoder, worker threads, memory budget, column visibility and widths, window size and position, Textures filters, sort and tile size, 2D/3D choice, 3D View controls, Advanced drawer state, NVTT path, the scan source (`[meshes]`) and the 3D preview's shader folder and size (`[render3d]`). Never part of `sdc.toml`. |
| `%APPDATA%\sdc\config.toml` | The user config, also read by the CLI. |
| `%APPDATA%\sdc\presets\*.toml` | User presets, also read by the CLI. A file that fails to load is skipped. |
| `%LOCALAPPDATA%\sdc\scan.db` | The mod scan database. |

## When something goes wrong

- A background job that crashes shows an "internal error in …" dialog, and the window keeps running.
- A failed plan or export shows its error in a dialog and in the status line.
- Tessalith needs the Windows system font Segoe UI and refuses to start with a message if it is missing. Segoe UI Symbol, Segoe UI Emoji and Consolas are used when present and skipped when not.

See also [Troubleshooting](Troubleshooting.md).

## Keyboard shortcuts

**F1** (or **?**) shows the full list, grouped by where each key works; **Ctrl+K** opens the command palette. The main keys:

| Keys | Action |
|---|---|
| Ctrl+S | Save. |
| Ctrl+Z / Ctrl+Y | Undo / redo (Textures and Meshes views). |
| Ctrl+M | Messages. |
| Arrow keys, Shift+arrows, Ctrl+A, Esc | Move through the Textures grid, extend the selection, select all, clear. |
| Enter / Space | Expand the preview of the active texture. |
| F2 | Rename. |
| In the preview: [ ], 1-6, PageUp/PageDown, L, T, P | Previous/next texture, channel, mip, lens, tiling, pin. |
| In 3D: Home, V, Ctrl+Shift+C | Recentre, next debug view, copy image. |
| F8 / Shift+F8 | Next / previous error in the JSON editor. |

Shortcuts leave a text field alone; Esc leaves the field.
