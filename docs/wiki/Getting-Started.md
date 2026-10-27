# Getting started

This page takes one small texture set from source images to finished DDS files, first with the `sdc` command line and then with the Tessalith GUI (`tessalith.exe`). It assumes you have unzipped the release (or built both programs; see [Installation](Installation.md)) and that `sdc` is on your `PATH`.

## The example project

You are making a TruePBR iron plate for Community Shaders. Your source folder looks like this:

```text
C:\Work\IronPlate\
  textures\pbr\armor\ironplate\
    ironplate.png             colour, no alpha channel
    ironplate_n.png           normal map, DirectX green (Skyrim's convention)
    ironplate_roughness.png   grayscale
    ironplate_metallic.png    grayscale
    ironplate_ao.png          grayscale
```

Things to notice:

- The folder path contains `textures`. Tessalith takes the game path of each file from the last `textures` folder in its path, so `textures\pbr\armor\ironplate\ironplate.png` has the game path `textures/pbr/armor/ironplate/ironplate.png`. Rules and sub-type detection use that game path.
- All five files share the base name `ironplate`, so they form one texture set.
- The raw `_roughness`, `_metallic` and `_ao` maps are packing inputs. Under the `cs` target they are packed into a single `ironplate_rmaos.dds` (R roughness, G metallic, B AO, A specular). Under `cs`, any of these makes the set a PBR set: a raw roughness, glossiness, metallic or AO map; an `_rmaos`, `_rfaos`, `_cnr` or `_f` file; or a `pbr` folder in the path.
- The output tree mirrors the input tree under the output folder, with `.dds` extensions.

See [Texture types](Texture-Types.md) for the full naming rules.

## Part 1: the command line

### Step 1. Look before you convert

`sdc inspect` runs the whole planning stage and prints what would happen, without encoding or writing anything:

```powershell
cd C:\Work\IronPlate
sdc inspect C:\Work\IronPlate --recursive --accept-suggestions high
```

`--recursive` is needed because the images sit several folders down. Without it, Tessalith reads only the files directly inside the folder you name.

`--accept-suggestions high` tells Tessalith to accept its High-confidence guesses about what each file is. On the command line the default is `none`, which leaves every file whose sub-type came from a suggestion (rather than from a rule, a `--set`, or a `[files]` entry) unresolved. The GUI accepts High-confidence suggestions automatically, so this flag makes the CLI behave the same way.

The output has one block per file and one per set. Abridged, it looks like this:

```text
set textures/pbr/armor/ironplate/ironplate  shader pbr (auto:pbr)
  pack textures/pbr/armor/ironplate/ironplate_rmaos.dds (rmaos) run
    R source:ironplate_roughness.png:R  G source:ironplate_metallic.png:R  B source:ironplate_ao.png:R  A default:1
C:\Work\IronPlate\textures\pbr\armor\ironplate\ironplate.png
  suggested pbr_albedo (high confidence: colour member of a PBR set)
  sub-type pbr_albedo  format bc7  tag srgb  alpha remove  output textures/pbr/armor/ironplate/ironplate.dds
  status ready
C:\Work\IronPlate\textures\pbr\armor\ironplate\ironplate_n.png
  suggested pbr_normal (high confidence: _n in a PBR set)
  sub-type pbr_normal  format bc7  tag unorm  alpha remove  output textures/pbr/armor/ironplate/ironplate_n.dds
  status ready
...
```

What to check:

| Line | Meaning |
|---|---|
| `set ... shader pbr (auto:pbr)` | The set's shader, and why it was chosen. `auto:<rule>` means Tessalith picked it; `set` means you did; `fallback` means the shader you asked for is not available under this target. |
| `pack ... run` | A packed output that will be built. `conflict`, `failed` or `disabled` means it will not, and the reason follows. |
| `R source:... A default:1` | Where each channel comes from. `source:<file>:R` reads the file's red channel (`:luma` when `channels.grayscale_source = "luma"`, and a `1-` prefix means inverted, as when gloss stands in for roughness). `default:` means no source map was found and the neutral default (on a 0 to 1 scale) is used; `fill:` means you set an 8-bit value. |
| `status ready` | The file will be converted. `unresolved`, `blocked` or `error` means it will not, and the reasons follow on the same line. `packing_input` (a raw map waiting to be packed) and `normal_layer` (an RNM layer) are not converted on their own either; they feed another output. |
| Lines starting with `warning` or `blocking` | Diagnostic codes. See [Troubleshooting](Troubleshooting.md). |

If a file shows `status unresolved`, the reason says what is missing: usually a sub-type that is only a Medium or Low confidence guess or the meaning of a varying alpha channel. Fix it with one of the methods in Step 2.

### Step 2. Record decisions in sdc.toml

Anything you would otherwise type as a `--set` flag every time belongs in a project config. Create `C:\Work\IronPlate\sdc.toml`:

```toml
schema_version = 1
target = "cs"

# This set has no specular map. Use 0.5 (128) instead of the default 1.0.
[files."textures/pbr/armor/ironplate/ironplate_rmaos"]
packing.fill.specular = 128
```

`sdc convert` and `sdc inspect` read `sdc.toml` from the first input folder automatically. `[files]` keys are paths relative to the folder holding `sdc.toml`; a packed output is keyed without an extension. See [Project config](Project-Config.md) for every key.

Here is how you would answer the usual open questions in the same file:

```toml
# A colour map whose alpha is a cutout mask
[files."textures/pbr/armor/ironplate/ironplate_trim.png"]
alpha.policy = "cutout"
alpha.threshold = 127

# A normal map saved as _normal, exported from an OpenGL tool
[files."textures/pbr/armor/ironplate/ironplate_normal.png"]
channels.green_convention = "opengl"

# A file whose name gives no clue
[files."textures/pbr/armor/ironplate/ironplate_mask.png"]
subtype = "generic_data"
```

### Step 3. Convert

```powershell
sdc convert C:\Work\IronPlate --recursive --accept-suggestions high -o C:\Work\IronPlate-out
```

Progress goes to stderr, one line per file. Paths in the log use forward slashes:

```text
ok    C:/Work/IronPlate/textures/pbr/armor/ironplate/ironplate.png -> C:/Work/IronPlate-out/textures/pbr/armor/ironplate/ironplate.dds
ok    C:/Work/IronPlate/textures/pbr/armor/ironplate/ironplate_n.png -> C:/Work/IronPlate-out/textures/pbr/armor/ironplate/ironplate_n.dds
ok    ... -> C:/Work/IronPlate-out/textures/pbr/armor/ironplate/ironplate_rmaos.dds
done: 3 converted, 0 up to date, 0 skipped (0 on purpose), 0 failed, 0 failed validation, 0 cancelled
```

The three raw maps do not get lines of their own: they were consumed by the packed output and are not written separately, and they are not counted as skipped.

Without `-o`, output goes to `.\sdc-out` in the current directory. The output folder is never read back as input, so it is safe to put it inside the source folder.

Check the exit code:

```powershell
$LASTEXITCODE
```

`0` means every file converted and passed validation. See [CLI reference](CLI-Reference.md#exit-codes) for the others.

### Step 4. Run it again

```powershell
sdc convert C:\Work\IronPlate --recursive --accept-suggestions high -o C:\Work\IronPlate-out
```

```text
skip  C:/Work/IronPlate/textures/pbr/armor/ironplate/ironplate.png: up to date
skip  C:/Work/IronPlate/textures/pbr/armor/ironplate/ironplate_n.png: up to date
skip  ...: up to date
done: 0 converted, 3 up to date, 0 skipped (0 on purpose), 0 failed, 0 failed validation, 0 cancelled
```

The default `--on-existing if-changed` compares each output with its record in `C:\Work\IronPlate-out\_sdc\manifest.jsonl` and re-encodes only what changed: the source file, the resolved settings, the texture type's encode version or the encoder version, or the output file itself. Edit `ironplate_roughness.png` and run again, and only the `_rmaos` is rebuilt. See [Validation and output](Validation-and-Output.md).

### Step 5. Verify a DDS

```powershell
sdc validate C:\Work\IronPlate-out\textures\pbr\armor\ironplate\ironplate.dds --subtype pbr_albedo
sdc inspect  C:\Work\IronPlate-out\textures\pbr\armor\ironplate\ironplate_rmaos.dds
```

`validate` checks the header, format and tag against the target's rules for that sub-type. `inspect` on a DDS prints its header and an alpha report for every mip level.

### Step 6. Install

Copy the contents of the output folder (everything except `_sdc`) into your mod, so the files land under `Data\textures\...`. A TruePBR set also needs a PGPatcher JSON that tells PGPatcher which meshes use it. The CLI does not write that JSON; the GUI does (Part 2).

## Part 2: the GUI

The GUI runs the same engine and reads and writes the same `sdc.toml`, so you can switch between the two at any point.

1. Start `tessalith.exe`. The start screen has one large **Open folder** button (and your recent folders below it). Click it and pick `C:\Work\IronPlate`, or drag the folder onto the window. The GUI reads the folder recursively, analyses every image and plans the conversion. Nothing is written yet.
2. Check **Mode** in the header. It should read `cs`. Changing it writes `target` to `sdc.toml`.
3. The Textures column on the left shows a tile per file. The three raw maps show "Packed into ..." because they feed the `_rmaos`. A tile with an amber border needs a decision. Click the **Needs decision** pill above the tiles to see only those (**Filter ▾** has the other filters and the sort).
4. Click a tile. The centre column shows its settings in groups (Type, Colour, Alpha, Mipmaps, Compression), and each value says where it came from. A sub-type the GUI accepted reads **(auto)**; hover it to see why.
5. For an amber tile, answer the question shown in the centre column: the sub-type buttons, the alpha buttons (*Remove*, *Cutout*, *Blend*, *Cutout + Blend*, *Data*), *DirectX* or *OpenGL* for a normal map's green channel, or **Acknowledge for this texture** for a blocking warning. Each answer is written to `sdc.toml` as a `[files]` entry.
6. Click the `ironplate` set in the Sets column on the right. Its card shows the set's status, its shader and a lettered chip per slot. The set editor shows a row for each slot (Diffuse, Normal, RMAOS and so on) and the set's **Shader**, which should read `pbr` (auto). On the RMAOS card you can set fills for missing channels; for this example set Specular to 128 to match the `sdc.toml` from Part 1.
7. Fill in the TruePBR settings for the set (Material, Features and the rest) if you want values other than the defaults. Leave **Matching** on the diffuse path.
8. Check **Export to** in the header (its menu has **Choose export folder…**). By default it is `sdc-out` inside the opened folder. Each project remembers its own export folder. Set the **JSON:** name; by default it is the folder name.
9. Press **Export**. The GUI saves `sdc.toml` and `sdc-project.toml` first, then converts. If something is blocked, undecided or worth a second look, the Review tab opens at the bottom first; fix or tick what it lists and press its Export. Then the Files tab shows each file, and the Problems tab lists the warnings.
10. When the export finishes, the GUI writes `<export folder>\PBRNIFPatcher\<name>.json` for every PBR set that exported fully.

To get an equivalent command line, press **Copy CLI command** in the Files tab at the bottom. The copied command already has `--accept-suggestions high`, because the GUI accepts High-confidence suggestions and the CLI does not by default. Add `--nvtt <path>` if you use NVTT. Set edits made in the Sets column live in `sdc-project.toml`, which the CLI does not read; see [GUI guide](GUI-Guide.md#export).

## Where to go next

- [GUI guide](GUI-Guide.md) for bulk editing, renaming, the 3D preview and the TruePBR JSON editor.
- [Mod scanning](Mod-Scanning.md) to let the GUI learn alpha modes and shaders from the meshes that use your textures.
- [Presets and targets](Presets-and-Targets.md) to switch to Vanilla output (with complex material `_m` maps if you want them), or to smaller BC1 files.
