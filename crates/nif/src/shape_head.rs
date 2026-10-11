//! Shape heads up to the shader/alpha refs (plan 01 Q3; nif.xml 3840-3870, 8228-8246).
use crate::cursor::Cursor;
use crate::header::Header;
use crate::text::lossy;
use crate::NifError;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ShapeKind {
    BSTriShape,
    BSSubIndexTriShape,
    BSDynamicTriShape,
    BSMeshLODTriShape,
    NiTriShape,
    NiTriStrips,
    BSLODTriShape,
    BSSegmentedTriShape,
    NiParticleSystem,
    BSStripParticleSystem,
}

impl ShapeKind {
    pub fn from_name(n: &[u8]) -> Option<ShapeKind> {
        use ShapeKind::*;
        Some(match n {
            b"BSTriShape" => BSTriShape,
            b"BSSubIndexTriShape" => BSSubIndexTriShape,
            b"BSDynamicTriShape" => BSDynamicTriShape,
            b"BSMeshLODTriShape" => BSMeshLODTriShape,
            b"NiTriShape" => NiTriShape,
            b"NiTriStrips" => NiTriStrips,
            b"BSLODTriShape" => BSLODTriShape,
            b"BSSegmentedTriShape" => BSSegmentedTriShape,
            b"NiParticleSystem" => NiParticleSystem,
            b"BSStripParticleSystem" => BSStripParticleSystem,
            _ => return None,
        })
    }
    fn bs_layout(self, bsver: u32) -> bool {
        use ShapeKind::*;
        match self {
            BSTriShape | BSSubIndexTriShape | BSDynamicTriShape | BSMeshLODTriShape => true,
            NiParticleSystem | BSStripParticleSystem => bsver >= 100,
            _ => false,
        }
    }
    fn has_vertex_head(self) -> bool {
        use ShapeKind::*;
        matches!(
            self,
            BSTriShape | BSSubIndexTriShape | BSDynamicTriShape | BSMeshLODTriShape
        )
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct ShapeHead {
    pub kind: ShapeKind,
    pub name: Option<String>,
    pub skinned: bool,
    pub shader: i32,
    pub alpha: i32,
    pub vertex_desc: Option<u64>,
    pub num_triangles: Option<u32>,
    pub num_vertices: Option<u32>,
}

pub fn decode_head(
    h: &Header,
    b: &[u8],
    block: usize,
    kind: ShapeKind,
) -> Result<ShapeHead, NifError> {
    let mut c = Cursor::new(b, h.offsets()[block]);
    let name = h.string(c.i32()?).map(lossy).filter(|s| !s.is_empty());
    let n = c.u32()? as usize;
    c.skip(4 * n)?;
    c.i32()?; // controller
    c.u32()?; // NiAVObject flags (u32 at BSVER > 26)
    c.skip(12 + 36 + 4)?; // translation, rotation, scale
    c.i32()?; // collision
    let (skinned, shader, alpha);
    let (mut vertex_desc, mut num_triangles, mut num_vertices) = (None, None, None);
    if kind.bs_layout(h.bsver) {
        c.skip(16)?; // bounding sphere
        skinned = c.i32()? != -1;
        shader = c.i32()?;
        alpha = c.i32()?;
        if kind.has_vertex_head() {
            vertex_desc = Some(c.u64()?);
            num_triangles = Some(c.u16()? as u32); // u16 at BSVER 100 (u32 only from 130)
            num_vertices = Some(c.u16()? as u32);
        }
    } else {
        c.i32()?; // data
        skinned = c.i32()? != -1;
        let m = c.u32()? as usize;
        c.skip(8 * m)?; // material names + extra data
        c.i32()?; // active material
        c.u8()?; // needs update
        shader = c.i32()?;
        alpha = c.i32()?;
    }
    Ok(ShapeHead {
        kind,
        name,
        skinned,
        shader,
        alpha,
        vertex_desc,
        num_triangles,
        num_vertices,
    })
}
