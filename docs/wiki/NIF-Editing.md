# NIF editing

The GUI can edit a mesh (NIF file) in the Meshes view and write an edited copy. Your source file is never changed. This page gives the outline; every field and message is in the [full GUI reference](../gui.md#editing-a-nif).

## Before you start

- Scan your mods first, so the Meshes view lists your meshes. See [Mod scanning](Mod-Scanning.md).
- Pick an export folder with **Export to ▾** in the header (**More ▾ → Choose export folder…** until a folder is chosen; a project never takes over the folder you picked for another mod). Edited NIFs go there, next to your textures. There is no default folder: until you pick one you can edit, but not export ("No export folder chosen"), and your edits are kept in `%LOCALAPPDATA%\sdc\nif-edits` so they survive a restart. Edits made with Tessalith 1.1 or earlier without an export folder are in `mods\sdc NIF Edits`: choose that folder in **Export to ▾** to reopen them.

## Open a mesh for editing

1. Switch to the **Meshes** view and select a mesh.
2. Press **Edit** next to its name. The edit panel opens, and the column shows the mesh's shape cards: a **Root** card, then one card per shape.
3. Click a card to choose what the tabs edit. A line under the tabs says which one, for example "Editing: BodyMesh (BSTriShape)".

The 3D view shows your edits as you make them. **Show only this shape** hides the other shapes.

## What you can edit

| Tab | What it changes |
|---|---|
| **Shader** | Texture paths, shader type, glossiness, specular, emissive, UV scale and offset, and the SLSF1 and SLSF2 flags. |
| **Alpha** | Blending, the test and its threshold. **Add alpha** and **Remove alpha**. |
| **Controllers** | Shader and alpha controllers: their timing and keys, on a curve graph or in a table. Add or remove controllers, and make, rename or remove animation clips or change their timing. |
| **Extra data** | BSXFlags and other extra data on the root or a shape, with a **Known** list of common entries, and **Heel height (RaceMenu)**. |

While a controller animates, **Play**, **Loop**, **Speed** and a **Time** slider sit under the 3D view, so you can watch it.

## Shared parts

Several shapes can share one alpha, texture set or shader. The panel tells you:

- A shared alpha or texture set is copied for this shape on your first edit, so only this shape changes.
- A shared shader changes every shape that uses it. A texture path edit on a shared shader is refused.

## Warnings

Values are never clamped. A value or flag that looks wrong shows an amber warning under the field, sometimes with a **Fix** button. A red error stops the NIF's export when your edit caused it. Some errors always stop the export, because the game would crash or draw black: for example the skin tint or FaceGen detail flag on the wrong shader type, or an Env map scale controller on a PBR shape.

## Pending edits, undo and source changes

- **Pending edits (N)** lists each edit in words. **Revert** removes one. A red row did not apply and says why.
- **Undo** and **Redo** (header, panel, **Ctrl+Z** / **Ctrl+Y**) act on the NIF edit while it is open. The header buttons then name the step, as in **Undo NIF edit: Set shader type**.
- Edits are kept in a small file under `.sdc\nif-edits` in the export folder, so they survive a restart. If you change the export folder while editing, the open edit moves with you to the new folder; the edits saved in the old one stay there. If the new folder already has saved edits for that NIF, the first save there moves them aside to `<name>.<seconds>.toml` beside them, and Messages names the kept file.
- If a mod update changes the source NIF, your edits are applied again by name. Any that clash offer **Take source** or **Keep mine**.

## Export

The bottom of the edit panel says **Ready**, "No edits to export.", or **Blocked:** with the reason. **Review…** opens Review: NIF edits export from there, together with your textures, when you press Review's **Export**.

- Review lists each NIF with edits as a row. The export writes `meshes\...\<name>.nif` under the export folder. Blocks that your edits left unused are left out, and the row says how many.
- A blocked NIF row is skipped; it never stops the texture export.
- If a file Tessalith did not write is already there, or the file changed since Tessalith wrote it, its row asks first: keep the file on disk, or overwrite it. Until you answer, that NIF is skipped.
- When the export folder is an MO2 mod, Messages says if the mod must be enabled or moved.

## PGPatcher

If PGPatcher has patched this mesh, the game loads PGPatcher's copy, which was made from the unedited file. After you export, **re-run PGPatcher** so its output includes your edit. A warning row reminds you.

The **In game (simulated)** view runs Tessalith's own PGPatcher simulation on your edited mesh, so you can see the patched look before you re-run PGPatcher. It reads loose PGPatcher JSON in MO2 mod order and does not read JSON inside BSAs.

## Not supported

- Skyrim LE meshes open without a 3D preview.
- Splitting a shared shader.
- Effect-shader controllers. The editor does not read them; only the alpha controllers of such a shape are listed.
- Controllers other than alpha, shader float and colour, and texture transform. They are shown read-only.
