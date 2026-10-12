# BC4/BC5 search radius (M6 Track D spec §7, DECISIONS Note T11)

Date: 2026-10-12. Command: `cargo run -p sdc-bench --release --offline -- radius45 --out target/bench/radius45`.
Corpus (read-only): `F:/ai/dds/bench/images2/mask_real` (BC4, R; 7 images), `images2/normal_real` (BC5, R+G; 7 images).
Encoder: rgbcx `encode_bc4_hq`/`encode_bc5_hq` through `encoders-sys`; decoder: bcdec_rs. Time: best of 3, all threads.

The measurement was run twice. "Before" is the first run, with upstream rgbcx, whose BC4 decode model floors the
interpolants (see "Why the first run was not monotonic" below). "After" is the re-run (Task 10b) with the
sdc rgbcx patch (`crates/encoders-sys/vendor/README-VENDOR.md`): rgbcx's BC4 palette now rounds to nearest exactly
as bcdec does. The after run was taken on an idle machine (no other build running).

## After the rgbcx round-to-nearest patch (current)

| format | radius | mean PSNR (dB) | Δ vs 5 (dB) | time (s) | × time of 5 |
|---|---|---|---|---|---|
| bc4 | 0 | 55.7871 | −1.4700 | 0.0112 | 0.04 |
| bc4 | 1 | 56.5667 | −0.6904 | 0.0424 | 0.13 |
| bc4 | 2 | 56.9168 | −0.3403 | 0.0918 | 0.29 |
| bc4 | 3 | 57.0994 | −0.1577 | 0.1560 | 0.49 |
| bc4 | 4 | 57.1980 | −0.0591 | 0.2309 | 0.73 |
| bc4 | 5 | 57.2571 | 0 | 0.3179 | 1.00 |
| bc4 | 10 | 57.3802 | +0.1231 | 0.8900 | 2.80 |
| bc5 | 0 | 58.9445 | −1.6578 | 0.0180 | 0.03 |
| bc5 | 1 | 60.0063 | −0.5960 | 0.0809 | 0.13 |
| bc5 | 2 | 60.3472 | −0.2551 | 0.1760 | 0.27 |
| bc5 | 3 | 60.4957 | −0.1066 | 0.3012 | 0.47 |
| bc5 | 4 | 60.5661 | −0.0362 | 0.4577 | 0.71 |
| bc5 | 5 | 60.6023 | 0 | 0.6442 | 1.00 |
| bc5 | 10 | 60.6622 | +0.0599 | 1.9071 | 2.96 |

PSNR now rises with the radius, in the means and for every one of the 14 images (no image loses PSNR from one
radius to the next).

## Before and after, mean PSNR under bcdec (dB)

| format | radius | before | after | gain |
|---|---|---|---|---|
| bc4 | 0 | 52.1310 | 55.7871 | +3.6561 |
| bc4 | 1 | 52.0335 | 56.5667 | +4.5332 |
| bc4 | 2 | 52.7307 | 56.9168 | +4.1861 |
| bc4 | 3 | 53.8712 | 57.0994 | +3.2282 |
| bc4 | 4 | 54.1446 | 57.1980 | +3.0534 |
| bc4 | 5 (`default`) | 54.1901 | 57.2571 | +3.0670 |
| bc4 | 10 (`max`) | 53.9772 | 57.3802 | +3.4030 |
| bc5 | 0 | 53.6305 | 58.9445 | +5.3140 |
| bc5 | 1 | 46.4010 | 60.0063 | +13.6053 |
| bc5 | 2 | 47.1085 | 60.3472 | +13.2387 |
| bc5 | 3 | 55.8233 | 60.4957 | +4.6724 |
| bc5 | 4 | 58.0855 | 60.5661 | +2.4806 |
| bc5 | 5 (`default`) | 57.9911 | 60.6023 | +2.6112 |
| bc5 | 10 (`max`) | 47.9202 | 60.6622 | +12.7420 |

Times are unchanged within noise (the patch changes only how candidate palettes are computed).

Before, with upstream rgbcx (first run, superseded):

| format | radius | mean PSNR (dB) | Δ vs 5 (dB) | time (s) | × time of 5 |
|---|---|---|---|---|---|
| bc4 | 0 | 52.1310 | −2.0591 | 0.0118 | 0.03 |
| bc4 | 1 | 52.0335 | −2.1566 | 0.0443 | 0.13 |
| bc4 | 2 | 52.7307 | −1.4594 | 0.0967 | 0.28 |
| bc4 | 3 | 53.8712 | −0.3189 | 0.1646 | 0.47 |
| bc4 | 4 | 54.1446 | −0.0455 | 0.2499 | 0.72 |
| bc4 | 5 | 54.1901 | 0 | 0.3472 | 1.00 |
| bc4 | 10 | 53.9772 | −0.2129 | 0.9632 | 2.77 |
| bc5 | 0 | 53.6305 | −4.3606 | 0.0185 | 0.03 |
| bc5 | 1 | 46.4010 | −11.5901 | 0.0858 | 0.12 |
| bc5 | 2 | 47.1085 | −10.8826 | 0.1867 | 0.27 |
| bc5 | 3 | 55.8233 | −2.1678 | 0.3206 | 0.46 |
| bc5 | 4 | 58.0855 | +0.0944 | 0.5002 | 0.72 |
| bc5 | 5 | 57.9911 | 0 | 0.6939 | 1.00 |
| bc5 | 10 | 47.9202 | −10.0709 | 2.1147 | 3.05 |

Per-image rows: `target/bench/radius45/radius45.csv` (not committed).

## Decision (re-applied to the after numbers)
- Rule Q (quality default becomes `max` if +0.10 dB at ≤ 1.5× time on both formats): does not fire. BC4 gains
  +0.1231 dB at radius 10 but takes 2.80× the time; BC5 gains only +0.0599 dB and takes 2.96× the time. `sdc-base`
  keeps `"default"` and its version is unchanged.
- Rule F (smallest radius < 5 within 0.10 dB of 5 on both formats): 4, as before. Radius 4 is −0.0591 dB (bc4) and
  −0.0362 dB (bc5) against 5, at 0.73× and 0.71× the time; radius 3 is −0.1577 dB (bc4) and −0.1066 dB (bc5).
  `BC45_RADIUS_FAST` stays 4; `cs-compact`, `vanilla-compact` and `fast-preview` keep `bc45_search_radius = "fast"`.
  No preset or schema changes.
- `max` is no longer worse than `default`: it is slightly better on both formats (and on no image worse), at about
  3× the time. A planned `max` warning ("slower, no quality gain") would now be untrue, so it is held for the
  owner's decision (Task 10b report).

## Why the first run was not monotonic: the encoder and the decoder rounded differently
rgbcx's `encode_bc4_hq` (used for BC4, both BC5 halves and BC3 alpha) picks endpoints by the lowest squared error under
its own decode model, `bc4_block::get_block_values` (rgbcx.h). Upstream, the interpolants were floored,
`(6·e0 + e1) / 7` and `(4·e0 + e1) / 5` in integer division. bcdec (the bench and `crates/engine/src/decode.rs`)
rounds to nearest: fixed point `(w·e0 + w'·e1 + 32768) >> 16`. So the interpolants differed by 1 whenever the exact
fraction was ≥ .5. The search sets are nested (radius r ⊂ r+1, ties keep the first), so under rgbcx's own model the
error could not grow with the radius, and it did not; but a wider search found more endpoint pairs that hit the source
exactly only because of the floor, and those missed by 1 under a rounding decoder.

Example (upstream rgbcx), Wood094 normal BC5, block 1, channel R. Source: 127 and 128 only.
- Radius 5: e0 = 128, e1 = 122 (8-value), selectors 0 and 2. Value 2 = 890 / 7 = 127.14: 127 under both decoders.
- Radius 10: e0 = 130, e1 = 120, selectors 2 and 3. Value 2 = 900 / 7 = 128.57: rgbcx gave 128 and bcdec 129.
  SSE 0 under rgbcx and 7 under bcdec. In Wood094 BC5, 77 621 of 131 072 channel-blocks had a higher bcdec SSE at
  radius 10 than at radius 5.

D3D's BC4 definition interpolates in higher precision (a fraction of 7 or 5), so a rounding decoder (bcdec) is the
closer model of the GPU, and the owner chose to patch rgbcx to match it (2026-10-12, decision A).

`cargo run -p sdc-bench --release --offline --example bc45_decoder_probe` (output in
`target/bench/radius45/decoder-probe.txt`) decodes the same encodings with bcdec and with rgbcx's own model (read
through the shim, so it is the compiled C++ model). Mean PSNR (dB):

| radius | bc4 bcdec before | bc4 model before | bc4 both after | bc5 bcdec before | bc5 model before | bc5 both after |
|---|---|---|---|---|---|---|
| 0 | 52.1310 | 55.7692 | 55.7871 | 53.6305 | 58.9575 | 58.9445 |
| 1 | 52.0335 | 56.5367 | 56.5667 | 46.4010 | 59.9650 | 60.0063 |
| 2 | 52.7307 | 56.8836 | 56.9168 | 47.1085 | 60.3017 | 60.3472 |
| 3 | 53.8712 | 57.0661 | 57.0994 | 55.8233 | 60.4487 | 60.4957 |
| 4 | 54.1446 | 57.1679 | 57.1980 | 58.0855 | 60.5178 | 60.5661 |
| 5 | 54.1901 | 57.2283 | 57.2571 | 57.9911 | 60.5528 | 60.6023 |
| 10 | 53.9772 | 57.3496 | 57.3802 | 47.9202 | 60.6121 | 60.6622 |

After the patch the bcdec and rgbcx-model columns are identical (shown once) and equal the radius45 run; 0 of
131 072 Wood094 BC5 channel-blocks have a higher bcdec SSE at radius 10 than at 5. The encoders-sys test
`bc4_search_model_matches_bcdec_for_every_endpoint_pair` checks the model against bcdec for all 65 536 endpoint pairs.

Effect on shipped output: at the shipped `default` (radius 5), BC4 gains 3.07 dB and BC5 2.61 dB under bcdec on this
corpus; `max` gains 3.40 dB (BC4) and 12.74 dB (BC5). The bench gate `sdc-bench gate rgbcx` now checks that no BC4
block (BC3 alpha, BC4, BC5) decodes worse under bcdec than bc7enc_rdo's own output, on all 48 bench images; none does.

## Caveats
- The means average PSNR, including 99 dB clamps (Metal049A), so a few near-lossless images weigh on the means.
  Per image at radius 4 against 5: BC5 every image within 0.075 dB; BC4 two images lose more than 0.10 dB
  (Leaf001_Opacity −0.109, Tiles141_Displacement −0.160), the others ≤ 0.059 dB.
- BC3 alpha uses the same rgbcx search, so it is now tuned for a rounding decoder too. bcdec_rs's own BC3 path
  (`bc3` → `smooth_alpha_block`) uses yet another formula, `(6·a0 + a1 + 1) / 7`, which matches neither, so sdc's
  BC3 validation PSNR (engine decode through `bcdec_rs::bc3`) does not model the GPU exactly.
