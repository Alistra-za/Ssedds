//! LAYER 1: SSE BSTriShape geometry for the 3D preview (plan B5a; 01 §Recommendation). LE meshes are never
//! rendered (00 §6.5), so NiTriShapeData is not decoded.
mod bstrishape;
pub mod skin_partition;
mod vertex_desc;

pub use bstrishape::read_shape;
pub use vertex_desc::{Attr, VertexDesc};

/// One shape's vertex streams. Every non-empty stream has one entry per position. `bitangents` and
/// `tangents` are present together (Normals + Tangents set). `triangles` index `positions`; triangles with
/// an out-of-range index are dropped (nifly `RemoveInvalidTris`).
#[derive(Debug, Clone, PartialEq)]
pub struct Mesh {
    pub positions: Vec<[f32; 3]>,
    pub uvs: Vec<[f32; 2]>,
    pub normals: Vec<[f32; 3]>,
    pub tangents: Vec<[f32; 3]>,
    pub bitangents: Vec<[f32; 3]>,
    pub colors: Vec<[u8; 4]>,
    pub bone_weights: Vec<[f32; 4]>,
    pub bone_indices: Vec<[u8; 4]>,
    pub eye: Vec<f32>,
    pub triangles: Vec<[u16; 3]>,
    /// The shape's local (parent-space) transform as a 3×4 row-major matrix: `p' = R·(s·p) + t`, where row r
    /// is `[s·R[r][0], s·R[r][1], s·R[r][2], t[r]]` and R's rows are the file's consecutive float triples
    /// (nifly `Matrix3::rows`, `MatTransform::ApplyTransform`). Parent NiNode transforms are not applied.
    pub transform: [[f32; 4]; 3],
}

impl Default for Mesh {
    fn default() -> Self {
        Mesh {
            positions: vec![],
            uvs: vec![],
            normals: vec![],
            tangents: vec![],
            bitangents: vec![],
            colors: vec![],
            bone_weights: vec![],
            bone_indices: vec![],
            eye: vec![],
            triangles: vec![],
            transform: [
                [1.0, 0.0, 0.0, 0.0],
                [0.0, 1.0, 0.0, 0.0],
                [0.0, 0.0, 1.0, 0.0],
            ],
        }
    }
}
