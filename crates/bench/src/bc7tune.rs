//! BC7 tuning study (docs/research/bc7e-tuning.md): bc7e variants and the reference encoder over a corpus
//! laid out as `<root>/<class>/*.png` (classes `diffuse`, `normal`, `cutout`, `smooth`), scored per image.

use crate::corpus::{load_png_rgba8, Rgba8};
use anyhow::{bail, Context, Result};
use encoders_sys::{bc7e_compress_blocks_tuned, gather_blocks, Bc7Level, Bc7Params, Bc7Tune};
use rayon::prelude::*;
use std::io::Write;
use std::path::Path;
use std::time::Instant;

#[derive(Clone, Copy, Debug)]
pub struct Variant {
    pub name: &'static str,
    pub level: Bc7Level,
    pub tune: Bc7Tune,
    /// bc7e patch 0001's exact-opaque flag (mode-6 p-bits pinned to 1 on all-255 blocks).
    pub exact_opaque: bool,
}

const V: Bc7Level = Bc7Level::Veryslow;
const T: Bc7Tune = Bc7Tune {
    partitions_to_try: None,
    uber_level: None,
    refinement_passes: None,
    pbit_search: None,
    weights: None,
    max_partitions_2subset: None,
    max_partitions_3subset: None,
    endpoint_search: None,
    refine_partitions: None,
};

/// Every bc7e variant the study measures. `base` is Tessalith's default (veryslow, linear weights).
pub const VARIANTS: &[Variant] = &[
    Variant {
        name: "base",
        exact_opaque: false,
        level: V,
        tune: T,
    },
    Variant {
        name: "slowest",
        exact_opaque: false,
        level: Bc7Level::Slowest,
        tune: T,
    },
    Variant {
        name: "parts4",
        exact_opaque: false,
        level: V,
        tune: Bc7Tune {
            partitions_to_try: Some(4),
            ..T
        },
    },
    Variant {
        name: "parts8",
        exact_opaque: false,
        level: V,
        tune: Bc7Tune {
            partitions_to_try: Some(8),
            ..T
        },
    },
    Variant {
        name: "parts16",
        exact_opaque: false,
        level: V,
        tune: Bc7Tune {
            partitions_to_try: Some(16),
            ..T
        },
    },
    Variant {
        name: "parts64",
        exact_opaque: false,
        level: V,
        tune: Bc7Tune {
            partitions_to_try: Some(64),
            ..T
        },
    },
    Variant {
        name: "uber3",
        exact_opaque: false,
        level: V,
        tune: Bc7Tune {
            uber_level: Some(3),
            ..T
        },
    },
    Variant {
        name: "uber4",
        exact_opaque: false,
        level: V,
        tune: Bc7Tune {
            uber_level: Some(4),
            ..T
        },
    },
    Variant {
        name: "refine2",
        exact_opaque: false,
        level: V,
        tune: Bc7Tune {
            refinement_passes: Some(2),
            ..T
        },
    },
    Variant {
        name: "refine4",
        exact_opaque: false,
        level: V,
        tune: Bc7Tune {
            refinement_passes: Some(4),
            ..T
        },
    },
    Variant {
        name: "nopbit",
        exact_opaque: false,
        level: V,
        tune: Bc7Tune {
            pbit_search: Some(false),
            ..T
        },
    },
    Variant {
        name: "parts4_uber4",
        exact_opaque: false,
        level: V,
        tune: Bc7Tune {
            partitions_to_try: Some(4),
            uber_level: Some(4),
            ..T
        },
    },
    Variant {
        name: "parts64_uber4",
        exact_opaque: false,
        level: V,
        tune: Bc7Tune {
            partitions_to_try: Some(64),
            uber_level: Some(4),
            ..T
        },
    },
    Variant {
        name: "parts8_uber4",
        exact_opaque: false,
        level: V,
        tune: Bc7Tune {
            partitions_to_try: Some(8),
            uber_level: Some(4),
            ..T
        },
    },
    Variant {
        name: "parts8_uber4_refine2",
        exact_opaque: false,
        level: V,
        tune: Bc7Tune {
            partitions_to_try: Some(8),
            uber_level: Some(4),
            refinement_passes: Some(2),
            ..T
        },
    },
    Variant {
        name: "parts16_uber4_refine2",
        exact_opaque: false,
        level: V,
        tune: Bc7Tune {
            partitions_to_try: Some(16),
            uber_level: Some(4),
            refinement_passes: Some(2),
            ..T
        },
    },
    // Patch 0002 (need the patch-0002 objects): the quantised endpoint search, full refinement while ranking
    // partitions, their combinations, and the sdc_max level itself.
    Variant {
        name: "ep",
        exact_opaque: false,
        level: V,
        tune: Bc7Tune {
            endpoint_search: Some(true),
            ..T
        },
    },
    Variant {
        name: "rp",
        exact_opaque: false,
        level: V,
        tune: Bc7Tune {
            refine_partitions: Some(true),
            ..T
        },
    },
    Variant {
        name: "rp_parts4",
        exact_opaque: false,
        level: V,
        tune: Bc7Tune {
            refine_partitions: Some(true),
            partitions_to_try: Some(4),
            ..T
        },
    },
    Variant {
        name: "rp_parts8",
        exact_opaque: false,
        level: V,
        tune: Bc7Tune {
            refine_partitions: Some(true),
            partitions_to_try: Some(8),
            ..T
        },
    },
    Variant {
        name: "ep_uber4",
        exact_opaque: false,
        level: V,
        tune: Bc7Tune {
            endpoint_search: Some(true),
            uber_level: Some(4),
            ..T
        },
    },
    Variant {
        name: "ep_rp_parts4_uber4_refine2",
        exact_opaque: false,
        level: V,
        tune: Bc7Tune {
            endpoint_search: Some(true),
            refine_partitions: Some(true),
            partitions_to_try: Some(4),
            uber_level: Some(4),
            refinement_passes: Some(2),
            ..T
        },
    },
    Variant {
        name: "ep_rp_parts8_uber4_refine2",
        exact_opaque: false,
        level: V,
        tune: Bc7Tune {
            endpoint_search: Some(true),
            refine_partitions: Some(true),
            partitions_to_try: Some(8),
            uber_level: Some(4),
            refinement_passes: Some(2),
            ..T
        },
    },
    Variant {
        name: "max",
        exact_opaque: false,
        level: Bc7Level::Max,
        tune: T,
    },
    // Normal-map weighting: more weight on X and Y than Z (the stored Z carries little information in a
    // unit normal), and the plain bc7e perceptual weights for comparison on colour.
    Variant {
        name: "wxy2",
        exact_opaque: false,
        level: V,
        tune: Bc7Tune {
            weights: Some([2, 2, 1, 1]),
            ..T
        },
    },
    Variant {
        name: "wxy4",
        exact_opaque: false,
        level: V,
        tune: Bc7Tune {
            weights: Some([4, 4, 1, 1]),
            ..T
        },
    },
    // Alpha weighting (the "Alpha weighting" section of docs/research/bc7e-tuning.md): the alpha error
    // weight raised to 2, 4 and 8 against RGB at 1, and the exact-opaque flag for comparison.
    Variant {
        name: "alpha_w2",
        exact_opaque: false,
        level: V,
        tune: Bc7Tune {
            weights: Some([1, 1, 1, 2]),
            ..T
        },
    },
    Variant {
        name: "alpha_w4",
        exact_opaque: false,
        level: V,
        tune: Bc7Tune {
            weights: Some([1, 1, 1, 4]),
            ..T
        },
    },
    Variant {
        name: "alpha_w8",
        exact_opaque: false,
        level: V,
        tune: Bc7Tune {
            weights: Some([1, 1, 1, 8]),
            ..T
        },
    },
    Variant {
        name: "exact_opaque",
        exact_opaque: true,
        level: V,
        tune: T,
    },
    // Tessalith's `_m` runs with exact-opaque on, so the weight is also measured on top of it.
    Variant {
        name: "alpha_w2_exact",
        exact_opaque: true,
        level: V,
        tune: Bc7Tune {
            weights: Some([1, 1, 1, 2]),
            ..T
        },
    },
    Variant {
        name: "alpha_w4_exact",
        exact_opaque: true,
        level: V,
        tune: Bc7Tune {
            weights: Some([1, 1, 1, 4]),
            ..T
        },
    },
];

pub fn variant(name: &str) -> Option<Variant> {
    VARIANTS.iter().copied().find(|v| v.name == name)
}

pub const CLASSES: [&str; 7] = [
    "diffuse",
    "normal",
    "cutout",
    "smooth",
    "n_spec",
    "n_spec_mip2",
    "m_height",
];

fn psnr(mse: f64) -> f64 {
    engine::validate::psnr_from_mse(mse)
}

/// Per-image scores of one decoded image against its source.
#[derive(Debug, Clone, Copy, Default)]
pub struct Scores {
    pub rgb: f64,
    pub luma: f64,
    pub alpha: f64,
    pub premul: f64,
    /// Mean angular error in degrees between the stored (x, y, z) vectors, both normalised.
    pub angle: f64,
    /// Largest absolute alpha error over the image.
    pub alpha_max: f64,
    /// Share of texels with source alpha 0 that decode to exactly 0 (NaN when there are none).
    pub exact0: f64,
    /// Share of texels with source alpha 255 that decode to exactly 255 (NaN when there are none).
    pub exact255: f64,
    /// SSIM of the alpha channel: Gaussian window sigma 1.5 (radius 5), K1 0.01, K2 0.03, L 255, valid region.
    pub assim: f64,
    /// Share of texels whose alpha-test result at 128 differs between source and decoded (cutout edges).
    pub cov128: f64,
}

pub fn score(src: &[[u8; 4]], dec: &[[u8; 4]], width: u32) -> Scores {
    assert_eq!(src.len(), dec.len());
    let n = src.len() as f64;
    let (mut se_rgb, mut se_y, mut se_a, mut se_pm, mut ang) = (0.0, 0.0, 0.0, 0.0, 0.0);
    let (mut amax, mut z0, mut z0_ok, mut f255, mut f255_ok, mut flips) =
        (0.0f64, 0usize, 0usize, 0usize, 0usize, 0usize);
    for (s, d) in src.iter().zip(dec) {
        let da = (s[3] as f64 - d[3] as f64).abs();
        amax = amax.max(da);
        if s[3] == 0 {
            z0 += 1;
            z0_ok += (d[3] == 0) as usize;
        }
        if s[3] == 255 {
            f255 += 1;
            f255_ok += (d[3] == 255) as usize;
        }
        flips += ((s[3] >= 128) != (d[3] >= 128)) as usize;
        let mut rgb = 0.0;
        for k in 0..3 {
            let t = s[k] as f64 - d[k] as f64;
            rgb += t * t;
        }
        se_rgb += rgb;
        let y = |c: &[u8; 4]| 0.2126 * c[0] as f64 + 0.7152 * c[1] as f64 + 0.0722 * c[2] as f64;
        let ty = y(s) - y(d);
        se_y += ty * ty;
        let ta = s[3] as f64 - d[3] as f64;
        se_a += ta * ta;
        for k in 0..3 {
            let t = s[k] as f64 * s[3] as f64 / 255.0 - d[k] as f64 * d[3] as f64 / 255.0;
            se_pm += t * t;
        }
        let v = |c: &[u8; 4]| {
            let x = c[0] as f64 / 255.0 * 2.0 - 1.0;
            let y = c[1] as f64 / 255.0 * 2.0 - 1.0;
            let z = c[2] as f64 / 255.0 * 2.0 - 1.0;
            let l = (x * x + y * y + z * z).sqrt().max(1e-9);
            [x / l, y / l, z / l]
        };
        let (a, b) = (v(s), v(d));
        let dot = (a[0] * b[0] + a[1] * b[1] + a[2] * b[2]).clamp(-1.0, 1.0);
        ang += dot.acos().to_degrees();
    }
    let share = |ok: usize, all: usize| {
        if all == 0 {
            f64::NAN
        } else {
            ok as f64 / all as f64
        }
    };
    Scores {
        rgb: psnr(se_rgb / (3.0 * n)),
        luma: psnr(se_y / n),
        alpha: psnr(se_a / n),
        premul: psnr(se_pm / (3.0 * n)),
        angle: ang / n,
        alpha_max: amax,
        exact0: share(z0_ok, z0),
        exact255: share(f255_ok, f255),
        assim: alpha_ssim(src, dec, width),
        cov128: flips as f64 / n,
    }
}

/// SSIM of the alpha channel (Wang et al. 2004): Gaussian window sigma 1.5 truncated at radius 5, K1 0.01,
/// K2 0.03, dynamic range 255, averaged over the region where the window fits (5 texels in from each edge).
pub fn alpha_ssim(src: &[[u8; 4]], dec: &[[u8; 4]], width: u32) -> f64 {
    let w = width as usize;
    let h = src.len() / w;
    const R: usize = 5;
    let g: Vec<f64> = {
        let raw: Vec<f64> = (0..=2 * R)
            .map(|i| (-((i as f64 - R as f64).powi(2)) / (2.0 * 1.5 * 1.5)).exp())
            .collect();
        let sum: f64 = raw.iter().sum();
        raw.iter().map(|v| v / sum).collect()
    };
    if w < 2 * R + 1 || h < 2 * R + 1 {
        return f64::NAN;
    }
    let a: Vec<f64> = src.iter().map(|c| c[3] as f64).collect();
    let b: Vec<f64> = dec.iter().map(|c| c[3] as f64).collect();
    // Separable Gaussian blur of x, y, x*x, y*y, x*y, over the valid region.
    let blur = |img: &[f64]| -> Vec<f64> {
        let mut tmp = vec![0.0; w * h];
        for y in 0..h {
            for x in R..w - R {
                let mut s = 0.0;
                for (k, gk) in g.iter().enumerate() {
                    s += gk * img[y * w + x + k - R];
                }
                tmp[y * w + x] = s;
            }
        }
        let mut out = vec![0.0; w * h];
        for y in R..h - R {
            for x in R..w - R {
                let mut s = 0.0;
                for (k, gk) in g.iter().enumerate() {
                    s += gk * tmp[(y + k - R) * w + x];
                }
                out[y * w + x] = s;
            }
        }
        out
    };
    let mu_a = blur(&a);
    let mu_b = blur(&b);
    let aa = blur(&a.iter().map(|v| v * v).collect::<Vec<_>>());
    let bb = blur(&b.iter().map(|v| v * v).collect::<Vec<_>>());
    let ab = blur(&a.iter().zip(&b).map(|(p, q)| p * q).collect::<Vec<_>>());
    let (c1, c2) = ((0.01f64 * 255.0).powi(2), (0.03f64 * 255.0).powi(2));
    let mut total = 0.0;
    let mut count = 0usize;
    for y in R..h - R {
        for x in R..w - R {
            let i = y * w + x;
            let (ma, mb) = (mu_a[i], mu_b[i]);
            let va = aa[i] - ma * ma;
            let vb = bb[i] - mb * mb;
            let cov = ab[i] - ma * mb;
            total += ((2.0 * ma * mb + c1) * (2.0 * cov + c2))
                / ((ma * ma + mb * mb + c1) * (va + vb + c2));
            count += 1;
        }
    }
    total / count as f64
}

fn encode_variant(img: &Rgba8, v: &Variant) -> Vec<u8> {
    let blocks = gather_blocks(&img.px, img.width, img.height);
    let row = img.width.div_ceil(4) as usize * 16;
    let mut out = vec![0u8; blocks.len()];
    let p = Bc7Params {
        level: v.level,
        perceptual: false,
        exact_opaque: v.exact_opaque,
    };
    out.par_chunks_mut(row)
        .zip(blocks.par_chunks(row))
        .for_each(|(o, b)| {
            for (oc, bc) in o.chunks_mut(64 * 16).zip(b.chunks(64 * 16)) {
                bc7e_compress_blocks_tuned(bc, oc, p, v.tune);
            }
        });
    out
}

/// Encode every image of every class with every named variant (and the reference when `ref_top > 0`),
/// appending CSV rows: variant,class,image,width,height,rgb,luma,alpha,premul,angle,secs.
pub fn run(
    root: &Path,
    out: &Path,
    variants: &[String],
    max_images: Option<usize>,
    ref_top: usize,
    repeats: usize,
) -> Result<()> {
    let chosen: Vec<Variant> = if variants.is_empty() {
        VARIANTS.to_vec()
    } else {
        variants
            .iter()
            .map(|n| variant(n).with_context(|| format!("unknown variant {n}")))
            .collect::<Result<_>>()?
    };
    let fresh = !out.exists();
    let mut f = std::fs::OpenOptions::new()
        .append(true)
        .create(true)
        .open(out)?;
    if fresh {
        writeln!(
            f,
            "variant,class,image,width,height,rgb,luma,alpha,premul,angle,alpha_max,exact0,exact255,assim,cov128,secs"
        )?;
    }
    for class in CLASSES {
        let dir = root.join(class);
        if !dir.is_dir() {
            continue;
        }
        let mut files: Vec<_> = std::fs::read_dir(&dir)?
            .filter_map(|e| e.ok().map(|e| e.path()))
            .filter(|p| p.extension().is_some_and(|e| e.eq_ignore_ascii_case("png")))
            .collect();
        files.sort();
        if let Some(n) = max_images {
            files.truncate(n);
        }
        for path in files {
            let img = load_png_rgba8(&path)?;
            if img.width % 4 != 0 || img.height % 4 != 0 {
                bail!("{}: dimensions must be multiples of 4", path.display());
            }
            let name = path.file_stem().unwrap().to_string_lossy().into_owned();
            let mut base_payload: Option<Vec<u8>> = None;
            for v in &chosen {
                let mut best = f64::MAX;
                let mut enc = vec![];
                for _ in 0..repeats.max(1) {
                    let t = Instant::now();
                    enc = encode_variant(&img, v);
                    best = best.min(t.elapsed().as_secs_f64());
                }
                let dec = crate::codec::decode_bc7_image(&enc, img.width, img.height);
                let s = score(&img.px, &dec, img.width);
                writeln!(
                    f,
                    "{},{class},{name},{},{},{:.4},{:.4},{:.4},{:.4},{:.5},{},{:.5},{:.5},{:.5},{:.6},{:.4}",
                    v.name, img.width, img.height, s.rgb, s.luma, s.alpha, s.premul, s.angle, s.alpha_max, s.exact0, s.exact255, s.assim, s.cov128, best
                )?;
                eprintln!(
                    "{class}/{name} {}: rgb {:.3} luma {:.3} a {:.3} pm {:.3} ang {:.4} amax {} e0 {:.3} e255 {:.3} assim {:.4} cov {:.5} {:.2}s",
                    v.name, s.rgb, s.luma, s.alpha, s.premul, s.angle, s.alpha_max, s.exact0, s.exact255, s.assim, s.cov128, best
                );
                if v.name == "base" {
                    base_payload = Some(enc.clone());
                }
            }
            if ref_top > 0 {
                let t = Instant::now();
                let enc = crate::bc7ref::encode_image(&img.px, img.width, img.height, ref_top);
                let secs = t.elapsed().as_secs_f64();
                let dec = crate::codec::decode_bc7_image(&enc, img.width, img.height);
                let s = score(&img.px, &dec, img.width);
                writeln!(
                    f,
                    "ref{ref_top},{class},{name},{},{},{:.4},{:.4},{:.4},{:.4},{:.5},{},{:.5},{:.5},{:.5},{:.6},{:.4}",
                    img.width, img.height, s.rgb, s.luma, s.alpha, s.premul, s.angle, s.alpha_max, s.exact0, s.exact255, s.assim, s.cov128, secs
                )?;
                eprintln!("{class}/{name} ref{ref_top}: rgb {:.3} luma {:.3} a {:.3} pm {:.3} ang {:.4} {:.1}s", s.rgb, s.luma, s.alpha, s.premul, s.angle, secs);
                if let Some(base) = &base_payload {
                    let a = attribute(&img, base, &enc);
                    let side = out.with_extension("attrib.csv");
                    let fresh = !side.exists();
                    let mut g = std::fs::OpenOptions::new()
                        .append(true)
                        .create(true)
                        .open(&side)?;
                    if fresh {
                        writeln!(g, "class,image,blocks,ref_better,ref_worse,same_structure_better,gain_same,gain_structure,base_modes,ref_modes")?;
                    }
                    let modes = |m: &[usize; 8]| {
                        m.iter()
                            .map(|c| c.to_string())
                            .collect::<Vec<_>>()
                            .join(" ")
                    };
                    writeln!(
                        g,
                        "{class},{name},{},{},{},{},{:.0},{:.0},{},{}",
                        a.blocks,
                        a.ref_better,
                        a.ref_worse,
                        a.same_better,
                        a.gain_same,
                        a.gain_structure,
                        modes(&a.base_modes),
                        modes(&a.ref_modes)
                    )?;
                    eprintln!(
                        "  attribution: {} blocks, ref better on {} ({} with the same mode+partition), worse on {}; gain from endpoints {:.0}, from structure {:.0}; modes base {:?} ref {:?}",
                        a.blocks, a.ref_better, a.same_better, a.ref_worse, a.gain_same, a.gain_structure, a.base_modes, a.ref_modes
                    );
                    let trans = out.with_extension("trans.csv");
                    let fresh = !trans.exists();
                    let mut h = std::fs::OpenOptions::new()
                        .append(true)
                        .create(true)
                        .open(&trans)?;
                    if fresh {
                        writeln!(h, "class,image,base_mode,ref_mode,blocks,gain")?;
                    }
                    for (bm, row) in a.transitions.iter().enumerate() {
                        for (rm, (n, gain)) in row.iter().enumerate() {
                            if *n > 0 {
                                writeln!(h, "{class},{name},{bm},{rm},{n},{gain:.0}")?;
                            }
                        }
                    }
                }
            }
        }
    }
    Ok(())
}

/// Where the reference's gain over `base` comes from: blocks where it keeps bc7e's mode and partition
/// but finds better endpoints, against blocks where it picks a different structure.
#[derive(Debug, Default)]
pub struct Attribution {
    pub blocks: usize,
    pub ref_better: usize,
    pub ref_worse: usize,
    pub same_better: usize,
    /// Summed squared-error reduction on blocks with the same mode and partition.
    pub gain_same: f64,
    /// Summed squared-error reduction on blocks where the structure differs.
    pub gain_structure: f64,
    pub base_modes: [usize; 8],
    pub ref_modes: [usize; 8],
    /// `[base mode][reference mode]` → (blocks, summed squared-error reduction) where the reference won.
    pub transitions: [[(usize, f64); 8]; 8],
}

/// (mode, partition, rotation) read from a block's leading bits.
pub fn block_structure(b: &[u8]) -> (u8, u8, u8) {
    let mode = b[0].trailing_zeros().min(7) as u8;
    let bits = u32::from_le_bytes([b[0], b[1], b[2], b[3]]);
    let part_bits = [4, 6, 6, 6, 0, 0, 0, 6][mode as usize];
    let partition = if part_bits > 0 {
        ((bits >> (mode + 1)) & ((1 << part_bits) - 1)) as u8
    } else {
        0
    };
    let rotation = if mode == 4 || mode == 5 {
        ((bits >> (mode + 1)) & 3) as u8
    } else {
        0
    };
    (mode, partition, rotation)
}

/// Squared error of one decoded block against its source texels (alpha counted only on alpha blocks).
fn block_sse(block: &[u8], px: &[[u8; 4]]) -> f64 {
    let mut o = [0u8; 64];
    bcdec_rs::bc7(block, &mut o, 16);
    let has_alpha = px.iter().any(|c| c[3] < 255);
    let mut e = 0.0;
    for (i, c) in px.iter().enumerate() {
        for k in 0..3 {
            let t = c[k] as f64 - o[i * 4 + k] as f64;
            e += t * t;
        }
        if has_alpha {
            let t = c[3] as f64 - o[i * 4 + 3] as f64;
            e += t * t;
        }
    }
    e
}

fn attribute(img: &Rgba8, base: &[u8], reference: &[u8]) -> Attribution {
    let src = gather_blocks(&img.px, img.width, img.height);
    let mut a = Attribution {
        blocks: base.len() / 16,
        ..Default::default()
    };
    for i in 0..a.blocks {
        let (bb, rb, px) = (
            &base[i * 16..i * 16 + 16],
            &reference[i * 16..i * 16 + 16],
            &src[i * 16..i * 16 + 16],
        );
        let (eb, er) = (block_sse(bb, px), block_sse(rb, px));
        let sb = block_structure(bb);
        let sr = block_structure(rb);
        a.base_modes[sb.0 as usize] += 1;
        a.ref_modes[sr.0 as usize] += 1;
        if er < eb {
            a.ref_better += 1;
            let t = &mut a.transitions[sb.0 as usize][sr.0 as usize];
            t.0 += 1;
            t.1 += eb - er;
            if sb == sr {
                a.same_better += 1;
                a.gain_same += eb - er;
            } else {
                a.gain_structure += eb - er;
            }
        } else if er > eb {
            a.ref_worse += 1;
        }
    }
    a
}
