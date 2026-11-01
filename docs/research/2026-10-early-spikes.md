# Early spikes and phase records (2026-10)

Moved verbatim from DECISIONS.md on 2026-10-31 (docs/DOCUMENTATION.md §7): measurement and process records of 2026-10-08 to 2026-10-13, not rulings.

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
