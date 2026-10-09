//! Load PNG/TGA/TIFF (8/16-bit) into float RGBA.
//!
//! Values are stored exactly as decoded (`k / 255.0` for 8-bit, `k / 65535.0` for 16-bit);
//! no colour-space conversion happens here, the sRGB decode is done later in preprocess.
//! Embedded ICC profiles are ignored by the `image` crate; the info event the spec (§5.1)
//! asks for is tracked under milestone M6.
use crate::image::{Rgba32F, SourceInfo};
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

/// Decode `path` to float RGBA. The dimension limit is checked from the header
/// before the full pixel buffer is decoded or allocated.
pub fn load_image(path: &Path) -> Result<(Rgba32F, SourceInfo), LoadError> {
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
    let img = reader.decode().map_err(err)?;
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
    ))
}
