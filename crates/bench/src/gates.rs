use crate::codec::*;
use crate::corpus::*;
use crate::{ddsraw, metrics};
use anyhow::{anyhow, bail, Result};
use encoders_sys::{gather_blocks, init, Bc1Model, Bc7Level, Bc7Params};
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

/// The BC4 halves of one `kind` block as (byte offset, source channel); empty for BC1.
fn bc4_halves(kind: RgbcxKind) -> &'static [(usize, usize)] {
    match kind {
        RgbcxKind::Bc1 { .. } => &[],
        RgbcxKind::Bc3 => &[(0, 3)],
        RgbcxKind::Bc4 => &[(0, 0)],
        RgbcxKind::Bc5 => &[(0, 0), (8, 1)],
    }
}

/// Squared error of one BC4 block decoded by bcdec against `src` channel `ch`.
fn bc4_sse(blk: &[u8], src: &[[u8; 4]], ch: usize) -> u64 {
    let mut d = [0u8; 16];
    bcdec_rs::bc4(blk, &mut d, 4, false);
    (0..16)
        .map(|t| (d[t] as i64 - src[t][ch] as i64).pow(2) as u64)
        .sum()
}

/// Spec 11.2 rgbcx row against bc7enc_rdo's bytes for the same images. BC1 (and BC3's colour half) must be
/// bit-exact. The BC4 halves (BC3 alpha, BC4, BC5) differ since the M6 Track D rgbcx round-to-nearest patch; the
/// search tries the same endpoint pairs and now scores them as bcdec decodes, so no block may decode (bcdec) worse
/// than the bench's.
pub fn rgbcx_vs_bench(
    root: &Path,
    bench_out: &str,
    images_dir: &str,
    sets: &[&str],
    kind: RgbcxKind,
) -> Result<GateReport> {
    init(Bc1Model::Ideal)?;
    let halves = bc4_halves(kind);
    let rule = if halves.is_empty() {
        "bit-exact"
    } else {
        "no BC4 block worse under bcdec"
    };
    let mut r = GateReport::new(&format!("rgbcx {kind:?} {rule} vs {bench_out}"));
    let mut total = 0;
    for set in sets {
        let files = set_files(root, images_dir, set, "png")?;
        if files.is_empty() {
            bail!("no images in {images_dir}/{set}");
        }
        for src in files {
            let stem = stem(&src);
            let img = load_png_rgba8(&src)?;
            let enc = encode_rgbcx_image(&img, kind);
            let file = std::fs::read(root.join(bench_out).join(set).join(format!("{stem}.dds")))?;
            let bench = ddsraw::payload(&file)?;
            total += 1;
            if halves.is_empty() || bench.len() != enc.len() {
                r.check(bench == enc.as_slice(), format!("{set}/{stem}"));
                continue;
            }
            let blocks = gather_blocks(&img.px, img.width, img.height);
            let bsz = enc.len() / (blocks.len() / 16);
            let (mut worse, mut other, mut ours, mut theirs) = (0usize, 0usize, 0u64, 0u64);
            for (bi, (o, t)) in enc.chunks(bsz).zip(bench.chunks(bsz)).enumerate() {
                let px = &blocks[bi * 16..bi * 16 + 16];
                for &(off, ch) in halves {
                    let (a, b) = (
                        bc4_sse(&o[off..off + 8], px, ch),
                        bc4_sse(&t[off..off + 8], px, ch),
                    );
                    worse += (a > b) as usize;
                    (ours, theirs) = (ours + a, theirs + b);
                }
                // BC3's colour half is rgbcx BC1, which the patch does not touch.
                other += (bsz == 16 && halves.len() == 1 && o[8..] != t[8..]) as usize;
            }
            r.check(
                worse == 0 && other == 0,
                format!("{set}/{stem}: bcdec SSE {ours} vs bench {theirs}, blocks worse {worse}, colour blocks differing {other}"),
            );
        }
    }
    r.info(format!("{total} images compared"));
    Ok(r)
}

pub const OPAQUE_SETS: [&str; 2] = ["rgb_real", "rgb_grad"];
pub const ALPHA_SETS: [&str; 8] = [
    "cutout_real",
    "cutout_grad",
    "alpha_real",
    "alpha_grad",
    "packed_real",
    "packed_grad",
    "stray_real",
    "stray_grad",
];

/// Images on disk per rgbcx gate; a gate that sees fewer has been run on a shrunken corpus.
pub const BC1_IMAGES: usize = 14;
pub const BC3_IMAGES: usize = 25;
pub const BC4_IMAGES: usize = 12;
pub const BC5_IMAGES: usize = 11;

/// Fail the report unless it compared exactly `expected` images.
fn expect_count(r: &mut GateReport, expected: usize) {
    let seen = r.lines.iter().filter(|l| !l.starts_with("info")).count();
    r.check(
        seen == expected,
        format!("image count {seen} == expected {expected}"),
    );
}

/// BC1/BC3/BC4/BC5 against the bench's bc7enc_rdo output (BC1 with transparent-black on, as benched).
pub fn rgbcx_all(root: &Path) -> Result<Vec<GateReport>> {
    let mut v = vec![
        rgbcx_vs_bench(
            root,
            "out/bc1_rgbcx_L18",
            "images",
            &OPAQUE_SETS,
            RgbcxKind::Bc1 {
                transparent_black: true,
            },
        )?,
        rgbcx_vs_bench(
            root,
            "out/bc3_rgbcx_L18",
            "images",
            &ALPHA_SETS,
            RgbcxKind::Bc3,
        )?,
        rgbcx_vs_bench(
            root,
            "out2/bc4_rgbcx",
            "images2",
            &["mask_real", "mask_grad"],
            RgbcxKind::Bc4,
        )?,
        rgbcx_vs_bench(
            root,
            "out2/bc5_rgbcx",
            "images2",
            &["normal_real", "normal_grad"],
            RgbcxKind::Bc5,
        )?,
    ];
    for (r, n) in v
        .iter_mut()
        .zip([BC1_IMAGES, BC3_IMAGES, BC4_IMAGES, BC5_IMAGES])
    {
        expect_count(r, n);
    }
    Ok(v)
}

/// Runs the bench's own bc7enc.exe with -b (3-colour mode never used for black) and compares bytes
/// with sdc's shipped BC1 setting (transparent-black off).
pub fn bc1_black_off_vs_tool(root: &Path, work: &Path) -> Result<GateReport> {
    init(Bc1Model::Ideal)?;
    let tool = root.join("build/bc7enc_rdo/bc7enc.exe");
    let mut r = GateReport::new("rgbcx BC1 transparent-black OFF == bc7enc -1 -b");
    for set in OPAQUE_SETS {
        for src in set_files(root, "images", set, "png")? {
            let stem = stem(&src);
            let dst = work.join(format!("{set}_{stem}.dds"));
            let out = std::process::Command::new(&tool)
                .args(["-q", "-g", "-1", "-b"])
                .arg(&src)
                .arg(&dst)
                .output()?;
            if !out.status.success() || !dst.exists() {
                bail!(
                    "bc7enc failed on {}: {}",
                    src.display(),
                    String::from_utf8_lossy(&out.stderr)
                );
            }
            let ours = encode_rgbcx_image(
                &load_png_rgba8(&src)?,
                RgbcxKind::Bc1 {
                    transparent_black: false,
                },
            );
            r.check(
                ddsraw::payload(&std::fs::read(&dst)?)? == ours.as_slice(),
                format!("{set}/{stem}"),
            );
        }
    }
    expect_count(&mut r, BC1_IMAGES);
    Ok(r)
}

/// Images on disk per BC6H set; a gate that sees fewer has been run on a shrunken corpus.
pub const HDR_REAL_IMAGES: usize = 4;
pub const HDR_GRAD_IMAGES: usize = 4;

/// Spec 11.2 last row. Input is the bench's raw `.f16` twin of each HDR image (identical data to `.ref.npy`).
/// The bench decoded with texconv; sdc decodes with bcdec_rs (spec 11.4 asserts they agree exactly for BC6H).
pub fn bc6h_veryslow(root: &Path) -> Result<GateReport> {
    init(Bc1Model::Ideal)?;
    let rows = read_results(&root.join("results/results2.csv"), "bc6_ispc_veryslow")?;
    let mut r = GateReport::new("ISPC bc6h_veryslow reproduces results2.csv mPSNR");
    let (mut identical, mut total) = (0, 0);
    for (set, expected) in [("hdr_real", HDR_REAL_IMAGES), ("hdr_grad", HDR_GRAD_IMAGES)] {
        let (mut ours, mut theirs) = (vec![], vec![]);
        let files = set_files(root, "images2", set, "f16")?;
        r.check(
            files.len() == expected,
            format!("{set}: image count {} == expected {expected}", files.len()),
        );
        for src in files {
            let stem = stem(&src);
            let img = load_f16(&src)?;
            let enc = encode_bc6h_image(&img);
            let bench = std::fs::read(
                root.join("out2/bc6_ispc_veryslow")
                    .join(set)
                    .join(format!("{stem}.dds")),
            )?;
            let same = ddsraw::payload(&bench)? == enc.as_slice();
            total += 1;
            identical += same as usize;
            let m = metrics::mpsnr(
                &half_to_rgb_f32(&img),
                &decode_bc6h_image(&enc, img.width, img.height),
            );
            let e = find(&rows, set, &stem)
                .and_then(|row| row.f("mpsnr"))
                .ok_or_else(|| anyhow!("no results2.csv row for {set}/{stem}"))?;
            r.check(
                (m - e).abs() <= 0.01,
                format!(
                    "{set}/{stem}: mPSNR {m:.4} (bench {e:.4}), bytes {}",
                    if same { "identical" } else { "DIFFER" }
                ),
            );
            ours.push(m);
            theirs.push(e);
        }
        if set == "hdr_real" {
            let m = metrics::mean(&ours);
            r.check(
                (m - 51.50).abs() <= 0.01,
                format!(
                    "hdr_real mean mPSNR {m:.4} (bench {:.4}, spec 51.50)",
                    metrics::mean(&theirs)
                ),
            );
        }
    }
    r.info(format!(
        "{identical}/{total} images have block payloads byte-identical to the bench"
    ));
    Ok(r)
}

fn dds_dims(bytes: &[u8]) -> (u32, u32) {
    let u = |i: usize| u32::from_le_bytes(bytes[i..i + 4].try_into().unwrap());
    (u(16), u(12)) // (width, height)
}

/// Bench variants scanned by S5a (all but the last are BC7; the last is BC1).
const ALPHA_SCAN_VARIANTS: [&str; 6] = [
    "bc7e_u5",
    "bc7e_u6",
    "ispc_slow",
    "nvtt_production",
    "cmp_q1.0",
    "bc1_rgbcx_L18",
];

/// S5a: decoded alpha at mip 0 for existing bench outputs of the opaque sets (no new encodes).
pub fn alpha_scan(root: &Path) -> Result<GateReport> {
    use engine::validate::alpha::alpha_stats;
    init(Bc1Model::Ideal)?;
    let mut r = GateReport::new("S5a decoded alpha on bench outputs (opaque sets, mip 0)");
    let mut seen_bricks = false;
    for v in ALPHA_SCAN_VARIANTS {
        for set in OPAQUE_SETS {
            let dir = root.join("out").join(v).join(set);
            if !dir.exists() {
                r.info(format!("{v}/{set}: missing, skipped"));
                continue;
            }
            for src in set_files(root, "images", set, "png")? {
                let stem = stem(&src);
                let bytes = std::fs::read(dir.join(format!("{stem}.dds")))?;
                let (w, h) = dds_dims(&bytes);
                let payload = ddsraw::payload(&bytes)?;
                let dec = if v.starts_with("bc1") {
                    decode_bc1_image(payload, w, h, Bc1Model::Ideal)
                } else {
                    decode_bc7_image(payload, w, h)
                };
                let s = alpha_stats(&dec);
                r.info(format!(
                    "{v}/{set}/{stem}: 254 share {:.2}%, below-254 {}, min {}",
                    s.share_254() * 100.0,
                    s.below254,
                    s.min
                ));
                if v == "bc7e_u6" && stem == "Bricks104" {
                    seen_bricks = true;
                    r.check(
                        (s.share_254() * 100.0 - 21.0).abs() <= 0.1,
                        format!(
                            "bc7e_u6 Bricks104 254 share {:.2}% (Note C: 21.0%)",
                            s.share_254() * 100.0
                        ),
                    );
                }
                if v == "nvtt_production" {
                    r.check(
                        s.eq255 == s.pixels,
                        format!("nvtt_production {set}/{stem} all 255"),
                    );
                }
                if v.starts_with("bc7e") {
                    r.check(
                        s.min >= 254,
                        format!("{v} {set}/{stem} min alpha {} >= 254", s.min),
                    );
                }
            }
        }
    }
    r.check(seen_bricks, "bc7e_u6 Bricks104 was scanned".to_string());
    // 6 variants x 14 opaque images: 84 info lines; checks counted by expect_count are the non-info lines.
    let scanned = r.lines.iter().filter(|l| l.contains(": 254 share")).count();
    r.check(
        scanned == ALPHA_SCAN_VARIANTS.len() * 14,
        format!(
            "scanned {scanned} images == expected {}",
            ALPHA_SCAN_VARIANTS.len() * 14
        ),
    );
    Ok(r)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExactMode {
    BytePatch,
    Native,
}

/// Images on disk per exact-opaque set; a gate that sees fewer has been run on a shrunken corpus.
pub const EXACT_OPAQUE_SETS: [(&str, usize); 4] = [
    ("rgb_real", 7),
    ("rgb_grad", 7),
    ("stray_real", 2),
    ("stray_grad", 2),
];

/// Spec §11.2 exact-opaque row at mip 0: opaque sets plus stray sets after `remove` (alpha forced to 255 before encoding).
/// Pass: decoded alpha == 255 on every pixel; per-set mean RGB PSNR within 0.05 dB of plain u5. Worst image reported.
/// The standalone S5 gate: it fails by design on the real corpus (DECISIONS S5); `run("all")` uses
/// [`exact_opaque_alpha_only`] instead.
pub fn exact_opaque(root: &Path, mode: ExactMode) -> Result<GateReport> {
    exact_opaque_checked(root, mode, true)
}

/// The exact-opaque row as `gate all` enforces it (Task 38 ruling: the flag is scoped to `_m`, and the owner
/// accepted the RGB cost, DECISIONS S5): pass is decoded alpha == 255 on every pixel and the image counts. The
/// per-set mean dRGB (limit -0.05 dB) is printed as info, not a criterion.
pub fn exact_opaque_alpha_only(root: &Path, mode: ExactMode) -> Result<GateReport> {
    exact_opaque_checked(root, mode, false)
}

fn exact_opaque_checked(root: &Path, mode: ExactMode, enforce_drgb: bool) -> Result<GateReport> {
    use encoders_sys::bc7_fixup::force_opaque_mode6;
    if mode == ExactMode::Native && !encoders_sys::BC7E_EXACT_OPAQUE_NATIVE {
        bail!("ExactMode::Native needs the patched bc7e objects (Task 14)");
    }
    init(Bc1Model::Ideal)?;
    let mut r = GateReport::new(&format!(
        "exact opaque alpha ({mode:?}), mip 0{}",
        if enforce_drgb {
            ""
        } else {
            ", alpha == 255 (dRGB info)"
        }
    ));
    for (set, expected) in EXACT_OPAQUE_SETS {
        let files = set_files(root, "images", set, "png")?;
        r.check(
            files.len() == expected,
            format!("{set}: image count {} == expected {expected}", files.len()),
        );
        let mut deltas = vec![];
        for src in files {
            let stem = stem(&src);
            let mut img = load_png_rgba8(&src)?;
            img.px.iter_mut().for_each(|p| p[3] = 255); // alpha `remove`
            let plain = encode_bc7_image(&img, U5);
            let fixed = match mode {
                ExactMode::BytePatch => {
                    let mut b = plain.clone();
                    let mut patched = 0;
                    for blk in b.chunks_mut(16) {
                        patched += force_opaque_mode6(blk.try_into().unwrap()) as usize;
                    }
                    r.info(format!(
                        "{set}/{stem}: {patched}/{} blocks were mode 6 and patched",
                        b.len() / 16
                    ));
                    b
                }
                ExactMode::Native => encode_bc7_image(
                    &img,
                    Bc7Params {
                        exact_opaque: true,
                        ..U5
                    },
                ),
            };
            let dp = decode_bc7_image(&plain, img.width, img.height);
            let df = decode_bc7_image(&fixed, img.width, img.height);
            let not255 = df.iter().filter(|p| p[3] != 255).count();
            r.check(
                not255 == 0,
                format!("{set}/{stem}: {not255} texels with alpha != 255"),
            );
            let (pp, pf) = (
                metrics::rgb_psnr(&img.px, &dp),
                metrics::rgb_psnr(&img.px, &df),
            );
            let d = pf - pp;
            r.info(format!(
                "{set}/{stem}: dRGB {d:+.4} dB (plain {pp:.4}, exact {pf:.4})"
            ));
            deltas.push(d);
        }
        let worst = deltas.iter().cloned().fold(f64::INFINITY, f64::min);
        let m = metrics::mean(&deltas);
        let line = format!("{set}: mean dRGB {m:+.4} dB (limit -0.05), worst image {worst:+.4} dB");
        if enforce_drgb {
            r.check(m >= -0.05, line);
        } else {
            r.info(line);
        }
    }
    Ok(r)
}

fn sha256_hex(b: &[u8]) -> String {
    use sha2::{Digest, Sha256};
    hex::encode(Sha256::digest(b))
}

/// Images on disk over all `BC7_SETS` (Task 9's total); the hash dump and its gate must see exactly this many.
pub const BC7_IMAGES: usize = 39;

/// Writes "<set>/<stem> <sha256 of the u5 flag-off BC7 payload>" for every BC7 bench image.
pub fn dump_bc7e(root: &Path, out: &Path) -> Result<usize> {
    init(Bc1Model::Ideal)?;
    let mut lines = vec![];
    for set in BC7_SETS {
        for src in set_files(root, "images", set, "png")? {
            let enc = encode_bc7_image(&load_png_rgba8(&src)?, U5);
            lines.push(format!("{set}/{} {}", stem(&src), sha256_hex(&enc)));
        }
    }
    if lines.len() != BC7_IMAGES {
        bail!("found {} BC7 images, expected {BC7_IMAGES}", lines.len());
    }
    std::fs::create_dir_all(out.parent().unwrap())?;
    std::fs::write(
        out,
        lines.join(
            "
",
        ) + "
",
    )?;
    Ok(lines.len())
}

/// §11.2 "bc7e patch, flag off: bit-identical to unpatched upstream on every set".
pub fn bc7e_matches_dump(root: &Path, hashes: &Path) -> Result<GateReport> {
    init(Bc1Model::Ideal)?;
    let text = std::fs::read_to_string(hashes)?;
    let want: std::collections::BTreeMap<&str, &str> =
        text.lines().filter_map(|l| l.split_once(' ')).collect();
    let mut r = GateReport::new("bc7e flag off is bit-identical to the recorded pre-patch hashes");
    r.check(
        want.len() == BC7_IMAGES,
        format!(
            "hash file has {} entries == expected {BC7_IMAGES}",
            want.len()
        ),
    );
    let mut seen = 0;
    for set in BC7_SETS {
        for src in set_files(root, "images", set, "png")? {
            let key = format!("{set}/{}", stem(&src));
            let got = sha256_hex(&encode_bc7_image(&load_png_rgba8(&src)?, U5));
            r.check(want.get(key.as_str()) == Some(&got.as_str()), key);
            seen += 1;
        }
    }
    r.check(
        seen == BC7_IMAGES,
        format!("image count {seen} == expected {BC7_IMAGES}"),
    );
    Ok(r)
}

/// Committed reference for the flag-off bit-identity gate (recorded from the unpatched objects).
pub fn pre_patch_hashes() -> std::path::PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/data/pre-patch-bc7e.sha256")
}

pub fn run(name: &str, root: &Path) -> Result<Vec<GateReport>> {
    Ok(match name {
        "s3" => vec![
            bc7e_u5(root, false)?,
            rgbcx_vs_bench(
                root,
                "out2/bc4_rgbcx",
                "images2",
                &["mask_real"],
                RgbcxKind::Bc4,
            )?,
        ],
        "rgbcx" => {
            let mut v = rgbcx_all(root)?;
            let work =
                Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/bench/bc1-black-off");
            std::fs::create_dir_all(&work)?;
            v.push(bc1_black_off_vs_tool(root, &work)?);
            v
        }
        "bc6h" => vec![bc6h_veryslow(root)?],
        "alpha-scan" => vec![alpha_scan(root)?],
        "engine-parity" => vec![crate::engine_gates::engine_parity(root)?],
        "full-chain" => vec![crate::engine_gates::full_chain_exact_opaque(root)?],
        // Spec §11.2: every gate in one run. The exact-opaque row passes on alpha == 255 (dRGB is info) plus the
        // bc7e flag-off bit-identity check; the standalone `exact-opaque-*` gates keep the -0.05 dB criterion.
        "all" => {
            let mut v = vec![];
            for g in [
                "s3",
                "rgbcx",
                "bc6h",
                "alpha-scan",
                "engine-parity",
                "full-chain",
            ] {
                v.extend(run(g, root)?);
            }
            v.push(exact_opaque_alpha_only(
                root,
                if encoders_sys::BC7E_EXACT_OPAQUE_NATIVE {
                    ExactMode::Native
                } else {
                    ExactMode::BytePatch
                },
            )?);
            v.push(bc7e_matches_dump(root, &pre_patch_hashes())?);
            v
        }
        "exact-opaque-bytepatch" => vec![exact_opaque(root, ExactMode::BytePatch)?],
        "exact-opaque-native" => vec![
            exact_opaque(root, ExactMode::Native)?,
            bc7e_matches_dump(root, &pre_patch_hashes())?,
        ],
        other => bail!("unknown gate '{other}'"),
    })
}
