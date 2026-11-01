# Artist checks

Six checks that read the numbers Tessalith already measures while it plans, and tell you about maps that will probably look wrong in game. For anyone reviewing a texture set before release. What each check measures, and the neutral heights the parallax check uses, are in [Height maps and the artist checks](Set-Shaders.md#height-maps-and-the-artist-checks).

## Artist checks

The checks only warn. No check blocks or cancels an export, none asks you anything, and none changes "ready": no amber dot on a tile, slot row or set, no "Not ready", and the header **Export** stays as it was. Each has its own switch in **Advanced → Checks** (all on by default, saved in `gui.toml`), and a switch hides its results at once, with no re-plan.

| Check (switch) | What it flags |
|---|---|
| **Value range** | A PBR albedo that is too dark or too bright, a metal that is not black or white, an `_rmaos` with mostly extreme roughness or a dark AO channel, metal that is darker than metal should be, an emissive map that is mostly bright background. Code `CHK-RANGE`. |
| **Plastic look** | A vanilla `_n` with no alpha, or an alpha that is almost all white: the specular is then flat and full. A soft warning, because mesh values vary. Code `CHK-PLASTIC`. |
| **Low detail** | A texture whose top mip adds almost nothing over mip 1 (an upscale, or a 2K made from a 1K). Measured during Export. Code `CHK-LOW-DETAIL`. See [Half size](#half-size). |
| **Green direction** | A normal map that looks like OpenGL when it is read as DirectX (`CHK-GREEN-GUESS`, an Info result, never an auto-flip) and a normal whose blue channel points the wrong way (`CHK-Z-SIGN`). |
| **Parallax** | A height map (`_p`) or a complex `_m` alpha that uses too little range, clips, is mostly white, or is steep enough to tear; landscape sets whose displacement scales differ by more than 2×. Codes `CHK-PARALLAX` and `CHK-PARALLAX-SCALE`. |
| **UV coverage** | UV islands that sit closer than 8 texels to each other for more than 2 % of the used texels, so colours bleed across them at a distance (mip 3 and up). Code `CHK-UV-GUTTER`. Needs a Meshes scan; see [below](#check-all-uv-coverage). |

**When they run.** Checks are computed from the analysis cache when a plan lands, so they cost no extra load. The set-level ones (dark metal, landscape scale) use the 128-pixel thumbnails of the set's members.

## Where the results show

- **Problems rows.** Each warning is a row in the [Problems](GUI-Problems.md) tab, after the ordinary warnings, with a grey **check** dot. The tab shows a **Checks** pill while any exist; click it to narrow the list to check rows (click again to see everything). With a set filter on, it narrows to that set's check rows and keeps the set.
- **The ⓘ badge.** A set card shows **ⓘ n** beside its **⚠ n**, in the weak text colour: *n* counts the set's Warning-level results (a plain **ⓘ** means only Info results). Hover it for the list; click it to open Problems on that set.
- **The green-direction guess** is a check row too, with **Treat as OpenGL** as its button (it flips green on that texture, one undo step). It also shows on the badge's hover and in the texture's own settings panel.
- **Never in Review or in the Files rows.** A check never opens Review and is not copied into an export's file rows, so it does not show twice.
- **The end-of-export summary.** When an export ends, one Info line (a toast and a Messages entry) counts the check rows, for example *Checks: 3 warnings on 1 texture and 1 set; see Problems*. It is not a dialog, so an open dialog stays.

## Preview overlays

**Range.** The preview toolbar's **Range** toggle (after *Tile 3×3*) paints the texels that break the value-range rule in magenta over the Source side, at the mip on show.

- It is drawn for every texture, as **UV coverage** is, and works only when the open texture has a per-texel `CHK-RANGE` warning (albedo luma, metal grey, extreme roughness or emissive background). The dark-metal and AO-dark warnings are not per-texel, so they leave it greyed out, and the hover says why. It is also greyed out with "Hidden while comparing" while a pin is compared.
- It is not saved, it is not drawn in the lens, and it is built in the background, so a large texture never stalls the window. With **Tile 3×3** the overlay repeats with the picture.

**UV coverage.** The **UV coverage** toggle (after *Range*) darkens every texel that no mesh using the texture covers and leaves the used ones clear, over the Source side (v = 0 at the top, as in the DDS). Hover it for the used share, the number of meshes and the gutter share. When the gutter cannot be measured (4096 pixels or wider on either axis, or until the written size is known) the hover says so and raises no warning.

### Check all UV coverage

UV coverage is read only on request. Turn on the preview's **UV coverage** toggle for one texture, or press **Check all UV coverage** in Advanced → Checks for every colour texture of the open folder; a bar shows *done / total*, and **Stop checking UV coverage** ends it.

It reads the Meshes scan: scan your MO2 instance in the [Meshes view](Mod-Scanning.md) first. The toggle is greyed out, with the reason:

- while no scan has finished (an empty scan counts as none);
- while a scan is running ("Wait for the Meshes scan to finish") or was stopped ("The Meshes scan was stopped: scan again to check UV coverage");
- for any sub-type but diffuse, PBR albedo and skin albedo;
- while a pin is compared.

Landscape diffuses are left out, because terrain does not use NIF shapes. Results are kept for the session and cleared when a scan finishes.

## Half size

With **Low detail** on, **Measure whether full size adds anything** (in Advanced → Checks, greyed until Low detail is on) measures every texture on each export, up-to-date ones too. Export always writes the full texture; nothing is held back. Review then asks about the textures the last export flagged (hover: mip 0 adds little detail over mip 1, `CHK-LOW-DETAIL`).

- Review's Undecided column lists them under **N textures added nothing at full size last export**, one row per texture with a menu: **Half size** or **Full size** (an unanswered row reads *Half or full size? ▾*). The rows count in *Undecided (n)*, and Export waits with "Answer half or full size for N textures" until each has an answer. When `sdc.toml` is read-only, **Half size** is greyed (hover: the half-size choice cannot be saved) and **Full size** still works.
- Review's Export saves `drop_top_mip = true` on each texture answered **Half size** (one undo step, *Half size*), waits for the new plan, and exports once: those textures are written from mip 1. **Full size** writes nothing to `sdc.toml`. **Close** and **Esc** write nothing; if the export is refused after the half size was saved (or Review is closed before it starts), that step is undone. There is no dialog after the export.
- An answer is for that export only: a texture answered Full size is measured and asked about again next time. A texture the plan no longer flags (resized, already at half size), or whose source file changed since that export, leaves the list and gets nothing written.
- The first export of a texture always writes it at full size, because nothing has been measured yet.
- Without the switch, Low detail only warns in Problems, and Review asks nothing.
- **Advanced → Half size** sets or clears the same per-file field by hand (a tick box with ↺; greyed with "No mips to drop" when the texture has no mip chain). The CLI honours the saved field ([`drop_top_mip`](Set-Shaders.md#height-maps-and-the-artist-checks)), so both write the same bytes. A half size saved on the source texture is a tick in the *Copy settings* list (*Half size: true*), like any other field.

## Gloss bake for a complex `_m`

On a `_n` in a set with a complex-material `_m`, setting its *Normal handling* to **Renormalise and bake** also bakes the normal's variance into that `_m`'s gloss, to cut shimmer at a distance. It is off by default; see [Gloss bake for a complex `_m`](Set-Shaders.md#gloss-bake-for-a-complex-_m-opt-in).
