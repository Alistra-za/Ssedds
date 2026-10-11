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

The columns can be collapsed to thin strips (**Collapse columns**). The **Advanced** drawer at the right edge holds the preset inspector, batch settings (worker threads, memory budget, BC1 decoder), packing fill values, the preview settings and a list of unmatched `[files]` entries.

## Deciding

The GUI never decides for you. A texture that needs an answer has an **amber border**: its sub-type, the meaning of its alpha, the green-channel convention of a normal map, or a blocking warning you must acknowledge. Select it and use the confirm controls in the centre column (for example the sub-type buttons, the *Remove / Cutout / Blend / Cutout + Blend / Data* alpha buttons, *DirectX / OpenGL* for green, or an acknowledge button for a blocking warning). Each answer is written explicitly, as a field value or as an `allow` code, so it stays in your project file.

## Editing

Any field in the centre column can be changed. Changes go into `sdc.toml` as `[files."<path>"]` entries, keyed by the file's path relative to the folder holding `sdc.toml` (packed outputs without an extension, for example `landscape/rock01_rmaos`). The GUI writes only `target`, the `[files.*]` tables and `defaults.packing.fill.*`; comments, ordering, rules and every other table in your `sdc.toml` are left byte for byte as they were.

**Normal base level** (normal maps only) stores mip 0 at unit length by default; choose *Keep as authored* for a normal map whose short normals are deliberate. A PBR set's normal map also bakes its detail into the set's `_rmaos`/`_rfaos` roughness (including a normal map shared into another set, which bakes into that set's own roughness); its Normal handling row then reads *decided by the texture set*. A `_cnr` bakes its detail into its own coat roughness by default, so its row reads *built-in default*.

An edited file is marked **●** until you save with **Save** or **Ctrl+S**. Export saves first. If `sdc.toml` changes on disk while you have unsaved edits, saving asks whether to reload it or overwrite it. Closing the window or opening another folder with unsaved edits asks first. So does closing or opening a folder while an export is running: the dialog offers **Cancel export and continue** (the export stops, then the window closes or the folder opens) or **Keep running**.

Per-texture `[files]` entries sit above preset and config rules and analysis, and below any `--set` on the command line, so `sdc convert <folder>` honours them.

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

Set edits are saved in `sdc-project.toml`, next to `sdc.toml`. It holds the set order, created sets, moves, borrows and the PGPatcher settings, and nothing else. If it cannot be parsed, the sets and the JSON go read-only (a banner says why) while `sdc.toml` editing carries on.

### The set editor

Select a set to edit it. Each texture slot is a card: Diffuse, Normal, Parameter, Height and so on. A card has a **preset dropdown** (it fills the PGPatcher settings for that kind of material), and **Slot commands** to move, share, duplicate or clear the slot. **Opaque** sits on the Diffuse card.

### PGPatcher settings

- Features that cannot go together are **greyed**, with the reason on hover: hair excludes multilayer, glint, fuzz, emissive, parallax and subsurface; multilayer excludes subsurface; multilayer, glint and fuzz exclude each other; fuzz and coat normal exclude each other, because they share one slot. A switch that is already on stays enabled so you can turn it off.
- A feature that is on without its texture blocks export.
- **Reset** returns a field, or the whole set, to its default.
- PGPatcher's own `default` block is fixed: you cannot edit defaults. Each entry writes `emissive`, `parallax` and `subsurface` only when on, and omits the three numbers when unset or equal to the fixed value.

### The JSON

After a complete export, **Export** writes `<export>\PBRNIFPatcher\<name>.json`. Only sets that exported fully are written, and the file name must be a single plain file name. If the file already exists and is not the one sdc wrote last time, you are asked before it is overwritten; a byte-identical file is never prompted. With a read-only `sdc-project.toml` the JSON is still written, but the next export asks before overwriting.

### Check by hand

The test harness cannot simulate drags. Reordering sets by dragging the **≡** handle and the drag-and-drop flows (texture onto a set, onto empty space, onto a filled slot) were verified by code review only; try them yourself.

## Where settings live

GUI-only settings (recent folders, export folder, preview limit, Live encode, worker threads, memory budget, BC1 decoder) are in `%APPDATA%\sdc\gui.toml`; they are never part of `sdc.toml`. The BC1 decoder model is read once when the GUI starts: after changing it the GUI shows "restart sdc-gui to apply". The user config of the command line (`%APPDATA%\sdc\config.toml`) also applies to the GUI.

sdc-gui ships no fonts. It loads the Windows system fonts (Segoe UI, Segoe UI Symbol, Segoe UI Emoji, Consolas) at start-up and refuses to start, with a message, if they are missing.

## When something goes wrong

- A background job that crashes shows an "internal error in ..." dialog; the window keeps running.
- A failed plan or export shows its error in a dialog and in the status line.
