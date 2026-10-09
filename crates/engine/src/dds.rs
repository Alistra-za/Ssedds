//! DDS container (spec §5.7): the engine's own writer, and the reader Validate uses to re-parse what was written.
use crate::config::{Format, HeaderForm};
use std::io::Write;
use std::ops::Range;
use std::path::Path;
use std::sync::atomic::{AtomicU64, Ordering};

pub const DDSD_CAPS: u32 = 0x1;
pub const DDSD_HEIGHT: u32 = 0x2;
pub const DDSD_WIDTH: u32 = 0x4;
/// The pitch field holds the row pitch of an uncompressed format.
pub const DDSD_PITCH: u32 = 0x8;
pub const DDSD_PIXELFORMAT: u32 = 0x1000;
pub const DDSD_MIPMAPCOUNT: u32 = 0x2_0000;
pub const DDSD_LINEARSIZE: u32 = 0x8_0000;
/// `dwDepth` is valid: a volume texture when it is above 1.
pub const DDSD_DEPTH: u32 = 0x80_0000;
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

/// Bytes in one mip level: `ceil(w/4) * ceil(h/4) * block_bytes`, or `None` if that overflows `usize`. A
/// dimension of 1..=3 is already one block, so no explicit clamp is needed; a zero dimension gives 0 (and is
/// rejected by `dds_bytes`).
pub fn checked_level_size(format: Format, w: u32, h: u32) -> Option<usize> {
    (w.div_ceil(4) as usize)
        .checked_mul(h.div_ceil(4) as usize)?
        .checked_mul(format.block_bytes())
}

/// `checked_level_size`, saturating to `usize::MAX` on overflow so it never panics and never equals a real
/// buffer length.
pub fn level_size(format: Format, w: u32, h: u32) -> usize {
    checked_level_size(format, w, h).unwrap_or(usize::MAX)
}

/// Levels in a full mip chain down to 1x1 for `w` x `h` (both non-zero): `floor(log2(max(w, h))) + 1`.
pub fn full_chain_len(w: u32, h: u32) -> usize {
    (u32::BITS - w.max(h).leading_zeros()) as usize
}

/// Why `dds_bytes` refused to serialise a texture. Every variant is an engine invariant violation (the encode
/// stage produced the wrong thing), not a user error.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum DdsError {
    #[error("DDS has a zero dimension ({width}x{height})")]
    ZeroDimension { width: u32, height: u32 },
    #[error("DDS has no mip levels")]
    NoLevels,
    #[error("{levels} mip levels exceed the full chain of {max} for {width}x{height}")]
    TooManyLevels {
        levels: usize,
        max: usize,
        width: u32,
        height: u32,
    },
    #[error("mip level {level} is {actual} bytes, expected {expected}")]
    LevelSize {
        level: usize,
        expected: usize,
        actual: usize,
    },
    #[error("no legacy FourCC for {format:?} (srgb={srgb}); use dx10")]
    NoLegacyForm { format: Format, srgb: bool },
    #[error("level 0 size {0} does not fit the 32-bit linear size field")]
    LinearSizeOverflow(usize),
    #[error("mip level {level} of {width}x{height} overflows the size arithmetic")]
    SizeOverflow {
        level: usize,
        width: u32,
        height: u32,
    },
    /// Reader: the input does not start with the `DDS ` magic.
    #[error("not a DDS file")]
    NotDds,
    /// Reader: the header, or the pixel data it promises, runs past the end of the input.
    #[error("truncated DDS file")]
    Truncated,
    /// Reader and `decode`: a file the engine does not handle, or an invalid header field.
    #[error("unsupported DDS: {0}")]
    Unsupported(String),
}

/// Serialise a DDS file: magic, 124-byte header, optional DX10 block, then `levels` (mip 0 first) back to back.
///
/// Validates what the header promises: non-zero dimensions, at least one level and no more than the full chain,
/// every level exactly `level_size` for its dimensions, and a legacy header only where one exists (not for
/// BC6H, BC7 or any `_SRGB` tag, which would otherwise lose the tag).
pub fn dds_bytes(desc: &DdsDesc, levels: &[Vec<u8>]) -> Result<Vec<u8>, DdsError> {
    let (w, h) = (desc.width, desc.height);
    if w == 0 || h == 0 {
        return Err(DdsError::ZeroDimension {
            width: w,
            height: h,
        });
    }
    if levels.is_empty() {
        return Err(DdsError::NoLevels);
    }
    let max = full_chain_len(w, h);
    if levels.len() > max {
        return Err(DdsError::TooManyLevels {
            levels: levels.len(),
            max,
            width: w,
            height: h,
        });
    }
    for (i, l) in levels.iter().enumerate() {
        // The clamp matters here: `w >> i` reaches 0 on the short axis of a non-square chain.
        let expected = checked_level_size(desc.format, (w >> i).max(1), (h >> i).max(1)).ok_or(
            DdsError::SizeOverflow {
                level: i,
                width: w,
                height: h,
            },
        )?;
        if l.len() != expected {
            return Err(DdsError::LevelSize {
                level: i,
                expected,
                actual: l.len(),
            });
        }
    }
    let fourcc = match desc.header {
        ResolvedHeader::Legacy => match legacy_fourcc(desc.format) {
            Some(cc) if !desc.srgb => cc,
            _ => {
                return Err(DdsError::NoLegacyForm {
                    format: desc.format,
                    srgb: desc.srgb,
                })
            }
        },
        ResolvedHeader::Dx10 => *b"DX10",
    };
    let size0 = level_size(desc.format, w, h); // level 0 passed the checked loop above
    let linear_size = u32::try_from(size0).map_err(|_| DdsError::LinearSizeOverflow(size0))?;
    let mips = levels.len() as u32;
    let mut hd = [0u32; 31];
    hd[0] = 124;
    hd[1] = DDSD_CAPS
        | DDSD_HEIGHT
        | DDSD_WIDTH
        | DDSD_PIXELFORMAT
        | DDSD_LINEARSIZE
        | if mips > 1 { DDSD_MIPMAPCOUNT } else { 0 };
    hd[2] = h;
    hd[3] = w;
    hd[4] = linear_size;
    hd[5] = 1; // depth: texconv writes 1 for 2D textures; not a reserved field
    hd[6] = mips;
    hd[18] = 32;
    hd[19] = DDPF_FOURCC;
    hd[20] = u32::from_le_bytes(fourcc);
    hd[26] = DDSCAPS_TEXTURE
        | if mips > 1 {
            DDSCAPS_COMPLEX | DDSCAPS_MIPMAP
        } else {
            0
        };
    let mut out = Vec::with_capacity(148 + levels.iter().map(Vec::len).sum::<usize>());
    out.extend_from_slice(b"DDS ");
    for v in hd {
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
    Ok(out)
}

static TMP_COUNTER: AtomicU64 = AtomicU64::new(0);

/// Removes the temp file unless disarmed (after a successful rename).
struct TempGuard<'a>(Option<&'a Path>);
impl Drop for TempGuard<'_> {
    fn drop(&mut self) {
        if let Some(p) = self.0 {
            let _ = std::fs::remove_file(p);
        }
    }
}

/// Write `bytes` to `path` via a temp file in the same directory, then rename over any existing file.
///
/// The temp is named `.sdc-<pid>-<thread>-<counter>.tmp` and created with `create_new`, so concurrent writers in
/// one directory never share a temp; a name left by a crashed run with a recycled pid is skipped. The temp is
/// removed on any error. A temp left behind by a crash (process killed between create and rename) is not
/// cleaned up: discovery ignores it (not `.dds`) and a sweep is deferred.
///
/// Failures surface as `io::Error`: for example the rename fails with access denied when the target is read-only
/// or open elsewhere without delete sharing, and when it is a directory.
pub fn write_atomic(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    let dir = path
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    std::fs::create_dir_all(dir)?;
    let thread: String = format!("{:?}", std::thread::current().id())
        .chars()
        .filter(char::is_ascii_digit)
        .collect();
    let (tmp_path, mut file) = loop {
        let n = TMP_COUNTER.fetch_add(1, Ordering::Relaxed);
        let tmp_path = dir.join(format!(".sdc-{}-{thread}-{n}.tmp", std::process::id()));
        match std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&tmp_path)
        {
            Ok(f) => break (tmp_path, f),
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(e) => return Err(e),
        }
    };
    let mut guard = TempGuard(Some(&tmp_path));
    file.write_all(bytes)?;
    file.sync_all()?;
    drop(file);
    // On Windows std::fs::rename replaces an existing target (MoveFileExW with MOVEFILE_REPLACE_EXISTING).
    std::fs::rename(&tmp_path, path)?;
    guard.0 = None;
    Ok(())
}

/// Pixel format of a DDS file as the reader understands it (the formats spec §2 lists as inputs, plus BC2).
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
pub enum DdsPixelFormat {
    Bc1 { srgb: bool },
    Bc2 { srgb: bool },
    Bc3 { srgb: bool },
    Bc4,
    Bc5,
    Bc6hUf16,
    Bc6hSf16,
    Bc7 { srgb: bool },
    Rgba8 { srgb: bool },
    Bgra8,
    Bgrx8,
    R8,
    Rg8,
    Rgba16f,
    Rgba32f,
}

impl DdsPixelFormat {
    /// Bytes per 4x4 block, or `None` for an uncompressed format.
    pub fn block_bytes(self) -> Option<usize> {
        use DdsPixelFormat::*;
        match self {
            Bc1 { .. } | Bc4 => Some(8),
            Bc2 { .. } | Bc3 { .. } | Bc5 | Bc6hUf16 | Bc6hSf16 | Bc7 { .. } => Some(16),
            _ => None,
        }
    }

    fn bytes_per_pixel(self) -> usize {
        use DdsPixelFormat::*;
        match self {
            Rgba8 { .. } | Bgra8 | Bgrx8 => 4,
            R8 => 1,
            Rg8 => 2,
            Rgba16f => 8,
            Rgba32f => 16,
            _ => 0,
        }
    }

    /// Bytes in a `w` x `h` level, or `None` when the size overflows `usize`.
    pub fn checked_level_size(self, w: u32, h: u32) -> Option<usize> {
        match self.block_bytes() {
            Some(b) => (w.div_ceil(4) as usize)
                .checked_mul(h.div_ceil(4) as usize)?
                .checked_mul(b),
            None => (w as usize)
                .checked_mul(h as usize)?
                .checked_mul(self.bytes_per_pixel()),
        }
    }

    /// Bytes in a `w` x `h` level; `usize::MAX` when the size overflows (never panics, never matches a real length).
    pub fn level_size(self, w: u32, h: u32) -> usize {
        self.checked_level_size(w, h).unwrap_or(usize::MAX)
    }

    /// The writable format and `_SRGB` tag, or `None` for formats sdc never writes (BC2, signed BC6H, uncompressed).
    pub fn as_format(self) -> Option<(Format, bool)> {
        use DdsPixelFormat as P;
        Some(match self {
            P::Bc1 { srgb } => (Format::Bc1, srgb),
            P::Bc3 { srgb } => (Format::Bc3, srgb),
            P::Bc4 => (Format::Bc4, false),
            P::Bc5 => (Format::Bc5, false),
            P::Bc6hUf16 => (Format::Bc6hUf16, false),
            P::Bc7 { srgb } => (Format::Bc7, srgb),
            _ => return None,
        })
    }
}

#[derive(
    Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize, schemars::JsonSchema,
)]
pub enum HeaderKind {
    Legacy,
    Dx10,
}

/// The five DWORDs of the `DDS_HEADER_DXT10` block.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
pub struct Dx10Fields {
    pub dxgi: u32,
    pub dimension: u32,
    pub misc: u32,
    pub array_size: u32,
    pub misc2: u32,
}

/// A parsed DDS file. `levels` describes face 0 / array slice 0 as `(width, height, range into data)`; `data`
/// is everything after the header(s).
#[derive(Debug, Clone)]
pub struct DdsFile {
    pub width: u32,
    pub height: u32,
    /// The header's mip count (at least 1). `levels` holds at most the full chain for the dimensions, so on a
    /// file whose count exceeds it `levels.len() < mip_count` and `check_header` reports the violation.
    pub mip_count: u32,
    pub format: DdsPixelFormat,
    pub header_kind: HeaderKind,
    pub fourcc: [u8; 4],
    pub dx10: Option<Dx10Fields>,
    pub flags: u32,
    pub pf_flags: u32,
    pub caps: u32,
    pub caps2: u32,
    /// The pitch / linear-size field: the row pitch for an uncompressed format, the mip-0 size for a compressed one.
    pub linear_size: u32,
    /// Any of `reserved1[11]`, `caps3`, `caps4`, `reserved2` is non-zero.
    pub reserved_nonzero: bool,
    pub is_cubemap: bool,
    pub levels: Vec<(u32, u32, Range<usize>)>,
    pub data: Vec<u8>,
}

const MAGIC_AND_HEADER: usize = 128;
const DX10_END: usize = 148;
const DDSCAPS2_CUBEMAP: u32 = 0x200;
const DDSCAPS2_VOLUME: u32 = 0x20_0000;
const DX10_MISC_CUBEMAP: u32 = 0x4;

fn from_dxgi(d: u32) -> Option<DdsPixelFormat> {
    use DdsPixelFormat::*;
    Some(match d {
        2 => Rgba32f,
        10 => Rgba16f,
        28 => Rgba8 { srgb: false },
        29 => Rgba8 { srgb: true },
        49 => Rg8,
        61 => R8,
        71 => Bc1 { srgb: false },
        72 => Bc1 { srgb: true },
        74 => Bc2 { srgb: false },
        75 => Bc2 { srgb: true },
        77 => Bc3 { srgb: false },
        78 => Bc3 { srgb: true },
        80 => Bc4,
        83 => Bc5,
        87 => Bgra8,
        88 => Bgrx8,
        95 => Bc6hUf16,
        96 => Bc6hSf16,
        98 => Bc7 { srgb: false },
        99 => Bc7 { srgb: true },
        _ => return None,
    })
}

/// Parse a DDS file.
///
/// Never panics and never allocates from a header field: the level table has at most 32 entries (clamped to the
/// full chain for the dimensions) and the only large allocation is the copy of `bytes` itself. Sizes use checked
/// arithmetic and are compared with the bytes actually present, so an absurd dimension reports `Truncated`.
/// Zero dimensions, a wrong header size, unknown FourCC / DXGI values and a legacy volume declaration
/// (`DDSD_DEPTH` with a depth above 1, or `DDSCAPS2_VOLUME`) report `Unsupported`; the volume check comes before
/// the length check, so a volume missing depth slices is unsupported too. Cubemaps count six faces and DX10
/// arrays `array_size` slices when checking the length.
pub fn read_dds(bytes: &[u8]) -> Result<DdsFile, DdsError> {
    if bytes.len() < 4 || &bytes[..4] != b"DDS " {
        return Err(DdsError::NotDds);
    }
    if bytes.len() < MAGIC_AND_HEADER {
        return Err(DdsError::Truncated);
    }
    let u = |i: usize| u32::from_le_bytes([bytes[i], bytes[i + 1], bytes[i + 2], bytes[i + 3]]);
    if u(4) != 124 || u(76) != 32 {
        return Err(DdsError::Unsupported(format!(
            "header sizes {}/{} (want 124/32)",
            u(4),
            u(76)
        )));
    }
    let (flags, height, width, linear_size, mips) = (u(8), u(12), u(16), u(20), u(28).max(1));
    if width == 0 || height == 0 {
        return Err(DdsError::Unsupported(format!(
            "zero dimension ({width}x{height})"
        )));
    }
    let reserved_nonzero = bytes[32..76]
        .iter()
        .chain(&bytes[116..MAGIC_AND_HEADER])
        .any(|&b| b != 0);
    let (pf_flags, fourcc) = (u(80), [bytes[84], bytes[85], bytes[86], bytes[87]]);
    let (bits, rm, gm, bm, am) = (u(88), u(92), u(96), u(100), u(104));
    let (caps, caps2) = (u(108), u(112));
    // This pipeline is 2D only: a declared volume is rejected rather than read as its first slice.
    if (flags & DDSD_DEPTH != 0 && u(24) > 1) || caps2 & DDSCAPS2_VOLUME != 0 {
        return Err(DdsError::Unsupported("volume texture".into()));
    }
    let mut off = MAGIC_AND_HEADER;
    let mut dx10 = None;
    use DdsPixelFormat::*;
    let format = if pf_flags & DDPF_FOURCC != 0 {
        match &fourcc {
            b"DX10" => {
                if bytes.len() < DX10_END {
                    return Err(DdsError::Truncated);
                }
                let f = Dx10Fields {
                    dxgi: u(128),
                    dimension: u(132),
                    misc: u(136),
                    array_size: u(140),
                    misc2: u(144),
                };
                off = DX10_END;
                dx10 = Some(f);
                from_dxgi(f.dxgi)
                    .ok_or_else(|| DdsError::Unsupported(format!("DXGI format {}", f.dxgi)))?
            }
            b"DXT1" => Bc1 { srgb: false },
            b"DXT2" | b"DXT3" => Bc2 { srgb: false },
            b"DXT4" | b"DXT5" => Bc3 { srgb: false },
            b"ATI1" | b"BC4U" => Bc4,
            b"ATI2" | b"BC5U" => Bc5,
            // D3DFMT_A16B16G16R16F, D3DFMT_A32B32G32R32F.
            [113, 0, 0, 0] => Rgba16f,
            [116, 0, 0, 0] => Rgba32f,
            other => {
                return Err(DdsError::Unsupported(format!(
                    "FourCC {:?}",
                    String::from_utf8_lossy(other)
                )))
            }
        }
    } else {
        match (bits, rm, gm, bm, am) {
            (32, 0xff, 0xff00, 0xff_0000, 0xff00_0000) => Rgba8 { srgb: false },
            (32, 0xff_0000, 0xff00, 0xff, 0xff00_0000) => Bgra8,
            (32, 0xff_0000, 0xff00, 0xff, 0) => Bgrx8,
            (8, 0xff, 0, 0, 0) => R8,
            (16, 0xff, 0xff00, 0, 0) => Rg8,
            _ => {
                return Err(DdsError::Unsupported(format!(
                    "uncompressed layout {bits} bits, masks {rm:x}/{gm:x}/{bm:x}/{am:x}"
                )))
            }
        }
    };
    let is_cubemap =
        caps2 & DDSCAPS2_CUBEMAP != 0 || dx10.is_some_and(|d| d.misc & DX10_MISC_CUBEMAP != 0);
    // A header count above the full chain is kept in `mip_count` for `check_header`, but only the chain that can
    // exist is laid out, so a hostile count cannot drive the loop or the length check.
    let chain = full_chain_len(width, height).min(mips as usize);
    let mut levels = Vec::with_capacity(chain);
    let (mut w, mut h, mut pos) = (width, height, 0usize);
    for _ in 0..chain {
        let size = format.checked_level_size(w, h).ok_or(DdsError::Truncated)?;
        let end = pos.checked_add(size).ok_or(DdsError::Truncated)?;
        levels.push((w, h, pos..end));
        pos = end;
        w = (w / 2).max(1);
        h = (h / 2).max(1);
    }
    let slices = if is_cubemap { 6 } else { 1 } * dx10.map_or(1, |d| d.array_size.max(1) as usize);
    let total = pos.checked_mul(slices).ok_or(DdsError::Truncated)?;
    if bytes.len() - off < total {
        return Err(DdsError::Truncated);
    }
    Ok(DdsFile {
        width,
        height,
        mip_count: mips,
        format,
        header_kind: if dx10.is_some() {
            HeaderKind::Dx10
        } else {
            HeaderKind::Legacy
        },
        fourcc,
        dx10,
        flags,
        pf_flags,
        caps,
        caps2,
        linear_size,
        reserved_nonzero,
        is_cubemap,
        levels,
        data: bytes[off..].to_vec(),
    })
}

/// Spec §5.7 rules for a written 2D file. Returns one message per violation; empty means the header is as the
/// writer would have produced it. A compressed format carries `DDSD_LINEARSIZE` and the mip-0 surface size; an
/// uncompressed one (which sdc never writes, but whose header is still checked) carries `DDSD_PITCH` and the row
/// pitch (Codex-B I2).
pub fn check_header(f: &DdsFile) -> Vec<String> {
    let mut v = vec![];
    let compressed = f.format.block_bytes().is_some();
    let (size_flag, size_name) = if compressed {
        (DDSD_LINEARSIZE, "LINEARSIZE")
    } else {
        (DDSD_PITCH, "PITCH")
    };
    let need = DDSD_CAPS | DDSD_HEIGHT | DDSD_WIDTH | DDSD_PIXELFORMAT | size_flag;
    if f.flags & need != need {
        v.push(format!(
            "flags {:#x} lack CAPS|HEIGHT|WIDTH|PIXELFORMAT|{size_name}",
            f.flags
        ));
    }
    if f.caps & DDSCAPS_TEXTURE == 0 {
        v.push("caps lack TEXTURE".into());
    }
    if f.mip_count > 1 {
        if f.flags & DDSD_MIPMAPCOUNT == 0 {
            v.push("mips present but flags lack MIPMAPCOUNT".into());
        }
        if f.caps & (DDSCAPS_COMPLEX | DDSCAPS_MIPMAP) != DDSCAPS_COMPLEX | DDSCAPS_MIPMAP {
            v.push("mips present but caps lack COMPLEX|MIPMAP".into());
        }
    }
    let full = full_chain_len(f.width.max(1), f.height.max(1));
    if f.mip_count as usize > full {
        v.push(format!(
            "mip count {} exceeds {} for {}x{}",
            f.mip_count, full, f.width, f.height
        ));
    }
    if compressed {
        let size0 = f.format.level_size(f.width, f.height);
        if f.linear_size as usize != size0 {
            v.push(format!(
                "linear size {} != mip 0 size {size0}",
                f.linear_size
            ));
        }
    } else {
        let row = f.format.level_size(f.width, 1);
        if f.linear_size as usize != row {
            v.push(format!("pitch {} != row pitch {row}", f.linear_size));
        }
    }
    if f.pf_flags & DDPF_NORMAL != 0 {
        v.push("DDPF_NORMAL set (non-standard)".into());
    }
    if f.reserved_nonzero {
        v.push("reserved fields are not zero".into());
    }
    if &f.fourcc == b"ATI1" || &f.fourcc == b"ATI2" {
        v.push("ATI1/ATI2 FourCC is never written by sdc".into());
    }
    if f.is_cubemap {
        v.push("cubemap flag set on a 2D texture".into());
    }
    if let Some(d) = f.dx10 {
        if d.dimension != DIMENSION_TEXTURE2D || d.array_size != 1 || d.misc2 != 0 {
            v.push(format!(
                "DX10 header: dimension {}, array {}, misc2 {} (want {DIMENSION_TEXTURE2D}, 1, 0)",
                d.dimension, d.array_size, d.misc2
            ));
        }
    }
    v
}
