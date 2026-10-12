# 01 — NIF reader for sdc: texture-set extraction now, 3D geometry later

Status: research plan (no code yet). Scope: Skyrim LE (NIF 20.2.0.7, user 12, BSVER 83) and SSE/AE (BSVER 100).
Evidence tags: **[Source]** read in vendored code/spec (file:line) · **[Doc]** official/project docs · **[Claim]** web, not verified · **[Inference]** my reasoning · **[Measured]** from my prototype scan of the local game install (method in §M).

## TL;DR

1. **The block-size array is present and reliable, so a seek-only scanner works.** NIF 20.2.0.7 headers carry one `u32` size per block (in the format since 20.2.0.5). I used it to jump directly to `BSLightingShaderProperty`, `BSEffectShaderProperty`, `BSShaderTextureSet`, `NiAlphaProperty` and the shape blocks, decoding no geometry. Two checks passed on every file: (a) header + all block sizes + footer equals the file length, on 21,504 of 21,504 vanilla+CC NIFs; (b) bytes decoded equal the declared size, on all 136,536 decoded property blocks. The scanner reads about **2 %** of each file's bytes. [Measured]
2. **Field layouts at BSVER 83 and 100 are identical** for every block the scanner needs. Shader Type is the **first u32 of a BSLightingShaderProperty block**, ahead of its Name. BSShaderTextureSet always had **9** slots (48,825 of 48,825 blocks), and slot index 8 was never used. **BSEffectShaderProperty has only two inline strings in Skyrim** (Source and Greyscale). The env, normal and env-mask strings exist only from BSVER 130 (FO4). [Source][Measured]
3. **How shapes link to shaders:** at BSVER > 34, every shape (NiTriShape/NiTriStrips/BSLODTriShape/BSSegmentedTriShape, BSTriShape and its subclasses, and NiParticleSystem/BSStripParticleSystem) has two dedicated `Shader Property` and `Alpha Property` refs. **Skyrim has no `Properties` array.** Particle systems must be scanned too: adding NiParticleSystem/BSStripParticleSystem (1,803 shapes) raised the share of effect shaders referenced by a shape from about 77 % (first 3,000-file sample: 377 of 1,608 unreferenced) to 100 % of all 67,303 shader blocks. [Source][Measured]
4. **Store paths in one canonical form:** `textures\<sub\path>.dds`, lowercase, backslash separators, no `data\` or leading `\`, cut at the first `\textures\` segment. Keep the raw string next to it. Vanilla+CC slot paths are 99.6 % already `textures\…` (mixed case, never `/`, never `data\`). The odd cases are build-machine paths, `c:\…` paths, `.tga`/`.bmp` files and control-character garbage. [Measured]
5. **Recommendation: a hand-rolled pure-Rust `nif` crate in layers, with nif.xml (GPLv3) and nifly (GPLv3) as reference.** Layer 0 is the seek-only scanner, shipped now. Layer 1 decodes geometry later and reuses layer 0's header, cursor and block-index modules. I found no usable Rust NIF crate. Binding nifly over FFI is a viable fallback but needs a C++17 shim. For BSA input, use a separate VFS module with a pure-Rust LZ4 decoder. In BSAs, LZ4 decompression dominates scan cost, not parsing.

---

## Q1. Header layout (20.2.0.7, BSVER 83/100) and the block-size array

Byte layout, all little-endian (nif.xml `Header` at nifxml-master/nif.xml:1963-1983; nifly `NiHeader::Get` at nifly-main/src/BasicTypes.cpp:640-770) [Source]:

| # | Field | Type | Notes |
|---|---|---|---|
| 1 | Header string | ASCII up to `\n` | `Gamebryo File Format, Version 20.2.0.7` |
| 2 | Version | u32 | `0x14020007` |
| 3 | Endian | u8 | 1 = little (nif.xml:1228-1231). Reject 0. |
| 4 | User Version | u32 | 12 for Skyrim (nif.xml:209-210) |
| 5 | Num Blocks | u32 | nifly caps at 1,048,576 (`NIF_BLOCK_INDEX_LIMIT`, include/BasicTypes.hpp:27) |
| 6 | BS Version | u32 | 83 = LE, 100 = SSE; 130 = FO4, 155 = F76 (nif.xml:209-213) |
| 7 | Author | ExportString (u8 len incl. NUL) | nif.xml:1889-1893 |
| 8 | Process Script | ExportString | if BSVER < 131 (nif.xml:1957) |
| 9 | Export Script | ExportString | |
| 10 | Max Filepath | ExportString | nif.xml: only if BSVER ≥ 103 (1959); nifly reads it only when ==130 (BasicTypes.cpp:706-707). Absent at 83/100 either way. |
| 11 | Num Block Types | u16 | |
| 12 | Block Types | SizedString[T] (u32 len + bytes) | |
| 13 | **Block Type Index** | u16[N] | Mask with `& 0x7FFF`: the top bit is a PhysX flag (nif.xml:300-303; NifSkope nifmodel.cpp:2117) |
| 14 | **Block Size** | u32[N] | "since 20.2.0.5" (nif.xml:1977; nifly BasicTypes.cpp:737-741) |
| 15 | Num Strings / Max String Length | u32, u32 | |
| 16 | Strings | SizedString[S] | Block names (`NiObjectNET.Name`) are `NiFixedString` = u32 indices into this table (nif.xml:352, 1664-1668). `BSShaderTextureSet` slots and `BSEffectShaderProperty` Source/Greyscale are inline `SizedString`s and are **not** in the table (see Q2). |
| 17 | Num Groups / Groups | u32, u32[G] | G was 0 in every scanned file [Measured] |

After the header, the blocks follow back-to-back. Footer: `u32 Num Roots` + `i32 Roots[R]` (nif.xml:1732-1736).

**Does seek-only work?** Yes. `offset[i] = header_end + Σ size[0..i)`.
- nifly reads known blocks sequentially. It uses `GetBlockSize` only to skip unknown block types (`NiUnknown`, NifFile.cpp:217-228) and recomputes every size when saving (NifFile.cpp:1505-1524). [Source] So any file that has passed through nifly (Outfit Studio, SSE NIF Optimizer, PGPatcher) has correct sizes. [Inference]
- NifSkope ignores block sizes by default (`"Ignore Block Size", true`, nifmodel.cpp:2064). With the option off, it reads each block, compares its position with `curpos + size`, warns, and seeks to the declared end (nifmodel.cpp:2145-2146, 2192-2211). [Source] So NifSkope expects sizes to be wrong sometimes, but has no repair logic beyond trusting them.
- **Real-world reliability [Measured]:** 21,504/21,504 vanilla + Creation Club NIFs pass `header_end + Σsize + 4 + 4·R == file_len`. Every decoded LSP (58,536), ESP (8,767), texture set (48,825) and alpha property (20,408) consumed exactly its declared size. Third-party mod NIFs were not available locally (open question).
- **Scanner rule:** check (a) the file-length identity before trusting offsets, and (b) consumed == size for every block decoded. On failure, mark the file `sizes_unreliable`. Fall back to layer-1 sequential decode once it exists; until then, report the file as "needs attention" and do not guess.

## Q2. Field order of the scanned blocks (BSVER 83 and 100 are identical)

Inheritance at BSVER > 34: `BSLightingShaderProperty → BSShaderProperty → NiShadeProperty → NiProperty → NiObjectNET`. NiShadeProperty's Flags and BSShaderProperty's legacy fields exist only at `#NI_BS_LTE_FO3#` (BSVER ≤ 34), so they contribute **0 bytes** (nif.xml:5062-5065, 6220-6226; nifly Shaders.cpp:11-14, 207-227). [Source]

**NiObjectNET prefix** (nif.xml:3359-3368; nifly Objects.cpp:12-25):
`[ShaderType u32 — BSLightingShaderProperty only, BSVER 83..139]` `Name u32(strIdx)` `NumExtraData u32` `ExtraData i32[n]` `Controller i32`.

**BSLightingShaderProperty** (nif.xml:6581-6636; nifly Shaders.cpp:404-571). Offsets are from block start, with n = NumExtraData. Each type-specific trailer was verified against all 58,536 blocks [Measured]:

| Off | Field | Type |
|---|---|---|
| 0 | **Shader Type** (BSLightingShaderType enum, nif.xml:1400-1423) | u32 |
| 4 | Name | u32 str idx |
| 8 | NumExtraData, ExtraData[n] | u32 + 4n |
| 12+4n | Controller | i32 |
| 16+4n | **SLSF1** (SkyrimShaderPropertyFlags1) | u32 |
| 20+4n | **SLSF2** | u32 |
| 24+4n | UV Offset | 2×f32 |
| 32+4n | UV Scale | 2×f32 |
| 40+4n | **Texture Set** ref | i32 |
| 44+4n | Emissive Color | 3×f32 |
| 56+4n | Emissive Multiple | f32 |
| 60+4n | Texture Clamp Mode | u32 (0..3, nif.xml:990-996) |
| 64+4n | Alpha | f32 |
| 68+4n | Refraction Strength | f32 |
| 72+4n | Glossiness | f32 |
| 76+4n | Specular Color | 3×f32 |
| 88+4n | Specular Strength | f32 |
| 92+4n | Lighting Effect 1 (nifly: `softlighting`) | f32 |
| 96+4n | Lighting Effect 2 (nifly: `rimlightPower`) | f32 |
| 100+4n | Trailer by type: 1 Env Map → EnvMapScale f32; 5 Skin Tint → Color3; 6 Hair Tint → Color3; 7 Parallax Occ → MaxPasses f32, Scale f32; 11 MultiLayer Parallax → InnerLayerThickness f32, RefractionScale f32, InnerLayerTextureScale 2×f32, EnvMapStrength f32; 14 Sparkle Snow → SparkleParams 4×f32; 16 Eye Envmap → EyeCubemapScale f32, LeftEyeReflectionCenter 3×f32, RightEyeReflectionCenter 3×f32. All other types: none. | nif.xml:6612-6635; nifly Shaders.cpp:534-569 |

Shader-type → slot → suffix semantics are in `research_notes/Skyrim DDS converter project plan/skyrim_conventions.md` §8 (line 567 onward). Not repeated here.

**BSEffectShaderProperty** (nif.xml:6639-6685; nifly Shaders.cpp:700-760). Verified on 8,767 blocks [Measured]:
NiObjectNET prefix (no Shader Type) → `SLSF1 u32` `SLSF2 u32` `UVOffset 2f` `UVScale 2f` **`Source Texture SizedString`** `TextureClampMode u8, LightingInfluence u8, EnvMapMinLOD u8, unused u8` (nifly reads these four bytes as one u32, Shaders.hpp:755) `FalloffStartAngle f` `FalloffStopAngle f` `FalloffStartOpacity f` `FalloffStopOpacity f` `BaseColor 4f` `BaseColorScale f` `SoftFalloffDepth f` **`Greyscale Texture SizedString`**.
- The **inline strings are SizedStrings, not string-table indices** and not a texture set. Env Map / Normal / Env Mask textures and EnvMapScale are `#BS_GTE_130#` only (nif.xml:6676-6679; nifly Shaders.cpp:752-757). **The task brief's list of five effect-shader strings is FO4-only.** For Skyrim, record two strings.

**BSShaderTextureSet** (nif.xml:6307-6319; nifly Shaders.cpp:376-378): `NumTextures u32` + `SizedString[NumTextures]`.
- nif.xml's default is 6, but nifly creates **9** for user 12 (Shaders.cpp:365-374).
- **[Measured]** All 48,825 vanilla+CC texture sets had count 9. Slots that were non-empty at least once: 0 (48,814), 1 (48,814), 2 (3,795), 3 (34), 4 (8,269), 5 (4,473), 6 (565), 7 (2,672). Slot 8: never non-empty. The scanner must read the count, not assume 9.

**NiAlphaProperty** (nif.xml:3972-3978; nifly Shaders.cpp:946-949): NiObjectNET prefix + `Flags u16` (AlphaFlags bitfield: blend bit0, src 1-4, dst 5-8, test bit9, test func 10-12, no-sorter bit13; nif.xml:1554-1564) + `Threshold u8`.

**Other shader blocks seen [Measured]:** BSWaterShaderProperty (36) and BSSkyShaderProperty (35). The sky shader has one inline Source Texture (nif.xml:6708-6719); record it but treat it as low priority. Controller blocks (`BS*ShaderProperty{Float,Color}Controller`) animate LSP/ESP fields and are worth noting for the renderer later.

**FO4 / F76 / SF: reject on BSVER ≥ 130.** Layouts diverge there: Root Material/BGSM name, SF1/SF2 CRC arrays from 132, wetness block, F76 shader type moved, ESP gains strings (nif.xml:6581-6611, 6639-6685; nifly Shaders.cpp:404-531). The prototype rejected 130/132/139/155/175 test files via the header alone [Measured]. Also reject anything that is not 20.2.0.7 + user 12 *before* parsing the string table. MW/OB headers have a different structure (the prototype crashed on them before I added the gate).

## Q3. Shape → shader property → texture set linkage

At BSVER > 34, `NiAVObject.Properties` does not exist (`#NI_BS_LTE_FO3#`, nif.xml:3488-3489; nifly Objects.cpp:72-73). Instead [Source]:
- **NiGeometry family** (NiTriShape, NiTriStrips, BSLODTriShape, BSSegmentedTriShape, and LE particle systems): NiObjectNET + NiAVObject (`Flags u32, Translation 3f, Rotation 9f, Scale f, Collision i32`) + `Data i32` `SkinInstance i32` `NumMaterials u32` `MaterialName u32[m]` `MaterialExtra i32[m]` `ActiveMaterial i32` `NeedsUpdate u8` **`ShaderProperty i32` `AlphaProperty i32`** (nif.xml:3840-3851, 3853-3870; nifly Geometry.cpp:2018-2045). Same at 83 and 100.
- **BSTriShape family** (SSE only: BSTriShape, BSSubIndexTriShape, BSDynamicTriShape): NiObjectNET + NiAVObject + `BoundingSphere 4f` `Skin i32` **`ShaderProperty i32` `AlphaProperty i32`** `VertexDesc u64` `NumTriangles u16` `NumVertices u16` `DataSize u32` … (nif.xml:8228-8246; nifly Geometry.cpp:449-466).
- **NiParticleSystem / BSStripParticleSystem at BSVER 100:** NiObjectNET + NiAVObject + `BoundingSphere` `Skin` **`Shader` `Alpha`** (nif.xml:3860-3869 `onlyT/excludeT` rows). At 83 they use the NiGeometry layout.
- `BSShaderProperty` → `Texture Set` ref (LSP only). The scanner resolves refs by block index; −1 means none.

**[Measured] vanilla+CC shape counts:** BSTriShape 66,758; NiParticleSystem 1,614; BSDynamicTriShape 495; NiTriShape 254 (nearly all inside BSVER-100 files); BSStripParticleSystem 189; BSLODTriShape 48; BSSubIndexTriShape 0. Once particle systems were included, every LSP/ESP block (67,303) was referenced by some shape. In the first 3,000-file sample without them, 377 of 1,608 ESPs (23 %) looked orphaned.

**LE NIFs in SSE load orders:**
- SSE ships at least one BSVER-83 file (1 of 21,505 scanned) and 254 NiTriShape shapes, nearly all inside BSVER-100 files. [Measured]
- The standard LE→SSE path is nifly's `OptimizeFor` (used by SSE NIF Optimizer and similar tools). It converts NiTriShape → BSTriShape, BSSegmentedTriShape → BSSubIndexTriShape, and head parts → BSDynamicTriShape, keeping shader and alpha refs (nifly NifFile.cpp:1542-1700). [Source]
- Community reports say unconverted LE meshes sometimes load and sometimes crash or render invisible in SSE (e.g. [Nexus forum: LE→SE conversion](https://forums.nexusmods.com/topic/10919323-need-some-help-converting-a-mesh-from-le-to-se)). [Claim]
- **Scanner policy:** support 83 and 100 equally. Flag `bsver==83` in the row so the UI can say "LE mesh — consider converting". The texture extraction is the same either way.

**Names to record:**
- Shape name: string-table index → string. These are not unique (e.g. `cylinder_1`, `PegLeft01:9`), so the primary key is the block index.
- Shader property name: it can carry material hints. FO4 uses it for BGSM paths; in Skyrim it is usually empty.
- Optional for the renderer: parent node chain. Every NiNode subclass has `NumChildren u32, Children i32[]` immediately after NiAVObject (nif.xml:4384-4390). With block sizes, a scanner can read children of any node-derived type without knowing the subclass's trailing fields. It needs only a list of node-derived type names. [Inference]

## Q4. Path normalisation

**Observed in vanilla+CC [Measured]:** 439,425 texture-set slot strings.
- 73.3 % are empty.
- Of the 117,436 non-empty strings, 116,996 (99.63 %) start with `textures\` (case-insensitive) and 95 % contain upper case.
- **0** contain `/` and **0** start with `data\`.
- Outliers (441):
  - build-machine paths: `skyrimhd\build\pc\data\textures\…`, `dlc01\build\pc\data\textures\…`
  - absolute paths: `c:\program files (x86)\steam\steamapps\common\cc-s\data\textures\…` (21)
  - missing prefix: `creationclub\fsvsse001\…`
  - non-DDS: `.tga`, `.bmp` (41)
  - control-character garbage: `"\x08ERR"`, `"\n"`, `"textures\\\ntextures\\"`
- Effect-shader strings: 17,534, of which 11,481 start with `textures\`, 10 are build paths and the rest are empty.
- One file had a non-cp1252 byte in a string. **Decode strings lossily from bytes; never fail the file on a bad string.**

**What other tools do [Source]:**
- **nifly `TrimTexturePaths`** (NifFile.cpp:1193-1229): trim whitespace → collapse `/`+ and `\`+ to `\` → strip everything up to the first `\textures\` when not already at the start (case-insensitive) → strip leading `\` → prepend `textures\` if relative and missing (not MW/OB) → keep case.
  - Latent bug [Inference]: it only trims effect-shader strings inside `if (textureSet)`, and Skyrim ESPs have no texture set (NifFile.cpp:1240-1262).
- **NifSkope `get_full_path`** (gamemanager.cpp:567-596): lowercase → `\` to `/` → find `textures/` at a segment boundary and drop the prefix before it, or prepend `textures/` → force the lookup extension. Archive index keys are lowercase `textures/…` (gamemanager.cpp:709-718). `TexCache::stripPath` also lowercases and strips configured data folders (gltex.cpp:207-236).
- **Community Shaders** `TexturePath::Normalize`: "lowercase and convert forward slashes to backslashes". Resource lookups use `Textures\<path>` with `.dds` appended if missing (src/CSEditor/WeatherUtils.h:600-630).
- Engine: I could not verify whether it adds a missing `textures\` prefix itself. [Claim — open question]

**Canonical form for sdc:** `norm = lowercase(path)` such that:
1. Trim ASCII whitespace. If any remaining char is < 0x20, mark `invalid`.
2. Replace `/` with `\`; collapse repeated `\`.
3. If a `\textures\` segment appears after position 0 (or the path is absolute, `X:\…`), cut everything before it.
4. Strip leading `\`, then a leading `data\`.
5. If it does not start with `textures\`, prepend it.
6. Lowercase the result. Keep the extension as-is and flag non-`.dds`.

Store `raw` (lossy UTF-8 of the original bytes) and `norm` (join key against the texture VFS, which should use the same rule), plus `flags` {empty, invalid, rebased, prefixed, non_dds}.

## Q5. FaceGen skip rule

Confirmed from BSA folder records [Measured]. The ninth segment is the plugin file name:
- NIFs: `meshes\actors\character\facegendata\facegeom\<plugin>\…` — `skyrim.esm`, `dawnguard.esm`, `dragonborn.esm`, `hearthfires.esm`, `update.esm`, and every CC plugin (`.esl`/`.esm`/`.esp`).
- Textures: `textures\actors\character\facegendata\facetint\<plugin>\…` (e.g. ccbgssse005-goldbrand.esl).
- 3,735 of 25,240 vanilla+CC NIFs (14.8 %) are FaceGen geometry.

Rule: skip when `norm_path` starts with `meshes\actors\character\facegendata\facegeom\`. On the texture side, `textures\actors\character\facegendata\facetint\` is "preserve, don't touch", consistent with skyrim_conventions.md:107. Make it a toggle (default skip) because facegeom NIFs reference head textures the user may still want listed.

## Q6. Rust implementation options

**Search results:**
- crates.io `nif`: amPerl/nif 0.5.0 (2024-08-02), described as "super-primitive Gamebryo NIF parser targeting version 20.0.0.4". It does not support 20.2.0.7 / BS stream, and its repo page shows no licence ([crates.io API](https://crates.io/api/v1/crates?q=nif&per_page=30), [GitHub](https://github.com/amPerl/nif)). [Doc]
- `project-wormhole-nif` 0.1.0: no repository. Other "nif" hits are Erlang NIF bindings. A `gamebryo` search turns up only an Epic Mickey crate; `skyrim` turns up only BSA/ESP/load-order crates ([crates.io gamebryo](https://crates.io/api/v1/crates?q=gamebryo&per_page=20), [crates.io skyrim](https://crates.io/api/v1/crates?q=skyrim&per_page=40)). [Doc]
- **PGPatcher** (formerly ParallaxGen, GPL-3.0, C++) parses NIFs with **nifly** as a git submodule (`external/nifly`, commit c3cbdd6) and reads BSAs with vcpkg `rsm-bsa` ([repo](https://github.com/hakasapl/PGPatcher), [vcpkg.json](https://raw.githubusercontent.com/hakasapl/PGPatcher/main/vcpkg.json), [external/](https://github.com/hakasapl/PGPatcher/tree/main/external)). [Doc]
- NiflySharp (C#) generates its code from nif.xml (nifly README.md:7). That shows nif.xml-driven codegen is practical, but it is not Rust. [Doc]
- Licences:
  - nifly: GPLv3 text, and source headers say only "See the included GPLv3 LICENSE file", so effectively GPL-3.0-only (nifly-main/LICENSE; src/Shaders.cpp:1-5) [Source]. Linking it into a GPL-3.0-or-later program is fine, but the combined binary is distributable only under GPL-3.0. [Inference]
  - nifxml: GPLv3 (nifxml-master/LICENSE). Code generated from it should be treated as GPL; compatible. [Source]
  - NifSkope: BSD-3-style (nifskope-develop/LICENSE.md). [Source]
  - Community Shaders: GPL-3.0 with modding/linking exceptions (COPYING, EXCEPTIONS.md). [Source]
- BSA crates:
  - `ba2` 3.0.1 (Ryan-rsm-McKenzie, same author as rsm-bsa; reportedly 0BSD; deps include lzzzz, flate2, memmap2, directxtex) [Claim] ([docs.rs](https://docs.rs/crate/ba2/latest), [lib.rs](https://lib.rs/crates/ba2)).
  - `bsa` 0.2.1 (2021, lz4 dep) [Claim] ([lib.rs](https://lib.rs/crates/bsa)).
  - `dream_archive` 1.1.0 (2026-09) [Doc, licence unchecked].
  - Rolling our own is small: the SSE BSA format is about 60 lines in the prototype, cross-checked against NifSkope's libfo76utils `loadBSAFile` (nifskope-develop/lib/libfo76utils/src/ba2file.cpp:195-290, 759-774). It still needs an LZ4-frame decoder crate.

**Options matrix** (1 = poor … 5 = best; my weighting: time-to-ship 25 %, robustness 25 %, licence 10 %, renderer reuse 25 %, speed 15 %):

| Option | Time-to-ship | Robustness | Licence (GPL-3+) | Renderer reuse | Speed | Weighted |
|---|---|---|---|---|---|---|
| **A. Hand-rolled Rust, layered (nif.xml + nifly as reference; tests on nifly fixtures + vanilla BSAs)** | 4: layer 0 is ~600 LOC; prototype already validated on 21.5k files | 4: bounds-checked cursor, size checks, no FFI; narrow scope (BSVER 83/100 only) | 5: own code (GPL-3+) | 5: layer 1 adds geometry on the same header/cursor/block index | 5: zero-copy, seek-only | **4.50** |
| B. Full codegen from nif.xml (build-time) | 2: needs an expression evaluator for `cond`/`vercond`/`onlyT`; inheritance quirks (nif.xml:3853-3870) | 4 once done | 4: generated code is GPLv3-derived; compatible, but sdc's "or later" does not extend to it | 4: covers every block, more than we need | 4 | 3.50 |
| C. FFI to nifly (C++17 via `cc` + C shim) | 3: shim + MSVC build; precedent in `encoders-sys` | 4: battle-tested (BodySlide, PGPatcher) but full parse, C++ exceptions/UB at the FFI edge | 4: GPL-3.0-only lib → combined binary GPL-3.0 | 3: geometry API exists, but nifly-shaped (vectors of structs); renderer still needs a Rust mirror | 3: full decode of every block, 50× more bytes than seek-only | 3.35 |
| D. `nif` crate | 1: wrong version target | 1 | ? no licence shown | 1 | — | reject |

## Recommendation: a layered `nif` crate (workspace member `crates/nif`)

```
crates/nif/src/
  cursor.rs      // LE bounds-checked reader over &[u8]: u8/u16/u32/i32/f32/f16, SizedString→Cow<[u8]>, no panics
  header.rs      // NifHeader { ver, user, bsver, block_types, type_idx (&0x7FFF), sizes, offsets, strings, groups }
                 // + validate(): version gate (20.2.0.7, user 12, bsver ∈ {83,100}), Σsize/footer identity
  block.rs       // BlockRef(i32), BlockIndex = typed view: kind(i) → &str, bytes(i) → &[u8]
  net.rs         // NiObjectNET prefix (shader-type quirk), NiAVObject (flags, transform, collision)
  props/{lighting,effect,texset,alpha}.rs  // full decoders of these small blocks; consumed==size check
  shape_head.rs  // NiGeometry-family and BSTriShape-family heads up to shader/alpha refs (+ VertexDesc, counts)
  scan.rs        // LAYER 0: fn scan(bytes) -> Result<NifScan>: header → shape heads → follow refs → rows
  path.rs        // canonical texture path (Q4), shared with the texture VFS
  // ---- later, LAYER 1 (renderer) — additive, reuses everything above ----
  graph.rs       // NiNode children (any node-derived type, by name list) → world transforms
  geom/vertex_desc.rs   // BSVertexDesc u64: size nibble, per-attr offsets, attr flags (nif.xml:2077-2105)
  geom/bstrishape.rs    // SSE vertex: pos f32×3 (always full precision at BSVER 100 — nifly Geometry.cpp:517),
                        //   bitangent.x f32, uv f16×2, normal u8×3 + bitangent.y, tangent u8×3 + bitangent.z,
                        //   color u8×4, skin f16×4 + u8×4, eye f32 (nif.xml:2128-2141); triangles u16×3
  geom/skin_partition.rs // SSE skinned BSTriShape: vertex data lives in NiSkinPartition (nif.xml:2142-2172;
                        //   nifly NifFile.cpp:2029 moves it back to the shape)
  geom/nitrishape.rs    // LE NiTriShapeData / NiTriStripsData (f32 arrays)
```
- **Correction to the brief:** half-float positions are FO4 (`BSVertexData`, nif.xml:2107-2126). At BSVER 100, positions are f32 (`BSVertexDataSSE`, nif.xml:2128-2131; nifly Geometry.cpp:517). The bitangent is split as x after the position, y in the normal's 4th byte and z in the tangent's 4th byte.
- The VFS (loose + BSA, mmap per archive, LZ4 via a pure-Rust crate) lives **outside** `nif`. `nif` takes `&[u8]`, so the same scanner serves loose files, BSA entries and test fixtures.
- **Test fixtures:** nifly-main/tests/expected/*.nif. These are GPLv3, so they can ship in sdc tests and cover LE/SE/skinned/dynamic/particles. Add an opt-in integration test that walks the user's vanilla BSAs.

## Q7. Performance

Measured on this machine (NVMe Samsung 990/980 Pro; pure-Python prototype, single thread) [Measured]:
- Vanilla `Skyrim - Meshes0/1.bsa` + 74 CC BSAs: **25,240 NIFs, 2.76 GB uncompressed**.
  - Size quantiles: p50 51 KB, p90 264 KB, p99 674 KB, max 5.36 MB; mean 109 KB.
- Of the 21,505 non-FaceGen NIFs, the scanner **touched 38.3 MB of 1.91 GB (2.0 %)**. Parse time was **1.93 s total in pure Python, about 90 µs/NIF**. Pure-Python LZ4 decompression took 43.3 s. **In BSAs, decompression dominates, not parsing.**
- Sequential read of `Skyrim - Meshes1.bsa` (379 MB) ran at about 2.9 GB/s. The cache was likely warm, so treat this as an upper bound.

Estimate for **100k NIFs** [Inference]:
- **Bytes:** about 11 GB uncompressed at a 109 KB mean.
  - Loose files: you could read only header + needed blocks via positioned reads, but whole-file reads are simpler. Median 51 KB means 1-2 reads per file anyway.
  - BSA entries: a whole LZ4 frame must be decoded, because the frame cannot be entered mid-stream.
- **CPU, Rust:**
  - Layer-0 parse should be ≥ 20× faster than the Python prototype (~5 µs/NIF), so about 0.5 s for 100k.
  - Pure-Rust LZ4 decodes at roughly GB/s per core [Claim], so 11 GB is a few CPU-seconds spread over cores.
- **Wall clock:**
  - Warm NVMe: roughly **10–30 s for 100k NIFs**, I/O- and file-open-bound. Windows per-file open/AV-scan overhead on loose files is the main unknown.
  - Cold NVMe: maybe 2–3× that. HDD: minutes, so read BSA entries in offset order.
- **Parallelism:**
  - `rayon` over files (the workspace already depends on rayon).
  - One mmap per BSA, entries sorted by offset, parallel decode.
  - Bounded in-flight memory: p99 is 674 KB, but cap the per-worker buffer and stream outliers.
  - Cache results keyed by (loose path, size, mtime) or (archive path, mtime, entry offset) so rescans only touch changed files.

## Q8. Interface contract (one row per shape)

```text
NifShapeRow {
  nif: SourceId { vfs_path_norm, origin: Loose{abs} | Bsa{archive, entry} },  bsver: u32 (83|100), sizes_ok: bool,
  shape_block: u32, shape_name: String (lossy), shape_kind: enum {BSTriShape, BSSubIndexTriShape, BSDynamicTriShape,
      NiTriShape, NiTriStrips, BSLODTriShape, BSSegmentedTriShape, NiParticleSystem, BSStripParticleSystem},
  parent_chain: Option<Vec<u32>>   // layer 0.5, via NiNode children
  is_skinned: bool (skin ref != -1), num_vertices/num_triangles: Option<u16/u32> (BSTriShape head),
  shader: None | Lighting{block, name, shader_type: u32, slsf1: u32, slsf2: u32,
                           texture_set_block: i32, slots: Vec<TexPath>  /* len = NumTextures, usually 9 */,
                           params: LightingParams }
               | Effect{block, name, slsf1, slsf2, source: TexPath, greyscale: TexPath, params: EffectParams }
               | Other{block, kind: "BSWaterShaderProperty" | "BSSkyShaderProperty" | …, source: Option<TexPath>},
  alpha: Option<{block, flags: u16, threshold: u8}>,   // decoded: blend, src, dst, test, test_func, no_sorter
  controllers: Vec<ControllerKind> /* optional: which shader fields are animated */
}
TexPath { raw: String, norm: String, flags: {empty, invalid, rebased, prefixed, non_dds} }
```

**Every BSLightingShaderProperty numeric field the renderer will want to edit live** (Q2 table; nif.xml:6581-6636):
- `uv_offset[2]`, `uv_scale[2]`
- `emissive_color[3]`, `emissive_multiple`
- `texture_clamp_mode` (0..3)
- `alpha`, `refraction_strength`
- `glossiness`, `specular_color[3]`, `specular_strength`
- `lighting_effect_1` (soft/subsurface), `lighting_effect_2` (rim/back power)
- type 1: `env_map_scale`
- type 5: `skin_tint_color[3]`
- type 6: `hair_tint_color[3]`
- type 7: `max_passes`, `scale`
- type 11: `parallax_inner_layer_thickness`, `parallax_refraction_scale`, `parallax_inner_layer_texture_scale[2]`, `parallax_envmap_strength`
- type 14: `sparkle_parameters[4]`
- type 16: `eye_cubemap_scale`, `left_eye_reflection_center[3]`, `right_eye_reflection_center[3]`
- Plus `shader_type` itself, which changes the trailer layout, and the SLSF1/SLSF2 bits. Both feed the shader permutation.

EffectParams:
- `uv_offset[2]`, `uv_scale[2]`
- `texture_clamp_mode`, `lighting_influence`, `env_map_min_lod`
- `falloff_start_angle`, `falloff_stop_angle`, `falloff_start_opacity`, `falloff_stop_opacity`
- `base_color[4]`, `base_color_scale`
- `soft_falloff_depth`

NiAlphaProperty: `flags` and `threshold`.

If live edits are ever written back to disk, keep block index + field offset so a patcher can rewrite the fixed-size fields in place without re-serialising, because sizes don't change. [Inference]

## Open questions

1. Block-size reliability for **third-party** NIFs (Blender NifTools/PyFFI exports, old LE mods). Measured only on Bethesda/CC files. Run the prototype on a real modlist before relying on the fallback being rare.
2. Does the engine itself tolerate missing `textures\` prefixes or absolute paths? Determines whether sdc should report those as "broken in game" or "cosmetic".
3. BSA crate choice: `ba2` (verify licence and LZ4 path on a v105 archive), or own reader + pure-Rust LZ4 crate (verify licence)?
4. Whether to include `facegeom` NIFs as read-only rows (the head textures they reference) or skip them entirely.
5. Skyrim VR: assumed BSVER 100 (nifly `IsSSE` = 20.2.0.7 + stream 100, include/BasicTypes.hpp:157), but not checked against real VR files.
6. Slot 8 and LE `NumTextures` ≠ 9 in modded files: the scanner reads the count, but the UI should show only slots 0-7 unless one is populated.
7. **LE (BSVER 83) evidence is thin.** It rests on 1 Bethesda-written vanilla file plus 6 nifly-written fixtures, and nifly rewrites block sizes on save (NifFile.cpp:1505-1524). Run the prototype on an original LE install or LE mod meshes before relying on LE size reliability.
8. Community Shaders / TruePBR: identified by SLSF/naming conventions and covered in skyrim_conventions.md §3. Confirm no additional NIF-side fields at BSVER 100.

## §M. Method of the measured numbers

- I wrote a throwaway Python prototype (pure-Python LZ4-frame decoder + SSE BSA v105 reader + the layer-0 scanner described above). It lives outside the repo, in the session scratchpad: `…\scratchpad\nifscan_scripts\scan.py`, results in `…\scratchpad\out\vanilla_full.json`.
- It was run read-only with `python -I` over `G:\SteamLibrary\steamapps\common\Skyrim Special Edition\Data\Skyrim - Meshes0.bsa`, `Skyrim - Meshes1.bsa` and all `cc*.bsa`, plus nifly-main/tests/expected/*.nif (LE + SE + FO4/F76/SF for rejection).
- BSA layout was cross-checked against libfo76utils:
  - header 36 bytes; folder record 24 bytes at v105 vs 16
  - file size bit 30 toggles compression; flag 0x100 means an embedded name bstring precedes the data
  - compressed data starts with a u32 original size, then an LZ4 frame
  - (ba2file.cpp:195-290, 759-774)
- 1 of 21,505 NIFs failed only on cp1252 decoding of a string. Handled by the "lossy strings" rule.

## Sources

Local [Source]:
- nifxml-master/nif.xml:
  - versions 172-233; tokens 5-38
  - BSStreamHeader 1952-1960; Header 1963-1983; Footer 1732-1736
  - basic types 300-303, 344-354, 1652-1656, 1664-1668, 1889-1893
  - EndianType 1228; TexClampMode 990-996; BSLightingShaderType 1400-1423; AlphaFlags 1554-1564
  - VertexAttribute/BSVertexDesc/BSVertexData(SSE) 2077-2141; SkinPartition 2142-2172
  - NiObjectNET 3359-3368; NiAVObject 3440-3495; NiProperty 3516; MaterialData 3840-3851; NiGeometry 3853-3870
  - NiAlphaProperty 3972-3978; NiNode 4384-4390; NiParticleSystem 4535-4541; NiShadeProperty 5062-5065; NiTriShape 5278
  - BSShaderProperty 6220-6226; BSShaderTextureSet 6307-6319; BSLightingShaderProperty 6581-6636; BSEffectShaderProperty 6639-6685
  - BSWater/BSSky 6695-6719; BSStripParticleSystem 6760
  - BSLODTriShape 8202-8207; BSTriShape 8228-8246; BSSubIndexTriShape 8271-8280; BSDynamicTriShape 8450-8453
  - LICENSE
- nifly-main:
  - src/BasicTypes.cpp:640-770, 493-502
  - src/NifFile.cpp:180-235, 1193-1262, 1505-1524, 1542-1700, 2018-2033, 2295-2303
  - src/Shaders.cpp:11-14, 207-227, 365-378, 404-571, 700-760, 946-949
  - src/Objects.cpp:12-25, 50-80
  - src/Geometry.cpp:449-560, 2018-2045, 1560-1566
  - include/BasicTypes.hpp:26-28, 155-157; include/Shaders.hpp:645, 755; include/NifFile.hpp:27-58
  - README.md; LICENSE
- nifskope-develop:
  - src/model/nifmodel.cpp:2061-2211, 3325-3329
  - src/gamemanager.cpp:567-596, 709-718
  - src/gl/gltex.cpp:170-236
  - lib/libfo76utils/src/ba2file.cpp:195-290, 759-774, 1100-1110
  - LICENSE.md
- skyrim-community-shaders-dev: src/CSEditor/WeatherUtils.h:600-630; COPYING; EXCEPTIONS.md
- research_notes/Skyrim DDS converter project plan/skyrim_conventions.md §8 (line 567+), line 107
- converter/Cargo.toml (workspace deps: rayon, cc; no BSA/LZ4 dep today)

Web:
- [crates.io search "nif"](https://crates.io/api/v1/crates?q=nif&per_page=30) [Doc]
- [crates.io "gamebryo"](https://crates.io/api/v1/crates?q=gamebryo&per_page=20) [Doc]
- [crates.io "skyrim"](https://crates.io/api/v1/crates?q=skyrim&per_page=40) [Doc]
- [amPerl/nif](https://github.com/amPerl/nif) [Doc]
- [PGPatcher repo](https://github.com/hakasapl/PGPatcher) [Doc]
- [PGPatcher vcpkg.json](https://raw.githubusercontent.com/hakasapl/PGPatcher/main/vcpkg.json) [Doc]
- [PGPatcher external/nifly submodule](https://github.com/hakasapl/PGPatcher/tree/main/external) [Doc]
- [ba2 on docs.rs](https://docs.rs/crate/ba2/latest) [Claim: licence]
- [ba2 on lib.rs](https://lib.rs/crates/ba2) [Claim]
- [bsa on lib.rs](https://lib.rs/crates/bsa) [Claim]
- [Nexus forum: LE→SE mesh conversion](https://forums.nexusmods.com/topic/10919323-need-some-help-converting-a-mesh-from-le-to-se) [Claim]
- UESP BSA format page returned HTTP 403, so I used libfo76utils instead.
