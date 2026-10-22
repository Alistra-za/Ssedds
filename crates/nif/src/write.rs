//! Byte-exact NIF writer. Untouched blocks, the footer and everything in the header the editor does not own are
//! copied from the source; a file with no structural change is written header-verbatim (plan A6).
use crate::header::{Header, MAX_BLOCKS};
use crate::NifError;
use std::borrow::Cow;

#[derive(Debug, thiserror::Error, PartialEq)]
pub enum WriteError {
    #[error(transparent)]
    Nif(#[from] NifError),
    #[error("patch of {len} bytes at {at} is outside block {block}")]
    Patch { block: u32, at: usize, len: usize },
    #[error("{0}")]
    Refused(String),
}

pub struct NifDoc<'a> {
    src: &'a [u8],
    header: Header,
    blocks: Vec<Cow<'a, [u8]>>,
    types: Vec<Vec<u8>>,
    type_raw: Vec<u16>,
    strings: Vec<Vec<u8>>,
    /// A size, block count, type list or string table changed (or a re-emit was forced).
    structural: bool,
}

impl<'a> NifDoc<'a> {
    pub fn open(src: &'a [u8]) -> Result<NifDoc<'a>, NifError> {
        let header = Header::parse(src)?;
        header.check_file_length(src)?;
        let blocks = header
            .offsets
            .iter()
            .zip(&header.sizes)
            .map(|(&o, &s)| Cow::Borrowed(&src[o..o + s as usize]))
            .collect();
        Ok(NifDoc {
            src,
            blocks,
            types: header.block_types.clone(),
            type_raw: header.type_index_raw.clone(),
            strings: header.strings.clone(),
            header,
            structural: false,
        })
    }

    /// The header of `src`, not of the edited document.
    pub fn header(&self) -> &Header {
        &self.header
    }

    pub fn len(&self) -> u32 {
        self.blocks.len() as u32
    }

    pub fn is_empty(&self) -> bool {
        self.blocks.is_empty()
    }

    /// Type name of a block; empty for an out-of-range block.
    pub fn kind(&self, block: u32) -> &[u8] {
        self.type_raw
            .get(block as usize)
            .and_then(|t| self.types.get((t & 0x7FFF) as usize))
            .map_or(&[], Vec::as_slice)
    }

    /// Current bytes of a block; empty for an out-of-range block.
    pub fn block(&self, block: u32) -> &[u8] {
        self.blocks.get(block as usize).map_or(&[], |b| b)
    }

    /// Overwrite bytes inside a block (block-relative, size unchanged).
    pub fn patch(&mut self, block: u32, at: usize, bytes: &[u8]) -> Result<(), WriteError> {
        let err = || WriteError::Patch {
            block,
            at,
            len: bytes.len(),
        };
        let b = self.blocks.get_mut(block as usize).ok_or_else(err)?;
        let end = at
            .checked_add(bytes.len())
            .filter(|&e| e <= b.len())
            .ok_or_else(err)?;
        b.to_mut()[at..end].copy_from_slice(bytes);
        Ok(())
    }

    /// Replace a block body; a size change forces a header re-emit. Out-of-range blocks are ignored.
    pub fn set_block(&mut self, block: u32, body: Vec<u8>) {
        if let Some(b) = self.blocks.get_mut(block as usize) {
            self.structural |= b.len() != body.len();
            *b = Cow::Owned(body);
        }
    }

    /// Refuses a block or type past what `Header::parse` reads back (Note CY2): the parser masks bit 15 of a type
    /// index and caps blocks at `MAX_BLOCKS`.
    pub fn append(&mut self, kind: &[u8], body: Vec<u8>) -> Result<u32, WriteError> {
        let t = self
            .types
            .iter()
            .position(|k| k == kind)
            .unwrap_or(self.types.len());
        if t > 0x7FFF || self.blocks.len() >= MAX_BLOCKS as usize {
            return Err(WriteError::Refused(
                "This NIF already lists too many block types or blocks.".into(),
            ));
        }
        if t == self.types.len() {
            self.types.push(kind.to_vec());
        }
        self.type_raw.push(t as u16);
        self.blocks.push(Cow::Owned(body));
        self.structural = true;
        Ok(self.blocks.len() as u32 - 1)
    }

    /// Refuses a string past `MAX_BLOCKS` strings, the parser's cap (Note CY2).
    pub fn string(&mut self, s: &[u8]) -> Result<i32, WriteError> {
        if let Some(i) = self.strings.iter().position(|e| e == s) {
            return Ok(i as i32);
        }
        if self.strings.len() >= MAX_BLOCKS as usize {
            return Err(WriteError::Refused(
                "This NIF already lists too many strings.".into(),
            ));
        }
        self.strings.push(s.to_vec());
        self.structural = true;
        Ok(self.strings.len() as i32 - 1)
    }

    pub fn rebuild_header(&mut self) {
        self.structural = true;
    }

    pub fn bytes(&self) -> Vec<u8> {
        let h = &self.header;
        let mut out = Vec::with_capacity(self.src.len());
        if !self.structural {
            out.extend_from_slice(&self.src[..h.data_start]);
        } else {
            out.extend_from_slice(&self.src[..h.prefix_end]);
            out[h.nblocks_at..h.nblocks_at + 4]
                .copy_from_slice(&(self.blocks.len() as u32).to_le_bytes());
            out.extend((self.types.len() as u16).to_le_bytes());
            for t in &self.types {
                out.extend((t.len() as u32).to_le_bytes());
                out.extend(t);
            }
            for t in &self.type_raw {
                out.extend(t.to_le_bytes());
            }
            for b in &self.blocks {
                out.extend((b.len() as u32).to_le_bytes());
            }
            out.extend((self.strings.len() as u32).to_le_bytes());
            let max = self.strings.iter().map(Vec::len).max().unwrap_or(0);
            out.extend((max as u32).to_le_bytes());
            for s in &self.strings {
                out.extend((s.len() as u32).to_le_bytes());
                out.extend(s);
            }
            out.extend_from_slice(&self.src[h.groups_at..h.data_start]);
        }
        for b in &self.blocks {
            out.extend_from_slice(b);
        }
        out.extend_from_slice(&self.src[h.footer_at..]);
        out
    }
}

/// One line per difference between two files, for the editor's change summary.
pub fn block_diff(old: &[u8], new: &[u8]) -> Result<Vec<String>, NifError> {
    let (ho, hn) = (Header::parse(old)?, Header::parse(new)?);
    ho.check_file_length(old)?;
    hn.check_file_length(new)?;
    let name = |h: &Header, i: usize| String::from_utf8_lossy(h.kind(i)).into_owned();
    let body = |h: &Header, i: usize| -> std::ops::Range<usize> {
        h.offsets[i]..h.offsets[i] + h.sizes[i] as usize
    };
    let mut v = Vec::new();
    for i in 0..ho.sizes.len().max(hn.sizes.len()) {
        match (ho.sizes.get(i), hn.sizes.get(i)) {
            (Some(&a), Some(&b)) if a != b => {
                v.push(format!("block {i} {}: {a} → {b} bytes", name(&hn, i)))
            }
            (Some(_), Some(_)) if old[body(&ho, i)] != new[body(&hn, i)] => v.push(format!(
                "block {i} {}: same size, bytes differ",
                name(&hn, i)
            )),
            (Some(_), Some(_)) => {}
            (None, Some(_)) => v.push(format!("block {i} {}: added", name(&hn, i))),
            _ => v.push(format!("block {i}: removed")),
        }
    }
    if hn.strings.len() > ho.strings.len() {
        v.push(format!("strings: +{}", hn.strings.len() - ho.strings.len()));
    }
    Ok(v)
}
