//! Skyrim SE BSA (version 105, LZ4 frame). Reads the folder/file records and the name table on open;
//! entry bytes on demand. Layout: header 36 B, folder records 24 B (v105), per-folder bzstring name + 16 B file
//! records, NUL-separated file names, data (optional embedded bstring full path; compressed = u32 size + LZ4 frame).
use crate::VfsError;
use nif::path::canon_mesh;
use nif::text::lossy;
use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};

const INCLUDE_DIR_NAMES: u32 = 0x1;
const INCLUDE_FILE_NAMES: u32 = 0x2;
const COMPRESSED_DEFAULT: u32 = 0x4;
const EMBED_NAMES: u32 = 0x100;

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
        let mut f = std::io::BufReader::new(std::fs::File::open(path)?);
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
        let mut counts = Vec::with_capacity(nfolders);
        for _ in 0..nfolders {
            let r = rd::<24>(&mut f)?;
            counts.push(u32::from_le_bytes(r[8..12].try_into().unwrap()) as usize);
        }
        let mut recs = Vec::with_capacity(nfiles);
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
        if recs.len() != nfiles {
            return Err(VfsError::Corrupt("file count mismatch".into()));
        }
        let mut names = vec![0u8; file_len];
        f.read_exact(&mut names)?;
        let names: Vec<&[u8]> = names.split(|&c| c == 0).collect();
        let entries = recs
            .into_iter()
            .enumerate()
            .map(|(i, (folder, size, offset, compressed))| {
                let file = names.get(i).map(|n| lossy(n)).unwrap_or_default();
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
        })
    }

    pub fn read(&self, e: &BsaEntry) -> Result<Vec<u8>, VfsError> {
        let mut f = std::fs::File::open(&self.path)?;
        f.seek(SeekFrom::Start(e.offset as u64))?;
        let mut data = vec![0u8; e.size as usize];
        f.read_exact(&mut data)?;
        let mut p = 0usize;
        if self.flags & EMBED_NAMES != 0 {
            p += 1 + *data
                .first()
                .ok_or_else(|| VfsError::Corrupt("empty entry".into()))?
                as usize;
        }
        if !e.compressed {
            return data
                .get(p..)
                .map(<[u8]>::to_vec)
                .ok_or_else(|| VfsError::Corrupt("entry too short".into()));
        }
        let orig = data
            .get(p..p + 4)
            .ok_or_else(|| VfsError::Corrupt("entry too short".into()))?;
        let orig = u32::from_le_bytes(orig.try_into().unwrap()) as usize;
        let mut out = Vec::with_capacity(orig);
        lz4_flex::frame::FrameDecoder::new(&data[p + 4..])
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
