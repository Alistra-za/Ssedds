//! Spike S7: can `nvcompress` encode pre-built levels one at a time so the engine assembles the chain?
//! Only the user-installed NVTT is invoked (its licence forbids bundling); nothing of it is copied.
use crate::codec::decode_bc7_image;
use crate::corpus::*;
use crate::gates::GateReport;
use crate::{ddsraw, metrics};
use anyhow::{bail, Context, Result};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Instant;

/// Levels with at most this many texels are treated as "overhead only": the encode work on
/// them is negligible next to process start, PNG read and DDS write.
const SMALL_LEVEL_TEXELS: u32 = 32 * 32;
const OVERHEAD_LIMIT_S: f64 = 0.2;

pub fn nvcompress_path() -> PathBuf {
    PathBuf::from(r"C:\Program Files\NVIDIA Corporation\NVIDIA Texture Tools\nvcompress.exe")
}

/// 2x2 box filter on 8-bit texels, rounding half up; odd edges clamp.
pub fn box_down_rgba8(img: &Rgba8) -> Rgba8 {
    let (w, h) = ((img.width / 2).max(1), (img.height / 2).max(1));
    let mut px = Vec::with_capacity((w * h) as usize);
    for y in 0..h {
        for x in 0..w {
            let mut acc = [0u32; 4];
            for (dx, dy) in [(0, 0), (1, 0), (0, 1), (1, 1)] {
                let sx = (x * 2 + dx).min(img.width - 1);
                let sy = (y * 2 + dy).min(img.height - 1);
                let p = img.px[(sy * img.width + sx) as usize];
                for c in 0..4 {
                    acc[c] += p[c] as u32;
                }
            }
            px.push(acc.map(|a| ((a + 2) / 4) as u8));
        }
    }
    Rgba8 {
        width: w,
        height: h,
        px,
    }
}

fn save_png(img: &Rgba8, p: &Path) -> Result<()> {
    let flat: Vec<u8> = img.px.iter().flatten().copied().collect();
    image::save_buffer(p, &flat, img.width, img.height, image::ColorType::Rgba8)?;
    Ok(())
}

/// One measured level: wall seconds of the nvcompress process and RGB PSNR of its decode vs the level fed in.
struct LevelRow {
    level: usize,
    w: u32,
    h: u32,
    secs: f64,
    psnr: f64,
}

pub fn run(root: &Path, work: &Path) -> Result<GateReport> {
    let nv = nvcompress_path();
    let mut r = GateReport::new("S7 NVTT per-mip feeding (BC7 Production, -noalpha)");
    // nvcompress has no -h; with no arguments it prints its usage.
    let help = Command::new(&nv)
        .output()
        .with_context(|| format!("running {}", nv.display()))?;
    let help =
        String::from_utf8_lossy(&help.stdout).to_string() + &String::from_utf8_lossy(&help.stderr);
    for flag in [
        "-nomips",
        "-bc7",
        "-bc6",
        "-production",
        "-noalpha",
        "-silent",
    ] {
        r.check(
            help.lines()
                .any(|l| l.split_whitespace().next() == Some(flag)),
            format!("nvcompress usage lists {flag}"),
        );
    }
    r.info(format!(
        "nvcompress banner: {}",
        help.lines().next().unwrap_or("")
    ));
    let rows = read_results(&root.join("results/results.csv"), "nvtt_production")?;
    let mut all: Vec<LevelRow> = vec![];
    let mut max_dpsnr = 0.0f64;
    for src in set_files(root, "images", "rgb_real", "png")?
        .into_iter()
        .take(3)
    {
        let stem = stem(&src);
        let mut level = load_png_rgba8(&src)?;
        let mut chain_bytes = 0usize;
        for l in 0.. {
            let png = work.join(format!("{stem}_{l}.png"));
            let dds = work.join(format!("{stem}_{l}.dds"));
            save_png(&level, &png)?;
            let t = Instant::now();
            let out = Command::new(&nv)
                .args(["-silent", "-nomips", "-bc7", "-production", "-noalpha"])
                .arg(&png)
                .arg(&dds)
                .output()?;
            let secs = t.elapsed().as_secs_f64();
            if !out.status.success() {
                bail!(
                    "nvcompress failed on {}: {}",
                    png.display(),
                    String::from_utf8_lossy(&out.stderr)
                );
            }
            let bytes = std::fs::read(&dds)?;
            let payload = ddsraw::payload(&bytes)?;
            chain_bytes += payload.len();
            let dec = decode_bc7_image(payload, level.width, level.height);
            let a = engine::validate::alpha::alpha_stats(&dec);
            r.check(
                a.eq255 == a.pixels,
                format!(
                    "{stem} mip {l} ({}x{}): decoded alpha all 255",
                    level.width, level.height
                ),
            );
            let p = metrics::rgb_psnr(&level.px, &dec);
            if l == 0 {
                let e = find(&rows, "rgb_real", &stem)
                    .and_then(|row| row.f("rgb_psnr"))
                    .unwrap_or(f64::NAN);
                r.check(
                    (p - e).abs() <= 0.01,
                    format!(
                        "{stem} mip 0 RGB PSNR {p:.4} vs direct encode {e:.4} (delta {:+.4} dB)",
                        p - e
                    ),
                );
                max_dpsnr = max_dpsnr.max((p - e).abs());
            }
            all.push(LevelRow {
                level: l,
                w: level.width,
                h: level.height,
                secs,
                psnr: p,
            });
            if level.width == 1 && level.height == 1 {
                break;
            }
            level = box_down_rgba8(&level);
        }
        r.info(format!(
            "{stem}: assembled chain payload {chain_bytes} bytes"
        ));
    }
    r.info(format!(
        "mip 0 max |PSNR delta| vs direct nvtt_production: {max_dpsnr:.4} dB"
    ));
    // Per-level table across the images.
    let max_level = all.iter().map(|x| x.level).max().unwrap_or(0);
    for l in 0..=max_level {
        let at: Vec<&LevelRow> = all.iter().filter(|x| x.level == l).collect();
        if at.is_empty() {
            continue;
        }
        let secs: Vec<f64> = at.iter().map(|x| x.secs).collect();
        let psnr: Vec<f64> = at.iter().map(|x| x.psnr).collect();
        r.info(format!(
            "level {l:2} (e.g. {}x{}): mean {:.3} s, max {:.3} s, mean RGB PSNR {:.2} dB over {} images",
            at[0].w,
            at[0].h,
            metrics::mean(&secs),
            secs.iter().cloned().fold(0.0, f64::max),
            metrics::mean(&psnr),
            at.len()
        ));
    }
    let every: Vec<f64> = all.iter().map(|x| x.secs).collect();
    r.info(format!(
        "all {} levels: mean {:.3} s (dominated by mip 0 encode time, not overhead)",
        every.len(),
        metrics::mean(&every)
    ));
    let small: Vec<f64> = all
        .iter()
        .filter(|x| x.w * x.h <= SMALL_LEVEL_TEXELS)
        .map(|x| x.secs)
        .collect();
    let mean = metrics::mean(&small);
    let max = small.iter().cloned().fold(0.0, f64::max);
    r.check(
        !small.is_empty() && mean <= OVERHEAD_LIMIT_S,
        format!(
            "per-level overhead (levels <= {SMALL_LEVEL_TEXELS} texels, n={}): mean {mean:.3} s, max {max:.3} s (limit {OVERHEAD_LIMIT_S} s mean)",
            small.len()
        ),
    );
    Ok(r)
}
