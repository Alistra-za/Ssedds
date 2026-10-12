//! NiObjectNET + NiAVObject prefix at BSVER 100 (nif.xml NiAVObject 3440-3460): name, extra-data refs,
//! controller, flags, translation, rotation, scale, collision object. Shared by the shape reader and the node
//! graph. SSE has no property list (that field is `NI_BS_LTE_FO3`).
use crate::cursor::Cursor;
use crate::header::Header;
use crate::text::lossy;
use crate::NifError;

/// `transform` has the `Mesh.transform` convention: row r is `[s·R[r][0], s·R[r][1], s·R[r][2], t[r]]`.
#[derive(Debug, Clone, PartialEq)]
pub struct AvHead {
    pub name: Option<String>,
    pub transform: [[f32; 4]; 3],
}

/// Reads from the start of a block body and leaves the cursor at the first field of the concrete type.
pub fn read_av_head(h: &Header, c: &mut Cursor) -> Result<AvHead, NifError> {
    let name = h.string(c.i32()?).map(lossy).filter(|s| !s.is_empty());
    let n = c.u32()? as usize;
    c.skip(n.saturating_mul(4))?; // extra data refs
    c.i32()?; // controller
    c.u32()?; // flags (u32 above BSVER 26)
    let t = c.f32s::<3>()?;
    let r = c.f32s::<9>()?;
    let s = c.f32()?;
    c.i32()?; // collision object
    Ok(AvHead {
        name,
        transform: [0, 1, 2].map(|i| [r[3 * i] * s, r[3 * i + 1] * s, r[3 * i + 2] * s, t[i]]),
    })
}
