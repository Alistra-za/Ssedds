# Mod scanning

`sdc-gui` can scan your installed mods: the meshes (NIF files, loose or in BSA archives) and, for a Mod Organizer 2 instance, the active plugins. The scan tells the GUI which meshes use each of your textures, and the GUI uses that to fill in decisions you would otherwise make by hand. Scanning is a GUI feature; `sdc` on the command line never reads the scan.

Scanning only reads. It never changes your mods, your MO2 instance or your game folder.

## What the scan is used for

| Use | Where you see it |
|---|---|
| Alpha meaning of colour maps | A colour map whose alpha varies takes Blend, Cutout (at the strictest mesh threshold), Cutout + Blend or Remove from the meshes that use it. The Alpha row reads "from the meshes that use this texture". See [GUI guide](GUI-Guide.md#alpha-from-the-meshes). |
| Set shader | When no map in the set decides it (PBR, Advanced skin, complex grass, complex `_m`, `_p`), the shader most of the scanned meshes use for the diffuse picks the set's shader. |
| Complex grass | After a scan that included plugins, a texture used by a grass (GRAS) record that has the complex-grass shape is tagged `complex_grass` automatically. |
| Path pickers | The Input name picker, and the match pickers in the TruePBR JSON editor, list texture paths and mesh folders the scan found. |
| Rename warnings | Renaming outputs that scanned meshes still use under their old name is flagged amber. |
| 3D preview | *Auto* in the set preview shows the set on a scanned game mesh that uses its diffuse. |
| Plugin references | The set editor's "Used by plugins" and the Meshes view's "Plugin file references…" window. |

Your own choices in `sdc.toml` always win over scan evidence.

Because the CLI does not read the scan, a folder decided partly from scan evidence can convert differently from the command line. Alpha meaning from meshes, mesh-voted shaders and the complex-grass auto-tag are evidence, not entries in `sdc.toml`. If you also convert with `sdc convert`, set those in `[files]` entries (`alpha.policy`, `alpha.threshold`, `subtype`) or with `--set set:<id>.shader=<shader>`.

## Choosing what to scan

The scan source is shown on the scan row (on both the Textures view and the Meshes view) as "MO2 instance X, profile Y" or "Folder X".

### A Mod Organizer 2 instance

**Choose MO2 instance…** picks the instance. Tessalith reads `ModOrganizer.ini` for the game path, the mods, overwrite and profiles folders, and the profile. It then follows that profile's mod list:

- Enabled mods (`+` in `modlist.txt`) in priority order, with the game's `Data` folder lowest and `overwrite` highest. Disabled mods and separators are skipped. An enabled mod whose folder is missing is reported.
- The active BSA archives, from the game INIs and the active plugins.
- The active plugins and their load order, from the profile.

For each file path, the scan works out which copy wins in game (the highest-priority loose file or archive).

If you start `sdc-gui` from inside MO2, it detects that and asks: "Opened from Mod Organizer 2 instance '…', profile '…'. Use it?" with **Use**, **Choose instance…** and **Open a folder instead**. Tessalith finds the instance through `portable.txt` next to `ModOrganizer.exe`, or MO2's current instance under `%LOCALAPPDATA%\ModOrganizer\`.

### A folder

**Scan a folder…** scans the meshes under one folder, for example an extracted mod or a mod you are building. A folder scan has no plugin pass, so plugin references and the complex-grass auto-tag are not available.

### Running a scan

Press **Scan**. Progress shows on the scan row; **Cancel** stops it. Cancelling during the plugin pass keeps the mesh results and notes that plugin results are incomplete until the next scan.

What is skipped: FaceGen meshes and LOD meshes. Meshes that are not Skyrim SE NIFs are counted and skipped. Meshes from Skyrim LE are read but not drawn in the 3D preview.

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
| PGPatcher is running, scan paused. | Close PGPatcher and press **Retry**, or **Scan without PGPatcher output**. |
| PGPatcher output '<name>' looks incomplete (no ParallaxGen_Diff.json). Re-run PGPatcher. | The marker set is incomplete. |
| PGPatcher output is out of date for N meshes. | Source meshes changed since PGPatcher ran. **Show list** lists them. Re-run PGPatcher. |

## The Meshes view

Switch to it with **Meshes view** in the header.

### Scan details

The scan row's **Scan details** lists what the last scan found and any notes (missing mod folders, archive notes, meshes that failed to read).

### Mods and meshes

The left column lists mods in priority order. Click a mod's name or its **▼** to list its meshes under it; **▲** closes it. Separators use the same arrows.

- **Filter mods** narrows the mod list.
- **Filter paths** narrows the open mod's meshes.
- Mesh filters: *All*, *Wins* (the copy used in game), *Overridden*, *Conflicted*, *Problems*.
- Chips on each mesh: **BSA** (from an archive), **game Data**, **PGPatcher** (a patched copy), **LE** (a Skyrim LE mesh), **Problem**.
- **All meshes** searches every scanned mesh by path instead. Type at least a few characters; the first matches are shown.

### Mesh detail

Select a mesh to see:

- **In game** / **Source**: the copy that wins in game, or the source mod's copy of a PGPatcher-patched mesh.
- **Shapes**: each shape with its shader, texture slots and alpha settings. **3D** shows the mesh.
- **Plugin overrides**: plugin records that use this mesh. **Show inert** includes records that have no effect.
- Override and conflict information ("Overridden by …").

### Plugin file references

**Plugin file references…** opens a window listing files that plugins reference: *Missing* (referenced but not found) and *Unused*, with **Show inert** and **Copy**. Available after the scan finishes, for MO2 scans.

## Error messages

| Message | Meaning |
|---|---|
| PGPatcher is running | A scan will not read the instance while PGPatcher or PGTools runs. |
| another scan is already running on this scan database | Another `sdc-gui` window is scanning. |
| scan database version N, this build reads M: scan again in the Meshes view | The database was written by another version. Scan again. |
| no MO2 instance: … | `ModOrganizer.ini` was not found, or the profile has no `modlist.txt`. |
| MO2 settings: gamePath missing / selected_profile missing | `ModOrganizer.ini` lacks a required setting. |
