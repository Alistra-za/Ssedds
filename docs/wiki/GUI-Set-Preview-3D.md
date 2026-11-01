# The 3D preview

The 3D view of a set on a mesh, rendered with Direct3D 11 and your own Community Shaders shader files. The Meshes view's 3D panel works the same way. For anyone checking a material in something close to the game before exporting.

## Set preview (3D)

The view does no work while it is hidden.

**Layout.** The panel has one toolbar line, the frame in all the height left, and a **Details** footer under it, collapsed until you open it (each panel remembers it in `gui.toml`).

The toolbar holds, for a set, *Encoded (exact export)* | *Source (faster, no compression)* and the **Mesh** list, then for both panels **View ▾**, **Copy image** and **Shader folder…**:

| Control | What it does |
|---|---|
| *Encoded (exact export)* / *Source (faster, no compression)* | Shows the bytes Export would write, or the prepared source without compression. |
| **Mesh** | *Auto* ("Mesh: Auto (Sphere)") or a built-in shape: Sphere, Plane, Cube, Material ball. The scanned meshes that use the diffuse are listed loose files before BSA ones, with ground models (a `gnd` word in the name, or a name ending `gnd` or `_go`) and `1stperson` meshes last, so *Auto* takes a loose, normal mesh when there is one. |
| **Material object layer** | Shown for a set with a material object block (CS PBR records). On, the preview draws the snow/ash layer the game puts on statics that use the object, from the block's values and the DATA of the object's record in the load order (the first of the block's EDIDs that the scan knows, read in the background). Coverage goes on non-skinned shapes that are not parallax (shader type 3), multi-layer parallax (type 11 or the Multi_Layer_Parallax flag), FaceGen (types 4 and 5), sparkle snow (type 14), eye (type 16) or model-space-normal shapes; the block's material values (colour scale, roughness, specular, glint) change PBR shapes only. Skinned shapes and the other shapes listed draw as before, without the layer. Grey lines under the toolbar say what the preview had to guess: no record in the load order (it then shows default coverage), non-PBR shapes in the mesh, and each of the four layer textures (textures\effects\projecteddiffuse.dds, projectednormal.dds, projectednormaldetail.dds, projectednoise.dds) the load order lacks (a plain fallback is drawn). Off at every start. A rescan that changes the winning record shows at once; changed layer textures, or new DATA under the same record, show after you turn the layer off and on. |
| **View ▾** | The debug view, **Accurate (deferred)**, *Background* (*Solid* with a colour swatch and hex box, any colour; or *Studio HDRI*, Poly Haven, CC0), *Sun brightness*, *Point light brightness* and *Point light radius*, and **Reset view**. It stays open while you use it. |
| **Copy image** | Copies the last frame to the clipboard (also **Ctrl+Shift+C**), at the size it was rendered; the panel says "Copied 3D view (W×H)". |
| **Shader folder…** | Points at your Community Shaders mod folder or its `Shaders` folder when it is not found automatically ("Community Shaders not found", **Choose folder…**). |

What needs attention shows as a line between the toolbar and the frame, never in Details: a map that failed, is stale or is blocked, a skipped shape or a patch badge, a shader that failed, an error, a missing texture (in magenta, with *Open in Textures*), "PGPatcher preview needs the CS target and a PBR set", Community Shaders not found, and the notices.

Details holds the rest: "Assumes your Tessalith JSON wins…", the preview size, the mesh line ("No scanned mesh uses this diffuse"), each map's encode time, the built-in shape line, the emissive note, Timings, the **Community Shaders features** toggle (on by default; off compiles the shaders without the CS feature defines), "Off in this preview: …" and the frame stats.

## A set in 3D

The **2D | 3D** switch at the left of a set's heading row (next to its input name box) changes the centre pane in place. *2D* (the default) shows the [set editor](GUI-Set-Editor.md). *3D* shows the set's 3D view across the whole centre instead; the heading row stays above it. The switch is remembered in `gui.toml`, and the 3D view does no work while 2D is showing.

**TruePBR settings…** in the heading row opens a window you can move and resize, with the set's settings: the TruePBR settings (the coat settings under Multilayer, NiAlphaProperty and the rest), or for an Advanced skin set the skin card, plus the PBR Landscape card or the CS PBR records (Community Shaders target) and the complex material card when the set has them. An edit there shows in 3D right away. Whether it is open, and where, is remembered in `gui.toml`.

While the switch is on 3D, selecting any texture of a set (from the set or straight from the Textures column) shows that set's 3D view, and the window (titled *<name> settings*) shows the texture's own settings above the set's. The window follows the selection, and closes when the new selection has none of these settings. A texture in no set shows its normal settings; the switch stays on 3D.

## Moving around

- Drag to orbit, scroll to zoom. Middle-drag, or Shift+drag, pans: the model follows the cursor, and the further away the camera is, the more a drag moves it. Double-click or **Home** recentres the camera.
- **Turning the sun.** Shift+right-drag turns the sun around the model (about the vertical). The studio HDRI background and its light turn with it. The turn is part of the View controls, so it is remembered with them.
- **Reset view** (in *View ▾*) puts the lights, the background and the camera back to their defaults (not *Accurate (deferred)*).

## View options

- **Debug view** (in *View ▾*, or **V** to step through): *Lit* is the normal Community Shaders render. *UV checker* puts a grey checker in every shape's diffuse (base colour) slot, so stretched, mirrored or seamed UVs show; the rest of the material still draws. *Albedo*, *Normals*, *Roughness*, *Metallic* and *AO* show one channel of the material. They are offered only when the Community Shaders build in use allows it and a loaded shape has that channel; where a shape has only a stand-in (non-PBR roughness as 1 − specular mask, vertex AO), the hover names it.
- **Accurate (deferred)** (in *View ▾*, on by default): opaque shapes draw through Community Shaders' own deferred pass and composite, as in game; blended shapes still draw on top the forward way. Off uses the older forward path. If your Community Shaders build cannot run it, the view stays forward and a notice says why. The setting is remembered.
- **Missing textures** show in solid magenta in the frame (colour slots only; a missing normal, glow, specular or RMAOS map draws neutral), and are listed above it in magenta. A path that is a texture of the open folder has an *Open in Textures* button that selects it in the Textures view. A missing cube map (an environment map) keeps the renderer's plain stand-in instead, because a flat colour cannot go in a cube slot. In a NIF edit's **Source** view, a path that no mod ships but that PGPatcher's JSON replaces at run time (a PBR mod whose NIF still names the old path) is not listed as missing: one grey line says "1 texture path is replaced by PGPatcher at run time: switch to In game to see them", with the paths on hover; the frame still draws those slots in magenta. A path nothing supplies is still listed as missing.

## What it assumes

- The PGPatcher simulation needs the `cs` target and a PBR set. Other sets show their plain textures.
- To preview on a real mesh, scan your mods first ([Mod scanning](Mod-Scanning.md)). "No scanned mesh uses this diffuse" means the scan found none. Meshes from Skyrim LE are not drawn; a built-in shape is shown instead. A built-in shape cannot match an entry's match keys, so it shows the plain textures.
- The preview assumes your JSON wins over other mods' PGPatcher configs.
- Large textures are previewed at a capped size ("Preview at N px, export writes full size").
- These Community Shaders features are off in the preview: Light Limit Fix, Skylighting, Screen-Space Shadows, Subsurface Scattering, Image-Based Lighting, Physical Sky, Wetness, Cloud Shadows, terrain effects, water, fog, Volumetric Shadows.
- A timing log is written while the preview runs. Set the environment variable `SDC_PREVIEW_LOG=0` to turn it off.
