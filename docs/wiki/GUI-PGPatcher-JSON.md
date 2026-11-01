# PGPatcher settings and the JSON

The TruePBR settings a PBR set has under `cs`, the PGPatcher JSON that Export writes from them, and the JSON editor that shows every entry at once. For anyone making a PBR mod that PGPatcher patches. What each key does in game is PGPatcher's documentation; this page covers how Tessalith edits and writes them.

## PGPatcher settings

A PBR set under `cs` has the settings PGPatcher reads in the set editor's **Material** tab, as framed groups in this order:

| Group | Settings |
|---|---|
| Material | Specular level, Roughness scale. |
| Features | Emissive, Parallax, Hair, Multilayer (coat), Glint, Fuzz. |
| Coat | Under the **Multilayer (coat)** checkbox: coat colour, specular level, roughness, strength, diffuse, normal, parallax and inner UV scale. |
| Subsurface | The **Off / Standard / Foliage** selector, then colour and opacity for Standard and Foliage. |
| Emissive settings | Emissive scale, Emissive colour. |
| Parallax settings | Displacement scale. |
| Glint settings | The glint parameters. |
| Fuzz settings | Fuzz colour and weight. |
| NiAlphaProperty | The schema's `alphaprop_*` keys ([below](#nialphaproperty)). Starts closed. |
| Matching | The exact texture paths the entry matches (PGPatcher's `match1` to `match8`). |
| CS PBR records | Community Shaders' PBR texture set and material object blocks ([Set materials](GUI-Set-Materials.md#community-shaders-pbr-records)). |
| Advanced TruePBR settings | Path contains, Mesh path contains, Rename; UV and cubemap (Smooth angle, Auto UV, UV scale, Cubemap path); vertex colours and depth; Texture locks. Starts closed. |

A small **TruePBR JSON editor…** button beside the Material tab opens the [JSON editor](#the-json-editor).

**Headers.** A header shows the group title and, right-aligned, its value: Subsurface Off / Standard / Foliage, Coat / Emissive / Parallax / Glint / Fuzz "on" or "off", NiAlphaProperty "set" or "off", Matching "2 matches" or an amber "matches nothing", CS PBR records "N records". Click a title to open or close it.

- The feature groups are always listed. While the feature is off they read "off" and are closed, and one opens when its feature turns on.
- A feature's state shows once, in its own group's header; the Features group (the ticks) has no value.
- A feature that is on but blocked (multilayer with hair) reads "on, blocked by hair" (amber) in its own header, stays closed and stays ticked in Features.

**Readable labels.** Skin and complex-material keys show plain names ("SSS width", "Fuzz F0"); hover one for its exact key.

**Features that cannot go together** are greyed, with the reason on hover: hair excludes multilayer, glint, fuzz, emissive, parallax and subsurface; multilayer excludes subsurface; multilayer, glint and fuzz exclude each other; fuzz and coat normal exclude each other, because they share one slot. A switch that is already on stays enabled so you can turn it off. A feature that is on without its texture blocks export.

**Switches turn on by themselves from the set's textures:** a `_cnr` turns on Multilayer and Coat normal, an `_f` Fuzz, a `_p` Parallax, a `_g` Emissive, and an `_s` Subsurface (Foliage when the set's scanned meshes are Tree Anim, or its path is under a trees or plants folder and there is no scan; Coat diffuse while Multilayer is on). They read "(auto)", with the reason on hover, and are written to the JSON. Untick one to store an explicit off, which wins; reset removes it. Removing the texture removes the auto value. Nothing is saved to the project until you set it. A hair diffuse defaults to cutout + blend alpha.

**Coat.** Multilayer is what makes the coat values take effect. With Multilayer off the group reads "off" and stays closed; the values are kept but not written, and a note says how many are kept. Inner UV scale only shows where the diffuse alpha is below 1 and the set has an inner-layer texture.

**Subsurface** is one selector, the first row of its group. Standard writes `subsurface`, Foliage writes `subsurface_foliage` (the Tree Anim term in Community Shaders), Off writes neither; it never writes both. Foliage needs an `_s`, like Standard.

- A project that stores both shows "Standard + Foliage (conflict)" and is not rewritten until you pick a mode.
- Export asks first on a conflict, on Foliage for a set whose scanned meshes are not Tree Anim, and on Standard for a set whose scanned meshes are Tree Anim (these two need a scan).

**Numbers.** Numbers cannot go below 0, and strengths, weights and opacities stay within 0–1. A typed value outside the range is clamped, and the box says "clamped to ≥ 0" or "clamped to 0–1". A value hand-edited in `sdc-project.toml` shows as it is, with the same note, and the JSON gets it clamped. The glint log density has no limit. Each number drags at its own speed and shows its own number of decimals: angles in half-degree steps, the specular levels in thousandths, the glint density randomisation in whole numbers. Keys whose useful values span a wide range (displacement, emissive scale, specular levels) speed up as their value grows.

**Colours.** Every colour input has a swatch that opens a colour square and a `#RRGGBB` box (the `#` is optional, case is ignored). The box commits on Enter or focus loss; invalid text gets a red outline and is not applied.

- Beside a TruePBR swatch (the coat colour included) are three number boxes, one float per channel (3 decimals) from 0 to 10. A channel above 1.0 is real (fuzz colour 1.25 is common).
- The swatch and the hex box show the colour clamped to 1.0, and write 0–1 values only when you change the colour through them, so opening the colour square and closing it leaves a stored 1.25 alone. The JSON gets the value as stored (0.08 stays 0.08).
- A colour saved as 0–255 whole numbers reads as value/255 and is written unchanged until you edit it.
- Switching **Fuzz** on starts from colour 0.08, 0.08, 0.08 and weight 1.

**Reset** (↺) returns one field to its default. There is no whole-set reset; *Reset all settings for N selected…* clears the selected textures' own entries.

PGPatcher's own `default` block is fixed: you cannot edit defaults. Each entry writes `emissive`, `parallax` and `subsurface` only when on, and omits the three numbers when unset or equal to the fixed value.

### Matching

**Matching** lists the exact texture paths the entry matches. The entry applies when any of them matches. The diffuse path is on by default.

- **+** adds a row for slot 3 to 8. A slot that holds a texture starts from the set's path for it; any other slot starts empty and says "this row matches nothing until a path is entered" (an empty row is not written to the JSON). **×** removes a row.
- A set that matches nothing is left out of the JSON, and Export asks first.
- A renamed set has one match: it says "renamed: matches by diffuse (PGPatcher rename needs match_diffuse)", and its entry is written as `match_diffuse` + `rename`, with no `match1`–`match8`. **+**, **Match on this** and the JSON editor's Match entries are greyed out while it has its one match, with the reason on hover. Extra rows from before the rename stay, say "not written", and Export asks about them. Path contains is treated the same way: a renamed set takes none, and one already there is not written and is marked so.

**Rename** (Advanced TruePBR settings) takes a new file name only, no folders. The outputs keep the matched texture's folder under `textures\pbr\` and take the new name, and Tessalith writes it to the JSON next to the matched diffuse as `match_diffuse`, the key PGPatcher renames from. If matches point at different textures and Rename is empty, a warning says the output prefix depends on which one hits first: edit the match paths so they point at the same texture.

### NiAlphaProperty

The collapsible **NiAlphaProperty** group holds the schema's `alphaprop_*` keys, which set the alpha blending and alpha testing of the shape in the NIF. A key that is off is not written, and each key that is set has a **reset** button.

- **Override flags** writes `alphaprop_flags`. The controls under it (Enable blending, Source and Destination blend mode, Enable testing, Test function) compose the number as NifSkope does, and "= 4844" shows it with an **edit raw** box (0–65535; anything else is outlined red and not written). Switching it on starts at 4844: testing on, Greater, Src Alpha / Inv Src Alpha, blending off.
- **No sorter (no effect in Skyrim SE)** comes last. A RenderDoc capture showed the game draws a blended shape in its stored triangle order whether bit 13 is set or not, so it is kept only so a stored value round-trips; Tessalith never sets it.
- **Alpha test threshold** writes `alphaprop_thresh` (0–255, starts at 128).
- **Create NiAlphaProperty if missing** gives shapes without one a new property. Without it, the flags and threshold change only shapes that already have one, and Export warns about that.
- **Delete NiAlphaProperty** removes the property and wins over the other alpha keys of the entry: they are greyed, and Export warns if any is set.
- A hand-edited `4844.0` reads as 4844. A negative, fractional or too large value shows an amber "stored … is not valid" note (the key counts as set, and the first edit replaces it).

The JSON editor lists the same keys.

## The JSON file

After a complete export, Export writes `<export folder>\PBRNIFPatcher\<name>.json`, with `<name>` from the editor's **JSON name:** field (by default the folder name). It is written in `cs` mode only.

- Only sets that exported fully are written, and the file name must be a single plain file name.
- If the file already exists and is not the one Tessalith wrote last time, you are asked before it is overwritten; a byte-identical file is never prompted.
- With a read-only `sdc-project.toml` the JSON is still written, but the next export asks before overwriting.

## The JSON editor

Open it with **PGPatcher JSON editor…** in the header's **Export to ▾** menu (**More ▾** until a folder is chosen), the **TruePBR JSON editor…** button beside the Material tab, or the command palette. It shows the whole PGPatcher JSON as a tab of the centre column, with **Pop out**, **Dock** and **Close** as the preset creator has ([Presets](GUI-Guide.md#presets)): one entry per TruePBR set, then the delete sections. A popped-out editor is a window titled "TruePBR JSON — Tessalith". Advanced skin sets have no entry.

Its top field, **JSON name:**, is the JSON's file name (by default the folder name, shown as grey hint text; empty restores it), with the full path beside it. A change applies when you leave the field, as one undo step.

- Each PBR set's entry is a pill with its title, output prefix and **Show** (selects the set and brings its editor to the front), then its keys as small pills with the same controls as the Material tab's groups. Each entry has a **Collapse** / **Expand** button.
- **+ key** adds any schema key. Keys Tessalith does not know are shown in amber and kept as written; they can be removed.
- A texture match pill picks how it matches: *Match: exact, slot N (diffuse)* (`matchN`) or *path contains*. A set's own diffuse path reads *derived from the set*. A lock key reads *Keep the original texture (PGPatcher won't replace it): diffuse* (`lock_diffuse`, shown on hover).
- The **▾** [path picker](GUI-Set-Editor.md#path-pickers) on a match lists the scan's textures for that slot, or every known texture path. The *mesh path contains* pill (`nif_filter`) has a picker of folders that hold scanned meshes. Without a scan, the pickers list only the project's own paths.
- **+ Delete section** adds an entry that belongs to no set and makes PGPatcher delete the shapes it matches, for example a vanilla glow shape the PBR base shape replaces. Sections are saved as `[[pgp_sections]]` in `sdc-project.toml` (keys `match1` to `match8`, `path_contains`, `nif_filter`) and written with `delete: true` after every set's entry. A section with no texture match is not written.
- A delete section that hits one of your sets (by exact match on the set's texture, or `path_contains` in its diffuse path) strikes that set's pill through, outlines it amber and warns. The set editor shows the same warning, with a button to the editor.
- The header reads "N errors, M warnings". Errors are what blocks Export (a red line under the entry they belong to); warnings are what Export asks about (an amber line). A problem that belongs to no entry (the JSON name, a set that matches nothing) is listed in a **General** box at the top.
- **Next error** and **Prev** (F8 and Shift+F8) jump to the next or previous entry with an error, unfold it and scroll to it. They wrap at the end.
- The search box keeps the entries whose set name or any path in them (match, slot, `path_contains`, `nif_filter`) contains the text, ignoring case; "showing K of N" says how many are left. The counts and Next error ignore the search: if the next error is hidden by it, Next error clears the search. An entry shown struck through because a delete section hits it carries no count: "0 warnings" can sit above it.
- **JSON text** shows the file as it would be written now, with **Copy**. It is built again only when something it is made from changes.

Edits in the editor and in the Material tab are the same edits; each shows the other's at once, and undo is shared.
