use crate::codec::*;
use crate::corpus::*;
use crate::{ddsraw, metrics};
use anyhow::{anyhow, bail, Result};
use encoders_sys::{init, Bc1Model, Bc7Level, Bc7Params};
use std::fmt;
use std::path::Path;

pub struct GateReport {
    pub name: String,
    pub pass: bool,
    pub lines: Vec<String>,
}

impl GateReport {
    pub fn new(name: &str) -> Self {
        GateReport {
            name: name.to_string(),
            pass: true,
            lines: vec![],
        }
    }
    pub fn check(&mut self, ok: bool, line: String) {
        self.pass &= ok;
        self.lines
            .push(format!("{} {line}", if ok { "ok  " } else { "FAIL" }));
    }
    pub fn info(&mut self, line: String) {
        self.lines.push(format!("info {line}"));
    }
}

impl fmt::Display for GateReport {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        writeln!(
            f,
            "== {} : {}",
            self.name,
            if self.pass { "PASS" } else { "FAIL" }
        )?;
        for l in &self.lines {
            writeln!(f, "   {l}")?;
        }
        Ok(())
    }
}

pub const BC7_SETS: [&str; 10] = [
    "rgb_real",
    "rgb_grad",
    "alpha_real",
    "alpha_grad",
    "cutout_real",
    "cutout_grad",
    "packed_real",
    "packed_grad",
    "stray_real",
    "stray_grad",
];

pub const U5: Bc7Params = Bc7Params {
    level: Bc7Level::Veryslow,
    perceptual: false,
    exact_opaque: false,
};

/// Spec §12 S3 and §11.2 rows 1-2: bc7e u5 (veryslow, linear) on every bench BC7 set, mip 0.
pub fn bc7e_u5(root: &Path, require_identical_bytes: bool) -> Result<GateReport> {
    init(Bc1Model::Ideal)?;
    let rows = read_results(&root.join("results/results.csv"), "bc7e_u5")?;
    let mut r = GateReport::new("bc7e u5 reproduces results.csv (S3, §11.2)");
    let (mut identical, mut total) = (0, 0);
    let mut max_delta = 0f64;
    for set in BC7_SETS {
        let (mut ours, mut theirs) = (vec![], vec![]);
        for src in set_files(root, "images", set, "png")? {
            let stem = stem(&src);
            let img = load_png_rgba8(&src)?;
            let enc = encode_bc7_image(&img, U5);
            let bench = std::fs::read(
                root.join("out/bc7e_u5")
                    .join(set)
                    .join(format!("{stem}.dds")),
            )?;
            let same = ddsraw::payload(&bench)? == enc.as_slice();
            total += 1;
            identical += same as usize;
            if require_identical_bytes {
                r.check(
                    same,
                    format!("{set}/{stem}: block bytes identical to bench"),
                );
            }
            let dec = decode_bc7_image(&enc, img.width, img.height);
            let (rgb, a) = (
                metrics::rgb_psnr(&img.px, &dec),
                metrics::a_psnr(&img.px, &dec),
            );
            let row = find(&rows, set, &stem)
                .ok_or_else(|| anyhow!("no results.csv row for {set}/{stem}"))?;
            let (er, ea) = (
                row.f("rgb_psnr")
                    .ok_or_else(|| anyhow!("no rgb_psnr for {set}/{stem}"))?,
                row.f("a_psnr")
                    .ok_or_else(|| anyhow!("no a_psnr for {set}/{stem}"))?,
            );
            max_delta = max_delta.max((rgb - er).abs()).max((a - ea).abs());
            r.check(
                (rgb - er).abs() <= 0.01 && (a - ea).abs() <= 0.01,
                format!(
                    "{set}/{stem}: rgb {rgb:.4} (bench {er:.4}), a {a:.4} (bench {ea:.4}), bytes {}",
                    if same { "identical" } else { "DIFFER" }
                ),
            );
            ours.push(rgb);
            theirs.push(er);
        }
        if set == "rgb_real" {
            let (m, mb) = (metrics::mean(&ours), metrics::mean(&theirs));
            r.check(
                (m - 50.98).abs() <= 0.01 && (m - mb).abs() <= 0.01,
                format!("rgb_real mean RGB PSNR {m:.4} dB (bench {mb:.4}, spec 50.98)"),
            );
        }
    }
    r.info(format!(
        "{identical}/{total} images have block payloads byte-identical to the bench"
    ));
    r.info(format!(
        "largest per-image |delta| over all BC7 sets (rgb and a): {max_delta:.6} dB"
    ));
    Ok(r)
}

/// Spec §11.2 rgbcx row: our rgbcx output must equal bc7enc_rdo's bytes for the same images.
pub fn rgbcx_bit_exact(
    root: &Path,
    bench_out: &str,
    images_dir: &str,
    sets: &[&str],
    kind: RgbcxKind,
) -> Result<GateReport> {
    init(Bc1Model::Ideal)?;
    let mut r = GateReport::new(&format!("rgbcx {kind:?} bit-exact vs {bench_out}"));
    for set in sets {
        let files = set_files(root, images_dir, set, "png")?;
        if files.is_empty() {
            bail!("no images in {images_dir}/{set}");
        }
        for src in files {
            let stem = stem(&src);
            let enc = encode_rgbcx_image(&load_png_rgba8(&src)?, kind);
            let bench = std::fs::read(root.join(bench_out).join(set).join(format!("{stem}.dds")))?;
            r.check(
                ddsraw::payload(&bench)? == enc.as_slice(),
                format!("{set}/{stem}"),
            );
        }
    }
    Ok(r)
}

pub fn run(name: &str, root: &Path) -> Result<Vec<GateReport>> {
    Ok(match name {
        "s3" => vec![
            bc7e_u5(root, false)?,
            rgbcx_bit_exact(
                root,
                "out2/bc4_rgbcx",
                "images2",
                &["mask_real"],
                RgbcxKind::Bc4,
            )?,
        ],
        other => bail!("unknown gate '{other}'"),
    })
}
