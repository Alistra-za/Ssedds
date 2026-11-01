# Keyboard and command palette

Every keyboard shortcut of the GUI and how the command palette finds actions, textures and sets. For anyone who prefers keys to the mouse.

## Keyboard

Press **F1** (or **?**) for the list of shortcuts, grouped by where they work. The sheet and the palette appear at once, without the fade a dialog opened with the mouse has, as does the path list when typing reopens it.

### Command palette

Press **Ctrl+K** for the command palette. Type part of an action's, a texture's or a set's name: the letters in order are enough. Names that start with what you typed come first, then names that contain it. Up and Down move, Enter runs the action or selects the texture or set as a click does and scrolls its column to it, and Esc closes the palette.

The palette offers the actions that make sense where you are (Open folder, Save, Export, Undo, Redo, Rename, the Textures | Meshes view, the set's 2D | 3D view, Live encode, the PGPatcher JSON editor and more) and lists at most 200 results.

### Keys and text fields

While a text field has the keyboard (the search box, a name, a number), the shortcuts leave it alone: only **Ctrl+K** opens the palette, **Ctrl+S** saves, and **Esc** leaves the field.

- **Ctrl+S**, or a click on **Save**, **Export** or Review's **Export**, first ends the edit, then saves or exports. A field that commits when you leave it keeps what you typed: one key or one click does both.
- A click on another texture or set in a column likewise commits the field before the selection changes, also for a quick tap.
- Esc in a text field that commits when you leave it (a name, a match path, the JSON name) cancels the edit.
- Undo and Redo work in the Textures and Meshes views.
- A key does nothing while its action is off: Ctrl+S with nothing to save outside a text field, Ctrl+Z with nothing to undo.
- No shortcut acts while a dialog, a menu or a box is open. A dialog has its own Enter and Esc.
- In the Textures grid the arrow keys follow the order on show (search and filters applied): Left and Right step one texture, Up and Down one row. Shift with an arrow selects the range from the active texture, as a Shift-click does.

### All shortcuts

| Area | Keys | Action |
|---|---|---|
| Global | Ctrl+K | Command palette |
| Global | F1 or ? | Keyboard shortcuts |
| Global | Ctrl+M | Messages |
| Global | Ctrl+S | Save |
| Global | Ctrl+Z | Undo |
| Global | Ctrl+Y or Ctrl+Shift+Z | Redo |
| Textures grid | Ctrl+A | Select every visible texture (every set when a set is active) |
| Textures grid | Left | Select the texture to the left |
| Textures grid | Right | Select the texture to the right |
| Textures grid | Up | Select the texture above |
| Textures grid | Down | Select the texture below |
| Textures grid | Shift+Left | Extend the selection to the left |
| Textures grid | Shift+Right | Extend the selection to the right |
| Textures grid | Shift+Up | Extend the selection up a row |
| Textures grid | Shift+Down | Extend the selection down a row |
| Textures grid | Enter or Space | Open the expanded preview of the active texture |
| Preview | Esc | Close the preview |
| Preview | [ | Previous texture |
| Preview | ] | Next texture |
| Preview | L | Magnifier lens on or off |
| Preview | T | Tile the picture 3×3 on or off |
| Preview | P | Pin this result |
| Preview | 1 | Show RGBA |
| Preview | 2 | Show R |
| Preview | 3 | Show G |
| Preview | 4 | Show B |
| Preview | 5 | Show A |
| Preview | 6 | Show the α-mask |
| Preview | PageUp | Larger mip |
| Preview | PageDown | Smaller mip |
| Sets | F2 | Rename the outputs of the selected sets, or the inputs of the selected textures |
| 3D preview | Home | Recentre the 3D view (as a double-click) |
| 3D preview | V | Next 3D debug view (Lit, UV checker) |
| 3D preview | Ctrl+Shift+C | Copy the 3D view as an image |
| JSON editor | F8 | Next error in the PGPatcher JSON editor |
| JSON editor | Shift+F8 | Previous error in the PGPatcher JSON editor |
| Problems list | Enter or Space | Accept the focused Problems row's suggestion and go to the next row |
| Dialogs | Enter | Press the dialog's safe default |
| Dialogs | Esc | Cancel the dialog |
