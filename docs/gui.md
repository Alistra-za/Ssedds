# Tessalith user guide (M3a, M3b)

`tessalith.exe` is Tessalith, the graphical front end of the Skyrim DDS converter (the command line is `sdc`). It runs the same engine as `sdc convert`, so a folder you decide and export here converts to the same files from the command line. This guide covers the converter GUI (M3a) and the set editor with the PGPatcher JSON (M3b); texture edits (M3c) come later.

## Opening a folder

Moved to [Main window](wiki/GUI-Main-Window.md#opening-a-folder) in the wiki.

## The three columns

Moved to [Textures column](wiki/GUI-Textures-Column.md), [Sets column](wiki/GUI-Sets-Column.md), [Texture settings panel](wiki/GUI-Texture-Panel.md) and [Advanced drawer](wiki/GUI-Advanced-Drawer.md).

## Deciding

Moved to [Texture settings panel](wiki/GUI-Texture-Panel.md#deciding).

## Editing

Moved to [Texture settings panel](wiki/GUI-Texture-Panel.md#editing).

## Bulk editing

Moved to [Bulk editing and renaming](wiki/GUI-Bulk-and-Rename.md#bulk-editing) and [Slot menus](wiki/GUI-Sets-Column.md#slot-menus).

## Renaming

Moved to [Bulk editing and renaming](wiki/GUI-Bulk-and-Rename.md#renaming).

## Sets and the PGPatcher JSON

Moved to [Sets column](wiki/GUI-Sets-Column.md), [The set editor](wiki/GUI-Set-Editor.md), [PGPatcher settings and the JSON](wiki/GUI-PGPatcher-JSON.md) and [Set materials](wiki/GUI-Set-Materials.md).

## Helper textures

Moved to [Helpers, RNM layers and cubemaps](wiki/GUI-Helpers-RNM-and-Cubemaps.md#helper-textures).

## Cubemaps

Moved to [Helpers, RNM layers and cubemaps](wiki/GUI-Helpers-RNM-and-Cubemaps.md#cubemaps).

## RNM normal layers

Moved to [Helpers, RNM layers and cubemaps](wiki/GUI-Helpers-RNM-and-Cubemaps.md#rnm-normal-layers).

## Complex grass

Moved to [Complex grass](wiki/Set-Shaders.md#complex-grass).

## PBR grass

Moved to [Set materials](wiki/GUI-Set-Materials.md#pbr-grass).

## Generate from normal map

Moved to [Generate from normal map](wiki/GUI-Generate-From-Normal.md).

## The preview

Moved to [The 2D preview](wiki/GUI-Preview.md).

## The 3D preview

Moved to [The 3D preview](wiki/GUI-Set-Preview-3D.md).

## The Meshes view: shapes

**The header.** The Meshes view draws the same one-row header as the Textures view (Open, Undo, Redo, MO2…, Mode, Preset, the view tabs, Export to, Save, Export) with the status line under it; only the column eyes are missing.

**Scan source and progress.** The scan source is behind the **MO2…** header button (both views): a popup with the instance and **Choose…**, the instance's profiles, **Scan a folder…** and **Scan** / **Cancel**, which stays open while you click in it. While a scan runs, the scan row (in the Meshes view and in the Textures view) shows a bar with the phase and its count, for example `reading 120/4000 · ETA 0:42`. The ETA appears once the phase has finished 3 items or run for 2 seconds. The ETA is the average over the last 10 seconds, as in the Files tab, and starts over with each phase.

**The mesh list** (Note GC). The left column lists every mesh path once, with the copy that wins in game: the path (without `meshes/`, cut to the column, full path on hover), its mod, and its chips, warnings first: **Problem**, **Stale patch** (PGPatcher patch out of date), **LE**, the edit badge, the status chip (**Wins**, **In game** for "Patched by PGPatcher (in game)", or **Overridden**), **PGPatcher**, and the mod's **Generated** (by PGPatcher), **BSA** or **game Data**. Chips that do not fit are cut; hovering the chips lists every one in full, with the copies and the problem's text. **Search paths and mods** keeps the rows whose path or mod name holds the text (ASCII case does not matter, `\` reads as `/`, a leading `meshes/` is ignored). The **Any mod** box picks one mod with meshes (PGPatcher's output mod is not offered) and lists all of its copies. The chips are *All*, *Wins* (with a mod picked only), *Overridden* (under **Any mod**, every losing copy with its own mod), *Conflicted* and *Problems* (a problem or an LE mesh). A count line reads "N of M NIFs" and adds "(the selected NIF is hidden by the filters)" when it is: filters and the picker never deselect. The rows are read in the background after a scan or reload ("Reading the scan…"); with no scan the column says "No scan yet."

Next to the **Meshes** title is a **Shapes** button (greyed out, "Select a mesh first", until a mesh is selected). Switched on, it replaces the mesh list with the shape cards of the selected mesh; switched off, or when the selection clears, the list is back. While **Edit** is open the column stays on the shapes and the button is greyed ("Close Edit to see the mesh list"): the cards are where you pick the shape to edit. The right-hand pane always shows the 3D view. The column starts at 420 px and is at most 40% of the window, narrowing further as needed so the 3D view keeps at least 240 px (with the edit panel open too); its lists have thin scroll bars that stay visible. Closing Edit puts the column back on what it showed before (Edit keeps it on Shapes).

Each shape is a framed card. It shows, from the top:

- **#N name**, with the block type on the right, a button as wide as the card. N is the shape's PGPatcher 3D index, the number plugins use for its alternate textures (a shape outside the scene tree reads `#–`). Click it to select the card: it gets the accent outline, and **Edit** then opens on that shape instead of Root. A NIF with one shape opens on it. Hover for the NIF block index ("NIF block 12").
- **The shader**, in bold: the lighting shader type, for example **Hair Tint (6)**, or "Effect shader".
- **The textures**: each set slot with its path, indented, and one weak line for the empty ones ("(4 empty slots)"). A missing texture says "→ missing" in red.
- **Alpha**, in words: "Alpha: blend Src Alpha → Inv Src Alpha · test Greater, threshold 5". Blending or testing is left out when it is off, and "Alpha: none" shows when both are. A shape with no alpha property says "No NiAlphaProperty". Hover for the raw flags, for example "flags 0x12ED (4845)".
- **Controllers**: how many, and their names, with "(clip)" for one a clip drives, for example "2: Emissive multiple, Alpha (clip)", or "none". A shape with an effect shader lists only its alpha controllers (the editor does not read the effect shader's), and has no line when it has none. Outside Edit the line is read from the NIF file once per scanned copy (it appears a moment after the cards; a file that no longer reads simply shows no line); inside Edit it follows the edited file, unsaved edits included.
- **Details**, closed: the NIF block and 3D index, **SLSF1 0x…** and **SLSF2 0x…** (open one to list its set flags by name, for example "Specular (0)"; a bit with no known name reads "bit N"), the geometry line (skinned, vertices, triangles), which mod each texture comes from ("→ A (2 copies)") and the raw slot strings.

A texture path that is a texture of the open folder has an **Open in Textures** button, on the shape card and on the "Users of" list's title. It switches to the Textures view, selects that texture and scrolls it into view.

### Editing a NIF

**Edit** (next to the NIF's name in the Meshes view) opens the edit panel for the selected NIF; click it again to close the panel. The panel shows the file's data path and `Source: <where it came from>`. Edits never change the source file. They are kept in a small edit document in `.sdc\nif-edits` under the export folder, and the export (from Review) writes a copy there. The export folder is the Textures one, once you have chosen it for the project (**Export to ▾** in the header, or **Choose…** in Review); there is no default. Until you choose one the edit opens and works, the export says "No export folder chosen", and the edit document is kept in `%LOCALAPPDATA%\sdc\nif-edits` (one file per NIF) so it survives a restart; the first save into a chosen folder moves it there. Edits made with Tessalith 1.1 or earlier without an export folder are in `mods\sdc NIF Edits`; choose that folder in **Export to ▾** to reopen them (Tessalith never edits a NIF from that folder).

**Cards.** In edit mode the shape cards get a **Root** card first, then one card per shape. Click a card's header to select it; the tabs below edit the selected card. A line under the tab strip names it: "Editing: BodyMesh (BSTriShape)", or "Editing: Root (NiNode)". **Show only this shape** hides the other shapes in the 3D view. While you edit, the cards show the edited file: a change to the shader type, a flag, the alpha or a texture path shows on its card at once.

**Tabs.**
- **Shader** edits the BSLightingShaderProperty of the selected shape. The texture slots come first (type a path, **Browse**, or pick from the scanned textures; **Check that files exist** and **Filter textures** help), then the shader type, glossiness, specular, emissive, UV scale and offset and the per-type fields, then the SLSF1 and SLSF2 flag lists, closed until you open them (a list with a flag warning starts open, so its fix button is in sight). A texture path change shows in the 3D view at once.
- **Alpha** edits the NiAlphaProperty: **Blend**, source and destination, **Test**, function, threshold, plus **Add alpha** and **Remove alpha**. "Shown as the engine draws it" and "Shown as CS evaluates it (alpha ≥ threshold)" say how the preview treats the test. A blend pair the engine mapping does not cover is drawn as stored and marked "Not verified in game".
- **Controllers** lists the shape's controllers (alpha, shader float and colour, texture transform) with their start, stop, frequency, phase, cycle and the animated value, and edits their keys. See **Animation** below. Controllers of other classes are read-only.
- **Extra data** lists the extra data of the root or the selected shape: BSXFlags bits, string, integer, boolean and float values, BSInvMarker. **Add to root**, **Add to shape** and **Remove** change the list. Each row shows its own warnings under it, for example "BSXFlags must be named BSX". Above the type and name, a **Known** list fills in an entry the game or a plugin reads, with its type, name and a starting value; hover an entry for what it does. On a shape: **AnisotropicAlphaMaterial** (Community Shaders see-through look; starts at 3, woven fabric). On the root: **BSX**, **INV**, **HDT Skinned Mesh Physics Object** (type the physics xml path in **Value**; Add stays off until you do) and **Prn (unconfirmed)** (the skeleton node the item attaches to; not checked against a source yet). An entry that goes on the other owner is greyed ("Goes on the root", "Goes on a shape"), and so is one this owner already has ("Already on this root"). **Custom…** keeps free typing for anything else. BODYTRI is not offered: BodySlide and Outfit Studio write it. **Heel height (RaceMenu)** (shapes only; on the root: "The game ignores heel height here. Put it on a shape.") raises the actor so heels stand on the ground. It starts at the shape's stored value, **Calculate** fills it from the lowest point of the shape in bind pose (rounded to 0.1, still editable), and **Set heel height** writes it: the shape's HH_OFFSET is changed in place or added, and an SDTA the shape already has gets the same height in its NPC entry (a new SDTA only when **Also write SDTA** is ticked). It says when an earlier shape's heel height wins, and for `_0`/`_1` files it reminds you to apply the same value to the other weight file. Heel data anywhere but on a shape, and on a shape after the first shape in the scene that has some, warns under its row. A negative height says it lowers the actor. When the shape's HH_OFFSET is not a number the editor can set (another type), Set heel height stays off until that row is removed.

**Shared blocks.** The panel says when a block has more than one user.
- A shared alpha: "Shared by N shapes: editing will split." The first edit splits it, so only this shape changes. On the Controllers tab you click **Split alpha** first. If a clip also drives that alpha, it cannot be split yet.
- A shared texture set: "Shared by N shapes: editing makes a copy for this shape."
- A shared shader: "Shared by N shapes: shader edits change all of them." A texture path edit on a shared shader is refused, because it would change the other shapes.
- A shared interpolator: a value edit is a conflict ("This interpolator is shared by N controllers"), and the value row is greyed. A key edit copies the interpolator and its keys first, so the other controller's curve does not change.

On a PBR shape, the SLSF2 flags that CS reads differently (Fit_Slope, Multi_Layer_Parallax, Soft, Back, Rim and Effect lighting) get a dimmer "PBR: <meaning>" label, and the same text in their hover; a non-PBR shape shows only the vanilla names.

**Warnings.** Values are never clamped. A value outside the range the game expects, or a flag combination that looks wrong, shows an amber warning under the field (red for an error), sometimes with a **Fix** button. A red error that your edit introduced blocks the NIF's export. An error that was already in the source only warns. Three flag warnings come from the corpus census: shader type 11 without the Multi_Layer_Parallax flag (SLSF2 24) and without PBR; SLSF2 24 on a shader type other than 11; Fit_Slope (SLSF2 12) without PBR. On a True PBR shape an empty slot 5 is amber: "Slot 5 (RMAOS) is empty: in game the mesh renders fully metallic".

**Flags that need a shader type.** The skin tint flag (SLSF1 21) works only on the Skin Tint type and the FaceGen detail flag (SLSF1 10) only on Face Tint (which needs it) or Skin Tint: a mismatch is a red error that always blocks the NIF's export, even in an untouched file, with a **Clear the flag** or **Set the flag** button; a type change fixes the flag in the same undo step and says so in a line under the type, and a flag that cannot apply to the current type is greyed with the reason on hover.

**Pending edits (N).** Every edit is a row in this list under the file name, named in words ("BodyMesh: SLSF1 flags"). A slider drag is one row. **Revert** removes that edit. Reverting a new or converted clip also removes the later timing edits of that clip, and its row says so: "(Revert also removes its clip timing)". A row in red did not apply, with the reason. The list is closed until a row turns red, which opens it; click its title to open or close it.

**Export status and Review.** The bottom of the edit panel, under the tabs, stays there however long a tab is. Its first line is a status: a green dot and **Ready**, a grey dot and "No edits to export." while nothing is edited, or a red dot and **Blocked:** with the reason. The reason is one of: an edit cannot be applied, conflicts are open, an edit introduced an error, or no export folder ("No export folder chosen"). When a pending row is red, **Show** opens the pending list. When a file Tessalith did not write is already at the export path, **Diff…** opens a list of the blocks that differ between that file and what the export writes, under the status line (it is read at the click; **Close** shuts it). Then "Writes meshes\…\x.nif to <export folder>\meshes\…\x.nif" (cut in the middle when long; hover for the full path) and "The source file is never changed. Change the folder with Export to ▾ in the header." **Review…** opens Review in the bottom panel: NIF edits export from there, with the textures, when you press Review's Export. The button itself never writes. In Review each NIF with edits is a row: its mod, how many unused blocks the export leaves out, and the result after the export (written, or skipped and why). A blocked NIF row is skipped and never blocks the texture export. Each write puts "Exported <path> to <mod>." in Messages, with a placement line when the mod order is wrong: when the export folder is a mod folder in the MO2 instance's `mods`, the export checks the mod order and says whether that mod must be enabled, or dragged below the highest source or above the lowest PGPatcher output; any other folder says "The export folder is not an MO2 mod here: load order not checked." and still exports. Changing the export folder while you have edits open keeps them in memory, unsaved; the next save or export goes to the new folder. If the new folder already holds saved edits for that NIF, your open edit replaces them: the first save there moves them aside to `<name>.<seconds>.toml` beside it, never overwriting them, and Messages names the kept file. The same happens to a folder's saved edits when a newer autosave of the NIF opens instead of them, and to an autosave this session did not open. The edits saved in the old folder stay there, and Messages names that file. If the edit document in the new folder cannot be opened (a file that does not read), the folder change waits: the edit stays blocked with "could not open the edit document in <folder>: <reason>", each save goes to the autosave in `%LOCALAPPDATA%\sdc\nif-edits`, and nothing is written to the old folder, until you choose a folder that works or go back to the old one. With no edits yet, the NIF's edits in the new folder open instead. After an export, the worklist has a Warning row "Re-run PGPatcher so its output includes this edit." when the NIF has a PGPatcher copy. The exported file leaves out the blocks your edits added that nothing uses any more (a removed controller or clip), so it can have fewer blocks than the edit shows. When that cleanup cannot read the file, an Info row says "Exports without cleanup: the file has a block Tessalith does not understand", and the export writes the file as edited.

**A file Tessalith did not write.** If the file at the destination is not the one Tessalith last wrote, its Review row waits for an answer, keep the file on disk or overwrite it, with neither picked. Until you answer, the export skips that NIF. An answer counts for one export, and only while the file on disk is the one you answered for: a file that changed since says "changed on disk since you answered" and asks again. The row's **Diff…** switches to the Meshes view and opens the edit panel's list of the blocks that differ.

**The NIF list.** A NIF with a saved edit gets a chip: **Edit pending**, **Edited** (exported and unchanged) or **Edit conflict**.

**Undo.** While an edit is open in the Meshes view, the header **Undo** and **Redo**, the command palette and **Ctrl+Z** / **Ctrl+Y** act on the NIF edit. With no edit open they act on the Textures history as before. The header buttons say which: **Undo** for the Textures history, **Undo NIF edit: Set shader type** (the step's name) for the NIF one, and the hover names the history (Note FC12). The panel has its own **Undo** and **Redo** buttons too. A slider drag is one step.

**Source changed.** If the source NIF changes on disk (a mod update), the edits are re-applied by name and role, not by position. The panel says "Source changed: N applied, M conflicts". A conflicting edit has **Take source** (drop your edit) and **Keep mine** (keep it on top of the new source).

**In game.** While an edit is open, the toggle reads **In game (simulated)** and **Edited source**; see **Animation** below. Once you have exported, the amber line "PGPatcher output predates your edit: re-run PGPatcher." shows: the game still loads PGPatcher's copy, which was made from the unedited NIF, until you re-run PGPatcher. If the edit preview cannot be shown, the line "Your edit could not be shown. Switch to In game (simulated) or Edited source to retry." says so; edits you make meanwhile still count.

**LE files.** A Skyrim LE NIF opens without a 3D preview ("LE mesh: no 3D preview").

**Not yet.** Splitting a shared shader, or a shared interpolator for a value edit (the edit is refused or is a conflict); an outline for the selected shape.

#### Animation

The edit preview plays the shader and alpha controllers of the NIF you are editing. A NIF shown without **Edit** does not animate.

**Playback.** When the edited NIF has a controller that animates, two rows sit under the 3D view: **Play** (or **Pause**), **Loop** ("Start again at the end"), **Speed** (0.1× to 4×; 1× is game speed), then the **Time** slider in seconds. Moving the slider jumps to that time and pauses. Without Loop, playback stops at the end, and Play starts again from the start. The slider runs from 0 to the end of the longest controller at its frequency (twice as long for a Reverse cycle). Below 30 frames a second the animation slows down instead of skipping. Playback only changes material values, so the mesh is never reloaded. A controller whose times are not numbers gets no controls, only the amber line "This animation has invalid times: playback is off." If you switch to the Textures view, the clock keeps its time and carries on when you come back. **In game (simulated)** pauses the clock and hides these rows; after switching back to **Edited source**, press **Play** again.

**The Controllers tab.** Each shader and alpha property lists its controllers, then **Add controller**. A float, colour or alpha-test controller has:
- A header with its name, its class, "Managed by a clip" when a clip drives it, and **Remove**. Remove unlinks the controller; the BSX Animated bit stays set. It is off for a controller a clip drives ("Driven by a clip: it cannot be removed.").
- **Variable**, **Cycle** (Loop, Reverse, Clamp), **Active**, **Play backwards**, **Frequency**, **Phase**, **Start**, **Stop** and the interpolator's value. You can change the Variable of a controller a clip drives: "Its clips follow the new variable."
- **Interpolation**: Linear or Quadratic. Linear to Quadratic adds tangents that keep the curve's shape; Quadratic to Linear drops them.
- The curve graph: the keys as handles, Quadratic tangent handles, and a cursor at the current time. Drag a key to change its time and value; drag a tangent handle up or down. One drag is one undo step. Click or drag the background to move the preview time. Times are never clamped: a key dragged past its neighbour gives the red error "Key times must not decrease". A curve too large to draw says "These keys are out of range for the graph."; the table still works.
- The key table: Time, Value (R, G and B for a colour), Forward and Backward for Quadratic keys, typed as exact numbers, and **Remove** for each key.
- A colour key also has a **Colour** swatch that opens a colour picker. Each drag in the picker is one undo step; numbers typed in the picker are one step until it closes. Channels you do not change keep their exact values. A key with a value outside 0-1 (a bright emissive colour) gets a greyed swatch: "Values outside 0-1: edit R, G and B directly."
- **Add key**: a new key after the last one, at the last key's time plus the average spacing between the keys (0.1 s with one key), with the last key's value. An empty curve starts at the start time. When no later time is possible (the last key is at the largest number), it is off: "No later time is possible for a new key."
- A controller's **Stop** follows its last key when you edit its keys (drag, table, Add key, Remove, Interpolation), in the same undo step; **Start** moves only when the first key goes before it. A clip's start and stop do not change.

TBC, XYZ and CONST keys are drawn but cannot be edited: "These keys are previewed, not edited." Keys you edit must be Linear or Quadratic, in time order and finite numbers. Otherwise a red error ("Key times must not decrease", "Only linear and quadratic keys can be authored", "Key values must be finite numbers") blocks the NIF's export. A key edit on a curve that another controller or clip shares makes a copy first.

**Add controller.** On a shader property, pick **Float** or **Colour** and the **Variable**. An alpha property adds an alpha-test controller. The new controller has two Linear keys, at 0 and 1 s, that hold the property's current value, so nothing changes until you move a key; it loops at frequency 1. Adding sets the BSX Animated bit (a root without BSXFlags gets one). On the Extra data tab that box stays ticked and locked while a controller outside any clip animates the model. The button is off, with the reason on hover, when the root is not a node or the property cannot be found again in the file. On a shared alpha, Add controller splits it first (one undo step).

**PBR names.** On a PBR shape, as on the Shader tab, glossiness reads **Specular level**, specular strength **Roughness scale**, and specular colour **Coat colour**, **Subsurface colour** or "Specular colour (no effect on PBR)".

**Warnings.** These come from an in-game check of each controller and show under the controller.
- Red: "Env map scale crashes the game on PBR materials." On a PBR shape, Env map scale (8) is greyed in both Variable lists with this text. A controller that already has it blocks the NIF's export, even when the source file had it.
- Amber, from the last In game simulation: on a shape that is not PBR but that PGPatcher makes PBR, an Env map scale controller gets "PGPatcher makes this shape PBR: its Env map scale controller will crash the game." It does not block export, since PGPatcher's output depends on the JSON at play time.
- Amber, no effect in game: "This controller has no effect: Specular is off when the mesh loads." and "…specular strength is 0 when the mesh loads." (glossiness, specular strength or colour on a non-PBR shape); "This controller has no effect: alpha test is off."; "Refraction strength has no visible effect in game."; "Two controllers drive <name>: only the last one plays."

**Clips.** When the file has animation clips, a **Preview** list at the top of the tab picks **Free chains** or one clip. A clip shows alone: the other controllers keep their file value while it plays. Under **Free chains**, a clip named SpecialIdle plays by itself, as in the game, and the Time slider covers it. Any other controller a clip drives holds the first key of the first clip the manager lists for it, as the game does while that clip is not called. When nothing moves (only held values), the preview shows them with no Play or Time controls. Under a controller a clip drives, each clip shows its name, cycle, time span and text keys (one line; hover for all of them). The selected clip's keys get the same graph, table and Add key; the others say "Pick this clip in Preview to see and edit its keys." A clip with several interpolators for one controller cannot be edited ("This clip has several interpolators for this controller: they cannot be edited."). "No clip in this file drives it." marks a controller flagged as managed that no clip uses; it holds its property value.

**Making clips.** These controls sit in one section at the end of the Controllers tab, **Clips (N · showing X)**, closed by default (N clips; X is the clip the Preview shows, or free chains). It opens by itself when a clip action is refused, so the red reason is seen. **Preview** stays at the top of the tab, outside the section. Under each controller a clip drives, one weak line names the clip: "In clip SpecialIdle · Loop · 0 to 2 s". Making a clip needs a node as the file's root ("The root is not a node: a clip needs a node root.").
- **Convert to clips.** One row per property that has controllers ("Convert to clips: shader property"), a name field (it starts as SpecialIdle) and **Convert to clips** move that property's free controllers into one new clip. The clip keeps their keys, uses their shared cycle and frequency, and runs from the earliest start to the latest stop. SpecialIdle loops constantly in game; Open and Close play as door states. The field and the button are off, with the reason on hover, when there is nothing to convert ("This property has no free controllers with keys."), the controllers run on different timings ("These controllers run on different timings: they cannot share one clip."), their cycle is not Loop, Reverse or Clamp ("The cycle must be Loop, Reverse or Clamp."), their time range is empty ("Stop must be after start."), the shape has no name or shares its name with another shape, or the alpha is shared ("This alpha is shared by N shapes: split it before editing."). A refusal found on the click shows in red under the row.
- **New clip.** At the end of the section: a **Name**, a **Cycle** (Loop, Reverse or Clamp; it starts on Clamp), **Start** and **Stop** in seconds (0 and 1 to begin with), and a tick box for each controller to put in the clip. **Add clip** adds it with each ticked controller's keys. A controller whose keys are TBC, XYZ or CONST starts flat instead (hover: "Its keys are not linear or quadratic: the clip starts flat."). The ticks clear when you open another file or select another shape. It is off, with the reason shown, when the name is empty ("Type a name for the clip."), nothing is ticked ("Tick at least one controller."), or Stop is not after Start ("Stop must be after start."). A controller on a shared alpha has a disabled tick box with the split hint.
- **Rename and Remove.** At the top of the section, with a clip picked in **Preview**, the **Clip name** field renames it when you leave the field (Escape cancels), and **Remove clip** deletes it. Each is one undo step, and Enter never removes a clip. Both are off on Free chains ("Pick a clip in Preview to rename or remove it.") and for a clip with no name ("This clip has no name: it cannot be renamed or removed here."). A name must not be empty or repeat another clip's name (any letter case). Removing the last clip returns the Preview to Free chains and frees the controllers it drove. It also takes the now empty clip manager off the root, because the game crashes on a manager with no clips, and the export leaves it out. A file whose own manager has no clips shows "This file has a NiControllerManager with no clips: the game crashes on it." as a warning. Removing the last clip on a controller gives it that clip's keys, so the order of removes decides which keys come back. A key edit made on a clip is refused as a conflict when another clip has since taken its place (after a Revert); the Preview then returns to Free chains rather than show the other clip. When an entry of the root's controller chain cannot be read, adding, renaming and removing clips are refused ("The root's controller chain cannot be read.").
- **Timing.** Under **Remove clip**, the picked clip's **Cycle** (Loop, Reverse or Clamp), **Frequency**, **Start** and **Stop** (seconds) are editable; the Time slider follows them. A change is one undo step ("Set clip timing"), and a drag is one step. Text keys are left as they are. A bad value is not clamped: an amber line says why ("Clip SpecialIdle timing: start is after stop; the clip keeps the timing it had"), and the export writes that clip with its old timing. The checks: the times must be numbers, the cycle Loop, Reverse or Clamp, the frequency not 0, and start not after stop.
- **Split alpha** also works on an alpha a clip drives: both alphas keep following the clip.

**Colour strip.** A colour controller's curve graph has a strip right under it that shows the colour the curve gives at each time. Click or drag it to move the preview time. Where a channel is outside 0-1 (a bright emissive colour) the strip has a dashed band along its top, and that key's swatch has an outline. A float controller has no strip. The strip shows the stored colour, so it can look dimmer than the game, which multiplies it by Emissive Multiple.

**In game (simulated) and Edited source.** While an edit is open, the toggle above the view reads **In game (simulated)** and **Edited source**, and the edit opens on Edited source. In game (simulated) runs Tessalith's PGPatcher simulation on your edited NIF and shows still materials (the clock pauses). PGPatcher's real output, made from the unedited NIF, is not shown while you edit. The line under the toggle, and the toggle's hover, say "Simulated as PGPatcher 2.3.1: loose JSON only, no modrules priority, read at the last scan". The JSON is read in the background ("Reading PGPatcher JSON…") and kept until the next scan or another scan source. A folder scan has no MO2 instance: "In game needs an MO2 instance: no PGPatcher JSON was read." For the selected shape a line says what happened: no JSON matches it, PGPatcher skips it (PG_IGNORE, amber), a file is not applied and why (amber; the next file can still match), it is already PBR, or which keys are not previewed. An amber line says when the shape carries a PG_STOCK block: PGPatcher first reverts such a shape to its recorded stock state, and the preview shows it as loaded. A red line says when PGPatcher 2.3 crashes on a JSON (a match key such as `match_diffuse` or `nif_filter` that is not a string, in any JSON the instance loads): the whole run stops, so every shape stays as edited and shows one red line per such file, including shapes PGPatcher would leave alone (no texture set, PG_IGNORE). The Textures view's 3D set preview shows the same red lines and no patched material. A red line also says when PGPatcher makes the shape PBR while it has an Env map scale controller, which crashes the game.

The simulation does not cover: JSON inside BSAs; PGPatcher's own mod priority (`modrules.json`; the MO2 order stands in); plugin alternate textures (TXST); the plugin and mesh-blocklist rules that decide which meshes PGPatcher patches; complex-material and parallax competition; shapes with no texture set; JSON changed on disk since it was read (rescan). It follows PGPatcher 2.3.1 (it does not simulate the PG_STOCK revert).

## Artist checks

Moved to [Artist checks](wiki/GUI-Artist-Checks.md).

## The bottom panel

Moved to [The bottom panel](wiki/GUI-Bottom-Tabs.md), [Problems](wiki/GUI-Problems.md) and [Review and Export](wiki/GUI-Review-and-Export.md#review).

## Export

Moved to [Review and Export](wiki/GUI-Review-and-Export.md) and [Also export as Vanilla and CM](wiki/GUI-Also-Export.md).

## Messages and toasts

Moved to [The bottom panel, Messages and toasts](wiki/GUI-Bottom-Tabs.md#messages).

## Keyboard

Moved to [Keyboard and command palette](wiki/GUI-Keyboard.md#keyboard) in the wiki.

## Presets

Moved to [Presets](wiki/GUI-Presets.md).

## Words

Moved to [Main window](wiki/GUI-Main-Window.md#words) in the wiki.

## Where settings live

Moved to [GUI settings](wiki/GUI-Settings.md#where-settings-live) in the wiki.

## When something goes wrong

Moved to [GUI settings](wiki/GUI-Settings.md#when-something-goes-wrong) in the wiki.
