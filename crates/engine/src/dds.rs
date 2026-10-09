//! DDS container (spec §5.7): the engine's own writer. (The reader used by Validate lands in a later task.)
use crate::config::{Format, HeaderForm};
use std::io::Write;
use std::path::Path;
use std::sync::atomic::{AtomicU64, Ordering};

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

/// Bytes in one mip level: `ceil(w/4) * ceil(h/4) * block_bytes`. A dimension of 1..=3 is already one block, so
/// no explicit clamp is needed; a zero dimension gives 0 (and is rejected by `dds_bytes`).
pub fn level_size(format: Format, w: u32, h: u32) -> usize {
    (w.div_ceil(4) as usize) * (h.div_ceil(4) as usize) * format.block_bytes()
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
        let expected = level_size(desc.format, (w >> i).max(1), (h >> i).max(1));
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
    let size0 = level_size(desc.format, w, h);
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
