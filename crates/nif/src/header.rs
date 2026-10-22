//! NIF 20.2.0.7 header (nif.xml Header 1963-1983; plan 01 Q1). Only Skyrim LE (BSVER 83) and SE (100) pass.
use crate::cursor::Cursor;
use crate::NifError;

pub struct Header {
    pub bsver: u32,
    pub block_types: Vec<Vec<u8>>,
    pub type_index: Vec<u16>,
    pub sizes: Vec<u32>,
    pub strings: Vec<Vec<u8>>,
    /// Offset of block 0.
    pub data_start: usize,
    /// Absolute offset of each block.
    pub offsets: Vec<usize>,
    /// Offset of Num Block Types.
    pub prefix_end: usize,
    /// Offset of the Num Blocks u32.
    pub nblocks_at: usize,
    /// Block type indices as stored (top bit kept).
    pub type_index_raw: Vec<u16>,
    /// Max String Length as stored.
    pub max_string_len: u32,
    /// Offset of Num Groups.
    pub groups_at: usize,
    /// data_start + sum of sizes: offset of the footer.
    pub footer_at: usize,
}

pub(crate) const MAX_BLOCKS: u32 = 1 << 20; // nifly NIF_BLOCK_INDEX_LIMIT

impl Header {
    pub fn parse(b: &[u8]) -> Result<Header, NifError> {
        let nl = b
            .iter()
            .take(128)
            .position(|&c| c == b'\n')
            .ok_or(NifError::BadHeaderLine)?;
        if !(b.starts_with(b"Gamebryo File Format") || b.starts_with(b"NetImmerse File Format")) {
            return Err(NifError::BadHeaderLine);
        }
        let mut c = Cursor::new(b, nl + 1);
        // MW/OB/FO3 headers end up here too and fail the version gate with NotSkyrim (better message than BadHeaderLine)
        let version = c.u32()?;
        if version != 0x1402_0007 {
            return Err(NifError::NotSkyrim {
                version,
                user: 0,
                bsver: 0,
            });
        }
        if c.u8()? != 1 {
            return Err(NifError::BadEndian);
        }
        let user = c.u32()?;
        let nblocks_at = c.pos();
        let nblocks = c.u32()?;
        let bsver = c.u32()?;
        if user != 12 || !(bsver == 83 || bsver == 100) || nblocks > MAX_BLOCKS {
            return Err(NifError::NotSkyrim {
                version,
                user,
                bsver,
            });
        }
        c.export_bytes()?; // author
        c.export_bytes()?; // process script (BSVER < 131)
        c.export_bytes()?; // export script; no Max Filepath below BSVER 103
        let prefix_end = c.pos();
        let ntypes = c.u16()? as usize;
        let block_types = (0..ntypes)
            .map(|_| c.sized_bytes().map(<[u8]>::to_vec))
            .collect::<Result<Vec<_>, _>>()?;
        let type_index_raw = (0..nblocks)
            .map(|_| c.u16())
            .collect::<Result<Vec<_>, _>>()?;
        let type_index: Vec<u16> = type_index_raw.iter().map(|t| t & 0x7FFF).collect();
        if type_index.iter().any(|&t| t as usize >= ntypes) {
            return Err(NifError::Truncated {
                at: c.pos(),
                need: 0,
            });
        }
        let sizes = (0..nblocks)
            .map(|_| c.u32())
            .collect::<Result<Vec<_>, _>>()?;
        let nstrings = c.u32()?;
        let max_string_len = c.u32()?;
        if nstrings > MAX_BLOCKS {
            return Err(NifError::TooManyStrings(nstrings));
        }
        let strings = (0..nstrings)
            .map(|_| c.sized_bytes().map(<[u8]>::to_vec))
            .collect::<Result<Vec<_>, _>>()?;
        let groups_at = c.pos();
        let ngroups = c.u32()? as usize;
        c.skip(4 * ngroups)?;
        let mut p = c.pos();
        let offsets = sizes
            .iter()
            .map(|&s| {
                let o = p;
                p += s as usize;
                o
            })
            .collect();
        Ok(Header {
            bsver,
            block_types,
            type_index,
            sizes,
            strings,
            data_start: c.pos(),
            offsets,
            prefix_end,
            nblocks_at,
            type_index_raw,
            max_string_len,
            groups_at,
            footer_at: p,
        })
    }

    pub fn offsets(&self) -> &[usize] {
        &self.offsets
    }

    pub fn kind(&self, block: usize) -> &[u8] {
        &self.block_types[self.type_index[block] as usize]
    }

    pub fn string(&self, idx: i32) -> Option<&[u8]> {
        usize::try_from(idx)
            .ok()
            .and_then(|i| self.strings.get(i))
            .map(Vec::as_slice)
    }

    /// Footer root refs.
    pub fn roots(&self, b: &[u8]) -> Result<Vec<i32>, NifError> {
        self.check_file_length(b)?;
        let mut c = Cursor::new(b, self.footer_at);
        let n = c.u32()?;
        (0..n).map(|_| c.i32()).collect()
    }

    /// data_start + Σ sizes + 4 + 4·roots == file length (plan 01 Q1 scanner rule a).
    pub fn check_file_length(&self, b: &[u8]) -> Result<(), NifError> {
        let end: usize = self.data_start + self.sizes.iter().map(|&s| s as usize).sum::<usize>();
        let mut c = Cursor::new(b, end);
        let roots = c.u32().map_err(|_| NifError::FileLength {
            expected: end + 4,
            actual: b.len(),
        })? as usize;
        let expected = end + 4 + 4 * roots;
        if expected != b.len() {
            return Err(NifError::FileLength {
                expected,
                actual: b.len(),
            });
        }
        Ok(())
    }
}
