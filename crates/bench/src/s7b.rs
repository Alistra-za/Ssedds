//! Spike S7b (M5 spec §8): does `nvcompress` take the engine's own uncompressed DDS levels, and does BC6H Production
//! work one level at a time down to 1x1? Only the user-installed NVTT is invoked; nothing of it is copied.
use crate::codec::{decode_bc6h_image, decode_bc7_image, half_to_rgb_f32};
use crate::corpus::*;
use crate::gates::GateReport;
use crate::s7::nvcompress_path;
use crate::{ddsraw, metrics};
use anyhow::{bail, Context, Result};
use engine::config::Format;
use engine::dds::{dds_bytes, DdsDesc, ResolvedHeader};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Instant;

const SMALL_LEVEL_TEXELS: u32 = 32 * 32;
/// Spec B2 §6: BC6H must keep at least this share of the source peak luminance at mip 0.
const MIN_PEAK_RATIO: f64 = 0.95;
/// `DXGI_FORMAT_BC6H_UF16`.
const DXGI_BC6H_UF16: u32 = 95;

/// 2x2 box filter on half texels in f32; odd edges clamp.
pub fn box_down_rgba16f(img: &HalfImage) -> HalfImage {
    let (w, h) = ((img.width / 2).max(1), (img.height / 2).max(1));
    let mut px = Vec::with_capacity((w * h) as usize);
    for y in 0..h {
        for x in 0..w {
            let mut acc = [0f32; 4];
            for (dx, dy) in [(0, 0), (1, 0), (0, 1), (1, 1)] {
                let sx = (x * 2 + dx).min(img.width - 1);
                let sy = (y * 2 + dy).min(img.height - 1);
                let p = img.px[(sy * img.width + sx) as usize];
                for c in 0..4 {
                    acc[c] += half::f16::from_bits(p[c]).to_f32();
                }
            }
            px.push(acc.map(|a| half::f16::from_f32(a / 4.0).to_bits()));
        }
    }
    HalfImage {
        width: w,
        height: h,
        px,
    }
}

fn dds_level(
    w: u32,
    h: u32,
    format: Format,
    header: ResolvedHeader,
    texels: Vec<u8>,
) -> Result<Vec<u8>> {
    let desc = DdsDesc {
        width: w,
        height: h,
        format,
        srgb: false,
        header,
    };
    Ok(dds_bytes(&desc, &[texels])?)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum HdrFeed {
    /// RGBA16F with a DX10 header (DXGI 10), as the engine writes it.
    Dds,
    /// RGBA16F with a legacy header, FourCC 113 (`D3DFMT_A16B16G16R16F`); the engine's writer has no such form.
    DdsLegacy,
    Exr,
    Hdr,
}

/// Writes `img` as `<base>.<ext>` in the given feed format and returns the path.
fn write_hdr_feed(img: &HalfImage, feed: HdrFeed, base: &Path) -> Result<PathBuf> {
    let f32s = |n: usize| -> Vec<u8> {
        img.px
            .iter()
            .flat_map(|p| p[..n].iter().map(|h| half::f16::from_bits(*h).to_f32()))
            .flat_map(f32::to_le_bytes)
            .collect()
    };
    let (w, h) = (img.width, img.height);
    let path = base.with_extension(match feed {
        HdrFeed::Dds => "in.dds",
        HdrFeed::DdsLegacy => "l113.dds",
        HdrFeed::Exr => "exr",
        HdrFeed::Hdr => "hdr",
    });
    let dx10 = || {
        let texels = img
            .px
            .iter()
            .flat_map(|p| p.iter().flat_map(|h| h.to_le_bytes()))
            .collect();
        dds_level(w, h, Format::Rgba16f, ResolvedHeader::Dx10, texels)
    };
    match feed {
        HdrFeed::Dds => std::fs::write(&path, dx10()?)?,
        HdrFeed::DdsLegacy => {
            let dx10 = dx10()?;
            let mut b = dx10[..128].to_vec();
            b[80..84].copy_from_slice(&4u32.to_le_bytes()); // DDPF_FOURCC only
            b[84..88].copy_from_slice(&113u32.to_le_bytes());
            b.extend_from_slice(&dx10[148..]);
            std::fs::write(&path, b)?;
        }
        HdrFeed::Exr => image::save_buffer(&path, &f32s(4), w, h, image::ColorType::Rgba32F)?,
        HdrFeed::Hdr => image::save_buffer(&path, &f32s(3), w, h, image::ColorType::Rgb32F)?,
    }
    Ok(path)
}

/// Runs nvcompress; returns (output file bytes, wall seconds).
fn nvcompress(nv: &Path, flags: &[&str], input: &Path, output: &Path) -> Result<(Vec<u8>, f64)> {
    let t = Instant::now();
    let out = Command::new(nv)
        .args(flags)
        .arg(input)
        .arg(output)
        .output()?;
    let secs = t.elapsed().as_secs_f64();
    if !out.status.success() {
        bail!(
            "nvcompress {flags:?} failed on {} ({}): {}",
            input.display(),
            out.status,
            String::from_utf8_lossy(&out.stderr)
        );
    }
    Ok((std::fs::read(output)?, secs))
}

/// Largest Rec. 709 luminance (the bench's peak_ratio numerator and denominator).
pub fn peak(px: &[[f32; 3]]) -> f32 {
    px.iter()
        .map(|c| 0.2126 * c[0] + 0.7152 * c[1] + 0.0722 * c[2])
        .fold(0.0f32, f32::max)
}

struct Bc6Level {
    w: u32,
    h: u32,
    payload: Vec<u8>,
    secs: f64,
}

pub fn run(root: &Path, work: &Path) -> Result<GateReport> {
    let nv = nvcompress_path();
    let mut r = GateReport::new("S7b NVTT DDS input and BC6H per level");

    // 1. Banner.
    let o = Command::new(&nv)
        .output()
        .with_context(|| format!("running {}", nv.display()))?;
    let text = String::from_utf8_lossy(&o.stdout).to_string() + &String::from_utf8_lossy(&o.stderr);
    let banner = text.lines().next().unwrap_or("");
    let version = banner
        .strip_prefix("NVIDIA Texture Tools ")
        .and_then(|s| s.split_whitespace().next())
        .filter(|v| v.split('.').count() == 3 && v.split('.').all(|n| n.parse::<u32>().is_ok()));
    r.check(
        version.is_some(),
        format!("banner {banner:?} -> version {version:?}"),
    );
    r.info(format!("no-argument exit status: {}", o.status));

    // 2. DDS input, BC7: B8G8R8A8 DDS (DX10, then legacy header) vs PNG of the same level.
    let src = set_files(root, "images", "rgb_real", "png")?
        .into_iter()
        .next()
        .context("no rgb_real image")?;
    let s = stem(&src);
    let img = load_png_rgba8(&src)?;
    let png = work.join(format!("{s}.png"));
    let flat: Vec<u8> = img.px.iter().flatten().copied().collect();
    image::save_buffer(&png, &flat, img.width, img.height, image::ColorType::Rgba8)?;
    let bc7 = [
        "-silent",
        "-nomips",
        "-bc7",
        "-production",
        "-noalpha",
        "-nocuda",
    ];
    let (a, ta) = nvcompress(&nv, &bc7, &png, &work.join(format!("{s}.png.out.dds")))?;
    let pa = ddsraw::payload(&a)?;
    r.info(format!(
        "{s} ({}x{}) BC7 from PNG: {ta:.2} s, RGB PSNR {:.4}",
        img.width,
        img.height,
        metrics::rgb_psnr(&img.px, &decode_bc7_image(pa, img.width, img.height))
    ));
    let mut dds_ok = false;
    for header in [ResolvedHeader::Dx10, ResolvedHeader::Legacy] {
        let bgra: Vec<u8> = img
            .px
            .iter()
            .flat_map(|p| [p[2], p[1], p[0], p[3]])
            .collect();
        let dds_in = work.join(format!("{s}.{header:?}.in.dds"));
        std::fs::write(
            &dds_in,
            dds_level(img.width, img.height, Format::Rgba8, header, bgra)?,
        )?;
        let (b, tb) = nvcompress(
            &nv,
            &bc7,
            &dds_in,
            &work.join(format!("{s}.{header:?}.out.dds")),
        )?;
        let pb = ddsraw::payload(&b)?;
        let same = pa == pb;
        dds_ok |= same;
        r.info(format!(
            "BC7 from B8G8R8A8 DDS ({header:?} header) byte-identical to PNG feed: {same} ({tb:.2} s; RGB PSNR {:.4})",
            metrics::rgb_psnr(&img.px, &decode_bc7_image(pb, img.width, img.height))
        ));
    }
    r.info(format!(
        "NV5 BC7 feed: {}",
        if dds_ok {
            "DDS"
        } else {
            "PNG (no DDS form matched)"
        }
    ));

    // 3. Which HDR feed nvcompress reads correctly: mip 0 of the first image as DX10 RGBA16F DDS (DX10, then legacy), EXR (f32), HDR (RGBE,
    // lossy). The first that keeps the peak wins, so lossless feeds are tried first.
    let rows = read_results(&root.join("results/results2.csv"), "bc6_nvtt_production")?;
    let hdr_files: Vec<_> = set_files(root, "images2", "hdr_real", "f16")?
        .into_iter()
        .take(3)
        .collect();
    let bc6 = ["-silent", "-nomips", "-bc6", "-production"];
    let first = load_f16(hdr_files.first().context("no hdr_real image")?)?;
    let mut feed = None;
    for f in [HdrFeed::Dds, HdrFeed::DdsLegacy, HdrFeed::Exr, HdrFeed::Hdr] {
        let input = write_hdr_feed(&first, f, &work.join("probe"))?;
        let out = work.join(format!("probe.{f:?}.out.dds"));
        let Ok((bytes, _)) = nvcompress(&nv, &bc6, &input, &out) else {
            r.info(format!("BC6H feed {f:?}: nvcompress failed"));
            continue;
        };
        let dec = decode_bc6h_image(ddsraw::payload(&bytes)?, first.width, first.height);
        let reference = half_to_rgb_f32(&first);
        let ratio = (peak(&dec) / peak(&reference)) as f64;
        let m = metrics::mpsnr(&reference, &dec);
        r.info(format!(
            "BC6H feed {f:?}: mip 0 peak ratio {ratio:.4}, mPSNR {m:.3} dB"
        ));
        if feed.is_none() && ratio >= MIN_PEAK_RATIO {
            feed = Some(f);
        }
    }
    r.check(feed.is_some(), format!("NV5 BC6H feed: {feed:?}"));
    let Some(feed) = feed else { return Ok(r) };

    // 3./4. BC6H per level on that feed, with CUDA (default) and with -nocuda.
    let bc6_nocuda = ["-silent", "-nomips", "-bc6", "-production", "-nocuda"];
    let (mut small_cuda, mut small_cpu, mut identical, mut total) = (vec![], vec![], 0, 0);
    let (mut sum_cuda, mut sum_cpu) = (0.0, 0.0);
    for src in &hdr_files {
        let s = stem(src);
        let mut level = load_f16(src)?;
        let mut cuda: Vec<Bc6Level> = vec![];
        let mut src_levels = vec![];
        for l in 0.. {
            let input = write_hdr_feed(&level, feed, &work.join(format!("{s}_{l}")))?;
            let (bytes, secs) =
                nvcompress(&nv, &bc6, &input, &work.join(format!("{s}_{l}.cuda.dds")))?;
            let payload = ddsraw::payload(&bytes)?.to_vec();
            let want = (level.width.div_ceil(4) * level.height.div_ceil(4) * 16) as usize;
            let dxgi = (&bytes[84..88] == b"DX10")
                .then(|| u32::from_le_bytes(bytes[128..132].try_into().unwrap()));
            r.check(
                payload.len() == want && dxgi == Some(DXGI_BC6H_UF16),
                format!(
                    "{s} mip {l} ({}x{}): payload {} bytes (want {want}), DXGI {dxgi:?} (want {DXGI_BC6H_UF16} UF16), {secs:.3} s",
                    level.width,
                    level.height,
                    payload.len()
                ),
            );
            if payload.len() == want {
                let reference = half_to_rgb_f32(&level);
                let dec = decode_bc6h_image(&payload, level.width, level.height);
                let m = metrics::mpsnr(&reference, &dec);
                if l == 0 {
                    let ratio = (peak(&dec) / peak(&reference)) as f64;
                    r.check(
                        ratio >= MIN_PEAK_RATIO,
                        format!("{s} mip 0 peak ratio {ratio:.4} (>= {MIN_PEAK_RATIO})"),
                    );
                    let e = find(&rows, "hdr_real", &s)
                        .and_then(|row| row.f("mpsnr"))
                        .unwrap_or(f64::NAN);
                    r.check(
                        (m - e).abs() <= 0.01,
                        format!("{s} mip 0 mPSNR {m:.4} vs direct bc6_nvtt_production {e:.4}"),
                    );
                } else {
                    r.info(format!("{s} mip {l}: mPSNR {m:.3} dB"));
                }
            }
            sum_cuda += secs;
            cuda.push(Bc6Level {
                w: level.width,
                h: level.height,
                payload,
                secs,
            });
            src_levels.push(input);
            if level.width == 1 && level.height == 1 {
                break;
            }
            level = box_down_rgba16f(&level);
        }
        for (l, (c, input)) in cuda.iter().zip(&src_levels).enumerate() {
            let (bytes, secs) = nvcompress(
                &nv,
                &bc6_nocuda,
                input,
                &work.join(format!("{s}_{l}.cpu.dds")),
            )?;
            let same = ddsraw::payload(&bytes)? == c.payload.as_slice();
            identical += same as usize;
            total += 1;
            sum_cpu += secs;
            if !same {
                r.info(format!(
                    "{s} mip {l} ({}x{}): -nocuda payload DIFFERS from CUDA",
                    c.w, c.h
                ));
            }
            if l == 0 {
                r.info(format!(
                    "{s} mip 0 ({}x{}): CUDA {:.3} s, -nocuda {secs:.3} s",
                    c.w, c.h, c.secs
                ));
            }
            if c.w * c.h <= SMALL_LEVEL_TEXELS {
                small_cuda.push(c.secs);
                small_cpu.push(secs);
            }
        }
    }
    r.info(format!(
        "BC6H -nocuda byte-identical to CUDA on {identical}/{total} levels"
    ));
    r.info(format!(
        "BC6H whole chains: CUDA {sum_cuda:.2} s, -nocuda {sum_cpu:.2} s"
    ));
    r.info(format!(
        "BC6H levels <= {SMALL_LEVEL_TEXELS} texels (n={}): mean CUDA {:.3} s, mean -nocuda {:.3} s",
        small_cuda.len(),
        metrics::mean(&small_cuda),
        metrics::mean(&small_cpu)
    ));

    // 5. Exit status on a truncated input.
    let bad = work.join("truncated.dds");
    std::fs::write(&bad, &std::fs::read(&png)?[..100])?;
    let o = Command::new(&nv)
        .args(bc7)
        .arg(&bad)
        .arg(work.join("truncated.out.dds"))
        .output()?;
    let tail = String::from_utf8_lossy(&o.stdout).to_string() + &String::from_utf8_lossy(&o.stderr);
    r.check(
        !o.status.success(),
        format!(
            "truncated input exits non-zero: {} ({:?})",
            o.status,
            tail.trim()
        ),
    );
    r.info(format!(
        "output written on truncated input: {}",
        work.join("truncated.out.dds").exists()
    ));
    Ok(r)
}
