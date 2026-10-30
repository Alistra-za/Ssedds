# Mod scanning

The GUI can scan your installed mods: the meshes (NIF files, loose or in BSA archives) and, for a Mod Organizer 2 instance, the active plugins. The scan tells the GUI which meshes use each of your textures, and the GUI uses that to fill in decisions you would otherwise make by hand. Scanning is a GUI feature; `sdc` on the command line never reads the scan.

Scanning is read-only and never changes your mods, your MO2 instance or your game folder.

## What the scan is used for

| Use | Where you see it |
|---|---|
| Alpha meaning of colour maps | A colour map whose alpha varies takes Blend, Cutout (at the strictest mesh threshold), Cutout + Blend or Remove from the meshes that use it. The Alpha row reads "from the meshes that use this texture". See [GUI guide](GUI-Guide.md#alpha-from-the-meshes). |
| Set shader | When no map in the set decides it (PBR, Advanced skin, complex grass, complex `_m`, `_p`), the shader most of the scanned meshes use for the diffuse picks the set's shader. |
| Complex grass | After a scan that included plugins, a texture that is the diffuse (slot 0) of a grass (GRAS) record's mesh gets a High-confidence `complex_grass` suggestion, which the GUI accepts, under either target. Only when nothing explicit (a rule, `--set`, `[files]` or a set shader) decided the sub-type, and the file passes the complex-grass shape tests (1:2, detection corner, unit normals). |
| Path pickers | The input name box and its ▾ list, and the match pickers in the TruePBR JSON editor, list texture paths and mesh folders the scan found. |
| Rename warnings | Renaming an output that scanned meshes still use under its old name (as the diffuse, slot 0) is flagged amber. PBR sets under `cs` are not flagged. |
| 3D preview | *Auto* in the set preview shows the set on a scanned game mesh that uses its diffuse. |
| Plugin references | The set editor's "Used by plugins" and the Meshes view's "Plugin file references…" window. |

Your own choices in `sdc.toml` always win over scan evidence.

Because the CLI does not read the scan, a folder decided partly from scan evidence can convert differently from the command line. Alpha meaning from meshes, mesh-voted shaders and the complex-grass auto-tag come from the scan and are not stored in `sdc.toml`. If you also convert with `sdc convert`, set those in `[files]` entries (`alpha.policy`, `alpha.threshold`, `subtype`) or with `--set set:<id>.shader=<shader>`.

## Choosing what to scan

The scan source sits behind the **MO2…** button in the header (on both the Textures view and the Meshes view); its hover text reads "MO2 instance X, profile Y" or "Folder X". The button opens a popup with the instance and **Choose…**, the instance's profiles (click one to switch), **Scan a folder…** and **Scan** / **Cancel**. A click inside the popup keeps it open; a click outside or Esc closes it. While a scan runs, its progress bar and ETA show on the scan row under the header.

### A Mod Organizer 2 instance

**Choose…** in the MO2… popup picks the instance. Tessalith reads `ModOrganizer.ini` for the game path, the mods, overwrite and profiles folders, and the profile. It then follows that profile's mod list:

- Enabled mods (`+` in `modlist.txt`) in priority order, with the game's `Data` folder lowest and `overwrite` highest. Disabled mods and separators are skipped. An enabled mod whose folder is missing is reported.
- The active BSA archives: every `Skyrim - *.bsa` in the game's `Data` folder (always active), then the archives the game INIs list (falling back to `Skyrim_Default.ini` in the game folder when the profile's INIs list none), then the archives the active plugins load.
- The active plugins and their load order: the five base masters, then `Skyrim.ccc` entries that are not in the profile's `plugins.txt`, then the profile's active `plugins.txt` entries.

For each file path, the scan works out which copy wins in game (the highest-priority loose file or archive).

If you start `tessalith.exe` from inside MO2, it detects that and asks: "Opened from Mod Organizer 2 instance '…', profile '…'. Use it?" with **Use**, **Choose instance…** and **Open a folder instead**. Tessalith finds the instance through `portable.txt` next to `ModOrganizer.exe`, or MO2's current instance under `%LOCALAPPDATA%\ModOrganizer\`.

### A folder

**Scan a folder…** scans the meshes under one folder, for example an extracted mod or a mod you are building. A folder scan has no plugin pass, so plugin references and the complex-grass auto-tag are not available.

### Running a scan

Press **Scan**. Progress shows on the scan row; **Cancel** stops it. Cancelling during the plugin pass keeps the mesh results and notes that plugin results are incomplete until the next scan.

The scan skips FaceGen meshes and LOD meshes. Meshes that are neither Skyrim SE nor Skyrim LE NIFs are counted and skipped. Meshes from Skyrim LE are read but not drawn in the 3D preview.

The scan database is `%LOCALAPPDATA%\sdc\scan.db`. Only one scan can run on it at a time. After an update changes the database format, you are asked to scan again.

## PGPatcher output

PGPatcher (ParallaxGen) writes patched copies of meshes. Tessalith detects PGPatcher's output folder by its marker files:

- `ParallaxGen_Diff.json`
- `PGPatcher.esp` or `PG_<number>.esp`
- `PGPatcher_UpdateCache.bin`
- `meshes\_pgpatcher_dups`

It treats that output as a layer derived from the source mods, links each patched mesh to its source, and marks the patched copies so you can open the source mods instead.

Banners about PGPatcher:

| Banner | Meaning and action |
|---|---|
| PGPatcher is running — scan paused. | Close PGPatcher and press **Retry**, or **Scan without PGPatcher output**. |
| PGPatcher output '<name>' looks incomplete (no ParallaxGen_Diff.json). Re-run PGPatcher. | The marker set is incomplete. |
| PGPatcher output is out of date for N meshes. | Source meshes changed since PGPatcher ran. **Show list** lists them. Re-run PGPatcher. |

## The Meshes view

Switch to it with the **Meshes** tab in the header.

### Scan details

The scan row's **Scan details** lists what the last scan found and any notes (missing mod folders, archive notes, meshes that failed to read).

### Meshes

The left column is one flat list of meshes: one row per mesh path, showing the copy that wins in game. Each row has the path (without `meshes/`; hover for the full path), the mod it comes from, and its chips.

- **Search paths and mods** narrows the list to rows whose path or mod name holds the text (case does not matter, `\` and `/` are the same).
- **Any mod** picks a mod instead: the list then shows every copy that mod has, winning or not. Only mods with meshes are offered; PGPatcher's output mod is not, and its copies show under **Any mod**.
- Mesh filters: *All*, *Wins* (only with a mod picked), *Overridden* (under **Any mod**: every copy that loses, with its own mod), *Conflicted* (more than one copy), *Problems* (a problem or a Skyrim LE mesh).
- Chips on each mesh, warnings first: **Problem**, **Stale patch** (PGPatcher patch out of date), **LE** (a Skyrim LE mesh), a status chip (**Wins**, **In game** for a mesh PGPatcher patched in game, or **Overridden**), **PGPatcher** (a patched copy), and the mod's **Generated** (by PGPatcher), **BSA** or **game Data** chip. Hover the chips to see them all in full.
- **Show in Meshes** under a texture's **Used by meshes** opens this view with a **Uses <texture> ✕** chip: only the meshes that use that texture are listed. **✕** shows them all again. While a scan runs, or after one that did not complete, the chip waits for a completed scan before it cuts the list.
- The filters only hide rows: the selected mesh stays selected, and the count line says when the filters hide it.

### Mesh detail

Select a mesh to see:

- **In game** / **Source**: the copy that wins in game, or the source mod's copy of a PGPatcher-patched mesh.
- **Shapes**: the **Shapes** button next to the list title swaps the list for the selected mesh's shape cards: number and type, shader, SLSF1/SLSF2 flags by name, geometry, alpha in words (blend, test, threshold) and the texture slots, with **Open in Textures** for files in the open folder. The right pane shows the mesh in 3D.
- **Plugin overrides**: plugin records that use this mesh. **Show inert** includes records that have no effect.
- Override and conflict information ("Overridden by …").
- **Edit**: opens the mesh for editing. See [NIF editing](NIF-Editing.md).

### Plugin file references

**Plugin file references…** opens a window listing files that plugins reference: *Missing* (referenced but not found, with **Show inert**) and *Unused*, with **Copy**. The button is there whenever a scan database is open. After a folder scan, which reads no plugins, the window says "Plugins were not read in the last scan".

## Error messages

| Message | Meaning |
|---|---|
| PGPatcher is running | A scan will not read the instance while PGPatcher or PGTools runs. |
| another scan is already running on this scan database | Another Tessalith window is scanning. |
| scan database version N, this build reads M: scan again in the Meshes view | The database was written by another version. Scan again. |
| no MO2 instance: … | `ModOrganizer.ini` was not found, or the profile has no `modlist.txt`. |
| MO2 settings: gamePath missing / selected_profile missing | `ModOrganizer.ini` lacks a required setting. |
