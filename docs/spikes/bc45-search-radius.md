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
  `fast-preview` use `bc45_search_radius = "fast"`; schemas regenerated. `sdc-base` keeps `"default"` (rule Q holds).
  The GUI dropdown entry (`crates/gui/src/ui/settings_panel.rs`, `opt(json!("fast"), "Fast (preview)")` after
  `Maximum`) is deferred: `crates/gui` was frozen for another task's review when this ran.

## Caveats
- The PSNR is not monotonic in the radius. On BC5, radius 10 is far worse than 5 on Wood094 (71.89 → 53.36 dB),
  Tiles141 (59.37 → 54.78) and Metal049A (99.0 → 51.36), and radii 1–2 are worse than 0 on Metal049A, Tiles141 and
  Wood094. BC4 Tiles141 drops 56.35 → 54.54 at radius 10. Metal049A reaches bit-exact output (the 99 dB clamp) at
  radii 0, 3, 4 and 5, so the gather/encode/decode/compare path is sound; this is rgbcx behaviour on near-flat or
  few-valued blocks. It bears on the shipped `max` value (radius 10), which this corpus shows as worse than `default`.
- The means average PSNR, including 99 dB clamps (Metal049A), so a few near-lossless images dominate the BC5 means.
  The rule F outcome holds per image on BC5 (every image within 0.07 dB or better at radius 4). On BC4 one image,
  Tiles141_Displacement, loses 0.12 dB at radius 4 (56.23 vs 56.35); the others lose ≤ 0.10 dB.
