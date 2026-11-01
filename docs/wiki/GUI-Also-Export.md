# Also export as Vanilla and CM

How to switch on, set up and read the result of an export that also writes a vanilla and a complex-material (CM) version of a PBR mod. For anyone shipping one mod in PBR, vanilla and CM flavours. What each folder holds, how each map is converted and how to install them are in [Also export as Vanilla and CM](Set-Shaders.md#also-export-as-vanilla-and-cm); the recipe values are in [Project config](Project-Config.md#export).

## Also export as Vanilla and CM

It needs the `cs` mode: the PBR folder is the cs export.

- **Where to switch it on.** **Export to ▾** (**More ▾** until a folder is chosen) has two checkable items, **Also export as Vanilla** and **Also export as Complex material**; the menu stays open on a tick, and there is no dialog. Without a folder open they are greyed out with "Open a folder first". Review has an **Also export as** column with the same two ticks. The column shows in the `cs` mode or while a tick is on; its red reason line (why Export cannot run yet) shows only while a tick is on. The ticks are saved as `[export] also` in `sdc.toml`.
- **The three folders.** While a tick is on, Export always opens Review, and Review's Export writes `<project> PBR` (the plain cs export, byte for byte, with the PGPatcher, CS PBR and complex-material JSON), `<project> Vanilla` and `<project> CM` side by side, where `<project>` is the opened folder's name. Ticking only CM writes the Vanilla folder too: CM installs on top of it. Files rows of the Vanilla and CM folders start with the folder name.
- **Review's column** holds the **Export location** (**Choose…**; per project, `[project] also_location` in `sdc-project.toml`, by default the export folder), the three folder names (empty means the default name, shown as the hint), **Keep metal brightness** and its **Strength** (CM only), **Also write a zip of each folder** and **Skip these sets**.
- **Copy CLI command** then passes the location as `-o`. **Open export folder**, the header's export path, the DDS header patch and the JSON editor stay on the export folder; by default the location is that folder, so it shows the three folders.
- **The env `_m` needs a scan.** It is written only for sets whose meshes use the environment-map shader, which Tessalith learns from the Meshes scan. Without a scan no env `_m` is written and the report says "no: no scan". The command line reads no scan, so it never writes one.
- **NIF edits follow the PBR folder** while a tick is on, so one mod folder holds the textures and the meshes, as a plain export does.

## Skipping a set

A PBR set can be left out of Vanilla and CM: tick it in Review's **Skip these sets**, or, in 2D, use **Skip vanilla/CM for this set** in the set editor's Material tab (its **Also export** group, shown while a tick is on, reads "converted" or "skipped"). Skipped sets are `[export] skip` in `sdc.toml`.

A PBR set with a map still undecided (or blocked by a code you have not acknowledged) is not exported to the PBR folder, so Vanilla and CM leave it out too; the report and the export notes say "not derived: N maps undecided in the PBR export".

## The recipe

**Also export recipe** in the [Advanced drawer](GUI-Advanced-Drawer.md#advanced-drawer) (it starts closed; Review's **Recipe settings are in Advanced** opens it) has one box per `[export.legacy]` value: diffuse brightness, AO strength (objects and landscape), curves opacity, metal darkening, specular scale, specular boost at full metal, landscape spec factor and dielectric env.

`_curves`, `_curvature` and `_cavity` maps join their PBR set and are read only by Also export ([Set-Shaders](Set-Shaders.md#also-export-as-vanilla-and-cm)). [Generate from normal map](GUI-Generate-From-Normal.md#curvature) can make a curvature map.

## Report and zips

Every Also export (not a dry run) writes `<project> conversion-report.md` next to the three folders: the counts per folder, the recipe settings, a row per PBR set (suggested NIF Glossiness, Specular Strength, whether the Environment Map shader is needed and the env `_m` was written), the sets not converted and why, and the install lines.

- With the zip on, each folder that ran becomes `<folder>.zip`, with a line per zip in `<project> SHA256SUMS.txt`.
- A zip the ZIP format cannot hold (65,535 files or more, or 4 GiB, counted from the planned outputs' sizes) is a Review blocker before anything is written: "Zip too large for the ZIP format (n files, size); untick "Also write a zip" or split the export".
- The zips and the report are written after the PGPatcher, CS PBR and complex-material JSON, so the PBR zip holds the JSON this export wrote. The export stays running (and can be cancelled) until they are done.

Files Tessalith did not write are never overwritten, and Tessalith never deletes either. A file in one of the folders that this export did not write (an earlier export's CM diffuse after Keep metal brightness was turned off, the outputs of a set now skipped) stays, and the report and the export notes list it as "not written by this export" (with the zip on, "not zipped"). Delete such files yourself: a stale CM file overrides the Vanilla file of the same name.

To start from scratch, delete the three folders, the report, the zips and the sums file together. Their record is in the PBR folder (`_sdc/also.json`), so deleting only the PBR folder leaves the report, zips and sums kept as files Tessalith did not write.
