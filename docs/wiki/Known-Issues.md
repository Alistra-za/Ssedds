# Known issues

This page covers version 1.0.0. These are the things you may run into, and what to do about them. For fixes to common errors, see [Troubleshooting](Troubleshooting.md).

## Crashes and memory

**The app can crash with a lost graphics device (GPU)**

Sometimes the graphics device is lost, and the app has to close. The cause is not proven. It often happens after a long session, around alt-tab or screen capture.

- Version 1.0.0 closes with a crash message about failing to create a "staging buffer".
- Later versions show a message that the graphics device was lost and that unsaved edits were kept, then close.

- Unsaved edits to `sdc.toml` and `sdc-project.toml` are kept beside the original as `<file>.recovery`. The original is not touched.
- The next time you open the folder, the edits come back by themselves as unsaved changes, and a toast says "Restored unsaved edits from the crash". The toast's Undo takes them back and deletes the `.recovery` file. Ctrl+Z takes them back but keeps the file. Clicking Save deletes it; the saves Tessalith makes on its own (an export folder, an export's JSON record) keep it.
- If `sdc.toml` or `sdc-project.toml` does not parse, it is read-only and cannot take the edits. The `.recovery` file stays, a banner names it, and the edits come back when you fix the file and reload.
- The crash log is in `%LOCALAPPDATA%\sdc\logs`, one `crash-<date>-<time>.log` per run. Attach it if you report the crash.

## Speed

**The NVTT encoder runs one file at a time**

Batches that use NVTT do not use extra CPU cores for the encoding. It is slower than it could be, but the output is correct.

**The Missing and Unused lists can freeze briefly the first time you open them**

On a very large load-order scan, the first open reads the data on the main thread, so the window stops responding for a moment. It comes back by itself.

## Things that do not work yet

**The Advanced skin card does not export**

The card (Advanced skin (Community Shaders)) saves its values in the project, but nothing is written to Community Shaders yet. The **Export JSON...** button on it is greyed out on purpose.

**PBR grass is approximated in the 3D preview**

The PBR grass shader type is available, but the preview draws it with the normal PBR shader. It has no wind, no wrapped diffuse and no grass LOD. The exported files are not affected.

**Debug views depend on your Community Shaders build**

The 3D preview's debug views (View menu, **Debug view**) follow the Community Shaders build you have installed. A view may differ from the game, or be missing, on another build.

## Behaviour that may surprise you

**Renaming a colour map's output path loses the shader suggestion**

If you change the output path of a colour map in the GUI, the shader suggestion that came from the meshes can disappear. The suggestion looks meshes up by the new path, and no mesh uses it. Pick the shader by hand.

**Plain `_normal`, `_nrm` and `_norm` maps are treated as DirectX green**

These names do not say whether the green channel is DirectX or OpenGL. Tessalith assumes DirectX (Skyrim's convention) and adds a note. If the map is really OpenGL, click **Treat as OpenGL (flip green)** in the GUI, or set `channels.green_convention = "opengl"` for the file in `sdc.toml`. See [Troubleshooting](Troubleshooting.md#a-normal-map-reports-in-green-unstated).

**Windows SmartScreen warns on first run**

The program is not code-signed yet. On first run, click **More info**, then **Run anyway**.

## Host plugins

**Paint.NET opens DDS files with its own reader**

Paint.NET always opens `.dds` with its built-in support, even files Tessalith wrote. To save with Tessalith again, use Save As once and pick **Tessalith DDS**.

**chaiNNer: the nodes vanish after a chaiNNer update**

An update installs chaiNNer into a new folder and deletes the old one. Close chaiNNer and run `install.ps1` again.

**Photoshop: a normal map converted from 16-bit to 32-bit**

Photoshop makes the values linear during that conversion, and the plugin then saves them as raw data. Convert the document back to 16-bit before saving. A normal map that was made as 32-bit (for example a Marmoset bake) is fine.

See [Host plugins](Host-Plugins.md) for more.

## Reporting a problem

Open an issue on the Tessalith GitHub page. Please include:

- The Tessalith version (the exe's file properties, or `sdc --version`).
- Your GPU and driver.
- Your Community Shaders and PGPatcher versions.
- The crash log from `%LOCALAPPDATA%\sdc\logs`, if the app crashed.
- The project's `sdc.toml`.
- What you did, what you expected and what happened.
