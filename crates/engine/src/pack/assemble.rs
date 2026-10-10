//! The packed float image of a recipe (spec §6.4 "Sources", "Dimensions", "Precision"). Each source contributes
//! its R channel (luma with `channels.grayscale_source = luma`); its alpha is ignored. Values stay as decoded
//! floats, so 16-bit sources keep their precision until Quantise. Sources are decoded **one at a time**: the peak
//! is the output plus one decoded source, inside the 3x per-file estimate of `api::estimated_bytes`, which reads
//! the first source (every source has its size).
use super::{ChannelSource, PackJob};
use crate::analyse::code8;
use crate::config::GraySource;
use crate::image::{Rgba32F, SourceInfo};
use crate::load::{load_image, LoadError};
use std::path::PathBuf;

#[derive(Debug, thiserror::Error)]
pub enum PackError {
    #[error("{0}")]
    Load(#[from] LoadError),
    #[error("sources differ in size: {}", sizes(.0))]
    SizeMismatch(Vec<(PathBuf, u32, u32)>),
}

/// `"<file> <w>x<h>, …"`; `pub(crate)` so `pack::plan` words its size-mismatch reason the same way.
pub(crate) fn sizes(s: &[(PathBuf, u32, u32)]) -> String {
    s.iter()
        .map(|(p, w, h)| {
            format!(
                "{} {w}x{h}",
                p.file_name().unwrap_or_default().to_string_lossy()
            )
        })
        .collect::<Vec<_>>()
        .join(", ")
}

pub struct Assembled {
    pub image: Rgba32F,
    /// Largest bit depth, any float source, 4 channels; `has_alpha` unless the alpha channel is `Opaque`.
    pub info: SourceInfo,
    /// Sources whose R, G and B differ by more than 4/255 somewhere (`CHAN-NOT-GRAY`).
    pub non_gray: Vec<PathBuf>,
}

/// The width and height every source shares, from the file headers alone.
pub fn check_sizes(sources: &[PathBuf]) -> Result<(u32, u32), PackError> {
    let mut found = vec![];
    for p in sources {
        let (w, h) = image::image_dimensions(p).map_err(|e| LoadError::Decode {
            path: p.clone(),
            msg: e.to_string(),
        })?;
        found.push((p.clone(), w, h));
    }
    let (w, h) = found.first().map_or((0, 0), |f| (f.1, f.2));
    if found.iter().any(|f| (f.1, f.2) != (w, h)) {
        return Err(PackError::SizeMismatch(found));
    }
    Ok((w, h))
}

/// The analysis' grey test (`analyse.rs`): the whole R/G/B spread within 4 codes (same `code8` rounding).
fn grey(q: &[f32; 4]) -> bool {
    let c = |v: f32| i32::from(code8(v));
    let (r, g, b) = (c(q[0]), c(q[1]), c(q[2]));
    r.max(g).max(b) - r.min(g).min(b) <= 4
}

/// Builds the packed image of `job`.
pub fn assemble(job: &PackJob, gray: GraySource) -> Result<Assembled, PackError> {
    let paths: Vec<PathBuf> = job.sources.iter().map(|s| s.path.clone()).collect();
    let (w, h) = check_sizes(&paths)?;
    let mut image = Rgba32F::new(w, h, [0.0, 0.0, 0.0, 1.0]);
    let mut info = SourceInfo {
        bit_depth: 8,
        channels: 4,
        has_alpha: !matches!(job.channels[3], ChannelSource::Opaque),
        is_float: false,
    };
    for (c, ch) in job.channels.iter().enumerate() {
        let value = match ch {
            ChannelSource::Default { value } => Some(*value),
            ChannelSource::Fill { code, .. } => Some(f32::from(*code) / 255.0),
            ChannelSource::Opaque => Some(1.0),
            ChannelSource::Source { .. } | ChannelSource::MirrorR => None,
        };
        if let Some(v) = value {
            for p in &mut image.px {
                p[c] = v;
            }
        }
    }
    let mut non_gray = vec![];
    for s in &job.sources {
        let targets: Vec<(usize, bool)> = job
            .channels
            .iter()
            .enumerate()
            .filter_map(|(c, ch)| match ch {
                ChannelSource::Source { path, invert, .. } if *path == s.path => Some((c, *invert)),
                _ => None,
            })
            .collect();
        let (src, si) = load_image(&s.path)?;
        // `check_sizes` read the headers; a file replaced since then is caught here.
        if (src.width, src.height) != (w, h) {
            return Err(PackError::SizeMismatch(vec![
                (paths[0].clone(), w, h),
                (s.path.clone(), src.width, src.height),
            ]));
        }
        info.bit_depth = info.bit_depth.max(si.bit_depth);
        info.is_float |= si.is_float;
        let mut all_grey = true;
        for (o, q) in image.px.iter_mut().zip(&src.px) {
            all_grey &= grey(q);
            let v = match gray {
                GraySource::R => q[0],
                GraySource::Luma => 0.2126 * q[0] + 0.7152 * q[1] + 0.0722 * q[2],
            };
            for &(c, invert) in &targets {
                o[c] = if invert { 1.0 - v } else { v };
            }
        }
        if !all_grey {
            non_gray.push(s.path.clone());
        }
    }
    let mirrored: Vec<usize> = (0..4)
        .filter(|&c| matches!(job.channels[c], ChannelSource::MirrorR))
        .collect();
    for p in &mut image.px {
        for &c in &mirrored {
            p[c] = p[0];
        }
    }
    Ok(Assembled {
        image,
        info,
        non_gray,
    })
}
