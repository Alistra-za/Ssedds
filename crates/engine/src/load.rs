//! Load PNG/TGA/TIFF/BMP/JPEG/OpenEXR/Radiance HDR into float RGBA.
//!
//! Values are stored exactly as decoded (`k / 255.0` for 8-bit, `k / 65535.0` for 16-bit);
//! no colour-space conversion happens here, the sRGB decode is done later in preprocess.
//! Embedded ICC profiles are detected and ignored (DECISIONS S8).
use crate::image::{Rgba32F, SourceInfo};
use image::ImageDecoder;
use std::path::{Path, PathBuf};

/// Largest accepted width or height (D3D11 texture limit).
pub const MAX_DIM: u32 = 16384;

#[derive(Debug, thiserror::Error)]
pub enum LoadError {
    #[error("{}: dimension {dim} exceeds {MAX_DIM} (D3D11 limit)", path.display())]
    TooLarge { path: PathBuf, dim: u32 },
    #[error("{}: {msg}", path.display())]
    Decode { path: PathBuf, msg: String },
}

/// What `load_image_meta` learns besides the pixels.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct SourceMeta {
    /// The file embeds a non-empty ICC profile; values are still read as stored (DECISIONS S8, `IN-ICC-IGNORED`).
    pub icc_profile: bool,
    /// DXGI format of a DDS input (DECISIONS S6); `None` for images.
    pub dds_dxgi: Option<u32>,
}

/// Decode `path` to float RGBA. The dimension limit is checked from the header before the full pixel buffer is
/// decoded or allocated.
pub fn load_image(path: &Path) -> Result<(Rgba32F, SourceInfo), LoadError> {
    load_image_meta(path).map(|(img, info, _)| (img, info))
}

/// `load_image`, with the ICC flag and a DDS input's DXGI format. A `.dds` path is read as a DDS (mip 0).
pub fn load_image_meta(path: &Path) -> Result<(Rgba32F, SourceInfo, SourceMeta), LoadError> {
    if path
        .extension()
        .is_some_and(|e| e.eq_ignore_ascii_case("dds"))
    {
        return load_dds(path);
    }
    let err = |e: image::ImageError| LoadError::Decode {
        path: path.to_path_buf(),
        msg: e.to_string(),
    };
    let (w, h) = image::image_dimensions(path).map_err(err)?;
    if w.max(h) > MAX_DIM {
        return Err(LoadError::TooLarge {
            path: path.to_path_buf(),
            dim: w.max(h),
        });
    }
    // The explicit MAX_DIM header check bounds the size, so the crate's default
    // 512 MiB allocation cap (which rejects valid 16384 px images) is lifted.
    let mut reader = image::ImageReader::open(path)
        .and_then(image::ImageReader::with_guessed_format)
        .map_err(|e| LoadError::Decode {
            path: path.to_path_buf(),
            msg: e.to_string(),
        })?;
    reader.no_limits();
    let mut decoder = reader.into_decoder().map_err(err)?;
    // A decoder that cannot read a profile, or finds none, is not an error: the profile is ignored either way.
    let icc_profile = decoder
        .icc_profile()
        .ok()
        .flatten()
        .is_some_and(|p| !p.is_empty());
    let img = image::DynamicImage::from_decoder(decoder).map_err(err)?;
    let ct = img.color();
    let channels = ct.channel_count();
    let info = SourceInfo {
        bit_depth: (ct.bits_per_pixel() / u16::from(channels)) as u8,
        channels,
        has_alpha: ct.has_alpha(),
        is_float: matches!(ct, image::ColorType::Rgb32F | image::ColorType::Rgba32F),
    };
    let px = img.to_rgba32f().pixels().map(|p| p.0).collect();
    Ok((
        Rgba32F {
            width: img.width(),
            height: img.height(),
            px,
        },
        info,
        SourceMeta {
            icc_profile,
            dds_dxgi: None,
        },
    ))
}

/// `SourceInfo` of a DDS input's pixel format (DECISIONS S6).
fn dds_source_info(f: crate::dds::DdsPixelFormat) -> SourceInfo {
    use crate::dds::DdsPixelFormat::*;
    let (bit_depth, channels, has_alpha, is_float) = match f {
        Bc1 { .. } | Bc2 { .. } | Bc3 { .. } | Bc7 { .. } | Rgba8 { .. } | Bgra8 { .. } => {
            (8, 4, true, false)
        }
        Bgrx8 => (8, 3, false, false),
        Bc4 | R8 => (8, 1, false, false),
        Bc5 | Rg8 => (8, 2, false, false),
        Bc6hUf16 | Bc6hSf16 => (16, 3, false, true),
        Rgba16f => (16, 4, true, true),
        Rgba32f => (32, 4, true, true),
    };
    SourceInfo {
        bit_depth,
        channels,
        has_alpha,
        is_float,
    }
}

/// A DDS input (DECISIONS S6): mip 0 decoded to float RGBA; every other mip is regenerated. 2D only.
fn load_dds(path: &Path) -> Result<(Rgba32F, SourceInfo, SourceMeta), LoadError> {
    let bad = |msg: String| LoadError::Decode {
        path: path.to_path_buf(),
        msg,
    };
    let bytes = std::fs::read(path).map_err(|e| bad(e.to_string()))?;
    let f = crate::dds::read_dds(&bytes).map_err(|e| bad(e.to_string()))?;
    drop(bytes);
    if f.is_cubemap || f.dx10.is_some_and(|d| d.array_size > 1) {
        return Err(bad(
            "a cubemap or array DDS cannot be re-encoded (2D only)".into()
        ));
    }
    if f.width.max(f.height) > MAX_DIM {
        return Err(LoadError::TooLarge {
            path: path.to_path_buf(),
            dim: f.width.max(f.height),
        });
    }
    let (w, h, r) = f.levels[0].clone();
    let px = crate::decode::decode_level_rgba(f.format, w, h, &f.data[r], None)
        .map_err(|e| bad(e.to_string()))?;
    Ok((
        Rgba32F {
            width: w,
            height: h,
            px,
        },
        dds_source_info(f.format),
        SourceMeta {
            icc_profile: false,
            dds_dxgi: Some(f.format.dxgi()),
        },
    ))
}
