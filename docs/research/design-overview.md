# How Tessalith came about: design overview

Written 2026-10-16 (commit 1bae037b).

Tessalith started in October 2026 as a written plan for an open-source DDS converter for Skyrim SE/AE modding, with a GUI and a CLI on one engine. The plan drew on the [encoder survey](encoder-survey.md), the [local benchmark](encoder-benchmark.md), the [mipmap research](mipmaps-and-alpha.md) and a review of Skyrim's texture conventions. It listed open questions, and each was settled by a short test ("spike") or by the project owner. This page describes the design that resulted and how it differs from the first plan. The decision record is [DECISIONS.md](../DECISIONS.md).

## The main idea: correct Skyrim policy matters more than the encoder

The benchmark showed that the top BC7 encoders are within 0.16 to 0.32 dB of each other on real colour, while the wrong format costs 5 to 15 dB and stray alpha 1 to 17 dB. Skyrim adds errors that are not gradual at all: a vanilla diffuse tagged `_SRGB` renders too dark, a landscape diffuse saved as BC1 loses its snow mask or parallax height, a BC5 normal decodes with Z = −1, and a complex-material `_m` without a full mip chain can switch its shading off.

So the design ranks quality in three layers:

1. **Correct format, colour tag and alpha handling** for each map type and target.
2. **Correct mips.**
3. **The best measured encoder at a practical speed.**

Most of the original work went into the first two. The encoders were taken from the benchmark unchanged.

## Who it is for

Texture artists converting a folder of source images (PNG, TGA, TIFF, EXR, PSD and others) to DDS. It is not a tool for "fixing" an installed game folder; DDS input is secondary. File names may suggest how a texture is treated, but the user always decides ([DECISIONS](../DECISIONS.md), "Scope clarification").

## Skyrim texture rules

These come from the Skyrim research notes, the Community Shaders (CS) source and the owner's answers.

- **Suffixes are a convention only.** The mesh's texture slot and shader type decide how a texture is read. `_s` alone has three meanings, and `_m` is plain or complex depending on its content.
- **Alpha often holds data.** The `_n` alpha is a specular mask; the landscape diffuse alpha is the snow mask (vanilla) or parallax height (CS); a complex `_m`'s alpha is height. Diffuse alpha means transparency only when the mesh says so.
- **Normal maps keep three channels.** Skyrim's `_n` stores XYZ in RGB and the shader reads Z directly. CS never rebuilds Z. A BC5 `_n` would decode with Z = −1 and lose its specular alpha. This is why the benchmark's "BC5 beats BC7 for normals" does not apply here.
- **TruePBR sets** live under `textures/pbr/`. `_rmaos` is roughness, metallic, AO and specular in RGBA. `_p` is height in red (0.5 neutral). `_s`, `_cnr`, `_f` and `_g` are subsurface/coat colour, coat normal, fuzz and emissive.
- **Complex material `_m`**: red environment mask, green gloss, blue metalness, alpha height (255 means flat). It needs a real alpha channel. A RenderDoc capture of the CS lighting shader (spike S8) confirmed the rule exactly: the shader samples the `_m` at its smallest mip and treats it as complex when that alpha is below 251/255, with a separate rule for grey and black RGB.
- **Colour space.** Vanilla stores gamma-encoded colour in UNORM files; an `_SRGB` tag makes it too dark in vanilla shaders, CS non-PBR paths and LOD. TruePBR expects colour maps tagged `_SRGB`. Data maps are always UNORM.
- **Loader limits.** SE/AE loads BC1 to BC7 and some uncompressed formats. It loads BC4 and BC5 with either the legacy `BC4U`/`BC5U` or the DX10 header, unsigned only. Mipmaps are expected everywhere except `interface/` and `sky/`.

## Targets

The first plan had three targets: Community Shaders, Complex Parallax Materials (`cpm`) and vanilla. The `cpm` target was removed before the first release ([DECISIONS](../DECISIONS.md) Note AZ). Today there are two:

| Target | What it allows |
|---|---|
| `cs` (default) | Everything: TruePBR, Advanced skin, `_SRGB` on PBR colour maps, BC6H and RGBA16F for HDR. |
| `vanilla` | Bethesda shaders, plus the complex-material `_m` for a set whose shader is complex material, and complex grass. No `_SRGB`, no BC6H, no PBR. |

See [Presets and targets](../wiki/Presets-and-Targets.md).

## Language and architecture

The plan compared three shapes (one language for engine, CLI and GUI; a CLI engine driven by a GUI over JSON Lines; a native core DLL with a GUI in another language) and three languages (Rust, C++20, C#/.NET).

**Chosen: one Rust workspace**, Windows x64 only. The reasons in the plan were one image-I/O code path, memory-safe parsing of files downloaded from mod sites, GPU decoding of BC textures for the preview, preset types that generate their own JSON Schema, and a single-file exe. C++20 was a close second. The owner confirmed Rust after spikes S3 and S4.

| Crate | Role |
|---|---|
| `engine` | Planning, policy, presets, mips, DDS writer, validation |
| `encoders-sys` | Vendored bc7e, rgbcx and ISPC objects with a small C++ shim |
| `cli` | `sdc`, the command-line tool |
| `gui` | `tessalith.exe`, built on eframe/egui with wgpu |
| `bench` | `sdc-bench`, the benchmark gates |
| `nif`, `esp`, `vfs`, `modscan`, `pgpsim`, `render3d` | Added later: mod scanning (meshes, plugins, BSA archives) and the 3D preview |

The engine reports progress as typed events. `sdc --json` prints the same events as JSON Lines, so the GUI and the CLI cannot drift apart.

The first plan listed mesh (NIF) parsing and BSA reading as non-goals. Both were added later, so that a mod scan can tell Tessalith what a texture's alpha means and which shader its set uses ([Mod scanning](../wiki/Mod-Scanning.md)).

## Encoders

| Component | Role | How it is used |
|---|---|---|
| bc7e (from bc7enc_rdo) | BC7 | Prebuilt ISPC objects, vendored with SHA-256 hashes, so building needs no ISPC compiler. One local patch adds exact-opaque alpha. |
| rgbcx (from bc7enc_rdo) | BC1, BC3, BC4, BC5 | Plain C++. One local patch makes its BC4 palette round like the decoder. |
| Intel ISPC TC | BC6H | Prebuilt ISPC objects. |
| bcdec_rs | Decoding for validation and preview | Rust crate. |
| NVTT 3 | Optional BC7 and BC6H at Production | The user's own `nvcompress.exe`, run as a separate process. Never linked or shipped, because its licence forbids use that would place it under an open-source licence. |
| DirectXTex / texconv | Test oracle only | Not shipped. |
| Compressonator, Oodle | Not used | |

Spike S4 tested a scalar C++ port of bc7e as a fallback. It was 3.8 to 4.8× slower and missed the per-image tolerance on 1 of 11 images, so it was not adopted. The vendored files and patches are listed in [`README-VENDOR.md`](../../crates/encoders-sys/vendor/README-VENDOR.md).

## The conversion pipeline

The research fixed the order of the steps. A batch goes through these stages:

| Stage | What happens | Why it is here |
|---|---|---|
| Discover, Group | Find files, take the game-relative path, group files into material sets | Paths decide colour tags; sets decide packing |
| Load, Analyse | Decode to float32 RGBA; measure channels, alpha, normal length | 16-bit and HDR sources are not cut to 8 bits early |
| Resolve | Presets, rules, suggestions and user choices become one fixed job per file; `--dry-run` stops here | GUI and CLI share one settings model |
| Preprocess | Alpha policy, channel swizzles, green flip, colour fill under alpha 0, resize, sRGB decode for colour | Stray alpha must go before mips and encoding |
| Mipmaps | Filter per channel group, renormalise normals, bake roughness, preserve coverage | Coverage comes after filtering and before encoding |
| Compress | Encode each level in parallel bands; apply the alpha fixes | The alpha-254 values come from the encoder |
| Validate, Write | Decode what was encoded, check headers, mips, alpha rules and errors; write to a temporary file and rename | The smallest mip's alpha only shows its real value after encoding |

Every output is written atomically, and a batch manifest records how each file was made, so `--on-existing if-changed` can skip files that are up to date.

## How the open questions were settled

| Question in the plan | Answer | Where |
|---|---|---|
| Two-channel maps beyond `_flow` | Only `_flow` by default; BC5 stays selectable | DECISIONS #1 |
| BC4/BC5 header form | SE loads both; unsigned DX10 by default, legacy as an option, `ATI1`/`ATI2` never written | DECISIONS #2, [`dds.rs`](../../crates/engine/src/dds.rs) |
| BC6H for emissive and sky | Usable under CS, not in vanilla | DECISIONS #3 |
| `_SRGB` in vanilla | Not usable | DECISIONS #4 |
| `_g` and `_f` colour space | Any colour data in a PBR set is `_SRGB` | DECISIONS #5, Spike S1 |
| Normal length | Renormalise; bake variance into roughness when the set has an `_rmaos` or `_rfaos` | DECISIONS #6, M4 rulings |
| Small BC7 textures crashing | 66 BC7 textures of 16 to 128 px with full chains ran in game with no crash; no fallback | Spike S2 |
| Forcing alpha 255 | Remove the alpha channel by default; exact-opaque BC7 only for `_m` and environment masks, because it costs about 0.5 to 1 dB RGB | Notes A and D, Spikes S5 and S6 |
| Coverage threshold | Default 127, set per texture; coverage on for cutout and cutout + blend | DECISIONS #10, Note E |
| BC1 decoder model | One batch-wide setting, default Ideal | Note F |
| BC1 transparent black | BC1 is opaque only; the 3-colour mode never writes transparent texels | Note F |
| Linux | Windows only | DECISIONS #12 |
| Coverage implementation | Rebuild the coverage idea in the engine, with credits ([CREDITS](../CREDITS.md)) | DECISIONS #13 |
| Rust or C++ | Rust | Phase 0 exit |
| Hash in the DDS header | Not done; a batch manifest records provenance, per-file sidecars are optional | DECISIONS #15, Note F |
| 16K previews | Default limit 8192, 16384 as a setting | DECISIONS #19 |
| Driving NVTT per mip level | Works: one `-nomips -nocuda` process per level, fed as a legacy-header DDS | Spikes S7 and S7b, Note NV |
| CS complex-material gate | Exact at 251/255 on the smallest mip; Tessalith warns at 247 | Spike S8 |

## Other changes since the plan

- **Plain `_normal` files are assumed DirectX**, with an info message, instead of blocking until the user states the convention ([DECISIONS](../DECISIONS.md) Note AW).
- **Version 0.8.0** is the first public alpha ([DECISIONS](../DECISIONS.md) Note AX).
- **Channel packing, RNM normal layers, complex grass, PGPatcher JSON export and a 3D preview** were added after the plan. See the [wiki](../wiki/Home.md).
