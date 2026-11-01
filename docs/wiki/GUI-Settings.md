# GUI settings and recovery

Where the GUI keeps its settings, what it remembers between sessions, and what happens when something goes wrong. For anyone who wants to back up, move or reset Tessalith's state.

## Where settings live

| File | Contents |
|---|---|
| `<folder>\sdc.toml` | Target and per-file decisions. Shared with the CLI. |
| `<folder>\sdc-project.toml` | Sets, shaders, slot pins, output names, TruePBR settings, JSON name, delete sections, the export folder, and the hashes of the JSON files and borrowed-slot copies Tessalith wrote, and the records of generated files (`[generated]`). GUI only. |
| `%APPDATA%\sdc\gui.toml` | Window and app settings: the default export folder (a project's own is in its `sdc-project.toml`), recent folders (10), preview limit, 16384 previews, Live encode, BC1 decoder, worker threads, memory budget, column visibility and widths, window size and position, Textures filters, sort and tile size, 2D/3D choice, 3D View controls, Advanced drawer state, NVTT path, the scan source (`[meshes]`) and the 3D preview's shader folder and size (`[render3d]`). Never part of `sdc.toml`. |
| `%APPDATA%\sdc\config.toml` | The user config, also read by the CLI. |
| `%APPDATA%\sdc\presets\*.toml` | User presets, also read by the CLI. A file that fails to load is skipped. |
| `%LOCALAPPDATA%\sdc\scan.db` | The mod scan database. |

The BC1 decoder model is read once when the GUI starts: after changing it the GUI shows "restart Tessalith to apply". The preset inspector in the Advanced drawer lists the user presets that loaded, with their files. The header's **Preset** menu picks the project's preset (written to `sdc.toml` as `preset`), and **Presets…** makes and edits user presets (see [Presets](GUI-Presets.md#presets)).

## What Tessalith remembers

These come back at the next start:
- the window: its size, its position and whether it is maximised;
- the width of each column and panel you dragged;
- the Textures or Meshes view;
- the Textures filters, sort and tile size;
- the Sets filter pills and shader (not the search text);
- the preview channel and layout;
- the 3D View controls (background, sun and its turn, point light, radius, Accurate (deferred)).

The zoom, the selection and the open folder are not remembered.

How `gui.toml` is written:
- It is saved a couple of seconds after you change something, and when you close the window. Starting and closing without changing anything does not rewrite it.
- If the saved window would be off every screen (a monitor was unplugged), it opens centred on the main screen. If only its title bar would be off the screen, it moves down so you can still drag it.
- A value that makes no sense is ignored on its own; the rest of the file still applies.
- If you edit `gui.toml` while Tessalith runs, your edit is kept: the next save reads the file again and keeps every value Tessalith has not changed since its last save. A value both changed takes Tessalith's.
- Recent folders merge folder by folder: a folder you removed by hand stays removed, one you added stays, and folders Tessalith added are kept.
- If `gui.toml` cannot be written, a warning says why, once per distinct error.
- A `gui.toml` that does not parse is kept as `gui.toml.bad`, with a warning, before the new one is written.

## Analyses are reused

Tessalith keeps what it learned about each image (its size, alpha, statistics and thumbnail) in `%LOCALAPPDATA%\sdc\analysis-cache`. A folder you opened before reuses the analyses of unchanged files instead of decoding them again. The GUI prunes the folder to 200 MB when it starts, and it is safe to delete. Only the GUI uses it.

## Fonts

Tessalith ships no fonts. It loads the Windows system fonts at start-up and refuses to start, with a message, if Segoe UI is missing. Segoe UI Symbol, Segoe UI Emoji and Consolas are used when present and skipped when missing.

## When something goes wrong

- A background job that crashes shows an "internal error in …" dialog, and the window keeps running.
- A failed plan or export shows its error in a dialog and in the status line.

See also [Troubleshooting](Troubleshooting.md).

### After a crash

If Tessalith itself crashes, the error goes to `%LOCALAPPDATA%\sdc\logs\crash-<time>.log`, and unsaved edits are written beside their file as `sdc.toml.recovery` / `sdc-project.toml.recovery`. The originals are not touched.

When you next open the folder, a `.recovery` file that is newer than its original is loaded into the document by itself:
- It counts as an unsaved change and is one Undo step called "Restore crash edits".
- A toast says "Restored unsaved edits from the crash" with an Undo button. The toast's Undo takes the edits back and deletes the `.recovery` file, because you turned the edits down. Ctrl+Z takes them back too, but keeps the file, so the next open restores them again.
- Saving the document (Save, or Overwrite when it changed on disk) deletes the file. The saves Tessalith makes on its own, such as recording an export folder or a JSON hash, keep it.
- An empty file, or one with the same text as the document, is deleted without a word. One older than its original is ignored.
- There is no banner, except when `sdc.toml` or `sdc-project.toml` does not parse: that document is read-only and cannot take the edits, so its `.recovery` file stays and the banner says where the edits are. They come back when you fix the file and reload.
- A second crash before that keeps the first `.recovery` file as `.recovery.<time>`.

### When the GPU device is lost

If the graphics driver drops the GPU device (a driver reset, for example), the log says so on a "wgpu device lost" line. This is not a crash: Tessalith shows a message ("Tessalith restarts and brings your unsaved edits back", button **Restart**), saves the window layout, and starts itself again on the same folder, where the edits are restored as described above. If the restarted Tessalith loses the device again within 60 s, or cannot restart, it shows a message saying the edits were kept and come back the next time you open the folder, and closes.
