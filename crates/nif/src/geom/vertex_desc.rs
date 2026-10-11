//! BSVertexDesc (nif.xml 2077-2105) and the per-vertex record BSVertexDataSSE (nif.xml 2128-2141; nifly
//! Geometry.cpp 495-560, Skin.cpp 125-188).
use super::Mesh;
use crate::cursor::Cursor;

/// The packed u64 vertex descriptor. Low nibble = vertex size in dwords; attribute flags at bits 44-55.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct VertexDesc(pub u64);

/// `VertexAttribute` bits (nif.xml 2077-2089), as stored at `desc >> 44`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Attr {
    Vertex = 0x1,
    Uvs = 0x2,
    Uvs2 = 0x4,
    Normals = 0x8,
    Tangents = 0x10,
    Colors = 0x20,
    Skinned = 0x40,
    LandData = 0x80,
    EyeData = 0x100,
    Instance = 0x200,
    FullPrecision = 0x400,
}

impl VertexDesc {
    /// Bytes per vertex record (`Vertex Data Size` nibble × 4).
    pub fn stride(self) -> usize {
        (self.0 & 0xF) as usize * 4
    }
    pub fn flags(self) -> u16 {
        ((self.0 >> 44) & 0xFFF) as u16
    }
    pub fn has(self, a: Attr) -> bool {
        self.flags() & a as u16 != 0
    }
}

/// [0,255] → [-1,1] (nif.xml `normbyte`, 2109).
fn nb(b: u8) -> f32 {
    b as f32 / 255.0 * 2.0 - 1.0
}

fn hf(c: &mut Cursor) -> Result<f32, String> {
    c.u16()
        .map(|x| half::f16::from_bits(x).to_f32())
        .map_err(|e| e.to_string())
}

/// Reads `count` BSVertexDataSSE records of `stride` bytes into `m`. Fields are read in nif.xml order within
/// each record; a record that would read past `stride` is an error, trailing padding is skipped.
/// At BSVER 100 positions are always f32 (nifly Geometry.cpp 500: `IsFullPrecision() || Stream() == 100`).
pub(crate) fn read_vertices(
    c: &mut Cursor,
    desc: VertexDesc,
    count: usize,
    stride: usize,
    m: &mut Mesh,
) -> Result<(), String> {
    if count == 0 {
        return Ok(());
    }
    if stride == 0 {
        return Err("vertex size is 0".into());
    }
    match count.checked_mul(stride) {
        Some(n) if n <= c.remaining() => {}
        _ => {
            return Err(format!(
                "{count} vertices × {stride} bytes exceed the block ({} bytes left)",
                c.remaining()
            ))
        }
    }
    if desc.has(Attr::Uvs2) || desc.has(Attr::LandData) {
        // not part of BSVertexDataSSE; reading on would misplace every later field
        return Err(format!(
            "vertex attributes {:#x} include UV2/land data, which SSE does not lay out",
            desc.flags()
        ));
    }
    let has = |a| desc.has(a);
    let (pos, uv, nrm) = (has(Attr::Vertex), has(Attr::Uvs), has(Attr::Normals));
    let tan = nrm && has(Attr::Tangents); // cond (ARG & 0x18) == 0x18
    let (col, skin, eye) = (has(Attr::Colors), has(Attr::Skinned), has(Attr::EyeData));
    let reserve = |on: bool| if on { count } else { 0 };
    m.positions.reserve(reserve(pos));
    m.uvs.reserve(reserve(uv));
    m.normals.reserve(reserve(nrm));
    m.tangents.reserve(reserve(tan));
    m.bitangents.reserve(reserve(tan));
    m.colors.reserve(reserve(col));
    m.bone_weights.reserve(reserve(skin));
    m.bone_indices.reserve(reserve(skin));
    m.eye.reserve(reserve(eye));
    let e = |e: crate::NifError| e.to_string();
    for i in 0..count {
        let start = c.pos();
        let mut bx = 0.0;
        if pos {
            m.positions.push(c.f32s::<3>().map_err(e)?);
            // Bitangent X when Tangents is set (0x11), otherwise "Unused W"
            bx = c.f32().map_err(e)?;
        }
        if uv {
            m.uvs.push([hf(c)?, hf(c)?]);
        }
        let mut by = 0.0;
        if nrm {
            let n = [c.u8().map_err(e)?, c.u8().map_err(e)?, c.u8().map_err(e)?];
            m.normals.push(n.map(nb));
            by = nb(c.u8().map_err(e)?);
        }
        if tan {
            let t = [c.u8().map_err(e)?, c.u8().map_err(e)?, c.u8().map_err(e)?];
            m.tangents.push(t.map(nb));
            let bz = nb(c.u8().map_err(e)?);
            m.bitangents.push([bx, by, bz]);
        }
        if col {
            m.colors.push([
                c.u8().map_err(e)?,
                c.u8().map_err(e)?,
                c.u8().map_err(e)?,
                c.u8().map_err(e)?,
            ]);
        }
        if skin {
            m.bone_weights.push([hf(c)?, hf(c)?, hf(c)?, hf(c)?]);
            m.bone_indices.push([
                c.u8().map_err(e)?,
                c.u8().map_err(e)?,
                c.u8().map_err(e)?,
                c.u8().map_err(e)?,
            ]);
        }
        if eye {
            m.eye.push(c.f32().map_err(e)?);
        }
        let used = c.pos() - start;
        if used > stride {
            return Err(format!(
                "vertex {i}: attributes {:#x} need {used} bytes but the vertex size is {stride}",
                desc.flags()
            ));
        }
        c.skip(stride - used).map_err(e)?;
    }
    Ok(())
}
