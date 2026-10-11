//! `nif::geom::Mesh` → D3D11 vertex streams (one buffer per reflected VS input), input layout and index buffer.
use crate::device::Gpu;
use nif::geom::Mesh;
use std::ffi::CString;
use windows::core::PCSTR;
use windows::Win32::Graphics::Direct3D11::{
    ID3D11Buffer, ID3D11InputLayout, D3D11_BIND_FLAG, D3D11_BIND_INDEX_BUFFER,
    D3D11_BIND_VERTEX_BUFFER, D3D11_BUFFER_DESC, D3D11_INPUT_ELEMENT_DESC,
    D3D11_INPUT_PER_VERTEX_DATA, D3D11_SUBRESOURCE_DATA, D3D11_USAGE_IMMUTABLE,
};
use windows::Win32::Graphics::Dxgi::Common::{
    DXGI_FORMAT, DXGI_FORMAT_R32G32B32A32_FLOAT, DXGI_FORMAT_R32G32_FLOAT, DXGI_FORMAT_R32_FLOAT,
    DXGI_FORMAT_R8G8B8A8_UNORM,
};

/// The `Mesh` invariant the renderer relies on (B5a carried minor): non-empty positions, every non-empty
/// stream as long as `positions`, `tangents`/`bitangents` together, every triangle index in range.
/// A violation is a per-shape error, never a panic.
pub fn validate(m: &Mesh) -> Result<(), String> {
    let n = m.positions.len();
    if n == 0 {
        return Err("no vertex positions".into());
    }
    let streams = [
        ("uvs", m.uvs.len()),
        ("normals", m.normals.len()),
        ("tangents", m.tangents.len()),
        ("bitangents", m.bitangents.len()),
        ("colors", m.colors.len()),
        ("bone weights", m.bone_weights.len()),
        ("bone indices", m.bone_indices.len()),
        ("eye data", m.eye.len()),
    ];
    for (name, len) in streams {
        if len != 0 && len != n {
            return Err(format!("{name}: {len} entries for {n} positions"));
        }
    }
    if m.tangents.len() != m.bitangents.len() {
        return Err("tangents and bitangents differ in length".into());
    }
    if m.bone_weights.len() != m.bone_indices.len() {
        return Err("bone weights and bone indices differ in length".into());
    }
    if let Some(t) = m
        .triangles
        .iter()
        .find(|t| t.iter().any(|&i| i as usize >= n))
    {
        return Err(format!("triangle {t:?} indexes past {n} vertices"));
    }
    if m.triangles.is_empty() {
        return Err("no triangles".into());
    }
    Ok(())
}

/// [-1, 1] → UNORM8, the inverse of the reader's `b / 255 · 2 − 1`.
fn unorm(v: f32) -> u8 {
    ((v.clamp(-1.0, 1.0) + 1.0) * 127.5).round() as u8
}

/// POSITION0, NORMAL0 and BINORMAL0 of vertex `i`, packed exactly as the SSE `BSVertexData` the engine
/// uploads as its vertex buffer (nif.xml `BSVertexData`; the reader in `nif::geom::vertex_desc`):
///
/// | Input | xyz | w |
/// |---|---|---|
/// | POSITION0 (float4) | Vertex | `Bitangent X` (raw float) |
/// | NORMAL0 (UNORM8×4) | Normal | `Bitangent Y` |
/// | BINORMAL0 (UNORM8×4) | **`Tangent`** | `Bitangent Z` |
///
/// `nif::geom::Mesh::tangents` holds the file's `Tangent` bytes and `bitangents` the three w components, so
/// BINORMAL0.xyz = `tangents` and the w's = `bitangents`. This is the **reverse** of the B7 brief's sentence
/// ("NIF bitangents for BINORMAL0, tangents for the w's"), which 04 §2.1 leaves open (Q13, not answered by
/// the p04 captures, `tools/rdc-out/p04/answers.md`). The file layout wins because the engine binds the
/// BSTriShape vertex data unchanged; Lighting.hlsl then builds the TBN rows as
/// `(Position.w, Normal.w·2−1, Bitangent.w·2−1)`, `Bitangent.xyz·2−1`, `Normal.xyz·2−1` (iLH VS main).
/// Without tangents the w's are 0 and BINORMAL0.xyz is the zero vector.
pub fn pack_tbn(m: &Mesh, i: usize) -> ([f32; 4], [u8; 4], [u8; 4]) {
    let p = m.positions[i];
    let b = m.bitangents.get(i).copied().unwrap_or([0.0; 3]);
    let n = m.normals.get(i).copied().unwrap_or([0.0, 0.0, 1.0]);
    let t = m.tangents.get(i).copied().unwrap_or([0.0; 3]);
    (
        [p[0], p[1], p[2], b[0]],
        [unorm(n[0]), unorm(n[1]), unorm(n[2]), unorm(b[1])],
        [unorm(t[0]), unorm(t[1]), unorm(t[2]), unorm(b[2])],
    )
}

/// One vertex stream: format, stride and bytes for a VS input `semantic`+`index`. Semantics the NIF has no
/// data for get neutral values (white colour, zero UV); unknown semantics get a zero float4 so the input
/// layout always validates.
pub fn stream_for(m: &Mesh, semantic: &str, index: u32) -> (DXGI_FORMAT, u32, Vec<u8>) {
    let n = m.positions.len();
    let mut out = Vec::new();
    let f32s = |out: &mut Vec<u8>, v: &[f32]| v.iter().for_each(|f| out.extend(f.to_le_bytes()));
    match (semantic.to_ascii_uppercase().as_str(), index) {
        ("POSITION", 0) => {
            for i in 0..n {
                f32s(&mut out, &pack_tbn(m, i).0);
            }
            (DXGI_FORMAT_R32G32B32A32_FLOAT, 16, out)
        }
        ("NORMAL", 0) => {
            (0..n).for_each(|i| out.extend(pack_tbn(m, i).1));
            (DXGI_FORMAT_R8G8B8A8_UNORM, 4, out)
        }
        ("BINORMAL", 0) => {
            (0..n).for_each(|i| out.extend(pack_tbn(m, i).2));
            (DXGI_FORMAT_R8G8B8A8_UNORM, 4, out)
        }
        ("TEXCOORD", 0) => {
            for i in 0..n {
                f32s(&mut out, &m.uvs.get(i).copied().unwrap_or([0.0; 2]));
            }
            (DXGI_FORMAT_R32G32_FLOAT, 8, out)
        }
        ("COLOR", 0) => {
            (0..n).for_each(|i| out.extend(m.colors.get(i).copied().unwrap_or([255; 4])));
            (DXGI_FORMAT_R8G8B8A8_UNORM, 4, out)
        }
        ("BLENDWEIGHT", 0) => {
            for i in 0..n {
                let w = m
                    .bone_weights
                    .get(i)
                    .copied()
                    .unwrap_or([1.0, 0.0, 0.0, 0.0]);
                f32s(&mut out, &w);
            }
            (DXGI_FORMAT_R32G32B32A32_FLOAT, 16, out)
        }
        // UNORM, not UINT: the shader computes `int4(765.01 · BoneIndices)` = 3 × index, i.e. it expects the raw
        // index byte normalised to [0, 1] and applies the 3-rows-per-bone stride itself (iLH VS main).
        ("BLENDINDICES", 0) => {
            (0..n).for_each(|i| out.extend(m.bone_indices.get(i).copied().unwrap_or([0; 4])));
            (DXGI_FORMAT_R8G8B8A8_UNORM, 4, out)
        }
        // EYE: `float EyeParameter : TEXCOORD2`.
        ("TEXCOORD", 2) if !m.eye.is_empty() => {
            for i in 0..n {
                f32s(&mut out, &[m.eye[i]]);
            }
            (DXGI_FORMAT_R32_FLOAT, 4, out)
        }
        _ => {
            out.resize(n * 16, 0);
            (DXGI_FORMAT_R32G32B32A32_FLOAT, 16, out)
        }
    }
}

pub struct GpuMesh {
    pub streams: Vec<(ID3D11Buffer, u32)>,
    pub layout: ID3D11InputLayout,
    pub index: ID3D11Buffer,
    pub index_count: u32,
}

fn buffer(gpu: &Gpu, bytes: &[u8], bind: D3D11_BIND_FLAG) -> Result<ID3D11Buffer, String> {
    let d = D3D11_BUFFER_DESC {
        ByteWidth: u32::try_from(bytes.len()).map_err(|_| "buffer over 4 GiB")?,
        Usage: D3D11_USAGE_IMMUTABLE,
        BindFlags: bind.0 as u32,
        CPUAccessFlags: 0,
        MiscFlags: 0,
        StructureByteStride: 0,
    };
    let init = D3D11_SUBRESOURCE_DATA {
        pSysMem: bytes.as_ptr() as _,
        SysMemPitch: 0,
        SysMemSlicePitch: 0,
    };
    let mut b = None;
    // SAFETY: `bytes` outlives the call; descriptors are locals.
    unsafe { gpu.device.CreateBuffer(&d, Some(&init), Some(&mut b)) }
        .map_err(|e| format!("CreateBuffer: {e}"))?;
    b.ok_or_else(|| "CreateBuffer returned no buffer".into())
}

/// Builds one stream per VS input (`inputs` = `Reflection.inputs` of the compiled VS; SV_ system values are
/// skipped) and the input layout against `vs_bytecode`. `m` must pass [`validate`].
pub fn build(
    gpu: &Gpu,
    m: &Mesh,
    inputs: &[(String, u32, u32)],
    vs_bytecode: &[u8],
) -> Result<GpuMesh, String> {
    validate(m)?;
    let inputs: Vec<_> = inputs
        .iter()
        .filter(|i| !i.0.to_ascii_uppercase().starts_with("SV_"))
        .collect();
    let names = inputs
        .iter()
        .map(|i| CString::new(i.0.as_str()).map_err(|_| format!("bad semantic {:?}", i.0)))
        .collect::<Result<Vec<_>, _>>()?;
    let mut streams = Vec::new();
    let mut elems = Vec::new();
    for (slot, (inp, name)) in inputs.iter().zip(&names).enumerate() {
        let (format, stride, bytes) = stream_for(m, &inp.0, inp.1);
        streams.push((buffer(gpu, &bytes, D3D11_BIND_VERTEX_BUFFER)?, stride));
        elems.push(D3D11_INPUT_ELEMENT_DESC {
            SemanticName: PCSTR(name.as_ptr() as _),
            SemanticIndex: inp.1,
            Format: format,
            InputSlot: slot as u32,
            AlignedByteOffset: 0,
            InputSlotClass: D3D11_INPUT_PER_VERTEX_DATA,
            InstanceDataStepRate: 0,
        });
    }
    let mut layout = None;
    // SAFETY: `elems` points into `names`, both alive for the call; the bytecode slice is valid.
    unsafe {
        gpu.device
            .CreateInputLayout(&elems, vs_bytecode, Some(&mut layout))
    }
    .map_err(|e| format!("CreateInputLayout: {e}"))?;
    let idx: Vec<u8> = m
        .triangles
        .iter()
        .flatten()
        .flat_map(|i| i.to_le_bytes())
        .collect();
    Ok(GpuMesh {
        streams,
        layout: layout.ok_or("CreateInputLayout returned no layout")?,
        index: buffer(gpu, &idx, D3D11_BIND_INDEX_BUFFER)?,
        index_count: (m.triangles.len() * 3) as u32,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tri() -> Mesh {
        Mesh {
            positions: vec![[0.0; 3], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]],
            triangles: vec![[0, 1, 2]],
            ..Mesh::default()
        }
    }

    #[test]
    fn validate_catches_broken_invariants() {
        assert!(validate(&tri()).is_ok());
        assert!(validate(&Mesh::default()).is_err());
        let mut m = tri();
        m.uvs = vec![[0.0; 2]];
        assert!(validate(&m).unwrap_err().contains("uvs"));
        let mut m = tri();
        m.triangles = vec![[0, 1, 3]];
        assert!(validate(&m).unwrap_err().contains("past 3"));
        let mut m = tri();
        m.tangents = vec![[0.0; 3]; 3];
        assert!(validate(&m).is_err());
    }

    #[test]
    fn tbn_follows_the_file_layout() {
        let mut m = tri();
        m.normals = vec![[0.0, 0.0, 1.0]; 3];
        m.tangents = vec![[1.0, 0.0, 0.0]; 3];
        m.bitangents = vec![[0.25, -1.0, 1.0]; 3];
        let (p, n, b) = pack_tbn(&m, 1);
        assert_eq!(p, [1.0, 0.0, 0.0, 0.25]); // Bitangent X raw in POSITION.w
        assert_eq!(n, [128, 128, 255, 0]); // Bitangent Y in NORMAL.w
        assert_eq!(b, [255, 128, 128, 255]); // Tangent in BINORMAL.xyz, Bitangent Z in w
    }

    #[test]
    fn blend_indices_are_unorm_bytes() {
        let mut m = tri();
        m.bone_indices = vec![[3, 1, 0, 0]; 3];
        m.bone_weights = vec![[1.0, 0.0, 0.0, 0.0]; 3];
        let (f, stride, bytes) = stream_for(&m, "BLENDINDICES", 0);
        assert_eq!((f, stride), (DXGI_FORMAT_R8G8B8A8_UNORM, 4));
        assert_eq!(&bytes[..4], &[3, 1, 0, 0]);
    }
}
