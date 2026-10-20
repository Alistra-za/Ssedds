//! NiObjectNET prefix: Name, extra-data refs, Controller. Offsets are absolute file offsets.
use crate::{header::Header, NifError};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ObjectNet {
    /// Offset of the Name field (after the leading u32 shader type on BSLightingShaderProperty).
    pub start: usize,
    pub name: i32,
    /// Extra-data refs; null (-1) entries are kept.
    pub extra: Vec<i32>,
    pub controller: i32,
    pub end: usize,
}

impl ObjectNet {
    pub fn extra_at(&self, i: usize) -> usize {
        self.start + 8 + 4 * i
    }
    pub fn controller_at(&self) -> usize {
        self.start + 8 + 4 * self.extra.len()
    }
}

pub fn read_net(h: &Header, b: &[u8], block: usize) -> Result<ObjectNet, NifError> {
    if block >= h.sizes.len() {
        return Err(NifError::Geometry {
            block: block as u32,
            msg: "no such block".into(),
        });
    }
    let bs = h.offsets()[block];
    let block_end = bs + h.sizes[block] as usize;
    let geo = |msg: &str| NifError::Geometry {
        block: block as u32,
        msg: msg.into(),
    };
    let start = bs
        + if h.kind(block) == b"BSLightingShaderProperty" {
            4
        } else {
            0
        };
    let u32_at = |at: usize| -> Result<u32, NifError> {
        match at.checked_add(4).filter(|&e| e <= block_end.min(b.len())) {
            Some(e) => Ok(u32::from_le_bytes(b[at..e].try_into().unwrap())),
            None => Err(geo("NiObjectNET runs past the block")),
        }
    };
    let name = u32_at(start)? as i32;
    let n = u32_at(start + 4)? as usize;
    // n <= u32::MAX, so this cannot overflow usize on 64-bit
    let end = start + 8 + 4 * n + 4;
    if end > block_end {
        return Err(geo("NiObjectNET extra-data count runs past the block"));
    }
    let extra = (0..n)
        .map(|i| u32_at(start + 8 + 4 * i).map(|v| v as i32))
        .collect::<Result<Vec<_>, _>>()?;
    let controller = u32_at(end - 4)? as i32;
    Ok(ObjectNet {
        start,
        name,
        extra,
        controller,
        end,
    })
}

pub fn encode_net(name: i32, extra: &[i32], controller: i32) -> Vec<u8> {
    let mut v = name.to_le_bytes().to_vec();
    v.extend((extra.len() as u32).to_le_bytes());
    for e in extra {
        v.extend(e.to_le_bytes());
    }
    v.extend(controller.to_le_bytes());
    v
}
