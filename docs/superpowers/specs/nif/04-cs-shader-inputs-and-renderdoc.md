# 04 — Community Shaders Lighting inputs, preview state, and RenderDoc protocol

Status: research plan, 2026-10-10. Read-only research; nothing in `converter/`, `bench/`, `tools/` or vendored repos was changed.
Purpose: decide what a future sdc 3D NIF preview must feed a re-implementation of CS `Lighting.hlsl`, what it can stub, and how to settle every open point with RenderDoc captures instead of assumptions.

Path abbreviations:
- `CS` = `F:\ai\dds\nifparseref\skyrim-community-shaders-dev` (local checkout, dev branch).
- `LH` = `CS\package\Shaders\Lighting.hlsl`.
- `BS` = `F:\ai\dds\nifparseref\BodySlide-and-Outfit-Studio-dev`.
- `TPM` = `F:\ai\dds\gui reference\ref\TruePBR-Manager-master`.

Tags: **[Source]** read in code here (file:line). **[Doc]** documentation or nif.xml. **[Capture]** seen in the existing S8 RenderDoc capture. **[Claim]** stated by others, not verified. **[Inference]** reasoned, not verified.

Shader-type / texture-slot / suffix tables are **not** repeated here. See `F:\ai\dds\research_notes\Skyrim DDS converter project plan\skyrim_conventions.md` §8 (line 567+) and §3 (TruePBR set, line 168+).

---

## TL;DR

1. **Compile-time vs runtime.** In CS, only part of the engine descriptor picks the compiled permutation. These stay compile-time: the technique (= shader type, bits 24–29), plus SKINNED, MODELSPACENORMALS, VC (VS only), PROJECTED_UV, ANISO_LIGHTING, WORLD_MAP, DO_ALPHA_TEST, and CS's TRUE_PBR / DEFERRED / GLINT. CS strips these from object pixel shaders: SPECULAR / SOFT / RIM / BACK lighting, the Glowmap technique, shadow bits, CharacterLight and Snow. The shader then reads them at runtime from `cb4 Permutation::PerShader.PixelShaderDescriptor`, which holds *only* the stripped bits [Source `CS\src\Hooks.cpp:170-180`, `CS\src\State.cpp:931-993`, `LH:845-868`].
2. **Objects are deferred.** In-world opaque Lighting draws are deferred: 8 MRTs [Source `CS\src\Deferred.cpp:252-298`, `LH:289-308`; Capture: 8 outputs at both S8 draws]. Lighting.hlsl writes these:
   - lit diffuse plus ambient, still gamma-ish;
   - specular, linear;
   - albedo;
   - reflectance;
   - normal / gloss;
   - masks.

   `DeferredCompositeCS` then forms `ToGamma(ToLinear(diffuse) + specular [+ reflectance × cubemap] [SSGI])` [Source `CS\package\Shaders\DeferredCompositeCS.hlsl:89-311`]. Fog is applied later still, in `ISSAOComposite` (`APPLY_FOG`) [Source `CS\package\Shaders\ISSAOComposite.hlsl:179-209`]. The parity target is the MRTs plus the composite, not one pixel shader.
3. **Gamma.** With CS defaults (Linear Lighting off) vanilla materials are lit in "Skyrim gamma": textures are taken raw and the Lighting pass takes no linear step. Specular and ambient mixing use `pow(x, 1.6)` / `pow(x, 1/1.6)`, not sRGB 2.2 [Source `CS\package\Shaders\Common\Color.hlsli:92-110, 208-215, 338-346`]. TruePBR is different. It treats the sampled albedo and glow as *already linear*, which implies hardware-sRGB views. It works in linear space via `pow 2.2` and multiplies the result by 0.65 [Source `Color.hlsli:180, 208-215, 246-252`; `LH:1853-1860`]. Per-slot view formats are still a capture item.
4. **Minimum preview state.** Use an interior cell and fixed lights. Turn off LightLimitFix and every screen-space or world-shadow feature, so the preview needs only these inputs:
   - one directional light (`PerGeometry.DirLightDirection/Color`);
   - a DALC ambient: a 3×4 matrix built from six colours [Source `LH:2624`; `CS\src\State.cpp:1172-1186`];
   - up to 7 point lights in `PerGeometry`;
   - the material cbuffer;
   - the NIF textures.

   Ambient here is the DALC, not IBL (IBL is off by default) [Source `CS\src\Features\IBL.h:52-64`]. Dynamic Cubemaps changes how envmaps are lit. Decide on it explicitly.
5. **Golden test plan.** Extend the S8 scripts into four stages:
   - A: enumerate draws.
   - B: dump SRV formats and all cbuffers per draw.
   - C: save the bound textures, VB/IB and the 8 MRTs.
   - D: `DebugPixel` traces.

   Then run the preview on the captured cbuffers and textures, and pixel-diff **per MRT** before comparing the composite. Pin the CS build. The S8 capture shows an installed CS that differs from the local source: there is a `PerMaterial` size mismatch and extra `PhysSky` SRVs [Capture `F:\ai\dds\tools\rdc-out\ps_ResourceId_17693.txt:32, 58, 62`].

---

## 1. Permutation mapping: NIF type and flags → descriptor → defines

### 1.1 How the descriptor is built and decoded

- **Descriptor layout** [Source `CS\src\ShaderCache.h:584-632`]:
  - Bits 24–29 hold the technique, `GetTechnique = 0x3F & (desc >> 24)` [`CS\src\ShaderCache.cpp:133-136`].
  - Bits 0–23 are flags.
  - The technique ids are the BSLightingShaderProperty shader-type numbers: 0 None/Default, 1 Envmap, 2 Glowmap, 3 Parallax, 4 Facegen, 5 FacegenRGBTint (Skin Tint), 6 Hair, 7 ParallaxOcc, 8 MTLand, 9 LODLand, 10 Snow (unused), 11 MultilayerParallax, 12 TreeAnim, 13 LODObjects, 14 MultiIndexSparkle, 15 LODObjectHD, 16 Eye, 17 Cloud (unused), 18 LODLandNoise, 19 MTLandLODBlend.
- **The engine builds the descriptor.** `BSLightingShaderProperty::GetRenderPasses` encodes it as `passEnum = 0x4800002D + descriptor` [Source `CS\src\TruePBR.cpp:786-787, 811`]. The NIF-flag → bit mapping lives in the game executable, not in CS. CS only post-edits it (TruePBR, InteriorSun) [Source `TruePBR.cpp:776-813`].
- **Decode to defines.** CS calls the *vanilla* decoder `VanillaGetLightingShaderDefines` (RELOCATION_ID 101631/108698). It then appends `DEFERRED` (bit 4), `TRUE_PBR` (bit 3) and `GLINT` (bit 16 together with TRUE_PBR), plus one define per loaded feature [Source `ShaderCache.cpp:138-162`].
- **The define list is not in CS source.** It comes from the exe. The best offline evidence is CS's CI table `CS\.github\configs\shader-validation.yaml`, which lists 493 Lighting PS and 30 VS descriptor → define entries. I correlated bits against defines over all entries with `scratchpad\decode.py`. The result is in table 1.2 [Inference from that data; yaml is supporting evidence only; see the caveat in 1.3].

### 1.2 Bit / define table (objects; landscape and LOD left out)

Source column meanings:
- **NIF**: from the NIF's shader property or alpha property.
- **Eng**: from engine or scene state.
- **CS**: injected by Community Shaders.

Fate column meanings:
- **Compile**: the bit selects the compiled permutation.
- **Runtime**: CS strips the bit from the compile key and the shader branches on `cb4`.

| Bit | Engine flag | HLSL define | Source (NIF flag) | Fate under CS (object PS) |
|---|---|---|---|---|
| 0 | VC | `VC` (VS only; PS gets `input.Color`=1 if absent) | NIF: SLSF2 bit 5 Vertex_Colors and vertex data has colors [Inference] | VS compile. PS: bit set on every PS entry in yaml, no PS define |
| 1 | Skinned | `SKINNED` | NIF: SLSF1 bit 1 Skinned, plus a skin instance | Compile (VS+PS) |
| 2 | ModelSpaceNormals | `MODELSPACENORMALS` | NIF: SLSF1 bit 12 | Compile (VS+PS) |
| 3 | TruePbr (CS) | `TRUE_PBR` | CS: NIF SLSF2 bit 23 "Unused01" (CommonLib `kMenuScreen`), material type Default. Coat PBR NIFs may carry shader type 11; CS replaces the material and clears `kMultiLayerParallax`, so technique 0 is presumably emitted [Inference; check in Q8] | Compile; VS stripped [Source `State.cpp:933-946`] |
| 4 | Deferred (CS) | `DEFERRED` | CS: set during the deferred pass [Source `State.cpp:973-974`] | Compile (PS) |
| 9 | Specular | `SPECULAR` | NIF: SLSF1 bit 0 | **Runtime** [Source `State.cpp:955-963`; `LH:845-868`] |
| 10 | SoftLighting | `SOFT_LIGHTING` | NIF: SLSF2 bit 25 | **Runtime** |
| 11 | RimLighting | `RIM_LIGHTING` | NIF: SLSF2 bit 26 | **Runtime** |
| 12 | BackLighting | `BACK_LIGHTING` | NIF: SLSF2 bit 27 | **Runtime** |
| 13 | ShadowDir | (none) | Eng: directional shadow on; InteriorSun forces it [Source `TruePBR.cpp:807-808`] | **Runtime** (`LH:1728, 2261`) |
| 14 | DefShadow | (none) | Eng: shadow-mask receiver | **Runtime** (`LH:1728, 2383, 2461`) |
| 15 | ProjectedUV / DepthWriteDecals | `PROJECTED_UV`; on HAIR technique `DEPTH_WRITE_DECALS` | NIF: SLSF1 bit 23 Projected_UV, plus engine snow/moss data | Compile [Source `CS\package\Shaders\Common\Permutation.hlsli:38-39`] |
| 16 | AnisoLighting | `ANISO_LIGHTING`; with TRUE_PBR becomes `GLINT` | NIF: SLSF2 bit 21; PBR glint (SLSF2 bit 12 Fit_Slope) | Compile |
| 17 | AmbientSpecular | (none) | Eng (INI/material ambient specular) [Inference] | Stripped (PS + VS) [Source `State.cpp:948-952`] |
| 18 | WorldMap | `WORLD_MAP` | Eng (map menu) | Compile |
| 19 | BaseObjectIsSnow | (none) | Eng | Stripped |
| 20 | DoAlphaTest | `DO_ALPHA_TEST` | NIF: NiAlphaProperty test bit (9) [Inference] | Compile |
| 21 | Snow | `SNOW` | NIF/Eng: snow material shader | Stripped [Source `State.cpp:971`]. The `SNOW` branches are dead for CS object draws [Inference] |
| 22 | CharacterLight | (none) | Eng: actor rim "character light" | **Runtime** (`LH:2541`) |
| 23 | AdditionalAlphaMask | (none) | Eng: fade dithering | Folded into DoAlphaTest, then runtime (`State.cpp:966-969`; `LH:2901`) |
| 24–29 | Technique | `ENVMAP`, `FACEGEN`, `FACEGEN_RGB_TINT`, `HAIR`, `PARALLAX`, `MULTI_LAYER_PARALLAX` (+`ENVMAP`), `EYE`, `TREE_ANIM`, `LODOBJECTS*`, `MULTI_INDEX`+`SPARKLE`+`PROJECTED_UV`, … | NIF: shader type (Skin Tint = 5) | Compile. **Exception: Glowmap (2) is cleared from the PS compile key**, so a glow mesh uses the Default PS and detects glow at runtime via `cb4 >> 24 == 2` [Source `State.cpp:989-993`; `LH:2556`]. VS technique is cleared for 2/3/4/5/6/13/14/15 [Source `State.cpp:976-987`] |

Notes that change the meaning of individual rows:
- **Defines derived inside the shader.**
  - `FACEGEN` or `FACEGEN_RGB_TINT` → `SKIN` [`LH:16-18`].
  - `TREE_ANIM` or `LANDSCAPE` → `VC` [`LH:24-26`].
  - `SKIN` + CS Skin loaded + not PBR → `CS_SKIN_SHADING`.
  - `HAIR` + CS Hair loaded + not PBR → `CS_HAIR_SHADING` [Source `CS\package\Shaders\Common\LightingCommon.hlsli:4-10`].
  - `EXTENDED_MATERIALS` and (PARALLAX | ENVMAP | TRUE_PBR) and not LOD → `EMAT` [`LH:805-811`].
- **Feature defines.** These are added to *every* Lighting permutation while the feature is loaded, with no per-draw choice [Source `ShaderCache.cpp:153-157`]: `IBL`, `DYNAMIC_CUBEMAPS`, `LIGHT_LIMIT_FIX`, `SCREEN_SPACE_SHADOWS`, `SKYLIGHTING`, `TERRAIN_SHADOWS`, `WETNESS_EFFECTS`, `CLOUD_SHADOWS`, `WATER_EFFECTS`, `SSS`, `CS_SKIN`, `CS_HAIR`, `EXTENDED_MATERIALS`, `EXP_HEIGHT_FOG`, `VOLUMETRIC_SHADOWS`, … The per-file list is `shader-validation.yaml:160+`.
- **TruePBR reuses NIF flags.** TruePBR reinterprets NIF flags at load, then *clears* them [Source `TruePBR.cpp:725-757`]:

  | NIF flags | Becomes |
  |---|---|
  | `kMultiLayerParallax` | TwoLayer (coat) |
  | `kMultiLayerParallax` + Soft | InterlayerParallax |
  | `kMultiLayerParallax` + Back | CoatNormal |
  | `kMultiLayerParallax` + EffectLighting (SLSF2 bit 30) | ColoredCoat |
  | Back alone | HairMarschner |
  | Rim | Subsurface |
  | Soft | Fuzz |
  | FitSlope (SLSF2 bit 12) | Glint |

  After that, CS clears `kMenuScreen, kSpecular, kGlowMap, kEnvMap, kMultiLayerParallax, kSoft, kRim, kBack, kAnisotropic, kEffectLighting, kFitSlope` and sets `kVertexLighting` (SLSF2 bit 10) as its internal PBR marker.
  - The descriptor gets bit 3, and bit 9 (Specular) is cleared [Source `TruePBR.cpp:789-805`].
  - PBR textures and constants are bound only for technique None (0) or TreeAnim (12), and for MTLand/LODBlend on landscape [Source `TruePBR.cpp:821-1068`].
  - The PBR flag bit is SLSF2 bit 23 [Source `BS` via nifly `F:\ai\dds\nifparseref\nifly-main\include\Shaders.hpp:108`; FO4 names the same bit `MENU_SCREEN` at `:178`]. The CommonLib name `kMenuScreen` = SLSF2 bit 23 is [Inference]; the CommonLib headers are not checked out.

### 1.3 Caveats

- **The yaml disagrees with the stripping code.** `shader-validation.yaml` lists PS descriptors with SPECULAR / RIM / BACK bits on technique 0, e.g. `Lighting:Pixel:1811` = RIM+BACK+DEFERRED (yaml:14969-14973). The current `ModifyShaderLookup` would never compile that. The yaml is either stale or describes the vanilla decoder's bit → define map, not CS's live compile keys [Inference]. **`State.cpp` and `LH` are authoritative.**
- **cb4 holds only the stripped bits.** `cb4.PixelShaderDescriptor = original & ~modified` [Source `Hooks.cpp:170-180`]. In a capture, cb4 shows only the runtime bits. The compiled defines have to be inferred from the shader's reflection: declared SRVs, input signature, MRT count.
  - The CS perf-event text `Draw: CS Lighting::<hex>` prints this masked value [Source `State.cpp:241-243`]. It is **not** the full descriptor.
  - Those markers are emitted only when Frame Annotations are on *and* the Performance Overlay is visible [Source `State.cpp:186-187, 241`]. Enabling the RenderDoc feature forces annotations on [Source `CS\src\Features\RenderDoc.cpp:154-162`].

---

## 2. Input inventory for an object (non-landscape) draw

Classification column:
- **(A)**: from the NIF or material.
- **(B)**: engine or scene state; the preview stubs a plausible constant.
- **(C)**: CS feature setting; default from CS settings structs. CS ships no default JSON. Defaults are the C++ initialisers in `CS\src\Features\*.h`, and users override them in `SKSE\Plugins\CommunityShaders\SettingsUser.json`.

### 2.1 Vertex shader

| Reg | Block / field | Class | Notes |
|---|---|---|---|
| IA | POSITION0 (xyz + w = tangent.x?), TEXCOORD0, NORMAL0 (xyz + w), BINORMAL0 (xyz + w), COLOR0 (VC), BLENDWEIGHT0/BLENDINDICES0 (SKINNED), TEXCOORD2 (EYE param) | A | TBN rows = `(Pos.w, N.w·2−1, B.w·2−1)`, `B.xyz·2−1`, `N.xyz·2−1` [Source `LH:33-53, 206-210`]. MSN meshes have no NORMAL/BINORMAL. **Which NIF array (tangent vs bitangent) feeds BINORMAL0 is an open capture item.** `Position.w` enters the TBN raw while `Normal.w`/`Bitangent.w` get `·2−1`, which suggests POSITION is a float/signed format and NORMAL/BINORMAL are UNORM8 [Inference]. Stage C's input layout settles it (Q13) |
| b0 | PerTechnique: HighDetailRange, FogParam, FogNearColor, FogFarColor | B | Vertex fog. It still feeds `input.FogParam`, but the deferred PS never uses it [Source `LH:94-100, 270-276`] |
| b1 | PerMaterial: LeftEyeCenter, RightEyeCenter, TexcoordOffset (offset.xy, scale.zw) | A | Eye centres come from the actor/facegen [B for previews]; UV offset/scale come from the NIF [Source `LH:102-107, 189`] |
| b2 | PerGeometry: World (3×4, camera-relative), PreviousWorld, EyePosition, LandBlendParams, TreeParams, WindTimers, TextureProj, IndexScale, WorldMapOverlayParameters | B (World from the NIF transform plus the camera) | [Source `LH:109-120`] |
| b9 / b10 | PreviousBones[240] / Bones[240] (float4 ×3 per bone) | B (preview: bind pose or the captured pose) | [Source `CS\package\Shaders\Common\Skinned.hlsli:4-12`] Pivot in `VS_PerFrame.BonesPivot` |
| b12 | VS_PerFrame: ScreenProj, ViewProj (c8), BonesPivot (c40), PreviousBonesPivot | B | [Source `LH:122-130`] |

### 2.2 Pixel shader constant buffers

| Reg | Block | Key fields → class |
|---|---|---|
| b0 | PerTechnique | `FogColor` (B), `ColourOutputClamp` (B, fLightingOutputColourClamp*), `VPOSOffset` (B, shadow-mask UV) [Source `LH:505-510`] |
| b1 | PerMaterial | Each field is listed below [Source `LH:512-556`] |
| b2 | PerGeometry | Each field is listed below [Source `LH:558-576`] |
| b3 | `LightLimitFix::StrictLightData` | NumStrictLights, RoomIndex, ShadowBitMask, StrictLights[15] — **B**. Only with LIGHT_LIMIT_FIX [Source `CS\features\Light Limit Fix\Shaders\LightLimitFix\LightLimitFix.hlsli:9-21`] |
| b4 | `Permutation::PerShader` | VertexShaderDescriptor, PixelShaderDescriptor (runtime bits only, see 1.3), ExtraShaderDescriptor (InWorld, InReflection, IsBeastRace, AdditiveLighting …), ExtraFeatureDescriptor, EffectRadius — **A + B** [Source `Permutation.hlsli:96-104`; `CS\src\State.h:250-274, 337-354`] |
| b5 | `SharedData` | WaterData[25], DirLightDirection/Color (copy of the sun), Sun/Masser/Secunda, CameraData, BufferDim, Timer, FrameCount, InInterior, HasDirectionalShadows, InMapMenu, HideSky, MipBias, WaterSystemHeight, AmbientSHR/G/B (the DALC as SH), HDRData — **B** [Source `CS\package\Shaders\Common\SharedData.hlsli:9-36`; filled in `State.cpp:1078-1190`] |
| b6 | `FeatureData` | Per-feature settings structs (grass, extendedMaterial, cubemapCreator, terraOcc, LLF, wetness, skylighting, cloudShadows, LOD blending, hairSpecular, terrainVariation, ibl, extendedTranslucency, **linearLighting**, enb, terrainBlending, expHeightFog, truePBR, skinData, horizonFix …) — **C** [Source `SharedData.hlsli:451-472`] |
| b7 | `Skin::SkinPerGeometry` | float4 — **C/B** [Source `CS\features\Skin\Shaders\Skin\Skin.hlsli:21-24`] |
| b8 | `LLPerGeometry.emissiveMult` | **A** (NIF emissive multiple, used only with LL on) [Source `CS\package\Shaders\Common\Color.hlsli:9-14, 240-244`] |
| b11 | `AlphaTestRefBuffer.AlphaTestRefRS` | **A** (NiAlphaProperty threshold/255) [Source `LH:578-581, 2945`] |
| b12 | `FrameBuffer::PerFrame` | Camera matrices, CameraPosAdjust, FrameParams (x = 1/gamma, yz = fog enable flags), DynamicResolutionParams — **B** [Source `CS\package\Shaders\Common\FrameBuffer.hlsli:7-30`] |

**b1 PerMaterial, field by field** (non-PBR layout; PBR reuses some fields):
- `LODTexParams` (B).
- `TintColor` (A: skin/hair tint).
- `EnvmapData` (x = A envmap scale; y = engine sets 1 if a `_m` exists).
- `ParallaxOccData` (A).
- `SpecularColor` (A: xyz color, w = glossiness/shininess). Whether xyz already includes NIF specular strength is an open question.
- `SparkleParams`.
- `MultiLayerParallaxData` (A: thickness, refraction, inner UV scale).
- `LightingEffectParams` (A: x = soft-light rolloff (NIF "Lighting Effect 1"), y = rim power ("Lighting Effect 2")).
- `IBLParams` (B/C).
- `CharacterLightParams` (B).
- `PBRFlags`, `PBRParams1` (roughness scale, displacement scale, specular level), `PBRParams2` (subsurface or coat colour + opacity/strength) (A, PBR).
- `MaterialObjectRGBScale` (A).

**b2 PerGeometry, field by field:**
- `DirLightDirection`, `DirLightColor` (B).
- `ShadowLightMaskSelect` (B).
- `MaterialData` (x = envmap LOD fade, y = specular LOD fade, z = alpha; alpha is A, the fades are B).
- `AlphaTestRef`, `EmitColor` (A: NIF emissive color × multiple [Inference]).
- `ProjectedUVParams`/`2`/`3` (A/B).
- `SSRParams` (B).
- `WorldMapOverlayParametersPS`.
- `DirectionalAmbient` (3×4, B).
- `AmbientSpecularTintAndFresnelPower` (B).
- `PointLightPosition[7]` (xyz + radius in w), `PointLightColor[7]`, `NumLightNumShadowLight` (B).

### 2.3 PS SRVs and samplers (object path)

Sampler `sN` pairs with `tN` unless noted. "Hardware view" below is the hypothesis for the capture to confirm.

| Reg | Vanilla meaning (define gate) | TruePBR meaning | Class | Colour space as the shader treats it (LL off / LL on) |
|---|---|---|---|---|
| t0 | Diffuse (`TexColorSampler`) | Albedo (+ alpha) | A | Vanilla: taken raw, gamma-space; no decode with LL off [`Color.hlsli:208-215`]. LL on: `pow(c, colorGamma 1.8) × vanillaDiffuseColorMult`. **PBR: assumed linear**; `Diffuse()` → `LinearToSrgb`, then `SrgbToLinear` for F0 [`LH:1853-1857`]. That only works if the view is `*_SRGB` |
| t1 | Normal; alpha = gloss/spec mask | Normal | A | Data, `n·2−1`. No green flip, no Z reconstruction [`LH:606-609, 1538`]. MSN: `normal.xzy·2−1` [`LH:1530`] |
| t2 | MSN specular `_s` (MODELSPACENORMALS only), `.x` = glossiness [`LH:443-445, 1532`] | (same) | A | Data |
| t3 | FACEGEN: tint (`TexTintSampler`); PARALLAX: height; PROJECTED_UV: projected diffuse | (unused) | A (facegen tint is B, generated by the engine) | Facegen math works in gamma space [`LH:673-683`] |
| t4 | Env cubemap (ENVMAP/MLP/EYE); FACEGEN: detail map | Displacement (`HasDisplacement`) | A | Cubemap: `SkyrimGammaToLinear` = pow 1.6 [`LH:2067`]. This decode applies **regardless of LL**, unlike every other row |
| t5 | Env mask `_m` (+CPM complex material: S8) | RMAOS (R rough × PBRParams1.x, G metal, B AO, A spec × PBRParams1.z) [`LH:1449-1450, 1837-1860`] | A | Data. S8 capture showed `_m` bound as `BC7_UNORM` [Capture `F:\ai\dds\tools\rdc-out\stage1.log`] |
| t6 | Glow map (Glowmap technique, runtime) | Emissive (`HasEmissive`) | A | Vanilla: treated as gamma, mixed via `SrgbToLinear` (pow 2.2) [`LH:2561-2587`]. PBR: assumed linear (`LinearToSrgb` in `Glowmap()`) [`Color.hlsli:246-252`] |
| t7 | Not declared on the object path. NIF slot 7 is routed to t2 (MSN spec) or t9 (backlight) by the engine [Inference] | (unused) | — | — |
| t8 | MLP inner layer (`TexLayerSampler`); PROJECTED_UV: projected normal | (unused) | A | Inner colour is used raw |
| t9 | Backlight map (BACK_LIGHTING runtime); CS Hair: flow map | FeaturesTexture1 = NIF slot 6: coat (CNR) or fuzz | A | — |
| t10 | Projected detail / sparkle | — | A/B | — |
| t11 | Character-light projection noise | — | B (engine noise) | — |
| t12 | Rim/soft light map `_sk` (SOFT/RIM runtime). CS Skin: SSS thickness (R) | FeaturesTexture0 = NIF slot 7: subsurface (`_s`) or coat colour | A | — |
| t13, t15 | LOD/landscape only | — | — | — |
| t14 | Shadow mask (screen-space; RGBA = 4 shadow lights) | same | B | Preview: white (1,1,1,1) |
| t17 | `SharedData::DepthTexture` | same | B | — |
| t18 | VolumetricShadows shared shadow map | — | B/C | — |
| t25 / t26 | Cloud shadows cube / self shadow | — | C | — |
| t30 / t31 | DynamicCubemaps `EnvReflectionsTexture` / `EnvTexture` | — | C | — |
| t35–t37 | LLF lights / lightList / lightGrid (structured) | — | B | — |
| t45 | Screen-space shadows | — | C | — |
| t50 / t53 | Skylighting probe array / shadow visibility | — | C | — |
| t55 | Terrain blending mask | — | C | — |
| t60 | Terrain shadow height | — | C | — |
| t65 | Water caustics | — | C | — |
| t70 | Wetness precip occlusion | — | C | — |
| t71 / t74 / t75 | CS Skin `_rfaos` / `_wet` / wet normal | — | A (by path convention, §8) | Data |
| t72 | CS Skin global detail normal (`skin_detail_n.dds`) | — | C | Data |
| t73 | CS Hair tangent shift | — | C | — |
| t76–t79 | IBL env/sky SH, static diffuse/specular IBL | — | C | — |
| t98 | `DirectionalShadowLights` (structured) | — | B | — |

Sampler registers: s0 colour (also used as `LinearSampler`), s1 terrain parallax, s2 spec, s3 tint/parallax/projected, s4 env (or PBR parallax), s5 env mask / RMAOS, s6 glow, s8 layer, s9 backlight, s10 projected detail, s11 char-light noise, s12 rim/soft, s14 shadow mask [Source `LH:312-396`]. Wrap/filter modes come from the NIF's texture clamp mode and the engine (PBR forces anisotropic) [Source `TruePBR.cpp:925-1046`].

Feature register sources:
- `CS\features\*\Shaders\*\*.hlsli` (grep `register(`).
- `CS\package\Shaders\Common\ShadowSampling.hlsli:34`.
- The S8 capture's PS reflection confirms the live set: `t0 t1 t4 t5 t6 t11 t14 t17 t25 t35-37 t45 t50 t60 t61 t65 t76 t77 t112` and `cb0-cb8, cb12` [Capture `ps_ResourceId_17693.txt:26-62`]. `t61`/`t112` "PhysSky" are **not in this local checkout**, so the installed CS ≠ local source.

**Vanilla NIF slot → register.** Half the slots do not map to the same register number. Tag: [Inference from §8 + the `LH:440-482` declarations]. Stage-A content matching (Q1/Q4) confirms it.

| NIF slot (0-based) | Register by technique / flag |
|---|---|
| 0 diffuse | t0 |
| 1 normal | t1 |
| 2 glow / `_sk` | t6 (Glowmap technique) **or** t12 (rim/soft light, CS Skin thickness) |
| 3 height / face detail | t3 (PARALLAX height) **or** t4 (FACEGEN detail) |
| 4 cubemap | t4 (ENVMAP / MLP / EYE) |
| 5 env mask | t5 |
| 6 inner layer / face tint | t8 (MLP inner) **or** t3 (FACEGEN tint) |
| 7 `_s` / backlight | t2 (MSN specular) **or** t9 (backlight) |

**TruePBR slot remap** [Source `CS\src\TruePBR\BSLightingShaderMaterialPBR.h:58-62`, `TruePBR.cpp:917-1068`]:

| NIF slot (0-based) | Content | Register |
|---|---|---|
| 0 | Albedo | t0 |
| 1 | Normal | t1 |
| 2 | Emissive | t6 |
| 3 | Displacement | t4 |
| 5 | RMAOS | t5 |
| 6 | Features1 (coat normal+roughness / fuzz) | t9 |
| 7 | Features0 (subsurface / coat colour) | t12 |

- PBR constants: `PBRFlags` (HasEmissive, HasDisplacement, HasFeaturesTexture0/1, Subsurface, TwoLayer, ColoredCoat, InterlayerParallax, CoatNormal, Fuzz, HairMarschner, Glint, ProjectedGlint) [Source `BSLightingShaderMaterialPBR.h:16-31`].
- `MultiLayerParallaxData` and `SparkleParams` are reused for coat, fuzz and glint params.
- `ParallaxOccData` and `MaterialObjectRGBScale` are reused for projected-material params.
- Off-by-one warning: PGPatcher JSON / TruePBR-Manager use **1-based** `slot1..slot8` names [Claim, TPM `JsonExporter.cpp:366-408`]. §8 uses 0-based indices.

### 2.4 Outputs (DEFERRED)

| RT | Content |
|---|---|
| SV_Target0 Diffuse | `color` = (direct diffuse + emissive) × base + ambient × albedo + transmission, × vertexColor. Still in "irradiance gamma" with LL off; no specular, no fog |
| 1 | MotionVectors |
| 2 | NormalGlossiness (`GBuffer::EncodeNormal(viewN)`, 1 − roughness, w = stochastic alpha) |
| 3 | Albedo |
| 4 | Specular (linear, includes vanilla envmap when Dynamic Cubemaps is off) |
| 5 | Reflectance (indirect specular weight, used only when Dynamic Cubemaps is on) |
| 6 | Masks (SSS mask, not-beast, ambient luma) |
| 7 | Masks2 (1 − vertexAO) |

[Source `LH:289-308, 3012-3063`]

---

## 3. Minimum viable preview state, and the deferred question

### 3.1 What happens after Lighting.hlsl (deferred objects)

1. **Opaque world pass.** `Main_RenderWorld_Start` → `StartDeferred` binds 8 RTs and sets `deferredPass`. Every Lighting draw then gets bit 4, i.e. `DEFERRED` [Source `CS\src\Deferred.cpp:252-298, 669-676`; `State.cpp:973-974`]. Alpha-blended geometry drawn after `EndDeferred` takes the forward path; that path adds specular and fog in-shader [Source `LH:2805-2870`].
2. **`DeferredPasses`.** Runs SSGI, then SSS (skin), then the Dynamic Cubemaps update, then the composite dispatch [Source `Deferred.cpp:301-417`].
3. **`DeferredCompositeCS`** [Source `DeferredCompositeCS.hlsl:97-311`]:
   - `color = IrradianceToLinear(MainRT) + SpecularRT`;
   - with SSGI: re-splits ambient using `Masks.z` and AO;
   - with DYNAMIC_CUBEMAPS: adds `Reflectance × cubemap irradiance`, normalised to DALC luma × 0.65 (`ReflectionNormalisationScale`). The INTERIOR variant uses `EnvTexture` only;
   - finally `IrradianceToGamma`.
4. **Fog.** `ISSAOComposite` with `APPLY_FOG` applies vanilla or exponential height fog from depth [Source `ISSAOComposite.hlsl:179-209`].
5. **Post.** Then come volumetric rays, HDR / tonemap (ISHDR) and TAA / upscaling. All of these are outside the material, and the preview should not emulate them for the golden test.

**Consequence.** To compare like with like, the preview should produce the same 8 MRTs and then run a port of the composite with SSGI / Skylighting / IBL off. The comparison point is the composite's `MainRW` output (pre-fog, pre-tonemap HDR). The backbuffer is the wrong comparison point.

### 3.2 Recommended preview state (the "golden profile")

| Item | Value | Why / source |
|---|---|---|
| Cell | Lit interior, e.g. `QASmoke` or a small interior with no windows. `InInterior` = 1 | Makes `GetWorldShadow` = 1 [Source `ShadowSampling.hlsli:51-55`]. Removes sky specular in the composite [Source `DeferredCompositeCS.hlsl:232-234`] |
| Time / weather | Fixed: `set gamehour to 12`, `fw <weather>` (both irrelevant indoors except for the lighting template) | Repeatability |
| Directional light | One. Interior directional from the cell's lighting template (`DirLightColor`, `DirLightDirection` in PerGeometry b2 and SharedData b5) | — |
| Ambient | DALC `PerGeometry.DirectionalAmbient` (3×4), `mul(M, float4(n,1))` [Source `LH:2624`]. Preview stub: from six colours c±X/±Y/±Z, `M[:,i] = (c+i − c−i)/2`, `t = mean of the six` [Source, inverse of `State.cpp:1172-1180`]. `SharedData.AmbientSH*` must come from the same six colours | — |
| IBL | Off (default `EnableIBL=0`) [Source `IBL.h:52`] | — |
| Shadows | None. Shadow mask t14 = 1. Clear DefShadow/ShadowDir. Point-light shadows off | — |
| Fog | Not part of the deferred Lighting PS. The preview simply doesn't run a fog stage. The capture is read at the composite, before ISSAOComposite | — |
| Point lights | 0 for the base captures, then 1 point light for a second capture. With LLF disabled, these arrive in `PerGeometry.PointLight*[7]` | — |
| Linear Lighting | Off (default `enableLinearLighting=false`) [Source `CS\src\Features\LinearLighting.h:28`] | Keeps the vanilla gamma model. LL-on is a later second profile |
| CS features to **Disable at Boot** for MVP parity | LightLimitFix (falls back to the 7-slot PerGeometry arrays [`LH:2368-2420`]), Skylighting, Screen Space GI, Subsurface Scattering, Screen-Space Shadows, Volumetric Shadows, Terrain Shadows, Cloud Shadows, Wetness Effects, Water Effects, Exponential Height Fog, Interior Sun, Volumetric Lighting, IBL, Upscaling/TAA if possible | Each one adds SRVs or terms that a preview would otherwise have to stub |
| Decide explicitly | **Dynamic Cubemaps**: when on, vanilla envmaps with a 1×1 cube take the DC path, and specular moves into Reflectance (`LH:2010-2061`). Recommend **off** for MVP, plus a second "DC on" capture. **Extended Materials (CPM)**: keep on; CPM is part of the converter scope (S8). **Skin** (`EnableSkin=true` by default [Source `CS\src\Features\Skin.h:57`]) and **Hair Specular** (`Enabled=true` [Source `CS\src\Features\HairSpecular.h:36`]) replace the vanilla BRDF for skin and hair. Capture once with them off (vanilla parity) and once as shipped | — |

---

## 4. Secondary references vs CS (short table)

| Aspect | CS (ground truth) | BodySlide / Outfit Studio | TruePBR-Manager |
|---|---|---|---|
| Lighting space / gamma | Vanilla: gamma-space, pow 1.6 for spec/ambient mixing; PBR: linear (pow 2.2) × 0.65 [Source `Color.hlsli:92-110, 180`] | Gamma-space on raw texels, Hable tonemap, clamp; PBR shader decodes pow 2.2 [Source BS `res\shaders\default.frag:126-136, 389-396`; `sk_truepbr.frag:137, 191`] | ACEScg linear, hardware sRGB for albedo/emissive, GT7 tonemap + sRGB OETF [Source TPM `src\renderer\PBRShader.hlsl:260-262`; `ToneMapShader.hlsl:62-77`] |
| Vanilla spec | `exp2(shininess·log2(N·H)) × SpecularColor × normal.a × lightColor`, no NdotL, no clamp, no Fresnel [Source `CS\package\Shaders\Common\LightingEval.hlsli:74-75, 141`] | `pow(N·H, glossiness) × specColor × specStrength × mask`, clamped to 1; no spec without a normal map [Source BS `default.frag:140-146, 245-270`] | n/a |
| PBR spec | GGX + Smith joint approx + Schlick; `F0 = lerp(RMAOS.a × specLevel, albedo, metal)`; Lambert diffuse × π compensation [Source `LH:1856`; `PBR.hlsli:19-26, 164`; `Color.hlsli:218-225`] | GGX, height-correlated Smith, Schlick; F0 = RMAOS.a × shininess [Claim, subagent: BS `sk_truepbr.frag:107-119`] | GGX + Smith joint + Schlick, Burley diffuse; F0 = `lerp(specLevel × RMAOS.a, albedo, metal)` [Claim, subagent: TPM `PBRShader.hlsl:307`] |
| Tangent normal decode | `rgb·2−1`, no green flip, B used as-is; TBN from packed vertex w components [Source `LH:206-210, 606-609`] | Same decode; TBN columns (bitangent, tangent, normal), i.e. NIF naming swap [Claim BS `default.vert:126-132`] | Same decode; optional CPU "flip G"; TBN from tangent.w sign [Claim TPM `PBRShader.hlsl:167, 265`] |
| MSN decode | `rgb.xzy·2−1` (G↔B swap), no negation. Unskinned: used directly as the world normal, no World rotation. Skinned: rotated by bones [Source `LH:1530, 1659-1662`] | `rgb·2−1`, then **negate X**, then view rotation; no xzy swap [Claim BS `default.frag:252-255`] | n/a |
| MSN spec | `_s.r` (t2) → glossiness [Source `LH:1532`] | `_s.r` (slot 7) as the spec mask [Claim] | n/a |
| Skin tint | FacegenRGBTint: `1.0117·(b² + 2·tint·b − 2·tint·b²)`, gamma space [Source `LH:685-691`] | `albedo *= tintColor` [Claim BS `default.frag:187-190`] | n/a |
| Face tint | `(b² + 2tb − 2tb²)·detail·3.98` [Source `LH:673-683`] | `mix(a, a·tint·2, tint.a)` [Claim] | n/a |
| Hair tint | `lerp(1, TintColor, vertexColor.g)` (vanilla); CS Hair: `lerp(1, tint, VC.g)` on base colour [Source `LH:2642, 1585`] | Multiply by tint [Claim] | n/a |
| Rim / back / soft | All three, runtime flags [Source `LightingEval.hlsli:133-140`] | Soft only, a different formula; rim and back commented out [Claim] | n/a |
| Envmap | `cube(reflect) × envScale × (mask or normal.a)`, pow 1.6, added to specular × diffuse irradiance [Source `LH:1996-2068, 2772`] | Added to albedo, effectively lit [Claim] | HDRI IBL |
| Lights | 1 directional + DALC ambient + point lights | 4 directional (one camera-frontal), ambient 0.2 added per light (≈0.8 total) [Claim BS `GLSurface.cpp:96-112`; `default.frag:145`] | 1 directional (intensity 3) + HDRI IBL [Claim] |

The main lesson: neither secondary renderer is a parity reference. BodySlide's MSN path (negate X, no xzy) and lighting rig differ from CS. TruePBR-Manager uses its own register layout and colour pipeline. Both are useful only for UI ideas.

---

## 5. RenderDoc capture protocol (checklist)

Reuse the S8 workflow:
- Capture in game with CS's RenderDoc feature (RenderDoc 1.43).
- Replay headless with `F:\ai\dds\tools\RenderDoc_1.43_64\qrenderdoc.exe --python <script>` [Doc `F:\ai\dds\converter\docs\DECISIONS.md:339-360`; `F:\ai\dds\converter\out\s8-renderdoc.md`].
- Keep the S8 script pattern: `InitialiseReplay`, then `OpenCapture`, then walk actions, then `GetPipelineState`, then `GetCBufferVariableContents`, then `PickPixel`, logging to `tools\rdc-out\` [Source `F:\ai\dds\tools\rdc-scripts\s8_stage1.py`, `s8_stage2.py`].

### 5.0 Before capturing

- [ ] Record the exact CS version and build (main menu / log). Check out the matching CS source tag next to the dev checkout. The S8 capture already differs from the local source, see TL;DR 5.
- [ ] Create a CS settings profile "golden" per §3.2: the Disable-at-Boot list, LL off, IBL off, Dynamic Cubemaps off. Save a copy of `SettingsUser.json` next to the captures.
- [ ] Turn on Advanced → Frame Annotations, make the Performance Overlay visible, and enable the RenderDoc feature. This gives `Draw: CS Lighting::<runtime bits>` and `Defines:` markers [Source `State.cpp:186-187, 241-243`; `RenderDoc.cpp:154-162`].
- [ ] Write the capture comment (CS feature field): profile name, cell, objects, CS version.
- [ ] Turn off TAA/upscaler if possible. Otherwise note the jitter; `FrameBuffer` has the unjittered matrices.

### 5.1 Scene

- [ ] Use `coc QASmoke`, or a small enclosed interior with one lighting template. Run `tfc 1` with a fixed camera, `tai`, `set timescale to 0`, `tm` to hide the HUD, and save a game for repeatability.
- [ ] Pick test objects in xEdit or NifSkope by their BSLightingShaderProperty type. Placing a base object by FormID only works for objects with a placeable base (STAT/MISC/ARMO on a mannequin). Use `player.placeatme <baseID>`; for actors, `player.placeatme <NPC>` plus `tai`. One object per shader type:
  - [ ] Default (0): tangent `_n` with SLSF1 Specular. Also one with Soft/Rim/Back, e.g. a cloth with `_sk`/`_b`.
  - [ ] EnvMap (1): a metal STAT with `_e` + `_m`. Include one CPM `_m` (reuse S8 `m_250_m`).
  - [ ] Glow (2): a lamp or soul gem with `_g`.
  - [ ] Parallax (3): an object with `_p`. Needs CPM/EMAT on.
  - [ ] FaceTint (4) + Eye (16): an NPC with the head in view. `player.placeatme` an NPC, `tai`.
  - [ ] SkinTint (5): the same NPC's body. Try one MSN body and one tangent-space body (UBE-style) [§8].
  - [ ] Hair (6): the same NPC.
  - [ ] MultiLayer (11): an armor/ice mesh with `_i`.
  - [ ] TruePBR: a PGPatcher-patched mesh (SLSF2 bit 23 set in the NIF) with `_rmaos`. Add one with `_g` emissive, one coat/subsurface if available.
  - [ ] Give every test object a **unique diffuse** (copy + recolour) so stage-A content matching (S8 method) ties draw ↔ object unambiguously.
- [ ] Capture three frames: (1) golden profile; (2) golden + 1 point light near the objects; (3) as-shipped profile (Skin, Hair, DC, Skylighting on).

### 5.2 Script stages (new scripts, to be written later under `tools\rdc-scripts\`, not now)

**`p04_stageA_enumerate.py`.** Walk all actions. For each draw:
- marker text: the nearest *preceding* `Draw: CS Lighting::<hex>` group and its `Defines: …` marker. The group is Begin → Marker → End with nothing inside, so it sits before the draw, not around it [Source `State.cpp:242-244`];
- PS and VS resource ids;
- PS reflection: SRV names and bind numbers, cbuffer names and sizes, output count;
- VS input signature (semantics → SKINNED, MSN, VC, EYE).

Content-match t0 against the test diffuses (S8 `mip0` method). Output `drawmap.json` = {event, object, runtimeBits, inferredDefines, psId}.
- Inferred defines rule set:
  - 8 outputs → DEFERRED;
  - `TexSpecularSampler` t2 → MSN;
  - `TexEnvSampler` t4 cube → ENVMAP/MLP/EYE;
  - `TexRMAOSSampler` t5 → TRUE_PBR;
  - `TexLayerSampler` t8 → MLP;
  - `TexTintSampler`/`TexDetailSampler` → FACEGEN;
  - BLENDWEIGHT in the VS → SKINNED;
  - TEXCOORD2 float → EYE.

**`p04_stageB_state.py`.** For each mapped draw:
- [ ] Per SRV slot: resource id, **`format.Name()` (look for `_SRGB`)**, dims, mips, array/cube; the view format if it differs from the resource format.
- [ ] Per sampler: filter, address U/V, LOD bias, max aniso.
- [ ] Every PS and VS cbuffer via `GetCBufferVariableContents` (S8 stage-2 code), dumped as JSON. Key fields to record: `PerMaterial.*`, `PerGeometry.DirLightDirection/Color/DirectionalAmbient/PointLight*/MaterialData/EmitColor`, `PerTechnique.*`, `Permutation.*`, `SharedData.{DirLight*,AmbientSH*,InInterior,MipBias}`, `FeatureData.linearLightingSettings/extendedMaterialSettings/truePBRSettings/skinData/hairSpecularSettings`, `AlphaTestRefRS`, `LLPerGeometry`.
- [ ] RT formats of all 8 outputs, plus the depth format.
- [ ] Blend, depth and rasterizer state (cull mode, for double-sided).

**`p04_stageC_dump.py`.** For each mapped draw:
- [ ] `SaveTexture` every bound SRV to DDS, full mips.
- [ ] VB/IB via `GetBufferData` + `GetVertexInputs`/input layout (offsets, formats). Compare against the NIF arrays to settle which NIF array feeds NORMAL0 / BINORMAL0 / POSITION.w.
- [ ] Bones cb10 for skinned draws.
- [ ] The 8 MRTs at the draw's end event, cropped to the object's bbox (`GetTextureData`). Also MainRW **before and after** the composite dispatch.
- [ ] Composite: find the `Dispatch` whose CS reflection has `AlbedoTexture`. Record its 16 SRVs (t0–t15), 3 UAVs, cb5/cb6/cb12, and the define set inferred from reflection (DYNAMIC_CUBEMAPS / SSGI / IBL / INTERIOR).

**`p04_stageD_debug.py`.** Pick 2–4 pixels per object (lit side, grazing, shadowed side). Run `ctrl.DebugPixel(x, y, …)` and record the trace values at the source variables:
- `baseColor`, `normal`, `glossiness`, `shininess`, `envMask`;
- `dirLightColor`, `directionalAmbientColor`, `specularColor`;
- `material.F0/Roughness/Metallic` (PBR).

The traces are the authoritative intermediate values for the preview's unit tests.

### 5.3 Open question → where to look → what to record

| # | Question | Draw / resource | Record |
|---|---|---|---|
| Q1 | sRGB-ness per slot (vanilla `_d`, `_n`, `_e`, `_m`, `_g`, `_sk`, `_b`, `_s`; PBR albedo, `_g`, `_rmaos`, features) | Stage B SRV formats per object | Format name per slot vs the DDS file's DXGI format. Does the engine pass the file format through, or force UNORM/SRGB views? |
| Q2 | Ambient / DALC values and their relation to `AmbientSH*` | PerGeometry.DirectionalAmbient + SharedData.AmbientSH* | Both. Reconstruct the six colours. Check that `mul(M,(n,1))` equals SH Unproject for ±axes |
| Q3 | Directional light: PerGeometry vs SharedData copies; direction space for **unskinned MSN** meshes (world or model?) | Default static vs an MSN static if any (eye/head are skinned) | `DirLightDirection` in b2 vs b5. If they differ for an object, the engine rotates lights into model space |
| Q4 | Runtime bits (cb4) per object vs NIF flags | `Permutation.PixelShaderDescriptor` + NIF SLSF1/2 dump (NifSkope) | Table: NIF flag → bit present? Settles VC, Specular, Soft/Rim/Back, AmbientSpecular, DoAlphaTest sources |
| Q5 | Does `SpecularColor.xyz` include NIF Specular Strength? Is `SpecularColor.w` the raw Glossiness? | PerMaterial on the Default object | Compare with NIF `Specular Color`, `Specular Strength`, `Glossiness` |
| Q6 | `EnvmapData.x/y`, `MaterialData.x/y` (LOD fades) | EnvMap object | Values. Expect `y = 1` when `_m` exists |
| Q7 | `EmitColor` / `emissiveMult` vs NIF Emissive Color / Multiple | Glow object | Values |
| Q8 | What TruePBR binds where; PBRFlags; params; the technique a coat (type 11) PBR mesh gets | PBR objects (plain + coat) stage B | t0/t1/t4/t5/t6/t9/t12 resources and formats; `PBRFlags`, `PBRParams1/2`, `MultiLayerParallaxData` vs the PGPatcher JSON |
| Q9 | Skin path: what is bound at t2/t3/t4/t12/t71/t72/t74 for FaceTint/SkinTint, MSN and tangent | Head and body draws (as-shipped capture) | Resources + `FeatureData.skinData`, `SkinPerGeometry` |
| Q10 | Facegen tint (t3) and detail (t4) origin | Head draw | Formats and dims (engine-generated tint mask?) |
| Q11 | Deferred composite inputs | The composite Dispatch | SRV list, which are null, defines; MainRW before/after at an object pixel |
| Q12 | GBuffer encodings | RT formats (stage B), `GBuffer.hlsli` | RT formats; check EncodeNormal vs the stage-D trace |
| Q13 | Vertex stream ↔ NIF arrays (TBN source, packed w components, bone index ×765.01) | Stage C VB dump | Map each attribute to NIF `tangents`/`bitangents`/`normals` |
| Q14 | Dead `SNOW` branches for objects (bit 21 stripped) | Snowy object, if any | Confirm no `SNOW` in the `Defines:` marker |
| Q15 | Point lights in PerGeometry when LLF is off | Frame 2 | `PointLightPosition/Color`, `NumLightNumShadowLight` |

### 5.4 Making the preview reproduce a capture (golden test)

1. **Fixture.** For each object, stage B/C produce one folder `golden/<cs-version>/<object>/` with:
   - `state.json`: all cbuffers, sampler states, RT formats;
   - the bound textures as DDS;
   - `mesh.vb/ib` + layout;
   - the 8 MRT crops and composite MainRW crops (EXR or raw float32);
   - `trace_*.json` from stage D.
2. **Replay in the preview.** sdc's renderer loads the *same NIF* (geometry and UVs) but takes `World`, `ViewProj`, `Bones`, every PS cbuffer and the textures from the fixture. Its own material extraction is not used here. This isolates **shader math**.
3. **Diff per MRT first.** Compare Diffuse (RT0), Specular (RT4), Albedo (RT3), Reflectance (RT5), NormalGlossiness (RT2). Use a mask of the object's pixels (depth/stencil from the capture). Metrics: max and mean abs error, plus % of pixels > 1/255 (RGBA8) or > 1e-3 relative (float). Then diff the composited MainRW.
4. **Diff material extraction.** Run again with the preview's own NIF → cbuffer/texture extraction. Differences now come from material mapping (Q4–Q8), not math.
5. **Unit tests from traces.** The stage-D `DebugPixel` values become scalar asserts for individual functions (facegen tint, MSN decode, envmap, PBR F0).
6. **Version pinning.** Golden sets are keyed by CS version. A CS update means re-capturing with the same save and settings profile, then diffing fixture JSON to see what changed.

---

## 6. Open questions (only a capture or the owner can settle these)

1. **Per-slot view formats (Q1).** Does Skyrim create SRVs in the file's DXGI format, so a `BC7_UNORM_SRGB` vanilla `_d` would be hardware-linearised and therefore darker? For PBR, CS's math requires sRGB-decoded albedo/emissive. Only `_m` = `BC7_UNORM` is confirmed [Capture]. This directly constrains the converter's sRGB policy for vanilla vs PBR diffuse.
2. **Engine descriptor sources (Q4).** Exactly which NIF flags set VC, AmbientSpecular, DoAlphaTest, ProjectedUV, the envmap technique (does type 1 also need SLSF1 bit 7?) and Glowmap (does type 2 also need SLSF2 bit 6?). BodySlide gates both on type + flag [Claim]. CS cannot answer; the decoder is in the exe.
3. **cb4 masking.** Confirmed in source (`Hooks.cpp:180`). Still needed: confirm in the capture that `Defines:` markers list runtime bits only, so scripts don't misread them.
4. **Unskinned MSN lighting space (Q3).** `LH:1659` uses the texture normal as the world normal with no World rotation. Either the engine supplies model-space light vectors for such draws, or a rotated unskinned MSN mesh is lit wrong in vanilla too. Capture an MSN static if one exists.
5. **TBN vertex packing (Q13).** NIF tangent/bitangent naming is swapped (BodySlide note). Settle which array the engine uploads as BINORMAL0 before the preview builds TBNs.
6. **Installed CS vs local source.** The capture shows `PerMaterial` cb1[15] (local source declares up to c17) and PhysSky SRVs t61/t112 that don't exist locally. Pick the CS version the preview targets; owner decision.
7. **Skin/Hair default-on BRDF replacements.** Should the preview's default mode match "CS as shipped" (Advanced Skin + Hair Specular on) or vanilla-in-CS? Owner decision. The protocol captures both.
8. **Dynamic Cubemaps parity.** Is DC a target? If yes, the preview needs a cubemap-irradiance stand-in and the composite's reflectance path.

---

## Sources

**CS, local checkout** (`F:\ai\dds\nifparseref\skyrim-community-shaders-dev`):
- `src\ShaderCache.h:584-632` (techniques, flags).
- `src\ShaderCache.cpp:133-162` (defines), `2932-2938`.
- `src\State.cpp:186-187, 241-243, 923-994, 1078-1190`; `src\State.h:250-385`.
- `src\Hooks.cpp:158-180`.
- `src\Deferred.cpp:252-449, 584-700`.
- `src\TruePBR.cpp:680-1060`; `src\TruePBR\BSLightingShaderMaterialPBR.h:5-62`.
- `src\Features\{IBL,Skin,LinearLighting,HairSpecular,RenderDoc}.{h,cpp}` (lines cited inline).
- `package\Shaders\Lighting.hlsl` (lines cited inline).
- `package\Shaders\Common\{Permutation,SharedData,FrameBuffer,Color,LightingEval,LightingCommon,ShadowSampling,Skinned}.hlsli`.
- `package\Shaders\DeferredCompositeCS.hlsl`, `ISSAOComposite.hlsl`, `ISLightingComposite.hlsl`.
- `features\*\Shaders\**\*.hlsli` (registers).
- `.github\configs\shader-validation.yaml` (bit ↔ define evidence).

**nifly:** `F:\ai\dds\nifparseref\nifly-main\include\Shaders.hpp:48-118` (SLSF1/SLSF2 bits). nif.xml `F:\ai\dds\nifparseref\nifxml-master\nif.xml:6432`.

**BodySlide** (`BS`), via subagent read with file:line, tagged [Claim] above:
- `res\shaders\default.{vert,frag}`, `sk_truepbr.frag`, `pbr_common.glsl`;
- `src\render\GLSurface.cpp`, `GLShader.cpp`, `GLMaterial.cpp`;
- `src\files\ResourceLoader.cpp`.

**TruePBR-Manager** (`TPM`), via subagent read, [Claim]:
- `src\renderer\PBRShader.hlsl`, `Common\*.hlsli`, `ToneMapShader.hlsl`, `D3D12Renderer.cpp`;
- `JsonExporter.cpp`, `PBRTextureSet.cpp`, `SPEC.md`, `README.md`.

**Existing workflow:**
- `F:\ai\dds\tools\rdc-scripts\s8_stage1.py`, `s8_stage2.py`;
- `F:\ai\dds\tools\rdc-out\stage1.log`, `stage2.log`, `ps_ResourceId_17693.txt`;
- `F:\ai\dds\converter\out\s8-renderdoc.md`;
- `F:\ai\dds\converter\docs\DECISIONS.md:339-360`.

**Conventions:** `F:\ai\dds\research_notes\Skyrim DDS converter project plan\skyrim_conventions.md` §3 (line 168), §8 (line 567).

**Analysis helper (scratch, not a deliverable):** `C:\Users\Curti\AppData\Local\Temp\claude\F--ai-dds\f6e60693-b887-4019-b7a9-84fda81ca99a\scratchpad\decode.py`.
