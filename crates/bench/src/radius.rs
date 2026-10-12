//! M6 Track D spec §7 (DECISIONS Note T11): BC4/BC5 search radius against PSNR and encode time on the bench corpus.
//! Inputs (`images2/mask_real` as BC4 on R, `images2/normal_real` as BC5 on R and G) are only read. The CSV goes to
//! `<out>/radius45.csv`, and `out` must not be inside the bench root.
use crate::corpus::{load_png_rgba8, set_files, stem};
use crate::metrics::{channel_psnr, mean};
use anyhow::{bail, Result};
use encoders_sys::{bc4_encode_block, bc5_encode_block, gather_blocks};
use engine::dds::DdsPixelFormat;
use engine::decode::{decode_level, Decoded};
use rayon::prelude::*;
use std::path::{Path, PathBuf};
use std::time::Instant;

/// 5 is the current `default` (`BC45_RADIUS_DEFAULT`), 10 is `max`.
pub const RADII: [u32; 7] = [0, 1, 2, 3, 4, 5, 10];

#[derive(Debug, Clone)]
pub struct Row {
    pub format: &'static str,
    pub image: String,
    pub radius: u32,
    pub psnr: f64,
    pub secs: f64,
}

/// Refuse an output folder inside the bench root (spec §7: nothing is written there). Windows paths are
/// case-insensitive, so both are compared lower-cased, component by component.
pub fn check_out(root: &Path, out: &Path) -> Result<()> {
    let norm = |p: &Path| -> Result<PathBuf> {
        Ok(PathBuf::from(
            std::path::absolute(p)?.to_string_lossy().to_lowercase(),
        ))
    };
    if norm(out)?.starts_with(norm(root)?) {
        bail!(
            "{} is inside the bench root {}; use a scratch folder",
            out.display(),
            root.display()
        );
    }
    Ok(())
}

fn encode(format: &str, blocks: &[[u8; 4]], r: u32) -> Vec<u8> {
    blocks
        .par_chunks(16)
        .flat_map_iter(|b| {
            let b: &[[u8; 4]; 16] = b.try_into().expect("16 texels per block");
            if format == "bc4" {
                bc4_encode_block(b, 0, r).to_vec()
            } else {
                bc5_encode_block(b, r).to_vec()
            }
        })
        .collect()
}

pub fn measure(root: &Path, out: &Path) -> Result<Vec<Row>> {
    check_out(root, out)?;
    let mut rows = vec![];
    for (format, set, pf, ch) in [
        ("bc4", "mask_real", DdsPixelFormat::Bc4, &[0usize][..]),
        ("bc5", "normal_real", DdsPixelFormat::Bc5, &[0usize, 1][..]),
    ] {
        for src in set_files(root, "images2", set, "png")? {
            let img = load_png_rgba8(&src)?;
            let blocks = gather_blocks(&img.px, img.width, img.height);
            for r in RADII {
                let (mut secs, mut data) = (f64::INFINITY, vec![]);
                for _ in 0..3 {
                    let t = Instant::now();
                    data = encode(format, &blocks, r);
                    secs = secs.min(t.elapsed().as_secs_f64());
                }
                let Decoded::Ldr(dec) = decode_level(pf, img.width, img.height, &data, None)?
                else {
                    bail!("{format} did not decode to LDR");
                };
                rows.push(Row {
                    format,
                    image: stem(&src),
                    radius: r,
                    psnr: channel_psnr(&img.px, &dec, ch),
                    secs,
                });
            }
        }
    }
    std::fs::create_dir_all(out)?;
    let mut csv = String::from("format,image,radius,psnr_db,secs\n");
    for r in &rows {
        csv += &format!(
            "{},{},{},{:.4},{:.6}\n",
            r.format, r.image, r.radius, r.psnr, r.secs
        );
    }
    std::fs::write(out.join("radius45.csv"), csv)?;
    Ok(rows)
}

/// Per format and radius: (format, radius, mean PSNR dB, total seconds).
pub fn summary(rows: &[Row]) -> Vec<(&'static str, u32, f64, f64)> {
    let mut v = vec![];
    for format in ["bc4", "bc5"] {
        for r in RADII {
            let sel: Vec<&Row> = rows
                .iter()
                .filter(|x| x.format == format && x.radius == r)
                .collect();
            if !sel.is_empty() {
                let p: Vec<f64> = sel.iter().map(|x| x.psnr).collect();
                v.push((format, r, mean(&p), sel.iter().map(|x| x.secs).sum()));
            }
        }
    }
    v
}

/// DECISIONS Note T11. Q: radius 10 gains >= 0.10 dB over 5 at <= 1.5x the time, on both formats. F: the smallest
/// radius below 5 that loses <= 0.10 dB against 5, on both formats.
pub fn decide(summary: &[(&str, u32, f64, f64)]) -> (bool, Option<u32>) {
    let at = |f: &str, r: u32| {
        summary
            .iter()
            .find(|s| s.0 == f && s.1 == r)
            .map(|s| (s.2, s.3))
    };
    let both = |p: &dyn Fn(&str) -> Option<bool>| ["bc4", "bc5"].iter().all(|f| p(f) == Some(true));
    let q = both(&|f| {
        let ((p5, t5), (p10, t10)) = (at(f, 5)?, at(f, 10)?);
        Some(p10 - p5 >= 0.10 && t10 <= 1.5 * t5)
    });
    let fast = [0, 1, 2, 3, 4].into_iter().find(|&r| {
        both(&|f| {
            let ((p5, _), (pr, _)) = (at(f, 5)?, at(f, r)?);
            Some(p5 - pr <= 0.10)
        })
    });
    (q, fast)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_output_must_be_outside_the_bench_root() {
        let root = Path::new("F:/ai/dds/bench");
        assert!(check_out(root, Path::new("F:/ai/dds/bench/out3/x")).is_err());
        assert!(check_out(root, Path::new("F:/AI/dds/Bench/images3")).is_err());
        assert!(check_out(root, Path::new("F:/ai/dds/bench2/x")).is_ok());
        assert!(check_out(root, Path::new("F:/ai/dds/converter/target/bench/radius45")).is_ok());
    }

    fn both(v: &[(u32, f64, f64)]) -> Vec<(&'static str, u32, f64, f64)> {
        ["bc4", "bc5"]
            .iter()
            .flat_map(|f| v.iter().map(move |&(r, p, t)| (*f, r, p, t)))
            .collect()
    }

    /// DECISIONS Note T11: Q needs >= 0.10 dB at <= 1.5x time on both formats; F is the smallest radius below 5
    /// within 0.10 dB of radius 5 on both formats.
    #[test]
    fn decision_rules() {
        let a = both(&[
            (0, 40.0, 1.0),
            (1, 40.5, 1.0),
            (2, 40.8, 1.0),
            (3, 40.95, 1.0),
            (4, 41.0, 1.0),
            (5, 41.0, 1.0),
            (10, 41.2, 1.4),
        ]);
        assert_eq!(decide(&a), (true, Some(3)));
        let b = both(&[
            (0, 30.0, 1.0),
            (1, 30.0, 1.0),
            (2, 30.0, 1.0),
            (3, 30.0, 1.0),
            (4, 40.0, 1.0),
            (5, 41.0, 1.0),
            (10, 41.2, 2.0),
        ]);
        assert_eq!(decide(&b), (false, None));
        let mut c = a.clone();
        c.retain(|s| !(s.0 == "bc5" && s.1 == 10));
        c.push(("bc5", 10, 41.05, 1.0));
        assert!(!decide(&c).0, "BC5 gains only 0.05 dB");
    }
}
