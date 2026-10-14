# sdc GUI tweaks (owner request, 2026-10-14)

The owner's list, with three screenshots (the set editor heading, the set editor with the 3D panel, the Meshes view).
Each item below is one ask, what sdc now does, and the default picked where the ask left a choice open.

## G1 Input name picker

Ask: "a dropdown file selector here to change input name exactly (from textures paths we have)", on the set editor
heading (`textures/landscape/grass/grassfieldtundra → textures\pbr\…`).

- An **Input name…** menu beside the heading lists known texture paths: every path the last mod scan saw in a mesh's
  diffuse slot (`tex_use`, slot 0), plus this project's own diffuse game paths. A filter box narrows it (all words
  must appear); at most 200 rows are listed. A typed path can be used as it is.
- Picking a path gives every member of the set an `[inputs]` alias (F3b AA1): each member keeps its own map suffix
  and extension, and files on disk are never renamed. One undo step, one plan.
- **Default (owner may overrule):** "input name" is read as the set's base name, i.e. the F3b input alias, not the
  PGPatcher `rename` key (that is the output side, F3b AA3).
- The set's `sdc-project.toml` entry follows the new automatic set id (`SetsDoc::retarget_auto`), so its TruePBR
  settings, locks and borrows stay with it.
- Refused, with the reason in the menu: a path outside the folder the project is read from (the part of the game path
  above `sdc.toml`, usually `textures/`), a name another file already has, a locked slot, the same name.

## G2 2D | 3D switch

Ask: "Set preview 3D and 2D should be a button to switch the two instead of both displayed at once (true pbr should
be visible in both)."

- A **2D | 3D** switch on the set editor heading and on the 3D panel heading (`settings.set_view_3d`, kept in
  `gui.toml`, default 2D).
- 2D: slot cards and TruePBR settings, no 3D panel. 3D: the 3D panel (now 480 px wide by default) and the TruePBR
  settings; the slot cards are left out. The 3D preview does no work while it is not shown.
- With a texture of a set selected, the 3D panel shows beside its settings in 3D only.

## G3 TruePBR JSON editor window

Ask: "a global whole file json editor (separate window popup) for true pbr, each material in its own pill".

- **Editor…** next to the JSON name in the header, and **Open the TruePBR JSON editor…** at the top of a set's
  TruePBR box, open it. It is a native window of its own (an egui immediate viewport); where the platform has no
  second window it is an embedded one.
- Each PBR set's entry is a rounded pill (title, output prefix, **Show** to select the set), then its keys, each a
  small pill with the same control as the TruePBR box. **+ key** adds any schema key (a text key waits for its
  text). Keys sdc does not know are listed in amber and can be removed.
- **JSON text** shows the file as it would be written now (every set as if exported), with **Copy**.
- Sync: both the window and the TruePBR box edit the set's settings through `AppModel::set_json_value`, so each shows
  the other's edits at once. Undo is shared.

## G4 Delete sections

Ask: "the option to add sections to it (for delete keys)".

- **+ Delete section** adds an entry that belongs to no set and deletes the trishapes it matches: for example a
  vanilla glow shape, when the PBR base shape uses emissive instead (owner, 2026-10-14). Sections are
  `[[pgp_sections]]` tables in `sdc-project.toml` (keys `match1`–`match8`, `path_contains`, `nif_filter`).
- Every section is written with `delete: true`, after every set's entry. A section with no texture match is not
  written (a mesh filter alone matches nothing); the editor says so.
- A JSON with only delete sections is written too.

## G5 Key pills with exact or path matches

Ask: "every key should be a little pill and if applicable for textures or nif filters, should have a little dropdown
for exact matches or path matches depending on the type."

- A texture match pill has a drop-down for how it matches: **exact · slot N** (`matchN`) or **path contains**
  (`path_contains`), and a **▾** picker: exact lists the scan's textures used in slot N, path contains lists every
  known texture path.
- The `nif_filter` pill (**mesh path contains**) has a **▾** picker of folders that hold scanned meshes.

## G6 Delete hits

Ask: "a delete key that would affect another textureset should cross it out and warn."

- A written delete section hits a set when one of its exact matches is the set's texture in that slot, or its
  `path_contains` is in the set's diffuse path. A `nif_filter` narrows the hit to those meshes; the warning says so.
- The hit set's pill title is struck through and outlined amber, with the warning; the section lists the sets it hits;
  the set editor shows the warning with a button to the editor.

## G7 Scan on the Textures view

The scan source, **Choose MO2 instance…**, **Scan a folder…** and **Scan** / **Cancel** with progress are on a row
under the Textures view's header too. Scan details, banners and plugin references stay in the Meshes view. The scan
is activated at start, not on the first visit to the Meshes view.

## G8 Column checkboxes and arrows

"Collapse columns" is replaced by **Textures column** and **Sets column** checkboxes (show or hide each column). Each
column heading has a **◀** that collapses it to its strip; the strip shows **▶** and expands it on click. Kept in
`gui.toml` (`textures_hidden`, `sets_hidden`, `textures_collapsed`, `sets_collapsed`); the old `columns_collapsed`
is ignored.

## G9 Meshes view: one column for mods and meshes

The Mods and NIFs columns are one **Mods and meshes** column. A mod opens with its ▼ (or a click on its name) to list
its NIFs under it and closes with ▲; separators use the same arrows. The NIF chips and path filter apply to the open
mod's list. **All meshes** lists the search in the column instead.

## Known paths

`AppModel::known` holds the scan's texture paths (with the lighting slots each is used in) and mesh folders, read on a
worker at start and after each scan (`model::known_paths`). Without a scan the pickers list only the project's own
paths.

## Out of scope

Editing the fixed `default` block; reordering entries; sections in user configs or presets; checking delete hits
against meshes (a hit is by texture path, so a `nif_filter` hit may not apply to every mesh).
