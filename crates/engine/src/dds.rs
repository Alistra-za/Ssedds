//! DDS container (spec §5.7): the engine's own writer, and the reader used by Validate and `sdc validate`.
use crate::config::{Format, HeaderForm};
use std::io::Write;
use std::path::Path;

pub const DDSD_CAPS: u32 = 0x1;
pub const DDSD_HEIGHT: u32 = 0x2;
pub const DDSD_WIDTH: u32 = 0x4;
pub const DDSD_PIXELFORMAT: u32 = 0x1000;
pub const DDSD_MIPMAPCOUNT: u32 = 0x2_0000;
pub const DDSD_LINEARSIZE: u32 = 0x8_0000;
pub const DDPF_FOURCC: u32 = 0x4;
/// Non-standard flag some tools set for normal maps. Defined so tests and the reader can assert it is absent;
/// the writer never sets it (spec §5.7).
pub const DDPF_NORMAL: u32 = 0x8000_0000;
pub const DDSCAPS_COMPLEX: u32 = 0x8;
pub const DDSCAPS_TEXTURE: u32 = 0x1000;
pub const DDSCAPS_MIPMAP: u32 = 0x40_0000;

/// `D3D10_RESOURCE_DIMENSION_TEXTURE2D`.
const DIMENSION_TEXTURE2D: u32 = 3;

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "lowercase")]
pub enum ResolvedHeader {
    Dx10,
    Legacy,
}

/// Legacy FourCC for `format`. `ATI1`/`ATI2` are never written; BC4/BC5 use `BC4U`/`BC5U`.
pub fn legacy_fourcc(format: Format) -> Option<[u8; 4]> {
    match format {
        Format::Bc1 => Some(*b"DXT1"),
        Format::Bc3 => Some(*b"DXT5"),
        Format::Bc4 => Some(*b"BC4U"),
        Format::Bc5 => Some(*b"BC5U"),
        Format::Bc6hUf16 | Format::Bc7 => None,
    }
}

/// Header form for a request. `Auto` picks legacy only for BC1/BC3 UNORM.
///
/// An explicit `legacy` on a format with no legacy form (BC6H, BC7, any `_SRGB`) maps to DX10 here as a defensive
/// fallback only; Resolve rejects such requests as a user error (`CFG-INVALID`) before this is reached.
pub fn resolve_header(format: Format, srgb: bool, h: HeaderForm) -> ResolvedHeader {
    let legacy_ok = !srgb && legacy_fourcc(format).is_some();
    match h {
        HeaderForm::Auto if !srgb && matches!(format, Format::Bc1 | Format::Bc3) => {
            ResolvedHeader::Legacy
        }
        HeaderForm::Legacy if legacy_ok => ResolvedHeader::Legacy,
        _ => ResolvedHeader::Dx10,
    }
}

pub fn dxgi_format(format: Format, srgb: bool) -> u32 {
    match (format, srgb) {
        (Format::Bc1, false) => 71,
        (Format::Bc1, true) => 72,
        (Format::Bc3, false) => 77,
        (Format::Bc3, true) => 78,
        (Format::Bc4, _) => 80,
        (Format::Bc5, _) => 83,
        (Format::Bc6hUf16, _) => 95,
        (Format::Bc7, false) => 98,
        (Format::Bc7, true) => 99,
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DdsDesc {
    pub width: u32,
    pub height: u32,
    pub format: Format,
    pub srgb: bool,
    pub header: ResolvedHeader,
}

/// Bytes in one mip level: `max(1, ceil(w/4)) * max(1, ceil(h/4)) * block_bytes`.
pub fn level_size(format: Format, w: u32, h: u32) -> usize {
    (w.div_ceil(4).max(1) as usize) * (h.div_ceil(4).max(1) as usize) * format.block_bytes()
}

/// Serialise a DDS file: magic, 124-byte header, optional DX10 block, then `levels` (mip 0 first) back to back.
pub fn dds_bytes(desc: &DdsDesc, levels: &[Vec<u8>]) -> Vec<u8> {
    let mips = levels.len() as u32;
    let mut h = [0u32; 31];
    h[0] = 124;
    h[1] = DDSD_CAPS
        | DDSD_HEIGHT
        | DDSD_WIDTH
        | DDSD_PIXELFORMAT
        | DDSD_LINEARSIZE
        | if mips > 1 { DDSD_MIPMAPCOUNT } else { 0 };
    h[2] = desc.height;
    h[3] = desc.width;
    h[4] = level_size(desc.format, desc.width, desc.height) as u32;
    h[5] = 1; // depth: texconv writes 1 for 2D textures; not a reserved field
    h[6] = mips;
    h[18] = 32;
    h[19] = DDPF_FOURCC;
    h[20] = u32::from_le_bytes(match desc.header {
        ResolvedHeader::Legacy => {
            legacy_fourcc(desc.format).expect("resolve_header guarantees a legacy form")
        }
        ResolvedHeader::Dx10 => *b"DX10",
    });
    h[26] = DDSCAPS_TEXTURE
        | if mips > 1 {
            DDSCAPS_COMPLEX | DDSCAPS_MIPMAP
        } else {
            0
        };
    let mut out = Vec::with_capacity(148 + levels.iter().map(Vec::len).sum::<usize>());
    out.extend_from_slice(b"DDS ");
    for v in h {
        out.extend_from_slice(&v.to_le_bytes());
    }
    if desc.header == ResolvedHeader::Dx10 {
        for v in [
            dxgi_format(desc.format, desc.srgb),
            DIMENSION_TEXTURE2D,
            0,
            1,
            0,
        ] {
            out.extend_from_slice(&v.to_le_bytes());
        }
    }
    for l in levels {
        out.extend_from_slice(l);
    }
    out
}

/// Write `bytes` to `path` via a temp file in the same directory, then rename over any existing file.
/// The temp file is removed on error (and by `NamedTempFile`'s drop).
pub fn write_atomic(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    let dir = path
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    std::fs::create_dir_all(dir)?;
    let mut tmp = tempfile::Builder::new()
        .prefix(".sdc-")
        .suffix(".tmp")
        .tempfile_in(dir)?;
    tmp.write_all(bytes)?;
    tmp.as_file().sync_all()?;
    tmp.persist(path).map_err(|e| e.error)?;
    Ok(())
}
