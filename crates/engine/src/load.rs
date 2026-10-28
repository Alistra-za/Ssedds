//! Load PNG/TGA/TIFF/BMP/JPEG/WebP/OpenEXR/Radiance HDR, Photoshop PSD and PSB, and Krita/OpenRaster (their
//! `mergedimage.png`) into float RGBA.
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
    /// A single cube DDS (DECISIONS DS7): the image is its six mip-0 faces side by side, a 6:1 strip.
    pub dds_cube: bool,
}

/// Decode `path` to float RGBA. The dimension limit is checked from the header before the float pixel buffer is
/// decoded or allocated; a DDS input is first read whole into memory (its file bytes), then checked.
pub fn load_image(path: &Path) -> Result<(Rgba32F, SourceInfo), LoadError> {
    load_image_as(path, false)
}

/// `load_image`; `exr_straight` reads an OpenEXR with alpha as straight alpha (as stored) instead of
/// un-premultiplying it (`alpha.exr_straight`, Note BQ15).
pub fn load_image_as(path: &Path, exr_straight: bool) -> Result<(Rgba32F, SourceInfo), LoadError> {
    load_meta(path, exr_straight).map(|(img, info, _)| (img, info))
}

/// `load_image`, with the ICC flag and a DDS input's DXGI format. A `.dds` path is read as a DDS (mip 0).
pub fn load_image_meta(path: &Path) -> Result<(Rgba32F, SourceInfo, SourceMeta), LoadError> {
    load_meta(path, false)
}

fn load_meta(
    path: &Path,
    exr_straight: bool,
) -> Result<(Rgba32F, SourceInfo, SourceMeta), LoadError> {
    if path
        .extension()
        .is_some_and(|e| e.eq_ignore_ascii_case("dds"))
    {
        return load_dds(path);
    }
    // By content, like the `image` decoders below: a PSD reads as its merged image (`psd`).
    if crate::psd::sniff(path) {
        return load_psd(path);
    }
    // A Krita or OpenRaster archive reads as its `mergedimage.png` (`archive`).
    if crate::archive::sniff(path) {
        let bad = |msg: String| LoadError::Decode {
            path: path.to_path_buf(),
            msg,
        };
        // DECISIONS AF2: the IHDR first (MAX_DIM, `DIM-TOO-LARGE`), then an inflation capped by that size.
        let (w, h) = image_size(path)?;
        let png = crate::archive::merged_png_capped(path, crate::archive::merged_cap(w, h))
            .map_err(bad)?;
        let reader =
            image::ImageReader::with_format(std::io::Cursor::new(png), image::ImageFormat::Png);
        return decode(reader, path, exr_straight);
    }
    let reader = image::ImageReader::open(path)
        .and_then(image::ImageReader::with_guessed_format)
        .map_err(|e| LoadError::Decode {
            path: path.to_path_buf(),
            msg: e.to_string(),
        })?;
    decode(reader, path, exr_straight)
}

/// What the png crate may allocate besides the image (DECISIONS AV1): its line buffer and ancillary chunks, the
/// iCCP profile inflated among them. The decoded pixels do not count against it.
const PNG_METADATA_BUDGET: u64 = 64 << 20;

/// The explicit MAX_DIM header check bounds the image, so the crate's default 512 MiB allocation cap (which
/// rejects valid 16384 px images) is lifted. A PNG keeps a bound: `max_alloc` is also what its iCCP inflates
/// under (png 0.18 `decompress_to_vec_bounded`), parsed before any size is known (audit 2026-10-15 item 1).
pub(crate) fn lift_limits<R: std::io::BufRead + std::io::Seek>(reader: &mut image::ImageReader<R>) {
    let mut limits = image::Limits::no_limits();
    if reader.format() == Some(image::ImageFormat::Png) {
        limits.max_alloc = Some(PNG_METADATA_BUDGET);
    }
    reader.limits(limits);
}

/// Decode with the `image` decoder `reader` picked, MAX_DIM-checked from its header first.
/// An OpenEXR with alpha is un-premultiplied (OpenEXR stores premultiplied colour; trace-debug #30, Note BQ15):
/// RGB / A where A > 0, unclamped; A = 0 texels stay as stored. `exr_straight` skips it.
fn decode<R: std::io::BufRead + std::io::Seek>(
    mut reader: image::ImageReader<R>,
    path: &Path,
    exr_straight: bool,
) -> Result<(Rgba32F, SourceInfo, SourceMeta), LoadError> {
    let err = |e: image::ImageError| LoadError::Decode {
        path: path.to_path_buf(),
        msg: e.to_string(),
    };
    let exr = reader.format() == Some(image::ImageFormat::OpenExr);
    lift_limits(&mut reader);
    let mut decoder = reader.into_decoder().map_err(err)?;
    // Checked on the decoder chosen by content (the one that allocates), not one chosen by extension.
    let (w, h) = decoder.dimensions();
    if w.max(h) > MAX_DIM {
        return Err(LoadError::TooLarge {
            path: path.to_path_buf(),
            dim: w.max(h),
        });
    }
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
    let mut px: Vec<[f32; 4]> = img.to_rgba32f().pixels().map(|p| p.0).collect();
    if exr && info.has_alpha && !exr_straight {
        for p in px.iter_mut().filter(|p| p[3] > 0.0) {
            let a = p[3];
            for c in &mut p[..3] {
                *c /= a;
            }
        }
    }
    Ok((
        Rgba32F {
            width: img.width(),
            height: img.height(),
            px,
        },
        info,
        SourceMeta {
            icc_profile,
            ..Default::default()
        },
    ))
}

/// Width and height of `path` from its header alone, MAX_DIM-checked: a `.dds` from its DDS header, anything else
/// from the decoder its content picks (the one `load_image_meta` decodes with), never one its extension names.
pub fn image_size(path: &Path) -> Result<(u32, u32), LoadError> {
    let bad = |msg: String| LoadError::Decode {
        path: path.to_path_buf(),
        msg,
    };
    let (w, h) = if path
        .extension()
        .is_some_and(|e| e.eq_ignore_ascii_case("dds"))
    {
        dds_header_size(path).ok_or_else(|| bad("not a readable DDS header".into()))?
    } else if crate::psd::sniff(path) {
        crate::psd::size(path).map_err(|e| psd_error(path, e))?
    } else if crate::archive::sniff(path) {
        crate::archive::size(path).map_err(bad)?
    } else {
        let mut reader = image::ImageReader::open(path)
            .and_then(image::ImageReader::with_guessed_format)
            .map_err(|e| bad(e.to_string()))?;
        lift_limits(&mut reader);
        reader.into_dimensions().map_err(|e| bad(e.to_string()))?
    };
    if w.max(h) > MAX_DIM {
        return Err(LoadError::TooLarge {
            path: path.to_path_buf(),
            dim: w.max(h),
        });
    }
    Ok((w, h))
}

/// Width and height of a DDS from its first 20 bytes (the offsets `dds::read_dds` uses; that reader needs the
/// whole payload). `None` when the file cannot be read or is not a DDS. Unchecked against MAX_DIM.
pub(crate) fn dds_header_size(path: &Path) -> Option<(u32, u32)> {
    dds_header_facts(path).map(|(size, _)| size)
}

/// [`dds_header_size`] and whether the header says cube (caps2 or the DX10 misc flag, as `dds::read_dds` decides).
/// The size is the face size; a cube loads as a 6w x h strip (DECISIONS DS7).
pub(crate) fn dds_header_facts(path: &Path) -> Option<((u32, u32), bool)> {
    use std::io::Read;
    let mut b = [0u8; 144];
    let n = std::fs::File::open(path).ok()?.read(&mut b).ok()?;
    if n < 20 || &b[..4] != b"DDS " {
        return None;
    }
    let u = |i: usize| u32::from_le_bytes([b[i], b[i + 1], b[i + 2], b[i + 3]]);
    let dx10 = n >= 144 && &b[84..88] == b"DX10";
    let cube = (n >= 116 && u(112) & 0x200 != 0) || (dx10 && u(136) & 0x4 != 0);
    Some(((u(16), u(12)), cube))
}

fn psd_error(path: &Path, e: crate::psd::PsdError) -> LoadError {
    match e {
        crate::psd::PsdError::TooLarge(dim) => LoadError::TooLarge {
            path: path.to_path_buf(),
            dim,
        },
        crate::psd::PsdError::Bad(msg) => LoadError::Decode {
            path: path.to_path_buf(),
            msg,
        },
    }
}

/// A PSD or PSB input: its merged image, MAX_DIM-checked from the header before anything is allocated.
fn load_psd(path: &Path) -> Result<(Rgba32F, SourceInfo, SourceMeta), LoadError> {
    let (img, info, icc_profile) =
        crate::psd::load(path, MAX_DIM).map_err(|e| psd_error(path, e))?;
    Ok((
        img,
        info,
        SourceMeta {
            icc_profile,
            ..Default::default()
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
    if f.dx10.is_some_and(|d| d.array_size > 1) {
        return Err(bad(
            "a cubemap or texture array DDS cannot be re-encoded (2D and single cubes only)".into(),
        ));
    }
    if f.width.max(f.height) > MAX_DIM {
        return Err(LoadError::TooLarge {
            path: path.to_path_buf(),
            dim: f.width.max(f.height),
        });
    }
    let decode = |w, h, r: std::ops::Range<usize>| {
        crate::decode::decode_level_rgba(f.format, w, h, &f.data[r], None)
            .map_err(|e| bad(e.to_string()))
    };
    // A single cube (DECISIONS DS7) loads as a 6:1 strip of its mip-0 faces, face k at x = k * w.
    let (w, h, px) = if f.is_cubemap {
        if f.width != f.height {
            return Err(bad(format!(
                "cube faces are {}x{}, not square",
                f.width, f.height
            )));
        }
        let (n, s) = (f.levels.len(), f.surfaces());
        let (w, h) = (f.width, f.height);
        let mut px = vec![[0.0; 4]; 6 * (w * h) as usize];
        for k in 0..6 {
            let (_, _, r) = s[k * n].clone();
            for (i, p) in decode(w, h, r)?.into_iter().enumerate() {
                let (x, y) = (i as u32 % w, i as u32 / w);
                px[(y * 6 * w + k as u32 * w + x) as usize] = p;
            }
        }
        (6 * w, h, px)
    } else {
        let (w, h, r) = f.levels[0].clone();
        (w, h, decode(w, h, r)?)
    };
    Ok((
        Rgba32F {
            width: w,
            height: h,
            px,
        },
        dds_source_info(f.format),
        SourceMeta {
            dds_dxgi: Some(f.format.dxgi()),
            dds_cube: f.is_cubemap,
            ..Default::default()
        },
    ))
}
