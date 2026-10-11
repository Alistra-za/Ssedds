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
  Lives only in `spikes/s4-scalar/` (own `[workspace]`); not part of the shipped build.
- Quality (veryslow, linear, rgb_real + alpha_real, 11 images, vs ISPC u5 payloads decoded the same way): max per-image |dPSNR| = 0.0759 dB (rgb_real/Metal049A, alpha channel; its rgb delta is 0.015 dB);
  every other image <= 0.0088 dB; bytes identical on 0/11 images (the port differs from the ISPC source, e.g. a denominator bias, so identical bytes are not expected). Deltas are of mixed sign.
  Against the pass limit of 0.01 dB per image: **fails on 1 of 11 images** (Metal049A), passes on the other 10.
- Speed (u5, rgb_real + alpha_real, 16 threads; three runs each): scalar 0.183 / 0.178 / 0.185 Mpix/s (1 thread), 2.073 / 2.114 / 2.101 Mpix/s (16 threads); ISPC u5 0.872 / 0.831 / 0.888 Mpix/s (1 thread), 8.193 / 8.206 / 7.889 Mpix/s (16 threads)
  → scalar is about 4.8x slower on one thread and 3.8–3.95x slower on 16 threads (limit 4x, so inside it only just, and with the machine otherwise idle).
- Verdict: **do not adopt as drop-in**: it misses the per-image quality limit on one image (0.076 dB vs 0.01 dB) and sits at the speed limit. It works and is close (mean deltas ~0.003 dB), so it remains a viable
  fallback if the ISPC objects ever fail on a needed configuration; adopting it would need the owner to accept a looser per-image tolerance. Open item §14 #11 stays "not built".
  Reproduce: `cargo run --release --manifest-path spikes/s4-scalar/Cargo.toml` and `cargo run --release -p sdc-bench -- speed-bc7e`.

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
- **P13 Invalid path characters block export** (plan) rather than being rewritten: a changed path is not where PGPatcher looks.
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
- **R13 Coverage, mipcheck round 2 B1/B6** (owner decision on the dither, 2026-10-11, verbatim: "I would probably encourage a dithering at lower mips, the whole point of coverage is to ensure it isn't lost, especially for foliage and grass, when your likely seeing mostly lower mip versions of them"): every level ≥ 1 passes exactly `k = round(p·n)` texels, the texels above the k-th largest alpha plus as many tied with it as are needed. The dither is in M4 at the owner's request. Ties are split in a hierarchical Bayer order of position (not plain 8×8, which biases the split spatially; deterministic, no RNG), so tied alpha on thin strands neither overshoots (up to ×32 before) nor vanishes (B1). `k = 0` keeps the box alpha ("no texel passes"); zero alpha is never raised, and a level with no positive alpha keeps its box alpha; there is no "more than 2× off" fallback. Remapped texels, the split's included, sit at least 2 codes clear of the threshold (B6). The clearance also applies to levels that skip the remap (early-out levels, including `k = 0`). At the ends of the range (threshold code c ≤ 2 or c ≥ 254) the clearance degrades to whatever codes are available (unreachable: `alpha.threshold` is limited to 5–200).
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
  - **`--out-dir` onto the input is a usage error** (controller ruling, Task 10 review, overriding the plan's Step 3 code): an `--out-dir` that resolves to an input itself (the S6 guard), or two inputs with one file name, exits 2 before anything is written. `PATCHED` is printed only after the write succeeds; a write failure exits 3 and stops the batch.
  - **The GUI Patch writes under `export_root()`** (controller ruling, Task 11 review, overriding the brief's code): the copy goes to the export root (the chosen export folder, else the default `sdc-out`), so the button never silently does nothing; every error reaches the status line and the confirmation always ends.
- **S8 ICC:** detected and ignored; `IN-ICC-IGNORED` (info).
- **S9 `IN-NONFINITE`:** reported at plan time (so `inspect` and `--dry-run` show it), once; the analysis statistics skip non-finite samples.
- **S10 Inputs:** `.exr` (first RGB(A) layer, f32), `.hdr`, `.bmp`, `.jpg`/`.jpeg` through `image` 0.25.10 features; `exr` 1.74.2 and `lebe` are BSD-3-Clause (THIRD_PARTY_NOTICES). Undecodable files are `IN-DECODE`.
- **S11 Version:** workspace 0.3.0: outputs changed (float sources, 8-bit emissives), so `if-changed` re-encodes once.
- **S12 Packed maps cannot use an HDR format** (owner, 2026-10-11, verbatim: "a packed map cannot use bc6h, they are mutually exclusive"): a Mixed-Packed sub-type (`complex_material`, `rmaos`, `skin_rfaos`, `flow`, `generic_two_channel`, `generic_data`) with `bc6h_uf16` is `CFG-INVALID` in `check_profile`. Generalised with S4 to every `is_hdr()` format, so `rgba16f` too: packed outputs are linear 8-bit data (the Stored domain).
