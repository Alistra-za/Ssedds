//! SSE NiSkinPartition (nif.xml 5093-5101 + SkinPartition 2143-2172; nifly Skin.cpp 104-265). A skinned SSE
//! BSTriShape stores DataSize 0 and keeps its vertices and triangles here (nifly NifFile.cpp 2029-2058).
use super::vertex_desc::{read_vertices, VertexDesc};
use super::Mesh;
use crate::cursor::Cursor;
use crate::header::Header;
use crate::NifError;

/// One partition's index data, kept raw so callers can check how `triangles` relates to `triangles_copy`.
#[derive(Debug, Clone, PartialEq)]
pub struct Partition {
    pub vertex_map: Vec<u16>,
    /// `Triangles` (only when the partition is not stripped).
    pub triangles: Vec<[u16; 3]>,
    /// SSE `Triangles Copy` (nif.xml 2171): nifly's `trueTriangles`, already in shape vertex indices.
    pub triangles_copy: Vec<[u16; 3]>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct SkinPartition {
    pub vertex_desc: VertexDesc,
    pub vertex_size: u32,
    /// Vertex attributes only (`triangles` empty, identity `transform`).
    pub vertices: Mesh,
    pub partitions: Vec<Partition>,
}

pub(crate) fn block_cursor<'a>(
    h: &Header,
    b: &'a [u8],
    block: usize,
) -> Result<Cursor<'a>, NifError> {
    let off = *h.offsets().get(block).ok_or(NifError::Geometry {
        block: block as u32,
        msg: "block index out of range".into(),
    })?;
    let end = off.saturating_add(h.sizes[block] as usize).min(b.len());
    Ok(Cursor::new(&b[..end], off))
}

pub(crate) fn tris(c: &mut Cursor, n: usize) -> Result<Vec<[u16; 3]>, NifError> {
    if n.saturating_mul(6) > c.remaining() {
        return Err(NifError::Truncated {
            at: c.pos(),
            need: n * 6,
        });
    }
    (0..n).map(|_| Ok([c.u16()?, c.u16()?, c.u16()?])).collect()
}

fn u16s(c: &mut Cursor, n: usize) -> Result<Vec<u16>, NifError> {
    if n.saturating_mul(2) > c.remaining() {
        return Err(NifError::Truncated {
            at: c.pos(),
            need: n * 2,
        });
    }
    (0..n).map(|_| c.u16()).collect()
}

/// Reads an SSE NiSkinPartition block.
pub fn read_skin_partition(h: &Header, b: &[u8], block: usize) -> Result<SkinPartition, NifError> {
    let geo = |msg: String| NifError::Geometry {
        block: block as u32,
        msg,
    };
    if h.kind(block) != b"NiSkinPartition" {
        return Err(geo(format!(
            "expected NiSkinPartition, found {}",
            crate::text::lossy(h.kind(block))
        )));
    }
    let mut c = block_cursor(h, b, block)?;
    let num_partitions = c.u32()?;
    // SSE (User 12, BSVER 100) vertex block
    let data_size = c.u32()?;
    let vertex_size = c.u32()?;
    let vertex_desc = VertexDesc(c.u64()?);
    let mut vertices = Mesh::default();
    if data_size > 0 {
        if vertex_size == 0 {
            return Err(geo("vertex size is zero with non-zero data size".into()));
        }
        if data_size % vertex_size != 0 {
            return Err(geo(format!(
                "data size {data_size} is not a multiple of vertex size {vertex_size}"
            )));
        }
        if vertex_size as usize != vertex_desc.stride() {
            return Err(geo(format!(
                "vertex size {vertex_size} disagrees with the vertex desc ({} bytes)",
                vertex_desc.stride()
            )));
        }
        let count = (data_size / vertex_size) as usize;
        read_vertices(
            &mut c,
            vertex_desc,
            count,
            vertex_size as usize,
            &mut vertices,
        )
        .map_err(geo)?;
    }
    // no with_capacity: each partition consumes at least 10 bytes, so truncation bounds the loop
    let mut partitions = vec![];
    for _ in 0..num_partitions {
        let nv = c.u16()? as usize;
        let nt = c.u16()? as usize;
        let nb = c.u16()? as usize;
        let ns = c.u16()? as usize;
        let nw = c.u16()? as usize;
        c.skip(2 * nb)?; // bones
        let vertex_map = if c.u8()? != 0 {
            u16s(&mut c, nv)?
        } else {
            vec![]
        };
        if c.u8()? != 0 {
            c.skip(nv.saturating_mul(nw).saturating_mul(4))?; // vertex weights, f32 × nw
        }
        let strip_lengths = u16s(&mut c, ns)?;
        let has_faces = c.u8()? != 0;
        if has_faces {
            let total: usize = strip_lengths.iter().map(|&l| l as usize).sum();
            c.skip(2 * total)?; // strips
        }
        let triangles = if ns == 0 && has_faces {
            tris(&mut c, nt)?
        } else {
            vec![]
        };
        if c.u8()? != 0 {
            c.skip(nv.saturating_mul(nw))?; // bone indices, u8 × nw
        }
        c.u8()?; // LOD level
        c.u8()?; // global VB
        c.u64()?; // per-partition vertex desc
        let triangles_copy = tris(&mut c, nt)?;
        partitions.push(Partition {
            vertex_map,
            triangles,
            triangles_copy,
        });
    }
    Ok(SkinPartition {
        vertex_desc,
        vertex_size,
        vertices,
        partitions,
    })
}
