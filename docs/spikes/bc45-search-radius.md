# BC4/BC5 search radius (M6 Track D spec §7, DECISIONS Note T11)

Date: 2026-10-12. Command: `cargo run -p sdc-bench --release --offline -- radius45 --out target/bench/radius45`.
Corpus (read-only): `F:/ai/dds/bench/images2/mask_real` (BC4, R; 7 images), `images2/normal_real` (BC5, R+G; 7 images).
Encoder: rgbcx `encode_bc4_hq`/`encode_bc5_hq` through `encoders-sys`; decoder: bcdec_rs. Time: best of 3, all threads.

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

## Decision
- Rule Q (quality default becomes `max` if +0.10 dB at ≤ 1.5× time on both formats): does not fire, because radius 10
  loses PSNR against 5 on both formats (bc4 −0.21 dB, bc5 −10.07 dB) and costs 2.77× (bc4) and 3.05× (bc5) the time.
  Either reason alone fails it.
- Rule F (smallest radius < 5 within 0.10 dB of 5 on both formats): 4, because radius 4 is −0.0455 dB (bc4) and
  +0.0944 dB (bc5) against 5, while radius 3 is −0.3189 dB (bc4) and −2.1678 dB (bc5). Radius 4 takes 0.72× the time
  of 5 on both formats.
- Applied: `SearchRadius::Fast` (`"fast"`) = `BC45_RADIUS_FAST` = 4; `cs-compact`, `vanilla-compact` and
  `fast-preview` use `bc45_search_radius = "fast"`; schemas regenerated; the GUI offers "Fast (preview)" after
  "Maximum". `sdc-base` keeps `"default"` (rule Q holds).

## Why PSNR is not monotonic in the radius: the encoder and the decoder round differently
rgbcx's `encode_bc4_hq` (used for BC4, both BC5 halves and BC3 alpha) picks endpoints by the lowest squared error under
its own decode model, `bc4_block::get_block_values` (rgbcx.h): interpolants are floored, `(6·e0 + e1) / 7` and
`(4·e0 + e1) / 5` in integer division. bcdec (the bench and `crates/engine/src/decode.rs`) rounds to nearest:
fixed point `(w·e0 + w'·e1 + 32768) >> 16`, about `round(x)`. So the interpolants differ by 1 whenever the exact
fraction is ≥ .5 — in both the 8-value and the 6-value mode; the endpoints and the 6-value mode's 0/255 agree.
The search sets are nested (radius r ⊂ r+1, ties keep the first), so under rgbcx's own model the error cannot grow
with the radius, and it does not. A wider search finds more endpoint pairs that hit the source exactly only because
of the floor, and those miss by 1 under a rounding decoder.

`cargo run -p sdc-bench --release --offline --example bc45_decoder_probe` (output in
`target/bench/radius45/decoder-probe.txt`), mean PSNR (dB) of the same encodings decoded both ways:

| radius | bc4 bcdec | bc4 rgbcx model | bc5 bcdec | bc5 rgbcx model |
|---|---|---|---|---|
| 0 | 52.1310 | 55.7692 | 53.6305 | 58.9575 |
| 1 | 52.0335 | 56.5367 | 46.4010 | 59.9650 |
| 2 | 52.7307 | 56.8836 | 47.1085 | 60.3017 |
| 3 | 53.8712 | 57.0661 | 55.8233 | 60.4487 |
| 4 | 54.1446 | 57.1679 | 58.0855 | 60.5178 |
| 5 | 54.1901 | 57.2283 | 57.9911 | 60.5528 |
| 10 | 53.9772 | 57.3496 | 47.9202 | 60.6121 |

The bcdec column equals the radius45 run exactly. The rgbcx-model column rises monotonically.

Example, Wood094 normal BC5, block 1, channel R. Source: 127 and 128 only.
- Radius 5: e0 = 128, e1 = 122 (8-value), selectors 0 and 2. Value 2 = 6·128 + 122 = 890 / 7 = 127.14, which is 127
  under both decoders. SSE 0 under both.
- Radius 10: e0 = 130, e1 = 120, selectors 2 and 3. Value 2 = 900 / 7 = 128.57: rgbcx gives 128 and bcdec 129.
  Value 3 = 890 / 7 = 127.14: both give 127. SSE is 0 under rgbcx and 7 under bcdec. rgbcx picks this block because,
  the search keeps the first zero-error pair it reaches, and (130, 120) comes before (128, 122).
- In Wood094 BC5, 77 621 of 131 072 channel-blocks have a higher bcdec SSE at radius 10 than at radius 5.

Which decoder the game uses cannot be settled from code: the GPU decodes. D3D's BC4 definition interpolates in
higher precision (a fraction of 7 or 5), so a rounding decoder (bcdec) is the closer model and a flooring one is not.
The radius table and the decisions above therefore stay on bcdec. Rule F is unchanged at 4 and rule Q still holds.
Under rgbcx's own model the decisions would be the same: radius 4 is −0.0604 dB (bc4) and −0.0350 dB (bc5) against 5
and radius 3 is −0.1622 dB on bc4, so F = 4; radius 10 gains +0.1213 dB (bc4) but only +0.0593 dB (bc5), so Q fails.

Effect on shipped output (not changed here): every rgbcx BC4/BC5 (and BC3 alpha) block is tuned for the floor model.
On this corpus at the shipped `default` (radius 5), the mean PSNR under bcdec is 3.04 dB (BC4) and 2.56 dB (BC5)
below what rgbcx believes it achieved. At `max` (radius 10) the BC5 gap is 12.69 dB. Fixing it means changing
rgbcx's `get_block_values` to round (or adding a rounding search), which is an encoder change for the owner.

## Caveats
- Per image, BC5 radius 10 vs 5 under bcdec: Wood094 71.89 → 53.36 dB, Tiles141 59.37 → 54.78, Metal049A
  99.0 → 51.36; BC4 Tiles141 56.35 → 54.54. All follow from the rounding mismatch above. The shipped `max` value
  (radius 10) is worse than `default` on this corpus.
- The means average PSNR, including 99 dB clamps (Metal049A), so a few near-lossless images dominate the BC5 means.
  The rule F outcome holds per image on BC5 (every image within 0.07 dB or better at radius 4). On BC4 one image,
  Tiles141_Displacement, loses 0.12 dB at radius 4 (56.23 vs 56.35); the others lose ≤ 0.10 dB.
