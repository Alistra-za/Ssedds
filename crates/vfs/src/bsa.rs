//! Skyrim SE BSA (version 105, LZ4 frame). Reads the folder/file records and the name table on open;
//! entry bytes on demand. Layout: header 36 B, folder records 24 B (v105), per-folder bzstring name + 16 B file
//! records, NUL-separated file names, data (optional embedded bstring full path; compressed = u32 size + LZ4 frame).
use crate::VfsError;
use nif::path::canon_mesh;
use nif::text::lossy;
use std::io::{Read, Seek};
use std::path::{Path, PathBuf};

const INCLUDE_DIR_NAMES: u32 = 0x1;
const INCLUDE_FILE_NAMES: u32 = 0x2;
const COMPRESSED_DEFAULT: u32 = 0x4;
const EMBED_NAMES: u32 = 0x100;
const MAX_ENTRY: usize = 512 * 1024 * 1024;

#[derive(Debug, Clone, PartialEq)]
pub struct BsaEntry {
    pub norm: String,
    pub offset: u32,
    pub size: u32,
    pub compressed: bool,
}

pub struct Bsa {
    pub path: PathBuf,
    pub version: u32,
    pub flags: u32,
    pub entries: Vec<BsaEntry>,
    /// Kept open for positional reads (spec §2 step 2); its length is checked once, at open.
    file: std::fs::File,
    total: u64,
}

fn rd<const N: usize>(f: &mut impl Read) -> Result<[u8; N], VfsError> {
    let mut b = [0u8; N];
    f.read_exact(&mut b)?;
    Ok(b)
}
fn u32le(f: &mut impl Read) -> Result<u32, VfsError> {
    Ok(u32::from_le_bytes(rd::<4>(f)?))
}

impl Bsa {
    pub fn open(path: &Path) -> Result<Bsa, VfsError> {
        let file = std::fs::File::open(path)?;
        let file_total = file.metadata()?.len();
        let mut f = std::io::BufReader::new(&file);
        let magic = rd::<4>(&mut f).map_err(|_| VfsError::NotBsa)?;
        if &magic != b"BSA\0" {
            return Err(VfsError::NotBsa);
        }
        let version = u32le(&mut f)?;
        if version != 105 {
            return Err(VfsError::UnsupportedBsaVersion(version));
        }
        let folder_off = u32le(&mut f)?;
        let flags = u32le(&mut f)?;
        let nfolders = u32le(&mut f)? as usize;
        let nfiles = u32le(&mut f)? as usize;
        let _folder_len = u32le(&mut f)?;
        let file_len = u32le(&mut f)? as usize;
        rd::<4>(&mut f)?; // file flags + padding
        if folder_off != 36
            || flags & (INCLUDE_DIR_NAMES | INCLUDE_FILE_NAMES)
                != (INCLUDE_DIR_NAMES | INCLUDE_FILE_NAMES)
        {
            return Err(VfsError::Corrupt(
                "archive without directory or file names".into(),
            ));
        }
        if nfiles > 4_000_000 || nfolders > nfiles.max(1) * 2 {
            return Err(VfsError::Corrupt("implausible counts".into()));
        }
        let mut counts = Vec::with_capacity(nfolders.min(65536));
        for _ in 0..nfolders {
            let r = rd::<24>(&mut f)?;
            counts.push(u32::from_le_bytes(r[8..12].try_into().unwrap()) as usize);
        }
        if counts.iter().sum::<usize>() != nfiles {
            return Err(VfsError::Corrupt("file count mismatch".into()));
        }
        let mut recs = Vec::with_capacity(nfiles.min(65536));
        for &count in &counts {
            let l = rd::<1>(&mut f)?[0] as usize;
            let mut name = vec![0u8; l];
            f.read_exact(&mut name)?;
            let folder = lossy(name.strip_suffix(&[0]).unwrap_or(&name));
            for _ in 0..count {
                let r = rd::<16>(&mut f)?;
                let size = u32::from_le_bytes(r[8..12].try_into().unwrap());
                let offset = u32::from_le_bytes(r[12..16].try_into().unwrap());
                let compressed = (flags & COMPRESSED_DEFAULT != 0) ^ (size & 0x4000_0000 != 0);
                recs.push((folder.clone(), size & 0x3FFF_FFFF, offset, compressed));
            }
        }
        let pos = f.stream_position()?;
        if pos.saturating_add(file_len as u64) > file_total {
            return Err(VfsError::Corrupt("name table past end of file".into()));
        }
        let mut names = vec![0u8; file_len];
        f.read_exact(&mut names)?;
        let names: Vec<&[u8]> = names
            .strip_suffix(&[0])
            .unwrap_or(&names)
            .split(|&c| c == 0)
            .collect();
        if names.len() < nfiles {
            return Err(VfsError::Corrupt("fewer file names than records".into()));
        }
        let entries = recs
            .into_iter()
            .enumerate()
            .map(|(i, (folder, size, offset, compressed))| {
                let file = lossy(names[i]);
                BsaEntry {
                    norm: canon_mesh(&format!("{folder}\\{file}")),
                    offset,
                    size,
                    compressed,
                }
            })
            .collect();
        Ok(Bsa {
            path: path.to_path_buf(),
            version,
            flags,
            entries,
            file,
            total: file_total,
        })
    }

    pub fn read(&self, e: &BsaEntry) -> Result<Vec<u8>, VfsError> {
        if e.offset as u64 + e.size as u64 > self.total {
            return Err(VfsError::Corrupt(format!(
                "{}: data past end of file",
                e.norm
            )));
        }
        let mut data = vec![0u8; e.size as usize];
        read_at(&self.file, &mut data, e.offset as u64)?;
        let mut p = 0usize;
        if self.flags & EMBED_NAMES != 0 {
            p += 1 + *data
                .first()
                .ok_or_else(|| VfsError::Corrupt("empty entry".into()))?
                as usize;
        }
        if !e.compressed {
            if p > data.len() {
                return Err(VfsError::Corrupt("entry too short".into()));
            }
            data.drain(..p); // in place: no second copy of the payload
            return Ok(data);
        }
        let orig = data
            .get(p..p + 4)
            .ok_or_else(|| VfsError::Corrupt("entry too short".into()))?;
        let orig = u32::from_le_bytes(orig.try_into().unwrap()) as usize;
        if orig > MAX_ENTRY {
            return Err(VfsError::Corrupt(format!(
                "{}: implausible size {orig}",
                e.norm
            )));
        }
        let mut out = Vec::with_capacity(orig);
        lz4_flex::frame::FrameDecoder::new(&data[p + 4..])
            .take(orig as u64 + 1)
            .read_to_end(&mut out)
            .map_err(|x| VfsError::Corrupt(x.to_string()))?;
        if out.len() != orig {
            return Err(VfsError::Corrupt(format!(
                "{}: {} bytes, header says {orig}",
                e.norm,
                out.len()
            )));
        }
        Ok(out)
    }
}

/// Fills `buf` from `off` without moving a shared cursor, so threads can read one handle at once.
fn read_at(f: &std::fs::File, buf: &mut [u8], off: u64) -> std::io::Result<()> {
    #[cfg(windows)]
    {
        use std::os::windows::fs::FileExt;
        let (mut buf, mut off) = (buf, off);
        while !buf.is_empty() {
            let n = f.seek_read(buf, off)?;
            if n == 0 {
                return Err(std::io::ErrorKind::UnexpectedEof.into());
            }
            buf = &mut buf[n..];
            off += n as u64;
        }
        Ok(())
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::FileExt;
        f.read_exact_at(buf, off)
    }
}
