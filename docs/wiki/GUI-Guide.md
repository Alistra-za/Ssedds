# GUI guide

Tessalith (`tessalith.exe`) runs the same engine as `sdc convert`. A folder you decide and export in the GUI converts to the same files from the command line, because every decision is saved in the folder's `sdc.toml`.

The window has two views, picked in the header: the Textures view (converter and set editor) and the Meshes view (mod scanning and NIF editing, covered in [Mod scanning](Mod-Scanning.md) and [NIF editing](NIF-Editing.md)).

## Main window

![Tessalith's main window with a folder open: the header on top, the Textures and Sets columns, the settings of the selected texture, the Advanced strip at the right and the bottom panel with its Problems tab. Numbered red markers point at the areas listed below.](../img/gui-main.png)

The picture shows the Textures view after a folder is open, with a small made-up mod loaded. The numbers match the list. The 2D preview is not shown: it replaces the centre when you double-click a tile (see [The 2D preview](#the-2d-preview)).

1. **Open, Undo, Redo** (header, far left). Pick a folder or a recent one, and step back and forward through your decisions. See [Header](#header).
2. **Mode, MO2…, Preset** (header). Mode is the target (`cs` or `vanilla`), **MO2…** is the scan source, and **Preset** is the preset the folder converts with. The three are saved with the project.
3. **Textures / Meshes tabs and the column eyes** (header, middle). Switch between the converter and the [mod scanning](Mod-Scanning.md) view, and show or hide the Textures and Sets columns.
4. **Export to ▾** (header, right; **More ▾** with no project open). The folder the converted textures go to for this project. Its menu opens it, changes it and switches on the Vanilla and CM extras. See [Export](#export).
5. **Save** (header, right). Writes `sdc.toml` and `sdc-project.toml`. **Ctrl+S** does the same.
6. **Export** (header, right edge). Converts the folder. If something is blocked or undecided, Review opens in the bottom panel first. See [Export](#export).
7. **Texture filters** (top of the Textures column). Pills with counts (*Undecided*, *Warnings*, *Failed*, *Not in a set*), the **Filter ▾** popup, the tile size buttons and the search box. See [Textures (left)](#textures-left).
8. **Texture tiles** (Textures column). One tile per source file with its thumbnail, name and badges. An amber border means it still needs an answer; click a tile to select it, double-click for the preview. See [Deciding](#deciding).
9. **Sets filters** (top of the Sets column). Pills for *Not ready*, *Ready* and *No JSON*, a **Filter ▾** and a search box for sets.
10. **A set card** (Sets column). A set groups the textures of one material: its shader, a status dot, a warning count and a chip for each slot (D, N, P, RMAOS and so on). Drag a texture onto a card to move it there. See [Sets and the set editor](#sets-and-the-set-editor).
11. **Settings of the selected texture** (centre). Output summary, then the groups Type, Colour, Alpha, Mipmaps and Compression, each value showing where it came from. A texture that needs an answer shows an *Undecided* box above them. Selecting a set shows its recipes instead. See [Centre](#centre).
12. **Advanced** (right edge). The drawer with tool paths, encoder and interface settings. Click the strip to open it. See [Advanced drawer](#advanced-drawer).
13. **Bottom panel tabs** (full width, bottom). **Problems** lists every warning with its fix as the row's own button (for example **Remove from set**, **Regenerate…**, **Rebuild Z**, **Open folder**, **Open report**, **Set layout ▾**, **Fix path**, **Undo this change**, and for what used to be done by hand **Open in editor** and **Reload** for an unreadable `sdc-project.toml`, **Clear read-only flag**, **Reset also.json**, **Tag as grass**, **Straight alpha**, **Remove match** and **Remove aliases from {id}**; buttons act only on their own click, never on Enter), in a sentence that says what to do (hover it for the engine's message and its code, such as `LOSS-ALPHA-DATA`; the codes are explained in [Troubleshooting](Troubleshooting.md)), **Messages** holds the notes of this session, **Files** shows an export's progress, and **Review** joins them while an export is being reviewed. Review opens at 40 % of the window's height (drag its top edge to change it; Review remembers its own height), and under 1200 points of window width its sections stack in one scrolling column. Click a tab to open or collapse the panel. A texture with a suggested type shows it as one button (**Set type: diffuse ✓**), with **▾** for the type picker (search box, suggestion on top); **Accept all suggestions (n)** at the top takes every suggestion, and **Enter** or **Space** on a focused row takes that row's. **Save** writes them.

The line under the header is the status line: the file and set counts, a VRAM estimate for the selection and for the folder, and the progress of anything running.

## Header

| Control | What it does |
|---|---|
| **Open** | A menu: **Open folder…** (pick a source folder), then your recent folders (the last ten), or a greyed "No recent folders". You can also drag a folder onto the window; a dropped file opens the folder that holds it. |
| **Undo** / **Redo** | Greyed out with nothing to undo; hover for the step ("Undo: Format"). |
| **Mode** | The target: `cs` or `vanilla`. Saved in `sdc.toml` as `target`. Framed together with **MO2…**. |
| **MO2…** | The scan source: the MO2 instance (**Choose…**), its profiles, **Scan a folder…** and **Scan** / **Cancel**. Hover for the instance and profile. |
| **Preset** | The preset the folder converts with: the Mode's default, or a built-in or user preset of this Mode. Saved in `sdc.toml` as `preset`. Its last entry, **Presets…**, opens the preset creator (see [Presets](#presets)). |
| **Textures** / **Meshes** (tabs) | Switch views; the selected one is filled. |
| **Textures**, **Sets** (eye toggles) | Show or hide each column: an open eye shows it, a struck-through eye hides it. Right after the view tabs, Textures view with a folder open only. |
| **Export to ▾** | Shows where converted textures go, for this project: it is saved in the project's `sdc-project.toml`, so each project keeps its own. Until you choose a folder for the project (the folder you picked for another mod is not inherited) it reads **Export to: not chosen ▾**, and Review lists "No export folder chosen" with a **Choose the export folder…** button. With no project open the menu is a plain **More ▾**. A long path is cut in the middle; hover for all of it. The menu holds **Choose export folder…**, **Open export folder** and, in cs mode with a folder open, **PGPatcher JSON editor…** (the TruePBR JSON editor, a centre tab; the JSON's file name is its top field). |
| **Save** (Ctrl+S) | Write `sdc.toml` and `sdc-project.toml`. Left of Export. |
| **Export** | Convert the folder. The big button at the right edge. It reads **Review n files** while anything is undecided or blocked, **Export n files** (accent) once Review would write without skipping, and plain **Export** with no folder open. While an export runs, its lower half shows a progress bar with done/total (hover for the current file; **Cancel** is in the Files tab). |

The window title shows the folder, with a dot when there are unsaved edits. Under the header is a status line, and a banner when something needs attention (for example an `sdc.toml` that failed to load, with a **Reload** button). The **MO2…** header button opens the scan source popup: the MO2 instance with **Choose…**, its profiles, **Scan a folder…** and **Scan** / **Cancel**; the button's hover text names the instance and profile. On the Textures view, a scan row under the header shows the scan's progress, status and errors when there are any.

The header is one row from about 1340 points of window width; in a narrower window **Export to**, **Save** and **Export** wrap onto a second line instead of overlapping. The Meshes view draws the same header. **Messages** is not a header button: it is the first tab of the bottom panel (its label counts the notices you have not seen; **Ctrl+M** opens it, and again collapses the panel).

**Start screen.** With no folder open the window shows one large **Open folder** button under "Open a folder of source textures to begin.", the **Recent** folders as links, and "Or drag a folder onto the window." The columns appear when a folder is open.

When you open a folder, the GUI reads it recursively, analyses every image and plans the conversion. Nothing is written to your textures. If `sdc.toml` cannot be parsed or the engine rejects it (an unknown field, an unknown `allow` code, an unknown sub-type; most out-of-range values are reported per file as `CFG-INVALID` instead), the folder is planned without it, the banner shows the error (in words; a line that names a code is kept whole in the Messages tab), and the GUI stays read-only until you fix the file and press **Reload**. When the rejected value came from an edit you made in the GUI, **Undo** still works: that edit is the top step, named with the error, and undoing it makes the file editable again.

While a folder opens, the status bar shows "Analysing 1,234 / 5,000 files…" with a bar and **Cancel**, which goes back to the folder you had before, with its selection and undo history. Thumbnails appear when the whole plan lands; a DDS input Tessalith does not re-encode shows the file as stored, shortly after. File and folder dialogs do not freeze the window; only one is open at a time. **Ctrl+K** opens a command palette and **F1** the shortcut sheet, both at once with no fade (see [Keyboard shortcuts](#keyboard-shortcuts)). Toasts report routine results and fade in and out; anything that needs a decision stays a dialog. **Advanced > Interface > Reduce motion** stops movement and shortens the fades. The window size and position, column widths, view, Textures and Sets filters (not the search text) and 3D view settings come back at the next start.

## The three columns

From left to right: Textures, Sets, then the centre. The right edge holds the Advanced drawer; with a set selected in 2D, Advanced is instead the last tab under the set editor's slot rows. Under all of them runs the bottom panel ([Main window](#main-window), item 13).

### Textures (left)

One tile per file, with a thumbnail, the name and badges. Above the search box is a row of pills with counts: *Undecided*, *Warnings*, *Failed* and *Not in a set*. **Filter ▾** opens a popup with combos for *Sub-type*, *Output format*, *Encoder*, *Set membership* and *Sort* (Name, Source size or Status), and stays open while you use it; it reads **Filter (k) ▾** while k of the four filter combos are set. The **−** / **+** buttons after it make the tiles smaller or larger (from 64 points up to the thumbnail's own size, 128 points at 100 % scaling, in steps of 16). Everything combines, and **Clear filters** resets the pills, the combos and the search (not the sort or the tile size). Helper files (raw AO, roughness, specular, opacity and other masks, RNM layers) are not tiles: they are compact rows in a **Helpers (n)** group under the tiles, each saying where it goes ("→ iron_rmaos R") or "not used", with an amber **✎** role chip when you set the role. Click the group's heading to collapse or open it (open by default). Click a row to set its role, its targets, the channel it is read from and Invert; see [Texture types](Texture-Types.md#helper-textures). A tile whose name gives no helper term can be made one with the **Helper role** combo at the top of its settings. The **◀** on the heading collapses the column to a strip; **▶** expands it.

### Centre

The settings of the selected texture, in groups: Type, Colour, Alpha, Mipmaps, Compression. Each value shows where it came from, for example "from sdc.toml", "suggested", or "from the meshes that use this texture". Selecting a set shows its recipes instead.

| Row | Choices |
|---|---|
| Sub-type | Grouped as PBR (Community Shaders), Advanced skin (Community Shaders), Colour maps, Normal maps, Grayscale maps, Packed and data, HDR maps, Other. Only sub-types the current Mode allows are offered, plus the texture's current sub-type. The list opens under the field with a search box and the suggested type on top, the same picker as the Problems tab's **Set type…**. |
| Content | sRGB-encoded colour, or Linear data. |
| DDS tag | _SRGB or UNORM (linear), limited to what the format and Mode allow. |
| Alpha | Remove, Cutout, Blend, Cutout + Blend, Data. |
| Threshold, Comparison | Slider 1 to 254; Greater than, or Greater than or equal. |
| Coverage, Exact opaque alpha | Auto / On / Off; on or off. |
| Normal handling, Normal base level | Renormalise or Renormalise and bake; Renormalise to unit length or Keep as authored. |
| Mipmaps: Levels, Filter, Edge | Full chain or None (base level only); Box, Tent, Mitchell, Kaiser; Wrap or Clamp. |
| Format, Effort, Encoder, BC7 weighting | The block formats plus RGBA8, R8, RG8 and (under `cs`) RGBA16F; bc7e effort; Built-in or NVTT Production; Linear, Perceptual, Alpha ×2, ×8 or ×16 (BC7 with the built-in encoder only). |

Options a texture cannot use are greyed out, with the reason on hover. For example, Tent, Mitchell and Kaiser are only for sRGB colour without alpha weighting; HDR textures may use Box or Tent; Edge is locked while the filter is Box; Effort is locked for uncompressed formats; BC7 weighting shows only for BC7 with the built-in encoder, and its Perceptual option only for colour without data alpha; the Encoder row is locked for BC1 to BC5 and hidden for uncompressed formats.

A texture's settings start with its output size ("Output: BC7 · 2048x2048 · 12 mips · 5.3 MiB"), and the status line adds a folder total (a VRAM estimate). After a mod scan, **Used by meshes (N)** lists the meshes that use the texture, with their mod and priority; **Show in Meshes** opens the Meshes view listing just those meshes. A complex `_m` has an **Enable parallax** box: ticked keeps its alpha as the height, unticked writes alpha 0.

For an emissive, the Compression group shows the `FMT-HDR-SOURCE` reason (why it is BC6H or BC7 sRGB).

A DDS file in the folder is left as it is and not written to the export. **Re-encode this file** decodes it and encodes it again with the project's settings, at some quality cost for block-compressed files: it sets `reencode = true` on the file in `sdc.toml` (one undo step), and **Stop re-encoding** goes back. The button is off for a DDS that another file already writes (say `rock.png` beside `rock.dds`), and re-encoding into the folder the DDS comes from fails that file. With several textures selected, **Re-encode *n* DDS files** does the same for the selected DDS files.

A DDS file also shows its header check (one line, the header fields on hover), a *Tag on patch* choice and **Patch header…**, then **Patch**, which writes a patched copy to the export folder. The DDS itself is never changed.

### BC7 weighting against BC3

**BC7 weighting** sets how much the built-in BC7 encoder weighs alpha against colour: Linear (the default), Perceptual, or Alpha ×2, ×8 or ×16 (alpha error counts 2, 8 or 16 times). The chart compares each with BC3, as mean PSNR in dB, on three kinds of texture.

![Grouped bar chart of mean RGB and alpha PSNR for BC3 and the BC7 weightings on three texture classes. On colour with smooth alpha, Alpha ×8 gives 51.40 dB alpha against 47.24 for BC3 and keeps 46.60 dB RGB against 41.00. On a normal map or packed rmaos with specular in alpha, BC3 keeps the highest alpha, and BC7 alpha rises with the weight.](../img/bc7-alpha-weights.svg)

Takeaway: for colour with smooth alpha, Alpha ×8 beats BC3 on alpha and keeps 5.6 dB more RGB. For specular in the alpha of a normal map or an `_rmaos`, BC3 still has the highest alpha. BC7 closes the gap as the weight rises, and it keeps the better RGB or normals. The "w4" row was measured in the study but is not a dropdown option. Source: local study, 2026-10-21, and [DECISIONS Note CP](../DECISIONS.md). The full results (SSIM, max error, exact 0 and 255, per texture class) are in [Research](Research.md#bc3-against-bc7-alpha-weights-study-results).

### Sets (between Textures and the centre)

Textures grouped into material sets, with the status of each packed output (run, conflict, disabled, failed). See [Sets and the set editor](#sets-and-the-set-editor).

## Deciding

A texture whose name follows the conventions decides itself: High-confidence suggestions are accepted. When the plan settles, those sub-types (and any alpha meaning taken from scanned meshes) are written to `sdc.toml` as one undo step, shown as one Problems row, "{n} types set automatically (High confidence)", with **Undo this change** and the list on hover. Save keeps them, so the command line reads the same types. On their own they add no dot to the title, and closing or opening another folder does not ask about them: the next plan records them again. A file you already set is never touched, and a written type is a project value like one you chose: a later scan does not change it, but while it is still the type Tessalith would pick, a set shader you pick (or Auto), PBR follow or a new preset alias re-types it. See [Texture types](Texture-Types.md) for the naming rules.

A texture that still needs an answer has an amber border. The centre column then shows an *Undecided* box with the question:

| Question | Controls |
|---|---|
| What kind of texture is this? | A sub-type dropdown and **Confirm**. |
| What does the alpha channel mean? | *Remove*, *Cutout*, *Blend*, *Cutout + Blend*, *Data*. |
| An info line "IN-GREEN-UNSTATED" on a plain `_normal` | The map is converted as DirectX. Press *Treat as OpenGL (flip green)* if it is an OpenGL map; an OpenGL file then shows *Green: OpenGL (flipped)* with a *Treat as DirectX* button that undoes it. |
| A blocking warning | **Acknowledge for this texture** (**Drop the alpha anyway for this texture** when the warning is a varying alpha the format discards). For `DIM-NPOT` there is also **Resize to a power of two**, which writes `resize = "pot-down"`. |

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
- If `sdc.toml` changes on disk while you have unsaved edits, saving asks whether to reload it (**Reload from disk**) or overwrite it (**Overwrite with my changes**). Without unsaved edits, the GUI reloads it when the window regains focus or when you export.
- Dialogs share one style: the button Enter presses is drawn filled (teal), a destructive button (Don't save, Discard, Delete, Reset, Overwrite, Stop the export) is outlined in red and needs a click (Enter or Space on it does nothing), and the footer reads plain, destructive, then the default. Closing the window or opening another folder with unsaved edits asks first (**Save**, **Don't save**, **Cancel**) and names the files with unsaved changes; Enter is Cancel there. During an export, the dialog offers **Stop the export and close** (or **Stop the export and open the folder**) or **Keep exporting**.
- **Ctrl+Z** undoes and **Ctrl+Y** redoes, up to 200 steps.

When a texture's sub-type changes (by a set shader, a slot command, a drag, or a sub-type edit), a format and tag you set that the new type does not allow are reset to the new type's default. An info row in Problems says which, with **Undo this change**. Textures in pinned slots are left alone and named in that row. One undo restores both.

**Resize** is in the Advanced drawer under *Advanced texture fields*: *None*, *Down to a power of two* (1000x600 becomes 512x512), or *Halve if larger than 512* (1024x512 becomes 512x256).

## Bulk editing

### Selecting several

In the Textures or Sets column:

| Input | Effect |
|---|---|
| Ctrl+click | Add or remove one entry. |
| Shift+click | Select the range from the last clicked entry, in the order shown. |
| Ctrl+A | Select everything shown (in the Sets column, the visible sets only). With nothing shown it does nothing and keeps the selection. |
| Esc | Closes a popup, the preview or a dialog. It never clears the selection. |

Textures and sets are never selected together. The column heading counts the selection, for example "12 selected (4 hidden by filter)". Textures and sets hidden by the search or a filter stay selected and are edited too (sets: "2 selected (1 hidden by filter)").

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
- *Copy settings from <texture> to N selected…*: a checklist of the active texture's own settings. Untick what you want to keep. A saved half size is one of the ticks.
- *Reset all settings for N selected…*: removes their `sdc.toml` entries after asking. Acknowledged warnings are kept.
- For sets, *Select members of kind*, which turns the selection into those textures.

Each bulk change is one undo step. A read-only `sdc.toml` takes no bulk change.

## Renaming

Right-click a texture or a set, or press **F2**, for the rename dialog. It has two modes.

| Mode | What it does |
|---|---|
| **Inputs** | Reads files under corrected names, for a source saved with the wrong name. Writes `[inputs."<file>"] as = "<new path>"` to `sdc.toml`. The file on disk is never renamed, and `sdc convert` reads the same alias. The tile shows "read as <name>". |
| **Outputs** | Gives a set's outputs a new base name. Each output keeps its folder and suffix (`steel.dds`, `steel_n.dds`, `steel_rmaos.dds`). On a TruePBR set this writes the set's PGPatcher Rename; on other sets it is saved in `sdc-project.toml`. |

The dialog changes only the base name (the part before `_n`, `_normal` and so on). Tools: *Find* and *Replace* (tick *Regex* for a pattern, `$1` for a group), *Prefix*, *Suffix* and *Case*. In Inputs mode, tick *Whole path* to run them over the folders too, which moves many files to another folder in one go. The map suffix and the extension still stay. Untick a row to leave it out.

Apply is greyed out while any row is red: two names collide, a name Windows cannot use, a changed extension, or an output name ending in a map suffix. Amber rows are warnings: a file whose name now reads as another kind of map, or an output that scanned meshes still use under its old name.

### Input name box

On the set editor heading, the **input name** box shows the path the set's textures are read as, and you can type a new one. While you type, the best known texture path (every path the last mod scan saw in a mesh's diffuse slot, plus this project's own diffuse paths) shows in gray after the text: **Tab** takes it up to the next `/`, Tab again keeps going, and **Right arrow** at the end takes all of it. A list of up to 12 matching paths opens under the box as you type; **Up/Down** highlight a row, **Enter** uses the highlighted row (or what you typed when none is), and clicking a row uses it. The first **Esc** closes the list. **Enter**, or clicking away, gives every member of the set an `[inputs]` alias with its own suffix and extension, as one undo step; **Esc** puts the old name back. The set's TruePBR settings, pins and borrows follow it. A name that cannot be used (outside the project's folder, a name another file already has, a pinned slot) gets a red outline, with the reason on hover, and is never applied.

On a TruePBR set in the 2D view, the **Match** box beside the name box picks the slot the ▾ list works on. **Match 1 · Diffuse** is the list above. **Match 2 · Normal _n** to **Match 8 · Subsurface _s** (named as the set editor names the slot, with the key, `match2` and so on, on hover) list the textures the mod scan saw in that slot; a pick sets that slot's match in the PGPatcher JSON and does not rename anything. Tick **All slots** in the list to see every known texture; a pick still goes to the selected match. The box starts at the set's lowest unused match, or at the one you last chose for that set (remembered per project), and never moves by itself.

The folder part of the name is locked: it is dimmed and you edit only the file name. The thin rail along the bottom of the box has a stop at each `/`; drag it (or press **Ctrl+Left / Ctrl+Right** in the box) to lock more or less of the path. The **▾** at the right lists the known paths under the locked part (never those under `textures\pbr\`, which are PGPatcher's outputs), with a filter box (all words must appear); click one to use it.

## The 2D preview

Double-click a tile (or a set member) to expand it into a preview with a Source side and an Encoded side. **Esc** or a double-click in a viewport closes it.

- **Live encode** is off by default. Press **Encode now** to encode. With Live encode on, the preview re-encodes a moment after each edit. A result that no longer matches the settings is marked stale.
- Each previewed mip is byte-identical to the same mip of the DDS that Export writes. The exception is a texture on NVTT: its preview uses the built-in encoder and says so.
- Only mips whose larger side is at most the preview size limit are encoded. The limit is 4096 by default, settable to 1024, 2048, 4096 or 8192, and 16384 after you tick *Allow 16384 previews* in Advanced. A GPU that cannot hold a texture that large lowers the limit and says so.
- Toolbar: three labelled clusters that wrap onto more rows when the preview is narrow. **Channels** (RGBA, R, G, B, A and α-mask); **View** (*Side by side* or *Split*, *Pin this result*, a *Heatmap* of the difference to the source, *Lens*, *Tile 3×3*, the *Range* and *UV coverage* overlays when they apply, *Fit* and *1:1*); **Encode** (*Live encode* and **Encode now**, filled while it can encode). The sRGB / UNORM display and the mip slider follow, and **✕ Close** stays at the right. Scroll zooms, drag pans. HDR formats are tone-mapped for display.
- Readouts under the viewports show PSNR, alpha and normal-length checks for the mip on show.
- **Pixel readout:** point at the picture to see the source and encoded value of the texel under the cursor (0 to 255 and floats), the difference per channel, and for a normal map both normals and the angle between them.
- **Lens** (L) magnifies 8x under the cursor. **Tile 3x3** (T) repeats the picture to check seams. HDR formats get an **Exposure** slider. The heatmap has a colour legend and a full-scale slider.
- **Pin and compare** (P): *Pin this result* keeps the finished encode with its settings. Change a setting or the encoder, encode again, and *Compare: Source | Pinned* shows the pin beside the new result, with PSNR for both, the readout difference and a pinned-vs-current heatmap. There is one pin; closing the preview, changing texture or opening a folder drops it.
- In a window 1280 points wide or wider the texture's settings start open beside the picture and stay open when Advanced opens. In a narrower window the picture is never narrower than 480 points: when the settings do not fit beside it, they start as a narrow **Settings** strip; click it to open them. They then stay open until the preview closes.
- A DDS file previews as stored (no source side). BC2 and signed BC6H cannot be previewed.
- Without a usable GPU the preview is off, and everything else still works.

## Export

**Export** converts the folder into the export folder. It cannot write into the folder it reads.

1. Export saves `sdc.toml` and `sdc-project.toml`.
2. If something is blocked, undecided or worth a second look, **Review** opens in the bottom panel instead of exporting (see the bottom panel, [Main window](#main-window) item 13). Decide the textures there (or **Accept all suggestions**), fix the blockers, tick the warnings, then press Review's **Export**, which exports the decided textures and skips the rest. It never decides for you and never exports by itself.
3. If a source changed on disk since it was planned, Export plans again first. The Textures column also offers **Reload** for changed files.
4. The bottom panel opens on its *Files* tab (one row per file: queued, running, converted, validation failed, up to date, skipped, failed, cancelled). Click a row to select that texture; hover it for the file's warnings, which *Problems* also lists.
5. After a complete export, the PGPatcher JSON is written (see below).

While it runs the Files tab shows done/total, elapsed time, files per second and an ETA. The *Files* tab handles thousands of rows, with **Failed only** and **Has warnings** filters; **Show in Explorer** on a row selects the output. Buttons: **Cancel** stops the batch. **Open export folder** opens it in Explorer. **Save report…** writes the report as JSON Lines or Markdown. **Close** collapses the panel. **Copy CLI command** copies an `sdc convert` command for this export: the same folder (with `--recursive`), output, BC1 decoder and thread count, plus `--accept-suggestions high`, because the GUI accepts High-confidence suggestions. Before you run it, check two things it does not carry over:

- It has no `--nvtt`. If you chose an NVTT path in Advanced, add `--nvtt <path>`.
- The CLI does not read `sdc-project.toml` or the scan. Set edits from the Sets column (moves, borrows, created sets, shader choices, output names), alpha taken from scanned meshes and the complex-grass auto-tag do not apply to `sdc convert`. Decisions written to `sdc.toml` do.

If an export is blocked, Review's *Blockers* column lists what to fix; **Go to set** selects the set.

Review's footer also lists **Stale outputs (n)**: files an earlier export wrote that the project no longer writes (a renamed set's old files, a skin set's old `_f.dds` or `textures\pbr\` files, the old `_rmaos.dds` after a recipe switch, the output of a deleted source). A file still exactly as Tessalith wrote it has **Delete**, which acts on a mouse click only; a file changed since reads "edited since Tessalith wrote it: left alone" and has no button. Old JSON files Tessalith wrote (a renamed PGPatcher JSON, a Community Shaders PBR or complex-material JSON no longer written) are listed the same way, checked against the hash Tessalith recorded. The rows never block Export; while any has **Delete**, Problems shows "*n* old outputs Tessalith wrote are no longer exported" with **Review…**.

Generated maps' records and the hashes of the JSON files Tessalith writes are also saved at once in `_sdc/generated.json` beside `sdc-project.toml`, so closing without saving, discarding edits or a crash does not lose them.

### Also export as Vanilla and CM

In the cs mode one export can also write a vanilla and a complex-material (CM) version of a PBR mod.

- **Switch it on** with **Export to ▾** (**More ▾** with no project open) **→ Also export as Vanilla** or **Also export as Complex material** (checkable, no dialog; "Open a folder first" without a project), or with the ticks in Review's **Also export as** column. The column shows in the cs mode or while a tick is on; its red reason line shows only while a tick is on.
- **While a tick is on, Export always opens Review.** Review's Export writes three folders side by side: `<project> PBR` (the normal cs export, with its JSON), `<project> Vanilla` and `<project> CM`. Ticking only CM writes Vanilla too.
- **Review's column** also holds **Also folders go in** (**Choose where the Also folders go…**; per project, `[project] also_location` in `sdc-project.toml`; by default the export folder, shown as "the export folder (not chosen yet)" until one is chosen), the three folder names, **Keep metal brightness** and its strength (CM), **Also write a zip of each folder** and **Skip these sets**.
- **Install** the PBR folder or the Vanilla folder. For CM, install the CM folder on top of the Vanilla folder. With Keep metal brightness off, the CM folder holds only `_m` files (plus the complex-grass stack of each PBR grass set). Files an earlier export left in the folders are kept and listed in the report as "not written by this export"; delete them yourself.
- **Vanilla** gets a converted diffuse, a `_n` with the specular mask in alpha and, where the meshes use the environment-map shader, an env `_m`. That needs a Meshes scan; without one no env `_m` is written. A PBR Landscape set's `_p` goes into Vanilla too (Terrain Helper). Textures that are not PBR are copied from the PBR folder when they would be the same. **CM** gets one `_m` per PBR set.
- **Skip a set** with Review's **Skip these sets**, or, in 2D, **Skip vanilla/CM for this set** in the set editor's Material tab.
- **NIF edits** go into the PBR folder while a tick is on.
- **The recipe** is the **Also export recipe** section in Advanced (closed at first; Review's **Recipe settings are in Advanced** opens it). See [Project Config](Project-Config.md#export) for each value.
- **After the export** `<project> conversion-report.md` sits next to the folders: counts per folder, the recipe, a row per PBR set with a suggested NIF Glossiness and Specular Strength and whether it needs the Environment Map shader, the sets not converted, and how to install. With the zip on, `<folder>.zip` per folder and `<project> SHA256SUMS.txt`; the PBR zip includes the PGPatcher JSON. A zip too large for the ZIP format (65,535 files or more, or 4 GiB) is a Review blocker before anything is written: untick **Also write a zip of each folder** or split the export.

## Sets and the set editor

### Editing sets

- **Drag a texture** from the Textures column onto a set to move it there, or onto empty space to start a new set. Dropping onto a filled slot asks **Replace** or **Cancel**.
- **Share** lets a set borrow another set's finished output. **Duplicate** copies the source into the set. A shared `_rmaos` is never variance-baked.
- The number at the left of a set is its place in the full order (1 to N). A filter does not renumber it.
- **≡** drags a set to reorder (off while a Sets filter is on). A drag reaches only the cards on screen; for a long move use **Move to…**.
- **Right-click** a set for its menu: a **Rename** submenu (**Rename…** for the set's name, **Rename outputs…** (F2), **Rename inputs…**), **Edit match…**, **Remove member**, **Delete set**, **Move up**, **Move down** and **Move to…**. **Move to…** opens a dialog: type a position (1 to N) or part of a set's name, pick a row with Up, Down and Enter, and the set moves there; it also works while a Sets filter is on. **Delete set** asks nothing: it is one undo step, and its toast has an **Undo** button. Only sets you created can be deleted; an automatic set would regroup from the file names at once. Right-clicking a member that fills a slot opens that slot's menu instead. A set whose files are all gone still shows, as missing, so you can delete its entry.
- **Fill from names…** (set menu) lists files in the set's folder named like its diffuse plus a known suffix, and moves the ticked ones in as one undo step.
- Dropping a file from Explorer on a slot row fills that slot when the file is a texture of the open folder and its sub-type fits; files are never copied.
- Each set is a card: its name, a status pill with a dot (red blocks export, amber not ready, green ready) and what the set is ("vanilla", "PBR → JSON", "PBR Landscape", "skin"), the **shader** combo across the full width, and one lettered slot chip per slot (D, N, RMAOS, P and so on). A filled chip is the thumbnail and drags, selects and opens its slot menu; an empty one is a dim box; amber-outlined means borrowed from another set; a red **?** is a file no match was found for. "⚠ n" opens the set's problems. Hover the pill for the long text: a missing file, a blocking problem, and which JSON a set writes ("PBR Landscape → CS + PGPatcher JSON", "→ CS JSON", "→ PGPatcher JSON (no CS records yet)" or "PBR Landscape (no JSON yet)").
- The Sets column has its own filter: a *Search sets* box, the pills *Not ready*, *Ready* and *No JSON* with counts, and **Filter ▾** for a shader. A filter only hides: the open set and a multi-selection stay selected and the editor stays open. With nothing matching, the column says "No sets match the filter" and offers **Clear filter**. Reordering is off while a filter is on, and a set you create while a filter is on stays shown until the filter changes (the column says "1 new set shown"). The pills and the shader are remembered; the search text is not.

Set edits are saved in `sdc-project.toml` next to `sdc.toml`: set order, created sets, moves, borrows, shader choices, output names, slot pins and TruePBR settings. If that file cannot be parsed, sets and the JSON go read-only while `sdc.toml` editing carries on; a red Problems row names the line, with **Open in editor** and **Reload**.

### The set editor

Select a set to edit it. The heading has the input name box and a **2D | 3D** switch (remembered in `gui.toml`, default 2D) that swaps the centre pane in place; in 3D the settings fold in sections above the view.

A set whose diffuse is an **Advanced skin** gets an **Advanced skin (Community Shaders)** card instead of the TruePBR groups in the Material tab: 20 per-material skin settings, each "not set" or an explicit value saved in `sdc-project.toml`. Such a set has no PGPatcher JSON entry, and its files go to their normal paths. Export does not write the skin settings to Community Shaders yet.

In 2D the editor is one column that scrolls as a whole: the input name box, the slot rows at full width, then a **Material** tab (**Set** on a vanilla set) with the set's material settings and a last **Advanced** tab with the same sections as the Advanced drawer. Nothing sits beside the slot rows, so nothing covers them at any window width. While the tabs are shown the drawer is not; in 3D, with a texture selected, or with several sets selected, the drawer is back.

Each PGPatcher slot is one row (at most 8), in slot order: Diffuse, Normal, Emissive _g (or Skin _sk), Height, RMAOS, Coat normal / Fuzz (slot 7: a `_cnr` or an `_f`), Subsurface. A set with both a `_cnr` and an `_f` (or both a `_g` and an `_sk`) shows the conflict in red on that row, with both files listed and a ✕ beside each to take one out of the set, and export warns about it. The cubemap slot has no row; set it with a slot command. Only the slots the set's shader uses are shown. A filled slot the shader cannot use keeps an amber outline, with the reason on hover.

A row has these columns: Slot (title; weak when empty; a lock when pinned), the thumbnail (click selects, double-click expands), File (the name cut to fit; full name and size on hover; the dimensions and file size under it), Format (the dropdown; an empty slot shows **Assign...**, which opens the slot menu, headed by the slot's name), Flags (Match on this and Opaque, each with a hover) and Status (a dot: green ready, amber a warning or decision, red blocks; the reason is on hover). The File column takes the width the other columns leave, so the rows fill the centre.

| Row control | What it does |
|---|---|
| **Slot commands…** | *From this set*, *Path*, *Clear*, and *Pin slot N* (the sdc pin). Each is one undo step. |
| Right-click | The slot commands plus *From another set*, which links the slot to another set's map, *Pin this slot (always use this file)* / *Unpin this slot*, and *Keep the original texture (PGPatcher won't replace it)*. *From this set* is off, with "This set has no file for this slot", when the set has none. |
| **Opaque** (Diffuse) | Strips alpha: the diffuse is fully opaque. |
| **Match on this** (Diffuse, Normal) | PGPatcher applies the entry to meshes with this exact texture in this slot. |
| RMAOS / RFAOS (skin) | On a packed output's card: the recipe and per-channel fills. **Reset** removes a fill. |

A pinned slot keeps its fill. It refuses drags in or out and the slot commands, and bulk edits skip it. The pin is saved in `sdc-project.toml` and never written to the JSON.

*Keep the original texture (PGPatcher won't replace it)* (*Let PGPatcher replace the texture* turns it off) turns on the slot's PGPatcher lock key (`lock_normal` and so on, the same key as in Texture locks): PGPatcher keeps the mesh's own texture in that slot. The card shows a grey padlock over the dimmed thumbnail, captioned *kept by PGPatcher*; its hover says "Kept by PGPatcher (lock_<slot>): the mesh's original texture is used here" and, with a scan, lists the meshes' textures in that slot, and the member thumbnail a small grey padlock. The file is still exported. A slot command on the same slot overrides the lock in PGPatcher, and a warning says so. The cubemap is not offered: `lock_cubemap` clears it.

A `_coat` map fills the Subsurface card, which then reads "Coat colour + strength". When RNM normal layers are active, the normal card has an outline in the accent colour (teal); see [RNM layers](#rnm-normal-layers).

**Shader** lists the shaders the current Mode offers. The pick reads "(auto)" when Tessalith chose it and "(fallback)" when the shader you asked for does not exist under this Mode. **Auto** clears your choice. Shader warnings show on the set card's **⚠ n** button. See [Texture types](Texture-Types.md#set-shaders).

**Used by plugins** lists the plugin texture sets (TXST) and alternate textures that use this set, from the last scan, with **Refresh**.

## TruePBR settings

A PBR set under `cs` has the settings PGPatcher reads in the set editor's Material tab, as framed groups in the order below. Each header shows the title and, on the right, its value (for example Subsurface "Standard", Coat "off", Matching "2 matches", CS PBR records "1 record"). Click a title to open or close the group. The feature groups are always listed; they read "off" and stay closed while the feature is off, and open when it turns on. A feature's state shows only there: the Features group, which holds the ticks, has no value. A small **TruePBR JSON editor…** button beside the Material tab opens the editor as a centre tab. Skin and complex-material keys show readable names ("SSS width"); hover one for its exact key.

| Group | Settings |
|---|---|
| Material | Specular level, Roughness scale. |
| Features | Emissive, Parallax, Hair, Multilayer (coat), Glint, Fuzz. |
| Emissive settings | Emissive scale, Emissive colour. |
| Parallax settings | Displacement scale. |
| Subsurface | The Off / Standard / Foliage selector, then colour and opacity for Standard and Foliage. |
| Coat | Under the Multilayer checkbox: coat colour, specular level, roughness, strength, diffuse, normal, parallax and inner UV scale. With Multilayer off the group reads "off" and stays closed; the values are kept but not written. |
| Glint settings, Fuzz settings | The glint parameters; Fuzz colour and weight. |
| Matching | The exact texture paths the entry matches (PGPatcher's `match1` to `match8`). |
| CS PBR records | Community Shaders' PBR texture set and material object blocks (collapsed; "N records"). |
| Advanced TruePBR settings | Path contains, Mesh path contains, Rename; UV and cubemap (Smooth angle, Auto UV, UV scale, Cubemap path); Vertex colours and depth; Texture locks. |
| NiAlphaProperty (starts closed) | The schema's `alphaprop_*` keys: override flags (blending, blend modes, testing, test function, no sorter, or a raw number), alpha test threshold, create if missing, delete. A key that is off is not written. |

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

After a complete export, Export writes `<export folder>\PBRNIFPatcher\<name>.json`, with `<name>` from the editor's **JSON name:** field (by default the folder name). Only sets that exported fully are written. If the file exists and is not the one Tessalith wrote last time, you are asked before it is overwritten; a byte-identical file is never prompted. The JSON is written in `cs` mode only.

### The PGPatcher JSON editor

Open it with **PGPatcher JSON editor…** in the header's **Export to ▾** menu (**More ▾** with no project open) or from the **PGPatcher JSON editor…** button in a set's Material tab. It opens as a tab of the centre column; **Pop out** moves it to a window of its own, titled "PGPatcher JSON — Tessalith", and **Dock** brings it back (see [Presets](#presets)). Its top field, **JSON name:**, names the JSON file; a change applies when you leave the field, as one undo step.

- Each PBR set's entry is a pill with its title, output prefix and **Show** (selects the set and brings its editor to the front), then its keys as small pills with the same controls as the Material tab's groups.
- **+ key** adds any schema key. Keys Tessalith does not know are shown in amber and kept as written; they can be removed.
- A texture match pill picks how it matches: *Match: exact, slot N (diffuse)* (`matchN`) or *path contains*. A set's own diffuse path reads *derived from the set*. A lock key reads *Keep the original texture (PGPatcher won't replace it): diffuse* (`lock_diffuse`, shown on hover). In Slot commands the box that locks slot N is named *Pin slot N (always use this file)*. The **▾** picker lists the scan's textures for that slot, or every known texture path. The *mesh path contains* pill (`nif_filter`) has a picker of folders that hold scanned meshes. Without a scan, the pickers list only the project's own paths.
- **+ Add a delete section** adds an entry that belongs to no set and makes PGPatcher delete the shapes it matches, for example a vanilla glow shape the PBR base shape replaces. Sections are saved as `[[pgp_sections]]` in `sdc-project.toml` (keys `match1` to `match8`, `path_contains`, `nif_filter`) and written with `delete: true` after every set's entry. A section with no texture match is not written.
- A delete section that hits one of your sets (by exact match on the set's texture, or `path_contains` in its diffuse path) strikes that set's pill through, outlines it amber and warns. The set editor shows the same warning with a button to the editor.
- The header reads "N errors, M warnings". Errors block Export and show as a red line under their entry; warnings are what Export asks about. **Next error** / **Prev** (F8 / Shift+F8) jump between entries with errors, and a search box filters entries by set name or path.
- **JSON text** shows the file as it would be written now, with **Copy**.

Edits in the editor and in the Material tab are the same edits; each shows the other's at once, and undo is shared.

## Set preview (3D)

Switch the set editor to **3D** to see the set on a mesh, rendered with Direct3D 11 and your own Community Shaders shader files. The preview does no work while it is hidden.

| Control | What it does |
|---|---|
| Encoded (exact export) / Source (faster, no compression) | Show the bytes Export would write, or the prepared source without compression. |
| **Mesh:** | *Auto* (the first scanned game mesh that uses the diffuse) or a built-in shape: Sphere, Plane, Cube, Material ball. The scanned meshes are listed loose before BSA, with ground models (`gnd`, `_go`) and `1stperson` meshes last. |
| **Shader folder…** | Point at your Community Shaders mod folder or its `Shaders` folder when it is not found automatically ("Community Shaders not found", **Choose folder…**). |
| Community Shaders features | On by default. Off compiles the shaders without CS feature defines. |
| **Accurate (deferred)** (in **View**) | On by default. Opaque shapes draw through Community Shaders' own deferred pass and composite, as in game; blended shapes still draw on top the forward way. Off uses the older forward path. If your Community Shaders build cannot run it, the view stays forward and a notice says why. The setting is remembered. |
| Background | Solid or Studio HDRI (Poly Haven, CC0). For Solid, a colour swatch with a hex field and a *Grey* slider (0 to 100 %) set the grey level; a non-grey colour is turned into its grey. |
| Sun brightness, Point light brightness and radius | Lighting. |
| **Reset view** | Reset the camera. |

Moving around: drag to orbit, scroll to zoom, middle-drag or Shift+drag to pan, Shift+right-drag to turn the sun, double-click or Home to recentre. **View** has a debug view (Lit or UV checker; V steps through them), **Accurate (deferred)** and **Reset view**. Missing textures show in magenta, listed under the frame with **Open in Textures** when the file is in the folder. **Copy image** (Ctrl+Shift+C) copies the frame to the clipboard.

Notes:

- The PGPatcher simulation needs the `cs` target and a PBR set. Other sets show their plain textures.
- To preview on a real mesh, scan your mods first. "No scanned mesh uses this diffuse" means the scan found none. Meshes from Skyrim LE are not drawn; a built-in shape is shown instead. A built-in shape cannot match an entry's match keys, so it shows the plain textures.
- The preview assumes your JSON wins over other mods' PGPatcher configs.
- Large textures are previewed at a capped size ("Preview at N px, export writes full size").
- These Community Shaders features are off in the preview: Light Limit Fix, Skylighting, Screen-Space Shadows, Subsurface Scattering, Image-Based Lighting, Physical Sky, Wetness, Cloud Shadows, terrain effects, water, fog, Volumetric Shadows.
- A timing log is written while the preview runs. Set the environment variable `SDC_PREVIEW_LOG=0` to turn it off.

## Cubemaps

A texture whose sub-type is **Cubemap** (under *Other* in the sub-type dropdown) gets a **Cubemap** section in its settings panel and a cross in the preview. Everything the panel writes goes to `sdc.toml` as the matching `cubemap.*` key, so the GUI and the CLI make the same bytes (see [Cubemaps](Cubemaps.md) for what each setting does).

- **In the Textures column.** A set of six face files is one item named `<output name> (6 faces)`. The size estimate counts all six faces and their mips. A six-file set's settings are written to all six face entries in `sdc.toml`, so the faces keep matching.
- **Settings panel.** The section is drawn only when every selected texture is a cube, and has five groups: **Source** (layout, face size, and for a face, cross or strip source the per-face rotation and flip, plus *Turn -Z upright* on a vertical cross), **Orientation** (yaw, *Mirror east and west*), **Colour grade** (exposure, brightness, contrast, midtones, saturation, tint, highlight clamp), **Ground** (*Ground fill* and *Horizon*) and **Look** (style and blur).
  - The layout box never offers six face files. A six-file set shows "Six face files" as read-only text.
  - **Horizon** is disabled with "needs a ground fill" until a ground fill is set.
  - A blur the engine fills in (a photo's 0.6, or the blur of a style or preset) cannot be unticked, so set 0 for no blur.
  - A value in `sdc.toml` outside the engine's range is shown as it is, with the engine's message in red under the row. Tessalith never changes it; fix it in the box or in the file.
  - Colours are raw 0 to 255 codes, not gamma-converted. A drag or colour pick writes once, when you let go.
- **Unclear layout.** A single image Tessalith cannot place (a square one with no `cubemap.layout`, say) shows "This image could be several cube layouts: pick one" in the settings panel's *Undecided* box, once the sub-type is confirmed. Pick a layout there (360° panorama, mirror ball, angular map or single photo; a cross or strip would already have been recognised); it is written for that file only. In the Review tab its row has a *Set layout ▾* menu with the same layouts.
- **Cube preview.** The encoded side draws all six faces of the mip on show as one cross (4 by 3 faces), sized like a 2D preview, so a big cube previews at a lower mip. *Face labels* (on by default) names each face. *Heatmap*, *Tile 3×3* and *Pin* are off for a cube ("Not for cubes"), and there is no texel readout.
  - A six-file set has six source buttons, +X to -Z, over the source side. Each shows the processed face (after turns, flips, sizing and mips), the same picture as its cell in the cross.
  - A single-image cube's source side shows the input file itself (panorama, ball, cross, strip or photo).
  - A set that cannot be built shows the engine's reason (for example `CUBE-FACES: faces differ in size`) in the encoded pane.
- **Make marker cube…** in **Export to ▾** (**More ▾** with no project open) opens a small dialog for a 1×1 dynamic-cubemap marker. It works with or without a project open.
  - **Colour** is raw codes; the hint reads F0 = (code/255)^1.6. **Roughness** is optional (the *Set roughness* tick): ticked it starts at 0.5, unticked the alpha is 255. The alpha it will store is shown under the controls.
  - **Save…** opens the file dialog (Enter never writes), in `<textures folder>\cubemaps` when that folder exists, named `marker_e.dds`.
  - It never overwrites a file Tessalith did not write; it says it kept it and leaves the dialog open so you can pick another name. The bytes equal what `sdc cubemap-marker` writes.

## RNM normal layers

A set's `_bump`, `_bump2`, `_detailnorm`, `_detailnorm2` and `_bent` files (and their masks) blend into its `_n` (or `_cnr`) before the mips are made. They are not shown as tiles; select the normal to see them.

- The normal's settings have an **RNM** section: a switch for the whole blend, a strength slider per layer, the layer order, and for `_bent` the AO options (*AO blend* or *Like a detail*, *AO power*, *use AO*).
- When the set has both `_n` and `_cnr`, each layer has a target switch: *Normal*, *CNR* or *Both*. It is written as `rnm.target` on the layer file's `[files]` entry.
- The `_bent` AO line shows the AO source (a raw AO map, or the B channel of `_rmaos`/`_rfaos`) as an inverted thumbnail. Supply normal AO (white = open); Tessalith inverts it.
- Set-level notes (no base for a layer, a layer ignored on an eye set, an orphan mask) appear under the set grid.

## Generate from normal map

Right-click the Height row (or open the set card's menu) and pick **Generate from normal map…** to make a height map from the set's normal. The dialog has Size, Tiling, Relief balance and Strength, with a preview; **Generate** writes `<base>_p_generated.png` (or `<base>_height_generated.png` for a complex material's `_m` alpha) as a 16-bit grey PNG next to the normal. See [Texture types](Texture-Types.md#generated-height-maps) for names and ranges. The same entry is a small **Generate…** button directly under **Assign…** on an empty Height or RMAOS row, and under the format combo on a filled one (**Regenerate…** once a generated file fills the slot); it is hidden for a packed texture, and in the preview the wheel zooms, a drag pans and a double-click fits.

- The Height row's File hover says "Generated from {normal}", and the Flags cell shows a weak **generated** marker.
- When the file is stale (the normal changed, the set wants the other neutral, or the shader reads no height), that row's status dot turns amber and its hover gives the reason. Problems gets a warning row on the generated map with **Regenerate…** (its set's dot is no longer ready), which opens the dialog with that map ticked; nothing is written until you press Generate.
- **Regenerate…** reuses the stored settings, with the set's current neutral and a size the normal still offers. A file you edited is never overwritten.
- **Delete generated file…** (row menu, or the set card's submenu) asks first; Cancel is the default. The box says when the file was edited or has no record; your confirm deletes it. It cannot be undone.
- A new `_p` makes a default set Parallax (PGPatcher then patches its meshes), and turns on `parallax` on a TruePBR set.

**AO.** On a PBR set under Community Shaders, right-click the `_rmaos` row (or tick **AO** in the dialog) to generate `<base>_ao_generated.png`. A baked AO is better; this is for sets with none. The AO group has Radius, Quality, Depth and Intensity (0.7–1.0 is a good range: CS applies AO to indirect light itself). When the set has a roughness or other `_rmaos` source, the AO takes its size. The file becomes the `_rmaos` blue channel (`GEN-AO`); the `_rmaos` row's File hover says "AO generated from {normal}", its dot turns amber when the AO is stale, and it offers **Delete generated file…**. A set with an RNM bent layer (where AO can be generated) gets a note at the top of the dialog: the AO also changes its normal map.

## Advanced drawer

The drawer at the right edge holds settings that are not per-texture, plus a few rarely used texture fields. With a set selected in 2D the same sections are the set editor's **Advanced** tab instead, and the open/closed choice is shared by the tab and the drawer. In 3D the drawer stays at the right edge.

| Section | Contents |
|---|---|
| Preset inspector | Headed *How {file}'s settings resolve* for the selected texture. Under it: the batch preset, and the user presets that loaded (with their files); with none loaded, a **Presets…** button opens the preset creator. Field names read in words (the key shows on hover); long values are cut to fit and show in full on hover. *Show only what differs from sdc-base* hides values equal to the base preset. Built-in presets are read-only. |
| Batch | Worker threads, Memory budget (MiB), BC1 decoder (Ideal, Ideal (round 4), NVIDIA, AMD). The BC1 decoder is read at start, so a change shows "restart Tessalith to apply". |
| NVTT | NVTT path, **Choose nvcompress.exe…**, **Automatic** (`PATH`, then the NVIDIA install folder). The status shows the version found and whether it is the tested 3.2.5, and is re-checked when the window regains focus. |
| Project-wide fills (legacy) | One line, shown only while `sdc.toml` holds fills in `defaults.packing.fill.*`. **Clear** removes them; set fills on each packed output's card instead. |
| Preview | Largest previewed side, Allow 16384 previews, Live encode. |
| Mipmaps | Levels for the selected texture. |
| Advanced texture fields | BC7 weighting, BC1 transparent black, BC4/BC5 search radius (Default, Maximum, Fast (preview)), rgbcx level, DDS header (Auto, DX10 extended header, Legacy FourCC), Resize, Half size. |
| Checks | The switches for the artist checks, **Measure whether full size adds anything** and **Check all UV coverage**. See *Artist checks* below. |
| Also export recipe | One box per `[export.legacy]` value of [Also export as Vanilla and CM](#also-export-as-vanilla-and-cm). Starts closed; Review's link opens it. |
| Unmatched [files] entries | `[files]` entries that name no file under the current target, with **Remove**. |

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

| File | Contents |
|---|---|
| `<folder>\sdc.toml` | Target and per-file decisions. Shared with the CLI. |
| `<folder>\sdc-project.toml` | Sets, shaders, slot pins, output names, TruePBR settings, JSON name, delete sections, the export folder, and the hashes of the JSON files and borrowed-slot copies Tessalith wrote, and the records of generated files (`[generated]`). GUI only. |
| `%APPDATA%\sdc\gui.toml` | Window and app settings: the default export folder (a project's own is in its `sdc-project.toml`), recent folders (10), preview limit, 16384 previews, Live encode, BC1 decoder, worker threads, memory budget, column visibility and widths, window size and position, Textures filters, sort and tile size, 2D/3D choice, 3D View controls, Advanced drawer state, NVTT path, the scan source (`[meshes]`) and the 3D preview's shader folder and size (`[render3d]`). Never part of `sdc.toml`. |
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
| Arrow keys, Shift+arrows, Ctrl+A | Move through the Textures grid, extend the selection, select all. |
| Enter / Space | Expand the preview of the active texture. |
| F2 | Rename. |
| In the preview: [ ], 1-6, PageUp/PageDown, L, T, P | Previous/next texture, channel, mip, lens, tiling, pin. |
| In 3D: Home, V, Ctrl+Shift+C | Recentre, next debug view, copy image. |
| F8 / Shift+F8 | Next / previous error in the JSON editor. |

Shortcuts leave a text field alone, except Ctrl+K (the palette) and Ctrl+S, which keeps what you typed and saves; Esc leaves the field. Clicking Save or Export while typing also keeps what you typed, in one click.
