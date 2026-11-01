# GUI guide

A map of the GUI (`tessalith.exe`): the main window with its areas numbered, each linked to the page that explains it. New to the GUI? Start with [Main window](GUI-Main-Window.md).

## Main window

![Tessalith's main window with a folder open: the header on top, the Textures and Sets columns, the settings of the selected texture, the Advanced strip at the right and the bottom panel with its Problems tab. Numbered red markers point at the areas listed below.](../img/gui/GUI-Guide/main-window.png)

The picture shows a small made-up mod. The [2D preview](GUI-Preview.md) is not shown: it replaces the centre when you double-click a tile.

1. **Open, Undo, Redo** (header, far left). Pick a folder or a recent one, and step back and forward through your decisions. See [Header](GUI-Main-Window.md#header).
2. **Mode, MO2…, Preset** (header). Mode is the target (`cs` or `vanilla`), **MO2…** is the scan source, and **Preset** is the preset the folder converts with. The three are saved with the project. See [Header](GUI-Main-Window.md#header).
3. **Textures / Meshes tabs and the column eyes** (header, middle). Switch between the converter and the [mod scanning](Mod-Scanning.md) view, and show or hide the Textures and Sets columns. See [Header](GUI-Main-Window.md#header).
4. **Export to ▾** (header, right; **More ▾** until a folder is chosen). The folder the converted textures go to for this project. Its menu opens it, changes it and switches on the Vanilla and CM extras. See [Export to](GUI-Main-Window.md#export-to).
5. **Save** (header, right). Writes `sdc.toml` and `sdc-project.toml`. **Ctrl+S** does the same. See [Keyboard](GUI-Keyboard.md#keyboard).
6. **Export** (header, right edge). Converts the folder. If something is blocked or undecided, Review opens in the bottom panel first. See [Export](GUI-Main-Window.md#export) and [Review and Export](GUI-Review-and-Export.md).
7. **Texture filters** (top of the Textures column). Pills with counts (*Undecided*, *Warnings*, *Failed*, *Not in a set*), the **Filter ▾** popup, the tile size buttons and the search box. See [Textures column](GUI-Textures-Column.md#filters).
8. **Texture tiles** (Textures column). One tile per source file with its thumbnail, name and badges. An amber border means it still needs an answer; click a tile to select it, double-click for the preview. See [Textures column](GUI-Textures-Column.md) and [Deciding](GUI-Texture-Panel.md#deciding).
9. **Sets filters** (top of the Sets column). Pills for *Not ready*, *Ready* and *No JSON*, a **Filter ▾** and a search box for sets. See [Sets column](GUI-Sets-Column.md#filter).
10. **A set card** (Sets column). A set groups the textures of one material: its shader, a status dot, a warning count and a chip for each slot (D, N, P, RMAOS and so on). Drag a texture onto a card to move it there. See [Sets column](GUI-Sets-Column.md#set-cards).
11. **Settings of the selected texture** (centre). Output summary, then the groups Type, Colour, Alpha, Mipmaps and Compression. A texture that needs an answer shows an *Undecided* box above them. Selecting a set shows its recipes instead. See [Texture settings panel](GUI-Texture-Panel.md#centre).
12. **Advanced** (right edge). The drawer with tool paths, encoder and interface settings. Click the strip to open it. See [Advanced drawer](GUI-Advanced-Drawer.md).
13. **Bottom panel tabs** (full width, bottom). **Problems** lists every warning with its fix, **Review** joins while an export is reviewed, **Messages** holds this session's notes and **Files** shows an export's progress. Click a tab to open or collapse the panel. See [The bottom panel](GUI-Bottom-Tabs.md), [Problems](GUI-Problems.md) and [Review](GUI-Review-and-Export.md#review).

The line under the header is the [status line](GUI-Main-Window.md#status-line-and-banner).

## Pages

- [Main window](GUI-Main-Window.md): opening a folder, the header, the columns, dialogs and words.
- [GUI settings and recovery](GUI-Settings.md): where settings live, crashes.
- [Keyboard and command palette](GUI-Keyboard.md): every shortcut.
- [Textures column](GUI-Textures-Column.md): tiles, filters, sort and tile size.
- [Sets column](GUI-Sets-Column.md): set cards, the Sets filter, moving and reordering sets, slot menus.
- [Bulk editing and renaming](GUI-Bulk-and-Rename.md): selecting several, editing them together, the rename dialog and the input name box.
- [Texture settings panel](GUI-Texture-Panel.md): the settings of one texture, deciding, BC7 weighting, how edits are written.
- [Advanced drawer](GUI-Advanced-Drawer.md): batch, NVTT, preview and interface settings.
- [Helpers, RNM layers and cubemaps](GUI-Helpers-RNM-and-Cubemaps.md): the Helpers group and panel, RNM layers, the cube controls.
- [The 2D preview](GUI-Preview.md): source beside encode, readouts, pin and compare.
- [Generate from normal map](GUI-Generate-From-Normal.md): height, AO and curvature from a normal.
- [The set editor](GUI-Set-Editor.md): slot rows, packed channel sources, the shader.
- [The 3D preview](GUI-Set-Preview-3D.md): a set on a mesh with Community Shaders.
- [PGPatcher settings and the JSON](GUI-PGPatcher-JSON.md): TruePBR settings, the JSON file and the JSON editor.
- [Set materials](GUI-Set-Materials.md): the skin, terrain and grass material cards.
- [The bottom panel, Messages and toasts](GUI-Bottom-Tabs.md): the panel's tabs and the session log.
- [Problems](GUI-Problems.md): every row and its fix button, deciding from the list.
- [Artist checks](GUI-Artist-Checks.md): the six checks, overlays, half size.
- [Review and Export](GUI-Review-and-Export.md): Export, the Review panel, the Files tab.
- [Also export as Vanilla and CM](GUI-Also-Export.md): the extra Vanilla and CM folders.
- [Presets](GUI-Presets.md): the preset creator.
- [The Meshes view](GUI-Meshes.md): the mesh list, mesh detail and shape cards.
- [Editing a NIF](GUI-NIF-Edit.md): the edit panel, warnings, pending edits and export.
- [NIF animation](GUI-NIF-Animation.md): playback, controllers, clips, In game.
