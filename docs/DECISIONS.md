# Decisions record

Answers from the project owner (2026-10-08) to the open questions in
`F:/ai/dds/reports/Skyrim DDS converter project plan.md` §12. Verbatim where possible.
Items marked **[Residual]** are places where the measured data qualifies the answer; see the note.

## Scope clarification

- The tool is for **texture artists** converting a folder of source images (PNG, TGA, TIFF, EXR, …) or a single
  image to DDS. It is **not** a game-folder DDS→DDS "fixer". DDS input is secondary.
- Suffix inference may propose the category, preset and defaults, but the **user always decides** how an image is
  treated (category, encoder, settings).
- Future extension: a `truepbr.json` generator in the spirit of https://github.com/jiayev/TruePBR-Manager (credit).

## Answers

| # | Question | Answer |
|---|---|---|
| 1 | Two-channel maps beyond `_flow` | Only `_flow` by default. BC5 stays selectable for niche cases ("an rmaos with an all-black AO channel") and future uses. |
| 2 | BC4/BC5 header form | Confirmed: SE loads both legacy `BC4U`/`BC5U` FourCC and DX10 `BC4_UNORM`/`BC5_UNORM`. Must be **unsigned**. `ATI1`/`ATI2` not seen. |
| 3 | BC6H emissive/sky | Usable **under Community Shaders** (TruePBR-Manager defaults emissive to BC6H; no issues seen). **Not usable in vanilla.** |
| 4 | `_SRGB` in vanilla | Confirmed: `_SRGB` formats are not usable with vanilla shaders. |
| 5 | `_g` / `_f` colour space | Yes: **any colour data** in the PBR set is `_SRGB`-tagged. |
| 6 | Normal length | **Renormalise**, unless an `_rmaos` (user also wrote "rfaos") map is present in the same texture set, in which case **bake variance into roughness**. |
| 7 | Small-texture BC7 | No crash encountered or heard of. No restriction. |
| 8 | Alpha-255 enforcement | **Remove the alpha channel completely** from the image before any mipmap or encoding step. **[Residual]** see note A. |
| 9 | 254 in `_m` | Not a concern; a plain `_m` is written with no alpha. **[Residual]** see note A. |
| 10 | Cutout coverage reference | Default alpha-test threshold **127**; common range **5–200**. Alpha blending is often mixed with alpha testing, so **cutout vs blend is a per-image selection**. |
| 11 | BC1 decoder model | Unanswered. Proposal: default `cBC1Ideal`, expose option. |
| 12 | Linux | Not a concern. **Windows only.** |
| 13 | AlphaBloom | **Rebuild** the Auto Coverage idea in the new engine, **with credits**. |
| 14 | Rust vs C++ | Proceed with Rust; confirm after spikes S3–S4. |
| 15 | Reserved-header hash | Deferred; sidecar first. |
| 16 | Code signing / AV | Packaging phase. |
| 17 | Scalar bc7e fallback | Spike S4. |
| 18 | Isolated batch workers | Measure during Phases 3–4. |
| 19 | 16K previews | Option. **Default max 8192**; allow 16384 as a setting. |

## Note A — decoded alpha after "remove alpha" (measured, bench round 1)

Removing the alpha channel fixes **stray alpha** (the 1–17 dB loss). It does **not** make the *decoded* alpha 255:
on RGB-only input, bc7e decoded alpha **254** on 28% of pixels (ISPC 15%, texconv 2.4%, Compressonator 1.1%;
NVTT 0%), because the opaque mode-6 path picks the p-bit for RGB only. Consequences:

- CS complex-material detection (smallest-mip alpha < 251/255 ⇒ complex): 254 is harmless.
- PGPatcher's "≥50% non-white alpha" rule: bc7e's 28% is a per-set average and varies by image — remains **[Open]**.
- The engine therefore needs: an alpha policy *Remove* (pre-mip channel drop), a stated decoded-alpha guarantee per
  encoder (bc7e ≥254; NVTT 255; BC1 with 3-colour mode off 255), and a post-encode check that decodes the smallest mip.

## Note B — `_rfaos` (owner clarification)

`_rfaos` = **Roughness, Fuzz, AO, Specular**: same layout as `_rmaos` except the **green channel holds a fuzz mask**
instead of metalness. Used for advanced skin in Community Shaders. Treat it as a sibling of `_rmaos`: linear, data,
alpha = specular, same format defaults, and the same normal-variance-to-roughness bake applies (R channel).

## Note C — example of decoded alpha 254

`F:/ai/dds/bench/out/bc7e_u6/rgb_real/Bricks104.dds` (bc7e `-u6`, source PNG is RGB with no alpha channel) decodes
to alpha 254 on 21.0% of pixels, in 4×4 blocks where mode 6 was chosen. Visualisation:
`F:/ai/dds/bench/out/alpha254/Bricks104_alpha_map.png` (red = 254).

## Note D — exact opaque alpha flag

With the per-profile flag `alpha.exact_opaque` ON (default only for `_m`), bc7e decoded alpha is exactly 255 via the forced p-bit patch; OFF keeps the upstream behaviour (>= 254). Spike S5 measures the RGB cost before widening the default.

## Note E — alpha test and coverage (owner, 2026-10-08)

- Coverage preservation is **on by default for both Cutout and Cutout+Blend**, switchable per image.
- The alpha-test threshold is set **per texture** (default 127, typical 5–200).
- Correction: the earlier spec text "coverage only for Cutout" was introduced by the assistant, not the owner.
- Variance-bake clamp **t = 0.25** (Unity HDRP `NORMALMAP_HIGHEST_VARIANCE` = 0.03125 = 0.25²/2), tunable per preset.

## Note F — BC1, sidecars, name (owner, 2026-10-08)

- BC1 is opaque-only in v1 (no 1-bit alpha). rgbcx 3-colour mode stays on but **never emits transparent-black texels** (equivalent to bc7enc_rdo `-b`), so decoded alpha is always 255.
- BC1 decoder model: **one batch-wide setting, default Ideal (D3D10 formula, no rounding)**; NVIDIA / AMD / IdealRound4 selectable for vendor-specific targets.
- Provenance: **batch manifest by default**; per-file `.dds.json` sidecars optional.
- Tool name: deferred. `sdc` is a working placeholder only.
