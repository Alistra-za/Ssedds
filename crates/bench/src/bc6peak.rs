//! BC6H peak-preserving mode study (Note FE): veryslow against `Bc6hLevel::VeryslowPeak` over the 18 real HDR
//! images (images2 + images3 `hdr_real`) and the 4 `hdr_grad` images. Reference = the `.f16` source as float.
use crate::codec::{decode_bc6h_image, encode_bc6h_image, half_to_rgb_f32};
use crate::corpus::{load_f16, set_files, stem};
use crate::metrics::{mean, mpsnr, rmsle};
use crate::s7b::peak;
use anyhow::Result;
use encoders_sys::Bc6hLevel;
use std::fmt::Write as _;
use std::path::Path;
use std::time::Instant;

pub struct Row {
    pub set: String,
    pub image: String,
    pub variant: &'static str,
    pub peak_ratio: f64,
    pub mpsnr: f64,
    pub rmsle: f64,
    pub seconds: f64,
}

const VARIANTS: [(&str, Bc6hLevel); 2] = [
    ("veryslow", Bc6hLevel::Veryslow),
    ("veryslow_peak", Bc6hLevel::VeryslowPeak),
];

/// Encodes every image with both variants and writes `out` (CSV, one row per image and variant).
pub fn run(root: &Path, out: &Path) -> Result<Vec<Row>> {
    let mut rows = vec![];
    for (dir, set) in [
        ("images2", "hdr_real"),
        ("images3", "hdr_real"),
        ("images2", "hdr_grad"),
    ] {
        for f in set_files(root, dir, set, "f16")? {
            let img = load_f16(&f)?;
            let reference = half_to_rgb_f32(&img);
            for (variant, level) in VARIANTS {
                let t = Instant::now();
                let enc = encode_bc6h_image(&img, level);
                let seconds = t.elapsed().as_secs_f64();
                let dec = decode_bc6h_image(&enc, img.width, img.height);
                rows.push(Row {
                    set: set.into(),
                    image: stem(&f),
                    variant,
                    peak_ratio: (peak(&dec) / peak(&reference).max(1e-9)) as f64,
                    mpsnr: mpsnr(&reference, &dec),
                    rmsle: rmsle(&reference, &dec),
                    seconds,
                });
            }
        }
    }
    let mut csv = String::from("set,image,variant,peak_ratio,mpsnr,rmsle,seconds\n");
    for r in &rows {
        writeln!(
            csv,
            "{},{},{},{:.6},{:.4},{:.6},{:.3}",
            r.set, r.image, r.variant, r.peak_ratio, r.mpsnr, r.rmsle, r.seconds
        )?;
    }
    std::fs::write(out, csv)?;
    Ok(rows)
}

/// Two-sided 95% Student-t critical value for `df` degrees of freedom (1..=30; normal beyond).
fn t975(df: usize) -> f64 {
    const T: [f64; 30] = [
        12.706, 4.303, 3.182, 2.776, 2.571, 2.447, 2.365, 2.306, 2.262, 2.228, 2.201, 2.179, 2.160,
        2.145, 2.131, 2.120, 2.110, 2.101, 2.093, 2.086, 2.080, 2.074, 2.069, 2.064, 2.060, 2.056,
        2.052, 2.048, 2.045, 2.042,
    ];
    T.get(df.wrapping_sub(1)).copied().unwrap_or(1.960)
}

/// Mean of `d` and its 95% Student-t half-width.
pub fn mean_ci(d: &[f64]) -> (f64, f64) {
    let m = mean(d);
    let n = d.len();
    if n < 2 {
        return (m, f64::NAN);
    }
    let var = d.iter().map(|x| (x - m).powi(2)).sum::<f64>() / (n - 1) as f64;
    (m, t975(n - 1) * (var / n as f64).sqrt())
}

/// Markdown table of the real images (one row each, both variants) plus the mean deltas.
pub fn summary(rows: &[Row]) -> String {
    let mut s = String::from(
        "| set | image | peak vs | peak new | mPSNR vs | mPSNR new | Δ mPSNR | RMSLE vs | RMSLE new | s vs | s new |\n|---|---|---|---|---|---|---|---|---|---|---|\n",
    );
    let (mut dp, mut dm, mut dr) = (vec![], vec![], vec![]);
    for pair in rows.chunks(2) {
        let (a, b) = (&pair[0], &pair[1]);
        writeln!(
            s,
            "| {} | {} | {:.3} | {:.3} | {:.2} | {:.2} | {:+.3} | {:.5} | {:.5} | {:.2} | {:.2} |",
            a.set,
            a.image,
            a.peak_ratio,
            b.peak_ratio,
            a.mpsnr,
            b.mpsnr,
            b.mpsnr - a.mpsnr,
            a.rmsle,
            b.rmsle,
            a.seconds,
            b.seconds
        )
        .unwrap();
        if a.set == "hdr_real" {
            dp.push(b.peak_ratio - a.peak_ratio);
            dm.push(b.mpsnr - a.mpsnr);
            dr.push(b.rmsle - a.rmsle);
        }
    }
    for (name, d) in [("peak ratio", &dp), ("mPSNR (dB)", &dm), ("RMSLE", &dr)] {
        let (m, h) = mean_ci(d);
        writeln!(
            s,
            "\nMean Δ {name} over {} real images (new - veryslow): {m:+.5} ± {h:.5} (95% t)",
            d.len()
        )
        .unwrap();
    }
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mean_ci_matches_a_hand_computed_interval() {
        // mean 2, sample sd 1, n 3: 4.303 * 1 / sqrt(3)
        let (m, h) = mean_ci(&[1.0, 2.0, 3.0]);
        assert_eq!(m, 2.0);
        assert!((h - 4.303 / 3f64.sqrt()).abs() < 1e-12);
    }
}
