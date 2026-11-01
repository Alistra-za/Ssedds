# The set editor

What the centre column shows with one set selected: its slot rows, packed channel sources, shader and material tabs. For anyone filling a material set's slots or checking why one is not ready. The set's cards and menus in the Sets column are on [Sets column](GUI-Sets-Column.md).

## The set editor

Select a set to edit it. The heading row has the input name box ([Input name box](GUI-Bulk-and-Rename.md#input-name-box)), the **Match** box on a TruePBR set, and the **2D | 3D** switch ([The 3D preview](GUI-Set-Preview-3D.md#a-set-in-3d)).

In 2D the editor is one column that scrolls as a whole, like a texture's settings:

1. the input name box at the top;
2. the slot rows at full width;
3. two tabs: **Material** (**Set** on a vanilla set), which holds the material settings, and **Advanced**, the last tab, with the same sections as the [Advanced drawer](GUI-Advanced-Drawer.md#advanced-drawer).

Nothing sits beside the slot rows, so nothing covers them at any window width. A row wider than the centre is cut at the window edge (it does not scroll sideways). The window-edge drawer is not drawn while the tabs are shown ([Advanced drawer](GUI-Advanced-Drawer.md#advanced-drawer) says when each is).

What the Material tab holds depends on the set: the [PGPatcher settings](GUI-PGPatcher-JSON.md#pgpatcher-settings) on a TruePBR set, or one of the cards on [Set materials](GUI-Set-Materials.md) (Advanced skin, PBR Landscape, CS PBR records, PBR grass).

## Slot rows

Each PGPatcher slot is one row, in slot order, so there are at most 8: Diffuse, Normal, Emissive _g (or Skin _sk), Height, RMAOS, **Coat normal / Fuzz (slot 7)**, Subsurface. Only the slots the set's shader can use are shown. A filled slot the shader cannot use stays, with an amber outline, and its title says why on hover. The cubemap slot has no row; set it with a slot command.

- Slot 7 holds the set's `_cnr` or `_f`, whichever it has. With both, the row shows the conflict in red (PGPatcher has one slot 7), lists both files with a ✕ each to take one out of the set, and export warns "_cnr and _f both use slot 7". Its slot commands, file drop and PGPatcher lock (`lock_cnr`) are slot 7's. The same holds for a set with both a `_g` and an `_sk` on slot 3.
- A `_coat` map fills the Subsurface row, which then reads **Coat colour + strength**: its alpha is the coat strength and is kept.
- A complex material `_m` (packed or loose) has no row. Its **Enable parallax** box sits under the rows, with a packed `_m`'s fill rows ([Texture settings panel](GUI-Texture-Panel.md#centre)).
- When RNM normal layers are active, the normal row has an outline in the accent colour (teal); see [RNM normal layers](GUI-Helpers-RNM-and-Cubemaps.md#rnm-normal-layers).

A row has these columns:

| Column | Shows |
|---|---|
| *Slot* | The title; weak when the slot is empty; a lock when pinned. |
| Thumbnail | Click selects, double-click expands. The PGPatcher lock padlock and its hover are on it. |
| *File* | The name cut to fit (the full name and size on hover), and under it the dimensions and file size. |
| *Format* | The format dropdown (format and tag together, one undo step). An empty slot shows **Assign…** here, which opens the slot menu, headed by the slot's name ("Emissive / Skin (slot 3)"). A packed output's recipe and fills sit under the dropdown. |
| *Flags* | Small toggles, each with a hover: **Match on this** (Diffuse, Normal: PGPatcher applies the entry to meshes with this exact texture in this slot; see [Matching](GUI-PGPatcher-JSON.md#pgpatcher-settings)) and **Opaque** (Diffuse: strips alpha, so the diffuse is fully opaque). |
| *Status* | A dot: green ready, amber a warning or decision, red blocks. Its hover gives the reason, such as "borrowed from X", "not used by the shader" or "RNM, 2 layers". |

The File column takes the width the other columns leave, so the rows fill the centre. While a set is selected, the Textures and Sets columns give the centre at least 540 px, down to their own minimums, so the rows fit without clipping from a 1024 px window up.

**Slot commands…** on a row offers *From this set*, *Path*, *Clear* and *Pin slot N* (the sdc pin), each one undo step. Right-click on a row opens the full [slot menu](GUI-Sets-Column.md#slot-menus), which also has *From another set*, the pin and the PGPatcher lock.

A row whose texture just changed (a drop, **Replace**, **Assign…**, a file from Explorer, undo or redo) outlines in the accent colour for about a third of a second, then fades. Only the first changed slot flashes, and a slot that only emptied does not. Two slot kinds share a number (Emissive and Skin 3, Coat normal and Fuzz 7), so with both filled a change to one of them may not flash. While the pointer is down on a slot's thumbnail it shrinks slightly (3 %).

## Packed channel sources

Under a packed output's format dropdown (`_rmaos`, `_rfaos`, `_m`, `_p`), each channel row is the channel's role (for example *AO*), then a **Source** picker. It reads **Source: <file>** or **Source: value**; it is as narrow as the format combo, so a long name is cut, and the hover shows it whole. The list holds the set's greyscale helpers (any role but the RNM layers) and **Value**:

- Picking a helper makes it this channel's source: the channel is added to that helper's `helper.fills` and taken out of every other helper's list, as one undo step. The [helper panel](GUI-Helpers-RNM-and-Cubemaps.md#the-helper-panel)'s Fills list shows the same setting.
- **Value** takes the channel out of every helper's list, so the channel uses its value fill: the row then shows the value box and its **Reset** after the picker (`packing.fill.*`). **Reset** removes the fill. Value is greyed out when the channel's file is not a helper ("<file> is not a helper; it feeds this channel by its name"), because no helper setting holds it.
- Picking the source already shown changes nothing. The engine still decides: a pick that cannot fill the channel shows the helper as not used, with the reason, in its row and panel.

## Drop files from Explorer

A file dropped on a slot row fills that slot, as one undo step, when it is a texture of the open folder and its sub-type belongs in that slot. It moves in the way a texture dragged from the Textures column does: a filled slot asks **Replace** or **Cancel**, a pinned slot refuses, and Share or Duplicate are not offered, so nothing is copied.

- A file outside the open folder is not copied either: the line under the rows says "brick.png is outside the open folder; files are never copied".
- Drop several files and only the first is used ("only the first file was used").
- A file dropped anywhere else opens its folder.

## Shader

The set's **Shader** dropdown lists the shaders the current Mode offers. The pick reads "(auto)" when Tessalith chose it, and "(fallback)" when the shader you asked for does not exist under the current Mode and `default` stands in. **Auto** clears your choice. Warnings (a missing or unused map, an unavailable shader) show on the set card's **⚠ n** button. A scanned mod's meshes decide the auto pick unless a PBR or skin member, a complex `_m` or a `_p` does. How each shader is picked and what it changes is in [Set shaders](Set-Shaders.md#set-shaders).

## Used by plugins

**Used by plugins** lists the plugin texture sets (TXST) and alternate textures that use this set, from the last scan, with **Refresh**.

## Path pickers

The ▾ beside the input name box, and the ▾ beside a match path or a mesh path in the [JSON editor](GUI-PGPatcher-JSON.md#the-json-editor), open a list of known paths with a filter box. Clicking or typing inside it (the box, the list, its scroll bar) keeps it open; it closes on a pick, on Esc, or on a click outside.
