//! BSTriShape / BSSubIndexTriShape / BSDynamicTriShape at BSVER 100 (nif.xml 8228-8246, 8450-8453; nifly
//! Geometry.cpp 449-600 and NifFile.cpp 2029-2058).
use super::skin_partition::{block_cursor, read_skin_partition, tris};
use super::vertex_desc::{read_vertices, VertexDesc};
use super::Mesh;
use crate::header::Header;
use crate::text::lossy;
use crate::NifError;

/// Decodes one SSE tri-shape block. Skinned shapes with `Data Size == 0` take their vertices and triangles
/// from `Skin → NiSkinInstance/BSDismemberSkinInstance → Skin Partition`.
pub fn read_shape(h: &Header, b: &[u8], block: usize) -> Result<Mesh, NifError> {
    let geo = |msg: String| NifError::Geometry {
        block: block as u32,
        msg,
    };
    if h.bsver != 100 {
        return Err(NifError::NotSkyrim {
            version: 0x1402_0007,
            user: 12,
            bsver: h.bsver,
        });
    }
    if block >= h.sizes.len() {
        return Err(geo("block index out of range".into()));
    }
    let kind = h.kind(block);
    let dynamic = match kind {
        b"BSTriShape" | b"BSSubIndexTriShape" => false,
        b"BSDynamicTriShape" => true,
        other => return Err(geo(format!("{} is not an SSE BSTriShape", lossy(other)))),
    };
    let mut c = block_cursor(h, b, block)?;
    // NiObjectNET
    c.i32()?; // name
    let n = c.u32()? as usize;
    c.skip(n.saturating_mul(4))?; // extra data refs
    c.i32()?; // controller
    c.u32()?; // NiAVObject (nif.xml 3440-3460) flags
    let t = c.f32s::<3>()?;
    let r = c.f32s::<9>()?;
    let s = c.f32()?;
    c.i32()?; // collision
    let mut m = Mesh {
        transform: [0, 1, 2].map(|i| [r[3 * i] * s, r[3 * i + 1] * s, r[3 * i + 2] * s, t[i]]),
        ..Mesh::default()
    };
    // BSTriShape
    c.skip(16)?; // bounding sphere
    let skin = c.i32()?;
    c.i32()?; // shader property
    c.i32()?; // alpha property
    let desc = VertexDesc(c.u64()?);
    let num_tris = c.u16()? as usize; // u16 below BSVER 130
    let num_verts = c.u16()? as usize;
    let data_size = c.u32()?;
    if data_size > 0 {
        read_vertices(&mut c, desc, num_verts, desc.stride(), &mut m).map_err(geo)?;
        m.triangles = tris(&mut c, num_tris)?;
    } else if skin != -1 {
        let part = skin_partition_of(h, b, skin).map_err(|e| match e {
            NifError::Geometry { block: sb, msg } => geo(format!("skin block {sb}: {msg}")),
            e => e,
        })?;
        let transform = m.transform;
        m = part.vertices;
        m.transform = transform;
        m.triangles = part
            .partitions
            .into_iter()
            .flat_map(|p| p.triangles_copy)
            .collect();
    }
    // Particle data (SSE only, nifly Geometry.cpp 584-600): uses the head counts
    let particle_size = c.u32()?;
    if particle_size > 0 {
        c.skip(num_verts * 12 + num_tris * 6)?;
    }
    if dynamic {
        // BSDynamicTriShape: Vector4 × (size / 16); xyz = position, w = bitangent X (nifly NifFile.cpp 2048)
        let size = c.u32()? as usize;
        let count = size / 16;
        if count.saturating_mul(16) > c.remaining() {
            return Err(NifError::Truncated {
                at: c.pos(),
                need: size,
            });
        }
        let verts = (0..count)
            .map(|_| c.f32s::<4>())
            .collect::<Result<Vec<_>, _>>()?;
        let n = m
            .positions
            .len()
            .max(m.normals.len())
            .max(m.uvs.len())
            .max(m.bone_weights.len());
        if n != 0 && n != count {
            return Err(geo(format!(
                "dynamic vertex count {count} differs from the vertex data ({n})"
            )));
        }
        m.positions = verts.iter().map(|v| [v[0], v[1], v[2]]).collect();
        for (bt, v) in m.bitangents.iter_mut().zip(&verts) {
            bt[0] = v[3];
        }
    }
    let n = m.positions.len();
    m.triangles.retain(|t| t.iter().all(|&i| (i as usize) < n));
    Ok(m)
}

fn skin_partition_of(
    h: &Header,
    b: &[u8],
    skin: i32,
) -> Result<super::skin_partition::SkinPartition, NifError> {
    let bad = |msg: String| NifError::Geometry {
        block: skin as u32,
        msg,
    };
    let si = usize::try_from(skin)
        .ok()
        .filter(|&i| i < h.sizes.len())
        .ok_or_else(|| bad(format!("skin ref {skin} out of range")))?;
    match h.kind(si) {
        b"NiSkinInstance" | b"BSDismemberSkinInstance" => {}
        other => return Err(bad(format!("skin ref points at {}", lossy(other)))),
    }
    let mut c = block_cursor(h, b, si)?;
    c.i32()?; // NiSkinData
    let p = c.i32()?;
    let pi = usize::try_from(p)
        .ok()
        .filter(|&i| i < h.sizes.len())
        .ok_or_else(|| bad(format!("skin partition ref {p} out of range")))?;
    read_skin_partition(h, b, pi)
}
