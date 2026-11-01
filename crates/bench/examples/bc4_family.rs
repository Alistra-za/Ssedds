//! BC4-family research (docs/research/bc4-family.md): why BC3 cutout alpha trails NVTT / Compressonator, and what a
//! better BC4 block search buys on BC3 alpha, BC4 and BC5. Reads the bench corpus (`SDC_BENCH_ROOT`) only and writes
//! CSVs to `--out` (must be outside the bench root).
//!
//! Everything is scored with the round-to-nearest BC4 decoder (bcdec weights, the vendored rgbcx after Note T13).
//! Variants (all start from the shipped result and only replace it on a strictly lower SSE, so ties keep its bytes):
//! - `v0`: a Rust port of `rgbcx::encode_bc4_hq` (radius 5, both modes), checked byte-identical to the FFI encoder.
//! - `v1`: v0 + a 6-value-mode search anchored on the block's inner range (values other than 0 and 255), e0 == e1
//!   allowed, for blocks that contain 0 or 255.
//! - `v2`: v1 + least-squares endpoint refits (both modes, 3 rounds, +-1 rounding neighbourhood).
//! - `v3`: v2 + an exhaustive pair search over [min-16, max+16] for both modes (`--oracle`; slow; a near-ceiling).
//! - `v4`: v2 + a 6-value search on trimmed inner ranges (texels within 2/6/12/24/48 of 0 or 255 left to the palette)
//!   + an exhaustive +-3 search around the best pair.
//! - `v5`: v4, but a block falls back to v0 when it would flip more texels across the alpha-test value 128.
//!
//! usage: cargo run --release -p sdc-bench --example bc4_family -- --out <dir> [--oracle] [--sets a,b] [--rounds 1,2,3]
// Research probe: index loops mirror the C++ it ports, and the per-encoder tuples stay local.
#![allow(clippy::needless_range_loop, clippy::type_complexity)]
use rayon::prelude::*;
use sdc_bench::corpus::{bench_root, load_png_rgba8, set_files, stem};
use sdc_bench::ddsraw::payload;
use std::fmt::Write as _;
use std::path::{Path, PathBuf};
use std::time::Instant;

const RAD: i32 = 5;
const ORACLE_PAD: i32 = 16;

/// The bcdec / patched-rgbcx BC4 palette (`get_block_values`); checked against the FFI for all 65536 pairs.
fn pal(e0: u8, e1: u8) -> [u8; 8] {
    let (l, h) = (e0 as u32, e1 as u32);
    let w = |a: u32, b: u32| ((a * l + b * h + 32768) >> 16) as u8;
    if l > h {
        [
            e0,
            e1,
            w(56173, 9363),
            w(46812, 18724),
            w(37450, 28086),
            w(28086, 37450),
            w(18724, 46812),
            w(9363, 56173),
        ]
    } else {
        [
            e0,
            e1,
            w(52429, 13107),
            w(39321, 26215),
            w(26215, 39321),
            w(13107, 52429),
            0,
            255,
        ]
    }
}

#[derive(Clone, Copy)]
struct Best {
    err: u32,
    e0: u8,
    e1: u8,
    sels: [u8; 16],
}

impl Best {
    fn new() -> Self {
        Best {
            err: u32::MAX,
            e0: 0,
            e1: 0,
            sels: [0; 16],
        }
    }
    fn bytes(&self) -> [u8; 8] {
        let mut bits = 0u64;
        for (i, &s) in self.sels.iter().enumerate() {
            bits |= (s as u64) << (3 * i);
        }
        let mut o = [0u8; 8];
        o[0] = self.e0;
        o[1] = self.e1;
        o[2..].copy_from_slice(&bits.to_le_bytes()[..6]);
        o
    }
    /// Try one endpoint pair exactly as rgbcx does (first minimum per texel, early out, strict improvement).
    fn try_pair(&mut self, v: &[u8; 16], e0: u8, e1: u8) {
        let p = pal(e0, e1);
        let mut err = 0u32;
        let mut sels = [0u8; 16];
        for i in 0..16 {
            let (mut be, mut bj) = (u32::MAX, 0u8);
            for (j, &pj) in p.iter().enumerate() {
                let d = pj as i32 - v[i] as i32;
                let e = (d * d) as u32;
                if e < be {
                    be = e;
                    bj = j as u8;
                    if e == 0 {
                        break;
                    }
                }
            }
            err += be;
            if err >= self.err {
                return;
            }
            sels[i] = bj;
        }
        *self = Best { err, e0, e1, sels };
    }
}

fn minmax(v: &[u8; 16]) -> (u8, u8) {
    (*v.iter().min().unwrap(), *v.iter().max().unwrap())
}

fn clampi(x: i32) -> u8 {
    x.clamp(0, 255) as u8
}

/// Port of `rgbcx::encode_bc4_hq(.., search_rad, BC4_USE_ALL_MODES)`.
fn v0(v: &[u8; 16]) -> Best {
    let (mn, mx) = minmax(v);
    if mn == mx {
        return Best {
            err: 0,
            e0: mn,
            e1: mn,
            sels: [0; 16],
        };
    }
    let mut b = Best::new();
    for mode in 0..2 {
        for lo in -RAD..=RAD {
            for hi in -RAD..=RAD {
                let (mut e0, mut e1) = (clampi(mx as i32 + hi), clampi(mn as i32 + lo));
                if e0 == e1 {
                    continue;
                }
                if (mode == 0 && e0 <= e1) || (mode == 1 && e0 > e1) {
                    std::mem::swap(&mut e0, &mut e1);
                }
                b.try_pair(v, e0, e1);
            }
        }
    }
    b
}

/// The 6-value-mode search rgbcx cannot reach: endpoints around the inner range, so 0 and 255 come from the palette.
fn inner6(v: &[u8; 16], b: &mut Best) {
    if !v.iter().any(|&x| x == 0 || x == 255) {
        return;
    }
    let inner: Vec<u8> = v.iter().copied().filter(|&x| x != 0 && x != 255).collect();
    let (Some(&imn), Some(&imx)) = (inner.iter().min(), inner.iter().max()) else {
        return;
    };
    for lo in -RAD..=RAD {
        for hi in -RAD..=RAD {
            let (e0, e1) = (clampi(imn as i32 + lo), clampi(imx as i32 + hi));
            if e0 <= e1 {
                b.try_pair(v, e0, e1);
            }
        }
    }
}

/// Fraction of e1 in texel value for selector `s` (None = fixed 0/255 in the 6-value mode).
fn frac(mode8: bool, s: u8) -> Option<f64> {
    match (mode8, s) {
        (_, 0) => Some(0.0),
        (_, 1) => Some(1.0),
        (true, s) => Some((s - 1) as f64 / 7.0),
        (false, 6 | 7) => None,
        (false, s) => Some((s - 1) as f64 / 5.0),
    }
}

/// Least-squares refit of the endpoints for the current selectors, then the +-1 neighbourhood in the same mode.
fn ls_refine(v: &[u8; 16], b: &mut Best) {
    for _ in 0..3 {
        let mode8 = b.e0 > b.e1;
        let (mut aa, mut ab, mut bb, mut av, mut bv) = (0.0, 0.0, 0.0, 0.0, 0.0);
        for i in 0..16 {
            if let Some(t) = frac(mode8, b.sels[i]) {
                let (wa, wb, x) = (1.0 - t, t, v[i] as f64);
                aa += wa * wa;
                ab += wa * wb;
                bb += wb * wb;
                av += wa * x;
                bv += wb * x;
            }
        }
        let det = aa * bb - ab * ab;
        if det.abs() < 1e-9 {
            return;
        }
        let a = (av * bb - bv * ab) / det;
        let c = (bv * aa - av * ab) / det;
        let before = b.err;
        for da in -1..=1 {
            for dc in -1..=1 {
                let (e0, e1) = (clampi(a.round() as i32 + da), clampi(c.round() as i32 + dc));
                if (e0 > e1) == mode8 {
                    b.try_pair(v, e0, e1);
                }
            }
        }
        if b.err >= before {
            return;
        }
    }
}

/// Exhaustive pairs over [min-16, max+16] (both modes), seeded with `b`.
fn oracle(v: &[u8; 16], b: &mut Best) {
    if b.err == 0 {
        return;
    }
    let (mn, mx) = minmax(v);
    let (lo, hi) = (
        clampi(mn as i32 - ORACLE_PAD),
        clampi(mx as i32 + ORACLE_PAD),
    );
    for e0 in lo..=hi {
        for e1 in lo..=hi {
            b.try_pair(v, e0, e1);
        }
    }
}

fn variant(k: usize, v: &[u8; 16]) -> Best {
    let mut b = v0(v);
    if k >= 1 {
        inner6(v, &mut b);
    }
    if k >= 2 {
        // Refit the best of each mode, not only the overall best.
        ls_refine(v, &mut b);
        let (mn, mx) = minmax(v);
        if mn != mx {
            // best block in the other mode (rgbcx's own candidates), refit too
            let mut o = Best::new();
            let mode8 = b.e0 > b.e1;
            for lo in -RAD..=RAD {
                for hi in -RAD..=RAD {
                    let (mut e0, mut e1) = (clampi(mx as i32 + hi), clampi(mn as i32 + lo));
                    if e0 == e1 {
                        continue;
                    }
                    if (e0 > e1) == mode8 {
                        std::mem::swap(&mut e0, &mut e1);
                    }
                    o.try_pair(v, e0, e1);
                }
            }
            ls_refine(v, &mut o);
            if o.err < b.err {
                b = o;
            }
        }
    }
    if k == 3 {
        oracle(v, &mut b);
    }
    if k >= 4 {
        trimmed6(v, &mut b);
        local(v, &mut b);
    }
    if k == 5 && flips(v, &b) > flips(v, &v0(v)) {
        b = v0(v);
    }
    b
}

/// 6-value mode with near-extreme texels left to the palette's 0/255: inner range = values in (t, 255 - t).
fn trimmed6(v: &[u8; 16], b: &mut Best) {
    for t in [2u8, 6, 12, 24, 48] {
        if !v.iter().any(|&x| x <= t || x >= 255 - t) {
            continue;
        }
        let inner: Vec<u8> = v
            .iter()
            .copied()
            .filter(|&x| x > t && x < 255 - t)
            .collect();
        let (Some(&imn), Some(&imx)) = (inner.iter().min(), inner.iter().max()) else {
            continue;
        };
        let mut c = *b;
        for lo in -2..=2 {
            for hi in -2..=2 {
                let (e0, e1) = (clampi(imn as i32 + lo), clampi(imx as i32 + hi));
                if e0 <= e1 {
                    c.try_pair(v, e0, e1);
                }
            }
        }
        if c.e0 <= c.e1 {
            ls_refine(v, &mut c);
        }
        if c.err < b.err {
            *b = c;
        }
    }
}

/// Exhaustive +-3 around the current best, same mode.
fn local(v: &[u8; 16], b: &mut Best) {
    let (c0, c1, m8) = (b.e0 as i32, b.e1 as i32, b.e0 > b.e1);
    for d0 in -3..=3 {
        for d1 in -3..=3 {
            let (e0, e1) = (clampi(c0 + d0), clampi(c1 + d1));
            if (e0 > e1) == m8 {
                b.try_pair(v, e0, e1);
            }
        }
    }
}

/// Texels whose alpha-test result at 128 differs from the source.
fn flips(v: &[u8; 16], b: &Best) -> u32 {
    let p = pal(b.e0, b.e1);
    (0..16)
        .filter(|&i| (v[i] >= 128) != (p[b.sels[i] as usize] >= 128))
        .count() as u32
}

fn decode(b: &[u8]) -> [u8; 16] {
    let mut o = [0u8; 16];
    bcdec_rs::bc4(b, &mut o, 4, false);
    o
}

fn sse(a: &[u8; 16], v: &[u8; 16]) -> u64 {
    a.iter()
        .zip(v)
        .map(|(&x, &y)| ((x as i64 - y as i64).pow(2)) as u64)
        .sum()
}

fn psnr(mse: f64) -> f64 {
    if mse <= 1e-12 {
        99.0
    } else {
        10.0 * (255.0f64 * 255.0 / mse).log10()
    }
}

/// Block class by value content (alpha-oriented, applied to every channel).
fn class(v: &[u8; 16]) -> &'static str {
    let (mn, mx) = minmax(v);
    let (z, f) = (v.contains(&0), v.contains(&255));
    let inner = v.iter().any(|&x| x != 0 && x != 255);
    if mn == mx {
        "const"
    } else if !inner {
        "bin"
    } else if z && f {
        "both+inner"
    } else if z || f {
        "one+inner"
    } else {
        "noext"
    }
}
const CLASSES: [&str; 5] = ["const", "bin", "both+inner", "one+inner", "noext"];

struct Job {
    round: &'static str,
    fmt: &'static str,
    set: String,
    png: PathBuf,
    /// external DDS files: (variant name, path)
    ext: Vec<(String, PathBuf)>,
}

const OWN: [&str; 6] = ["v0", "v1", "v2", "v3", "v4", "v5"];

fn ext_names(fmt: &str) -> [&'static str; 3] {
    match fmt {
        "bc3" => ["sdc_bc3_rounded", "bc3_nvtt_highest", "bc3_cmp_q1"],
        "bc4" => ["sdc_bc4_rounded", "bc4_nvtt_highest", "bc4_cmp"],
        _ => ["sdc_bc5_rounded", "bc5_nvtt_highest", "bc5_cmp"],
    }
}

fn channels(fmt: &str) -> &'static [usize] {
    match fmt {
        "bc3" => &[3],
        "bc4" => &[0],
        _ => &[0, 1],
    }
}

/// Per image results: variant -> (metrics, per-class (blocks, sse, mode8 count, mode6 count, inner6 count), secs, worse blocks)
struct ImgOut {
    rows: Vec<String>,
    classes: Vec<String>,
    attrib: Vec<String>,
    mismatch_v0_ffi: u64,
    mismatch_ext_sdc: u64,
    blocks: u64,
}

fn run(job: &Job, oracle_on: bool) -> anyhow::Result<ImgOut> {
    let img = load_png_rgba8(&job.png)?;
    let blocks = encoders_sys::gather_blocks(&img.px, img.width, img.height);
    let nb = blocks.len() / 16;
    let chans = channels(job.fmt);
    let bs = if job.fmt == "bc4" { 8 } else { 16 };
    // channel values per block
    let vals: Vec<Vec<[u8; 16]>> = chans
        .iter()
        .map(|&c| {
            (0..nb)
                .map(|b| std::array::from_fn(|t| blocks[b * 16 + t][c]))
                .collect()
        })
        .collect();
    // decoded channel blocks per encoder (name -> chan -> block -> 16 values, plus raw bytes)
    let mut enc: Vec<(String, Vec<Vec<[u8; 8]>>, f64)> = vec![];
    // shipped FFI encoder (timed)
    let t = Instant::now();
    let ffi: Vec<Vec<[u8; 8]>> = chans
        .iter()
        .map(|&c| {
            (0..nb)
                .map(|b| {
                    let src: &[[u8; 4]; 16] = blocks[b * 16..b * 16 + 16].try_into().unwrap();
                    encoders_sys::bc4_encode_block(src, c, RAD as u32)
                })
                .collect()
        })
        .collect();
    let ffi_secs = t.elapsed().as_secs_f64();
    enc.push(("ffi".into(), ffi, ffi_secs));
    for (k, name) in OWN.iter().enumerate() {
        if k == 3 && !oracle_on {
            continue;
        }
        let t = Instant::now();
        let out: Vec<Vec<[u8; 8]>> = vals
            .iter()
            .map(|cv| cv.iter().map(|v| variant(k, v).bytes()).collect())
            .collect();
        enc.push((name.to_string(), out, t.elapsed().as_secs_f64()));
    }
    for (name, path) in &job.ext {
        let bytes = std::fs::read(path)?;
        let p = payload(&bytes)?;
        if p.len() < nb * bs {
            anyhow::bail!("{} short", path.display());
        }
        let out: Vec<Vec<[u8; 8]>> = (0..chans.len())
            .map(|ci| {
                (0..nb)
                    .map(|b| p[b * bs + 8 * ci..b * bs + 8 * ci + 8].try_into().unwrap())
                    .collect()
            })
            .collect();
        enc.push((name.clone(), out, f64::NAN));
    }
    let mismatch_v0_ffi = (0..chans.len())
        .map(|ci| {
            (0..nb)
                .filter(|&b| enc[0].1[ci][b] != enc[1].1[ci][b])
                .count() as u64
        })
        .sum();
    let sdc = &job.ext.first().map(|e| e.0.clone());
    let mismatch_ext_sdc = match enc.iter().position(|e| Some(&e.0) == sdc.as_ref()) {
        Some(i) => (0..chans.len())
            .map(|ci| {
                (0..nb)
                    .filter(|&b| enc[0].1[ci][b] != enc[i].1[ci][b])
                    .count() as u64
            })
            .sum(),
        None => u64::MAX,
    };
    // per-encoder per-block decoded SSE (summed over channels) and decoded values
    let src_px = &img.px;
    let (w, h) = (img.width as usize, img.height as usize);
    let bw = w.div_ceil(4);
    let base_sse: Vec<u64> = (0..nb)
        .map(|b| {
            (0..chans.len())
                .map(|ci| sse(&decode(&enc[0].1[ci][b]), &vals[ci][b]))
                .sum()
        })
        .collect();
    let mut rows = vec![];
    let mut classes = vec![];
    let mut per_enc_sse: Vec<(String, Vec<u64>)> = vec![];
    for (name, out, secs) in &enc {
        let mut dec_img = vec![[0u8; 4]; w * h];
        let mut bsse = vec![0u64; nb];
        let mut worse = 0u64;
        let mut cls = vec![(0u64, 0u64, 0u64, 0u64, 0u64); CLASSES.len()];
        for b in 0..nb {
            for (ci, &c) in chans.iter().enumerate() {
                let blk = &out[ci][b];
                let d = decode(blk);
                let e = sse(&d, &vals[ci][b]);
                bsse[b] += e;
                let k = CLASSES
                    .iter()
                    .position(|&x| x == class(&vals[ci][b]))
                    .unwrap();
                let (mn, mx) = minmax(&vals[ci][b]);
                let m8 = blk[0] > blk[1];
                let inner =
                    !m8 && (blk[0] as i32 > mn as i32 + RAD || (blk[1] as i32) < mx as i32 - RAD);
                let s = &mut cls[k];
                s.0 += 1;
                s.1 += e;
                s.2 += m8 as u64;
                s.3 += (!m8) as u64;
                s.4 += inner as u64;
                let (bx, by) = (b % bw, b / bw);
                for t in 0..16 {
                    let (x, y) = (bx * 4 + t % 4, by * 4 + t / 4);
                    if x < w && y < h {
                        dec_img[y * w + x][c] = d[t];
                    }
                }
            }
            if bsse[b] > base_sse[b] {
                worse += 1;
            }
        }
        // metrics over the real image area
        let mut m = String::new();
        let n = (w * h) as f64;
        match job.fmt {
            "bc3" => {
                let mut se = 0.0;
                let mut cov = 0u64;
                for (s, d) in src_px.iter().zip(&dec_img) {
                    se += ((s[3] as f64) - (d[3] as f64)).powi(2);
                    cov += ((s[3] >= 128) != (d[3] >= 128)) as u64;
                }
                write!(m, "{:.6},{:.6},,,", psnr(se / n), cov as f64 / n * 100.0)?;
            }
            "bc4" => {
                let se: f64 = src_px
                    .iter()
                    .zip(&dec_img)
                    .map(|(s, d)| ((s[0] as f64) - (d[0] as f64)).powi(2))
                    .sum();
                write!(m, ",,{:.6},,", psnr(se / n))?;
            }
            _ => {
                let mut se = 0.0;
                let mut ang = 0.0;
                for (s, d) in src_px.iter().zip(&dec_img) {
                    se += ((s[0] as f64) - (d[0] as f64)).powi(2)
                        + ((s[1] as f64) - (d[1] as f64)).powi(2);
                    let tv = |x: u8| x as f64 / 255.0 * 2.0 - 1.0;
                    let sv = [tv(s[0]), tv(s[1]), tv(s[2])];
                    let (dx, dy) = (tv(d[0]), tv(d[1]));
                    let dz = (1.0 - dx * dx - dy * dy).clamp(0.0, 1.0).sqrt();
                    let dv = [dx, dy, dz];
                    let nrm =
                        |a: [f64; 3]| (a[0] * a[0] + a[1] * a[1] + a[2] * a[2]).sqrt().max(1e-9);
                    let dot = (sv[0] * dv[0] + sv[1] * dv[1] + sv[2] * dv[2]) / (nrm(sv) * nrm(dv));
                    ang += dot.clamp(-1.0, 1.0).acos().to_degrees();
                }
                write!(m, ",,{:.6},{:.6},", psnr(se / (2.0 * n)), ang / n)?;
            }
        }
        let mp = n / 1e6;
        rows.push(format!(
            "{},{},{},{},{},{}{},{}",
            job.round,
            job.fmt,
            job.set,
            stem(&job.png),
            name,
            m,
            if secs.is_nan() {
                String::new()
            } else {
                format!("{:.6}", secs / mp)
            },
            worse
        ));
        for (k, c) in CLASSES.iter().enumerate() {
            let s = cls[k];
            classes.push(format!(
                "{},{},{},{},{},{},{},{},{},{},{}",
                job.round,
                job.fmt,
                job.set,
                stem(&job.png),
                name,
                c,
                s.0,
                s.1,
                s.2,
                s.3,
                s.4
            ));
        }
        per_enc_sse.push((name.clone(), bsse));
    }
    // attribution: swap the sdc baseline (ffi) blocks of one class for another encoder's and report the PSNR change
    let mut attrib = vec![];
    let n = (w * h * chans.len()) as f64;
    let total = |v: &[u64]| v.iter().sum::<u64>() as f64;
    let blk_class: Vec<usize> = (0..nb)
        .map(|b| {
            // class of the block = worst-case class over its channels (first channel for bc3/bc4)
            CLASSES
                .iter()
                .position(|&x| x == class(&vals[0][b]))
                .unwrap()
        })
        .collect();
    let base = &per_enc_sse[0].1;
    for (name, s) in per_enc_sse.iter().skip(1) {
        let p0 = psnr(total(base) / n);
        let mut line = format!(
            "{},{},{},{},{},{:.6}",
            job.round,
            job.fmt,
            job.set,
            stem(&job.png),
            name,
            psnr(total(s) / n) - p0
        );
        for k in 0..CLASSES.len() {
            let hyb: f64 = (0..nb)
                .map(|b| if blk_class[b] == k { s[b] } else { base[b] } as f64)
                .sum();
            write!(line, ",{:.6}", psnr(hyb / n) - p0)?;
        }
        attrib.push(line);
    }
    Ok(ImgOut {
        rows,
        classes,
        attrib,
        mismatch_v0_ffi,
        mismatch_ext_sdc,
        blocks: (nb * chans.len()) as u64,
    })
}

fn jobs(root: &Path, rounds: &[String], sets: &Option<Vec<String>>) -> anyhow::Result<Vec<Job>> {
    // (round, images dir, out dir, fmt, set)
    let mut spec: Vec<(&'static str, &str, &str, &'static str, &str)> = vec![];
    for s in ["cutout_real", "alpha_real", "packed_real", "stray_real"] {
        spec.push(("1", "images", "out", "bc3", s));
        spec.push(("3", "images3", "out3/r1", "bc3", s));
    }
    spec.push(("2", "images2", "out2", "bc4", "mask_real"));
    spec.push(("2", "images2", "out2", "bc5", "normal_real"));
    spec.push(("3", "images3", "out3/r2", "bc4", "mask_real"));
    spec.push(("3", "images3", "out3/r2", "bc5", "normal_real"));
    let mut v = vec![];
    for (round, img, out, fmt, set) in spec {
        if !rounds.iter().any(|r| r == round)
            || sets.as_ref().is_some_and(|s| !s.iter().any(|x| x == set))
        {
            continue;
        }
        for png in set_files(root, img, set, "png")? {
            let ext = ext_names(fmt)
                .iter()
                .map(|n| {
                    (
                        n.to_string(),
                        root.join(out)
                            .join(n)
                            .join(set)
                            .join(format!("{}.dds", stem(&png))),
                    )
                })
                .filter(|(_, p)| p.exists())
                .collect();
            v.push(Job {
                round,
                fmt,
                set: set.into(),
                png,
                ext,
            });
        }
    }
    Ok(v)
}

fn main() -> anyhow::Result<()> {
    let args: Vec<String> = std::env::args().collect();
    let get = |k: &str| {
        args.iter()
            .position(|a| a == k)
            .map(|i| args[i + 1].clone())
    };
    let out = PathBuf::from(get("--out").expect("--out <dir>"));
    let oracle_on = args.iter().any(|a| a == "--oracle");
    let sets = get("--sets").map(|s| s.split(',').map(String::from).collect());
    let rounds: Vec<String> = get("--rounds")
        .unwrap_or("1,2,3".into())
        .split(',')
        .map(String::from)
        .collect();
    let root = bench_root();
    sdc_bench::radius::check_out(&root, &out)?;
    std::fs::create_dir_all(&out)?;

    // self-check: the Rust palette is the vendored one
    for e0 in 0..=255u8 {
        for e1 in 0..=255u8 {
            assert_eq!(
                pal(e0, e1),
                encoders_sys::bc4_block_values(e0, e1),
                "palette {e0} {e1}"
            );
        }
    }
    let jobs = jobs(&root, &rounds, &sets)?;
    eprintln!("{} images, oracle {oracle_on}", jobs.len());
    let t = Instant::now();
    let res: Vec<ImgOut> = jobs
        .par_iter()
        .map(|j| run(j, oracle_on))
        .collect::<anyhow::Result<_>>()?;
    eprintln!("done in {:.0}s", t.elapsed().as_secs_f64());
    let (mut r, mut c, mut a) = (
        String::from("round,fmt,set,image,variant,a_psnr,coverage_err_pct,c0_or_rg_psnr,ang_mean,us_per_mp_or_secs_per_mp,worse_blocks_vs_ffi\n"),
        String::from("round,fmt,set,image,variant,class,blocks,sse,mode8,mode6,mode6_inner\n"),
        String::from("round,fmt,set,image,variant,d_psnr_all,d_const,d_bin,d_both_inner,d_one_inner,d_noext\n"),
    );
    let (mut mm, mut me, mut nb) = (0u64, 0u64, 0u64);
    for x in &res {
        for l in &x.rows {
            r.push_str(l);
            r.push('\n');
        }
        for l in &x.classes {
            c.push_str(l);
            c.push('\n');
        }
        for l in &x.attrib {
            a.push_str(l);
            a.push('\n');
        }
        mm += x.mismatch_v0_ffi;
        me += x.mismatch_ext_sdc.min(u64::MAX / 2);
        nb += x.blocks;
    }
    std::fs::write(out.join("per_image.csv"), r)?;
    std::fs::write(out.join("classes.csv"), c)?;
    std::fs::write(out.join("attrib.csv"), a)?;
    println!("blocks {nb}; v0 (Rust port) != ffi: {mm}; ffi != engine DDS (sdc_*_rounded): {me}");
    Ok(())
}
