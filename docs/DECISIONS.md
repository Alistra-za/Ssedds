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
- **Missing channels:** neutral defaults plus a warning naming each defaulted channel (AO white, metallic black, specular alpha white = TruePBR default). **Do not block when roughness is missing if an AO or metallic map is found** — roughness then takes a neutral default with a warning. [Assistant proposal: default roughness 0.5, configurable in the preset; Open until the owner confirms.]
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
`validate` infers the alpha policy from the file: mip-0 alpha ≥ 254 everywhere → `remove`; otherwise the sub-type's alpha-keeping default, or `data`. `--alpha-policy` overrides it.
A DDS in a format sdc never writes (BC2, uncompressed, signed) gets header checks only; a clean header gives "not judged", exit 0.

## Note K — Task 44 CLI edge-case rulings (controller, 2026-10-09)

- **Ctrl-C:** the handler is installed before planning (a failed install prints a warning to stderr). The first Ctrl-C cancels cooperatively (exit 1, cancelled files listed in `batch_done`); a second one exits 3 at once. Outputs are written atomically, so an abort never leaves a partial `.dds`; it can leave `.sdc-*.tmp` files in the output folders and a just-renamed output without a manifest record (reported as `OUT-UNKNOWN-EXISTING` next run; `--on-existing overwrite` replaces it). With `--json`, an aborted run's stream ends without `batch_done`; consumers treat that as "aborted". The handler does no I/O (Codex-CLI 1): the first Ctrl-C sets the cancel flag and wakes a helper thread that prints the "cancelling" note (ignoring write errors), and the second calls `std::process::exit(3)` without writing, so a full or closed stderr pipe can neither block nor kill it. The token reaches planning too: a Ctrl-C while planning or during a dry run lists every pending file (not yet loaded, or ready but not started, including files that would have been up to date or kept) as cancelled, exit 1.
- **`--mem-budget 0`** is a usage error (exit 2), not "default"; omit the flag for 50% of RAM.
- **`encoder.bc1_transparent_black = true` with a BC1 output is `CFG-INVALID`** (Note F: BC1 is opaque-only in v1). The setting stays in the schema for the day BC1 alpha is offered; it is inert for other formats.
- **Test-only hooks** (behind the engine cargo feature `test-hooks`, off by default; the `sdc` feature of the same name forwards to it; compiled out of `cargo build --release -p sdc`, which has no `SDC_TEST_*` strings. Engine and CLI dev-dependencies enable the feature, so plain `cargo test --workspace` runs the tests that use them): `SDC_TEST_PANIC_ON=<input file name>` panics that file; `SDC_TEST_FAIL_VALIDATION=<input file stem>` adds one `VAL-HEADER` error to that file's validation report; `SDC_TEST_CANCEL_ON=<input file name>` cancels the batch while planning, just before that file loads (a Ctrl-C without a console). The second exists because no setting reaches an error-severity `VAL-*` code, so exit 4 would otherwise have no deterministic trigger.

## Note L — near-opaque source alpha (owner, 2026-10-09)

Found in the Task 45 real-folder run: PBR normal maps re-exported from BC7 DDS carry alpha 251–255. That is BC encode noise, not data.
- **Rule:** source alpha whose minimum is **≥ 251** (no texel below 251) is treated as opaque. It resolves like constant-255 alpha, so `remove` applies and no LOSS-ALPHA-DATA is raised. An info code `ALPHA-NEAR-OPAQUE` reports the minimum.
- **Below the cutoff:** one texel below 251 keeps the normal varying-alpha rules.
- **Override:** an explicit `alpha.policy` still wins.
- **Why 251:** it matches Community Shaders' 251 opacity gate (Note A).
