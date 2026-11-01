# Editing a NIF

How to edit a mesh's shader, alpha, controllers and extra data in the Meshes view, and export an edited copy. For mod authors fixing or tuning meshes. Your source file is never changed. Controllers and clips are on [NIF animation](GUI-NIF-Animation.md).

## Before you start

- Scan your mods first, so the Meshes view lists your meshes ([Mod scanning](Mod-Scanning.md)).
- Pick an export folder with **Export to ▾** in the header ([Export to](GUI-Main-Window.md#export-to)). Edited NIFs go there, next to your textures.

## Editing a NIF

**Edit** (next to the NIF's name in the Meshes view) opens the edit panel for the selected NIF; click it again to close the panel. The panel shows the file's data path and `Source: <where it came from>`. Edits never change the source file. They are kept in a small edit document in `.sdc\nif-edits` under the export folder, and the export (from Review) writes a copy there. The export folder is the Textures one, once you have chosen it for the project (**Export to ▾** in the header, or **Choose…** in Review); there is no default. Until you choose one the edit opens and works, the export says "No export folder chosen", and the edit document is kept in `%LOCALAPPDATA%\sdc\nif-edits` (one file per NIF) so it survives a restart; the first save into a chosen folder moves it there. Edits made with Tessalith 1.1 or earlier without an export folder are in `mods\sdc NIF Edits`; choose that folder in **Export to ▾** to reopen them (Tessalith never edits a NIF from that folder).

**Cards.** In edit mode the shape cards get a **Root** card first, then one card per shape. Click a card's header to select it; the tabs below edit the selected card. A line under the tab strip names it: "Editing: BodyMesh (BSTriShape)", or "Editing: Root (NiNode)". **Show only this shape** hides the other shapes in the 3D view. While you edit, the cards show the edited file: a change to the shader type, a flag, the alpha or a texture path shows on its card at once.

## Tabs

- **Shader** edits the BSLightingShaderProperty of the selected shape. The texture slots come first (type a path, **Browse**, or pick from the scanned textures; **Check that files exist** and **Filter textures** help), then the shader type, glossiness, specular, emissive, UV scale and offset and the per-type fields, then the SLSF1 and SLSF2 flag lists, closed until you open them (a list with a flag warning starts open, so its fix button is in sight). A texture path change shows in the 3D view at once.
- **Alpha** edits the NiAlphaProperty: **Blend**, source and destination, **Test**, function, threshold, plus **Add alpha** and **Remove alpha**. "Shown as the engine draws it" and "Shown as CS evaluates it (alpha ≥ threshold)" say how the preview treats the test. A blend pair the engine mapping does not cover is drawn as stored and marked "Not verified in game".
- **Controllers** lists the shape's controllers (alpha, shader float and colour, texture transform) with their start, stop, frequency, phase, cycle and the animated value, and edits their keys ([NIF animation](GUI-NIF-Animation.md#the-controllers-tab)). Controllers of other classes are read-only.
- **Extra data** lists the extra data of the root or the selected shape: BSXFlags bits, string, integer, boolean and float values, BSInvMarker. **Add to root**, **Add to shape** and **Remove** change the list. Each row shows its own warnings under it, for example "BSXFlags must be named BSX". Above the type and name, a **Known** list fills in an entry the game or a plugin reads, with its type, name and a starting value; hover an entry for what it does. On a shape: **AnisotropicAlphaMaterial** (Community Shaders see-through look; starts at 3, woven fabric). On the root: **BSX**, **INV**, **HDT Skinned Mesh Physics Object** (type the physics xml path in **Value**; Add stays off until you do) and **Prn (unconfirmed)** (the skeleton node the item attaches to; not checked against a source yet). An entry that goes on the other owner is greyed ("Goes on the root", "Goes on a shape"), and so is one this owner already has ("Already on this root"). **Custom…** keeps free typing for anything else. BODYTRI is not offered: BodySlide and Outfit Studio write it. **Heel height (RaceMenu)** (shapes only; on the root: "The game ignores heel height here. Put it on a shape.") raises the actor so heels stand on the ground. It starts at the shape's stored value, **Calculate** fills it from the lowest point of the shape in bind pose (rounded to 0.1, still editable), and **Set heel height** writes it: the shape's HH_OFFSET is changed in place or added, and an SDTA the shape already has gets the same height in its NPC entry (a new SDTA only when **Also write SDTA** is ticked). It says when an earlier shape's heel height wins, and for `_0`/`_1` files it reminds you to apply the same value to the other weight file. Heel data anywhere but on a shape, and on a shape after the first shape in the scene that has some, warns under its row. A negative height says it lowers the actor. When the shape's HH_OFFSET is not a number the editor can set (another type), Set heel height stays off until that row is removed.

## Shared blocks

The panel says when a block has more than one user.
- A shared alpha: "Shared by N shapes: editing will split." The first edit splits it, so only this shape changes. On the Controllers tab you click **Split alpha** first. If a clip also drives that alpha, it cannot be split yet.
- A shared texture set: "Shared by N shapes: editing makes a copy for this shape."
- A shared shader: "Shared by N shapes: shader edits change all of them." A texture path edit on a shared shader is refused, because it would change the other shapes.
- A shared interpolator: a value edit is a conflict ("This interpolator is shared by N controllers"), and the value row is greyed. A key edit copies the interpolator and its keys first, so the other controller's curve does not change.

## Warnings

On a PBR shape, the SLSF2 flags that CS reads differently (Fit_Slope, Multi_Layer_Parallax, Soft, Back, Rim and Effect lighting) get a dimmer "PBR: <meaning>" label, and the same text in their hover; a non-PBR shape shows only the vanilla names.

Values are never clamped. A value outside the range the game expects, or a flag combination that looks wrong, shows an amber warning under the field (red for an error), sometimes with a **Fix** button. A red error that your edit introduced blocks the NIF's export. An error that was already in the source only warns. Three flag warnings come from the corpus census: shader type 11 without the Multi_Layer_Parallax flag (SLSF2 24) and without PBR; SLSF2 24 on a shader type other than 11; Fit_Slope (SLSF2 12) without PBR. On a True PBR shape an empty slot 5 is amber: "Slot 5 (RMAOS) is empty: in game the mesh renders fully metallic".

**Flags that need a shader type.** The skin tint flag (SLSF1 21) works only on the Skin Tint type and the FaceGen detail flag (SLSF1 10) only on Face Tint (which needs it) or Skin Tint: a mismatch is a red error that always blocks the NIF's export, even in an untouched file, with a **Clear the flag** or **Set the flag** button; a type change fixes the flag in the same undo step and says so in a line under the type, and a flag that cannot apply to the current type is greyed with the reason on hover.

## Pending edits, undo and source changes

**Pending edits (N).** Every edit is a row in this list under the file name, named in words ("BodyMesh: SLSF1 flags"). A slider drag is one row. **Revert** removes that edit. Reverting a new or converted clip also removes the later timing edits of that clip, and its row says so: "(Revert also removes its clip timing)". A row in red did not apply, with the reason. The list is closed until a row turns red, which opens it; click its title to open or close it.

**Undo.** While an edit is open in the Meshes view, the header **Undo** and **Redo**, the command palette and **Ctrl+Z** / **Ctrl+Y** act on the NIF edit. With no edit open they act on the Textures history. The header buttons say which: **Undo** for the Textures history, **Undo NIF edit: Set shader type** (the step's name) for the NIF one, and the hover names the history. The panel has its own **Undo** and **Redo** buttons too. A slider drag is one step.

**Source changed.** If the source NIF changes on disk (a mod update), the edits are re-applied by name and role, not by position. The panel says "Source changed: N applied, M conflicts". A conflicting edit has **Take source** (drop your edit) and **Keep mine** (keep it on top of the new source).

## Export

The bottom of the edit panel, under the tabs, stays there however long a tab is. Its first line is a status: a green dot and **Ready**, a grey dot and "No edits to export." while nothing is edited, or a red dot and **Blocked:** with the reason. The reason is one of: an edit cannot be applied, conflicts are open, an edit introduced an error, or no export folder ("No export folder chosen"). When a pending row is red, **Show** opens the pending list. When a file Tessalith did not write is already at the export path, **Diff…** opens a list of the blocks that differ between that file and what the export writes, under the status line (it is read at the click; **Close** shuts it). Then "Writes meshes\…\x.nif to <export folder>\meshes\…\x.nif" (cut in the middle when long; hover for the full path) and "The source file is never changed. Change the folder with Export to ▾ in the header."

**Review…** opens Review in the bottom panel: NIF edits export from there, with the textures, when you press Review's Export. The button itself never writes. In Review each NIF with edits is a row: its mod, how many unused blocks the export leaves out, and the result after the export (written, or skipped and why). A blocked NIF row is skipped and never blocks the texture export. Each write puts "Exported <path> to <mod>." in Messages, with a placement line when the mod order is wrong: when the export folder is a mod folder in the MO2 instance's `mods`, the export checks the mod order and says whether that mod must be enabled, or dragged below the highest source or above the lowest PGPatcher output; any other folder says "The export folder is not an MO2 mod here: load order not checked." and still exports.

### Changing the export folder

Changing the export folder while you have edits open keeps them in memory, unsaved; the next save or export goes to the new folder. If the new folder already holds saved edits for that NIF, your open edit replaces them: the first save there moves them aside to `<name>.<seconds>.toml` beside it, never overwriting them, and Messages names the kept file. The same happens to a folder's saved edits when a newer autosave of the NIF opens instead of them, and to an autosave this session did not open. The edits saved in the old folder stay there, and Messages names that file. If the edit document in the new folder cannot be opened (a file that does not read), the folder change waits: the edit stays blocked with "could not open the edit document in <folder>: <reason>", each save goes to the autosave in `%LOCALAPPDATA%\sdc\nif-edits`, and nothing is written to the old folder, until you choose a folder that works or go back to the old one. With no edits yet, the NIF's edits in the new folder open instead.

### After an export

After an export, the worklist has a Warning row "Re-run PGPatcher so its output includes this edit." when the NIF has a PGPatcher copy. The exported file leaves out the blocks your edits added that nothing uses any more (a removed controller or clip), so it can have fewer blocks than the edit shows. When that cleanup cannot read the file, an Info row says "Exports without cleanup: the file has a block Tessalith does not understand", and the export writes the file as edited.

### A file Tessalith did not write

If the file at the destination is not the one Tessalith last wrote, its Review row waits for an answer, keep the file on disk or overwrite it, with neither picked. Until you answer, the export skips that NIF. An answer counts for one export, and only while the file on disk is the one you answered for: a file that changed since says "changed on disk since you answered" and asks again. The row's **Diff…** switches to the Meshes view and opens the edit panel's list of the blocks that differ.

## PGPatcher and the game

While an edit is open, the toggle reads **In game (simulated)** and **Edited source** ([In game (simulated) and Edited source](GUI-NIF-Animation.md#in-game-simulated-and-edited-source)). Once you have exported, the amber line "PGPatcher output predates your edit: re-run PGPatcher." shows: the game still loads PGPatcher's copy, which was made from the unedited NIF, until you re-run PGPatcher. If the edit preview cannot be shown, the line "Your edit could not be shown. Switch to In game (simulated) or Edited source to retry." says so; edits you make meanwhile still count.

## Not supported

- A Skyrim LE NIF opens without a 3D preview ("LE mesh: no 3D preview").
- Splitting a shared shader, or a shared interpolator for a value edit (the edit is refused or is a conflict); an outline for the selected shape.
- Effect-shader controllers: the editor does not read them; only the alpha controllers of such a shape are listed.
