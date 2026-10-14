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
- *Correction (2026-10-09, fix batch 41):* bc7e alone does not hold ≥254. On high-entropy opaque RGB (Task 41: 64×64
  white noise, `exact_opaque` off) it picks BC7 mode 4/5 with a channel rotation and decodes alpha as low as 239. The
  encode stage therefore applies the lossless rotated-slot fix (rotated channel's endpoints set to all-ones, RGB
  bit-identical) to **every** BC7 block whose input alpha is 255 throughout, whatever `alpha.exact_opaque` and the
  alpha policy. The guarantee "bc7e ≥254" holds for bc7e plus this fix; with the flag off only mode 6 can decode 254.

## Note B — `_rfaos` (owner clarification)

`_rfaos` = **Roughness, Fuzz, AO, Specular**: same layout as `_rmaos` except the **green channel holds a fuzz mask**
instead of metalness. Used for advanced skin in Community Shaders. Treat it as a sibling of `_rmaos`: linear, data,
alpha = specular, same format defaults, and the same normal-variance-to-roughness bake applies (R channel).

## Note C — example of decoded alpha 254

`F:/ai/dds/bench/out/bc7e_u6/rgb_real/Bricks104.dds` (bc7e `-u6`, source PNG is RGB with no alpha channel) decodes
to alpha 254 on 21.0% of pixels, in 4×4 blocks where mode 6 was chosen. Visualisation:
`F:/ai/dds/bench/out/alpha254/Bricks104_alpha_map.png` (red = 254).

## Note D — exact opaque alpha flag

With the per-profile flag `alpha.exact_opaque` ON (default only for `_m`), bc7e decoded alpha is exactly 255 via the forced p-bit patch; OFF keeps the upstream mode-6 behaviour (>= 254). Spike S5 measures the RGB cost before widening the default.
The flag controls only the mode-6 p-bit constraint (native patch or byte fallback), the part that costs RGB. The lossless rotated mode 4/5 fix runs on every all-255 block with the flag on or off (fix batch 41, see Note A).

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

## Spike S3 — workspace links vendored encoders and reproduces the bench (2026-10-08)

- Result: **PASS** (criterion: builds without ISPC on PATH; rgb_real mean RGB PSNR within 0.01 dB of 50.98; rgbcx BC4 bit-exact on mask_real).
- rgb_real mean RGB PSNR: 50.9771 dB (results.csv mean 50.977); largest per-image |Δ| over all 10 BC7 sets: 0.000000 dB (rgb and alpha PSNR).
- Block payloads byte-identical to the bench: 39/39.
- rgbcx BC4 mask_real: 7/7 bit-exact.
- Toolchain: rustc 1.99.0 (b940084d7 2026-09-28), MSVC 14.44.35207 (VS 2022 Build Tools; cl.exe not on PATH, version read from the install directory), objects from ISPC 1.31.0 (bench build).

## Spike S5b — byte-patch fallback for exact opaque alpha (2026-10-08)

- Result: **FAIL on RGB cost** (criterion: decoded alpha 255 on every texel; per-set mean RGB PSNR within 0.05 dB of plain bc7e u5).
  The test `s5b_byte_patch_gives_255_everywhere_on_opaque_sets` is `#[ignore]`d with this note; the limit was not loosened.
- Method: bc7e u5 at mip 0 on rgb_real, rgb_grad, stray_real, stray_grad (alpha forced to 255 = `remove`), then
  `encoders_sys::bc7_fixup::force_opaque_mode6` on every block (mode-6 blocks get alpha endpoints 127 and both p-bits 1).
- Alpha: **0 texels != 255** on all 18 images (every non-mode-6 block already decodes 255). *Correction (2026-10-09, Task 32 review):* not universal — with perceptual off, opaque noise can pick mode 4 with rotation 1–3, which decodes alpha 239–255; the encode stage now applies a lossless post-encode fix (rotated slot endpoints set to max, RGB bit-identical) in both the native and fallback paths. *Further correction (fix batch 41):* the fix is unconditional for all-255 blocks, with `exact_opaque` on or off and under any alpha policy (Note A).
- Mean ΔRGB PSNR vs plain u5: rgb_real **−2.70 dB** (worst Metal049A −7.76), rgb_grad **−8.41 dB** (worst subtle_dark
  −28.15: 84.01 → 55.86), stray_real −1.02 dB, stray_grad −4.90 dB. Smallest loss: Rock064 −0.07 dB.
- Why: forcing p = 1 moves every p = 0 RGB endpoint up one 8-bit LSB without re-fitting; mode 6 is chosen on 11–99% of
  blocks per image, and near-lossless images lose the most. The patch bounds the extra error to +1 LSB per channel.
- Consequence: the byte patch is only a last-resort fallback behind the native bc7e patch (Task 14, `0001-exact-opaque-alpha`),
  which chooses p-bits with the RGB fit. Full per-image output: `cargo run --release -p sdc-bench -- gate exact-opaque-bytepatch`.

## Spike S5 — smallest-mip alpha check and exact opaque alpha cost (2026-10-09)

- Check implemented (`engine::validate::alpha`); agrees with Note C (bc7e -u6 Bricks104 254 share 20.96%, Note C 21.0%) and NVTT (0%).
- Native patch 0001: flag off bit-identical on 39/39 images; flag on: alpha == 255 on every texel of rgb_real, rgb_grad, stray_* (mip 0), 18/18 images.
  Mean dRGB per set: rgb_real −0.515, rgb_grad −1.020, stray_real −0.078, stray_grad −0.382 dB; worst image rgb_grad/subtle_dark −5.166 dB (84.01 → 78.85).
  Result: **FAIL on RGB cost** (limit 0.05 dB per set; not loosened). `s5c_native_patch_flag_on_gives_255_within_0_05_db` is `#[ignore]`d with this note.
  Why: with both p-bits forced to 1, mode-6 RGB endpoints can only take odd 8-bit values, i.e. 7-bit precision; bc7e re-fits under that constraint
  (and still falls back to modes 1–5 where they win), so the loss is the constraint itself, not a missed re-fit. Near-lossless and dark gradients lose most.
- Byte-patch fallback: mean dRGB rgb_real −2.696, rgb_grad −8.410, stray_real −1.015, stray_grad −4.899 dB. The native patch is better on every image.
- Full chain (Task 38, `cargo run --release -p sdc-bench -- gate full-chain`, per-level 254 shares in target/bench/s5-full-chain.txt): flag off >= 254 at every mip down to 1x1 on rgb_real, rgb_grad, stray_real, stray_grad (18/18 images; 254 share at 1x1: 0% on all, but 100% at 2x2 on Metal049A, 4x2 on PaintedPlaster017 and 4x4 on subtle_dark);
  flag on and BC1 (Ideal and bcdec decoders): 255 at every mip incl. 1x1 on all 18 images; mean dRGB at mip 0 (engine path == S5 codec path, RGB bit-identical): rgb_real -0.515, rgb_grad -1.020, stray_real -0.078, stray_grad -0.382 dB (the 0.05 dB RGB-cost limit stays unmet, see above; recorded, not gated).
- Engine path (load -> preprocess -> quantise -> encode) reproduces the bench bc7e u5 mip 0 bytes: 39/39 BC7 bench images (generic_data, alpha kept) and 14/14 on rgb_real/rgb_grad under `remove`; the quantised level equals the PNG's 8-bit RGBA on 53/53 (`gate engine-parity`).
- Shipped mechanism: native patch when `alpha.exact_opaque` is on (proposed: better than the byte patch on every image; owner confirms at Task 17). Open item §14 #5: dRGB <= 0.05 dB on all opaque sets → do not propose exact_opaque on everywhere (owner decides; Task 17).
  Full per-image output: `cargo run --release -p sdc-bench -- gate exact-opaque-native`.

## Spike S4 — scalar bc7e fallback (2026-10-09)

- Source: https://raw.githubusercontent.com/BinomialLLC/basis_universal/99f52d63aa6799cbdaecfe977111dc5ec3b31d47/encoder/basisu_bc7e_scalar.cpp (basis_universal commit `99f52d63aa6799cbdaecfe977111dc5ec3b31d47`, Apache-2.0),
  SHA-256 `684b106e6037042bee065694e5956a97ba91cf2b2417c1a44217e0087a5e808f`; plus its header `encoder/basisu_bc7e_scalar.h` (same commit), SHA-256 `5395f673cbdb9b4ce5115d18369f36b6002712bf88ac3ce71170cdeace889b79`.
  Self-contained: yes (the .cpp inlines its ISPC-compat shim; only the one header is needed; namespace `bc7e_scalar`). Compiled unmodified with MSVC `/O2 /EHsc /std:c++17` plus `-DNDEBUG` (the ISPC objects use `--opt=disable-assertions`), baseline SSE2 (no `/arch`).
  It lived in `spikes/s4-scalar/` (own `[workspace]`), removed 2026-10-13 (Note Y10); rebuild it from the commit and SHA-256 above.
- Quality (veryslow, linear, rgb_real + alpha_real, 11 images, vs ISPC u5 payloads decoded the same way): max per-image |dPSNR| = 0.0759 dB (rgb_real/Metal049A, alpha channel; its rgb delta is 0.015 dB);
  every other image <= 0.0088 dB; bytes identical on 0/11 images (the port differs from the ISPC source, e.g. a denominator bias, so identical bytes are not expected). Deltas are of mixed sign.
  Against the pass limit of 0.01 dB per image: **fails on 1 of 11 images** (Metal049A), passes on the other 10.
- Speed (u5, rgb_real + alpha_real, 16 threads; three runs each): scalar 0.183 / 0.178 / 0.185 Mpix/s (1 thread), 2.073 / 2.114 / 2.101 Mpix/s (16 threads); ISPC u5 0.872 / 0.831 / 0.888 Mpix/s (1 thread), 8.193 / 8.206 / 7.889 Mpix/s (16 threads)
  → scalar is about 4.8x slower on one thread and 3.8–3.95x slower on 16 threads (limit 4x, so inside it only just, and with the machine otherwise idle).
- Verdict: **do not adopt as drop-in**: it misses the per-image quality limit on one image (0.076 dB vs 0.01 dB) and sits at the speed limit. It works and is close (mean deltas ~0.003 dB), so it could be rebuilt as a
  fallback (from the pinned source above) if the ISPC objects ever fail on a needed configuration; adopting it would need the owner to accept a looser per-image tolerance. Open item §14 #11 stays "not built".
  Reproduce: rebuild the spike from the pinned source, then `cargo run --release -p sdc-bench -- speed-bc7e`.

## Spike S7 — NVTT per-mip feeding (2026-10-09)

- nvcompress NVIDIA Texture Tools 3.2.5. It has no `-h`; running it with no arguments prints the usage, which lists `-nomips`, `-bc7`, `-bc6`, `-production`, `-noalpha` and `-silent` (6/6 yes).
- Method: `rgb_real` Bricks104, Fabric084, Metal049A (first 3), each full chain to 1x1 (33 levels): each level box-filtered in 8-bit (`box_down_rgba8`), saved as PNG, encoded by its own
  `nvcompress -silent -nomips -bc7 -production -noalpha` process, decoded, payloads concatenated. Alpha: decoded alpha 255 at every level, 33/33 (yes).
- Mip-0 PSNR vs the bench's direct `nvtt_production` rows: max |Δ| 0.0000 dB (47.7768 / 53.1727 / 55.4568 identical to four decimals).
- Per-level RGB PSNR of each level against the level fed in (mean of 3 images; not a comparison against anything, there is no direct encode of the downsampled levels): L0 52.14, L1 51.57, L2 51.74, L3 51.74, L4 51.72, L5 52.90, L6 55.89, L7 59.32, L8 72.11, L9 99.00, L10 99.00 dB.
- Per-level overhead (wall time of the process): mean 0.142 s, max 0.180 s over the 18 levels of at most 32x32 texels (encode work negligible there); limit 0.2 s → pass.
  The whole-chain mean (2.41 s per level) is dominated by BC7 Production encode time (mip 0 of a 1024x1024 image 13.9–27.1 s), which is not overhead and is not what the 0.2 s limit measures.
  Per-level wall times (mean of 3): 18.7, 5.0, 1.34, 0.44, 0.22, 0.165, 0.137, 0.144, 0.138, 0.132, 0.135 s for levels 0..10.
- CPU-only BC7 Production (observed with `-profile`): "NVTT Warning: BC7 Compressor: no slow-mode for bc7-gpu, using cpu instead" — `-production` runs `encode_bc7_cpu()` even with "CUDA acceleration ENABLED" (RTX 4090 present),
  and the output is byte-identical with `-nocuda` (SHA-256 equal). It is multi-threaded: 191 s CPU for 14.7 s wall on 1024x512 (about 13 cores busy), roughly 0.036–0.07 Mpix/s.
  The ~0.14 s fixed per-process cost is CUDA context creation (`nvtt::Context()` 0.110 s): with `-nocuda` a tiny level takes 0.015 s (5 runs, 0.015–0.018 s), so the backend should always pass `-nocuda` for BC7 Production.
- Not tested: the BC6H half (`-bc6` on RGBA16F levels) is deferred to M5 where the NVTT backend is built; only the flag's presence is confirmed.
- Verdict for open item §14 #10: **one `-nomips` process per level** works and meets every pass criterion; the engine can build the chain itself and feed NVTT level by level (with `-nocuda`). Caveat: Production BC7 is slow (mip 0 dominates, about 14–27 s per 1 Mpix), so it is a quality option, not a default.
  Reproduce: `cargo run --release -p sdc-bench -- s7-nvtt` or `cargo test --release -p sdc-bench --test s7 -- --ignored --nocapture`.

## Spike S7b — NVTT BC6H per level and DDS input (2026-10-13)

M5 spec §8, NVTT 3.2.5 at the default install path (RTX 4090 present). Reproduce: `cargo run --release -p sdc-bench -- s7b-nvtt`
or `cargo test --release -p sdc-bench --test s7b -- --ignored --nocapture`. Report: PASS.

1. **DDS input: yes, but only with a legacy header.** `rgb_real/Bricks104` mip 0 (1024x512) as B8G8R8A8 written by `dds_bytes`:
   with the DX10 header (DXGI 87) nvcompress reads garbage (RGB PSNR 5.05 dB; `nvddsinfo` shows a valid header, so NVTT's
   loader just does not decode uncompressed DX10 formats). With the legacy header (`ResolvedHeader::Legacy`, BGRA masks) the
   BC7 payload is byte-identical to the PNG feed (RGB PSNR 47.7768, same as S7's direct encode). For RGBA16F the DX10 header
   also reads as garbage (peak ratio 0.0000); a legacy header with FourCC 113 (`D3DFMT_A16B16G16R16F`, `DDPF_FOURCC` only,
   payload unchanged) works. EXR written by the `image` crate is refused ("Error opening input file"); Radiance HDR works but
   is RGBE (lossy) and is not needed.
2. **BC6H per level: yes, every level to 1x1.** First 3 `hdr_real` images (1024x512), each level box-filtered in f32
   (`box_down_rgba16f`) and fed as legacy RGBA16F with `-silent -nomips -bc6 -production`: 33/33 levels have a payload of
   exactly `ceil(w/4)*ceil(h/4)*16` bytes and a DX10 header with DXGI 95 (`BC6H_UF16`; the bench's unsigned decoder agrees).
   Mip 0 equals the bench's direct `bc6_nvtt_production` rows: mPSNR 41.6678 / 58.5623 / 52.6373 dB (Δ 0.0000), peak ratio
   1.0060 / 0.9514 / 0.9957 (all ≥ 0.95; kloofendal is NVTT's own 0.9514 in results2.csv too). No ISPC fallback for small
   levels is needed, so `ENC-NVTT-PARTIAL` is not added.
3. **`-nocuda` with BC6H: byte-identical** on 33/33 levels. Per-level wall time for levels of at most 32x32 (n=18): 0.151 s
   with CUDA, 0.016 s with `-nocuda`; mip 0 1.25–1.85 s with `-nocuda` vs 1.41–2.00 s with CUDA; whole chains 6.55 s vs
   10.91 s. CUDA buys nothing for BC6H Production either.
4. **Banner and exit codes.** First line `NVIDIA Texture Tools 3.2.5 - Copyright NVIDIA Corporation 2007 - 2024`; the
   version is the token after the prefix. The no-argument run **exits 1** (the probe must not require success). A truncated
   input exits 1 with `Error opening input file '<path>'.` and writes no output.

Choices:
- **NV4:** `-nocuda` always, for BC6H as for BC7.
- **NV5:** each level is fed as a DDS with a **legacy header**: BC7 as B8G8R8A8 via `dds_bytes(.., header: Legacy)`, BC6H as
  RGBA16F with FourCC 113 (the engine's writer has no legacy RGBA16F form, so the NVTT module writes that header itself).
  Never the DX10 header.

## Phase 0 exit (2026-10-09)

| Gate | Result |
|---|---|
| S3 bc7e u5 rgb_real within 0.01 dB, BC4 bit-exact | PASS (39/39 payloads byte-identical, max Δ 0.000 dB, mean 50.9771; BC4 7/7) |
| rgbcx BC1/BC3/BC4/BC5 bit-exact (+ transparent-black off vs bc7enc -b) | PASS (14/25/12/11; off 14/14) |
| ISPC BC6H veryslow 51.50 dB | PASS (51.4995 dB; 8/8 payloads byte-identical) |
| S5 exact opaque (mip 0): mechanism native (patch 0001), dRGB −0.515 dB rgb_real / −1.020 rgb_grad | FAIL on the 0.05 dB limit; alpha 255 PASS; flag-off bit-identical PASS |
| S4 scalar bc7e | do not adopt (max Δ 0.0759 dB on 1/11; 3.8–4.8× slower) |
| S7 NVTT per mip | viable (mip-0 Δ 0.0000 dB; 0.142 s fixed overhead per process) |
| ISPC regeneration byte-reproducible (Task 3 Step 8) | yes (all objects MATCH, re-run 2026-10-09) |

Owner answers (2026-10-09):
- Confirm Rust (DECISIONS #14): **"Yes, Rust"**.
- `alpha.exact_opaque` default: **"Only _m / env masks"** (S5 cost exceeded the 0.05 dB condition).
- MVP without the variance bake: **"Yes, bake in M4"** — the MVP renormalises `_n` mips; the DECISIONS #6 bake arrives with milestone M4.

## Note G — preset suffix aliases (owner, 2026-10-09)

- A preset may declare **exact suffix aliases** that bind a filename suffix to a canonical sub-type suffix and to that preset's settings, e.g. `rmaosultrahigh → rmaos`.
- Matching is **exact, no separator**: `iron_rmaosultrahigh.png` matches only an alias `rmaosultrahigh`. A preset declaring `rmaosultrahigh` does **not** make `_nultrahigh` match; each alias is declared individually.
- The file is treated as its canonical suffix for grouping and output naming (`iron_rmaosultrahigh.png` → set `iron`, written as `iron_rmaos.dds`).
- The alias's preset may define **any preset fields**, layered like any other preset (on top of the target preset; explicit `--set`/per-file overrides still win). Provenance records the alias.
- A source whose alias and canonical forms collide on the same output (e.g. `iron_rmaos.png` and `iron_rmaosultrahigh.png`) is a duplicate error.
- Clarification (owner, 2026-10-09): an alias is any declared name, not a tag appended to its target. Example: one preset declares `rmaosultrahigh = "rmaos"`, another declares `randomsuffix = "n"`. The `_nultrahigh` example meant only that `_nultrahigh` is not inferred from `rmaosultrahigh`; any preset can declare it (or any other name) with its own target. Matching is the exact declared name in both the preset and the filename.

## Note H — industry suffix vocabulary and channel packing (owner, 2026-10-09)

- **Suffix vocabulary:** common exporter suffixes (Substance Painter/Designer, Quixel, ambientCG, Unreal/Unity) are recognised. `_d` and `_colormap` (and `_basecolor`, `_base_color`, `_albedo`, `_diffuse`, `_color`, `_col`) **can safely be assumed albedo/diffuse** (High confidence for the colour role; target/PBR set still pick `diffuse` vs `pbr_albedo`; alpha meaning is still never assumed). Normal variants (`_normal`, `_nrm`, `_normalgl`, `_normaldx`) map to `_n`. Outputs always use Skyrim names.
- **Channel packing:** raw single-channel maps (`_roughness`, `_metallic`, `_ao`, `_specular`, `_glossiness`, `_height`, `_fuzz`, …) in one texture set are **packed automatically per target** (CS `_rmaos`/`_rfaos` + `_p`; CPM complex `_m`; vanilla `_m` only when a source exists). Raw maps are not written on their own by default; overridable per set.
- **Missing channels:** neutral defaults plus a warning naming each defaulted channel (AO white, metallic black, specular alpha white = TruePBR default). **Do not block when roughness is missing if an AO or metallic map is found** — roughness then takes a neutral default with a warning. [Assistant proposal: default roughness 0.5, configurable in the preset; confirmed by the owner on 2026-10-09, see the last bullet.]
- **Plain `_normal`:** no default green convention — unresolved until a preset/flag/rule states `opengl` or `directx`; `_normalgl`/`_normaldx` resolve automatically.
- **Timing:** suffix vocabulary lands in Phase 1 now; channel packing is its own milestone right after the CLI MVP.
- Owner (2026-10-09): **roughness default 0.5 confirmed.** For every packed channel without a source map, the user can set a **global fill value** — a slider in the GUI and a CLI option (e.g. metallic 255 writes 255 instead of the neutral 0); the neutral defaults above apply only when no value is given. Scope: milestone M2.5.

## Note I — intentional skips do not make the batch exit 1 (owner, 2026-10-09)

Owner answer to the Task 37 review question O1: files skipped on purpose do not count toward exit code 1.
- **Covers:** existing `.dds` inputs next to sources (IN-DDS-SKIPPED), outputs skipped by an explicit `--on-existing skip`, and raw packing inputs (IN-PACKING-INPUT, already exempt).
- **Still exit 1:** unresolved and blocked files, unknown existing outputs (OUT-UNKNOWN-EXISTING), and failures.
- **Reporting:** intentional skips are still reported as `file_skipped` events and counted in the summary.

## Note J — result verbs print to stdout (controller ruling, 2026-10-09, Task 42 review)

The verbs whose report *is* the result print their human report on **stdout**: `inspect`, `validate`, `presets` and `encoders`.
`convert` prints its progress log on **stderr**. With `--json`, stdout carries only JSON Lines for every verb.
`validate` infers the alpha policy from the file: mip-0 alpha ≥ 254 everywhere → `remove`; otherwise the sub-type's alpha-keeping default, or `data`. `--alpha-policy` overrides it. `alpha.exact_opaque` is a promise of sdc's own encoder, so `validate` sets it off whatever the sub-type's preset says (a third-party BC7 whose alpha decodes to 254 passes with the `ALPHA-254` info); `--exact-opaque` asks for the 255 guarantee (Codex-CLI 3). Conversion-time validation is unchanged. For sub-type inference, mip-0 alpha with a minimum of 251 or more is not varying (Note L).
A DDS in a format sdc never writes (BC2, uncompressed, signed) gets header checks only; a clean header gives "not judged", exit 0.

## Note K — Task 44 CLI edge-case rulings (controller, 2026-10-09)

- **Ctrl-C:** the handler is installed before planning (a failed install prints a warning to stderr). The first Ctrl-C cancels cooperatively (exit 1, cancelled files listed in `batch_done`); a second one exits 3 at once. Outputs are written atomically, so an abort never leaves a partial `.dds`; it can leave `.sdc-*.tmp` files in the output folders and a just-renamed output without a manifest record (reported as `OUT-UNKNOWN-EXISTING` next run; `--on-existing overwrite` replaces it). With `--json`, an aborted run's stream ends without `batch_done`; consumers treat that as "aborted". The handler does no I/O (Codex-CLI 1): the first Ctrl-C sets the cancel flag and wakes a helper thread that prints the "cancelling" note (ignoring write errors), and the second calls `std::process::exit(3)` without writing, so a full or closed stderr pipe can neither block nor kill it. The token reaches planning too: a Ctrl-C while planning or during a dry run lists every pending file (not yet loaded, or ready but not started, including files that would have been up to date or kept) as cancelled, exit 1.
- **`--mem-budget 0`** is a usage error (exit 2), not "default"; omit the flag for 50% of RAM.
- **`encoder.bc1_transparent_black = true` with a BC1 output is `CFG-INVALID`** (Note F: BC1 is opaque-only in v1). The setting stays in the schema for the day BC1 alpha is offered; it is inert for other formats.
- **Test-only hooks** (behind the engine cargo feature `test-hooks`, off by default; the `sdc` feature of the same name forwards to it; compiled out of `cargo build --release -p sdc`, which has no `SDC_TEST_*` strings. Engine and CLI dev-dependencies enable the feature, so plain `cargo test --workspace` runs the tests that use them): `SDC_TEST_PANIC_ON=<input file name>` panics that file during conversion; `SDC_TEST_PANIC_ON_PLAN=<input file name>[@load|analyse|resolve]` panics that file while planning, as it enters the named stage (default `load`), to prove per-file panic isolation in `Engine::plan` (final review I1); `SDC_TEST_FAIL_VALIDATION=<input file stem>` adds one `VAL-HEADER` error to that file's validation report; `SDC_TEST_CANCEL_ON=<input file name>` cancels the batch while planning, just before that file loads (a Ctrl-C without a console). `SDC_TEST_FAIL_VALIDATION` exists because no setting reaches an error-severity `VAL-*` code, so exit 4 would otherwise have no deterministic trigger.

## Note L — near-opaque source alpha (owner, 2026-10-09)

Found in the Task 45 real-folder run: PBR normal maps re-exported from BC7 DDS carry alpha 251–255. That is BC encode noise, not data.
- **Rule:** source alpha whose minimum is **≥ 251** (no texel below 251) is treated as opaque. It resolves like constant-255 alpha, so `remove` applies and no LOSS-ALPHA-DATA is raised. An info code `ALPHA-NEAR-OPAQUE` reports the minimum.
- **Below the cutoff:** one texel below 251 keeps the normal varying-alpha rules.
- **Override:** an explicit `alpha.policy` still wins.
- **Why 251:** it matches Community Shaders' 251 opacity gate (Note A).

## Note M — channel packing decisions (M2.5 plan, 2026-10-09)

Defaults for spec §14 #18–#22 and the M2.5 design choices; confirmed by the owner at plan review.

- **M1 (#18) Env mask:** a packed `cpm`/`vanilla` `_m` R is the metallic source, else the specular source. Under `cpm` with neither it is `packing.fill.metallic`, else `packing.fill.specular`, else 0. The `cpm` `_m` height default is 0 (owner 2026-10-10): alpha 0–4 keeps CS complex material on with parallax off; 0.5 ran parallax on a flat height (Spike S8 DXBC).
- **M2 (#19) Raw maps without a recipe** (`_opacity`, specular-only and fuzz-only `cs` sets, height under `vanilla`, AO and fuzz under `cpm`, …) raise `PACK-UNUSED-INPUT` and are not written.
- **M3 (#20) Specular** is copied unchanged into `_rmaos`/`_rfaos` A.
- **M4 (#21) Exit code:** raw inputs reported `IN-PACKING-INPUT` or `PACK-UNUSED-INPUT` do not make the batch exit 1.
- **M5 (#22) Fill values:** field `packing.fill.<role>` (one per §4.4 raw-map role: `roughness`, `metallic`, `ao`, `specular`, `glossiness`, `height`, `opacity`, `fuzz`), an 8-bit code 0–255. CLI `--fill <role>=<0..255>` is an unscoped `--set packing.fill.<role>=<v>` added after every `--set`, so it beats an unscoped `--set` of the same field and loses to sub-type- and file-scoped ones. A roughness fill is used for a glossiness channel as `255 − v` and back. A filled channel has provenance `fill:<code written>` and raises **no** `PACK-DEFAULTED` (the user chose it). The GUI slider lands with M3's settings panel.
- **M6 Two-pass resolve:** `packing.*` of a packed output is read from its profile resolved with a placeholder analysis (the packed image does not exist yet); the real packed image is then assembled, analysed and resolved again for everything else. A rule that matches on `alpha` therefore cannot change `packing.*`. The same holds for `channels.grayscale_source`: the value of that first profile is stored in `PackJob::gray` and used by the plan-time assembly, the provenance, the report and the pipeline's assembly, so the written image is the analysed one even when the final profile's value differs.
- **M7 Set membership:** the packed suffix (`rmaos`, `rfaos`, `p`, `m`) is added to the set's suffixes before any member resolves, predicted from the raw roles present, so `set_has` rules and PBR detection see it. It is predicted even when `packing.enabled = false` turns that recipe off. A raw map taken out of the recipes by a `--set` or rule (for example given its own sub-type) still adds its predicted suffix, so a `set_has` rule can match a set that writes no packed file of that suffix.
- **M8 Job identity:** a packed job's `input` is its first source in R, G, B, A order; the raw jobs a recipe consumes leave `Plan::jobs`. A raw map feeds at most one recipe per target, so no two jobs share an `input`.
- **M9 Pre-packed conflict** is keyed on an image member whose canonical suffix equals the recipe's suffix (`rmaos`, `rfaos`, `p` under `cs`; `m` under `cpm`/`vanilla`). DDS members are never written, so they never conflict. With `packing.prefer = raw` the replaced file is skipped with `PACK-CONFLICT` and counts as an intentional skip (Note I). Exception (never write neither): when the recipe's sources differ in size, the sizes are checked first, the pre-packed file stays a job (no skip, no `PACK-CONFLICT`) and the recipe fails with `PACK-SIZE-MISMATCH`. When `packing.prefer = raw` but the raw sources differ in size, the recipe fails (`PACK-SIZE-MISMATCH`) and the pre-packed file is kept; it converts under the normal never-auto-decide rule (its sub-type is a suggestion, so it needs `--accept-suggestions` or a rule).
- **M10 Profile hash:** `packing` joins every `Profile`, so every output's `profile_sha256` changes once and `--on-existing if-changed` re-encodes every existing output on the first M2.5 run.
- **M11 Failed source:** a recipe with a source that failed to load is `failed` in `set_resolved` and has no job; the batch already exits 1 through that source's `file_failed`. A recipe with a source whose Load was cancelled is `failed` too, reason "cancelled" (the batch exits 1 through `cancelled`).
- **M12 Provenance:** the recipe's sub-type is `recipe:<kind>`; each packed channel is recorded under `pack.r`, `pack.g`, `pack.b`, `pack.a` as `source:<file>:R` (`source:<file>:luma` with `channels.grayscale_source = luma`, `source:1-<file>:R` when inverted), `default:<value>` or `fill:<code>`.
- **M13 Duplicate roles:** with two raw maps of one role in a set, the first by path is used and the others are `PACK-UNUSED-INPUT`.

## Note N — M3a converter GUI decisions (owner and M3a plan, 2026-10-10)

The owner split M3 into M3a (converter GUI), M3b (set editor and PGPatcher JSON) and M3c (texture edits) and approved the M3a design section by section (spec `docs/superpowers/specs/2026-10-10-sdc-m3a-converter-gui-design.md`).

- **N1 Preview size limit 4096 by default** (owner): replaces the 8192 of #19 for the GUI preview only. Values 1024–16384; 16384 only as an explicit setting. The preview encodes only mips at or below the limit; each is byte-identical to the exported mip.
- **N2 `[files]` in `sdc.toml`** (owner): per-texture GUI edits are saved as `[files."<path relative to sdc.toml>"]` entries (any profile field plus `allow = [codes]`). They are their own layer: above preset and config rules and analysis facts, below command-line `--set`. A key matches a file when `rel == key` or `rel` without its last extension `== key`, case-insensitively; packed outputs are keyed without extension. Provenance `project_file`. Only the project config may hold `[files]`.
- **N3 Proceed anyway** (owner): exporting with undecided textures asks "Proceed anyway / Cancel"; proceeding skips the undecided textures and never decides for the user.
- **N4 In-memory project config** (plan): `BatchRequest::project_config_inline` lets the GUI re-resolve unsaved edits; Export saves `sdc.toml` first, so `sdc convert <folder>` reproduces the last export.
- **N5 Skip kinds** (plan): the skip kind of a `file_skipped` event is one of `up_to_date`, `on_existing_skip`, `unresolved`, `blocked`, `duplicate`, `dds_input`, `packing_input`, `unknown_existing`, `pack_conflict`, `packed_source`. In the JSON events the field is named `skip` (Rust: `FileSkipped::kind`).
- **N6 Job key** (plan): every per-file event carries the job key, the output path relative to the output root (`manifest::output_key`). In the JSON events the field is named `job_key` (Rust: `EngineEvent::job`).
- **N7 Manifest sources record role and invert** (plan, M2.5 carry): replaces the `pack.*` provenance comparison in `if-changed`; schema-1 records re-encode once.
- **N8 Preview API** (plan): `Session::preview(&ResolvedJob, max_size, &CancelToken)`.
- **N9 Typed unresolved needs** (plan): `JobStatus::Unresolved.needs` lists `subtype`, `alpha_meaning` and `green_convention`.
- **N10 No bundled fonts** (licence ruling): sdc-gui ships no fonts. eframe is built without `default_fonts` and the GUI loads Windows system fonts (Segoe UI, Segoe UI Symbol, Segoe UI Emoji, Consolas) at run time, because egui's bundled fonts are OFL-1.1/UFL-1.0.
- **N11 DDS formats without an engine `Format`** (plan): a DDS input in a format the engine has no `Format` for (uncompressed, BC2, signed BC6H) shows "cannot be previewed" in M3a. **Ended in M6 (Note S5).**

## Note O — roadmap additions (owner, 2026-10-10)

- **O1 Uncompressed DDS** (owner): sdc writes uncompressed DDS (`rgba8` as B8G8R8A8 UNORM/SRGB, `r8`, `rg8`, `rgba16f`) and reads the uncompressed layouts listed in spec §1 for inspect, validate, re-encode and GUI preview. Scheduled in M6; it ends the M3a limit N11. **Done in M6 (Note S4, S5).**
- **O2 HDR check before BC6H on emissive** (owner): `pbr_emissive` defaults to BC6H_UF16 only when the source is float **and** peaks above 1.0. Every other source defaults to BC7_UNORM_SRGB. This overrides spec §4.1 and the DECISIONS #3 default for emissive; BC6H stays available by override. Scheduled in M6, and depends on the float-source transfer split. **Done in M6 (Note S3); `hdr_sky` open.**

## Note P — M3b set editor and PGPatcher JSON decisions (owner and M3b plan, 2026-10-10)

The owner approved the M3b design section by section (spec `docs/superpowers/specs/2026-10-10-sdc-m3b-set-editor-design.md`); the plan refined it as follows.

- **P1 Output paths, not prefixes** (plan): the engine takes `output_paths` (explicit paths per source file or per packed recipe) instead of a per-set prefix. The ownership rule (§3.3) can place a shared output at another set's prefix; explicit paths say that, and the engine stays free of PGPatcher concepts.
- **P2 Delete and order are GUI-only** (plan): the engine's `SetEdit` has Create, Rename, Move, Remove. Deleting a created set drops its edits (its files regroup automatically); order lives in `sdc-project.toml`.
- **P3 Only created sets can be deleted** (plan): an automatic set would regroup at once.
- **P4 Vanilla/CPM borrowed slots are file copies** (plan): copied after the run, listed in the report, not tracked by the manifest.
- **P5 A PBR set has a `pbr_albedo` diffuse** (plan; Note R-review C1 adds `skin_albedo`): the `set_resolved.pbr` flag is path-based and misses user-built sets.
- **P6 Slots are PGPatcher's 1-based `slotN`** (plan): `_cnr`/`_f` share slot 7 (TX06), `_s` is slot 8.
- **P7 JSON-only edits do not re-plan** (plan): match, rename, set and `[files]` edits do.
- **P8 A bad `sdc-project.toml` makes sets and JSON read-only** (plan); `sdc.toml` editing continues.
- **P9 Undo keeps document snapshots** (plan), 200 steps; Duplicate source records the copy and its SHA-256.
- **P10 Colours are stored 0–255** (owner: edited as 0–255) and written as 0–1 with 4 decimals; `emissive_color` gets alpha 1.0 (owner).
- **P11 Borrowed slot paths are written with `textures\`** (plan).
- **P12 In-tree schema validation** (plan): no new dependency; a test fails if the vendored schema uses a keyword the validator lacks.
- **P13 Invalid path characters block export** (plan) rather than being rewritten: a changed path is not where PGPatcher looks. Amended (owner, 2026-10-14): a leading `\` or `/` typed into a slot path, `match1`–`match8` or `rename` (set editor or output name) is dropped as it is stored, since every one of these paths is relative and the separator changes nothing. `..`, a drive letter and invalid characters still block, a value already stored with a leading `\` still blocks, and `path_contains` keeps its leading `\` (it means "a whole folder name").
- **P14 GUI = CLI holds for non-PBR sets only** (owner: the JSON is GUI-only): a PBR set on CS is written under `textures\pbr\` by the GUI alone.
- **P15 "Not in a set" is a clickable count** in the Textures header (plan), not an automatically switched filter.
- **P16 The PGPatcher `default` block is fixed** (owner, 2026-10-10): in schema order `emissive` = `false`, `parallax` = `false`, `displacement_scale` = 1.0, `specular_level` = 0.04, `roughness_scale` = 1.0, `subsurface` = `false`. Users cannot edit defaults (no Defaults card, no `[defaults]` in `sdc-project.toml`; a hand-written one is ignored). Entries write `emissive`/`parallax`/`subsurface` only as `true`, and omit the three numbers when unset or equal to the fixed value; everything else comes from the set.
- **P17 Glint and fuzz switches are GUI-only settings keys** (plan, execution ruling): `glint_on` / `fuzz_on`, never written to the JSON. A bool at `glint`/`fuzz` can't coexist with the `glint.*`/`fuzz.*` sub-table in TOML.
- **P18 A set whose files are all gone still shows, as missing** (plan, execution ruling): its entry can be deleted. This extends P3. A borrow from a set with nothing in that slot is shown as missing too.
- **P19 Moving, sharing or duplicating onto a filled slot asks "Replace / Cancel"** (spec §3.1; rename dialog: plan, execution ruling). Rename is a dialog, not inline.
- **P20 Export refuses a plan that doesn't match the current sets** (plan, execution ruling), with an actionable message. The JSON name is a single file name: no separators, no `..`, no reserved Windows device names, no trailing dot or space. It is checked only when there are PBR entries.
- **P21 Read-only `sdc-project.toml` at export** (plan, execution ruling): the JSON is still written but its SHA-256 is not recorded (a note says so), so the next export asks before overwriting. A byte-identical file on disk is never prompted.
- **P22 Vanilla/CPM copies are skipped, with a note,** when the owner's output didn't export Done/UpToDate (plan, execution ruling).
- **P23 Fuzz and coat normal grey each other out** (owner: a set cannot hold both `_cnr` and `_f`; the greying detail is a plan execution ruling): fuzz is greyed when the set's coat slot (`_cnr`) is filled, and `coat_normal` is greyed when the fuzz slot (`_f`) is filled. A switch that is already on stays enabled so it can be turned off; export blocks the clash.
- **P24 The packed `cpm` `_m` height default is 0** (owner; Task 3b, Note M1 amended): see M1 above.
- **P25 LOSS-ALPHA-DATA is narrower** (owner, 2026-10-11: "the alpha warning is too broad"): a varying alpha dropped from a `pbr_normal` (vanilla specular, which PBR normals do not use) is the Info `ALPHA-PBR-NORMAL-DROPPED`, not a block; and an `alpha.policy = "remove"` the user set explicitly (any layer but a built-in default, the same test as the alpha-meaning check, spec §5.2) raises neither code, since the user already decided. Every other discarded varying alpha still blocks.
- **P26 PGPatcher JSON paths that never match block export** (owner, 2026-10-11; gap found by the NIF team, `nifparseref/plan/p1/01-pgpatcher-truepbr-rules.md` §1.3). PGPatcher prepends `\` to `match_diffuse`/`match_normal` and matches them against paths relative to `textures\`, so a value that starts with `textures\` (any case) never matches; it now blocks with "remove textures\" instead of exporting a dead entry. `match1`–`match8` are unaffected (PGPatcher prepends `textures\` itself). PGPatcher also never converts `/`, and the writer writes values as typed, so a `/` in `match_diffuse`, `match_normal`, `match1`–`match8`, `rename` or a `slotN` path blocks too (owner chose block over auto-convert, consistent with P13). Output paths, set names and the derived match are unaffected (sdc builds those with `\`).
- **Owner, in the design conversation:** `emissive`, `parallax`, `subsurface` are binary and written only as `true` (the fixed default block supplies `false`); `hair` and `multilayer` binary, written only when on; `lock_*`, `vertex_colors`, `zbuffer_write` three-state; mutual exclusion greys (hair excludes multilayer, glint, fuzz, emissive, parallax, subsurface; multilayer excludes subsurface; multilayer/glint/fuzz exclude each other); a set cannot hold both `_cnr` and `_f`; a feature on without its texture blocks export (no lock quick fix); `delete` and hex colour entry are later rounds; Opaque sits on the Diffuse card.

## Phase 2 exit — CLI MVP (2026-10-09)

- `sdc-bench --gates`: PASS, exit 0, 13/13 gate reports (target/bench/mvp-gates.txt). Exact opaque gates alpha == 255 and flag-off bit-identity; ΔRGB is info (S5).
- Workspace tests: 472 passed, 0 failed, 4 ignored, by design: the two S5 ΔRGB tests (S5b byte patch, S5c native patch), the S7 NVTT test (needs `nvcompress`, about a minute) and the 512 MiB decode-limit test (allocates GiBs).
- Header spot check: nvddsinfo reports the format, size and mip count sdc wrote (BC6H DX10 DXGI 95, BC7 sRGB/UNORM, BC4). texconv decodes every sampled file without error.
- Real folder: 110 PNG sources sampled from the owner's Faultier's PBR Clutter, Faultier's PBR Landscapes 4k and Vanilla Table Replacers Complex Material, capped at 2048 (F:/ai/dds/mvp-test).
  - First run: 85 converted. It surfaced the near-opaque normal-alpha false block, fixed as Note L.
  - Re-run: **97 converted**. 13 skipped, all `pbr_albedo` with SUG-LOW-CONFIDENCE (medium confidence, alpha meaning unconfirmed; 6 also carry real alpha). 0 failed.
  - `sdc validate` over the 97 outputs: 97 ok, exit 0.
- Defects found by the MVP work and fixed:
  - Rotated BC7 mode 4/5 alpha below 254 without exact_opaque.
  - A DDS next to its PNG counted as a duplicate.
  - validate judged stored alpha as removed.
  - Near-opaque source alpha (Note L).
  - Codex B and CLI findings.
- In-game spikes S1, S2, S6, S8: pack and checklist ready (docs/spikes/in-game-checklist.md, target/spike-src). Results pending from the owner.
- Next milestones: M2.5 channel packing (incl. per-channel fill values), M3 GUI, M4 variance bake, M5 NVTT, M6 presets/inputs (incl. resize), M7 CI, M8 packaging.

## Final review (2026-10-09)

Whole-branch review of `feat/phase0-2` (59cf60c): 0 Critical, 8 Important, 16 Minor; no wrong bytes or exit codes on any shipped path. Controller rulings:
- **Fixed now:** I1 planning had no per-file panic isolation (a decoder panic ended the whole batch); I4 a float source written to an LDR format was silently mis-decoded (interim blocking `IN-FLOAT-LDR`, plus `LOSS-CLAMP-HDR` for values outside [0, 1], until the M6 transfer split); I7 plan-time decisions were applied to whatever the file held at run time (size and mtime are recorded at plan time and compared before Load).
- **Docs:** I2 the fill-value requirement (Note H, last bullet) is now in spec §3.2, §6.4, §14 #16/#22 and the M2.5 scope; I3 carried items and deferred minors live in the plan, per milestone; I8 the spec is re-synced with the code and these notes.
- **Deferred to M3 entry (one `EVENT_SCHEMA` bump):** I5 per-file event identity, typed `file_planned.job` and a skip kind; I6 per-file allow and an exact-path scope. Cost if wrong: one schema bump at M3.

## Spike S1 — not run; owner answer (2026-10-09)

- The owner skipped the in-game check: PBR `_f` and `_g` are sRGB only, with BC6H as the option for an emissive `_g`.
- The shipped defaults already match (`_g` and `_f` = `BC7_UNORM_SRGB`, BC6H by override). Nothing changes.

## Spike S2 — small BC7 textures do not crash (2026-10-09)

- Result: **PASS** (criterion: no crash in about 10 minutes).
- Owner test: in the anssestudio mod (`mods/s2/textures`), 66 textures were replaced by s2 outputs: 16–128 px BC7 with full chains, 17 at 16, 17 at 32, 16 at 64, 16 at 128. Seven `_n` maps were re-encoded smaller (16–64 px BC7). `_e` cubemaps and one cubemap without the suffix were kept. Mapping: `out/s2-swap/mapping.txt` (git-ignored).
- The Studio cell with those textures also loaded and rendered during the S8 session with no crash.
- Owner note: the crash reports that blamed small textures were most likely misattributed.
- Consequence: no small-texture fallback (DECISIONS #7 stands).

## Spike S6 — plain `_m` with the exact-opaque flag off (2026-10-09)

- Result: **7 of 28 files at 40% or more**, so `ALPHA-EXACT-OFF-M` is now **blocking** (spec §14 #9). Owner OK on 2026-10-09.
- Method: done offline, without loading the game. Sources were 28 real plain `_m` from 28 of the owner's mods, decoded read-only to PNG (`out/s6-src/SOURCES.txt`). They were converted as BC7 with `alpha.policy=remove` and `alpha.exact_opaque=false`, and the decoded mip-0 alpha was measured.
- Share of alpha ≠ 255 per file: from 1.57% to 57.95%, about 27% on average. The worst are `frozenmarshice_m` 57.95%, `hair_acc_m` 57.19% and `Dibella_m` 51.02%. Per-file list: `out/s6-results.txt`.
- Consequence: turning the flag off on a BC7 `_m` now needs `--allow ALPHA-EXACT-OFF-M`. The flag is on by default for `_m`, so default runs are unaffected. BC4 and BC3 `_m` never raise the code.

## Coverage remap — owner keeps the curve (2026-10-09)

- RULINGS #50: the mip coverage step keeps the monotone piecewise-linear remap (0 → 0, a → T, 1 → 1), not Castaño's uniform t/a scale.
- The owner decided after viewing `F:/ai/dds/coverage-compare` (synthetic and real 4096² sources, mips 1024 and 128).
- Spec §5.5 marks it decided.

## Spike S8 — CS complex-material gate, read from the shader (2026-10-09)

- Result: **the 251/255 gate is exact**. Evidence comes from the shader, not from looking at parallax, which can't be judged on a flat test map.
- Method:
  - One frame captured in game with Community Shaders' RenderDoc feature (RenderDoc 1.43; replayed with portable RenderDoc, which auto-updated to 1.46).
  - Scene: `DLC1VampireCastleGuildhall`. The Comfy Coffins `SCoffin01` uses `_m` = s8 `heightmask_m`, with a Dragonborn septim pile (`_m` = s8 `m_250_m`) on top. Both were placed with `player.placeatme` through DevBench.
  - Scripts: `F:/ai/dds/tools/rdc-scripts/s8_stage1.py` and `s8_stage2.py`. Output and the full DXBC: `F:/ai/dds/tools/rdc-out/`. Write-up: `out/s8-renderdoc.md`.
- CS settings at both draws: `EnableComplexMaterial` 1, `EnableParallax` 1, `EnableParallaxWarpingFix` 1.
- The decision in the CS Lighting pixel shader:
  - It samples `TexEnvMask` at LOD 15, i.e. the smallest mip.
  - `grey` = every pair of R, G, B differs by less than 4/255.
  - `black` = R, G and B all below 4/255, and 4/255 < alpha < 251/255.
  - complex = not (`grey` and not `black`), and alpha < 251/255 (0.984314).
  - Parallax then runs only on texels whose full-resolution alpha is in (4/255, 251/255).
- Decoded smallest mips in the capture:
  - `m_250_m`: (80, 160, 40, 250) at every level → complex on.
  - `heightmask_m`: (0, 0, 0, 126) → complex on, through the black exception.
- For the s8 set (RGB 80/160/40, not grey): 245, 248 and 250 → on; 252 and 255 → off. Finn's Lute (`m_252_m`) was placed but not drawn in the captured frame.
- Consequences:
  - §14 #7 is closed: `complex_m_warn_alpha` stays 247 (4 codes of headroom below the gate).
  - `VAL-M-COMPLEX-MARGIN` now says how many codes below the gate the file is, or that complex is off at 251 and above. Its data carries `smallest_alpha` and `complex`.
  - `VAL-M-GRAYSCALE` now applies the shader's rule to the smallest mip. Before, it tested mip 0 for over 95% of texels with spread ≤ 4 and a mean above 25. As a result:
    - a dark grey such as (10, 10, 10) is now flagged;
    - only RGB below 4 with a height in range is exempt;
    - a coloured base that averages to grey at 1×1 is flagged.
  - The cpm `_m` constant height 0.5 (128) is below the gate. The M2.5 packer must still keep the packed RGB from being grey at the smallest mip, for example a constant-filled env mask. `VAL-M-GRAYSCALE` catches it. (superseded by M1, owner 2026-10-10: 0)

## Owner checks pending

Open owner confirmations after Phase 2. None blocks the merge; each has a shipped default.
1. **Coverage remap shape (Task 30).** **Closed:** the owner keeps the curve (Coverage remap entry above).
2. **Colour-term output names (spec §14 #17).** Exporter colour terms (`_basecolor`, `_albedo`, …) are written `<base>.dds`; `_d` keeps `<base>_d.dds`.
3. **`_normalgl` / `_normaldx` precedence.** They state the green convention as a fact that beats presets, configs and rules but not `--set`.
4. **Plain `_normal` / `_nrm`.** It stays unresolved, even with `--accept-suggestions high` or `--allow`, until a preset, config, rule or `--set` states `opengl` or `directx`.
5. **Raw packing maps mark a `cs` set PBR immediately.** A raw roughness, glossiness, metallic or AO map makes the set PBR (spec §4.3 item 1) before channel packing exists in M2.5.
6. **`_d` aliases stay High.** A preset alias to `d` raises the colour reading to High confidence (Task 26a ruling); the owner can ask to add `d` to the `_s` exception.
7. **Open packing questions for M2.5 (spec §14 #18–#21):** the env-mask source of a packed `cpm`/`vanilla` `_m` and the constant height of a `cpm` `_m`; raw maps without a recipe (`_opacity`, specular-only and fuzz-only sets, RGB `_sheen`); the scale of a raw specular map; whether a set missing its packed map should exit 1. (superseded by M1, owner 2026-10-10: the cpm `_m` height is 0)
8. **Interim `IN-FLOAT-LDR` block.** Until the M6 transfer split, a 32-bit float source written to anything but BC6H needs `--allow IN-FLOAT-LDR`. Cost if wrong: float albedo TIFFs need the flag.
9. **In-game spikes (Task 46).** Pack: `target/spike-src` (regenerate with `cargo run --release -p sdc-bench -- gen-spike-assets --out target/spike-src`); steps: `docs/spikes/in-game-checklist.md`.
   - S1: closed by the owner's answer (Spike S1 above).
   - S2: **passed** (Spike S2 above).
   - S6: **closed**; `ALPHA-EXACT-OFF-M` is blocking (Spike S6 above).
   - S8: **closed** (Spike S8 above).

## Note R — per-texture packing fills and the _rfaos choice (owner, 2026-10-11)

- **R-fills-1 `packing.recipe` (`auto`/`rmaos`/`rfaos`).** Owner: "allow building a rfaos without fuzz and using fuzz fill ... Rfaos = skin = unorm diffuse, very different shader". `auto` (default) keeps today's rule: `_rfaos` when a fuzz source exists, else `_rmaos`. `rfaos` builds `_rfaos` even with no fuzz source: the G channel takes `packing.fill.fuzz`, else defaults to 0 (no fuzz; `PACK-DEFAULTED`), and a metallic source goes unused ("_rfaos has no metallic channel"). `rmaos` builds `_rmaos` even when a fuzz source exists; the fuzz source goes unused ("the set is set to _rmaos (packing.recipe)"). It only matters on `cs`. `_rfaos` is the skin shader. The skin sub-types are covered in Task 1b. Set per set at the packed output's `[files]` entry: the key carries the suffix the recipe decides, so Pass 1 reads both candidate keys (`<base>_rfaos`, `<base>_rmaos`) and an explicit non-auto value on either key wins, whatever it says (so the GUI may write the key of whichever output exists now). If the two keys disagree, the `<base>_rfaos` key wins and the packed output gets a `PACK-RECIPE-CONFLICT` warning naming both keys. A project, preset or `--set` value applies to every set. A lone metallic map cannot start a forced `_rfaos`: the set falls back to `auto` (`_rmaos`) and the packed output carries a `PACK-RECIPE-UNBUILDABLE` warning saying `rfaos` needs a roughness, glossiness or AO source. The set-membership prediction (`predicted_suffixes`) still assumes `auto`: a pre-packed `_rfaos` file next to a forced-`rfaos` set without a fuzz source is not grouped into the set (deferred; it needs per-set config before grouping). `RawMap::Opacity` stays a recognised raw map ("no recipe packs opacity yet").
- **R-skin-1 Advanced skin sub-types (owner, 2026-10-11).** Owner: "Add a separate section to hold Advanced skin maps, rfaos, sk(grayscale vs vanilla), wet(normal w/binary mask or grayscale height)"; "advanced skin uses unorm as it's albedo, that's why I said it should be it's own thing"; "_wet is either a normal with a binary mask or a grayscale height map, advanced skin supports both map types with no distinction"; "_sk maps for advanced skin use only the red channel"; "ensure all advanced skin matierials are prefixed like pbr is, so it's not confusing". Four sub-types, all `cs` only (unavailable on `cpm`/`vanilla`, no fallback): `skin_albedo` (Color; BC7 **UNORM**, never sRGB, so it is not `is_pbr_color`; alpha like `diffuse`, the mesh decides), `skin_rfaos` (Mixed-Packed; the former `rfaos`, unchanged formats), `skin_sk` (Grayscale; red only, BC4 UNORM) and `skin_wet` (see below). **Rename:** `rfaos` became `skin_rfaos`. `rfaos` stays a deserialise alias (`sdc.toml`, presets, `--set`, rules), the new id is written, the schema lists `rfaos` as deprecated, and the sub-type table `[subtypes.rfaos]` of `sdc-base` is now `[subtypes.skin_rfaos]`. The packed file suffix stays `_rfaos` (PGPatcher/CS naming) and the `packing.recipe` value stays `rfaos` (it names the output recipe, not a sub-type). Output bytes and names are unchanged; the golden cases only changed where they print the sub-type id (`skin_rfaos`). Existing projects: a set with an `_rfaos` member or a fuzz (`_sheen`) source now infers `skin_albedo` (UNORM, High) for its diffuse instead of `pbr_albedo` (sRGB), so that diffuse's tag and bytes change, `if-changed` re-encodes it, and `--accept-suggestions high` now converts it. **`skin_wet`** has two forms of one sub-type. Grey content (`R = G = B` within 4/255, the `VAL-M-GRAYSCALE` test) is a height map: BC4 UNORM, plain box mips, alpha removed. Anything else is a tangent-space normal with a binary mask in alpha: BC7 UNORM, RGB renormalised like `normal`, alpha policy `data`. The form is picked by analysis as a fact below every explicit layer, so a preset, config, rule or `--set` that sets `format` (BC4 vs BC7) wins (BC4 on colour content reads red only; `CHAN-NOT-GRAY` applies). `SKIN-WET-FORM` (Info) states which form was chosen and why. `wet` collides with no vocabulary term; it is a suffix only in a cs Advanced skin set (R-review I3). **Detection:** a set is Advanced skin when it contains an `_rfaos` member, or a raw packing input whose resolved `packing.recipe` builds `_rfaos` (`auto` with a fuzz source, or `rfaos`; read per set the way Pass 1 does, `set_is_advanced_skin`). There the diffuse suggests `skin_albedo`, `_sk` suggests `skin_sk` and `_wet` suggests `skin_wet`, all High. Outside one `_sk` keeps `backlight_tint` (Low) and `_wet` stays part of the base name (R-review I3; the earlier low `skin_wet` suggestion is gone). **Slots (open question):** `skin_albedo` is the Diffuse slot. `skin_sk` belongs to slot 3 (glow/skin, P6 index 3) but the set editor has no slot kind for it yet, so Task 2 must add one. `skin_wet` stays unslotted in the GUI set model and the PGPatcher JSON. Owner, 2026-10-11: "it's currently patched at runtime, I think it is forced into slot 5 or 7 using a non standard method"; sdc only converts it. **Codes:** Task 1 had reused `PACK-CONFLICT` for the two `packing.recipe` notices. They are now `PACK-RECIPE-CONFLICT` (the two keys disagree) and `PACK-RECIPE-UNBUILDABLE` (`rfaos` asked for on a metallic-only set), both Warning; `PACK-CONFLICT` keeps its pre-packed meaning. `SKIN-WET-FORM` is the third new code (catalogue 59 to 62).
- **R-fills-2 GUI: per-output fills, the recipe choice and the skin follow (owner, 2026-10-11).** Owner: "packing fill values needs to be per texture, not a global value ... take them out of advanced and put them in rmaos / rfaos / m ... cards"; "vanilla m doesn't need a fill. It is a one channel texture"; "keep fuzz fill for rfaos"; "i assume specular is for non pbr normals" (it is the `_rmaos`/`_rfaos` A channel, the PBR specular level). **Fills** are drawn on the packed output's card, only for a channel that has no source: `_rmaos` roughness, metallic, AO, specular; `_rfaos` roughness, fuzz, AO, specular (no metallic); cpm `_m` glossiness, metallic, height (shown under the slot cards, as the cpm `_m` fills no slot); vanilla `_m`, the parallax `_p` and the opacity channel have none, and `_rmaos` never shows fuzz. A channel with a source shows read-only "from <file>". The value is a 0..255 box (it shows the neutral default while unset) and a Reset that removes the key. Each is stored at `packing.fill.<role>` on that output's own `[files]` entry, so it does not leak to another set; an edit commits when the drag or typed value settles, as one undo step. Old project-wide `defaults.packing.fill.*` keys still load and still apply under the per-output ones; the Advanced drawer's fill panel is gone and the drawer lists them on one read-only line "Project-wide fills (legacy): ..." with a Clear button. **Where the recipe is written (reading of the Task 1 ruling):** a `packing.recipe` left on the key of the output that is not planned reads as an unmatched `[files]` entry (CFG-FILE-UNMATCHED, with a Remove button on the entry that holds the choice) and the resolved profile of the output reads `auto`. The card therefore writes `rmaos`/`rfaos` on the key of the recipe it picks (`<base>_rfaos` for RFAOS), removes `packing.recipe` from the other key so the two never disagree, and moves the fills both recipes have (roughness, AO, specular) to the new key (metallic and fuzz are dropped). The selector shows the recipe the engine chose, with "(auto)" while neither key holds an explicit value. **RFAOS switches the set to Advanced skin by engine suggestion only:** the card writes just the recipe (and the fill moves). With the recipe resolved to `rfaos` the engine (R-skin-1) suggests `skin_albedo` and `skin_sk` for the members whose sub-type is inferred, and suggests the vanilla ones again when the recipe goes back to `rmaos`; a member the user set has a provenance other than `Suggestion` and stays put. Nothing is written for members and there is no tracked list (an earlier draft had `[project] auto_skin`; it was dropped as redundant, since it turned suggestions into explicit `[files]` entries). It works the same while `sdc-project.toml` is read-only. A fill a recipe switch removes (metallic going to RFAOS, fuzz going to RMAOS) is reported on the card ("metallic fill removed (RFAOS has no metallic channel)") until the next edit. **Slot 3:** `SlotKind::SkinSk` (suffix `_sk`, label "Skin _sk") shares slot 3 with the emissive `_g`; a set holding both is a slot clash and blocks export ("_g and _sk both use slot 3"), like `_cnr`/`_f` at slot 7. `skin_wet` stays unslotted. **Dropdown:** the group "Advanced skin (Community Shaders)" (`skin_albedo`, `skin_rfaos`, `skin_sk`, `skin_wet`) follows the PBR group, and `ui_sky` moved from "Colour maps" to a last group "Other". Sub-type labels stay `id` with spaces ("skin rfaos"), the `pbr_*` style.
- **R-review Final-review rulings (controller, 2026-10-11).** These amend R-skin-1, R-fills-2 and P5 where they disagree.
  - **C1:** an Advanced skin set (`skin_albedo` diffuse) is a PBR set for the PGPatcher JSON and every PBR export check (owner: "Rfaos = skin … very different shader", still Community Shaders TruePBR). The GUI's `is_pbr_diffuse` (`pbr_albedo` or `skin_albedo`) sets `SetView.pbr` (P5 amended), `SetView.skin` gives the "skin → JSON" badge, the PBR follow and Accept-all ordering treat `skin_albedo` as PBR, a skin set's `_rfaos` keeps its name under the set prefix (it had been written as `<prefix>_rmaos.dds`) and is named in slot6, and an own `_sk` is always named in slot3 (PGPatcher's slot 3 default is not known to find `_sk`).
  - **I1:** skin detection uses the recipe actually chosen. An `rfaos` in the set's suffixes counts as an `_rfaos` member only when `auto` does not predict it from the raw roles; otherwise `chosen_recipe` decides, so a forced `rmaos` with a fuzz source builds `_rmaos` and is not skin.
  - **I2:** RFAOS is offered only where the engine can build it (a roughness, glossiness or AO source). Elsewhere the option is disabled with "RFAOS needs a roughness, glossiness or AO source", and the model refuses it without moving any fill, so dropped-fill notes always land on the key the card shows. Picking the built recipe again clears a stale explicit value; an `auto` choice is never pinned by a re-click.
  - **I3:** `_wet` is not a §6.1 suffix. It splits off only on `cs` and only when the base is an Advanced skin set (a real `_rfaos` member, or a resolved `packing.recipe` that builds `_rfaos`). Grouping cannot see the target or recipe, so `group::join_skin_wet` runs after grouping and before set edits and moves `x_wet` into set `x`. Everywhere else `_wet` stays part of the base name as before (vanilla `road_wet` is `landscape_diffuse`, `cobble_wet` + `cobble_wet_n` are one set), and the low `skin_wet` suggestion is gone.

**M4 variance bake and coverage fixes (owner and M4 plan, 2026-10-11)**

Owner decisions of 2026-10-11 (R1–R3) and the rulings of `docs/superpowers/plans/2026-10-11-sdc-m4-variance-bake.md` (R4–R14).

- **R1 Normal mip 0 is renormalised by default** (owner: "per-preset option, default renormalise"): field `mips.normal_base` = `renormalize` (default) or `keep`, settable per preset, rule, `[files]` entry or `--set`, for authors who keep short normals on purpose. `CHAN-NORMAL-SHORT` (info) reports a normal source whose mean length at mip 0 is below 0.9 (not for Z-rebuilt sources). Controller correction (2026-10-11): the normal chain and the bake average mip 0 **as it is written**, unit length under `renormalize` and as authored under `keep`, so variance reflects real normal divergence rather than exporter shortness; a uniformly short flat `_n` adds no roughness by default. Outputs change for existing normal maps with short sources (mip 0 and the levels averaged from it).
- **R2 `_cnr` self-bake** (owner): the coat normal's own variance widens its own alpha (coat roughness) with the §6.2 formula, 1:1, no companion; on by default (`coat_normal` `mips.normal` = `renormalize_bake`), off with `renormalize`. The alpha keeps its plain box filter: A = (min(r̄⁴ + min(2v, t²), 1))^¼ only where the footprint adds variance, so a flat coat normal's alpha is bit-identical to before.
- **R3 Not in M4** (owner): the DDS header-patch idea and the CS/Vanilla two-mode idea.
- **R4 The roughness member decodes its own `_n`** (plan): `ResolvedJob::bake` (`BakeJob`) names the `_n` and carries its profile; the member's job decodes it, keeps only the footprint lengths, drops the image, then loads its own source. No cross-job state and no scheduler ordering; replaces spec §6.3's summed-area table kept between jobs. Admission counts the decode peak of `load_image`: a bake job is admitted at `w·h·(decoded+32)` bytes (execution ruling, Task 7 review).
- **R5 Footprint lengths are a box pyramid** (plan): exact at power-of-two ratios, 1.33 bytes per `_n` texel. The bake needs both bases to be powers of two with one aspect; anything else is `SET-BAKE-RATIO` and no bake.
- **R6 The bake decision lives on the `_n`** (plan): a baking set upgrades its `pbr_normal`'s built-in `mips.normal = renormalize` to `renormalize_bake` with the new provenance `set`; an explicit `renormalize` from any other layer opts the set out. The `_n` is renormalised either way. `normal`, `landscape_normal` and `msn` never bake (no bake into vanilla `_n` alpha or `_m` green, spec §6.3). A companion `BakeJob` attaches only to roughness members (`rmaos`, `skin_rfaos`); a `_cnr` only self-bakes (execution ruling, Tasks 7 and 8).
- **R7 Shared roughness never bakes** (M3b v1 instruction): `BatchRequest::shared_roughness` lists the ids of every set (owner and borrowers) whose `_rmaos`/`_rfaos` slot (sub-type `rmaos` or `skin_rfaos`) is one output used by several sets; those sets renormalise only and raise no missing-companion code. The GUI fills it from its borrows and sends it on both the plan and the export request; the CLI leaves it empty.
- **R8 `SET-*` triggers** (plan): `SET-MISSING-COMPANION` (info) for a `cs` PBR set with a `pbr_normal` but no roughness member or the reverse, an `_msn` without `_s`, a `_flow` without `_n`; `SET-COMPANION-DDS` (warning) when the missing companion is only a DDS (in the set or on disk); `SET-COMPANION-OUTSIDE` (info) when it is an image on disk next to the file but not in the batch; both directions. The code goes on the job it concerns. A PBR set holding only an albedo raises none. A job that already carries `CFG-UNAVAILABLE` skips the `_msn`/`_flow` `SET-MISSING-COMPANION` note (pre-flight ruling: an unavailable profile raises nothing else).
- **R9 A usable `_n` is decided** (plan): Ready, or Blocked (e.g. `FMT-BC5-NORMAL`: the bake reads the source). An Unresolved `_n` gives its members `SET-MISSING-COMPANION` ("not decided yet"). With two `pbr_normal` jobs in one GUI-built set, the first by path is used.
- **R10 sRGB roughness** (plan): a roughness member with `color.content = srgb` raises `TAG-CONTENT-MISMATCH` and does not bake.
- **R11 No alpha weighting for vectors or roughness** (plan, Task 29 carry): normal, `_msn`, `_flow` and roughness chains are never alpha-weighted, even with a cutout policy forced on them. Preprocess dilation under α = 0 still runs for alpha-tested policies.
- **R12 `if-changed` and the bake** (plan): the baked `_n` is recorded in the manifest's `sources` with `bake` = the SHA-256 of its decode settings (`channels`, `alpha.policy`, `mips.normal_base`) and level shift; a changed `_n`, changed settings, or the bake turning on or off re-encodes the member. `Profile` gains `mips.normal_base` and `mips.bake_clamp`, so every `profile_sha256` changes once and the first M4 run re-encodes every output (as Note M10). Final review Minor 2: a `pbr_normal`'s `profile_sha256` hashes `mips.normal = renormalize_bake` as `renormalize` (`manifest::profile_sha256`), because the bake changes none of the `_n`'s own bytes; the bake turning on or off re-encodes only the member, not the `_n`. A `coat_normal` keeps its own hash (its self-bake changes its bytes).
- **R13 Coverage, mipcheck round 2 B1/B6** (owner decision on the dither, 2026-10-11, verbatim: "I would probably encourage a dithering at lower mips, the whole point of coverage is to ensure it isn't lost, especially for foliage and grass, when your likely seeing mostly lower mip versions of them"): every level ≥ 1 passes exactly `k = round(p·n)` texels, the texels above the k-th largest alpha plus as many tied with it as are needed. The dither is in M4 at the owner's request. Ties are split in a hierarchical Bayer order of position (not plain 8×8, which biases the split spatially; deterministic, no RNG), so tied alpha on thin strands neither overshoots (up to ×32 before) nor vanishes (B1). `k = 0` keeps the box alpha ("no texel passes"); zero alpha is never raised, and a level with no positive alpha keeps its box alpha; there is no "more than 2× off" fallback. Remapped texels, the split's included, sit at least 2 codes clear of the threshold (B6). The clearance also applies to levels that skip the remap (early-out levels, including `k = 0`). At the ends of the range (threshold code c ≤ 2 or c ≥ 254) the clearance degrades to whatever codes are available (reachable since owner O2, M6 Track E, widened `alpha.threshold` to 1–254; the clearance saturates).
- **R14 `sdc-base` 1.1.0** (plan): new defaults `mips.normal_base`, `mips.bake_clamp` (0.25) and the `coat_normal` self-bake. There is no preset changelog file; the bump is recorded here and in spec §3.5. `cs`, `cpm` and `vanilla-quality` stay at 1.0.0.
- **M4-final-1 A borrowed `_n` bakes the borrower's own roughness** (controller ruling on final review I1, 2026-10-11; amends R7's "the engine has no notion of borrowing"): `BatchRequest::borrowed_normal` maps the id of each set whose Normal slot borrows another set's `_n` (GUI share) to the id of the set that owns it. `plan_bakes` bakes the borrower's own `rmaos`/`skin_rfaos` members from the owner's `pbr_normal` (the first by path, R9) under every usual rule: the decided check, the R6 opt-out, R10, `SET-BAKE-RATIO`, the stamp, the manifest `sources`, `if-changed` and bake admission. A borrower whose roughness slot is itself shared (R7) still never bakes. The lent `_n` raises no `SET-MISSING-COMPANION` in its own set when a borrower with a roughness member uses it; a borrower with a roughness member but no usable owner `_n` gets `SET-MISSING-COMPANION` naming the borrowed `_n` file (or saying the owner set has no usable `_n`). While a set borrows its Normal slot, its own `pbr_normal` (if any) is ignored by the bake, as the GUI shows the borrowed one. The GUI fills the map on `cs` only, on both the plan and the export request; the CLI leaves it empty, so CLI runs are unchanged.
- **M4-final-2 (owner, 2026-10-11): an 8-bit noise floor for the bake** ("Ignore tiny variance", on final review Minor 9): a footprint whose average normal length L is at least `NOISE_FLOOR_L` = 0.999 adds no variance (was 1 − 10⁻⁶), in the `_n` → roughness bake and the `_cnr` self-bake alike (`bake::variance_add`). A visually flat `_n` with ±1–2 codes of quantisation noise has 1 − L of about 1e-5 to 1e-4 and lifted r = 0 to 0.04–0.10 at mip ≥ 1; it now adds nothing at any mip. Worst-case ±k-code noise stays above the floor up to k = 4. Real bumps still widen: the bumpy golden `_n` and `_cnr` cases are far below the floor, and a tilt of ±6 X codes (L = 0.99889) widens while ±5 codes (L = 0.99923, about 2.2°) does not. The golden f64 reference restates the floor independently. Outputs change only where some footprint has 0.999 ≤ L < 1 − 10⁻⁶.

## Note S — M6 Track B: inputs and formats (plan rulings, 2026-10-11)

Rulings of `docs/superpowers/plans/2026-10-11-sdc-m6-inputs-formats.md`, for owner review. Owner confirmed the defaults on 2026-10-11: `hdr_sky` keeps BC6H for every source (S3), `rgba16f` is `cs`-only (S4), header-patch destination as S7.

- **S1 Float-source transfer split** (replaces the final-review I4 interim guard): a float source (32-bit float TIFF, OpenEXR, Radiance HDR, float or half DDS) is linear light. Preprocess never sRGB-decodes it; for an LDR output Quantise clamps each channel to [0, 1] and encodes it per `color.content` (`srgb` → sRGB OETF, `linear` → stored), the encoding a decoded integer source already gets. Keyed on content, not the tag: vanilla `diffuse`/`glow` are content `srgb` with tag `unorm` and must stay gamma-encoded. Values outside [0, 1] raise `LOSS-CLAMP-HDR` (warning). A float normal, `_msn` or `_flow` keeps today's stored-value reading. An explicit `color.content = srgb` no longer decodes a float source.
- **S2 `IN-FLOAT-LDR` retired:** never emitted, kept in the catalogue as Info so `--allow IN-FLOAT-LDR` and `[files] allow` lists still load (catalogue count unchanged).
- **S3 O2 for `pbr_emissive`:** BC6H only when the source is float and some finite R, G or B is above 1.0; otherwise BC7 with tag `srgb`, as the fact `analysis:hdr-source`, applied after every layer and skipped when any explicit layer (user/project preset, config, rule, `[files]`, `--set`) set `format`. On `pbr_emissive` the tag follows the final format (`n/a` for BC6H, else `srgb`) unless a layer set it, so `--set file:<glob>.format=…` alone wins in both directions. `FMT-HDR-SOURCE` (info) states the reason on every available `pbr_emissive`. `sdc-base` is unchanged (still 1.1.0). `hdr_sky` is unchanged (owner question).
  - **Precedence deviation accepted** (controller ruling, Task 4): O2's literal "below a suffix fact and `--set`" is widened to "skipped by any explicit layer that sets `format`", because an explicit user choice should win wherever it is made. Cost if wrong: a config-level `bc6h_uf16` on an 8-bit `_g` stays BC6H (bigger file, same look).
- **S4 Uncompressed outputs (O1):** `rgba8` (DXGI 87/91, B8G8R8A8; legacy RGB masks under `header = auto` for UNORM), `r8` (61), `rg8` (49), `rgba16f` (10, tag `n/a`, the `Hdr` domain, alpha kept as half, `cs` only). `DDSD_PITCH` with a tight row pitch. Backend `uncompressed`. `r8`/`rg8` refuse `srgb` (`CFG-INVALID`) and block on normals (`FMT-BC5-NORMAL`). texconv/nvddsinfo check: both read all five uncompressed outputs (`rgba8` UNORM and sRGB, `r8`, `rg8`, `rgba16f`), PASS. texconv cannot write an R8G8 PNG (`80070032`; it reads the `rg8` DDS fine as `R8G8_UNORM` with 7 mips), so the env-gated interop test converts to `R8G8B8A8_UNORM` first. The writer is unchanged.
  - **BC6H-only checks generalised to `is_hdr()`** (controller ruling, Task 6): the `color.tag` rule, the HDR-outside-`cs` check, the `pbr_emissive` tag fact, the float-source-is-linear fact and the CLI `validate` tag inference (`n/a`) key on `Format::is_hdr()` (BC6H and `rgba16f`), so an `rgba16f` job is judged like BC6H.
- **S5 Uncompressed input (O1):** `Bgra8` gains its sRGB form (DXGI 91); sdc's own uncompressed files are fully validated; RGBA-order RGBA8, BGRX8 and RGBA32F stay header-only in `validate` but are inspected, previewed and re-encoded. The reader assumes tight rows: a padded pitch is `VAL-HEADER`. N11 ends; BC2 and signed BC6H still cannot be previewed.
- **S6 DDS re-encode:** `reencode` is a plan-time selector (not a `Profile` field), boolean, unscoped/file/`[files]` only. Mip 0 is decoded, the rest regenerated; `Analysis.dds_dxgi` records the input's DXGI. `LOSS-REENCODE` (warning) only for block-compressed inputs. A re-encoded DDS takes part in duplicate detection; one whose output is its own path fails at plan; cubemaps and arrays fail to load; a re-encoded DDS member does not bake.
  - **No-bake guard enforced in plan** (controller ruling, Task 8 review): no later task enforced the last sentence, so plan excludes an opted-in DDS member from the variance bake, with a test.
  - **One canonical same-file guard** (controller rulings, Task 10 and Task 11 reviews): "its own path" is judged by `engine::dds::same_file` (canonical path, with junctions, symlinks and `..` resolved and a missing output judged by its folder; else lexical; case-insensitive). The re-encode check, `sdc patch-header --out-dir` and the GUI Patch all use it. Hard links are not detected.
  - `inspect` ignores `reencode`: a DDS input always shows its header report.
- **S7 DDS header patch (owner idea 2026-10-11):** header-only, payload byte-identical (new header + the old bytes after the old header). Patchable: flags, caps, linear size/pitch, mip count (the whole-level count that fills the payload), reserved fields, `DDPF_NORMAL`, DX10 fields, `ATI1`/`ATI2` → DX10, legacy → DX10 where the tag needs it, and the tag only on an explicit `--tag`, `--subtype` or GUI choice. Refused with a reason (`DDS-HEADER-UNPATCHABLE`): unreadable, cubemap/array, a format sdc does not write, sRGB on a format without one, a payload holding no whole chain, missing mips when a full chain is wanted, BC1 with transparent texels, the wrong block format under `--format`. `DDS-HEADER-PATCHED` (info) lists the fixes. CLI `sdc patch-header` needs `--out-dir` or `--in-place`; the GUI writes a copy to the export folder only. Not recorded in the manifest.
  - *Amended (final review I1, 2026-10-11):* the GUI refuses the patch, with the reason under that item's panel, when another item in the job list exports to the same output key (e.g. `rock.dds` next to `rock.png`); an existing file at the destination alone is not a refusal, so the same DDS can be re-patched.
  - **`--out-dir` onto the input is a usage error** (controller ruling, Task 10 review, overriding the plan's Step 3 code): an `--out-dir` that resolves to an input itself (the S6 guard), or two inputs with one file name, exits 2 before anything is written. `PATCHED` is printed only after the write succeeds; a write failure exits 3 and stops the batch.
  - **The GUI Patch writes under `export_root()`** (controller ruling, Task 11 review, overriding the brief's code): the copy goes to the export root (the chosen export folder, else the default `sdc-out`), so the button never silently does nothing; every error reaches the status line and the confirmation always ends.
- **S8 ICC:** detected and ignored; `IN-ICC-IGNORED` (info).
- **S9 `IN-NONFINITE`:** reported at plan time (so `inspect` and `--dry-run` show it), once; the analysis statistics skip non-finite samples.
- **S10 Inputs:** `.exr` (first RGB(A) layer, f32), `.hdr`, `.bmp`, `.jpg`/`.jpeg` through `image` 0.25.10 features; `exr` 1.74.2 and `lebe` are BSD-3-Clause (THIRD_PARTY_NOTICES). Undecodable files are `IN-DECODE`.
- **S11 Version:** workspace 0.3.0: outputs changed (float sources, 8-bit emissives), so `if-changed` re-encodes once.
- **S12 Packed maps cannot use an HDR format** (owner, 2026-10-11, verbatim: "a packed map cannot use bc6h, they are mutually exclusive"): a Mixed-Packed sub-type (`complex_material`, `rmaos`, `skin_rfaos`, `flow`, `generic_two_channel`, `generic_data`) with `bc6h_uf16` is `CFG-INVALID` in `check_profile`. Generalised with S4 to every `is_hdr()` format, so `rgba16f` too: packed outputs are linear 8-bit data (the Stored domain).

### M6 Track C: mip filters and resize (plan rulings, 2026-10-11)

Rulings of `docs/superpowers/plans/2026-10-11-sdc-m6-mips-resize.md`. The owner agreed with the plan's recommendations on 2026-10-11, so the plan's three owner questions are **closed** (S-mips-2, S-mips-4, S-mips-6).

- **S-mips-0 Execution order** (owner, 2026-10-11: "start track c"): Track C ran **before** Track A (A is on hold until the NIF merge), on top of main with Track B merged. The code count here is 69 (`Code::ALL.len()` 67 on main + `DIM-RESIZED` + `CFG-FILTER-IGNORED`, S-mips-13). Track A's plan must rebase its "merged + N" counts on this (69 + 4 = **73**; it was 72 before S-mips-13). Track A's hair-shader default layer sets `alpha.policy = cutout`, so a user's `mips.filter = kaiser` on such a diffuse is barred (S-mips-2): set at sub-type or file scope it is `CFG-INVALID`, set unscoped it falls back to box with `CFG-FILTER-IGNORED` (S-mips-13). A's goldens and docs must expect both.
- **S-mips-1 The default filter stays `box`** for every sub-type and preset. No preset gains a non-box default, so existing outputs are unchanged.
- **S-mips-2 Where non-box filters are allowed** (spec §3.2, §5.5 read literally). `tent`, `mitchell` and `kaiser` require category Color, sRGB content (domain `LinearLight`) and an alpha policy that does not weight (`remove` or `data`). `tent` is also allowed for HDR, `format.is_hdr()` (`bc6h_uf16` or `rgba16f`), but only for category Color or Hdr with an alpha policy that does not weight; HDR with `mitchell`/`kaiser` is refused ("HDR mips use box or tent"). Anything else with a non-box filter set at sub-type or file scope is **`CFG-INVALID`** (Error; an unscoped value falls back to box instead, S-mips-13), with the reason: normals, `_msn`, `_flow`, data, grayscale, packed, roughness members, linear-content colour, and colour with `cutout`/`blend`/`cutout_blend`. The GUI shows these as locked options. The engine also falls back to box in `mips.rs` (`effective_filter`), but only as a guard; a blocked job never reaches it. **Owner question 2 closed** (2026-10-11): alpha-tested and blended colour (foliage) stays on alpha-weighted box; no alpha-weighted tent (revisit later).
- **S-mips-3 Tent/Mitchell/Kaiser levels are built directly from mip 0**, with the kernel widened by the level's scale (2^N at power-of-two sizes; `src/dst` per axis in general). Values stay f32 between steps; weights are f64. **Box stays recursive**, with today's code untouched. Only RGB is non-box: **alpha keeps the recursive box** (spec §5.5). Source: mipcheck RESULTS-2 B8 (recursion costs tent 1.1–2.7 dB and Mitchell 0.8–1.8 dB against Lanczos-8).
- **S-mips-4 Kaiser is fixed at width 3, α 4, stretch 1.** Mitchell is B = C = 1/3 with radius 2; tent has radius 1. They are constants in `filter.rs`, not profile fields, so the spec's `kaiser{width,alpha,stretch}` syntax is deferred. **Owner question 1 closed** (2026-10-11): the Kaiser knobs stay fixed.
- **S-mips-5 `mips.edge` = `wrap` (default, every sub-type) or `clamp`.** Only the polyphase kernels read it, in the mips and in a non-box resize. Box (recursive pair, area-exact odd path, area-exact resize) never reads outside the image, so it ignores the edge.
- **S-mips-6 `resize` = `none` (default) or `pot-down`**, a top-level profile field (`--set resize=pot-down`). `pot-down` maps each axis independently to the largest power of two ≤ its size (1000×600 → 512×512; a power-of-two axis is kept). The aspect may change; UVs are normalised, so meshes map the same. The resize runs in Preprocess step 6 (after sRGB decode), in the Work's domain, with the **effective mip filter** and edge: box is area-exact (the generalised `downsample_area`), tent/Mitchell/Kaiser are polyphase on RGB. Alpha is always area box; alpha-weighted colour is weighted by area × alpha; a roughness member is resized on r²; normals, `_msn` and `_flow` are box-resized as vectors and mip 0 is then renormalised unless `mips.normal_base = keep`. Upscaling and a size cap are out of scope. **Owner question 3 closed** (2026-10-11): no preset defaults to `pot-down`; `DIM-NPOT` blocks until the user opts in.
  - **Resize runs at the end of `preprocess_with`** (pre-flight F1): after the Hdr clamp, in the function the pipeline and `BakeJob::load_lengths` call (`preprocess` is only a wrapper), so the written DDS and the bake both see the resized image.
  - **A box resize never clamps** (pre-flight M8): only a non-box resize clamps LinearLight RGB to [0, 1] (polyphase overshoot), mirroring `finalize`; a float LDR value > 1 survives a box resize until Quantise.
  - **`axis_taps` returns identity taps when n == m**, so an axis that is already a power of two is copied unchanged under `pot-down` with any filter (no blur at scale 1).
- **S-mips-7 Resize in planning:** `DIM-NPOT`, `DIM-SMALL` and `DIM-FLOW-SMALL` test the **post-resize** size. A resized job gets the new **`DIM-RESIZED`** (Info) code: "1000x600 resized to 512x512 (resize = pot-down)", data `{"from":[w,h],"to":[w',h']}`; a power-of-two input with `pot-down` gets none. The bake plans its shift on the post-resize sizes and `BakeJob::load_lengths` builds the pyramid from the resized, renormalised `_n`; the bake's `NormalVariance` is built on `target_dims`, so the shift check holds on an NPOT `pot-down` pair. `BakeJob::identity_sha256` adds the `_n`'s `resize` only when it is not `none`, so existing identities are unchanged. `PACK-SIZE-MISMATCH` still compares source sizes before assembly; the packed image is resized after assembly.
- **S-mips-8 Coverage reference after a resize** is the resized mip 0 (`base_coverage(&chain[0])`, unchanged code). Matching mip 0's coverage to the source is deferred.
- **S-mips-9 Hash stability, no version bumps.** `mips.edge` and `resize` serialise only when not default (`skip_serializing_if`), so `profile_sha256` and every manifest stay as they are for unchanged settings. An opt-in changes the hash and `if-changed` re-encodes it. The workspace version stays 0.3.0 and `sdc-base` stays 1.1.0: the golden suite passed unedited (Task 7) and `default_profiles_hash_as_before` holds.
- **S-mips-10 GUI:** the basic Mipmaps group's "Filter" row is a live combo (Box / Tent / Mitchell / Kaiser); disallowed options are greyed out with the S-mips-2 reason. It stays in the basic panel (owner, 2026-10-11). An "Edge" row (Wrap / Clamp) sits under it, locked "only tent, Mitchell and Kaiser read the edge" while the filter is box; a default edge or resize the profile does not serialise shows as Wrap / None. "Resize" (None / Down to a power of two) is the last Advanced texture field. The `DIM-NPOT` decision box gains "Resize to a power of two" next to "Acknowledge for this texture"; it writes `resize = "pot-down"` on that file's `[files]` entry.
- **S-mips-11 Memory and speed:** the new kernels are separable and row-parallel on the shared rayon pool. Admission (`api::job_bytes`) adds `8·w·h` bytes for a non-box filter and `32·w·h` for a resize, with `(w, h)` from one shared `input_dims` helper (also used by `estimated_bytes`: a DDS by its header, else `load::image_size`), so a re-encoded DDS is admitted on its real size. The 13 GiB box peak at 16K and the single-threaded per-channel `powf` in Preprocess stay deferred to the scheduler revisit.
- **S-mips-12 Kaiser golden on `rgba8`** (Task 7): the "Kaiser diffuse differs from box, its alpha does not" golden encodes `rgba8`, because BC7's joint RGB/alpha error moves the alpha differently for each filter (an encoder effect, not the filter's).
- **S-mips-13 An unscoped filter only affects colour maps** (owner, 2026-10-11: "change the unscoped kaiser behaviour, print a message in the console instead saying that it only effects color maps"). A non-box `mips.filter` that S-mips-2 bars for a job is judged by the scope of the **last layer that set it**. Unscoped (an unscoped `--set`, or a preset or config `defaults` table): the job resolves **box** in `layered_profile` (after every layer and analysis fact, since the check reads format, content, category and alpha), with provenance `fallback`, and gets the new **`CFG-FILTER-IGNORED`** (Info): "mips.filter = kaiser only affects colour maps; wall_n.png uses box". On a colour map that is barred for another reason (alpha weighting, linear content, HDR), the S-mips-2 reason follows after a colon. Scoped (a `subtypes.<st>` table, a sub-type- or file-scoped `--set`, a rule, a `[files]` entry): unchanged, `CFG-INVALID`. The fallback is in the resolved profile, so the manifest, `profile_sha256` and if-changed see box: such a `_n` hashes as a default run and stays up to date with or without the set. The CLI's human log prints this one Info code (other Info codes stay hidden). The GUI writes filters at file or set scope, so its edits are unaffected; a texture whose inherited unscoped filter fell back shows Box with provenance `fallback`. The golden "CFG-INVALID Kaiser normal" now sets the filter file-scoped. Code count 69; Track A becomes 73 (S-mips-0).
- **Deferred:** the Kaiser width/alpha/stretch syntax; alpha-weighted tent for foliage; `resize` upscaling (`pot-up`, `pot-nearest`) and a size cap; matching the resized mip 0's coverage to the source (S-mips-8); resizing pack sources of different sizes to a common size (`PACK-SIZE-MISMATCH` still errors); the 16K box peak and the single-threaded `powf` (S-mips-11).

### M6 Track A: shader modes (plan rulings, 2026-10-11)

Rulings of `docs/superpowers/plans/2026-10-11-sdc-m6-shader-modes.md`. The owner's answers of 2026-10-11 are folded in. Where the plan text and the build differ, this subsection describes the build.

- **S-shader-1 `--set` set scope (owner / plan, 2026-10-11).** There was no set scope before. `set:<id>.shader=<shader>` is the only form, and `shader` the only field a set scope takes. Only a trailing `.shader` splits the key, because set ids may contain dots. The id matches by `sets::set_id` (case-folded); an id without `/` also names every set whose key ends in `/<id>`. GUI ids (`sdc-project.toml`) are canonical and match exactly, with no tail match (final review I2). An id that names no set is one `SET-EDIT-STALE`. A command-line `set:` beats the GUI's choice (a later `--set` beats an earlier one).
- **S-shader-2 `sdc-project.toml` (owner / plan, 2026-10-11).** `shader = "<id>"` goes in the set's `[[sets]]` entry (the spec's `[sets."<base>"]` adapted to the file's real layout). It is a reserved key, never a JSON setting; an unknown id reads as unset and stays in the file.
- **S-shader-3 Precedence (owner / plan, 2026-10-11).** `Provenance::Shader` (shown `shader:<id>`) is a new layer applied after the preset chain, configs, rules, the analysis-alpha step and `skin_wet_form`, and before the vocabulary green step and the ranked overrides. It writes a field only where `field_explicit` is false (built-in values and built-in presets), so it beats built-in profiles and suggestions and loses to user presets, configs, rules, analysis facts, suffix facts, `[files]` and `--set`. It counts as explicit itself, so a shader alpha policy answers the alpha-meaning question. In the `Provenance` order it sits between `analysis:<fact>` and `suffix:<term>`. Consequence: `hair` on a diffuse with no alpha keeps the analysis `remove`; on varying alpha it gets `cutout` and asks nothing.
- **S-shader-4 Sub-type effects (owner / plan, 2026-10-11).** Eye `_n` becomes `msn` and a landscape colour member becomes `landscape_diffuse`. Applied in `decide_subtype` after `--set subtype` and rules. An explicit shader decides it (`decided: true`, provenance `shader:<id>`); an auto or fallback shader inserts a High suggestion "<id> shader (<origin>): <why>" at the front, decided only as any High suggestion is.
- **S-shader-5 Member defaults (owner / plan, 2026-10-11).** `hair` diffuse: `alpha.policy = cutout`, `mips.coverage = on`. `multilayer` diffuse: `alpha.policy = data` (owner: the alpha is the layer mask, kept exact). `envmap` and `eye` envmask, `parallax` height: `format = bc4`. Finding: `envmask` and `height` were already BC4 in `sdc-base`, so those rows change provenance (`shader:<id>`), not bytes. An `_m` is never re-typed: a varying-alpha `_m` under an explicit `envmap` stays `complex_material` and is reported as an extra map.
- **S-shader-6 `_i` and rule 12 (owner, 2026-10-11).** `_i` is added to the §6.1 suffix list (a grouping change) and suggests `diffuse` (Medium). Rule 12 auto-picks `default`, never `pbr` without a PBR member. `default_for(target)` (`pbr` on `cs`, else `default`) is only the dropdown's first entry and the fallback shader.
- **S-shader-7 Planning order (plan, 2026-10-11).** Pass 1 resolves every file; the shader is picked per set after pass 1; members of shaders with defaults or sub-type effects are re-resolved; panic isolation is kept. Shader warnings (`SHADER-AUTO`, `SHADER-UNAVAILABLE`, `SHADER-EXTRA-MAP`, `SHADER-MISSING-MAP`) ride on `set_resolved`, not as `warning` events. `EVENT_SCHEMA` stays 2 (the fields are additive).
- **S-shader-8 Target independence (plan, 2026-10-11).** The PBR rule is target-independent, so a PBR set under vanilla or cpm auto-picks `pbr` and falls back to `default` with `SHADER-UNAVAILABLE`. PGPatcher JSON still follows `is_pbr_diffuse` (P5/C1), not the shader.
- **S-shader-9 `_coat` (owner, 2026-10-11: "_coat (for the coat diffuse)").** A built-in §4.4 vocabulary term (`VocabRole::CoatColour`, canonical `s`), not a preset alias.
  - `x_coat.png` joins set `x` as its `_s` member, `pbr_subsurface` (High), slot 8, written `x_s.dds`;
  - it collides with `x_s.png`, and with a re-encoded `x_s.dds` (Track B S6), as `IN-DUPLICATE`;
  - it is blocked by `CFG-UNAVAILABLE` under vanilla and cpm;
  - a user preset alias `coat` is now refused, because aliases may not be vocabulary terms.
- **S-shader-10 Mesh-reported shader (owner, 2026-10-11: "map wins, assume the user runs PGPatcher").** Auto-pick order: explicit → advanced skin → PBR → complex `_m` → `_p` → mesh majority (`auto:mesh`) → envmap `_m`, `_g`, `_i`, `_sk`, hair path, eyes path, landscape, default. Only the maps PGPatcher can patch a mesh for (complex `_m`, `_p`) beat the mesh. Evidence is `BatchRequest::mesh_shaders`, keyed by `shader::mesh_key` (diffuse game folder + stem, lower case) and filled by the GUI from the scan database. Votes: lighting shapes using `<key>.dds` in slot 0, from the whole load order (every enabled origin), not TruePBR shapes (SLSF2 bit 23), not PGPatcher-derived copies; one vote per NIF path and block. `Shader::from_lighting_type`: 0 default, 1 envmap, 2 glow, 3 parallax, 5 skin, 6 hair, 7 parallax (Parallax Occlusion; owner-provisional), 8 landscape, 11 multilayer, 16 eye; any other type is no vote. A strict majority is required, else the suffix rules decide. PBR is never read from a NIF (the TruePBR bit is PGPatcher's output marker), so a PBR or skin member always wins. The CLI sends no votes. `modscan` is untouched: `crates/gui/src/model/meshes.rs` reads it with raw SQL on `Store::conn()` (a `shortcut:` comment there; move it into a typed `modscan::store` query when modscan is next edited). There is no envmap-to-complex refinement: complex `_m` (rule 3) fires before the mesh.
- **S-shader-11 Owner answers and Track C interaction (2026-10-11).** The owner's four answers on the mesh evidence: the map wins over the mesh for complex `_m` and `_p` (PGPatcher patches the mesh for them); the votes come from the whole load order; lighting type 7 maps to `parallax` (provisional); the scan database is read with raw SQL now. Track C (S-mips-0/-13): the shader layer runs before `unscoped_filter_falls_back`, so a hair diffuse's `cutout` makes an unscoped non-box filter box + `CFG-FILTER-IGNORED` and a scoped one `CFG-INVALID`. Catalogue count 69 → 73; workspace version 0.3.0 → 0.4.0 (`if-changed` re-encodes).
- **Also (Track A).** A landscape set without `_n` shows `SHADER-MISSING-MAP` (Warning) on its card.

## Note T — M6 Track D: presets (plan rulings, 2026-10-12)

Rulings of `docs/superpowers/plans/2026-10-12-sdc-m6-presets.md` on spec `docs/superpowers/specs/2026-10-12-sdc-m6-presets-design.md` (owner-approved 2026-10-12; §2.2 corrected by the owner in 956da58).

- **T1 Preset ids (owner / plan, 2026-10-12).** `cs-compact` (base `cs-quality`, target cs) and `vanilla-compact` (base `vanilla-quality`, target vanilla). There is one `fast-preview` (base `sdc-base`, no target): the registry needs no target, and the batch target decides. It does not inherit later `*-quality` settings.
- **T2 BC1 rule mechanics (plan, 2026-10-12).**
  - `RuleMatch` had no sub-type matcher, so it gained `match.subtypes` (matched while layering, never while deciding a sub-type).
  - `match.alpha` matches the source's alpha content, so there are two rules: `none`, and `constant255` for sub-types whose policy resolves to `remove`.
  - No resolved-policy matcher was added (it would be circular).
  - An explicit non-`remove` policy on an opaque colour map under compact is `FMT-BC1-ALPHA`.
  - `bc1_transparent_black` stays false.
- **T3 The colour and data lists (owner / plan, 2026-10-12).** Exactly as in the plan.
  - Opaque colour becomes BC1 at full size (its rules set `resize = "none"`). Colour that stays BC7 (colour with alpha) halves, like every BC7 colour sub-type (owner).
  - `landscape_diffuse` is excluded from BC1 (`FMT-LANDSCAPE-BC1`); as BC7 colour it halves (owner).
  - `pbr_emissive` (BC6H) and `hdr_sky` stay full size. `hdr_sky` is excluded from resize (owner).
  - `pbr_subsurface` and `pbr_fuzz` are BC1 only without alpha.
- **T4 `resize = half` (owner, 2026-10-12).** Both sides halve together when the larger is above 512 (`HALF_FLOOR`); the aspect ratio is kept and the larger side floors (1024×512 becomes 512×256); there is no POT step; each side is at least 1 px; `DIM-RESIZED` names the mode; admission counts it like `pot-down`.
- **T5 `validation.mips = smallest` (plan, 2026-10-12).** PSNR on the last mip only; coverage runs on every level (amended by audit 13, Note Y6). The default `all` is not serialised.
- **T6 The user presets folder (plan, 2026-10-12).**
  - Location: `%APPDATA%\sdc\presets\*.toml`, non-recursive, in file-name order.
  - Load order: all files load first, then each preset is checked, so a base may come later in the order. Failures are skipped with `CFG-PRESET-SKIPPED`, repeatedly, so orphans go too.
  - Path: `BatchRequest::user_presets`, set by the CLI and the GUI, `None` in tests.
  - The same file reached twice loads once.
- **T7 Duplicate ids are refused (plan, 2026-10-12).** Carried fix 1, naming both sources: `CFG-PRESET-SKIPPED` in the folder, an error from `--preset-file`. `is_builtin` means "loaded from the embedded list".
- **T8 Verbs (owner / plan, 2026-10-12).**
  - `diff` compares profiles from presets alone (rules are not shown: spec gap).
  - `export` writes the preset's own TOML under a new id (`<id>-export` or `--id`); `base` stays a link, and there is no `--flat` (owner).
  - `export` writes the parsed table re-serialised, not the original text: comments are dropped and keys may be reordered (final review Minor 9). The output is equivalent and validates.
  - `export` copies `suffix_aliases` as they are and does not strip them. A copy of a preset with suffix aliases therefore cannot load beside the original (the aliases conflict, `CFG-PRESET-SKIPPED`); rename or remove the aliases before putting the copy in the user presets folder. The `export` help says so (final review Minor 8).
  - `export --out` refuses to overwrite the source file of any loaded preset (final review Minor 10).
  - `diff` help and its human output footer say "rules are not compared" (final review Minor 13).
  - `validate` reports the first problem per file, and validates a folder file in place of itself.
- **T9 Migration table (plan, 2026-10-12).** Presets none, config none, manifest records v1 → v2 (`schema` set to 2; the manifest's key is `schema`). Version 0 is refused. `PresetError::NewerConfigSchema(file, v)` is the spec's `ConfigError::NewerSchema`. Config files accept an optional `schema_version`.
- **T10 Carried fixes (plan, 2026-10-12).**
  - #2: `merge_json`, where a `null` patch never adds a key (the spec's two sentences disagreed; the function's contract won).
  - #3: a rule whose `subtype` is not the final sub-type does not apply. This includes a `CFG-UNAVAILABLE` fallback.
  - #4: `CFG-GREEN-CONFLICT`; the explicit `flip_green` wins.
  - #5: the landscape match was already whole-component; pinned by tests.
  - #6: `hdr_sky` only under a `sky` component.
- **T11 BC4/BC5 search radius (plan / owner, 2026-10-12).** Measured in `docs/spikes/bc45-search-radius.md`, re-run after T13.
  - Rule Q (quality default becomes `max` if +0.10 dB at ≤ 1.5× time on both formats) did not fire. BC4 at radius 10 gains +0.1231 dB at 2.80× the time; BC5 gains +0.0599 dB at 2.96×. `sdc-base` keeps `"default"` and stays 1.1.0.
  - Rule F (smallest radius below 5 within 0.10 dB of 5 on both formats) gives 4: −0.0591 dB (BC4) and −0.0362 dB (BC5) against 5, at 0.73× and 0.71× the time. It is `SearchRadius::Fast` (`"fast"`), used by `cs-compact`, `vanilla-compact` and `fast-preview`.
  - Mean PSNR (dB) at radius 0 / 4 / 5 (`default`) / 10 (`max`): BC4 55.79 / 57.20 / 57.26 / 57.38; BC5 58.94 / 60.57 / 60.60 / 60.66.
  - No warning on the `max` radius (owner, 2026-10-12); `ENC-RADIUS-MAX` was not added.
- **T12 Versions (plan, 2026-10-12).** Codes 73 → 75 (`CFG-PRESET-SKIPPED`, `CFG-GREEN-CONFLICT`). `sdc-base` stays 1.1.0 (rule Q did not fire). Workspace 0.4.0 → 0.5.0 so `if-changed` re-encodes.
- **T13 rgbcx rounding patch (owner, 2026-10-12).**
  - The vendored rgbcx's BC4/BC5/BC3-alpha decode model now rounds to nearest like bcdec. It used to floor the interpolants, so a wider search found endpoint pairs that were exact only under the floor.
  - Gain at the default radius: BC4 +3.07 dB, BC5 +2.61 dB (mean PSNR under bcdec, bench corpus). PSNR now rises monotonically with the radius.
  - A new manifest `rgbcx_patch` field makes old BC3/BC4/BC5 outputs re-encode. Like `bc7e_patch` it is a content tag, `bc4-round-to-nearest@sha256:<hash of the patched vendor/bc7enc_rdo/rgbcx.h>` (final review Minor 11), so an in-place change to the patched header also re-encodes them.
  - Two bench gates moved from byte-exact to never-worse (per-block bcdec SSE for BC3 alpha, BC4, BC5); BC1 and BC3 colour stay byte-exact.
  - Known quirk: sdc's own BC3 alpha validation decodes through `bcdec_rs::bc3`, which uses `(6a0+a1+1)/7` and matches neither model.

## Note U — M6 Track E: NIF-aware alpha and fewer confirmations (plan rulings, 2026-10-12)

Rulings of `docs/superpowers/plans/2026-10-12-sdc-m6-nif-alpha-confident.md` on spec `docs/superpowers/specs/2026-10-12-sdc-m6-nif-alpha-confident-design.md` (owner-approved 2026-10-12), with the owner overrides O1 and O2 made while building.

- **U1 No suffix is the colour member, at High (owner, 2026-10-12: "No suffix should equal diffuse by default").**
  - This reverses Note H's "a bare stem is only a guess". A bare stem suggests `diffuse`, `pbr_albedo` (a PBR set) or `ui_sky` (`interface/`, `sky/`) at High, like `_d`.
  - `landscape_diffuse` and `skin_albedo` were already High. `hdr_sky` (a float source under `sky/` may be an HDRI) and `_i` stay Medium.
  - Names that break the conventions (a normal map without `_n`) now resolve as colour maps; fixing them is the mod author's job (owner).
  - Note H's colour-term raise was removed: every colour member it raised is now High.
- **U2 `_coat` is coat colour + strength (owner, 2026-10-12).**
  - `pbr_subsurface` keeps alpha policy `data` (`sdc-base`), so a varying alpha is kept as the strength and stays BC7 under compact (T3).
  - The reason reads "exporter suffix _coat (coat colour + strength); the PBR coat map (_s)".
  - The slot card reads "Coat colour + strength" when its source file is a `_coat` (by file name).
- **U3 The GUI accepts High suggestions (owner, 2026-10-12).**
  - `AppModel::accept` is `High`, and `request()` sends it. Medium and Low suggestions still ask.
  - Explicit choices win: `[files]`, rules and `--set subtype` decide before any suggestion.
  - The CLI is unchanged (`--accept-suggestions high`).
  - The M3-era GUI test fixtures set `accept = None` to keep their undecided members.
- **U4 "(auto)" (plan, 2026-10-12).** There is no new engine field. A job shows "(auto)" in the Sub-type row's provenance column when all three hold, with the top suggestion's reason on hover:
  - its `provenance.subtype` is `suggestion`;
  - its top suggestion is High;
  - it does not need a sub-type.
- **U5 Mesh alpha evidence (plan, 2026-10-12).**
  - `BatchRequest::mesh_alpha` is keyed by `shader::colour_mesh_key`, the output stem, so a renamed `x_albedo.png` is keyed `x`. The CLI sends none.
  - The GUI reads it with raw SQL (`meshes::mesh_alpha`). The query is composed with `format!` from a static `FROM` fragment shared with `mesh_votes` (`SLOT0_SOURCE_SHAPES`), and the key is a bound parameter, so no user data reaches the SQL text (review ruling, Task 4). The fragment selects:
    - lighting shapes using `<key>.dds` in slot 0;
    - non-derived copies of enabled origins;
    - TruePBR excluded;
    - one entry per NIF path, block and alpha property.
  - Unlike the votes, every lighting shader type counts.
  - NiAlphaProperty bit 0 is blend and bit 9 is test. A shape with no property is no alpha.
  - A database read error gives no evidence, as in `mesh_votes` (the Meshes view reports database errors), so the alpha question is asked as before. Surfacing the error in the planner is parked as minor.
- **U6 The rule (owner, 2026-10-12; threshold range owner O2).**
  - Any shape with alpha beats shapes without; there is no vote.
  - Blend only gives `blend`; test only gives `cutout`; blend and test anywhere, on one shape or two, give `cutout_blend`; no alpha gives `remove`.
  - The threshold is the largest of the testing shapes (the stricter cutoff), clamped to 1..=254. Spec gap: a NIF may say 0..255.
  - O2 (owner): the valid `alpha.threshold` range widens from 5..=200 to 1..=254 (`CFG-INVALID` outside it), replacing the plan's clamp to 5..=200. Earlier notes that say 5..=200 describe the typical range.
- **U7 The `mesh:alpha` layer (plan, 2026-10-12).**
  - **Provenance.** `Provenance::MeshAlpha` (`mesh:alpha`) sits between `shader:<id>` and `suffix:<term>`.
  - **Where.** It is applied after the analysis-alpha step and `skin_wet_form`, before the shader layer, and only for `alpha_from_mesh` sub-types where `alpha.policy` is not explicit.
  - **What beats it.** User presets, configs and rules (before it), and `[files]` and `--set` (after it).
  - **The analysis `remove` keeps priority** (no alpha channel, or alpha constant 255). This extends spec §4.3's "source without alpha" to constant 255.
  - **What it beats.** A shader's alpha default (hair `cutout`).
  - **Explicit.** It counts as explicit, so the alpha question is not asked. `alpha.threshold` gets the same provenance unless that field is explicit.
- **U8 Compact (owner O1, 2026-10-12; replaces the plan's BC7 ruling).**
  - Under Compact, a mesh-decided `remove` on a varying-alpha colour member is opaque colour: it goes BC1 at full size, like other opaque colour. No `LOSS-ALPHA-DATA` is raised (explicit remove, P25).
  - Mechanism: rules are matched on the file's real source alpha first. Only when no explicit layer (preset, config, `[files]`/`--set` override, or a matching rule) sets `alpha.policy` does a mesh `remove` make the rules see alpha `none`, so Compact's BC1 rules match. A rule that sets `alpha.policy` is never suppressed, and no BC1 + blend `CFG-INVALID` arises from it (Task 3 review fix).
  - A mesh `cutout`, `blend` or `cutout_blend` keeps the varying-alpha rules. A mesh `cutout` never reaches a constant-255 source (U7), so an opaque RGBA diffuse stays BC1.
- **U9 Two-pass GUI (plan, 2026-10-12).** The first plan of a folder asks the alpha question until the scan's evidence is read. `EngineInputs` carries `mesh_alpha`, so the automatic re-plan settles it (`refresh_mesh_votes` reads votes and alpha together and reports whether either changed).
- **U10 Versions (plan, 2026-10-12).**
  - There are no new codes (75).
  - The workspace goes 0.5.0 → 0.6.0, so `if-changed` re-encodes.
  - There is no manifest change: the up-to-date check compares `engine_version` and `profile_sha256`, and both change.
  - `schema/` was regenerated only for the `Provenance` doc text.

## Note V — M6 Track F1: TruePBR panel polish (plan rulings, 2026-10-12)

- **V1 What matchN holds (plan, 2026-10-12).**
  - PGPatcher compares `matchN` exactly with the unpatched mesh's raw slot path (no `strip_pbr`). So the value is the set's derived diffuse path, which comes from the source, not from the set id or the output.
  - `match1` is that path, with its `_d`/`mask` kept.
  - Slot N > 1 is that path with a trailing `_d` cut, plus `_<canonical suffix>` (`match2` = `…\rock01_n`; `facemask` → `facemask_n`).
  - Proven against pgpsim (`slot_matches_hit_the_unpatched_mesh_and_land_on_the_sdc_prefix`).
- **V2 Storage and writing.**
  - `match1` absent means the derived path (on by default; old projects get it too). `match1 = ""` means off. `match2`–`match8` are on when present.
  - Stored values have no `.dds`.
  - The JSON gets the value without a leading `textures\` (any case) and with `.dds` exactly once.
  - `match_diffuse`/`match_normal` are never written (`NOT_EXPOSED`). There is no migration (owner, 2026-10-12: nobody has used them): old `match_diffuse`/`match_normal` values in a project are ignored, and `match1` falls back to the derived default.
- **V3 Prefix.**
  - The prefix comes from the lowest active `matchN`, with that slot's suffix cut, under `textures\pbr\`.
  - With no slot match, it is the derived path.
  - `rename` is a new file name (owner, 2026-10-12): no folders (a `\` or `/` blocks export), one `.dds` cut. It replaces the last component of the prefix above, so the folder stays the matched one: `textures\pbr\<folder of the matched path>\<name>`.
  - The JSON's `rename` is that full prefix, not the typed name. PGPatcher substitutes `rename` for the whole matched path when the entry has no `match_diffuse`/`match_normal` (always, for sdc), so every hit (any `matchN`, or `path_contains`, with or without a slot match) lands where sdc writes. Proven against pgpsim (`a_renamed_entry_lands_on_the_sdc_prefix_for_slot_matches_and_path_contains`).
  - When active matches derive different prefixes and `rename` is empty, a warning says the output prefix depends on which match hits first, and to edit the match paths so they point at the same texture. It does not block export.
  - Open point, still unverified against real PGPatcher: PGPatcher prepends `\` to `rename` before substituting, so pgpsim's matched path for a renamed entry starts with a backslash (`\textures\pbr\…`); the round-trip tests compare without it. Before F1 this applied only to the old whole-prefix rename; now every renamed set goes through it. Check one renamed set in real PGPatcher (does the NIF get `\textures\pbr\…`, and does its file-existence check accept it?).
- **V4 No match.** A set with no active `matchN` and no `path_contains` shows "this entry matches nothing", asks before export, and is left out of the JSON. The schema's `hasMatcher` refuses it. Its textures still export.
- **V5 Number ranges.**
  - 0–1: `coat_strength`, `subsurface_opacity`, `fuzz.weight`.
  - Min 0: the other 15 number keys. That includes `smooth_angle` and `auto_uv`, which fit no class.
  - No range: `glint.log_microfacet_density`, a log value that can be negative (`NumRange::Any`).
  - No CS or PGPatcher limits are documented.
- **V6 Clamping.**
  - The number box has `DragValue::range`, and a typed value outside the range shows "clamped to ≥ 0" / "clamped to 0–1". A stored out-of-range value shows the same note (ruling: it explains why the JSON differs from the box). `Any` has no note.
  - A stored out-of-range value is shown as it is (`clamp_existing_to_range(false)`). An equal value is no edit, so drawing never dirties the project.
  - The writer clamps.
  - `pgpsim::range_warnings` mirrors the ranges (it skips `glint.log_microfacet_density` and the removed `env_map_*` keys), and a GUI test (`pgpsim_warns_on_exactly_the_gui_ranges`) pins the two together.
- **V7 Layout.**
  - The TruePBR box shows the visual settings, then **Matching**, then **Advanced TruePBR settings** (collapsed: `path_contains`, `nif_filter`, `rename`, UV, vertex, locks, inner UV scale, coat colour).
  - The Advanced drawer's "PGPatcher (advanced)" section is gone.
- **V8 Slot cards.**
  - The set editor's cards (the 9-slot row; the Sets-column pill shows members, not slots) are the picked shader's usable slots, filled or empty, plus filled slots it cannot use (amber outline, hover reason).
  - They are ordered by PGPatcher slot 1–8.
  - Usable slots are the shader roles' sub-types mapped to slot cards (`usable_slots`). Roles matched by suffix only map to none.
  - A fallback pick uses the fallback shader. With no pick, only the filled slots show.
- **V9 Versions.** No bump (0.6.0): no engine change, and a bump would re-encode every texture through `engine_version`. No new codes (75). No `schema/` change.
- **V10 The + menu (owner, 2026-10-12).**
  - It offers slots 3–8 without a row. A slot the set fills starts from `slot_match_path` (V1); any other slot adds an empty row (`matchN = ""`, N ≥ 2) that says "this row matches nothing until a path is entered".
  - An empty row is not a match: it is not written to the JSON and does not count for the prefix or the warnings. Clearing a row's text removes it.
  - A filled slot is prefilled only when PGPatcher's slot-N list strips its suffix, so a prefilled row never splits the prefix (controller ruling). Subsurface `_s` in slot 8 is not in that list (`_b`/`_bl`/`_flow`; pgpsim, `a_match8_on_an_s_map_keeps_the_s_in_pgpatchers_prefix`), so slot 8 starts empty. A typed path that splits still gets the "match8 does not strip _s" and divergent-prefix warnings.

## Note W — M6 Track F2: RNM normal layers (plan rulings, 2026-10-12)

- **W1 Blend point.** The blend runs on the *preprocessed* base: after `preprocess_with`, so after the base's own `flip_green`/`reconstruct_z` and its `resize`, and before the mips. Layers resample to that size. The spec's "base size" is the base as it will be written, so a `half` preset blends at half size, which is cheaper and gives the same result. The base vector is normalised before blending.
- **W2 Escape hatch.** A layer file whose sub-type is set by a rule, `--set` or `[files]` converts on its own, as a packing input does (`resolve_file`). This rescues a real texture that happens to end in `_bent`.
- **W3 Selector.** `rnm.target` is a per-file selector like `reencode`, not a `Profile` field. It is read with `rnm_target_requested` from the layer file's `[files]` entry or a file/path `--set`. A sub-type scope refuses it.
- **W4 Who gets a layer.**
  - A layer with no prefix defaults to `n`, falling back to `cnr` when the set has no `_n` base.
  - Prefixed layers default to their prefix.
  - A layer whose effective targets name no base in the set gets `RNM-NO-BASE`.
  - In one stack, the candidates for one layer id are ranked: a prefix naming this base, then no prefix, then the other prefix, with ties broken by path. The first wins, and each loser gives `RNM-LAYER-SHADOWED` on the base job.
- **W5 Bases.** `n` = `normal`, `pbr_normal`, `landscape_normal`; `cnr` = `coat_normal`. A base job must be planned (not `Error`) and not packed. Any shader `Eye` set makes every layer `RNM-IGNORED-EYE`.
- **W6 Settings shape.**
  - Every `rnm.*` field is optional at its default, and `Profile.rnm` is skipped from JSON at its default, so no existing profile hash changes.
  - `rnm.order` drops duplicates (the first is kept) and appends the missing ids in the default order `bent, bump, bump2, detail, detail2`. The resolved profile stores it normalised, or not at all when it equals the default order.
  - `rnm.*` on a sub-type that cannot be a base is reset to the default, so a global `--set rnm.enabled=false` does not re-hash every diffuse.
- **W7 Ranges.** `rnm.<layer>.strength` must be within 0..=2 and finite. `rnm.bent.ao_power` must be within 0.1..=8. Either one out of range gives `CFG-INVALID`.
- **W8 Bump maths.** The bump slope is computed at the bump's own size: height = mean RGB, `slope = k·(h[+1] − h[−1])`, +v up the image, with green flipped to DirectX. The vectors are then resampled and renormalised. `flip_green` on a bump flips the computed Y (an "invert vertical" escape) and defaults to off: a bump is height, not an OpenGL/DirectX normal, so it never follows the base's `flip_green`. Normal layers default to the base's. A bump mask scales the slope, as rnm.py does.
- **W9 AO.**
  - The AO source is, in order:
    1. the set's raw AO file (vocabulary role `PackingInput(Ao)`), channel R;
    2. otherwise the set's `_rmaos`/`_rfaos` **file**, channel B.
  - It is read only for a `bent` layer in `ao_blend` mode with `rnm.bent.ao = true`.
  - `RNM-AO-MISSING` (info) goes on the base when that mode wants AO and there is none.
- **W10 Resampling.** Downscaling uses `mips::area_resample`. If any axis grows, a new bilinear filter is used (texel centres, clamped edges). Each axis scales independently.
- **W11 GUI.**
  - Layer jobs produce no `TextureItem`. Their set-level warnings (`RNM-NO-BASE`, `RNM-IGNORED-EYE`, `RNM-MASK-ORPHAN`) are kept per set in `PlanView::rnm_notes` and shown under the set grid.
  - Base-level codes show in the RNM section.
  - The target switch writes `rnm.target` on the layer file's key.
- **W12 Versions.** 0.7.0: any set with layers now converts differently. Codes 80.
- **W13 `_norm`.** `_norm` (rnm.py's base name) is a vocabulary term `Normal(None)`, like `_normal`. Its green convention is unstated, so it asks `IN-GREEN-UNSTATED` once. rnm.py reads `_norm` as OpenGL, but sdc does not guess.
- **W14 Memory.** Memory admission counts the base only. Layers load one at a time at encode, which is a `shortcut:` comment in `pipeline.rs`.
- **W15 Test fixtures.** Shared engine RNM test helpers live in `crates/engine/tests/rnm_support/mod.rs` (one copy; `plan_dir` always recursive).
- **W16 AO identity.** The AO source's manifest role carries its channel (`bent.ao.r` for a raw AO map's R, `bent.ao.b` for `_rmaos`/`_rfaos` B), so swapping a byte-identical file between the two re-encodes the normal.
- **W17 GUI storage.** The per-set RNM notes and layer keys are `AppModel.rnm_notes` / `AppModel.rnm_keys` (the model keeps no `PlanView`); the RNM section applies its own edits only when a value changed.
- **W18 Roughness bake after the blend.** Owner (2026-10-12): the roughness variance bake reads the `_n` after its RNM blend (plan_rnm runs before plan_bakes; `BakeJob.rnm`); the bake identity and the roughness member's sources include the RNM inputs, so a layer edit re-bakes it.

## Note X — Complex grass sub-type (plan rulings, 2026-10-13)

Spec `docs/superpowers/specs/2026-10-12-complex-grass-design.md`, plan `docs/superpowers/plans/2026-10-12-plan-complex-grass.md` (rulings R1–R12 there are binding; summarised here).

- **X1 Shape and build.** `complex_grass` (Mixed-Packed, BC7 UNORM) is one power-of-two image with height = 2 × width. Each half is mipped on its own and the levels are stacked, so no filter crosses the seam. Top half: sRGB colour with coverage-preserving alpha. Bottom half: DirectX normal, renormalised, with alpha as specular data. The chain ends at 1×2, one level short of a full `w`×`2w` chain (R6). Other shapes are `GRASS-SHAPE` (error). A size-changing `resize` is `CFG-INVALID` (R7).
- **X2 Detection texel.** Community Shaders detects complex grass from texel (0, height−1) of mip 0 (`abs(length(rgb*2−1) − 1) < ComplexGrassThreshold`, owner's value 0.03). The encoder re-encodes that block at the slowest BC7 level when it decodes at ≥ 0.02. A file that still fails is written and fails validation with `VAL-GRASS-DETECT` (R8, owner-confirmed). ENB is assumed to use the same detector (owner ruling).
- **X3 Vanilla.** The sub-type stays encodable under vanilla, with a `GRASS-VANILLA` warning. It is never auto-tagged there (R1, owner-confirmed). The pickers never offer it on any target (`policy::offered`; owner 2026-10-13): the set shader `complex_grass` or the GRAS auto-detect chooses it, and an explicit other set shader suppresses the auto-detect.
- **X4 Auto-detect.** The GUI passes the textures used as the slot-0 diffuse of a GRAS record's model (`Store::gras_diffuse_textures`, from the plugin scan) in `BatchRequest::gras_diffuses`. A file is tagged only when it is in that list, is 1:2, its source corner passes 0.03, and at least 99 % of its bottom half is unit normals. It is a High suggestion, so any user tag wins, and the CLI never auto-tags (R11). The suggestion runs after the packing-input/normal-layer gates, so a layer or raw-map file is never retagged.
- **X5 GRAS query.** A PGPatcher-derived NIF winner is read through its source copy. TruePBR shapes are excluded and origins must be enabled, as in every other slot-0 query.
- **X6 Header patch.** `PatchRequest.complex_grass` (set by `sdc patch-header --subtype complex_grass`) accepts exactly one level short of the full chain for a 1:2 file.
- **X7 No version bump.** No preset and no existing output changes, so the engine version stays 0.7.0 and nothing re-encodes under `if-changed`. Codes 80 → 86.
- **X8 Accepted limits.** `VAL-COVERAGE`/`base_coverage` measure the whole stacked level, so the bottom half's specular alpha counts toward coverage (warnings only). A grass file whose stem ends in a raw-map or normal-layer suffix is treated as that, not as grass.
- **X9 Map check.** A set with a `complex_grass` member gets no `SHADER-EXTRA-MAP`/`SHADER-MISSING-MAP`: the grass shader reads diffuse and normal from the one file, so the lighting shader's map roles do not apply (final review I1).
- **X10 Auto-detect needs a `textures` folder.** Like mesh votes and mesh alpha, the GRAS match uses the input's game path, so a folder opened without a `textures` ancestor is never auto-tagged; tag by hand there.
- **X11 CLI vs GUI.** The CLI never fills `gras_diffuses`, so a CLI run over a folder the GUI exported with auto-tagged grass resolves those files as diffuse and re-encodes them. Tag grass in `sdc.toml` `[files]` when both tools are used (same class as mesh-alpha evidence).
- **X12 complex_grass alpha.** No alpha question; varying alpha defaults to cutout (top half), bottom data; owner's mods: 159/193 loose grass diffuses cutout, 32 mostly hard (2026-10-13). Supersedes plan ruling R5's alpha question.

## Note Y — audit fixes (plan rulings, 2026-10-13)

- **Y1 Output stamp (audit 5; owner D1).** A record stores the output's `SourceStamp` (size, mtime) taken after the rename. `if-changed` keeps an output only when the stamp still matches, and this check runs before any hashing. A record without the field is stale once: the first run after upgrading re-encodes each recorded output one time, then never again. The engine version stays 0.7.0 (owner: "stay on .7"). A same-size, same-mtime replacement is not detected (metadata only, by design).
- **Y2 Sidecar (audit 15).** Under `--sidecar`, an up-to-date output with no sidecar gets one, written from its stored record (no re-encode). If that write fails, the file re-encodes, and the encode reports the error.
- **Y3 Parallel decisions (audit 9).** The `if-changed` checks run on the batch pool. Skip events are still emitted in plan order. The source is not re-used by `process_file`: a file found stale by its hash is hashed once more, and only when it really changed.
- **Y4 Discovery (audit 6).** An unreadable entry under an input folder is skipped with `IN-UNREADABLE` (warning, batch-level), and the batch continues. An unreadable input root itself is still an error. `inspect` skips the same entries silently. The GUI shows no notice for skipped entries yet (follow-up). Per-thread glob matcher cache: it is never evicted (shortcut: bounded by the distinct patterns of one run; evict if a long-lived process ever feeds it unbounded patterns).
- **Y5 RNM strength (audit 12; owner D2).** `decode` clamps z to ≥ 0. At strength ≤ 1 this is a no-op. Above 1 it deviates from rnm.py, which has the same formula with no clamp: a steep texel becomes horizontal instead of pointing into the surface. The strength range stays 0..=2.
- **Y6 Coverage under `smallest` (audit 13).** This amends T5. `validation.mips = smallest` checks PSNR on the last level only and coverage on every level, so `summary.coverage` has one entry per level again. The schema doc text says so.
- **Y7 GUI caches (audit 7, 8).** Set views and export blocks are cached under `(items_rev, sets_doc rev, root, export_root, target)`: the view-cache revision key. `SetsDoc` revisions come from one global counter, so a replaced document never reuses a revision. Code that mutates `items` or `sets` in place calls `AppModel::invalidate_views()`. Engine thumbnails are `Arc`-shared. A GUI plan evicts cached files outside its inputs, and packs not in the plan (uncancelled plans only). `ThumbCache` clears when `items_rev` changes.
- **Y8 Undo (audit 10, 11).** An undo or redo ends the merge run. "Accept all suggestions" is one history step and one re-plan, whatever the count.
- **Y9 `--nvtt` (owner D3).** `sdc convert --nvtt` is a usage error (exit 2) until M5. `sdc encoders --nvtt` (detection only) is unchanged. Superseded by Note NV (M5): `--nvtt` selects nvcompress.
- **Y10 S4 spike (owner D4).** `spikes/s4-scalar` is deleted. It can be rebuilt from the pinned commit and SHA-256 in Spike S4.
- **Y11 Bench (audit 18, left-out).** Corpus tests skip (and say so on stderr) when `SDC_BENCH_ROOT` is unset and the default folder is missing; an explicit root that is missing fails. The bench uses the engine's PSNR (`engine::validate::psnr`), so validation and the engine-parity gate cannot drift. `.gitattributes` marks `crates/*/tests/fixtures/**` and `crates/*/tests/data/**` as `-text`, which also reaches the NIF and render3d fixture folders (a root file; only the eol attribute).

## Note Z — M6 Track F3a: multi-select and bulk settings (plan rulings, 2026-10-13)

- **Z1 Selection.**
  - `AppModel::selection` stays the active entry. `AppModel::multi` (keys of one kind, in the order they were added) is in effect only with two or more keys that include the active one.
  - Plain click, `select_item` and `select_set` end it. Ctrl toggles.
  - Shift ranges in the displayed order (search and filters applied for textures) and keeps the active entry.
  - Ctrl+A selects what is visible; Esc clears. Neither acts while a widget has focus, a dialog or popup is open, or a texture is expanded.
  - A plan drops gone keys; a gone active key hands over to the last remaining one.
  - The Textures heading counts the selection and how many of it the filter hides.
- **Z2 Bulk rule.**
  - A bulk write of a field skips a texture with no job, with the field hidden (Coverage outside cutout), with the field's `FieldView::lock`, or (for a filter write) with that filter barred.
  - One rule (`bulk::skip_of`) feeds both the panel's "applies to K of N" and the apply.
  - Locks and not-applicable cases are one category at this base. The hover lists "locked on M of N: <reason>".
  - The complex_grass Sub-type lock (fbaceb4, merged into this branch as 4404dc5, ledger R1) applies with no change, because it is a `FieldView::lock`. If the owner rejects grass round 2, revert that merge.
  - A bulk Clear (reset) is not blocked by a field lock (R6): clearing a stale `[files]` entry is harmless and is the only way to remove it. Only a Set is skipped when locked.
  - A bulk Set validates its value the way the single-texture path does (R7): a null, a bad sub-type or an unparsable `mips.filter` is reported, not written.
- **Z3 One step, one plan.** Every bulk command runs in `AppModel::batch_edit`: one snapshot, `edit_docs` without steps or plans, one step, one `replan`. "Accept all suggestions" uses it too. `edit_docs` takes no snapshot while batching.
- **Z4 Sub-type in bulk** runs `set_field` → `edit_subtype` per texture, so the PBR follow-on applies. A texture's own write wins over a follow-on.
- **Z5 Copy from active.**
  - It offers the fields of `fields::all_fields()` that the active texture's `[files]` entry sets, all ticked.
  - `allow`, `rnm.*`, `pack.*` and `channels.*` are not offered: they belong to one file.
  - It is textures only. For sets, "Select members of kind" first (R2: copying settings between sets is out of scope, a known gap).
- **Z6 Reset all** removes whole `[files]` entries but keeps acknowledged warnings (`allow`, R3): acknowledgements are decisions, not settings. A `[files]` sub-type is cleared through `clear_field` first. Field locks do not apply (R6). Slot locks always apply: a texture filling a locked slot is skipped and reported (Z10). `sdc-project.toml` is not touched.
- **Z7 Version.** It stays 0.7.0 (R4). `ENGINE_VERSION` is the workspace version and is part of `is_up_to_date` (`api.rs:603`), so a GUI-only bump would re-encode every output under `if-changed`. No presets-changelog entry.
- **Z8 Out of scope** (decisions D6):
  - bulk recipes or presets, auto-sync, hidden modifier keys, regex;
  - path, rename and output edits (F3b);
  - bulk editing of the Advanced drawer's fields and of set-level settings;
  - modifier clicks on a pill's member thumbnails.
- **Z9 Slot context menus (R8, owner addition).**
  - Set-editor cards and a pill's member thumbnails have a right-click menu: the slot commands (From this set, Path, Clear), a "From another set" link, and Lock/Unlock.
  - **Link value (R9).** "From another set" writes the linked member's `engine_inputs` output path when it has one (PBR, P11); otherwise its `textures\...` game path from `job.game_path`; with neither (no `textures` ancestor) the entry is disabled. A link never dangles.
  - **Exact slot kind (M4).** A link needs the same slot kind. Shared slots 3 (glow / skin_sk) and 7 (coat / fuzz) never offer the other kind.
- **Z10 Slot lock (R8, R10).**
  - A lock keeps the slot's fill and refuses a drag in or out. Slot commands refuse a locked slot too, one undo step each.
  - Bulk edits skip it with the reason "slot locked", or "slot locked in <set>" for a fill borrowed from another set. This includes Reset all (Z6): slot locks always apply, unlike field locks.
  - Automatic re-typing respects it, through the same rule (`locked_items`), so a texture borrowed into another set's locked slot is not re-typed either. A manual single sub-type edit of the locked slot's texture is allowed: the lock covers automatic changes only.
  - It is stored in `sdc-project.toml` as `[[sets]] lock = ["slotN"]` and is reserved there. It never reaches the PGPatcher JSON.
- **Z11 Encoding follows type (owner, 2026-10-13, Task 9).**
  - When a plan changes a texture's sub-type (a set shader, a slot command, a drag, the PBR follow-on, a manual or bulk sub-type edit), its explicit `[files]` format and tag are checked against `policy::allowed_encodings` for the new sub-type. A pair it no longer allows is cleared (`clear_field("format")`, which clears the tag too), so the new type's default applies. An explicit pair that is still allowed is kept. Other fields are not touched.
  - A texture in a locked slot (Z10), or whose Format row is field-locked, is not touched; the note reports it as kept.
  - The sub-types before a plan chain are kept by `apply_plan`. They are compared with the final pass when the chain settles in `finish_plan`, so a type that is only intermediate (pass 1 of the two-pass flow) is never cleaned up after. The cleanup is one batched edit and one replan. Its own one-shot guard keeps that replan from cleaning again, and it does not use the `auto_replans` budget of the output-path replan.
  - Undo: the cleanup is folded into the user step that started the chain. That step is tracked explicitly: an edit that records a step and re-plans marks the chain. A change no step caused (scan votes, a document changed outside history) is its own step, "Encoding follows type". Plans of an undo, redo, open or reload never clean up.
  - A short note follows the status line ("g.png: complex grass → pbr albedo; format reset to the default (BC7 · sRGB)."). Several textures read as one count line with each listed on hover. No dialog. The next edit, an undo, redo, open or reload dismisses it.
  - Merge of main (Task 9 Part A): the bulk Format row offers the (format, tag) pairs every selected texture allows (`bulk_encodings`, from `fields::encoding_options`); when they share none, every pair any of them allows. A pair a texture's sub-type does not allow skips it ("not valid for this map", applies to K of N). Copy settings copies the format with its tag the same way (R13): the pair is written through `set_format`, the tag is not copied on its own, and a texture that does not allow the pair is skipped.

## Note AA — M6 Track F3b: input aliases, output names and the PGPatcher rename (plan rulings, 2026-10-14)

Spec `docs/superpowers/specs/2026-10-13-sdc-m6-f3b-design.md`, decisions `…-f3b-decisions.md` (D1-D8), plan `docs/superpowers/plans/2026-10-14-sdc-m6-f3b.md`.

- **AA1 Input alias (owner, 2026-10-13: "Alias in sdc.toml").** Input fixing (edit path, input rename) is one feature: `[inputs."<real path>"] as = "<path>"` in the project `sdc.toml`. Files on disk are never renamed or moved.
  - Applied at discovery, before canonicalising and grouping (`discover::apply_read_as`, planning step 1a). The alias becomes `rel`, `game_path` and the output; the decoder reads the real `path`.
  - Keys match like `[files]` keys (`project_rel` + `path_key_matches`: case-insensitive, with or without the extension). The alias keeps the file's own extension (one may be left out); a different image extension is refused.
  - **Identity stays the real file.** `InputFile::real_rel` keeps the name found on disk, and `project_rel` uses it, so `[files]` entries, set edits (`add`/`remove`, borrows) and `OutputKey::File` keys never move with an alias. Adding or clearing one needs no re-keying.
  - **Rule globs see the alias** (they read `rel` and `game_path`): a rule for `**/*_n.png` matches an aliased `wall.png`. The alias says what the file is.
  - Refused (alias ignored, file planned under its real name): absolute, `..`, outside the folder the file was found in, another extension, empty, or equal to another file's real or aliased path. Reported as `IN-ALIAS-INVALID` (Warning, batch-level; D1 had per-file Blocking, which the engine has no form for short of skipping the file, spec §9). A key matching no file is `IN-ALIAS-STALE` (Info). Catalogue 87 → 89.
  - A user config may not hold `[inputs]` (like `[files]`). The CLI reads it. `schema/config.schema.json` gained `inputs` / `ReadAs`.
- **AA2 Output names.** A set's outputs take a new base name; each output keeps its folder and Skyrim suffix (`renamed_output`). Plan-only: nothing on disk is renamed; outputs written under old names stay (the manifest and `OUT-UNKNOWN-EXISTING` rules apply).
  - Non-PBR sets (and every set off CS): `output_name` in the set's `[[sets]]` table (a reserved key, never in the JSON). `engine_inputs` now sends explicit `output_paths` on every target for those sets.
  - A name ending in a map suffix (`wall_n`) is refused (`check_output_name`): the suffix belongs to each output, and the name would read back as base plus suffix, so a re-plan would not be idempotent. Windows-illegal and reserved device names (`CON`, `COM1`…, any extension) and a trailing dot or space are refused (`check_file_name`).
- **AA3 Auto PGPatcher rename.** On a CS PBR set an output rename writes the set's PGPatcher `rename` key (V3: a file name; the prefix, the outputs and the JSON follow) and removes any `output_name`. A stale `output_name` on a set that later becomes PBR is ignored; the set editor says so with Clear.
- **AA4 Rename dialog.** One dialog, Inputs or Outputs mode, from the tile and pill menus and F2 (Outputs for selected sets, Inputs for textures). Find/replace (literal, or the `regex` crate: linear time, `$1`/`${name}`), prefix, suffix and case run on the base name only (the part before the detected suffix or exporter term); a row can be typed whole or excluded. The preview and Apply share `rename::plan_rename`.
  - Blocks: collisions inside the batch and with every other file's real or aliased name (Inputs) or every other set's outputs (Outputs); illegal names; a changed extension; a name ending in a suffix (Outputs). An invalid pattern disables Apply.
  - Warns: an input whose name-read kind changes ("read as _n now"); a non-PBR output whose colour map scanned meshes still use in slot 0 (`meshes_using`, from the scan's mesh evidence).
  - Skips: packed outputs in Inputs mode (no file); a texture filling a locked slot in Inputs mode (an alias could pull it out of the slot). Outputs mode changes no membership and ignores slot locks.
  - Apply is one undo step and one plan (`batch_edit`, Z3). The recipe is offered again on the next open (this session only; no saved recipes).
  - Tiles show "read as <name>"; *Clear input alias* in the tile menu removes aliases. (Spec §7 also had a Clear button in the settings header; the menu entry covers it.)
- **AA5 Version.** Stays 0.7.0 (Z7): unaliased outputs are unchanged. No presets-changelog entry.
- **AA6 Out of scope (D7).** Renaming or moving files on disk; output folder edits; counters, tokens, trim, Title case; saved recipes; a CLI command that writes aliases; aliases in presets or user configs.
- **AA7 Owner checks.** (1) Answered 2026-10-14, see AA8: real PGPatcher 1.2.0 applies a rename only next to a `match_diffuse`/`match_normal` (or old `texture`) key; sdc's old form (matchN or path_contains plus a whole-path rename) patched nothing. (2) Open: one aliased normal map in a real mod shows correctly in game.
- **AA8 A renamed set's entry carries `match_diffuse`** (owner, 2026-10-14). Evidence from the owner's load order (PGPatcher 1.2.0, read-only): all 713 rename entries in 169 JSONs pair `rename` with `texture`/`match_diffuse`/`match_normal`, and a frost atronach entry of that form wrote `textures\pbr\actors\frostatronach\frostatronach*.dds` (no leading `\`, lower case). The owner's test entry `{"match1": "dlc01\\clothes\\vampire\\robesf.dds", "rename": "textures\\pbr\\dlc01\\clothes\\vampire\\robesf2"}` matched the robe (so a `matchN` without `textures\` does match, as V2 says; PGPatcher source TP:209-212) and the files were present, yet no mesh was patched. The log names the cause, 48 times over 16 meshes: `Texture "\textures\pbr\dlc01\clothes\vampire\robesf2.dds" does not exist from PBR json … (Skipping)`. Without `match_diffuse`, PGPatcher substitutes `\` + rename for the whole path (pgpsim `prefix()`) and its file check fails. So when a set has a rename, the writer adds `match_diffuse` (owner: `texture` is the old name for it; PGPatcher copies `texture` into `match_diffuse`) = the matched texture (the lowest active match, else the derived diffuse, slot suffix cut) and writes `rename` as the renamed path; both are relative to `textures\pbr\`, like real mods. The match slots stay as the entry's matches (owner: matchN is the default; the PGPatcher wiki, Mod Authors `rename`, says rename renames "the matching part (from match_diffuse or match_normal)", which is why a renamed entry needs `match_diffuse` too). PGPatcher then keeps `textures\pbr` and appends the rename. Two costs. PGPatcher cuts each hit's path by `match_diffuse`'s length, so only meshes hit through the first matched texture are sure to get the renamed files (a hit through `path_contains` or a match slot naming another texture gets a wrong path; the set editor warns). And `match_diffuse` also matches by diffuse, so a set with `match1` off still matches its diffuse once renamed.

## Note NV — M5: NVTT Production backend (plan rulings, 2026-10-13)

Spec `docs/superpowers/specs/2026-10-13-sdc-m5-nvtt-design.md` (owner-approved, §9 at its proposed defaults). Supersedes
Note Y9: `sdc convert --nvtt` works.

- **NV1 Scope.** BC7 and BC6H UF16 only, Production only, one `-nomips` process per engine-built level. NVTT is never a
  default and never suggested.
- **NV2 Locate.** Explicit path (`--nvtt`, GUI NVTT path; missing → not found, no fallback), else `nvcompress.exe` on
  `PATH`, else `C:\Program Files\NVIDIA Corporation\NVIDIA Texture Tools\nvcompress.exe`. Under `cargo test` the target
  folder, which holds the fake `nvcompress`, is on `PATH`, so the real-NVTT tests pass the default path explicitly.
- **NV3 Version.** The probe runs `nvcompress` with no arguments (it exits 1; Spike S7b), parses the banner
  `NVIDIA Texture Tools <x.y.z>` and requires the usage to list every flag NV4 passes. Tested = 3.2.5. Untested →
  `ENC-NVTT-VERSION` (blocking, allowable). Not found or probe failed → `ENC-NVTT-MISSING` (blocking). The batch probes
  lazily (`NvttCell`): a batch with no profile on NVTT never starts nvcompress.
- **NV4 Flags.** `-silent -nomips -production`, then `-bc7` or `-bc6`; for BC7 `-noalpha` when `alpha.policy = remove`
  and `-alpha` for every policy that keeps alpha (coordinator, pending the owner's card: nvcompress weights alpha 0
  unless told the input has it); `-nocuda` always (S7, S7b: byte-identical, 0.13 s less per process). No colour flags;
  the engine writes the header.
- **NV5 Feed.** Each level as a DDS with a **legacy** header (S7b: NVTT reads uncompressed DX10 DDS as garbage): BC7 as
  B8G8R8A8 via `dds_bytes(.., Legacy)`, BC6H as RGBA16F with FourCC 113, written by `nvtt.rs` (the engine's writer has
  no legacy RGBA16F form). One temp folder per file, `%TEMP%\sdc-nvtt-<pid>-<n>`, removed on drop.
- **NV6 Post.** `force_opaque_rotated` and the complex-grass corner guard run on NVTT BC7 output (`bc7_post_fix`); the
  mode-6 byte patch does not.
- **NV7 Slot.** One nvcompress process at a time per process (static mutex). Shortcut: fixed at 1; make it a setting if
  cores idle.
- **NV8 Failure.** `ENC-NVTT-FAILED` (error, Compress): the `file_failed` error starts `ENC-NVTT-FAILED:` and carries the
  exit status and the last 5 output lines. No fallback to the built-in encoder. Cancel kills the child. No timeout
  (shortcut: add one if a hung nvcompress is ever seen). No `ENC-NVTT-PARTIAL`: S7b encodes every BC6H level.
- **NV9 Preview.** Previews of an NVTT texture encode with the format's built-in backend; the export uses NVTT. There is
  no `PreviewResult` flag: the GUI reads `profile.encoder.backend` (its struct literals are in `crates/gui`).
- **NV10 Records.** `EncoderVersions.nvtt` is set on NVTT records only; `if-changed` re-encodes an NVTT output when the
  version found now differs or NVTT is missing. Built-in records ignore it, so pre-M5 records stay current. Engine
  version stays 0.7.0.

Real-NVTT checks (`cargo test --release -p engine --test nvtt_real -- --ignored --nocapture --test-threads 1`,
2026-10-13, NVTT 3.2.5):
- (a) `rgb_real/Bricks104`, `alpha.policy = remove`: engine mip 0 RGB PSNR 47.776810 dB = direct `nvtt_production`
  47.776810 dB (Δ 0.000000); decoded alpha 255 on every level.
- (b) `hdr_real/studio_small_08` as a float `_g` (BC6H): validation passed, peak ratio 0.9957.
- (c) Engine run of that BC7 file 18.07 s vs direct per-level `-nocuda` processes 17.66 s (ratio 1.023, limit 1.10).
- (d) `alpha_real/Fingerprints009`, `alpha.policy = blend`: mip 0 alpha PSNR 38.7786 dB on NVTT (`-alpha`), 38.9259 dB on
  bc7e, bench direct `nvtt_production` 38.7883 dB. Not measured: the same file without `-alpha`.

## Note AB — GUI tweaks (owner request, 2026-10-14)

Spec `docs/superpowers/specs/2026-10-14-sdc-gui-tweaks-design.md` (G1-G9).

- **AB1 Input name.** The set editor's input name is the set's base name, changed through `[inputs]` aliases (AA1), not the PGPatcher `rename` (AA3). The set's `sdc-project.toml` entry follows the new automatic id (`SetsDoc::retarget_auto`). Default picked in the thread; the owner may overrule.
- **AB2 Delete sections** are `[[pgp_sections]]` in `sdc-project.toml`, written after every set's entry, always with `delete: true` (they exist to delete trishapes, such as a vanilla glow shape the PBR base replaces with emissive; owner, 2026-10-14), and only with a texture match. Supersedes the "`delete` is a later round" line of `pgp::schema::NOT_EXPOSED` for sections; set entries still never write `delete`.
- **AB3 Delete hits** compare texture paths only (exact slot match, or `path_contains` in the diffuse path); a `nif_filter` is reported, not checked against meshes.
- **AB4 2D | 3D** is one switch kept in `gui.toml`, default 2D; the 3D preview does no work in 2D.
- **AB5 Version** stays 0.7.0 (Z7): no output changes unless the user adds sections or aliases.
